//! RomM: a game library the player hosts themselves. The device signs in to it by a code scanned
//! with a phone, lists its games by system, fetches them into `Games/<SYS>`, and can take each
//! game's art from it in place of unslot's own search. It also fetches the games RomM is asked to
//! send to the device, and can keep the games' saves in step with RomM's.
//!
//! What the device knows of the server is kept in `Config/romm.conf`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use slot_store::{Cart, Core, Platform};

pub const CONF: &str = "Config/romm.conf";

/// What the device asks to be let do, when it signs in.
const SCOPES: [&str; 7] = [
    "me.read",
    "platforms.read",
    "roms.read",
    "assets.read",
    "assets.write",
    "devices.read",
    "devices.write",
];

/// How many games one page of the library holds.
const PAGE: usize = 500;

/// The slot a game's save is paired on across devices, as Grout names it, so a save made on a
/// handheld running Grout and one made here are the same save.
const SLOT: &str = "autosave";

/// How often RomM is asked for games sent to the device, while it is awake.
const POLL: Duration = Duration::from_secs(30);

/// Where a game's art, and later its details, come from.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Source {
    #[default]
    Unslot,
    Romm,
}

/// The server, and the device's sign in to it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Conf {
    /// Its address, as `http://host:port`, with no slash after.
    pub server: String,
    pub token: String,
    /// Who signed in, as RomM names them.
    pub user: String,
    /// The device, as RomM knows it from the sign in.
    pub device: String,
    pub art: Source,
    /// Whether the games' saves are kept in step with RomM's.
    pub saves: bool,
}

impl Conf {
    pub fn signed_in(&self) -> bool {
        !self.server.is_empty() && !self.token.is_empty()
    }

    /// Whether games' art comes from RomM rather than unslot's own search.
    pub fn art_from_romm(&self) -> bool {
        self.signed_in() && self.art == Source::Romm
    }

    /// Whether saves are synced, which needs the device RomM signed in.
    pub fn syncs_saves(&self) -> bool {
        self.signed_in() && self.saves && !self.device.is_empty()
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
        server: get("server"),
        token: get("token"),
        user: get("user"),
        device: get("device"),
        art: match get("art").as_str() {
            "romm" => Source::Romm,
            _ => Source::Unslot,
        },
        saves: get("saves") == "romm",
    }
}

pub fn write(root: &Path, conf: &Conf) -> std::io::Result<()> {
    let art = match conf.art {
        Source::Unslot => "unslot",
        Source::Romm => "romm",
    };
    let saves = match conf.saves {
        true => "romm",
        false => "off",
    };
    let text = format!(
        "server={}\ntoken={}\nuser={}\ndevice={}\nart={art}\nsaves={saves}\n",
        conf.server, conf.token, conf.user, conf.device
    );
    std::fs::create_dir_all(root.join("Config"))?;
    slot_store::atomic_write(&root.join(CONF), text.as_bytes())
}

/// A server's address as typed, made whole: `http://` when no scheme is given, and no slash after.
pub fn normalise(typed: &str) -> Option<String> {
    let typed = typed.trim().trim_end_matches('/');
    if typed.is_empty() || typed.contains(char::is_whitespace) {
        return None;
    }
    Some(match typed.contains("://") {
        true => typed.to_string(),
        false => format!("http://{typed}"),
    })
}

/// The server's address without its scheme, as the settings show it.
pub fn host(server: &str) -> &str {
    server.split_once("://").map_or(server, |(_, h)| h)
}

/// One request to the server.
pub struct Req<'a> {
    pub method: &'a str,
    pub url: String,
    pub token: Option<&'a str>,
    pub json: Option<String>,
    /// Where to write what comes back, in place of answering it.
    pub to: Option<&'a Path>,
    /// A file sent as the form's `saveFile`, under the name given, in place of `json`.
    pub file: Option<(&'a Path, &'a str)>,
}

impl<'a> Req<'a> {
    fn get(url: String, token: &'a str) -> Req<'a> {
        Req {
            method: "GET",
            url,
            token: Some(token),
            json: None,
            to: None,
            file: None,
        }
    }

    fn post(url: String, json: Value) -> Req<'a> {
        Req {
            method: "POST",
            url,
            token: None,
            json: Some(json.to_string()),
            to: None,
            file: None,
        }
    }

    /// A request that changes something, as the signed in device.
    fn send(method: &'a str, url: String, token: &'a str, json: Value) -> Req<'a> {
        Req {
            method,
            token: Some(token),
            ..Req::post(url, json)
        }
    }
}

/// The network, as the client uses it: the status and the body, which is empty when `to` took it.
pub trait Http: Send {
    fn send(&mut self, req: &Req) -> Result<(u16, Vec<u8>), String>;
}

/// The network, through BaseOS's curl. The token and the body go in on curl's standard input,
/// so neither shows in the list of what is running.
pub struct Curl;

/// A value for a curl config file, quoted.
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Http for Curl {
    fn send(&mut self, req: &Req) -> Result<(u16, Vec<u8>), String> {
        use std::io::Write;
        let tmp = std::env::temp_dir().join(format!(
            "slot-romm-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let out = req.to.unwrap_or(&tmp);
        let mut config = format!(
            "url = {}\nrequest = {}\noutput = {}\n",
            quoted(&req.url),
            req.method,
            quoted(&out.to_string_lossy())
        );
        if let Some(token) = req.token {
            config += &format!(
                "header = {}\n",
                quoted(&format!("Authorization: Bearer {token}"))
            );
        }
        if let Some((path, name)) = req.file {
            // curl's own quoting, inside the config's: a name may hold a `;`.
            let field = format!(
                "saveFile=@\"{}\";filename=\"{}\"",
                path.to_string_lossy().replace('"', ""),
                name.replace('"', "'")
            );
            config += &format!("form = {}\n", quoted(&field));
        } else if let Some(json) = &req.json {
            config += "header = \"Content-Type: application/json\"\n";
            config += &format!("data-binary = {}\n", quoted(json));
        }
        let mut child = std::process::Command::new("curl")
            .args([
                "-sSL",
                "--connect-timeout",
                "15",
                "-w",
                "%{http_code}",
                "-K",
                "-",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(config.as_bytes());
        }
        let done = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
        let code = String::from_utf8_lossy(&done.stdout)
            .trim()
            .parse()
            .unwrap_or(0);
        let body = match req.to {
            Some(_) => Vec::new(),
            None => {
                let body = std::fs::read(&tmp).unwrap_or_default();
                let _ = std::fs::remove_file(&tmp);
                body
            }
        };
        match code {
            0 => Err("The RomM server could not be reached".into()),
            code => Ok((code, body)),
        }
    }
}

/// The network for this build: the server on the device, a pretend one anywhere else.
pub fn for_build() -> Box<dyn Http> {
    #[cfg(all(feature = "device", unix))]
    return Box::new(Curl);
    #[cfg(not(all(feature = "device", unix)))]
    Box::new(SimRomm)
}

/// A pretend RomM for the desktop and the tests: it approves a sign in as soon as it is asked
/// about it, its library is one Game Boy game, nothing is ever sent to the device, and every save
/// is in step.
pub struct SimRomm;

impl SimRomm {
    pub const CODE: &'static str = "SIMCODE1";
    pub const GAME: &'static str = "Sim Game.gb";
    pub const DEVICE: &'static str = "sim-device";
}

impl Http for SimRomm {
    fn send(&mut self, req: &Req) -> Result<(u16, Vec<u8>), String> {
        let path = req.url.split_once("://").map_or("", |(_, r)| r);
        let path = &path[path.find('/').unwrap_or(path.len())..];
        let body = match path.split('?').next().unwrap_or("") {
            "/api/auth/device/init" => serde_json::json!({
                "device_code": "sim", "user_code": SimRomm::CODE,
                "verification_path_complete": format!("/pair/device?user_code={}", SimRomm::CODE),
                "expires_in": 600, "interval": 1,
            }),
            "/api/auth/device/token" => {
                serde_json::json!({ "access_token": "rmm_sim", "device_id": SimRomm::DEVICE })
            }
            "/api/users/me" => serde_json::json!({ "username": "player" }),
            "/api/platforms" => serde_json::json!([
                { "id": 1, "slug": "gb", "fs_slug": "gb", "name": "Game Boy", "rom_count": 1 },
            ]),
            "/api/roms" => serde_json::json!({ "total": 1, "items": [{
                "id": 1, "name": "Sim Game", "fs_name": SimRomm::GAME,
                "fs_name_no_ext": "Sim Game", "fs_size_bytes": 8, "has_multiple_files": false,
            }]}),
            "/api/devices/sim-device/installs/claim" => serde_json::json!([]),
            "/api/devices/sim-device" => serde_json::json!({}),
            "/api/sync/negotiate" => serde_json::json!({ "session_id": 1, "operations": [] }),
            "/api/sync/sessions/1/complete" => serde_json::json!({}),
            p if p.starts_with("/api/roms/1/content/") => {
                if let Some(to) = req.to {
                    std::fs::write(to, [0u8; 8]).map_err(|e| e.to_string())?;
                }
                return Ok((200, Vec::new()));
            }
            _ => return Ok((404, Vec::new())),
        };
        Ok((200, body.to_string().into_bytes()))
    }
}

/// A request's answer as JSON, or why there was none.
fn json(http: &mut dyn Http, req: &Req) -> Result<Value, String> {
    let (code, body) = http.send(req)?;
    match code {
        200..=299 => serde_json::from_slice(&body).map_err(|e| format!("RomM answered oddly: {e}")),
        401 | 403 => Err("RomM turned the device away: sign in again".into()),
        code => Err(format!("RomM answered {code}")),
    }
}

/// A sign in waiting on the player to approve it in RomM.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pairing {
    pub device_code: String,
    /// The code the player is shown, which the link carries too.
    pub user_code: String,
    /// The page that approves it, for the phone to open.
    pub url: String,
    pub interval: Duration,
    pub expires: Duration,
}

/// Asks the server to sign the device in, for the player to approve.
pub fn pair(http: &mut dyn Http, server: &str, name: &str) -> Result<Pairing, String> {
    let body = serde_json::json!({
        "client_device_identifier": name,
        "name": name,
        "client": "unslot",
        "platform": "linux",
        "client_version": env!("CARGO_PKG_VERSION"),
        "requested_scopes": SCOPES,
    });
    let v = json(
        http,
        &Req::post(format!("{server}/api/auth/device/init"), body),
    )?;
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    if s("device_code").is_empty() {
        return Err("This server cannot sign devices in: RomM 5.4 or later can".into());
    }
    Ok(Pairing {
        device_code: s("device_code"),
        user_code: s("user_code"),
        url: format!("{server}{}", s("verification_path_complete")),
        interval: Duration::from_secs(v["interval"].as_u64().unwrap_or(5).max(1)),
        expires: Duration::from_secs(v["expires_in"].as_u64().unwrap_or(600)),
    })
}

/// What a sign in the player approved gives the device.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Approved {
    pub token: String,
    /// The device, as RomM keeps it, for what is sent to it and its saves.
    pub device: String,
}

/// Whether the player has approved the sign in: its token once they have.
pub fn token(
    http: &mut dyn Http,
    server: &str,
    device_code: &str,
) -> Result<Option<Approved>, String> {
    let body = serde_json::json!({ "device_code": device_code });
    let (code, body) = http.send(&Req::post(format!("{server}/api/auth/device/token"), body))?;
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    match (code, v["detail"].as_str()) {
        (200, _) => Ok(v["access_token"].as_str().map(|t| Approved {
            token: t.to_string(),
            device: v["device_id"].as_str().unwrap_or_default().to_string(),
        })),
        (_, Some("authorization_pending" | "slow_down")) => Ok(None),
        (_, Some("access_denied")) => Err("The sign in was turned down in RomM".into()),
        (_, Some("expired_token")) => Err("The code ran out: sign in again".into()),
        (code, _) => Err(format!("RomM answered {code}")),
    }
}

/// Who the token signs in as.
pub fn whoami(http: &mut dyn Http, server: &str, token: &str) -> Result<String, String> {
    let v = json(http, &Req::get(format!("{server}/api/users/me"), token))?;
    Ok(v["username"].as_str().unwrap_or_default().to_string())
}

/// A system in the library that unslot plays.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Shelf {
    pub id: u64,
    pub name: String,
    pub platform: Platform,
    pub count: u64,
}

/// RomM's names for systems, as `slug` or `fs_slug`, against unslot's.
const SLUGS: &[(&str, Platform)] = &[
    ("gb", Platform::Gb),
    ("gbc", Platform::Gbc),
    ("gba", Platform::Gba),
    ("nes", Platform::Nes),
    ("famicom", Platform::Nes),
    ("fc", Platform::Nes),
    ("snes", Platform::Snes),
    ("sfam", Platform::Snes),
    ("sfc", Platform::Snes),
    ("genesis", Platform::Genesis),
    ("megadrive", Platform::Genesis),
    ("sms", Platform::MasterSystem),
    ("mastersystem", Platform::MasterSystem),
    ("gamegear", Platform::GameGear),
    ("tg16", Platform::PcEngine),
    ("pcengine", Platform::PcEngine),
    ("neo-geo-pocket", Platform::NeoGeoPocket),
    ("neo-geo-pocket-color", Platform::NeoGeoPocket),
    ("ngp", Platform::NeoGeoPocket),
    ("ngpc", Platform::NeoGeoPocket),
    ("wonderswan", Platform::WonderSwan),
    ("wonderswan-color", Platform::WonderSwan),
    ("lynx", Platform::Lynx),
    ("psx", Platform::PlayStation),
    ("ps1", Platform::PlayStation),
    ("arcade", Platform::Arcade),
    ("mame", Platform::Arcade),
    ("fbneo", Platform::Arcade),
    ("cps1", Platform::Cps1),
    ("cps2", Platform::Cps2),
    ("cps3", Platform::Cps3),
    ("neogeoaes", Platform::NeoGeo),
    ("neogeomvs", Platform::NeoGeo),
    ("neogeo", Platform::NeoGeo),
    ("neo-geo-cd", Platform::NeoGeoCd),
    ("neogeocd", Platform::NeoGeoCd),
    ("fds", Platform::Fds),
    ("n64", Platform::Nintendo64),
    ("virtualboy", Platform::VirtualBoy),
    ("pokemon-mini", Platform::PokemonMini),
    ("g-and-w", Platform::GameAndWatch),
    ("sega32", Platform::Sega32x),
    ("sega32x", Platform::Sega32x),
    ("segacd", Platform::SegaCd),
    ("sg1000", Platform::Sg1000),
    ("dc", Platform::Dreamcast),
    ("dreamcast", Platform::Dreamcast),
    ("turbografx-cd", Platform::PcEngineCd),
    ("pcenginecd", Platform::PcEngineCd),
    ("supergrafx", Platform::SuperGrafx),
    ("pc-fx", Platform::PcFx),
    ("atari2600", Platform::Atari2600),
    ("atari5200", Platform::Atari5200),
    ("atari7800", Platform::Atari7800),
    ("atari800", Platform::Atari800),
    ("atari8bit", Platform::Atari800),
    ("atari-st", Platform::AtariSt),
    ("colecovision", Platform::ColecoVision),
    ("intellivision", Platform::Intellivision),
    ("vectrex", Platform::Vectrex),
    ("odyssey", Platform::Odyssey2),
    ("odyssey-2", Platform::Odyssey2),
    ("videopac", Platform::Odyssey2),
    ("fairchild-channel-f", Platform::ChannelF),
    ("supervision", Platform::Supervision),
    ("mega-duck-slash-cougar-boy", Platform::MegaDuck),
    ("msx", Platform::Msx),
    ("msx2", Platform::Msx),
    ("c64", Platform::Commodore64),
    ("amiga", Platform::Amiga),
    ("zxs", Platform::ZxSpectrum),
    ("acpc", Platform::AmstradCpc),
    ("dos", Platform::Dos),
    ("pc-9800-series", Platform::Pc98),
    ("sharp-x68000", Platform::X68000),
    ("scummvm", Platform::ScummVm),
    ("pico-8", Platform::Pico8),
    ("pico", Platform::Pico8),
    ("tic-80", Platform::Tic80),
    ("psp", Platform::Psp),
    ("nds", Platform::Nds),
];

/// The system a RomM platform is, by its slug, then its folder's, then the folder named as
/// unslot names it.
pub fn platform_for(slug: &str, fs_slug: &str) -> Option<Platform> {
    let named = |s: &str| {
        SLUGS
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(s))
            .map(|(_, p)| *p)
    };
    named(slug)
        .or_else(|| named(fs_slug))
        .or_else(|| Platform::from_key(&fs_slug.to_ascii_lowercase()))
        .filter(|p| !matches!(p, Platform::Ports | Platform::Apps))
}

/// The library's systems that unslot plays and that hold games, by name.
pub fn shelves(http: &mut dyn Http, conf: &Conf) -> Result<Vec<Shelf>, String> {
    let v = json(
        http,
        &Req::get(format!("{}/api/platforms", conf.server), &conf.token),
    )?;
    let mut out: Vec<Shelf> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let s = |k: &str| p[k].as_str().unwrap_or_default();
            Some(Shelf {
                id: p["id"].as_u64()?,
                name: Some(s("display_name"))
                    .filter(|n| !n.is_empty())
                    .unwrap_or(s("name"))
                    .to_string(),
                platform: platform_for(s("slug"), s("fs_slug"))?,
                count: p["rom_count"].as_u64().unwrap_or(0),
            })
        })
        .filter(|s| s.count > 0)
        .collect();
    out.sort_by_key(|s| s.name.to_lowercase());
    Ok(out)
}

/// One game in the library.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Game {
    pub id: u64,
    pub name: String,
    pub platform: Platform,
    /// Its file's name, or its folder's when it is more than one file.
    pub fs_name: String,
    pub fs_name_no_ext: String,
    pub size: u64,
    pub multi: bool,
    /// Where its cover is on the server, without the server.
    pub cover: Option<String>,
}

impl Game {
    fn from_json(r: &Value, platform: Platform) -> Option<Game> {
        let s = |k: &str| r[k].as_str().unwrap_or_default().to_string();
        let fs_name = s("fs_name");
        if fs_name.is_empty() {
            return None;
        }
        Some(Game {
            id: r["id"].as_u64()?,
            name: Some(s("name"))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| s("fs_name_no_ext")),
            platform,
            fs_name_no_ext: s("fs_name_no_ext"),
            fs_name,
            size: r["fs_size_bytes"].as_u64().unwrap_or(0),
            multi: r["has_multiple_files"].as_bool().unwrap_or(false),
            // The time it was made goes after a `?`, with a space in it: it is not needed.
            cover: r["path_cover_large"]
                .as_str()
                .or(r["path_cover_small"].as_str())
                .map(|p| p.split('?').next().unwrap_or(p).to_string())
                .filter(|p| !p.is_empty()),
        })
    }

    /// Where the game goes on the card: its file, or its folder.
    pub fn path(&self, root: &Path) -> PathBuf {
        root.join("Games")
            .join(self.platform.dir_name())
            .join(&self.fs_name)
    }

    /// Whether it is on the card, as fetched or as unpacked from its zip.
    pub fn on_card(&self, root: &Path) -> bool {
        let path = self.path(root);
        path.exists()
            || (is_zip(&self.fs_name)
                && !self.platform.accepts(&path)
                && self
                    .platform
                    .extensions()
                    .iter()
                    .map(|e| path.with_file_name(format!("{}.{e}", self.fs_name_no_ext)))
                    .chain([path.with_file_name(&self.fs_name_no_ext)])
                    .any(|p| p.exists()))
    }

    /// The name the shelf will know it by once it is on the card, as a scan names it.
    pub fn stem(&self) -> String {
        match self.multi {
            true => self.fs_name.clone(),
            false => Path::new(&self.fs_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&self.fs_name_no_ext)
                .to_string(),
        }
    }
}

/// The games of one of the library's systems, by name.
pub fn games(http: &mut dyn Http, conf: &Conf, shelf: &Shelf) -> Result<Vec<Game>, String> {
    let mut out = Vec::new();
    loop {
        let url = format!(
            "{}/api/roms?platform_ids={}&limit={PAGE}&offset={}&order_by=name&order_dir=asc\
             &with_char_index=false&with_filter_values=false",
            conf.server,
            shelf.id,
            out.len()
        );
        let v = json(http, &Req::get(url, &conf.token))?;
        let items = v["items"].as_array().cloned().unwrap_or_default();
        let total = v["total"].as_u64().unwrap_or(0) as usize;
        let got = items.len();
        out.extend(
            items
                .iter()
                .filter_map(|r| Game::from_json(r, shelf.platform)),
        );
        if got == 0 || out.len() >= total {
            break;
        }
    }
    Ok(out)
}

/// A name made safe to go in a URL's path or query.
fn escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One file into `to`, by way of a hidden file beside it so a scan never finds half a game.
fn fetch_to(http: &mut dyn Http, conf: &Conf, url: String, to: &Path) -> Result<(), String> {
    let part = to.with_file_name(format!(
        ".{}.part",
        to.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download")
    ));
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let req = Req {
        to: Some(&part),
        ..Req::get(url, &conf.token)
    };
    let fetched = http.send(&req).and_then(|(code, _)| match code {
        200..=299 => std::fs::rename(&part, to).map_err(|e| format!("{}: {e}", to.display())),
        401 | 403 => Err("RomM turned the device away: sign in again".into()),
        code => Err(format!("RomM answered {code}")),
    });
    if fetched.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    fetched
}

/// Where a game's download is being written, for its progress to be read off the card.
pub fn part_of(root: &Path, game: &Game) -> PathBuf {
    game.path(root)
        .with_file_name(format!(".{}.part", game.fs_name))
}

fn is_zip(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// Fetches a game onto the card, so it is on the shelf: answers the file the shelf plays. A game
/// of many files goes in a folder of its name, with a playlist of its discs named after the
/// folder, which is how the shelf finds it. A zip, for a system that plays no zips, is unpacked.
/// Nothing the shelf could not show is left on the card.
pub fn download(
    http: &mut dyn Http,
    conf: &Conf,
    root: &Path,
    game: &Game,
) -> Result<PathBuf, String> {
    let path = game.path(root);
    if !game.multi {
        let url = format!(
            "{}/api/roms/{}/content/{}",
            conf.server,
            game.id,
            escape(&game.fs_name)
        );
        if game.platform.accepts(&path) {
            return fetch_to(http, conf, url, &path).map(|()| path);
        }
        if !is_zip(&game.fs_name) {
            return Err(cannot_play(game));
        }
        let zip = part_of(root, game).with_extension("zip");
        fetch_to(http, conf, url, &zip)?;
        let unpacked = unpack(&zip, root, game);
        let _ = std::fs::remove_file(&zip);
        return unpacked;
    }
    let v = json(
        http,
        &Req::get(format!("{}/api/roms/{}", conf.server, game.id), &conf.token),
    )?;
    let top = v["full_path"].as_str().unwrap_or_default().to_string();
    // The folder is filled under a hidden name, then given its own once it is whole.
    let part = part_of(root, game);
    let _ = std::fs::remove_dir_all(&part);
    let mut names = Vec::new();
    for f in v["files"].as_array().into_iter().flatten() {
        let (Some(id), Some(name)) = (f["id"].as_u64(), f["file_name"].as_str()) else {
            continue;
        };
        // A file in a folder within the game's keeps its place there.
        let dir = f["file_path"].as_str().unwrap_or_default();
        let sub = dir.strip_prefix(&top).unwrap_or("").trim_start_matches('/');
        let rel = Path::new(sub).join(name);
        if rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            continue;
        }
        let url = format!(
            "{}/api/roms/{}/content/{}?file_ids={id}",
            conf.server,
            game.id,
            escape(name)
        );
        if let Err(e) = fetch_to(http, conf, url, &part.join(&rel)) {
            let _ = std::fs::remove_dir_all(&part);
            return Err(e);
        }
        names.push(rel);
    }
    if names.is_empty() {
        return Err(format!("RomM has no files for {}", game.name));
    }
    let placed = place_folder(&part, &names, game, &path);
    if placed.is_err() {
        let _ = std::fs::remove_dir_all(&part);
    }
    placed
}

fn cannot_play(game: &Game) -> String {
    let ext = Path::new(&game.fs_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    format!(
        "unslot cannot play .{ext} games for {}",
        game.platform.name()
    )
}

/// Gives a filled folder its own name among the games, with a playlist if it needs one to be
/// found. Answers what the shelf plays of it.
fn place_folder(part: &Path, names: &[PathBuf], game: &Game, to: &Path) -> Result<PathBuf, String> {
    let name = to.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let own = |ext: &str| part.join(format!("{name}.{ext}"));
    let exts = game.platform.extensions();
    if !exts.iter().any(|e| own(e).is_file()) {
        let m3u = playlist(names, game.platform);
        if m3u.is_empty() || !exts.contains(&"m3u") {
            return Err(format!(
                "{} has no game among its files that unslot plays",
                game.name
            ));
        }
        std::fs::write(own("m3u"), m3u).map_err(|e| e.to_string())?;
    }
    let ext = exts
        .iter()
        .find(|e| own(e).is_file())
        .copied()
        .unwrap_or("m3u");
    std::fs::rename(part, to).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(to.join(format!("{name}.{ext}")))
}

/// A fetched zip's game among the system's: its one file, or a folder of its files.
fn unpack(zip: &Path, root: &Path, game: &Game) -> Result<PathBuf, String> {
    let part = part_of(root, game);
    let _ = std::fs::remove_dir_all(&part);
    let unpacked = unpack_into(zip, &part, game);
    if unpacked.is_err() {
        let _ = std::fs::remove_dir_all(&part);
    }
    unpacked
}

fn unpack_into(zip: &Path, part: &Path, game: &Game) -> Result<PathBuf, String> {
    crate::unzip::extract(zip, part, "")?;
    let mut names = Vec::new();
    files_in(part, Path::new(""), &mut names);
    if !names.iter().any(|n| game.platform.accepts(n)) {
        return Err(cannot_play(game));
    }
    let games = zip.parent().unwrap_or(Path::new("."));
    if let [one] = names.as_slice() {
        let to = games.join(one.file_name().unwrap_or_default());
        std::fs::rename(part.join(one), &to).map_err(|e| format!("{}: {e}", to.display()))?;
        let _ = std::fs::remove_dir_all(part);
        return Ok(to);
    }
    place_folder(part, &names, game, &games.join(&game.fs_name_no_ext))
}

/// The files under `dir`, by where they are in it.
fn files_in(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir.join(rel)) else {
        return;
    };
    for e in entries.flatten() {
        let rel = rel.join(e.file_name());
        match e.file_type() {
            Ok(t) if t.is_dir() => files_in(dir, &rel, out),
            Ok(t) if t.is_file() => out.push(rel),
            _ => {}
        }
    }
}

/// A playlist of a game's discs: its cue sheets where it has them, else each file the system
/// plays, in name order.
fn playlist(names: &[PathBuf], platform: Platform) -> String {
    let ext = |p: &PathBuf, e: &str| {
        p.extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x.eq_ignore_ascii_case(e))
    };
    let mut discs: Vec<&PathBuf> = names.iter().filter(|p| ext(p, "cue")).collect();
    if discs.is_empty() {
        discs = names
            .iter()
            .filter(|p| !ext(p, "m3u") && platform.accepts(p))
            .collect();
    }
    discs.sort();
    discs
        .iter()
        .map(|p| format!("{}\n", p.to_string_lossy()))
        .collect()
}

/// A game's cover, as RomM keeps it.
pub fn cover(http: &mut dyn Http, conf: &Conf, path: &str) -> Result<Option<Vec<u8>>, String> {
    let (code, body) = http.send(&Req::get(format!("{}{path}", conf.server), &conf.token))?;
    Ok((code == 200 && !body.is_empty()).then_some(body))
}

/// Each system's games in the library.
pub type Library = HashMap<Platform, Vec<Game>>;

/// The whole library, by system.
pub fn library(http: &mut dyn Http, conf: &Conf) -> Result<Library, String> {
    let mut all: Library = HashMap::new();
    for shelf in shelves(http, conf)? {
        all.entry(shelf.platform)
            .or_default()
            .extend(games(http, conf, &shelf)?);
    }
    Ok(all)
}

/// The library's game a cart on the card is, matched by file name.
pub fn game_of<'a>(library: &'a Library, cart: &Cart) -> Option<&'a Game> {
    let file = cart.rom.file_name().and_then(|f| f.to_str()).unwrap_or("");
    let folder = cart
        .rom
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|f| f.to_str())
        .unwrap_or("");
    library
        .get(&cart.platform)
        .into_iter()
        .flatten()
        .find(|g| match g.multi {
            true => g.fs_name == folder,
            false => g.fs_name == file || g.fs_name_no_ext == cart.stem,
        })
}

/// A cart's label, made from its cover in RomM. `library` keeps the library once asked for.
pub fn label_for(
    http: &mut dyn Http,
    conf: &Conf,
    library: &mut Option<Library>,
    cart: &Cart,
) -> Result<Option<Vec<u8>>, String> {
    if library.is_none() {
        *library = Some(self::library(http, conf)?);
    }
    let game = library.as_ref().and_then(|l| game_of(l, cart));
    let Some(path) = game.and_then(|g| g.cover.clone()) else {
        return Ok(None);
    };
    Ok(cover(http, conf, &path)?.and_then(|bytes| made_label(cart, &bytes)))
}

/// A label for a cart from its cover.
pub fn made_label(cart: &Cart, cover: &[u8]) -> Option<Vec<u8>> {
    let (w, h) = slot_ui::label_size(cart);
    let hue = slot_ui::hue_of(slot_ui::label_colour(&slot_ui::label_text(cart)));
    slot_ui::scraped_label(None, Some(cover), hue, w * 2, h * 2)
}

/// The cart a game makes once it is on the card as `rom`, for its label to be drawn to fit.
fn cart_of(game: &Game, rom: PathBuf) -> Cart {
    Cart {
        platform: game.platform,
        stem: rom
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| game.stem()),
        rom,
        label: None,
        title: String::new(),
        code: String::new(),
        shell: None,
    }
}

/// Whether a system's saves are the `.sav` beside the others, which the sync keeps. The systems
/// played outside slot keep theirs where their own emulators do.
fn saves_here(platform: Platform) -> bool {
    !matches!(
        platform,
        Platform::Nds | Platform::Psp | Platform::Pico8 | Platform::Ports | Platform::Apps
    )
}

/// A time as RomM writes one, in UTC.
fn rfc3339(secs: i64) -> String {
    let (y, m, d) = slot_store::civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// A time RomM wrote, as seconds since 1970: its offset is taken off, its fraction left off.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let days = slot_store::days_from_civil(num(0..4)?, num(5..7)?, num(8..10)?);
    let secs = days * 86_400 + num(11..13)? * 3600 + num(14..16)? * 60 + num(17..19)?;
    let rest = s
        .get(19..)?
        .trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match rest.as_bytes().first() {
        Some(sign @ (b'+' | b'-')) => {
            let hm = rest.get(1..)?.replace(':', "");
            let at =
                hm.get(0..2)?.parse::<i64>().ok()? * 3600 + hm.get(2..4)?.parse::<i64>().ok()? * 60;
            if *sign == b'+' {
                at
            } else {
                -at
            }
        }
        _ => 0,
    };
    Some(secs - offset)
}

fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What a sync of the saves came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Synced {
    pub sent: usize,
    pub fetched: usize,
    pub failed: usize,
}

/// A game on the card with its save, as the sync knows it.
struct Kept {
    rom_id: u64,
    cart: Cart,
    core: Core,
    sav: Option<Vec<u8>>,
    /// When its save was last written.
    at: i64,
}

impl Kept {
    fn file_name(&self) -> String {
        format!("{}.sav", self.cart.stem)
    }
}

/// Keeps the card's saves in step with RomM's: the newer of two goes both ways, and the one it
/// replaces is kept in RomM. Saves pair on the slot Grout uses, so they cross with its devices.
pub fn sync_saves(http: &mut dyn Http, conf: &Conf, root: &Path) -> Result<Synced, String> {
    let library = library(http, conf)?;
    let carts = slot_store::scan(root).map_err(|e| e.to_string())?;
    let mut kept: Vec<Kept> = Vec::new();
    for cart in carts.into_iter().filter(|c| saves_here(c.platform)) {
        let Some(rom_id) = game_of(&library, &cart).map(|g| g.id) else {
            continue;
        };
        // A game on the card twice is synced once.
        if kept.iter().any(|k| k.rom_id == rom_id) {
            continue;
        }
        let (platform, stem) = (cart.platform, cart.stem.clone());
        let Ok(sav) = crate::persist::read_sav(root, platform, &stem) else {
            continue;
        };
        let at = crate::persist::sav_file(root, platform, &stem)
            .and_then(|p| std::fs::metadata(p).ok()?.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        kept.push(Kept {
            rom_id,
            core: slot_store::core_for_platform(root, &stem, platform),
            cart,
            sav,
            at,
        });
    }
    let saves: Vec<Value> = kept
        .iter()
        .filter_map(|k| {
            let sav = k.sav.as_ref()?;
            Some(serde_json::json!({
                "rom_id": k.rom_id,
                "file_name": k.file_name(),
                "slot": SLOT,
                "emulator": k.core.as_str(),
                "content_hash": md5_hex(sav),
                "updated_at": rfc3339(k.at),
                "file_size_bytes": sav.len(),
            }))
        })
        .collect();
    let mut ask = serde_json::json!({ "device_id": conf.device, "saves": saves });
    if kept.len() <= 500 {
        // Saves of games on the card that has none yet come to it too.
        ask["rom_ids"] = kept.iter().map(|k| k.rom_id).collect();
        ask["restore_unlisted"] = true.into();
    }
    let url = format!("{}/api/sync/negotiate", conf.server);
    let v = json(http, &Req::send("POST", url, &conf.token, ask))?;
    let session = v["session_id"].as_u64();
    let mut synced = Synced::default();
    for op in v["operations"].as_array().into_iter().flatten() {
        let Some(k) = kept
            .iter()
            .find(|k| Some(k.rom_id) == op["rom_id"].as_u64())
        else {
            continue;
        };
        let save_id = op["save_id"].as_u64();
        let done = match op["action"].as_str().unwrap_or_default() {
            "upload" => {
                send_save(http, conf, k, session, Some(SLOT), false).map(|()| synced.sent += 1)
            }
            "download" => fetch_save(http, conf, root, k, save_id).map(|()| synced.fetched += 1),
            "conflict" => {
                let theirs = op["server_updated_at"].as_str().and_then(parse_rfc3339);
                match theirs.is_some_and(|t| t > k.at) {
                    // RomM's is newer: the card's is kept there, out of the slot, then replaced.
                    true => send_save(http, conf, k, session, None, false)
                        .and_then(|()| fetch_save(http, conf, root, k, save_id))
                        .map(|()| synced.fetched += 1),
                    // The card's is newer: it goes in the slot, which keeps RomM's as before.
                    false => send_save(http, conf, k, session, Some(SLOT), true)
                        .map(|()| synced.sent += 1),
                }
            }
            _ => continue,
        };
        if let Err(e) = done {
            eprintln!("slot: romm: {}: {e}", k.cart.stem);
            synced.failed += 1;
        }
    }
    if let Some(id) = session {
        let url = format!("{}/api/sync/sessions/{id}/complete", conf.server);
        let total = serde_json::json!({
            "operations_completed": synced.sent + synced.fetched,
            "operations_failed": synced.failed,
        });
        if let Err(e) = json(http, &Req::send("POST", url, &conf.token, total)) {
            eprintln!("slot: romm: sync: {e}");
        }
    }
    Ok(synced)
}

/// Sends a game's save to RomM, into `slot`, or kept out of the way without one.
fn send_save(
    http: &mut dyn Http,
    conf: &Conf,
    k: &Kept,
    session: Option<u64>,
    slot: Option<&str>,
    overwrite: bool,
) -> Result<(), String> {
    let sav = k.sav.as_deref().ok_or("it has no save to send")?;
    let mut url = format!(
        "{}/api/saves?rom_id={}&emulator={}&device_id={}&content_hash={}",
        conf.server,
        k.rom_id,
        escape(k.core.as_str()),
        escape(&conf.device),
        md5_hex(sav)
    );
    if let Some(slot) = slot {
        url += &format!("&slot={}", escape(slot));
    }
    if let Some(id) = session {
        url += &format!("&session_id={id}");
    }
    if overwrite {
        url += "&overwrite=true";
    }
    let tmp =
        std::env::temp_dir().join(format!("slot-romm-{}-{}.sav", std::process::id(), k.rom_id));
    std::fs::write(&tmp, sav).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let name = k.file_name();
    let req = Req {
        method: "POST",
        json: None,
        file: Some((&tmp, &name)),
        ..Req::get(url, &conf.token)
    };
    let sent = json(http, &req).map(|_| ());
    let _ = std::fs::remove_file(&tmp);
    sent
}

/// Puts RomM's save for a game on the card. Its resume, from before, would put the old save back
/// once loaded, so it goes with the others the player can still pick.
fn fetch_save(
    http: &mut dyn Http,
    conf: &Conf,
    root: &Path,
    k: &Kept,
    save_id: Option<u64>,
) -> Result<(), String> {
    let id = save_id.ok_or("RomM named no save")?;
    let url = format!(
        "{}/api/saves/{id}/content?device_id={}&optimistic=false",
        conf.server,
        escape(&conf.device)
    );
    let (code, bytes) = http.send(&Req::get(url, &conf.token))?;
    match code {
        200..=299 if !bytes.is_empty() => {}
        200..=299 => return Err("RomM's save is empty".into()),
        code => return Err(format!("RomM answered {code}")),
    }
    let (platform, stem) = (k.cart.platform, k.cart.stem.as_str());
    crate::persist::write_sav(root, platform, stem, &bytes).map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    slot_store::StateRing::new(root, platform, k.core, stem)
        .retire_resume(&slot_store::format_stamp(now))
        .map_err(|e| e.to_string())?;
    let url = format!("{}/api/saves/{id}/downloaded", conf.server);
    let told = serde_json::json!({ "device_id": conf.device, "content_hash": md5_hex(&bytes) });
    json(http, &Req::send("POST", url, &conf.token, told)).map(|_| ())
}

/// Fetches the games RomM was asked to send to the device, telling it how each went. Answers
/// the names of those it put on the shelf.
pub fn installs(http: &mut dyn Http, conf: &Conf, root: &Path) -> Result<Vec<String>, String> {
    let base = format!(
        "{}/api/devices/{}/installs",
        conf.server,
        escape(&conf.device)
    );
    let claim = Req::send(
        "POST",
        format!("{base}/claim"),
        &conf.token,
        serde_json::json!({}),
    );
    let asked = json(http, &claim)?;
    let mut got = Vec::new();
    for r in asked.as_array().into_iter().flatten() {
        let Some(id) = r["id"]
            .as_str()
            .map(str::to_string)
            .or(r["id"].as_u64().map(|n| n.to_string()))
        else {
            continue;
        };
        let installed = install(http, conf, root, r["rom_id"].as_u64());
        let (status, reason) = match &installed {
            Ok(Some(_)) => ("done", String::new()),
            Ok(None) => ("already_installed", String::new()),
            // RomM keeps no more of a reason than this.
            Err(e) => ("failed", e.chars().take(500).collect()),
        };
        let told = serde_json::json!({ "status": status, "reason": reason });
        let url = format!("{base}/{}", escape(&id));
        if let Err(e) = json(http, &Req::send("PUT", url, &conf.token, told)) {
            eprintln!("slot: romm: install {id}: {e}");
        }
        match installed {
            Ok(Some(name)) => got.push(name),
            Ok(None) => {}
            Err(e) => eprintln!("slot: romm: install {id}: {e}"),
        }
    }
    Ok(got)
}

/// Tells RomM the device takes games sent to it, which RomM asks of a device before it lets
/// anything be sent.
pub fn announce(http: &mut dyn Http, conf: &Conf) -> Result<(), String> {
    let url = format!("{}/api/devices/{}", conf.server, escape(&conf.device));
    let caps = serde_json::json!({ "capabilities": { "remote_install": true } });
    json(http, &Req::send("PUT", url, &conf.token, caps)).map(|_| ())
}

/// One game sent to the device: its name once on the shelf, or None if it was already.
fn install(
    http: &mut dyn Http,
    conf: &Conf,
    root: &Path,
    rom_id: Option<u64>,
) -> Result<Option<String>, String> {
    let rom_id = rom_id.ok_or("RomM named no game")?;
    let r = json(
        http,
        &Req::get(format!("{}/api/roms/{rom_id}", conf.server), &conf.token),
    )?;
    let s = |k: &str| r[k].as_str().unwrap_or_default();
    let platform = platform_for(s("platform_slug"), s("platform_fs_slug"))
        .ok_or_else(|| format!("unslot has no shelf for {}", s("platform_display_name")))?;
    let game = Game::from_json(&r, platform).ok_or("RomM's game has no file")?;
    if game.on_card(root) {
        return Ok(None);
    }
    let rom = download(http, conf, root, &game)?;
    if conf.art_from_romm() {
        label_game(http, conf, root, &game, rom);
    }
    Ok(Some(game.name))
}

/// What the app asks of the worker.
enum Job {
    Pair { server: String, name: String },
    Shelves,
    Games(Shelf),
    Get(Game),
    Sync,
}

/// What the worker is doing and has come to, for the pages to show.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RommView {
    /// What it is busy with, said as the page's status.
    pub busy: Option<String>,
    /// A sign in waiting on the player.
    pub pairing: Option<Pairing>,
    /// Bumped each time the sign in changes, for the app to read the file again.
    pub signed: usize,
    pub shelves: Option<Vec<Shelf>>,
    /// The games of the system last opened.
    pub games: Option<(u64, Vec<Game>)>,
    /// The game being fetched, with where its bytes are landing.
    pub getting: Option<(Game, PathBuf)>,
    /// Bumped with each game fetched, for the shelves to look again.
    pub got: usize,
    /// What the last thing done came to, when it is worth saying.
    pub note: Option<String>,
    pub error: Option<String>,
}

/// The handle the app keeps.
pub struct Romm {
    jobs: Sender<Job>,
    view: Arc<Mutex<RommView>>,
    /// Set to give up on a sign in waiting for the player.
    stop: Arc<AtomicBool>,
    /// Whether RomM may be asked for games sent to the device: not while it dozes or is offline.
    awake: Arc<AtomicBool>,
}

impl Romm {
    pub fn spawn(http: Box<dyn Http>, root: &Path) -> Romm {
        let (jobs, rx) = channel();
        let view = Arc::new(Mutex::new(RommView::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let awake = Arc::new(AtomicBool::new(false));
        let (shared, stopped, root) = (view.clone(), stop.clone(), root.to_path_buf());
        let woke = awake.clone();
        let spawned = std::thread::Builder::new()
            .name("slot-romm".into())
            .spawn(move || work(http, root, rx, shared, stopped, woke));
        if let Err(e) = spawned {
            eprintln!("slot: romm: {e}");
        }
        Romm {
            jobs,
            view,
            stop,
            awake,
        }
    }

    pub fn for_card(root: &Path) -> Romm {
        Romm::spawn(for_build(), root)
    }

    pub fn view(&self) -> RommView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn ask(&self, job: Job) {
        let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
        view.error = None;
        view.note = None;
        let _ = self.jobs.send(job);
    }

    /// Signs the device in to `server` as `name`, by a code the player approves.
    pub fn pair(&self, server: &str, name: &str) {
        self.stop.store(false, Ordering::Relaxed);
        self.ask(Job::Pair {
            server: server.to_string(),
            name: name.to_string(),
        });
    }

    /// Gives up on the sign in waiting for the player.
    pub fn stop_pairing(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn list_shelves(&self) {
        self.ask(Job::Shelves);
    }

    pub fn list_games(&self, shelf: Shelf) {
        self.ask(Job::Games(shelf));
    }

    pub fn get(&self, game: Game) {
        self.ask(Job::Get(game));
    }

    /// Brings the card's saves and RomM's into step.
    pub fn sync(&self) {
        self.ask(Job::Sync);
    }

    pub fn set_awake(&self, awake: bool) {
        self.awake.store(awake, Ordering::Relaxed);
    }

    /// Forgets what was listed, once the device signs out.
    pub fn forget(&self) {
        let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
        view.shelves = None;
        view.games = None;
    }
}

fn work(
    mut http: Box<dyn Http>,
    root: PathBuf,
    jobs: Receiver<Job>,
    view: Arc<Mutex<RommView>>,
    stop: Arc<AtomicBool>,
    awake: Arc<AtomicBool>,
) {
    let set =
        |f: &mut dyn FnMut(&mut RommView)| f(&mut view.lock().unwrap_or_else(|e| e.into_inner()));
    // The first look for games sent to the device waits for it to settle in after boot.
    let mut poll_at = Instant::now() + Duration::from_secs(5);
    // The device RomM was last told takes games sent to it.
    let mut announced = String::new();
    loop {
        let job = match jobs.recv_timeout(poll_at.saturating_duration_since(Instant::now())) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => {
                poll_at = Instant::now() + POLL;
                if awake.load(Ordering::Relaxed) {
                    poll(&mut *http, &root, &mut announced, &set);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        // Read afresh each time, for a sign in or a change of server since.
        let conf = read(&root);
        let result = match job {
            Job::Pair { server, name } => {
                set(&mut |v| v.busy = Some("Asking RomM for a code".into()));
                sign_in(&mut *http, &root, &server, &name, &stop, &set)
            }
            Job::Shelves => {
                set(&mut |v| v.busy = Some("Looking through the library".into()));
                shelves(&mut *http, &conf).map(|s| set(&mut |v| v.shelves = Some(s.clone())))
            }
            Job::Games(shelf) => {
                set(&mut |v| v.busy = Some(format!("Looking through {}", shelf.name)));
                games(&mut *http, &conf, &shelf)
                    .map(|g| set(&mut |v| v.games = Some((shelf.id, g.clone()))))
            }
            Job::Get(game) => {
                set(&mut |v| {
                    v.busy = Some(format!("Fetching {}", game.name));
                    v.getting = Some((game.clone(), part_of(&root, &game)));
                });
                let got = download(&mut *http, &conf, &root, &game);
                set(&mut |v| v.getting = None);
                got.map(|rom| {
                    let labelled =
                        conf.art_from_romm() && label_game(&mut *http, &conf, &root, &game, rom);
                    set(&mut |v| {
                        v.got += 1;
                        v.note = Some(match labelled {
                            true => format!("{} is on the shelf, with its art", game.name),
                            false => format!("{} is on the shelf", game.name),
                        });
                    })
                })
            }
            Job::Sync => {
                set(&mut |v| v.busy = Some("Syncing saves with RomM".into()));
                sync_saves(&mut *http, &conf, &root).map(|s| set(&mut |v| v.note = Some(said(s))))
            }
        };
        set(&mut |v| {
            v.busy = None;
            v.pairing = None;
            if let Err(e) = &result {
                eprintln!("slot: romm: {e}");
                v.error = Some(e.clone());
            }
        });
    }
}

/// Puts a fetched game's cover on its cart, `rom`. Whether it had one.
fn label_game(http: &mut dyn Http, conf: &Conf, root: &Path, game: &Game, rom: PathBuf) -> bool {
    let cart = cart_of(game, rom);
    let Some(png) = game
        .cover
        .as_deref()
        .and_then(|p| cover(http, conf, p).ok().flatten())
        .and_then(|bytes| made_label(&cart, &bytes))
    else {
        return false;
    };
    let dir = root.join("Labels").join(game.platform.dir_name());
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join(format!("{}.png", cart.stem)), png))
        .is_ok()
}

/// What a sync came to, said for the page.
fn said(s: Synced) -> String {
    let mut out = match (s.sent, s.fetched) {
        (0, 0) => "Saves in step with RomM".to_string(),
        (sent, fetched) => format!("Saves in step with RomM: {sent} sent, {fetched} fetched"),
    };
    if s.failed > 0 {
        out += &format!(", {} could not be synced", s.failed);
    }
    out
}

/// Fetches what RomM was asked to send to the device, so it is on the shelf.
fn poll(http: &mut dyn Http, root: &Path, announced: &mut String, set: &Set<'_>) {
    let conf = read(root);
    if !conf.signed_in() || conf.device.is_empty() {
        return;
    }
    if *announced != conf.device {
        if let Err(e) = announce(http, &conf) {
            eprintln!("slot: romm: announce: {e}");
            return;
        }
        *announced = conf.device.clone();
    }
    match installs(http, &conf, root) {
        Ok(got) if got.is_empty() => {}
        Ok(got) => set(&mut |v| {
            v.got += 1;
            v.note = Some(match got.as_slice() {
                [one] => format!("{one} came from RomM: it is on the shelf"),
                many => format!("{} games came from RomM: they are on the shelf", many.len()),
            });
        }),
        Err(e) => eprintln!("slot: romm: installs: {e}"),
    }
}

/// Changes what the menu sees of the worker.
type Set<'a> = dyn Fn(&mut dyn FnMut(&mut RommView)) + 'a;

/// Waits on the player to approve the device's sign in, then keeps its token.
fn sign_in(
    http: &mut dyn Http,
    root: &Path,
    server: &str,
    name: &str,
    stop: &AtomicBool,
    set: &Set<'_>,
) -> Result<(), String> {
    let pairing = pair(http, server, name)?;
    let (interval, until) = (pairing.interval, Instant::now() + pairing.expires);
    set(&mut |v| {
        v.busy = None;
        v.pairing = Some(pairing.clone());
    });
    let approved = loop {
        // A short sleep at a time, so leaving the page is felt at once.
        let next = Instant::now() + interval;
        while Instant::now() < next {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if Instant::now() > until {
            return Err("The code ran out: sign in again".into());
        }
        if let Some(t) = token(http, server, &pairing.device_code)? {
            break t;
        }
    };
    let user = whoami(http, server, &approved.token).unwrap_or_default();
    let was = read(root);
    let conf = Conf {
        server: server.to_string(),
        token: approved.token,
        user: user.clone(),
        device: approved.device,
        art: was.art,
        saves: was.saves,
    };
    write(root, &conf).map_err(|e| format!("Could not save the sign in: {e}"))?;
    set(&mut |v| {
        v.signed += 1;
        v.shelves = None;
        v.games = None;
        v.note = Some(match user.is_empty() {
            true => "Signed in to RomM".into(),
            false => format!("Signed in to RomM as {user}"),
        });
    });
    Ok(())
}

/// Signs the device out, keeping the server and the choices of art and of saves.
pub fn sign_out(root: &Path) -> std::io::Result<()> {
    let conf = read(root);
    write(
        root,
        &Conf {
            token: String::new(),
            user: String::new(),
            device: String::new(),
            ..conf
        },
    )
}
