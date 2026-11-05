//! A page a phone on the same Wi-Fi opens, from the code on the Upload page, to send the card a
//! game, a bezel or a wallpaper. Up only while the Upload page is open.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use slot_store::Platform;

/// The port asked for first; any free one if something else has it.
pub const PORT: u16 = 8080;
const WALLPAPER: &str = "wallpaper";
const WALLPAPER_MAX: u64 = 64 << 20;
/// `to` for a bezel: this, then the system's folder, as `bezel:GBA`.
const BEZEL: &str = "bezel:";
const BEZEL_MAX: u64 = 32 << 20;

/// A file that has come in whole.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Landed {
    /// In `wallpaper::OWN`, by its name.
    Wallpaper(String),
    /// In the system's games folder, by its name.
    Game(Platform, String),
    /// In the system's folder in `bezel::GAME_BEZELS`, by its name: a PNG, or the `.info`
    /// beside one that says where its window is.
    Bezel(Platform, String),
}

pub struct Upload {
    pub port: u16,
    stop: Arc<AtomicBool>,
    landed: Receiver<Landed>,
}

impl Upload {
    pub fn start(root: &Path) -> std::io::Result<Upload> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT))
            .or_else(|_| TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, landed) = channel();
        let (root, halt) = (root.to_path_buf(), stop.clone());
        std::thread::Builder::new()
            .name("slot-upload".into())
            .spawn(move || serve(listener, &root, &halt, &tx))?;
        Ok(Upload { port, stop, landed })
    }

    /// What has come in since the last look.
    pub fn landed(&self) -> Vec<Landed> {
        self.landed.try_iter().collect()
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(listener: TcpListener, root: &Path, stop: &AtomicBool, tx: &Sender<Landed>) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(e) = handle(stream, root, tx) {
                    eprintln!("slot: upload: {e}");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("slot: upload: {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn handle(stream: TcpStream, root: &Path, tx: &Sender<Landed>) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut out = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((key, value)) = header.split_once(':') {
            if key.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<u64>().ok();
            }
        }
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match (method, path) {
        ("GET", "/") => reply(&mut out, "200 OK", "text/html; charset=utf-8", &page()),
        ("PUT", "/upload") | ("POST", "/upload") => {
            let Some(length) = length else {
                return reply(&mut out, "411 Length Required", "text/plain", "No length");
            };
            let to = param(query, "to").unwrap_or_default();
            let name = param(query, "name").unwrap_or_default();
            match place(root, &to, &name, length) {
                Ok((dest, landed)) => {
                    let mut body = reader.take(length);
                    match save(&mut body, &dest, length) {
                        Ok(()) => {
                            let _ = tx.send(landed);
                            reply(&mut out, "200 OK", "text/plain", "Saved")
                        }
                        Err(e) => reply(&mut out, "500 Internal Server Error", "text/plain", &e),
                    }
                }
                Err(why) => reply(&mut out, "400 Bad Request", "text/plain", why),
            }
        }
        _ => reply(&mut out, "404 Not Found", "text/plain", "Not here"),
    }
}

/// Where a file sent `to` a wallpaper or a system's dir, called `name`, goes, or why it cannot.
pub fn place(
    root: &Path,
    to: &str,
    name: &str,
    length: u64,
) -> Result<(PathBuf, Landed), &'static str> {
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) || name.contains("..")
    {
        return Err("That name will not do");
    }
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if to == WALLPAPER {
        if !["png", "jpg", "jpeg"]
            .iter()
            .any(|x| ext.eq_ignore_ascii_case(x))
        {
            return Err("A wallpaper is a PNG or a JPEG");
        }
        if length > WALLPAPER_MAX {
            return Err("That picture is too big");
        }
        let dest = root.join(crate::wallpaper::OWN).join(name);
        return Ok((dest, Landed::Wallpaper(name.to_string())));
    }
    if let Some(system) = to.strip_prefix(BEZEL) {
        let platform = Platform::from_key(system).ok_or("No such system")?;
        if !["png", "info", "cfg"]
            .iter()
            .any(|x| ext.eq_ignore_ascii_case(x))
        {
            return Err("A bezel is a PNG, with its .info, or a RetroArch overlay's .cfg");
        }
        if length > BEZEL_MAX {
            return Err("That bezel is too big");
        }
        let dest = root
            .join(crate::bezel::GAME_BEZELS)
            .join(platform.dir_name())
            .join(name);
        return Ok((dest, Landed::Bezel(platform, name.to_string())));
    }
    let platform = Platform::from_key(to).ok_or("No such system")?;
    if !platform.accepts(Path::new(name)) {
        return Err("That system does not take that kind of file");
    }
    let dest = root.join("Games").join(platform.dir_name()).join(name);
    Ok((dest, Landed::Game(platform, name.to_string())))
}

/// Writes the body beside `dest` and moves it into place once it is all there.
fn save(body: &mut impl Read, dest: &Path, length: u64) -> Result<(), String> {
    let dir = dest.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let got = std::fs::File::create(&part)
        .and_then(|mut f| std::io::copy(body, &mut f).and_then(|n| f.sync_all().map(|_| n)));
    match got {
        Ok(n) if n == length => std::fs::rename(&part, dest).map_err(|e| e.to_string()),
        Ok(_) => {
            let _ = std::fs::remove_file(&part);
            Err("The file did not all arrive".into())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e.to_string())
        }
    }
}

fn reply(out: &mut TcpStream, status: &str, kind: &str, body: &str) -> std::io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    out.flush()
}

fn param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| decode(v))
}

/// Undoes a URL's percent escapes, and `+` for a space.
pub fn decode(text: &str) -> String {
    let raw = text.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'%' if i + 2 < raw.len() => {
                let hex = std::str::from_utf8(&raw[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        bytes.push(b);
                        i += 3;
                    }
                    None => {
                        bytes.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                bytes.push(b' ');
                i += 1;
            }
            b => {
                bytes.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn page() -> String {
    let systems: String = Platform::ALL
        .iter()
        .filter(|p| **p != Platform::Apps)
        .map(|p| format!("<option value=\"{}\">{}</option>", p.dir_name(), p.name()))
        .collect();
    PAGE.replace("{BRAND}", BRAND)
        .replace("{SYSTEMS}", &systems)
        .replace("{WALLPAPER}", WALLPAPER)
        .replace("{BEZEL}", BEZEL)
}

/// The page, in the look of unslot.clxud.dev: its colours, its type and its rounded tabs.
/// Everything is inline, as the phone may have no way out to the internet.
const PAGE: &str = include_str!("upload.html");
const BRAND: &str = include_str!("../../../assets/brand/unslot-dark.svg");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bezel_goes_in_its_systems_folder() {
        let root = Path::new("/card");
        let (dest, landed) = place(root, "bezel:GBA", "Frame.png", 1000).unwrap();
        assert_eq!(dest, root.join("Bezels/GBA/Frame.png"));
        assert_eq!(landed, Landed::Bezel(Platform::Gba, "Frame.png".into()));
        let (dest, _) = place(root, "bezel:GBA", "Frame.info", 100).unwrap();
        assert_eq!(dest, root.join("Bezels/GBA/Frame.info"));
        let (dest, _) = place(root, "bezel:GBA", "Overlay.cfg", 100).unwrap();
        assert_eq!(dest, root.join("Bezels/GBA/Overlay.cfg"));
    }

    #[test]
    fn a_bezel_must_be_a_png_for_a_real_system_and_not_too_big() {
        let root = Path::new("/card");
        assert!(place(root, "bezel:GBA", "frame.jpg", 10).is_err());
        assert!(place(root, "bezel:NOPE", "frame.png", 10).is_err());
        assert!(place(root, "bezel:GBA", "frame.png", BEZEL_MAX + 1).is_err());
        assert!(place(root, "bezel:GBA", "../frame.png", 10).is_err());
        assert!(place(root, "bezel:GBA", ".frame.png", 10).is_err());
    }

    #[test]
    fn wallpapers_and_games_still_land_where_they_did() {
        let root = Path::new("/card");
        let (dest, _) = place(root, WALLPAPER, "sky.jpg", 10).unwrap();
        assert_eq!(dest, root.join(crate::wallpaper::OWN).join("sky.jpg"));
        let (dest, landed) = place(root, "GBA", "game.gba", 10).unwrap();
        assert_eq!(dest, root.join("Games/GBA/game.gba"));
        assert_eq!(landed, Landed::Game(Platform::Gba, "game.gba".into()));
    }

    #[test]
    fn the_page_offers_every_system_and_every_kind() {
        let page = page();
        assert!(!page.contains("{SYSTEMS}") && !page.contains("{BEZEL}"));
        assert!(page.contains("value=\"GBA\""));
        assert!(page.contains(BEZEL) && page.contains(WALLPAPER));
        // Nothing is fetched from elsewhere.
        assert!(!page.contains("https://"));
    }

    #[test]
    fn a_bezel_sent_over_http_lands_whole() {
        let d = tempfile::tempdir().unwrap();
        let up = Upload::start(d.path()).unwrap();
        let png = b"\x89PNG fake bezel bytes";
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, up.port)).unwrap();
        write!(
            s,
            "PUT /upload?to=bezel%3AGBA&name=My%20Frame.png HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            png.len()
        )
        .unwrap();
        s.write_all(png).unwrap();
        let mut answer = String::new();
        s.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        let got = std::fs::read(d.path().join("Bezels/GBA/My Frame.png")).unwrap();
        assert_eq!(got, png);
        let mut landed = Vec::new();
        for _ in 0..50 {
            landed.extend(up.landed());
            if !landed.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            landed,
            [Landed::Bezel(Platform::Gba, "My Frame.png".into())]
        );
    }
}
