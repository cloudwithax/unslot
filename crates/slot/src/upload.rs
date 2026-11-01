//! A page a phone on the same Wi-Fi opens, from the code on the Upload page, to send the card a
//! wallpaper or a game. Up only while the Upload page is open.

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

/// A file that has come in whole.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Landed {
    /// In `wallpaper::OWN`, by its name.
    Wallpaper(String),
    /// In the system's games folder, by its name.
    Game(Platform, String),
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
        .map(|p| format!("<option value=\"{}\">{}</option>", p.dir_name(), p.name()))
        .collect();
    format!(
        r#"<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1">
<title>unslot</title><style>
body{{font:17px system-ui,sans-serif;max-width:32em;margin:2em auto;padding:0 1em;background:#111;color:#eee}}
select,input,button{{font:inherit;width:100%;margin:.4em 0;padding:.6em;box-sizing:border-box}}
#log div{{margin:.3em 0}}</style></head><body>
<h2>Send to unslot</h2>
<label>As<select id="to"><option value="{WALLPAPER}">Wallpaper</option>
<optgroup label="Game for">{systems}</optgroup></select></label>
<input id="files" type="file" multiple>
<button id="go">Send</button><div id="log"></div>
<script>
const log=t=>{{const d=document.createElement('div');d.textContent=t;document.getElementById('log').prepend(d);return d}};
document.getElementById('go').onclick=async()=>{{
 const to=document.getElementById('to').value;
 for(const f of document.getElementById('files').files){{
  const line=log(f.name+': sending…');
  try{{const r=await fetch('/upload?to='+encodeURIComponent(to)+'&name='+encodeURIComponent(f.name),{{method:'PUT',body:f}});
   line.textContent=f.name+': '+await r.text()}}catch(e){{line.textContent=f.name+': '+e}}
 }}
}};
</script></body></html>"#
    )
}
