//! Videos, through the ffmpeg a build ships in System/bin: one ffmpeg sends the pictures as raw
//! RGBA, laid out to the page's size at a steady rate, and another sends the sound to be mixed
//! into the output. Skipping starts both again from the new place.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The picture's size: a list page's, the video fitted inside it.
pub const W: u32 = slot_ui::LIST_W;
pub const H: u32 = slot_ui::LIST_H;
const FPS: u32 = 30;
/// Sound decoded ahead, in samples: a fifth of a second.
const AHEAD: usize = crate::audio::OUTPUT_HZ as usize * 2 / 5;

/// The picture showing, `serial` counting each new one.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub serial: u64,
    pub rgba: Arc<Vec<u8>>,
}

struct Shared {
    frame: Mutex<Frame>,
    sound: Mutex<VecDeque<i16>>,
    paused: AtomicBool,
    stop: AtomicBool,
    ended: AtomicBool,
    volume: std::sync::atomic::AtomicU8,
}

pub struct Video {
    pub path: PathBuf,
    pub title: String,
    /// Where it started from, in seconds, and when, less any time paused.
    from: f64,
    began: Instant,
    paused_at: Option<Instant>,
    shared: Arc<Shared>,
    children: Vec<Child>,
    pub error: Option<String>,
}

/// ffmpeg: the one System ships, else one on the PATH.
pub fn ffmpeg(root: &Path) -> PathBuf {
    let ours = root.join("System/bin/ffmpeg");
    match ours.is_file() {
        true => ours,
        false => PathBuf::from("ffmpeg"),
    }
}

impl Video {
    pub fn open(root: &Path, path: &Path, from: f64, volume: u8) -> Video {
        let title = super::music::name(path);
        let shared = Arc::new(Shared {
            frame: Mutex::new(Frame::default()),
            sound: Mutex::new(VecDeque::new()),
            paused: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            volume: std::sync::atomic::AtomicU8::new(volume),
        });
        let mut video = Video {
            path: path.to_path_buf(),
            title,
            from,
            began: Instant::now(),
            paused_at: None,
            shared,
            children: Vec::new(),
            error: None,
        };
        let picture = format!(
            "fps={FPS},scale={W}:{H}:force_original_aspect_ratio=decrease,\
             pad={W}:{H}:(ow-iw)/2:(oh-ih)/2"
        );
        let pictures = video.spawn(
            root,
            &["-an", "-vf", &picture, "-pix_fmt", "rgba", "-f", "rawvideo"],
        );
        let rate = crate::audio::OUTPUT_HZ.to_string();
        let sound = video.spawn(root, &["-vn", "-ac", "2", "-ar", &rate, "-f", "s16le"]);
        match pictures {
            Some(out) => {
                let s = video.shared.clone();
                let began = video.began;
                let _ = std::thread::Builder::new()
                    .name("slot-video".into())
                    .spawn(move || show(out, &s, began));
            }
            None => video.error = Some("Videos need ffmpeg in System/bin".into()),
        }
        if let Some(out) = sound {
            let s = video.shared.clone();
            let _ = std::thread::Builder::new()
                .name("slot-video-sound".into())
                .spawn(move || hear(out, &s));
            let tap = video.shared.clone();
            crate::audio::set_music_tap(
                "video",
                Some(Arc::new(move |out: &mut [i16], _| mix(&tap, out))),
            );
        }
        video
    }

    fn spawn(&mut self, root: &Path, args: &[&str]) -> Option<std::process::ChildStdout> {
        let mut child = Command::new(ffmpeg(root))
            .args(["-nostdin", "-loglevel", "error", "-ss"])
            .arg(format!("{:.2}", self.from))
            .arg("-i")
            .arg(&self.path)
            .args(args)
            .arg("pipe:1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let out = child.stdout.take();
        self.children.push(child);
        out
    }

    pub fn frame(&self) -> Frame {
        self.shared
            .frame
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Where it is, in seconds.
    pub fn at(&self) -> f64 {
        let to = self.paused_at.unwrap_or_else(Instant::now);
        self.from + to.duration_since(self.began).as_secs_f64()
    }

    pub fn paused(&self) -> bool {
        self.paused_at.is_some()
    }

    pub fn toggle(&mut self) {
        match self.paused_at.take() {
            Some(at) => self.began += at.elapsed(),
            None => self.paused_at = Some(Instant::now()),
        }
        self.shared
            .paused
            .store(self.paused_at.is_some(), Ordering::Relaxed);
    }

    pub fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, volume: u8) {
        self.shared.volume.store(volume, Ordering::Relaxed);
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        crate::audio::set_music_tap("video", None);
        self.shared.stop.store(true, Ordering::Relaxed);
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Reads each picture and holds it until its time comes.
fn show(mut out: impl Read, shared: &Shared, began: Instant) {
    let size = (W * H * 4) as usize;
    let mut n: u64 = 0;
    let mut paused = Duration::ZERO;
    loop {
        let mut rgba = vec![0u8; size];
        if out.read_exact(&mut rgba).is_err() {
            break;
        }
        let due = Duration::from_secs_f64(n as f64 / f64::from(FPS));
        loop {
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            if shared.paused.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(20));
                paused += Duration::from_millis(20);
                continue;
            }
            let now = began.elapsed().saturating_sub(paused);
            if now >= due {
                break;
            }
            std::thread::sleep((due - now).min(Duration::from_millis(20)));
        }
        n += 1;
        let mut frame = shared.frame.lock().unwrap_or_else(|e| e.into_inner());
        *frame = Frame {
            serial: n,
            rgba: Arc::new(rgba),
        };
    }
    shared.ended.store(true, Ordering::Relaxed);
}

fn hear(mut out: impl Read, shared: &Shared) {
    let mut bytes = [0u8; 4096];
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        if shared.sound.lock().unwrap_or_else(|e| e.into_inner()).len() > AHEAD {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let n = match out.read(&mut bytes) {
            Ok(0) | Err(_) => return,
            Ok(n) => n & !1,
        };
        let mut sound = shared.sound.lock().unwrap_or_else(|e| e.into_inner());
        sound.extend(
            bytes[..n]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]])),
        );
    }
}

fn mix(shared: &Shared, out: &mut [i16]) {
    if shared.paused.load(Ordering::Relaxed) {
        return;
    }
    let gain = crate::audio::volume::gain(shared.volume.load(Ordering::Relaxed));
    let mut sound = shared.sound.lock().unwrap_or_else(|e| e.into_inner());
    for s in out.iter_mut() {
        let Some(v) = sound.pop_front() else {
            break;
        };
        *s = s.saturating_add((f32::from(v) * gain) as i16);
    }
}
