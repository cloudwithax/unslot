//! RetroAchievements: the account the card signs in with, the curl the requests go out
//! through, and the tracker the emulator thread runs a game's achievements in.
//!
//! Softcore unless Hardcore Achievements is on. Only a token is kept, in `Config/retroachievements.conf`: the password is
//! swapped for it once at sign-in and never written down, as FAT keeps no file permissions.
//!
//! With no connection, achievements are still earned: requests go through
//! `slot_cheevos::offline::Offline`, which keeps what it reads in `OFFLINE` and queues unlocks
//! there until the Wi-Fi is back.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use slot_cheevos::offline::Offline;
use slot_cheevos::{Cheevos, Console, Http, HttpRequest, Notice};
use slot_retro::RetroCore;
use slot_store::Platform;

pub const CONF: &str = "Config/retroachievements.conf";
/// Kept answers and unlocks waiting to be sent.
pub const OFFLINE: &str = "Config/retroachievements";
const API: &str = "https://retroachievements.org/dorequest.php";
/// How often a core that has not shown its memory yet is asked again, in frames.
const MAP_EVERY: u32 = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub user: String,
    pub token: String,
}

/// The account on the card, if one has signed in.
pub fn account(root: &Path) -> Option<Account> {
    let conf = slot_store::ini::read(root, CONF);
    let user = conf.get("user").filter(|u| !u.is_empty())?;
    let token = conf.get("token").filter(|t| !t.is_empty())?;
    Some(Account {
        user: user.clone(),
        token: token.clone(),
    })
}

pub fn save_account(root: &Path, account: &Account) -> std::io::Result<()> {
    let text = format!("user = {}\ntoken = {}\n", account.user, account.token);
    std::fs::create_dir_all(root.join(slot_store::CONFIG_DIR))?;
    slot_store::atomic_write(&root.join(CONF), text.as_bytes())
}

pub fn sign_out(root: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(root.join(CONF)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// RetroAchievements' console for a system, for the ones whose games it can hash. A core in a
/// host process of its own has its achievements followed there, beside its memory.
pub fn console_for(platform: Platform) -> Option<Console> {
    let id = match platform {
        Platform::Genesis => 1,
        Platform::Nintendo64 => 2,
        Platform::Snes => 3,
        Platform::Gb => 4,
        Platform::Gba => 5,
        Platform::Gbc => 6,
        Platform::Nes => 7,
        Platform::PcEngine | Platform::SuperGrafx => 8,
        Platform::SegaCd => 9,
        Platform::Sega32x => 10,
        Platform::MasterSystem => 11,
        Platform::PlayStation => 12,
        Platform::Lynx => 13,
        Platform::NeoGeoPocket => 14,
        Platform::GameGear => 15,
        Platform::Odyssey2 => 23,
        Platform::PokemonMini => 24,
        Platform::Atari2600 => 25,
        Platform::Arcade | Platform::Cps1 | Platform::Cps2 | Platform::Cps3 | Platform::NeoGeo => {
            27
        }
        Platform::VirtualBoy => 28,
        Platform::Msx => 29,
        Platform::Sg1000 => 33,
        Platform::AmstradCpc => 37,
        Platform::Dreamcast => 40,
        Platform::Psp => 41,
        Platform::ColecoVision => 44,
        Platform::Intellivision => 45,
        Platform::Vectrex => 46,
        Platform::PcFx => 49,
        Platform::Atari7800 => 51,
        Platform::WonderSwan => 53,
        Platform::NeoGeoCd => 56,
        Platform::ChannelF => 57,
        Platform::ZxSpectrum => 59,
        Platform::Supervision => 63,
        Platform::Tic80 => 65,
        Platform::MegaDuck => 69,
        Platform::PcEngineCd => 76,
        Platform::Fds => 81,
        _ => return None,
    };
    Some(Console(id))
}

fn user_agent() -> String {
    // unslot is a libretro frontend, like RetroArch. The RA server checks this header
    // to decide whether hardcore unlocks may be submitted. The version must match a
    // release the server considers current for the rcheevos it ships.
    format!(
        "RetroArch/1.19.1 (Linux; aarch64) rcheevos/{}",
        slot_cheevos::version()
    )
}

/// The internet, through BaseOS's curl. A request's body goes in on stdin, so a password or
/// token never shows in the process list.
pub struct Curl;

impl Http for Curl {
    fn send(&self, request: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
        post(
            &request.url,
            request.post.as_deref(),
            request.content_type.as_deref(),
        )
    }
}

fn post(url: &str, body: Option<&str>, kind: Option<&str>) -> Result<(u16, Vec<u8>), String> {
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "--connect-timeout", "10", "--max-time", "30", "-A"])
        .arg(user_agent())
        .args(["-w", "\n%{http_code}"]);
    if body.is_some() {
        if let Some(kind) = kind {
            cmd.arg("-H").arg(format!("Content-Type: {kind}"));
        }
        cmd.args(["--data-binary", "@-"]);
    }
    let mut child = cmd
        .arg(url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    if let (Some(mut stdin), Some(body)) = (child.stdin.take(), body) {
        let _ = stdin.write_all(body.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    let at = out
        .stdout
        .iter()
        .rposition(|&b| b == b'\n')
        .ok_or("curl gave no answer")?;
    let code = String::from_utf8_lossy(&out.stdout[at + 1..])
        .trim()
        .parse::<u16>()
        .unwrap_or(0);
    if code == 0 {
        return Err("RetroAchievements could not be reached".into());
    }
    Ok((code, out.stdout[..at].to_vec()))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The internet for a game's achievements: curl, through the card's cache and queue.
pub fn http(root: &Path) -> Arc<dyn Http> {
    Arc::new(Offline::new(&root.join(OFFLINE), Arc::new(Curl)))
}

/// How many unlocks wait to be sent.
pub fn pending(root: &Path) -> usize {
    Offline::pending(&root.join(OFFLINE))
}

/// Sends the unlocks earned with no connection, on a thread of its own.
pub fn sync_in_background(root: &Path) {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if pending(root) == 0 || RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let root = root.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("slot-cheevos-sync".into())
        .spawn(move || {
            Offline::new(&root.join(OFFLINE), Arc::new(Curl)).sync();
            RUNNING.store(false, Ordering::Release);
        });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::Release);
    }
}

/// Swaps a user name and password for the account's token. Blocks on the network.
pub fn sign_in(user: &str, password: &str) -> Result<Account, String> {
    let body = format!("r=login2&u={}&p={}", encode(user), encode(password));
    let (_, reply) = post(API, Some(&body), Some("application/x-www-form-urlencoded"))?;
    read_sign_in(&String::from_utf8_lossy(&reply))
}

/// What RetroAchievements said to a sign-in.
pub fn read_sign_in(reply: &str) -> Result<Account, String> {
    match (
        crate::json::string(reply, "User"),
        crate::json::string(reply, "Token"),
    ) {
        (Some(user), Some(token)) if !token.is_empty() => Ok(Account { user, token }),
        _ => Err(crate::json::string(reply, "Error")
            .unwrap_or_else(|| "RetroAchievements did not sign you in".into())),
    }
}

/// What the emulator thread needs to follow a game's achievements.
pub struct Start {
    pub account: Account,
    pub console: Console,
    pub http: Arc<dyn Http>,
    pub hardcore: bool,
}

/// The calls a core host's `Host` answers, the first byte of each.
const SIDE_START: u8 = 0;
const SIDE_FRAME: u8 = 1;
const SIDE_IDLE: u8 = 2;
const SIDE_RESET: u8 = 3;
const SIDE_SOFTCORE: u8 = 4;
const SIDE_PAUSE: u8 = 5;
/// How many idle calls a tracker in a core host lets by for each it passes on: rcheevos only
/// needs them for pings and retries, and each is a trip to the host.
const IDLE_EVERY: u32 = 30;

/// One game's achievements, on the emulator thread with the core whose memory it reads, or,
/// for a core in a host process of its own, beside it there (`Host`).
pub enum Tracker {
    Here(Local),
    There { notices: Vec<Notice>, idles: u32 },
}

impl Tracker {
    pub fn start(start: Start, rom: &Path, core: &mut dyn RetroCore) -> Option<Tracker> {
        let mut msg = vec![SIDE_START];
        msg.extend_from_slice(&start.console.0.to_le_bytes());
        msg.push(start.hardcore as u8);
        for field in [
            start.account.user.as_str(),
            &start.account.token,
            &rom.to_string_lossy(),
        ] {
            put(&mut msg, field);
        }
        match core.sidecar(&msg) {
            Some(reply) if reply == [1] => Some(Tracker::There {
                notices: Vec::new(),
                idles: 0,
            }),
            Some(_) => None,
            None => Local::start(start, rom).map(Tracker::Here),
        }
    }

    /// After every frame the game really ran, not one run ahead and taken back.
    pub fn frame(&mut self, core: &mut dyn RetroCore) {
        match self {
            Tracker::Here(t) => t.frame(core),
            Tracker::There { notices, .. } => ask(core, SIDE_FRAME, notices),
        }
    }

    /// While the game is paused, so pings and retried unlocks still go out.
    pub fn idle(&mut self, core: &mut dyn RetroCore) {
        match self {
            Tracker::Here(t) => t.idle(),
            Tracker::There { notices, idles } => {
                *idles += 1;
                if *idles >= IDLE_EVERY {
                    *idles = 0;
                    ask(core, SIDE_IDLE, notices);
                }
            }
        }
    }

    /// After the game's state was put back to another moment: a rewind or a loaded state.
    pub fn reset(&mut self, core: &mut dyn RetroCore) {
        match self {
            Tracker::Here(t) => t.reset(),
            Tracker::There { notices, .. } => ask(core, SIDE_RESET, notices),
        }
    }

    /// Hardcore off for the rest of the game, as the player turned it off.
    pub fn softcore(&mut self, core: &mut dyn RetroCore) {
        match self {
            Tracker::Here(t) => t.cheevos.set_hardcore(false),
            Tracker::There { notices, .. } => ask(core, SIDE_SOFTCORE, notices),
        }
    }

    /// Before play pauses: the frames hardcore wants played first, or 0 if it may pause now.
    pub fn pause_wait(&mut self, core: &mut dyn RetroCore) -> u32 {
        match self {
            Tracker::Here(t) => t.cheevos.pause_wait(),
            Tracker::There { .. } => core
                .sidecar(&[SIDE_PAUSE])
                .and_then(|r| Some(u32::from_le_bytes(r.get(..4)?.try_into().ok()?)))
                .unwrap_or(0),
        }
    }

    pub fn take(&mut self) -> Vec<Notice> {
        match self {
            Tracker::Here(t) => t.cheevos.take(),
            Tracker::There { notices, .. } => std::mem::take(notices),
        }
    }
}

/// Makes a call to the tracker in the core host, keeping the notices it answers with.
fn ask(core: &mut dyn RetroCore, call: u8, notices: &mut Vec<Notice>) {
    if let Some(reply) = core.sidecar(&[call]) {
        notices.extend(decode(&reply));
    }
}

/// The tracker for a core run where slot can read its memory.
pub struct Local {
    cheevos: Cheevos,
    console: Console,
    mapped: bool,
    until_map: u32,
}

impl Local {
    fn start(start: Start, rom: &Path) -> Option<Local> {
        let cheevos = Cheevos::new(start.http)?;
        cheevos.set_hardcore(start.hardcore);
        cheevos.login(&start.account.user, &start.account.token);
        cheevos.load_game(rom, start.console);
        Some(Local {
            cheevos,
            console: start.console,
            mapped: false,
            until_map: 0,
        })
    }

    fn frame(&mut self, core: &mut dyn RetroCore) {
        if !self.mapped {
            if self.until_map > 0 {
                self.until_map -= 1;
                self.cheevos.idle();
                return;
            }
            self.until_map = MAP_EVERY;
            let (descriptors, count) = core.memory_map();
            // The descriptors and the memory are the core's, which outlives the tracker.
            let bytes = unsafe {
                self.cheevos
                    .set_memory(descriptors, count, self.console, &mut |id| {
                        core.memory_region(id)
                    })
            };
            if bytes == 0 {
                self.cheevos.idle();
                return;
            }
            eprintln!("slot: cheevos: {bytes} bytes of the game's memory mapped");
            self.mapped = true;
        }
        self.cheevos.frame();
    }

    fn idle(&self) {
        self.cheevos.idle();
    }

    fn reset(&self) {
        self.cheevos.reset();
    }
}

/// The tracker in a core host, answering the `Tracker::There` in slot.
pub struct Host {
    root: std::path::PathBuf,
    tracker: Option<Local>,
}

impl Host {
    pub fn new(root: &Path) -> Host {
        Host {
            root: root.to_path_buf(),
            tracker: None,
        }
    }

    fn start(&mut self, msg: &[u8]) -> Option<()> {
        let console = Console(u32::from_le_bytes(msg.get(..4)?.try_into().ok()?));
        let hardcore = *msg.get(4)? == 1;
        let mut rest = &msg[5..];
        let (user, token, rom) = (take(&mut rest)?, take(&mut rest)?, take(&mut rest)?);
        let start = Start {
            account: Account { user, token },
            console,
            http: http(&self.root),
            hardcore,
        };
        self.tracker = Some(Local::start(start, Path::new(&rom))?);
        Some(())
    }
}

impl slot_retro::Sidecar for Host {
    fn call(&mut self, core: &mut dyn RetroCore, msg: &[u8]) -> Vec<u8> {
        let Some((&call, rest)) = msg.split_first() else {
            return Vec::new();
        };
        if call == SIDE_START {
            return vec![self.start(rest).is_some() as u8];
        }
        let Some(t) = &mut self.tracker else {
            return Vec::new();
        };
        if call == SIDE_PAUSE {
            return t.cheevos.pause_wait().to_le_bytes().to_vec();
        }
        match call {
            SIDE_FRAME => t.frame(core),
            SIDE_IDLE => t.idle(),
            SIDE_RESET => t.reset(),
            SIDE_SOFTCORE => t.cheevos.set_hardcore(false),
            _ => {}
        }
        encode_notices(&t.cheevos.take())
    }
}

fn put(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn take(from: &mut &[u8]) -> Option<String> {
    let len = u32::from_le_bytes(from.get(..4)?.try_into().ok()?) as usize;
    let s = String::from_utf8_lossy(from.get(4..4 + len)?).into_owned();
    *from = &from[4 + len..];
    Some(s)
}

/// Notices as they cross from a core host to slot: a kind and its strings, each.
fn encode_notices(notices: &[Notice]) -> Vec<u8> {
    let mut out = Vec::new();
    for n in notices.iter().cloned() {
        let (kind, fields) = match n {
            Notice::LoggedIn(Ok(())) => (0, vec![]),
            Notice::LoggedIn(Err(e)) => (1, vec![e]),
            Notice::GameLoaded(Ok((summary, badge))) => (2, vec![summary, badge]),
            Notice::GameLoaded(Err(e)) => (3, vec![e]),
            Notice::Unlocked {
                title,
                description,
                points,
                badge,
            } => (4, vec![title, description, points.to_string(), badge]),
            Notice::GameCompleted { title, badge } => (5, vec![title, badge]),
            Notice::LeaderboardStarted { title, description } => (6, vec![title, description]),
            Notice::LeaderboardFailed { title } => (7, vec![title]),
            Notice::LeaderboardSubmitted { title, score } => (8, vec![title, score]),
            Notice::ServerError { api, message } => (9, vec![api, message]),
            Notice::Disconnected => (10, vec![]),
            Notice::Reconnected => (11, vec![]),
            Notice::Softcore => (12, vec![]),
        };
        out.push(kind);
        for f in &fields {
            put(&mut out, f);
        }
    }
    out
}

fn decode(mut from: &[u8]) -> Vec<Notice> {
    let mut notices = Vec::new();
    while let Some((&kind, rest)) = from.split_first() {
        from = rest;
        let mut s = || take(&mut from);
        let n = (|| {
            Some(match kind {
                0 => Notice::LoggedIn(Ok(())),
                1 => Notice::LoggedIn(Err(s()?)),
                2 => Notice::GameLoaded(Ok((s()?, s()?))),
                3 => Notice::GameLoaded(Err(s()?)),
                4 => Notice::Unlocked {
                    title: s()?,
                    description: s()?,
                    points: s()?.parse().ok()?,
                    badge: s()?,
                },
                5 => Notice::GameCompleted {
                    title: s()?,
                    badge: s()?,
                },
                6 => Notice::LeaderboardStarted {
                    title: s()?,
                    description: s()?,
                },
                7 => Notice::LeaderboardFailed { title: s()? },
                8 => Notice::LeaderboardSubmitted {
                    title: s()?,
                    score: s()?,
                },
                9 => Notice::ServerError {
                    api: s()?,
                    message: s()?,
                },
                10 => Notice::Disconnected,
                11 => Notice::Reconnected,
                12 => Notice::Softcore,
                _ => return None,
            })
        })();
        match n {
            Some(n) => notices.push(n),
            None => break,
        }
    }
    notices
}

/// The line a notice is shown with during play, if it is one to show.
pub fn line(notice: &Notice) -> Option<String> {
    match notice {
        Notice::LoggedIn(Err(e)) => Some(format!("RetroAchievements: {e}")),
        Notice::GameLoaded(Ok((summary, _))) => Some(summary.clone()),
        Notice::Unlocked { title, points, .. } => Some(format!("Unlocked: {title} ({points})")),
        Notice::GameCompleted { title, .. } => Some(format!("Mastered {title}")),
        Notice::LeaderboardSubmitted { title, score } => Some(format!("{title}: {score}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sign_in_keeps_the_token_and_says_why_it_failed() {
        let ok = r#"{"Success":true,"User":"Benni","Token":"t0k3n","Score":10}"#;
        assert_eq!(
            read_sign_in(ok),
            Ok(Account {
                user: "Benni".into(),
                token: "t0k3n".into()
            })
        );
        let no = r#"{"Success":false,"Error":"Invalid user/password combination. Please try again.","Code":"invalid_credentials"}"#;
        assert_eq!(
            read_sign_in(no),
            Err("Invalid user/password combination. Please try again.".into())
        );
    }

    #[test]
    fn the_account_is_kept_without_its_password_and_forgotten_on_sign_out() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(account(d.path()), None);
        let a = Account {
            user: "Benni".into(),
            token: "t0k3n".into(),
        };
        save_account(d.path(), &a).unwrap();
        assert_eq!(account(d.path()), Some(a));
        sign_out(d.path()).unwrap();
        assert_eq!(account(d.path()), None);
        sign_out(d.path()).unwrap();
    }

    #[test]
    fn a_password_is_sent_percent_encoded() {
        assert_eq!(encode("p&ss word=1"), "p%26ss%20word%3D1");
    }

    #[test]
    fn systems_in_a_core_host_have_their_consoles() {
        assert_eq!(console_for(Platform::Gba), Some(Console::GBA));
        assert_eq!(console_for(Platform::Dreamcast), Some(Console::DC));
        assert_eq!(console_for(Platform::Nintendo64), Some(Console::N64));
        assert_eq!(console_for(Platform::Psp), Some(Console::PSP));
        assert_eq!(console_for(Platform::ScummVm), None);
    }

    #[test]
    fn notices_cross_from_a_core_host_as_they_were() {
        let notices = vec![
            Notice::LoggedIn(Ok(())),
            Notice::GameLoaded(Ok(("Crazy Taxi: 1 of 30 achievements".into(), "u".into()))),
            Notice::Unlocked {
                title: "Fare".into(),
                description: "Take a fare".into(),
                points: 5,
                badge: "".into(),
            },
            Notice::ServerError {
                api: "award".into(),
                message: "no".into(),
            },
            Notice::Reconnected,
            Notice::Softcore,
        ];
        assert_eq!(decode(&encode_notices(&notices)), notices);
        assert_eq!(decode(&[]), vec![]);
    }
}
