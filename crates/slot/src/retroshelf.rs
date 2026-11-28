//! RetroShelf: play-session syncing to `https://www.retroshelf.org`.
//!
//! Uses RetroShelf's OAuth 2.0 device authorization flow (`POST /api/auth/device/code` and
//! `/api/auth/device/token`), registers the handheld via `POST /api/syncClient/completeLink`, and
//! pushes completed sessions from [`crate::playtime::PlaytimeStore`] via
//! `POST /api/syncClient/syncSessions`.
//!
//! Credentials live in `Config/retroshelf.conf`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use serde_json::{json, Value};
use slot_store::Platform;

use crate::playtime::{now_unix, platform_from_dir, PlaytimeStore, SessionRecord};
use crate::romm::{Http, Req};

pub const CONF: &str = "Config/retroshelf.conf";
pub const BASE_URL: &str = "https://www.retroshelf.org";
const CLIENT_ID: &str = "retroshelf";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Maximum sessions sent in one `syncSessions` request.
const BATCH: usize = 50;
/// ROMs up to this size are MD5-hashed for exact RetroShelf game matching.
const MAX_HASH_BYTES: u64 = 64 * 1024 * 1024;
/// How often the worker checks for pending unsynced sessions while awake and online.
const POLL: Duration = Duration::from_secs(60);

/// Saved RetroShelf link state on the card.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Conf {
    pub token: String,
    pub client_id: String,
    pub fingerprint: String,
    pub last_sync: i64,
    pub last_note: String,
}

impl Conf {
    pub fn linked(&self) -> bool {
        !self.token.is_empty() && !self.client_id.is_empty()
    }
}

pub fn read(root: &Path) -> Conf {
    let text = std::fs::read_to_string(root.join(CONF)).unwrap_or_default();
    let get = |key: &str| {
        text.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == key))
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_default()
    };
    Conf {
        token: get("token"),
        client_id: get("client_id"),
        fingerprint: get("fingerprint"),
        last_sync: get("last_sync").parse().unwrap_or(0),
        last_note: get("last_note"),
    }
}

pub fn write(root: &Path, conf: &Conf) -> std::io::Result<()> {
    let text = format!(
        "token={}\nclient_id={}\nfingerprint={}\nlast_sync={}\nlast_note={}\n",
        conf.token,
        conf.client_id,
        conf.fingerprint,
        conf.last_sync,
        conf.last_note.replace(['\r', '\n'], " "),
    );
    std::fs::create_dir_all(root.join("Config"))?;
    slot_store::atomic_write(&root.join(CONF), text.as_bytes())
}

/// Unlinks the device, keeping its stable fingerprint so re-linking reuses the same identity.
pub fn sign_out(root: &Path) -> std::io::Result<()> {
    let conf = read(root);
    write(
        root,
        &Conf {
            fingerprint: conf.fingerprint,
            ..Conf::default()
        },
    )
}

/// Returns the saved fingerprint or generates a fresh stable 32-hex-char one.
fn ensure_fingerprint(root: &Path, conf: &mut Conf) -> String {
    if !conf.fingerprint.is_empty() {
        return conf.fingerprint.clone();
    }
    let mut h = Md5::new();
    h.update(root.to_string_lossy().as_bytes());
    h.update(now_unix().to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    if let Ok(mac) = std::fs::read("/sys/class/net/wlan0/address") {
        h.update(&mac);
    }
    if let Ok(mid) = std::fs::read("/etc/machine-id") {
        h.update(&mid);
    }
    let fp: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    conf.fingerprint = fp.clone();
    let _ = write(root, conf);
    fp
}

/// Maps BaseOS/TrimUI device target names to RetroShelf's device slugs.
pub fn device_slug() -> Option<&'static str> {
    let target = crate::baseos::var("BASEOS_TARGET")
        .or_else(|| crate::baseos::var("BASEOS_MODEL"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let slug = match target.as_str() {
        "rg28xx" => "anbernic-rg-28xx",
        "rg34xx" => "anbernic-rg-34xx",
        "rg34xxsp" => "anbernic-rg-34xx-sp",
        "rg35xx2024" => "anbernic-rg-35xx-2024",
        "rg35xxh" => "anbernic-rg-35xx-h",
        "rg35xxplus" => "anbernic-rg-35xx-plus",
        "rg35xxpro" => "anbernic-rg-35xx-pro",
        "rg35xxsp" => "anbernic-rg-35xx-sp",
        "rg40xxh" => "anbernic-rg-40xx-h",
        "rg40xxv" => "anbernic-rg-40xx-v",
        "rgcubexx" => "anbernic-rg-cube-xx",
        "brick" | "trimui-brick" | "tg3040" => "trimui-brick",
        "tsp" | "trimui-smart-pro" | "tg5040" => "trimui-smart-pro",
        _ if crate::pak::CHIP == "a133p" => "trimui-brick",
        _ => return None,
    };
    Some(slug)
}

/// Maps an unslot `Platform` to a RetroShelf `platformHint` string.
pub fn platform_hint(platform: Platform) -> &'static str {
    match platform {
        Platform::Gba => "GBA",
        Platform::Gb => "GB",
        Platform::Gbc => "GBC",
        Platform::Nes => "NES",
        Platform::Snes => "SNES",
        Platform::Genesis => "MD",
        Platform::MasterSystem => "SMS",
        Platform::GameGear => "GG",
        Platform::PcEngine => "PCE",
        Platform::PcEngineCd => "PCECD",
        Platform::SuperGrafx => "SGFX",
        Platform::PcFx => "PCFX",
        Platform::NeoGeoPocket => "NGP",
        Platform::WonderSwan => "WS",
        Platform::Lynx => "LYNX",
        Platform::PlayStation => "PS",
        Platform::Nintendo64 => "N64",
        Platform::Nds => "NDS",
        Platform::Psp => "PSP",
        Platform::Dreamcast => "DC",
        Platform::Sega32x => "32X",
        Platform::SegaCd => "SEGACD",
        Platform::Sg1000 => "SG1000",
        Platform::Fds => "FDS",
        Platform::VirtualBoy => "VB",
        Platform::PokemonMini => "POKEMINI",
        Platform::GameAndWatch => "GW",
        Platform::Arcade => "ARCADE",
        Platform::NeoGeo => "NEOGEO",
        Platform::NeoGeoCd => "NEOCD",
        Platform::Pico8 => "PICO8",
        Platform::Tic80 => "TIC80",
        Platform::Dos => "DOS",
        Platform::ScummVm => "SCUMMVM",
        other => other.dir_name(),
    }
}

/// An in-progress device code sign-in waiting for approval on `retroshelf.org/link`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pairing {
    pub device_code: String,
    pub user_code: String,
    pub url: String,
    pub interval: Duration,
    pub expires: Duration,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PollOutcome {
    Pending,
    SlowDown,
    Token(String),
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SyncOutcome {
    pub submitted: usize,
    pub created: usize,
    pub matched: usize,
}

/// The network for this build: real curl on device, simulated RetroShelf on host/tests.
pub fn for_build() -> Box<dyn Http> {
    #[cfg(all(feature = "device", unix))]
    return Box::new(crate::romm::Curl);
    #[cfg(not(all(feature = "device", unix)))]
    Box::new(SimRetroshelf)
}

/// A pretend RetroShelf server for desktop runs and tests.
pub struct SimRetroshelf;

impl SimRetroshelf {
    pub const CODE: &'static str = "SIMRS1";
    pub const CLIENT_ID: &'static str = "sim-retroshelf-client";
}

impl Http for SimRetroshelf {
    fn send(&mut self, req: &Req) -> Result<(u16, Vec<u8>), String> {
        let path = req.url.split_once("://").map_or("", |(_, r)| r);
        let path = &path[path.find('/').unwrap_or(path.len())..];
        let body = match path.split('?').next().unwrap_or("") {
            "/api/auth/device/code" => json!({
                "device_code": "sim-device-code",
                "user_code": SimRetroshelf::CODE,
                "verification_uri": format!("{BASE_URL}/link"),
                "verification_uri_complete": format!("{BASE_URL}/link?user_code={}", SimRetroshelf::CODE),
                "expires_in": 600,
                "interval": 1,
            }),
            "/api/auth/device/token" => json!({
                "access_token": "rs_sim_token",
            }),
            "/api/syncClient/completeLink" => json!({
                "syncClientId": SimRetroshelf::CLIENT_ID,
                "lastSyncCursor": 0,
            }),
            "/api/syncClient/syncSessions" => {
                let count = req
                    .json
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    .and_then(|v| v["sessions"].as_array().map(|a| a.len()))
                    .unwrap_or(0);
                json!({
                    "submitted": count,
                    "created": count,
                    "synced": count,
                    "matched": count,
                    "unmatched": 0,
                    "results": [],
                })
            }
            _ => return Ok((404, Vec::new())),
        };
        Ok((200, body.to_string().into_bytes()))
    }
}

fn post(url: String, token: Option<&str>, body: Value) -> Req<'_> {
    Req {
        method: "POST",
        url,
        token,
        json: Some(body.to_string()),
        to: None,
        file: None,
    }
}

fn api_error(code: u16, body: &[u8]) -> String {
    if let Ok(v) = serde_json::from_slice::<Value>(body) {
        if let Some(msg) = v["message"]
            .as_str()
            .or_else(|| v["error_description"].as_str())
            .or_else(|| v["error"].as_str())
            .filter(|s| !s.is_empty())
        {
            return format!("RetroShelf: {msg}");
        }
    }
    match code {
        401 | 403 => "RetroShelf turned this device away: link again".into(),
        code => format!("RetroShelf answered {code}"),
    }
}

fn post_json(http: &mut dyn Http, req: &Req) -> Result<Value, String> {
    let (code, body) = http
        .send(req)
        .map_err(|_| "RetroShelf could not be reached".to_string())?;
    match code {
        200..=299 => {
            serde_json::from_slice(&body).map_err(|e| format!("RetroShelf answered oddly: {e}"))
        }
        _ => Err(api_error(code, &body)),
    }
}

/// Starts a RetroShelf device-code link flow.
pub fn pair(http: &mut dyn Http, base: &str) -> Result<Pairing, String> {
    let req = post(
        format!("{base}/api/auth/device/code"),
        None,
        json!({ "client_id": CLIENT_ID }),
    );
    let v = post_json(http, &req)?;
    let device_code = v["device_code"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("RetroShelf gave no device code")?
        .to_string();
    let user_code = v["user_code"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("RetroShelf gave no link code")?
        .to_string();
    let url = v["verification_uri_complete"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            let uri = v["verification_uri"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("https://www.retroshelf.org/link");
            format!("{uri}?user_code={user_code}")
        });
    let interval = Duration::from_secs(v["interval"].as_u64().unwrap_or(5).clamp(1, 30));
    let expires = Duration::from_secs(v["expires_in"].as_u64().unwrap_or(600).max(60));
    Ok(Pairing {
        device_code,
        user_code,
        url,
        interval,
        expires,
    })
}

/// Polls `/api/auth/device/token` once for approval.
pub fn poll_token(
    http: &mut dyn Http,
    base: &str,
    device_code: &str,
) -> Result<PollOutcome, String> {
    let req = post(
        format!("{base}/api/auth/device/token"),
        None,
        json!({
            "grant_type": GRANT_TYPE,
            "client_id": CLIENT_ID,
            "device_code": device_code,
        }),
    );
    let (code, body) = http
        .send(&req)
        .map_err(|_| "RetroShelf could not be reached".to_string())?;
    let v = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    if (200..=299).contains(&code) {
        if let Some(tok) = v["access_token"].as_str().filter(|s| !s.is_empty()) {
            return Ok(PollOutcome::Token(tok.to_string()));
        }
    }
    match v["error"].as_str() {
        Some("authorization_pending") => Ok(PollOutcome::Pending),
        Some("slow_down") => Ok(PollOutcome::SlowDown),
        Some("expired_token") => Err("The link code ran out: try again".into()),
        Some("access_denied") => Err("RetroShelf link was declined".into()),
        _ => Err(api_error(code, &body)),
    }
}

/// Completes device linking and returns the assigned `syncClientId`.
pub fn complete_link(
    http: &mut dyn Http,
    base: &str,
    token: &str,
    name: &str,
    fingerprint: &str,
    slug: Option<&str>,
) -> Result<String, String> {
    let mut body = json!({
        "name": name,
        "platform": "linux",
        "fingerprint": fingerprint,
        "metadata": {
            "cfw": "unslot",
            "osVersion": crate::build_info::Build::current().version,
        },
    });
    if let Some(slug) = slug {
        body["deviceSlug"] = json!(slug);
    }
    let req = post(
        format!("{base}/api/syncClient/completeLink"),
        Some(token),
        body,
    );
    let v = post_json(http, &req)?;
    v["syncClientId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "RetroShelf gave no sync client ID".into())
}

fn rom_md5(root: &Path, rom_path: &str) -> Option<String> {
    use std::io::Read;
    let rel = rom_path.trim_start_matches('/');
    let full = root.join(rel);
    let meta = std::fs::metadata(&full).ok()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_HASH_BYTES {
        return None;
    }
    let mut file = std::fs::File::open(&full).ok()?;
    let mut hasher = Md5::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

fn session_json(root: &Path, rec: &SessionRecord, now: i64) -> Value {
    let rom_path = if rec.rom_path.starts_with('/') {
        rec.rom_path.clone()
    } else {
        format!("/{}", rec.rom_path)
    };
    let hint = platform_from_dir(&rec.platform)
        .map(platform_hint)
        .unwrap_or(rec.platform.as_str());
    let age = now
        .saturating_sub(rec.played_at + rec.duration as i64)
        .max(0);
    let mut obj = json!({
        "romPath": rom_path,
        "romName": rec.rom_name,
        "duration": rec.duration,
        "devicePlayedAt": rec.played_at,
        "platformHint": hint,
        "ageSeconds": age,
    });
    if let Some(hash) = rom_md5(root, &rec.rom_path) {
        obj["romHash"] = json!(hash);
    }
    obj
}

/// Pushes all unsynced sessions in `Config/playtime.json` to RetroShelf in batches of [`BATCH`].
pub fn sync_pending(
    http: &mut dyn Http,
    base: &str,
    conf: &Conf,
    root: &Path,
) -> Result<SyncOutcome, String> {
    if !conf.linked() {
        return Err("Link your RetroShelf account first".into());
    }
    let mut store = PlaytimeStore::read(root);
    let pending: Vec<SessionRecord> = store
        .sessions
        .iter()
        .filter(|s| !s.synced)
        .cloned()
        .collect();
    if pending.is_empty() {
        return Ok(SyncOutcome::default());
    }
    let mut total = SyncOutcome::default();
    let chunks: Vec<&[SessionRecord]> = pending.chunks(BATCH).collect();
    for (idx, chunk) in chunks.iter().enumerate() {
        let now = now_unix();
        let sessions: Vec<Value> = chunk.iter().map(|r| session_json(root, r, now)).collect();
        let has_more = idx + 1 < chunks.len();
        let req = post(
            format!("{base}/api/syncClient/syncSessions"),
            Some(&conf.token),
            json!({
                "syncClientId": conf.client_id,
                "sessions": sessions,
                "hasMore": has_more,
                "deviceNow": now,
            }),
        );
        let v = post_json(http, &req)?;
        total.submitted += v["submitted"].as_u64().unwrap_or(chunk.len() as u64) as usize;
        total.created += v["created"]
            .as_u64()
            .or_else(|| v["synced"].as_u64())
            .unwrap_or(chunk.len() as u64) as usize;
        total.matched += v["matched"].as_u64().unwrap_or(0) as usize;

        let keys: Vec<(i64, String, String, u64)> = chunk
            .iter()
            .map(|r| (r.played_at, r.platform.clone(), r.stem.clone(), r.duration))
            .collect();
        // Re-read in case the main thread appended a new session while HTTP was in flight.
        store = PlaytimeStore::read(root);
        store.mark_synced(&keys);
        store
            .write(root)
            .map_err(|e| format!("Could not save sync state: {e}"))?;
    }
    let mut updated = read(root);
    updated.last_sync = now_unix();
    updated.last_note = outcome_text(&total);
    let _ = write(root, &updated);
    Ok(total)
}

pub fn outcome_text(o: &SyncOutcome) -> String {
    match (o.submitted, o.created) {
        (0, _) => "Up to date with RetroShelf".into(),
        (_, 1) => "Synced 1 session to RetroShelf".into(),
        (_, n) if n > 0 => format!("Synced {n} sessions to RetroShelf"),
        (s, 0) => format!("Checked {s} sessions: already synced"),
        _ => "Up to date with RetroShelf".into(),
    }
}

enum Job {
    Pair { name: String },
    Sync,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RetroshelfView {
    pub busy: Option<String>,
    pub pairing: Option<Pairing>,
    /// Bumped each time linking completes, so App re-reads `Config/retroshelf.conf`.
    pub linked: usize,
    /// Bumped each time a sync finishes, so App re-reads `Config/playtime.json` and conf.
    pub synced: usize,
    pub note: Option<String>,
    pub error: Option<String>,
}

pub struct Retroshelf {
    jobs: Sender<Job>,
    view: Arc<Mutex<RetroshelfView>>,
    stop: Arc<AtomicBool>,
    awake: Arc<AtomicBool>,
}

impl Retroshelf {
    pub fn spawn(http: Box<dyn Http>, root: &Path) -> Retroshelf {
        Self::spawn_at(http, BASE_URL.into(), root)
    }

    pub fn spawn_at(http: Box<dyn Http>, base: String, root: &Path) -> Retroshelf {
        let (jobs, rx) = channel();
        let view = Arc::new(Mutex::new(RetroshelfView::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let awake = Arc::new(AtomicBool::new(false));
        let (shared, stopped, woke, root) = (
            view.clone(),
            stop.clone(),
            awake.clone(),
            root.to_path_buf(),
        );
        let spawned = std::thread::Builder::new()
            .name("slot-retroshelf".into())
            .spawn(move || work(http, base, root, rx, shared, stopped, woke));
        if let Err(e) = spawned {
            eprintln!("slot: retroshelf: {e}");
        }
        Retroshelf {
            jobs,
            view,
            stop,
            awake,
        }
    }

    pub fn for_card(root: &Path) -> Retroshelf {
        Retroshelf::spawn(for_build(), root)
    }

    pub fn view(&self) -> RetroshelfView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn ask(&self, job: Job) {
        let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
        view.error = None;
        view.note = None;
        let _ = self.jobs.send(job);
    }

    pub fn pair(&self, name: &str) {
        self.stop.store(false, Ordering::Relaxed);
        self.ask(Job::Pair {
            name: name.to_string(),
        });
    }

    pub fn stop_pairing(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn sync(&self) {
        self.ask(Job::Sync);
    }

    pub fn set_awake(&self, awake: bool) {
        self.awake.store(awake, Ordering::Relaxed);
    }

    pub fn forget(&self) {
        let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
        view.note = None;
        view.error = None;
        view.pairing = None;
    }
}

type Set<'a> = dyn Fn(&mut dyn FnMut(&mut RetroshelfView)) + 'a;

fn work(
    mut http: Box<dyn Http>,
    base: String,
    root: PathBuf,
    jobs: Receiver<Job>,
    view: Arc<Mutex<RetroshelfView>>,
    stop: Arc<AtomicBool>,
    awake: Arc<AtomicBool>,
) {
    let set = |f: &mut dyn FnMut(&mut RetroshelfView)| {
        f(&mut view.lock().unwrap_or_else(|e| e.into_inner()))
    };
    let mut poll_at = Instant::now() + Duration::from_secs(10);
    loop {
        let job = match jobs.recv_timeout(poll_at.saturating_duration_since(Instant::now())) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => {
                poll_at = Instant::now() + POLL;
                if awake.load(Ordering::Relaxed) {
                    let conf = read(&root);
                    if conf.linked() && PlaytimeStore::read(&root).pending_count() > 0 {
                        if let Ok(outcome) = sync_pending(&mut *http, &base, &conf, &root) {
                            set(&mut |v| {
                                v.synced += 1;
                                if outcome.submitted > 0 {
                                    v.note = Some(outcome_text(&outcome));
                                }
                            });
                        }
                    }
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let result = match job {
            Job::Pair { name } => {
                set(&mut |v| v.busy = Some("Asking RetroShelf for a code".into()));
                link(&mut *http, &base, &root, &name, &stop, &set)
            }
            Job::Sync => {
                set(&mut |v| v.busy = Some("Syncing playtime with RetroShelf".into()));
                let conf = read(&root);
                sync_pending(&mut *http, &base, &conf, &root).map(|outcome| {
                    set(&mut |v| {
                        v.synced += 1;
                        v.note = Some(outcome_text(&outcome));
                    })
                })
            }
        };
        set(&mut |v| {
            v.busy = None;
            v.pairing = None;
            if let Err(e) = &result {
                eprintln!("slot: retroshelf: {e}");
                v.error = Some(e.clone());
            }
        });
    }
}

fn link(
    http: &mut dyn Http,
    base: &str,
    root: &Path,
    name: &str,
    stop: &AtomicBool,
    set: &Set<'_>,
) -> Result<(), String> {
    let pairing = pair(http, base)?;
    let (mut interval, until) = (pairing.interval, Instant::now() + pairing.expires);
    set(&mut |v| {
        v.busy = None;
        v.pairing = Some(pairing.clone());
    });
    let token = loop {
        let next = Instant::now() + interval;
        while Instant::now() < next {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if Instant::now() > until {
            return Err("The link code ran out: try again".into());
        }
        match poll_token(http, base, &pairing.device_code)? {
            PollOutcome::Pending => {}
            PollOutcome::SlowDown => interval += Duration::from_secs(5),
            PollOutcome::Token(tok) => break tok,
        }
    };
    set(&mut |v| {
        v.pairing = None;
        v.busy = Some("Finishing RetroShelf link".into());
    });
    let mut conf = read(root);
    let fp = ensure_fingerprint(root, &mut conf);
    let client_id = complete_link(http, base, &token, name, &fp, device_slug())?;
    conf.token = token;
    conf.client_id = client_id;
    write(root, &conf).map_err(|e| format!("Could not save RetroShelf link: {e}"))?;
    // Push any sessions already recorded on the card right away.
    let note = match sync_pending(http, base, &conf, root) {
        Ok(o) if o.submitted > 0 => format!("Linked to RetroShelf · {}", outcome_text(&o)),
        _ => "Linked to RetroShelf".into(),
    };
    set(&mut |v| {
        v.linked += 1;
        v.synced += 1;
        v.note = Some(note.clone());
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHttp {
        polls: usize,
        requests: Vec<(String, Value)>,
    }

    impl Http for FakeHttp {
        fn send(&mut self, req: &Req) -> Result<(u16, Vec<u8>), String> {
            let body = req
                .json
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or(Value::Null);
            self.requests.push((req.url.clone(), body.clone()));
            if req.url.ends_with("/api/auth/device/code") {
                return Ok((
                    200,
                    json!({
                        "device_code": "dc-1",
                        "user_code": "ABC123",
                        "verification_uri": "https://www.retroshelf.org/link",
                        "verification_uri_complete": "https://www.retroshelf.org/link?user_code=ABC123",
                        "expires_in": 300,
                        "interval": 1,
                    })
                    .to_string()
                    .into_bytes(),
                ));
            }
            if req.url.ends_with("/api/auth/device/token") {
                self.polls += 1;
                if self.polls == 1 {
                    return Ok((
                        400,
                        json!({ "error": "authorization_pending" })
                            .to_string()
                            .into_bytes(),
                    ));
                }
                return Ok((
                    200,
                    json!({ "access_token": "tok-xyz" })
                        .to_string()
                        .into_bytes(),
                ));
            }
            if req.url.ends_with("/api/syncClient/completeLink") {
                assert_eq!(req.token, Some("tok-xyz"));
                return Ok((
                    200,
                    json!({ "syncClientId": "cid-42", "lastSyncCursor": 0 })
                        .to_string()
                        .into_bytes(),
                ));
            }
            if req.url.ends_with("/api/syncClient/syncSessions") {
                assert_eq!(req.token, Some("tok-xyz"));
                let n = body["sessions"].as_array().map_or(0, Vec::len);
                return Ok((
                    200,
                    json!({ "submitted": n, "created": n, "synced": n, "matched": n, "unmatched": 0 })
                        .to_string()
                        .into_bytes(),
                ));
            }
            Ok((404, Vec::new()))
        }
    }

    #[test]
    fn device_flow_links_and_syncs_pending_sessions_with_md5() {
        let d = tempfile::tempdir().unwrap();
        let rom = d.path().join("Games/GBA/Emerald.gba");
        std::fs::create_dir_all(rom.parent().unwrap()).unwrap();
        std::fs::write(&rom, b"pokemon-emerald-rom").unwrap();

        let mut store = PlaytimeStore::default();
        store.finish_session(
            Platform::Gba,
            "Emerald",
            &rom,
            d.path(),
            600,
            600,
            1700000000,
            1700000600,
        );
        store.write(d.path()).unwrap();

        let mut http = FakeHttp {
            polls: 0,
            requests: Vec::new(),
        };
        let p = pair(&mut http, BASE_URL).unwrap();
        assert_eq!(p.user_code, "ABC123");
        assert_eq!(p.url, "https://www.retroshelf.org/link?user_code=ABC123");

        assert_eq!(
            poll_token(&mut http, BASE_URL, &p.device_code).unwrap(),
            PollOutcome::Pending
        );
        assert_eq!(
            poll_token(&mut http, BASE_URL, &p.device_code).unwrap(),
            PollOutcome::Token("tok-xyz".into())
        );

        let cid = complete_link(
            &mut http,
            BASE_URL,
            "tok-xyz",
            "my-sp",
            "fp123",
            Some("anbernic-rg-35xx-sp"),
        )
        .unwrap();
        assert_eq!(cid, "cid-42");

        let conf = Conf {
            token: "tok-xyz".into(),
            client_id: cid,
            fingerprint: "fp123".into(),
            ..Conf::default()
        };
        write(d.path(), &conf).unwrap();

        let out = sync_pending(&mut http, BASE_URL, &conf, d.path()).unwrap();
        assert_eq!(out.submitted, 1);
        assert_eq!(out.created, 1);

        // Verify payload fields sent to RetroShelf.
        let (_, sent) = http.requests.last().unwrap();
        assert_eq!(sent["syncClientId"], "cid-42");
        let s0 = &sent["sessions"][0];
        assert_eq!(s0["romPath"], "/Games/GBA/Emerald.gba");
        assert_eq!(s0["romName"], "Emerald.gba");
        assert_eq!(s0["platformHint"], "GBA");
        assert_eq!(s0["duration"], 600);
        assert_eq!(s0["devicePlayedAt"], 1700000000);
        assert_eq!(s0["romHash"].as_str().unwrap().len(), 32);

        // Session is now marked synced; second sync is a no-op.
        assert_eq!(PlaytimeStore::read(d.path()).pending_count(), 0);
        let out2 = sync_pending(&mut http, BASE_URL, &conf, d.path()).unwrap();
        assert_eq!(out2.submitted, 0);
    }
}
