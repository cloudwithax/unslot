//! The music player: a thread that decodes the queue a track at a time and a buffer the audio
//! output mixes in over everything else, so it plays on with its page closed. It holds while a
//! game is running.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::files::Kind;

/// Your music, on the card.
pub const DIR: &str = "Music";

/// A quarter of a second, decoded ahead.
const AHEAD_FRAMES: usize = 12_000;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct MusicView {
    pub queue: Vec<PathBuf>,
    /// The track playing, or paused.
    pub at: Option<usize>,
    pub paused: bool,
    /// How far into the track, in seconds.
    pub secs: u32,
    pub error: Option<String>,
}

impl MusicView {
    pub fn playing(&self) -> Option<&Path> {
        Some(self.queue.get(self.at?)?.as_path())
    }
}

enum Cmd {
    Play(Vec<PathBuf>, usize),
    Skip(isize),
    Stop,
}

struct Shared {
    buf: Mutex<VecDeque<i16>>,
    room: Condvar,
    paused: AtomicBool,
    /// Held while a game runs.
    held: AtomicBool,
    volume: AtomicU8,
    rate: AtomicU32,
}

pub struct Music {
    cmds: Sender<Cmd>,
    view: Arc<Mutex<MusicView>>,
    shared: Arc<Shared>,
}

impl Music {
    pub fn spawn() -> Music {
        let shared = Arc::new(Shared {
            buf: Mutex::new(VecDeque::new()),
            room: Condvar::new(),
            paused: AtomicBool::new(false),
            held: AtomicBool::new(false),
            volume: AtomicU8::new(100),
            rate: AtomicU32::new(crate::audio::OUTPUT_HZ),
        });
        let tap = shared.clone();
        crate::audio::set_music_tap(
            "music",
            Some(Arc::new(move |out: &mut [i16], rate| mix(&tap, out, rate))),
        );
        let (cmds, rx) = channel();
        let view = Arc::new(Mutex::new(MusicView::default()));
        let (v, s) = (view.clone(), shared.clone());
        let spawned = std::thread::Builder::new()
            .name("slot-music".into())
            .spawn(move || work(rx, &v, &s));
        if let Err(e) = spawned {
            eprintln!("slot: music: {e}");
        }
        Music { cmds, view, shared }
    }

    pub fn view(&self) -> MusicView {
        let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner()).clone();
        view.paused = self.shared.paused.load(Ordering::Relaxed);
        view
    }

    pub fn play(&self, queue: Vec<PathBuf>, at: usize) {
        self.shared.paused.store(false, Ordering::Relaxed);
        let _ = self.cmds.send(Cmd::Play(queue, at));
    }

    pub fn toggle(&self) {
        let paused = self.shared.paused.load(Ordering::Relaxed);
        self.shared.paused.store(!paused, Ordering::Relaxed);
        self.shared.room.notify_all();
    }

    pub fn skip(&self, by: isize) {
        let _ = self.cmds.send(Cmd::Skip(by));
        self.shared.room.notify_all();
    }

    pub fn stop(&self) {
        let _ = self.cmds.send(Cmd::Stop);
        self.shared.room.notify_all();
    }

    pub fn set_held(&self, held: bool) {
        self.shared.held.store(held, Ordering::Relaxed);
    }

    pub fn set_volume(&self, volume: u8) {
        self.shared.volume.store(volume, Ordering::Relaxed);
    }
}

impl Drop for Music {
    fn drop(&mut self) {
        crate::audio::set_music_tap("music", None);
        let _ = self.cmds.send(Cmd::Stop);
    }
}

/// Every track under `dir`, through its folders, in path order.
pub fn tracks(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in super::files::list(&d, Some(&[Kind::Audio])) {
            match e.kind {
                Kind::Folder => dirs.push(e.path),
                _ => out.push(e.path),
            }
        }
    }
    out.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    out
}

fn mix(shared: &Shared, out: &mut [i16], rate: u32) {
    if rate > 0 {
        shared.rate.store(rate, Ordering::Relaxed);
    }
    if shared.paused.load(Ordering::Relaxed) || shared.held.load(Ordering::Relaxed) {
        return;
    }
    let gain = crate::audio::volume::gain(shared.volume.load(Ordering::Relaxed));
    let mut buf = shared.buf.lock().unwrap_or_else(|e| e.into_inner());
    for s in out.iter_mut() {
        let Some(m) = buf.pop_front() else {
            break;
        };
        *s = s.saturating_add((m as f32 * gain) as i16);
    }
    drop(buf);
    shared.room.notify_all();
}

/// What the thread is to do once a track stops: carry on to the next, or what it was told.
enum After {
    Next,
    Told(Cmd),
    Gone,
}

fn work(rx: Receiver<Cmd>, view: &Mutex<MusicView>, shared: &Shared) {
    let set = |f: &dyn Fn(&mut MusicView)| f(&mut view.lock().unwrap_or_else(|e| e.into_inner()));
    let mut next = match rx.recv() {
        Ok(cmd) => Some(cmd),
        Err(_) => return,
    };
    loop {
        let cmd = match next.take() {
            Some(cmd) => cmd,
            None => match rx.recv() {
                Ok(cmd) => cmd,
                Err(_) => return,
            },
        };
        let (queue, mut at) = match cmd {
            Cmd::Play(queue, at) => (queue, at),
            Cmd::Skip(by) => {
                let v = view.lock().unwrap_or_else(|e| e.into_inner()).clone();
                match v.at {
                    Some(at) => (v.queue, (at as isize + by).max(0) as usize),
                    None => continue,
                }
            }
            Cmd::Stop => {
                shared.buf.lock().unwrap_or_else(|e| e.into_inner()).clear();
                set(&|v| v.at = None);
                continue;
            }
        };
        shared.buf.lock().unwrap_or_else(|e| e.into_inner()).clear();
        set(&|v| {
            v.queue = queue.clone();
            v.error = None;
        });
        loop {
            let Some(track) = queue.get(at) else {
                set(&|v| v.at = None);
                break;
            };
            set(&|v| {
                v.at = Some(at);
                v.secs = 0;
            });
            match play(track, &rx, view, shared) {
                Ok(After::Next) => at += 1,
                Ok(After::Told(cmd)) => {
                    next = Some(cmd);
                    break;
                }
                Ok(After::Gone) => return,
                Err(e) => {
                    eprintln!("slot: music: {}: {e}", track.display());
                    set(&|v| v.error = Some(format!("{} will not play", name(track))));
                    at += 1;
                }
            }
        }
    }
}

pub fn name(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Decodes one track into the buffer, at the output's rate.
fn play(
    path: &Path,
    rx: &Receiver<Cmd>,
    view: &Mutex<MusicView>,
    shared: &Shared,
) -> Result<After, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio in it")?;
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let mut resample: Option<Resample> = None;
    let mut out = Vec::new();
    let mut frames_out: u64 = 0;
    loop {
        match rx.try_recv() {
            Ok(cmd) => return Ok(After::Told(cmd)),
            Err(TryRecvError::Disconnected) => return Ok(After::Gone),
            Err(TryRecvError::Empty) => {}
        }
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(After::Next)
            }
            Err(Error::ResetRequired) => return Ok(After::Next),
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        let frames: Vec<[f32; 2]> = samples
            .samples()
            .chunks_exact(channels)
            .map(|f| [f[0], f[channels.min(2) - 1]])
            .collect();
        let rate = shared.rate.load(Ordering::Relaxed).max(1);
        let r = resample.get_or_insert_with(|| Resample::new(spec.rate, rate));
        out.clear();
        r.run(&frames, &mut out);
        frames_out += (out.len() / 2) as u64;
        let secs = (frames_out / u64::from(rate)) as u32;
        view.lock().unwrap_or_else(|e| e.into_inner()).secs = secs;
        // Waits for room, looking out for a word from the page as it does.
        let mut buf = shared.buf.lock().unwrap_or_else(|e| e.into_inner());
        while buf.len() / 2 > AHEAD_FRAMES {
            buf = shared
                .room
                .wait_timeout(buf, Duration::from_millis(50))
                .map(|(b, _)| b)
                .unwrap_or_else(|e| e.into_inner().0);
            drop(buf);
            match rx.recv_timeout(Duration::ZERO) {
                Ok(cmd) => return Ok(After::Told(cmd)),
                Err(RecvTimeoutError::Disconnected) => return Ok(After::Gone),
                Err(RecvTimeoutError::Timeout) => {}
            }
            buf = shared.buf.lock().unwrap_or_else(|e| e.into_inner());
        }
        buf.extend(out.iter().copied());
    }
}

/// Linear resampling from a track's rate to the output's, carried across packets.
struct Resample {
    step: f64,
    pos: f64,
    last: [f32; 2],
}

impl Resample {
    fn new(from: u32, to: u32) -> Resample {
        Resample {
            step: f64::from(from.max(1)) / f64::from(to.max(1)),
            pos: 1.0,
            last: [0.0; 2],
        }
    }

    fn run(&mut self, input: &[[f32; 2]], out: &mut Vec<i16>) {
        let n = input.len();
        if n == 0 {
            return;
        }
        // Frame 0 is the last of the packet before, then this packet's.
        let at = |i: usize| if i == 0 { self.last } else { input[i - 1] };
        while self.pos < n as f64 {
            let i = self.pos as usize;
            let f = (self.pos - i as f64) as f32;
            let (a, b) = (at(i), at(i + 1));
            for c in 0..2 {
                let v = a[c] + (b[c] - a[c]) * f;
                out.push((v.clamp(-1.0, 1.0) * 32767.0) as i16);
            }
            self.pos += self.step;
        }
        self.pos -= n as f64;
        self.last = input[n - 1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_rate_passes_every_frame_through_a_frame_behind() {
        let mut r = Resample::new(48_000, 48_000);
        let mut out = Vec::new();
        r.run(&[[0.5, -0.5]; 100], &mut out);
        r.run(&[[0.25, 0.25]; 100], &mut out);
        assert_eq!(out.len(), 2 * 199);
        assert_eq!(out[0], (0.5 * 32767.0) as i16);
        assert_eq!(out[1], (-0.5 * 32767.0) as i16);
        assert_eq!(
            out[2 * 99],
            (0.5 * 32767.0) as i16,
            "the last frame was not carried over"
        );
    }

    #[test]
    fn half_the_rate_makes_twice_the_frames() {
        let mut r = Resample::new(24_000, 48_000);
        let mut out = Vec::new();
        for _ in 0..10 {
            r.run(&[[0.25, 0.25]; 100], &mut out);
        }
        assert!((out.len() as i64 / 2 - 2000).abs() <= 2, "{}", out.len());
    }
}
