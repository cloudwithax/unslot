//! Labels for the card's games, found online and drawn into `Labels/<system>/<game>.png`, where
//! the shelf already looks for them.
//!
//! A game is known by its CRC32 and by its file name, both, for coverage: the CRC finds a dump
//! however it is named, the name finds one the database does not list. libretro's databases give
//! the names, and libretro-thumbnails the logos and box art, for every system and with no account.
//! ScreenScraper is asked first when the card has `Config/screenscraper.conf`, the way ES-DE asks
//! it: the CRC, size and file name together, so it can match on any of them. It needs a developer
//! id of unslot's own, so it is off until one is there.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use slot_store::{Cart, Platform};

use crate::json;

/// The databases fetched once and kept, under the card's System folder with the label cache.
pub const CACHE: &str = "System/Cache/Scraper";
pub const SS_CONF: &str = "Config/screenscraper.conf";
const DATS: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat";
const THUMBS: &str = "https://thumbnails.libretro.com";
const SS_API: &str = "https://api.screenscraper.fr/api2/jeuInfos.php";
/// Bigger than any cartridge; a disc is matched by name instead of read whole.
const CRC_MAX: u64 = 64 << 20;
/// How many names a game is looked for under before it is given up on.
const NAMES: usize = 4;

/// libretro's sets for a system's games, the one to try first first, and ScreenScraper's id for
/// it, as Taizi has them.
pub fn source(platform: Platform, rom: &Path) -> (Vec<&'static str>, u32) {
    let ext = rom
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match platform {
        Platform::Gba => (vec!["Nintendo - Game Boy Advance"], 12),
        Platform::Gb => (vec!["Nintendo - Game Boy", "Nintendo - Game Boy Color"], 9),
        Platform::Gbc => (vec!["Nintendo - Game Boy Color", "Nintendo - Game Boy"], 10),
        Platform::Nes => (vec!["Nintendo - Nintendo Entertainment System"], 3),
        Platform::Snes => (vec!["Nintendo - Super Nintendo Entertainment System"], 4),
        Platform::Genesis => (vec!["Sega - Mega Drive - Genesis"], 1),
        Platform::MasterSystem => (vec!["Sega - Master System - Mark III"], 2),
        Platform::GameGear => (vec!["Sega - Game Gear"], 21),
        Platform::PcEngine => (vec!["NEC - PC Engine - TurboGrafx 16"], 31),
        Platform::NeoGeoPocket if ext == "ngp" => (
            vec!["SNK - Neo Geo Pocket", "SNK - Neo Geo Pocket Color"],
            25,
        ),
        Platform::NeoGeoPocket => (
            vec!["SNK - Neo Geo Pocket Color", "SNK - Neo Geo Pocket"],
            82,
        ),
        Platform::WonderSwan if ext == "ws" => {
            (vec!["Bandai - WonderSwan", "Bandai - WonderSwan Color"], 45)
        }
        Platform::WonderSwan => (vec!["Bandai - WonderSwan Color", "Bandai - WonderSwan"], 46),
        Platform::Lynx => (vec!["Atari - Lynx"], 28),
        Platform::PlayStation => (vec!["Sony - PlayStation"], 57),
    }
}

fn dat_url(set: &str) -> String {
    let dir = if set == "Sony - PlayStation" {
        "redump"
    } else {
        "no-intro"
    };
    format!("{DATS}/{dir}/{}.dat", encode(set))
}

/// libretro-thumbnails names a picture after the game, with what its filesystem forbids made `_`.
pub fn thumbnail_name(game: &str) -> String {
    game.chars()
        .map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c })
        .collect()
}

fn thumb_url(set: &str, kind: &str, game: &str) -> String {
    format!(
        "{THUMBS}/{}/{kind}/{}.png",
        encode(set),
        encode(&thumbnail_name(game))
    )
}

/// Percent-encoding for a URL path segment.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A libretro database: which game each CRC32 is, and every game's name.
#[derive(Default, Debug)]
pub struct Dat {
    by_crc: HashMap<u32, String>,
    by_title: HashMap<String, Vec<String>>,
    names: std::collections::HashSet<String>,
}

/// Reads a clrmamepro dat as libretro writes them: `game (` blocks with a `name` and `rom` lines.
pub fn parse_dat(text: &str) -> Dat {
    let mut dat = Dat::default();
    let mut game: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with("game (") {
            game = None;
        } else if let Some(rest) = line.strip_prefix("name \"") {
            let name = rest.trim_end_matches('"').to_string();
            if dat.names.insert(name.to_lowercase()) {
                dat.by_title
                    .entry(title_key(&name))
                    .or_default()
                    .push(name.clone());
            }
            game = Some(name);
        } else if line.starts_with("rom (") {
            let crc = line
                .split_whitespace()
                .skip_while(|w| *w != "crc")
                .nth(1)
                .and_then(|c| u32::from_str_radix(c, 16).ok());
            if let (Some(crc), Some(name)) = (crc, &game) {
                dat.by_crc.entry(crc).or_insert_with(|| name.clone());
            }
        }
    }
    dat
}

/// A name with its tags and punctuation gone, so `Pokemon - Emerald Version (USA, Europe)` and
/// `pokemon_emerald_version` meet.
pub fn title_key(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            c if depth == 0 && c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ => {}
        }
    }
    out
}

/// How much a dump's tags are wanted: a release before a beta, the common regions first.
fn rank(name: &str) -> usize {
    let lower = name.to_lowercase();
    let region = ["(usa", "(world", "(europe", "(japan"]
        .iter()
        .position(|r| lower.contains(r))
        .unwrap_or(4);
    let unfinished = ["(beta", "(proto", "(demo", "(sample", "(pirate", "(hack"]
        .iter()
        .any(|t| lower.contains(t));
    region + usize::from(unfinished) * 10
}

impl Dat {
    /// The names a game might be listed under, best first: its CRC's, its file name as it is,
    /// then the games that share its title once tags are set aside.
    pub fn names_for(&self, crc: Option<u32>, stem: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |n: &str| {
            if !out.iter().any(|o| o == n) {
                out.push(n.to_string());
            }
        };
        if let Some(name) = crc.and_then(|c| self.by_crc.get(&c)) {
            add(name);
        }
        if self.names.contains(&stem.to_lowercase()) {
            add(stem);
        }
        let mut alike = self
            .by_title
            .get(&title_key(stem))
            .cloned()
            .unwrap_or_default();
        alike.sort_by_key(|n| rank(n));
        for name in alike.iter().take(NAMES) {
            add(name);
        }
        out
    }
}

/// The CRC32 of a cartridge, as the databases list it: of the whole file.
pub fn crc_of(rom: &Path) -> Option<u32> {
    let mut f = std::fs::File::open(rom).ok()?;
    if f.metadata().ok()?.len() > CRC_MAX {
        return None;
    }
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hasher.finalize())
}

fn is_disc(rom: &Path) -> bool {
    let ext = rom.extension().and_then(|e| e.to_str()).unwrap_or_default();
    ["cue", "chd", "pbp", "m3u", "iso"]
        .iter()
        .any(|d| ext.eq_ignore_ascii_case(d))
}

/// A ScreenScraper login: unslot's developer id, which every call needs, and optionally the
/// player's own account, which raises their quota.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SsLogin {
    pub devid: String,
    pub devpassword: String,
    pub softname: String,
    pub ssid: Option<String>,
    pub sspassword: Option<String>,
}

/// `Config/screenscraper.conf`, `key=value` lines. None until a developer id is in it.
pub fn read_ss_conf(root: &Path) -> Option<SsLogin> {
    let text = std::fs::read_to_string(root.join(SS_CONF)).ok()?;
    let get = |key: &str| {
        text.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == key))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    Some(SsLogin {
        devid: get("devid")?,
        devpassword: get("devpassword")?,
        softname: get("softname").unwrap_or_else(|| "unslot".into()),
        ssid: get("ssid"),
        sspassword: get("sspassword"),
    })
}

/// The `jeuInfos` call ES-DE makes: the CRC, size and file name together, so ScreenScraper can
/// match on whichever it knows.
pub fn ss_url(login: &SsLogin, system: u32, crc: Option<u32>, size: u64, file: &str) -> String {
    let mut q = vec![
        ("devid", login.devid.clone()),
        ("devpassword", login.devpassword.clone()),
        ("softname", login.softname.clone()),
        ("output", "json".into()),
        ("systemeid", system.to_string()),
        ("romtype", "rom".into()),
        ("romnom", file.to_string()),
        ("romtaille", size.to_string()),
    ];
    if let Some(crc) = crc {
        q.push(("crc", format!("{crc:08X}")));
    }
    if let (Some(id), Some(pw)) = (&login.ssid, &login.sspassword) {
        q.push(("ssid", id.clone()));
        q.push(("sspassword", pw.clone()));
    }
    let query: Vec<String> = q
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect();
    format!("{SS_API}?{}", query.join("&"))
}

/// The URL of a game's media of `kind` (`wheel` for its logo, `box-2D` for its box) in a
/// `jeuInfos` answer, the world's or the common regions' first.
pub fn ss_media(text: &str, kind: &str) -> Option<String> {
    let medias = &text[text.find("\"medias\"")?..];
    let found: Vec<(String, String)> = medias
        .split('}')
        .filter(|m| json::string(m, "type").as_deref() == Some(kind))
        .filter_map(|m| {
            Some((
                json::string(m, "region").unwrap_or_default(),
                json::string(m, "url")?,
            ))
        })
        .collect();
    ["wor", "us", "eu", "ss", "jp", ""]
        .iter()
        .find_map(|r| found.iter().find(|(region, _)| region == r))
        .or(found.first())
        .map(|(_, url)| url.clone())
}

/// Fetching, as the worker does it.
pub trait Fetch: Send {
    /// What is at `url`: `Ok(None)` when there is nothing there, `Err` when the network failed.
    fn get(&mut self, url: &str) -> Result<Option<Vec<u8>>, String>;
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ScrapeView {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub current: Option<String>,
    /// The games given a label since slot started, which the shelf shows once it restarts.
    pub made: Vec<(Platform, String)>,
    /// Games looked for and not found.
    pub missed: usize,
    pub error: Option<String>,
}

/// The handle the app keeps.
pub struct Scraper {
    jobs: Sender<Vec<Cart>>,
    view: Arc<Mutex<ScrapeView>>,
    pub screenscraper: bool,
}

impl Scraper {
    pub fn spawn(fetch: Box<dyn Fetch>, root: &Path) -> Scraper {
        let (jobs, rx) = channel();
        let view = Arc::new(Mutex::new(ScrapeView::default()));
        let shared = view.clone();
        let root = root.to_path_buf();
        let screenscraper = read_ss_conf(&root).is_some();
        let spawned = std::thread::Builder::new()
            .name("slot-scraper".into())
            .spawn(move || work(fetch, root, rx, shared));
        if let Err(e) = spawned {
            eprintln!("slot: scraper: {e}");
        }
        Scraper {
            jobs,
            view,
            screenscraper,
        }
    }

    /// The scraper for this build: the internet on the device, pretend art anywhere else.
    pub fn for_card(root: &Path) -> Scraper {
        #[cfg(all(feature = "device", unix))]
        let fetch: Box<dyn Fetch> = Box::new(Curl);
        #[cfg(not(all(feature = "device", unix)))]
        let fetch: Box<dyn Fetch> = Box::new(SimFetch);
        Scraper::spawn(fetch, root)
    }

    /// Finds labels for `carts`.
    pub fn fetch(&self, carts: Vec<Cart>) {
        let _ = self.jobs.send(carts);
    }

    pub fn view(&self) -> ScrapeView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn work(
    mut fetch: Box<dyn Fetch>,
    root: PathBuf,
    jobs: Receiver<Vec<Cart>>,
    view: Arc<Mutex<ScrapeView>>,
) {
    let set =
        |f: &mut dyn FnMut(&mut ScrapeView)| f(&mut view.lock().unwrap_or_else(|e| e.into_inner()));
    let mut dats: HashMap<&'static str, Dat> = HashMap::new();
    let login = read_ss_conf(&root);
    while let Ok(carts) = jobs.recv() {
        let total = carts.len();
        set(&mut |v| {
            v.running = true;
            v.done = 0;
            v.total = total;
            v.error = None;
        });
        for cart in carts {
            set(&mut |v| v.current = Some(cart.stem.clone()));
            match label_for(&mut *fetch, &root, &mut dats, login.as_ref(), &cart) {
                Ok(Some(png)) => {
                    let dir = root.join("Labels").join(cart.platform.dir_name());
                    let path = dir.join(format!("{}.png", cart.stem));
                    let written =
                        std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, png));
                    match written {
                        Ok(()) => set(&mut |v| v.made.push((cart.platform, cart.stem.clone()))),
                        Err(e) => set(&mut |v| v.error = Some(format!("{}: {e}", path.display()))),
                    }
                }
                Ok(None) => set(&mut |v| v.missed += 1),
                Err(e) => {
                    eprintln!("slot: scraper: {e}");
                    set(&mut |v| v.error = Some(e.clone()));
                    break;
                }
            }
            set(&mut |v| v.done += 1);
        }
        set(&mut |v| {
            v.running = false;
            v.current = None;
        });
    }
}

/// A label for one game, or None when no art for it turned up.
fn label_for(
    fetch: &mut dyn Fetch,
    root: &Path,
    dats: &mut HashMap<&'static str, Dat>,
    login: Option<&SsLogin>,
    cart: &Cart,
) -> Result<Option<Vec<u8>>, String> {
    let (sets, system) = source(cart.platform, &cart.rom);
    let crc = (!is_disc(&cart.rom)).then(|| crc_of(&cart.rom)).flatten();
    let png = |bytes: Option<Vec<u8>>| bytes.filter(|b| b.starts_with(b"\x89PNG"));
    let (mut logo, mut boxart) = (None, None);

    if let Some(login) = login {
        let size = std::fs::metadata(&cart.rom).map(|m| m.len()).unwrap_or(0);
        let file = cart
            .rom
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&cart.stem);
        if let Some(answer) = fetch.get(&ss_url(login, system, crc, size, file))? {
            let answer = String::from_utf8_lossy(&answer);
            if let Some(url) = ss_media(&answer, "wheel") {
                logo = png(fetch.get(&url)?);
            }
            if let Some(url) = ss_media(&answer, "box-2D") {
                boxart = png(fetch.get(&url)?);
            }
        }
    }

    for set in sets {
        if logo.is_some() {
            break;
        }
        let dat = load_dat(fetch, root, dats, set)?;
        let mut names = dat.names_for(crc, &cart.stem);
        if !names.iter().any(|n| n == &cart.stem) {
            names.push(cart.stem.clone());
        }
        for name in names.iter().take(NAMES + 2) {
            logo = png(fetch.get(&thumb_url(set, "Named_Logos", name))?);
            if boxart.is_none() {
                boxart = png(fetch.get(&thumb_url(set, "Named_Boxarts", name))?);
            }
            if logo.is_some() {
                break;
            }
        }
    }

    let (w, h) = slot_ui::label_size(cart);
    let hue = slot_ui::hue_of(slot_ui::label_colour(&slot_ui::label_text(cart)));
    Ok(slot_ui::scraped_label(
        logo.as_deref(),
        boxart.as_deref(),
        hue,
        w * 2,
        h * 2,
    ))
}

/// A set's database, from memory, the card, or libretro, in that order.
fn load_dat<'a>(
    fetch: &mut dyn Fetch,
    root: &Path,
    dats: &'a mut HashMap<&'static str, Dat>,
    set: &'static str,
) -> Result<&'a Dat, String> {
    if !dats.contains_key(set) {
        let path = root.join(CACHE).join(format!("{set}.dat"));
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => {
                let bytes = fetch.get(&dat_url(set))?.unwrap_or_default();
                if !bytes.is_empty() {
                    let _ = std::fs::create_dir_all(root.join(CACHE));
                    let _ = std::fs::write(&path, &bytes);
                }
                String::from_utf8_lossy(&bytes).into_owned()
            }
        };
        dats.insert(set, parse_dat(&text));
    }
    Ok(&dats[set])
}

/// The internet, through BaseOS's curl.
#[cfg(all(feature = "device", unix))]
struct Curl;

#[cfg(all(feature = "device", unix))]
impl Fetch for Curl {
    fn get(&mut self, url: &str) -> Result<Option<Vec<u8>>, String> {
        let tmp = std::env::temp_dir().join(format!("slot-scrape-{}", std::process::id()));
        let out = std::process::Command::new("curl")
            .args([
                "-sSL",
                "--connect-timeout",
                "15",
                "--max-time",
                "90",
                "-w",
                "%{http_code}",
                "-o",
            ])
            .arg(&tmp)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("curl: {e}"))?;
        let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let body = std::fs::read(&tmp).unwrap_or_default();
        let _ = std::fs::remove_file(&tmp);
        match code.as_str() {
            "200" => Ok(Some(body)),
            // ScreenScraper says it does not know a game with a 404 or a 400.
            "404" | "400" | "430" => Ok(None),
            _ if !out.status.success() => Err("The art servers could not be reached".into()),
            other => Err(format!("The art server answered {other}")),
        }
    }
}

/// Pretend art for the desktop and the tests: a libretro database knowing one game per system by
/// the CRC of a ROM of eight zero bytes, and a logo and box for each game it knows.
pub struct SimFetch;

impl SimFetch {
    pub const CRC_ZEROS8: u32 = 0x6522_DF69;

    pub fn logo() -> Vec<u8> {
        sim_png(32, 8, [255, 255, 255, 255])
    }
}

fn sim_png(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let data: Vec<u8> = (0..w * h).flat_map(|_| px).collect();
        if let Ok(mut writer) = enc.write_header() {
            let _ = writer.write_image_data(&data);
        }
    }
    out
}

impl Fetch for SimFetch {
    fn get(&mut self, url: &str) -> Result<Option<Vec<u8>>, String> {
        if url.ends_with(".dat") {
            return Ok(Some(
                format!(
                    "game (\n\tname \"Pretend Game (USA)\"\n\trom ( name \"Pretend Game (USA).bin\" size 8 crc {:08X} )\n)\n",
                    SimFetch::CRC_ZEROS8
                )
                .into_bytes(),
            ));
        }
        if url.contains("Pretend%20Game%20%28USA%29") {
            return Ok(Some(match url.contains("Named_Logos") {
                true => SimFetch::logo(),
                false => sim_png(16, 16, [200, 40, 40, 255]),
            }));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAT: &str = r#"clrmamepro (
	name "Nintendo - Game Boy Advance"
)

game (
	name "Pokemon - Emerald Version (USA, Europe)"
	rom ( name "Pokemon - Emerald Version (USA, Europe).gba" size 16777216 crc 1F1C08FB md5 X sha1 Y )
)
game (
	name "Pokemon - Emerald Version (Japan)"
	rom ( name "Pokemon - Emerald Version (Japan).gba" size 16777216 crc 4881F3F8 md5 X sha1 Y )
)
game (
	name "Pokemon - Emerald Version (USA) (Beta)"
	rom ( name "Pokemon - Emerald Version (USA) (Beta).gba" size 16777216 crc 0BADBEEF md5 X sha1 Y )
)
"#;

    #[test]
    fn a_crc_finds_its_game_however_the_file_is_named() {
        let dat = parse_dat(DAT);
        assert_eq!(
            dat.names_for(Some(0x4881_F3F8), "emerald hack final")[0],
            "Pokemon - Emerald Version (Japan)"
        );
    }

    #[test]
    fn a_name_finds_a_game_whose_crc_is_unknown_best_release_first() {
        let dat = parse_dat(DAT);
        let names = dat.names_for(Some(0x1234_5678), "pokemon_emerald_version");
        assert_eq!(
            names,
            [
                "Pokemon - Emerald Version (USA, Europe)",
                "Pokemon - Emerald Version (Japan)",
                "Pokemon - Emerald Version (USA) (Beta)",
            ]
        );
        assert_eq!(
            dat.names_for(None, "Pokemon - Emerald Version (Japan)")[0],
            "Pokemon - Emerald Version (Japan)",
            "an exact file name did not come first"
        );
    }

    #[test]
    fn thumbnails_are_named_and_addressed_as_libretro_has_them() {
        assert_eq!(
            thumbnail_name("Lilo & Stitch: Trouble?"),
            "Lilo _ Stitch_ Trouble_"
        );
        assert_eq!(
            thumb_url("Nintendo - Game Boy", "Named_Logos", "Tetris (World) (Rev 1)"),
            "https://thumbnails.libretro.com/Nintendo%20-%20Game%20Boy/Named_Logos/Tetris%20%28World%29%20%28Rev%201%29.png"
        );
        assert!(dat_url("Sony - PlayStation").contains("/redump/Sony%20-%20PlayStation.dat"));
        assert!(dat_url("Atari - Lynx").contains("/no-intro/"));
    }

    #[test]
    fn a_colour_rom_looks_in_the_colour_set_first() {
        assert_eq!(
            source(Platform::WonderSwan, Path::new("a.wsc")),
            (vec!["Bandai - WonderSwan Color", "Bandai - WonderSwan"], 46)
        );
        assert_eq!(source(Platform::WonderSwan, Path::new("a.ws")).1, 45);
        assert_eq!(source(Platform::NeoGeoPocket, Path::new("a.ngc")).1, 82);
    }

    #[test]
    fn screenscraper_is_asked_the_way_es_de_asks() {
        let login = SsLogin {
            devid: "dev".into(),
            devpassword: "pw".into(),
            softname: "unslot".into(),
            ssid: Some("me".into()),
            sspassword: Some("s3cret&".into()),
        };
        let url = ss_url(&login, 12, Some(0x1F1C_08FB), 16777216, "Emerald (USA).gba");
        assert!(url.starts_with(SS_API));
        for part in [
            "systemeid=12",
            "crc=1F1C08FB",
            "romtaille=16777216",
            "romnom=Emerald%20%28USA%29.gba",
            "romtype=rom",
            "ssid=me",
            "sspassword=s3cret%26",
            "softname=unslot",
        ] {
            assert!(url.contains(part), "{part} missing from {url}");
        }
    }

    #[test]
    fn screenscraper_media_prefers_the_worlds_then_the_usas() {
        let answer = r#"{"response":{"jeu":{"medias":[
            {"type":"box-2D","parent":"jeu","url":"https:\/\/ss\/box-jp.png","region":"jp","format":"png"},
            {"type":"wheel","parent":"jeu","url":"https:\/\/ss\/wheel-us.png","region":"us","format":"png"},
            {"type":"wheel","parent":"jeu","url":"https:\/\/ss\/wheel-wor.png","region":"wor","format":"png"}
        ]}}}"#;
        assert_eq!(
            ss_media(answer, "wheel").as_deref(),
            Some("https://ss/wheel-wor.png")
        );
        assert_eq!(
            ss_media(answer, "box-2D").as_deref(),
            Some("https://ss/box-jp.png")
        );
        assert_eq!(ss_media(answer, "screenmarquee"), None);
    }

    #[test]
    fn screenscraper_stays_off_without_a_developer_id() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read_ss_conf(d.path()), None);
        std::fs::create_dir_all(d.path().join("Config")).unwrap();
        std::fs::write(d.path().join(SS_CONF), "# mine\nssid=me\nsspassword=pw\n").unwrap();
        assert_eq!(
            read_ss_conf(d.path()),
            None,
            "a player login alone is not enough"
        );
        std::fs::write(
            d.path().join(SS_CONF),
            "devid=d\ndevpassword=p\nssid=me\nsspassword=pw\n",
        )
        .unwrap();
        let login = read_ss_conf(d.path()).unwrap();
        assert_eq!(
            (login.softname.as_str(), login.ssid.as_deref()),
            ("unslot", Some("me"))
        );
    }

    #[test]
    fn a_game_the_database_knows_gets_a_label_on_the_card() {
        let d = tempfile::tempdir().unwrap();
        let games = d.path().join("Games/GBA");
        std::fs::create_dir_all(&games).unwrap();
        std::fs::write(games.join("whatever.gba"), [0u8; 8]).unwrap();
        std::fs::write(games.join("unknown.gba"), [1u8; 8]).unwrap();
        let carts = slot_store::scan(d.path()).unwrap();
        let s = Scraper::spawn(Box::new(SimFetch), d.path());
        s.fetch(carts);
        let start = std::time::Instant::now();
        let v = loop {
            let v = s.view();
            if (!v.running && v.done == 2) || start.elapsed().as_secs() > 10 {
                break v;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(v.made, [(Platform::Gba, "whatever".to_string())]);
        assert_eq!(v.missed, 1);
        let label = d.path().join("Labels/GBA/whatever.png");
        assert!(std::fs::read(&label).unwrap().starts_with(b"\x89PNG"));
        assert!(
            d.path()
                .join(CACHE)
                .join("Nintendo - Game Boy Advance.dat")
                .exists(),
            "the database was not kept"
        );
    }
}
