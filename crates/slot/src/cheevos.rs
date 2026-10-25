//! RetroAchievements: the account the card signs in with, the curl the requests go out
//! through, and the tracker the emulator thread runs a game's achievements in.
//!
//! Softcore only. Only a token is kept, in `Config/retroachievements.conf`: the password is
//! swapped for it once at sign-in and never written down, as FAT keeps no file permissions.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use slot_cheevos::{Cheevos, Console, Http, HttpRequest, Notice};
use slot_retro::RetroCore;
use slot_store::{Core, Platform};

pub const CONF: &str = "Config/retroachievements.conf";
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

/// RetroAchievements' console for a system, for the ones whose games it can hash and whose
/// core runs inside slot, where its memory can be read each frame.
pub fn console_for(platform: Platform, core: Core) -> Option<Console> {
    if matches!(
        core,
        Core::Nodosumi | Core::Flycast | Core::ParallelN64 | Core::Ppsspp
    ) {
        return None;
    }
    let id = match platform {
        Platform::Genesis => 1,
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
        Platform::Arcade
        | Platform::Cps1
        | Platform::Cps2
        | Platform::Cps3
        | Platform::NeoGeo => 27,
        Platform::VirtualBoy => 28,
        Platform::Msx => 29,
        Platform::Sg1000 => 33,
        Platform::AmstradCpc => 37,
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
    format!(
        "unslot/{} (Linux; aarch64) rcheevos/{}",
        env!("CARGO_PKG_VERSION"),
        slot_cheevos::version()
    )
}

/// The internet, through BaseOS's curl. A request's body goes in on stdin, so a password or
/// token never shows in the process list.
pub struct Curl;

impl Http for Curl {
    fn send(&self, request: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
        post(&request.url, request.post.as_deref(), request.content_type.as_deref())
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
}

/// One game's achievements, on the emulator thread with the core whose memory it reads.
pub struct Tracker {
    cheevos: Cheevos,
    console: Console,
    mapped: bool,
    until_map: u32,
}

impl Tracker {
    pub fn start(start: Start, rom: &Path) -> Option<Tracker> {
        let cheevos = Cheevos::new(start.http)?;
        cheevos.login(&start.account.user, &start.account.token);
        cheevos.load_game(rom, start.console);
        Some(Tracker {
            cheevos,
            console: start.console,
            mapped: false,
            until_map: 0,
        })
    }

    /// After every frame the game really ran, not one run ahead and taken back.
    pub fn frame(&mut self, core: &mut dyn RetroCore) {
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

    /// While the game is paused, so pings and retried unlocks still go out.
    pub fn idle(&self) {
        self.cheevos.idle();
    }

    /// After the game's state was put back to another moment: a rewind or a loaded state.
    pub fn reset(&self) {
        self.cheevos.reset();
    }

    pub fn take(&self) -> Vec<Notice> {
        self.cheevos.take()
    }
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
    fn cores_in_their_own_process_are_left_out() {
        assert_eq!(
            console_for(Platform::Gba, Core::Mgba),
            Some(Console::GBA)
        );
        assert_eq!(console_for(Platform::Dreamcast, Core::Flycast), None);
        assert_eq!(console_for(Platform::ScummVm, Core::Mgba), None);
    }
}
