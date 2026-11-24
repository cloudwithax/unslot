//! The runtimes a PortMaster port needs that the card has not got: Godot, Mono, .NET and the
//! like, squashfs images the port's script mounts from PortMaster's libs folder. PortMaster
//! downloads them from its releases when a port is installed; there are some 2 GB of them for
//! arm64, too many to put in every update, so slot does what PortMaster would. Before a port
//! runs, and when its script asks PortMaster's harbourmaster for one, the missing ones are looked
//! up in PortMaster's list, slot asks on screen whether to download them, and they go in
//! System/PortMaster/libs, checked against the list's md5.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use slot_input::{Action, Btn, Gestures, InputSource, Millis};

use crate::json;

/// Where the runtimes go, under the card's root: PortMaster's libs folder.
pub const LIBS: &str = "System/PortMaster/libs";
/// PortMaster's list of its ports and runtimes, from its latest release.
const LIST: &str =
    "https://github.com/PortsMaster/PortMaster-New/releases/latest/download/ports.json";
/// Room kept free on the card past the downloads, for what the port writes.
const SPARE: u64 = 32 << 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Runtime {
    pub file: String,
    pub name: String,
    pub size: u64,
    pub md5: String,
    pub url: String,
}

/// The file a runtime is kept as; ports name some with `.squashfs` and some without.
pub fn file_name(runtime: &str) -> String {
    match runtime.ends_with(".squashfs") {
        true => runtime.to_string(),
        false => format!("{runtime}.squashfs"),
    }
}

/// The runtimes `script`'s port says it needs, in the port.json of the folder in `ports` whose
/// items name the script, as PortMaster installs them.
pub fn wanted(ports: &Path, script: &Path) -> Vec<String> {
    let Some(name) = script.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    let Ok(dirs) = std::fs::read_dir(ports) else {
        return Vec::new();
    };
    for dir in dirs.flatten() {
        let Ok(text) = std::fs::read_to_string(dir.path().join("port.json")) else {
            continue;
        };
        if json::strings(&text, "items").iter().any(|i| i == name) {
            // A list, or in older ports a single name.
            let mut runtimes = json::strings(&text, "runtime");
            if runtimes.is_empty() {
                runtimes.extend(json::string(&text, "runtime"));
            }
            return runtimes.iter().map(|r| file_name(r)).collect();
        }
    }
    Vec::new()
}

/// `file` as `list`, PortMaster's ports.json, has it among its runtimes.
pub fn listed(list: &str, file: &str) -> Option<Runtime> {
    let key = format!("\"{file}\"");
    let mut from = 0;
    while let Some(at) = list[from..].find(&key) {
        from += at + key.len();
        // A port's list of the runtimes it needs names it too, but only the runtime's own entry
        // is followed by an object.
        let Some(body) = list[from..]
            .trim_start()
            .strip_prefix(':')
            .and_then(|r| r.trim_start().strip_prefix('{'))
        else {
            continue;
        };
        let body = &body[..body.find('}')?];
        return Some(Runtime {
            file: file.to_string(),
            name: json::string(body, "name")?,
            size: json::number(body, "size")?,
            md5: json::string(body, "md5")?,
            url: json::string(body, "url")?,
        });
    }
    None
}

/// The runtimes named that are not in `libs` yet.
pub fn missing(libs: &Path, files: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in files {
        if !libs.join(f).is_file() && !out.contains(f) {
            out.push(f.clone());
        }
    }
    out
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

fn names(runtimes: &[Runtime]) -> String {
    let names: Vec<&str> = runtimes.iter().map(|r| r.name.as_str()).collect();
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

pub(crate) fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut fs: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(path.as_ptr(), &mut fs) } == 0)
        .then(|| fs.f_bavail as u64 * fs.f_frsize as u64)
}

fn fetch_list() -> mpsc::Receiver<Option<String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let out = Command::new("curl")
            .args([
                "-fsSL",
                "--connect-timeout",
                "10",
                "--max-time",
                "120",
                LIST,
            ])
            .stderr(Stdio::inherit())
            .output();
        let list = match out {
            Ok(o) if o.status.success() => String::from_utf8(o.stdout).ok(),
            Ok(o) => {
                eprintln!("slot: port runtime: PortMaster's list: curl {}", o.status);
                None
            }
            Err(e) => {
                eprintln!("slot: port runtime: curl: {e}");
                None
            }
        };
        let _ = tx.send(list);
    });
    rx
}

fn md5_of(file: &Path) -> Option<String> {
    let out = Command::new("md5sum").arg(file).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    Some(text.split_whitespace().next()?.to_ascii_lowercase())
}

enum Screen {
    Looking(mpsc::Receiver<Option<String>>),
    Ask(Vec<Runtime>),
    Fetching {
        runtimes: Vec<Runtime>,
        index: usize,
        curl: Child,
        part: PathBuf,
    },
    Failed {
        title: String,
        line: String,
        retry: bool,
    },
}

/// What the screen says: its title, the line under it, how far the downloads are, if they are
/// going, and its buttons.
struct Words {
    title: String,
    line: String,
    fraction: Option<f32>,
    legend: Vec<(&'static str, &'static str)>,
}

fn words(screen: &Screen) -> Words {
    let (title, line, fraction, legend) = match screen {
        Screen::Looking(_) => (
            "Finding what this port needs".into(),
            "Asking PortMaster".into(),
            None,
            vec![("B", "Cancel")],
        ),
        Screen::Ask(runtimes) => (
            format!("This port needs {}", names(runtimes)),
            format!(
                "Download {} from PortMaster?",
                megabytes(runtimes.iter().map(|r| r.size).sum())
            ),
            None,
            vec![("B", "Cancel"), ("A", "Download")],
        ),
        Screen::Fetching {
            runtimes,
            index,
            part,
            ..
        } => {
            let total: u64 = runtimes.iter().map(|r| r.size).sum();
            let done: u64 = runtimes[..*index].iter().map(|r| r.size).sum::<u64>()
                + std::fs::metadata(part).map_or(0, |m| m.len());
            let done = done.min(total);
            (
                format!("Downloading {}", runtimes[*index].name),
                format!("{} of {}", megabytes(done), megabytes(total)),
                Some(done as f32 / total.max(1) as f32),
                vec![("B", "Cancel")],
            )
        }
        Screen::Failed { title, line, retry } => (
            title.clone(),
            line.clone(),
            None,
            match retry {
                true => vec![("B", "Cancel"), ("A", "Try Again")],
                false => vec![("B", "Close")],
            },
        ),
    };
    Words {
        title,
        line,
        fraction,
        legend,
    }
}

fn start(libs: &Path, runtimes: Vec<Runtime>, index: usize) -> Screen {
    let rt = &runtimes[index];
    let part = libs.join(format!("{}.part", rt.file));
    let curl = Command::new("curl")
        .args(["-fsSL", "--connect-timeout", "15", "--speed-limit", "1024"])
        .args(["--speed-time", "30", "-o"])
        .arg(&part)
        .arg(&rt.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn();
    match curl {
        Ok(curl) => Screen::Fetching {
            runtimes,
            index,
            curl,
            part,
        },
        Err(e) => {
            eprintln!("slot: port runtime: curl: {e}");
            Screen::Failed {
                title: format!("Couldn't download {}", rt.name),
                line: "curl did not start".into(),
                retry: true,
            }
        }
    }
}

/// Where the screen goes once PortMaster's list is in, or not.
fn looked_up(
    libs: &Path,
    files: &[String],
    list: Option<String>,
    strict: bool,
) -> Result<Screen, i32> {
    let Some(list) = list else {
        return Ok(Screen::Failed {
            title: "Couldn't reach PortMaster".into(),
            line: "Connect to Wi-Fi in Settings, then try again".into(),
            retry: true,
        });
    };
    let mut runtimes = Vec::new();
    for f in files {
        match listed(&list, f) {
            Some(rt) => runtimes.push(rt),
            None => {
                eprintln!("slot: port runtime: PortMaster does not list {f}");
                if strict {
                    return Ok(Screen::Failed {
                        title: format!("PortMaster has no {f}"),
                        line: "This port can't run without it".into(),
                        retry: false,
                    });
                }
            }
        }
    }
    // A port.json can name a runtime by a name PortMaster never had; its script, which asks for
    // the right one, is let run.
    if runtimes.is_empty() {
        return Err(0);
    }
    let need: u64 = runtimes.iter().map(|r| r.size).sum::<u64>() + SPARE;
    if let Some(free) = free_bytes(libs).filter(|&free| free < need) {
        return Ok(Screen::Failed {
            title: "Not enough room on the card".into(),
            line: format!(
                "This port needs {}; the card has {} free",
                megabytes(need),
                megabytes(free)
            ),
            retry: false,
        });
    }
    Ok(Screen::Ask(runtimes))
}

/// How the download in `screen` is going: the next screen, or the exit code once all are in.
fn advance(libs: &Path, screen: Screen) -> Result<Screen, i32> {
    let Screen::Fetching {
        runtimes,
        index,
        mut curl,
        part,
    } = screen
    else {
        return Ok(screen);
    };
    let status = match curl.try_wait() {
        Ok(None) => {
            return Ok(Screen::Fetching {
                runtimes,
                index,
                curl,
                part,
            })
        }
        Ok(Some(s)) => s,
        Err(e) => {
            eprintln!("slot: port runtime: curl: {e}");
            let _ = curl.kill();
            let _ = curl.wait();
            std::process::ExitStatus::default()
        }
    };
    let rt = &runtimes[index];
    let fail = |line: &str| {
        let _ = std::fs::remove_file(&part);
        Screen::Failed {
            title: format!("Couldn't download {}", rt.name),
            line: line.into(),
            retry: true,
        }
    };
    if !status.success() {
        eprintln!("slot: port runtime: {}: curl {status}", rt.url);
        return Ok(fail("The download stopped; check Wi-Fi and try again"));
    }
    if md5_of(&part).as_deref() != Some(rt.md5.to_ascii_lowercase().as_str()) {
        eprintln!("slot: port runtime: {} does not match its md5", rt.file);
        return Ok(fail("It didn't match PortMaster's checksum"));
    }
    if let Err(e) = std::fs::rename(&part, libs.join(&rt.file)) {
        eprintln!("slot: port runtime: {}: {e}", rt.file);
        return Ok(fail("It couldn't be put on the card"));
    }
    eprintln!("slot: port runtime: downloaded {}", rt.file);
    match index + 1 < runtimes.len() {
        true => Ok(start(libs, runtimes, index + 1)),
        false => Err(0),
    }
}

/// What a press does: the next screen, or the exit code to leave with.
fn press(libs: &Path, screen: Screen, action: Action) -> Result<Screen, i32> {
    match (screen, action) {
        (Screen::Ask(runtimes), Action::GbaDown(Btn::A)) => Ok(start(libs, runtimes, 0)),
        (Screen::Failed { retry: true, .. }, Action::GbaDown(Btn::A)) => {
            Ok(Screen::Looking(fetch_list()))
        }
        (Screen::Fetching { mut curl, part, .. }, Action::GbaDown(Btn::B)) => {
            let _ = curl.kill();
            let _ = curl.wait();
            let _ = std::fs::remove_file(part);
            Err(1)
        }
        (_, Action::GbaDown(Btn::B)) => Err(1),
        (screen, _) => Ok(screen),
    }
}

/// A line of the screen's words, `px` tall at most, wrapped to `lines`, centred in a box as
/// wide as `w`.
fn face(text: &str, w: u32, lines: usize, max_px: f32, min_px: f32) -> slot_ui::UndoFace {
    use slot_ui::text;
    let blank = slot_ui::UndoFace {
        rgba: vec![0; 4],
        w: 1,
        h: 1,
    };
    let Some(font) = text::label_font() else {
        return blank;
    };
    let layout = text::fit(font, text, w as f32, lines, max_px, min_px);
    let Some(vm) = font.horizontal_line_metrics(layout.px) else {
        return blank;
    };
    let h = (vm.new_line_size * layout.lines.len().max(1) as f32).ceil() as u32 + 4;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    text::draw_centred(&mut rgba, w, h, &layout, slot_ui::HUD_INK);
    slot_ui::UndoFace { rgba, w, h }
}

/// `--port-runtimes` and `--port-runtime`: offers the runtimes in `files` the card has not got
/// for download, on screen, and puts them in LIBS; 0 once they are all there, 1 if any is not.
/// `strict` is for a runtime the port's script asked for by name, which the port cannot do
/// without; one a port.json names that PortMaster does not list is let go.
pub fn run(root: &Path, files: &[String], strict: bool) -> i32 {
    use slot_gfx::{Canvas, Compositor, Draw, FbdevSurface, Layout, Surface};

    let libs = root.join(LIBS);
    let files = missing(&libs, files);
    if files.is_empty() {
        return 0;
    }
    if let Err(e) = std::fs::create_dir_all(&libs) {
        eprintln!("slot: port runtime: {}: {e}", libs.display());
        return 1;
    }
    let mut surface = match FbdevSurface::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("slot: port runtime: {e}");
            return 1;
        }
    };
    let mut compositor = match Compositor::new(&surface) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("slot: port runtime: {e}");
            return 1;
        }
    };
    let window = surface.window_size();
    compositor.fit(window);
    let canvas = Canvas::of(Layout::Native, window);
    let text_w = (canvas.w as u32).saturating_sub(96).max(1);

    let mut input = crate::input::DeviceInput::open(root);
    let mut gestures = Gestures::new();
    let start_at = Instant::now();
    let mut screen = Screen::Looking(fetch_list());
    // The faces drawn, and the words they were drawn from.
    let mut shown: Option<Words> = None;
    let mut title = None;
    let mut line = None;
    let mut hints: Vec<(slot_gfx::TexId, u32)> = Vec::new();
    let mut draws = Vec::new();
    loop {
        let now = start_at.elapsed().as_millis() as Millis;
        let mut actions = gestures.tick(now);
        for ev in input.poll(now) {
            actions.extend(gestures.feed(ev, now));
        }
        for a in actions {
            screen = match press(&libs, screen, a) {
                Ok(s) => s,
                Err(code) => return code,
            };
        }
        screen = match screen {
            Screen::Looking(rx) => match rx.try_recv() {
                Ok(list) => match looked_up(&libs, &files, list, strict) {
                    Ok(s) => s,
                    Err(code) => return code,
                },
                Err(mpsc::TryRecvError::Empty) => Screen::Looking(rx),
                Err(mpsc::TryRecvError::Disconnected) => {
                    match looked_up(&libs, &files, None, strict) {
                        Ok(s) => s,
                        Err(code) => return code,
                    }
                }
            },
            s @ Screen::Fetching { .. } => match advance(&libs, s) {
                Ok(s) => s,
                Err(code) => return code,
            },
            s => s,
        };

        let now_says = words(&screen);
        // The count moves every frame of a download, so a face is drawn again into its texture.
        let mut put = |slot: &mut Option<(slot_gfx::TexId, u32, u32)>, f: slot_ui::UndoFace| {
            match *slot {
                Some((tex, ..)) => compositor.update_texture(tex, f.w, f.h, &f.rgba),
                None => *slot = Some((compositor.create_texture(f.w, f.h, &f.rgba), 0, 0)),
            }
            if let Some(s) = slot {
                (s.1, s.2) = (f.w, f.h);
            }
        };
        let was = shown.as_ref();
        if was.map(|w| &w.title) != Some(&now_says.title) {
            put(&mut title, face(&now_says.title, text_w, 2, 40.0, 24.0));
        }
        if was.map(|w| &w.line) != Some(&now_says.line) {
            put(&mut line, face(&now_says.line, text_w, 2, 26.0, 18.0));
        }
        if was.map(|w| &w.legend) != Some(&now_says.legend) {
            hints = now_says
                .legend
                .iter()
                .map(|(key, label)| {
                    let f = slot_ui::hint_face(key, label);
                    (compositor.create_texture(f.w, f.h, &f.rgba), f.w)
                })
                .collect();
        }
        let fraction = now_says.fraction;
        shown = Some(now_says);

        draws.clear();
        draws.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: canvas.w,
            h: canvas.h,
            colour: [0.0, 0.0, 0.0, 1.0],
        });
        match fraction {
            Some(fraction) => slot_ui::RebuildScreen {
                title,
                count: line,
                fraction,
            }
            .draw(canvas, &mut draws),
            None => {
                let gap = 24.0;
                let th = title.map_or(0.0, |(_, _, h)| h as f32);
                let lh = line.map_or(0.0, |(_, _, h)| h as f32);
                let top = (canvas.h - th - gap - lh) / 2.0;
                for (face, y) in [(title, top), (line, top + th + gap)] {
                    if let Some((tex, w, h)) = face {
                        draws.push(Draw::Tex {
                            x: canvas.centred(w as f32),
                            y,
                            w: w as f32,
                            h: h as f32,
                            tex,
                            alpha: 1.0,
                        });
                    }
                }
            }
        }
        let hint_y = canvas.h - slot_ui::HINT_H as f32 - 32.0;
        for (tex, w, x) in slot_ui::centred_hints(canvas, &hints, slot_ui::LEGEND_GAP) {
            draws.push(Draw::Tex {
                x,
                y: hint_y,
                w: w as f32,
                h: slot_ui::HINT_H as f32,
                tex,
                alpha: 1.0,
            });
        }
        compositor.begin_frame(Layout::Native);
        compositor.set_screen_power(1.0);
        compositor.draw_list(&draws);
        compositor.end_frame(window);
        if let Err(e) = surface.swap() {
            eprintln!("slot: port runtime: {e}");
            return 1;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{
  "ports": {
    "game.zip": {
      "attr": { "runtime": ["frt_3.5.2.squashfs"] }
    }
  },
  "utils": {
    "frt_3.5.2.armhf.squashfs": {
      "name": "Godot/FRT 3.5.2 (armhf)",
      "md5": "aaaa",
      "size": 1,
      "url": "https://example/armhf"
    },
    "frt_3.5.2.squashfs": {
      "name": "Godot/FRT 3.5.2",
      "runtime_arch": "aarch64",
      "md5": "d98d82d86ae7630b8ef62da9705cbda8",
      "size": 12775424,
      "url": "https://example/frt_3.5.2.squashfs"
    }
  }
}"#;

    #[test]
    fn a_runtime_is_read_from_its_own_entry_not_a_port_naming_it() {
        let rt = listed(LIST, "frt_3.5.2.squashfs").unwrap();
        assert_eq!(rt.name, "Godot/FRT 3.5.2");
        assert_eq!(rt.size, 12775424);
        assert_eq!(rt.md5, "d98d82d86ae7630b8ef62da9705cbda8");
        assert_eq!(rt.url, "https://example/frt_3.5.2.squashfs");
        assert_eq!(listed(LIST, "godot_4.3.squashfs"), None);
    }

    #[test]
    fn a_port_json_names_the_runtimes_for_its_script() {
        let dir = std::env::temp_dir().join(format!("slot-port-runtime-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("anodyne")).unwrap();
        std::fs::create_dir_all(dir.join("other")).unwrap();
        std::fs::write(
            dir.join("other/port.json"),
            r#"{"items": ["Other.sh", "other"], "attr": {"runtime": ["mono"]}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("anodyne/port.json"),
            r#"{"items": ["Anodyne.sh", "anodyne"], "items_opt": null,
               "attr": {"runtime": ["mono-6.12.0.122-aarch64.squashfs", "weston_pkg_0.2"]}}"#,
        )
        .unwrap();
        assert_eq!(
            wanted(&dir, &dir.join("Anodyne.sh")),
            [
                "mono-6.12.0.122-aarch64.squashfs",
                "weston_pkg_0.2.squashfs"
            ]
        );
        assert!(wanted(&dir, &dir.join("Unknown.sh")).is_empty());
        std::fs::write(
            dir.join("other/port.json"),
            r#"{"items": ["Other.sh", "other"], "attr": {"runtime": "frt_3.5.2.squashfs"}}"#,
        )
        .unwrap();
        assert_eq!(wanted(&dir, &dir.join("Other.sh")), ["frt_3.5.2.squashfs"]);

        std::fs::write(dir.join("weston_pkg_0.2.squashfs"), b"").unwrap();
        let files = wanted(&dir, &dir.join("Anodyne.sh"));
        assert_eq!(missing(&dir, &files), ["mono-6.12.0.122-aarch64.squashfs"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
