//! Earning achievements with no connection. `Offline` sits between rcheevos and the internet:
//! what it reads (the account, a game's achievements, what was unlocked before) is kept on the
//! card each time it comes back, and given again when the server cannot be reached. An unlock or
//! leaderboard entry that cannot go out is written to a queue on the card and answered as if the
//! server had taken it, so play goes on; the queue is sent, oldest first, once a request gets
//! through again.
//!
//! On the card, under the folder it is given:
//!   cache/<request>-<key>.json   the last answer to each read
//!   queue/<when>-<n>.txt         an unsent request: the time it was queued, its URL and body
//!   unlocks/<user>.txt           achievements unlocked here, "id when" or "id when h" for hardcore

use std::collections::{BTreeMap, HashMap};
use std::ffi::c_int;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Http, HttpRequest};

/// The reads that are kept, to be given again with no connection.
const CACHED: [&str; 6] = [
    "login2",
    "gameid",
    "patch",
    "achievementsets",
    "startsession",
    "systemgames",
];
/// The writes that are queued when they cannot go out.
const QUEUED: [&str; 2] = ["awardachievement", "submitlbentry"];
/// Parameters that change from one request to the next for the same answer, or are secret.
const UNKEYED: [&str; 5] = ["t", "p", "l", "v", "o"];

#[repr(C)]
struct Md5State {
    count: [u32; 2],
    abcd: [u32; 4],
    buf: [u8; 64],
}

extern "C" {
    fn md5_init(state: *mut Md5State);
    fn md5_append(state: *mut Md5State, data: *const u8, n: c_int);
    fn md5_finish(state: *mut Md5State, digest: *mut u8);
}

/// rcheevos' own MD5, as lower-case hex.
fn md5(parts: &[&str]) -> String {
    let mut state = Md5State {
        count: [0; 2],
        abcd: [0; 4],
        buf: [0; 64],
    };
    let mut digest = [0u8; 16];
    unsafe {
        md5_init(&mut state);
        for p in parts {
            md5_append(&mut state, p.as_ptr(), p.len() as c_int);
        }
        md5_finish(&mut state, digest.as_mut_ptr());
    }
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A form body's parameters, in order, still percent-encoded.
fn params(body: &str) -> Vec<(String, String)> {
    body.split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn param<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn decode(text: &str) -> String {
    let raw = text.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let hex = (raw[i] == b'%' && i + 2 < raw.len())
            .then(|| std::str::from_utf8(&raw[i + 1..i + 3]).ok())
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (hex, raw[i]) {
            (Some(b), _) => {
                out.push(b);
                i += 3;
            }
            (None, b'+') => {
                out.push(b' ');
                i += 1;
            }
            (None, b) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A name safe for a file on FAT: letters, digits, `-` and `_`.
fn file_safe(text: &str) -> String {
    text.chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                true => c,
                false => '_',
            },
        )
        .collect()
}

/// 64-bit FNV-1a, for a cache file's name.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

/// Whether the server answered at all, so the answer is the server's to keep.
fn reached(reply: &Result<(u16, Vec<u8>), String>) -> bool {
    matches!(reply, Ok((code, _)) if (200..500).contains(code) && ![408, 425, 429].contains(code))
}

// rcheevos treats an already-awarded response as acknowledgement after a lost reply.
fn acknowledged(kind: &str, body: &[u8]) -> bool {
    succeeded(body)
        || (kind == "awardachievement"
            && serde_json::from_slice::<serde_json::Value>(body)
                .ok()
                .and_then(|v| {
                    v.get("Error")
                        .and_then(|v| v.as_str())
                        .map(|e| e.starts_with("User already has"))
                })
                .unwrap_or(false))
}

fn succeeded(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("Success").and_then(|v| v.as_bool()))
        == Some(true)
}

#[derive(Default)]
struct Locks {
    files: Mutex<()>,
    syncing: Mutex<()>,
    seq: AtomicU64,
}

// A game and the shelf's background sync must coordinate even with separate HTTP wrappers.
fn locks(dir: &Path) -> Arc<Locks> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Locks>>>> = OnceLock::new();
    let mut all = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let key = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if let Some(locks) = all.get(&key).and_then(Weak::upgrade) {
        return locks;
    }
    let locks = Arc::new(Locks::default());
    all.retain(|_, v| v.strong_count() > 0);
    all.insert(key, Arc::downgrade(&locks));
    locks
}

pub struct Offline {
    dir: PathBuf,
    upstream: Arc<dyn Http>,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
    locks: Arc<Locks>,
}

impl Offline {
    /// Keeps its files in `dir` and reaches the server through `upstream`.
    pub fn new(dir: &Path, upstream: Arc<dyn Http>) -> Offline {
        Offline::with_clock(dir, upstream, Box::new(now_secs))
    }

    /// With a clock of the caller's, in seconds since 1970.
    pub fn with_clock(
        dir: &Path,
        upstream: Arc<dyn Http>,
        clock: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> Offline {
        Offline {
            dir: dir.to_path_buf(),
            upstream,
            clock,
            locks: locks(dir),
        }
    }

    /// How many unlocks and leaderboard entries wait to be sent.
    pub fn pending(dir: &Path) -> usize {
        queued(dir).len()
    }

    /// Sends what is queued, oldest first, until one cannot go out. How many went.
    pub fn sync(&self) -> usize {
        let Ok(_one) = self.locks.syncing.try_lock() else {
            return 0;
        };
        let mut sent = 0;
        for file in queued(&self.dir) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let mut lines = text.lines();
            let (Some(at), Some(url), Some(body)) = (
                lines.next().and_then(|l| l.trim().parse::<u64>().ok()),
                lines.next(),
                lines.next(),
            ) else {
                eprintln!(
                    "slot: cheevos: unreadable queue entry retained: {}",
                    file.display()
                );
                continue;
            };
            let late = (self.clock)().saturating_sub(at);
            let request = HttpRequest {
                url: url.to_string(),
                post: Some(resign(body, late)),
                content_type: Some("application/x-www-form-urlencoded".into()),
            };
            let reply = self.upstream.send(&request);
            // A rate limit, expired token, proxy error, or refusal must never erase an unlock.
            // Keep it until the service explicitly confirms success.
            if !matches!(&reply, Ok((200..=299, got)) if acknowledged(param(&params(body), "r").unwrap_or(""), got))
            {
                break;
            }
            let _held = self.locks.files.lock().unwrap_or_else(|e| e.into_inner());
            if std::fs::remove_file(&file).is_err() {
                break;
            }
            sent += 1;
        }
        if sent > 0 {
            eprintln!("slot: cheevos: sent {sent} queued");
        }
        sent
    }

    // A newly authenticated token must also reach awards saved before signing in again.
    fn refresh_token(&self, user: &str, token: &str) {
        let _held = self.locks.files.lock().unwrap_or_else(|e| e.into_inner());
        let token: String = token
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect();
        for file in queued(&self.dir) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let mut lines = text.lines();
            let (Some(at), Some(url), Some(body)) = (lines.next(), lines.next(), lines.next())
            else {
                continue;
            };
            let mut ps = params(body);
            if !decode(param(&ps, "u").unwrap_or("")).eq_ignore_ascii_case(user) {
                continue;
            }
            ps.retain(|(key, _)| key != "t");
            ps.push(("t".into(), token.clone()));
            let body = ps
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&");
            if let Err(e) = durable_write(&file, format!("{at}\n{url}\n{body}\n").as_bytes()) {
                eprintln!("slot: cheevos: could not refresh queued credentials: {e}");
            }
        }
    }

    fn cache_file(&self, r: &str, params: &[(String, String)]) -> PathBuf {
        let key: BTreeMap<&str, &str> = params
            .iter()
            .filter(|(k, _)| !UNKEYED.contains(&k.as_str()))
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let key = format!("{key:?}");
        self.dir
            .join("cache")
            .join(format!("{}-{:016x}.json", file_safe(r), fnv(&key)))
    }

    fn unlocks_file(&self, user: &str) -> PathBuf {
        self.dir
            .join("unlocks")
            .join(format!("{}.txt", file_safe(user)))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let _held = self.locks.files.lock().unwrap_or_else(|e| e.into_inner());
        durable_write(path, bytes).map_err(|e| format!("offline cache: {e}"))
    }

    /// Notes an unlock only after it was accepted or safely queued.
    fn remember_unlock(&self, user: &str, id: &str, hardcore: bool) -> Result<(), String> {
        let _held = self.locks.files.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.unlocks_file(user);
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        if text.lines().any(|l| {
            let mut fields = l.split(' ');
            fields.next() == Some(id) && (!hardcore || fields.nth(1) == Some("h"))
        }) {
            return Ok(());
        }
        let mode = if hardcore { " h" } else { "" };
        text.push_str(&format!("{id} {}{mode}\n", (self.clock)()));
        durable_write(&path, text.as_bytes()).map_err(|e| format!("offline unlock: {e}"))
    }

    fn queue(&self, request: &HttpRequest, body: &str) -> Result<(), String> {
        let _held = self.locks.files.lock().unwrap_or_else(|e| e.into_inner());
        let at = (self.clock)();
        loop {
            let n = self.locks.seq.fetch_add(1, Ordering::Relaxed);
            let path = self.dir.join("queue").join(format!(
                "{at:012}-{:06}-{n:04}.txt",
                std::process::id() % 1_000_000
            ));
            if path.exists() {
                continue;
            }
            return durable_write(&path, format!("{at}\n{}\n{body}\n", request.url).as_bytes())
                .map_err(|e| format!("Could not save offline achievement: {e}"));
        }
    }

    /// A cached session's answer, with what was unlocked here since and the time now.
    fn session(&self, cached: &str, user: &str) -> String {
        let Ok(mut data) = serde_json::from_str::<serde_json::Value>(cached) else {
            return cached.to_string();
        };
        if !data.is_object() {
            return cached.to_string();
        }
        data["ServerNow"] = (self.clock)().into();
        if !data.get("Unlocks").is_some_and(|v| v.is_array()) {
            data["Unlocks"] = serde_json::json!([]);
        }
        let mine = std::fs::read_to_string(self.unlocks_file(user)).unwrap_or_default();
        for line in mine.lines() {
            let mut fields = line.split(' ');
            let (Some(id), Some(when)) = (fields.next(), fields.next()) else {
                continue;
            };
            if let (Ok(id), Ok(when)) = (id.parse::<u64>(), when.parse::<u64>()) {
                let key = if fields.next() == Some("h") {
                    "HardcoreUnlocks"
                } else {
                    "Unlocks"
                };
                if !data.get(key).is_some_and(|v| v.is_array()) {
                    data[key] = serde_json::json!([]);
                }
                let unlocks = data[key].as_array_mut().unwrap();
                if !unlocks.iter().any(|v| v["ID"].as_u64() == Some(id)) {
                    unlocks.push(serde_json::json!({"ID": id, "When": when}));
                }
            }
        }
        data.to_string()
    }
}

impl Http for Offline {
    fn send(&self, request: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
        let body = request.post.clone().unwrap_or_default();
        let ps = params(&body);
        let r = param(&ps, "r").unwrap_or("").to_string();
        let user = decode(param(&ps, "u").unwrap_or(""));

        if QUEUED.contains(&r.as_str()) {
            // Earlier unlocks go first, so the server sees them in order.
            self.sync();
            let reply = self.upstream.send(request);
            if reached(&reply) {
                if matches!(&reply, Ok((200..=299, got)) if acknowledged(&r, got))
                    && r == "awardachievement"
                {
                    if let Some(id) = param(&ps, "a") {
                        if let Err(e) =
                            self.remember_unlock(&user, id, param(&ps, "h") == Some("1"))
                        {
                            eprintln!("slot: cheevos: {e}");
                        }
                    }
                }
                return reply;
            }
            self.queue(request, &body)?;
            if r == "awardachievement" {
                if let Some(id) = param(&ps, "a") {
                    self.remember_unlock(&user, id, param(&ps, "h") == Some("1"))?;
                }
            }
            eprintln!("slot: cheevos: offline, queued {r}");
            return Ok((200, stand_in(&r, &ps).into_bytes()));
        }

        // A sign-in with a password is never answered from the card.
        let cacheable = CACHED.contains(&r.as_str()) && param(&ps, "p").is_none();
        let reply = self.upstream.send(request);
        if reached(&reply) {
            if let Ok((200, got)) = &reply {
                if r == "login2" && succeeded(got) {
                    if let Ok(data) = serde_json::from_slice::<serde_json::Value>(got) {
                        if let (Some(user), Some(token)) =
                            (data["User"].as_str(), data["Token"].as_str())
                        {
                            self.refresh_token(user, token);
                        }
                    }
                }
                if cacheable && succeeded(got) {
                    if let Err(e) = self.write(&self.cache_file(&r, &ps), got) {
                        eprintln!("slot: cheevos: {e}");
                    }
                }
            }
            if !queued(&self.dir).is_empty() {
                self.sync();
            }
            return reply;
        }
        if cacheable {
            let cached = std::fs::read_to_string(self.cache_file(&r, &ps)).or_else(|e| {
                if r != "startsession" {
                    return Err(e);
                }
                let mut other = ps.clone();
                let Some((_, mode)) = other.iter_mut().find(|(key, _)| key == "h") else {
                    return Err(e);
                };
                *mode = match mode.as_str() {
                    "0" => "1".into(),
                    "1" => "0".into(),
                    _ => return Err(e),
                };
                std::fs::read_to_string(self.cache_file(&r, &other))
            });
            if let Ok(cached) = cached {
                eprintln!("slot: cheevos: offline, {r} from the card");
                let text = match r.as_str() {
                    "startsession" => self.session(&cached, &user),
                    _ => cached,
                };
                return Ok((200, text.into_bytes()));
            }
        }
        if r == "ping" {
            return Ok((200, br#"{"Success":true}"#.to_vec()));
        }
        reply
    }
}

/// What the server would have said to a queued request.
fn stand_in(r: &str, ps: &[(String, String)]) -> String {
    match r {
        "awardachievement" => format!(
            r#"{{"Success":true,"AchievementID":{}}}"#,
            param(ps, "a").unwrap_or("0")
        ),
        _ => {
            let score = param(ps, "s").unwrap_or("0");
            format!(
                r#"{{"Success":true,"Response":{{"Score":{score},"BestScore":{score},"RankInfo":{{"Rank":0,"NumEntries":"0"}},"TopEntries":[]}}}}"#
            )
        }
    }
}

// Rename only after the data is flushed; a successful enqueue must survive a power loss.
fn durable_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let mut file = std::fs::File::create(&part)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&part, path)?;
    if let Ok(dir) = std::fs::File::open(parent) {
        // FAT does not support directory fsync on every target.
        let _ = dir.sync_all();
    }
    Ok(())
}

/// A queued body, sent `late` seconds after it was queued: its `o`, the seconds since the
/// unlock, moved on by that and its signature `v` worked out again to match.
fn resign(body: &str, late: u64) -> String {
    let mut ps = params(body);
    let r = param(&ps, "r").unwrap_or("").to_string();
    let o = param(&ps, "o")
        .and_then(|o| o.parse::<u64>().ok())
        .unwrap_or(0)
        .saturating_add(late);
    if o == 0 || !QUEUED.contains(&r.as_str()) {
        return body.to_string();
    }
    let user = decode(param(&ps, "u").unwrap_or(""));
    let o_text = o.to_string();
    let v = match r.as_str() {
        "awardachievement" => {
            let a = param(&ps, "a").unwrap_or("0").to_string();
            let h = param(&ps, "h").unwrap_or("0").to_string();
            md5(&[&a, &user, &h, &a, &o_text])
        }
        _ => {
            let i = param(&ps, "i").unwrap_or("0").to_string();
            let s = decode(param(&ps, "s").unwrap_or("0"));
            md5(&[&i, &user, &s, &o_text])
        }
    };
    ps.retain(|(k, _)| k != "o" && k != "v");
    ps.push(("o".into(), o_text));
    ps.push(("v".into(), v));
    ps.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn queued(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("queue"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_is_rcheevos_own() {
        assert_eq!(md5(&[""]), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5(&["ab", "c"]), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn a_late_unlock_is_signed_for_its_true_age() {
        let body = "r=awardachievement&u=Benni&t=tok&a=42&h=0&m=abc&v=old";
        let sent = resign(body, 90);
        let ps = params(&sent);
        assert_eq!(param(&ps, "o"), Some("90"));
        assert_eq!(
            param(&ps, "v").unwrap(),
            md5(&["42", "Benni", "0", "42", "90"])
        );
        assert_eq!(param(&ps, "t"), Some("tok"));
        // One that already carried an age keeps counting from it.
        let ps = params(&resign("r=awardachievement&u=B&a=1&h=0&o=5&v=x", 10));
        assert_eq!(param(&ps, "o"), Some("15"));
        let ps = params(&resign("r=submitlbentry&u=B&i=7&s=-3&v=x", 4));
        assert_eq!(param(&ps, "v").unwrap(), md5(&["7", "B", "-3", "4"]));
    }

    #[test]
    fn a_percent_escape_at_the_end_is_kept() {
        assert_eq!(decode("a%20b%2"), "a b%2");
        assert_eq!(decode("x+y"), "x y");
    }
}
