//! The picture on its way to a TV: the compositor's canvas, read back at the end of a frame for an
//! encoder on another thread. Nothing is read while no tap is set.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// A frame of the canvas: `w` by `h` XRGB8888, bytes blue, green, red, unused, top row first.
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pixels: Pixels,
    /// When the compositor finished it.
    pub at: Instant,
    /// How long the compositor spent on it.
    pub read: Duration,
    /// GPU-fence and cache-sync waiting, which a reader can overlap with the previous encode.
    pub read_wait: Duration,
}

enum Pixels {
    Read(Vec<u8>),
    Lent(Box<dyn Lent>),
}

/// A frame the GPU is still copying out, into memory the CPU can read once it is done.
pub(crate) trait Lent: Send {
    /// The frame, once the GPU has finished it.
    fn pixels(&mut self) -> &[u8];
}

impl Frame {
    /// The frame's pixels: for one the GPU is still copying out, once it is done.
    pub fn pixels(&mut self) -> &[u8] {
        match &mut self.pixels {
            Pixels::Read(v) => v,
            Pixels::Lent(l) => {
                let start = Instant::now();
                let pixels = l.pixels();
                self.read_wait += start.elapsed();
                pixels
            }
        }
    }
}

/// Where the compositor leaves a frame every `every` for the encoder to take. A frame not taken
/// by the time the next is due is replaced, so a slow encoder drops frames rather than falling
/// behind.
pub struct FrameTap {
    every: Duration,
    bounds: (u32, u32),
    state: Mutex<State>,
    ready: Condvar,
}

#[derive(Default)]
struct State {
    frame: Option<Frame>,
    spare: Vec<u8>,
    last: Option<Instant>,
    /// Frames replaced before the encoder took them.
    dropped: u32,
}

impl FrameTap {
    pub fn new(fps: u32, bounds: (u32, u32)) -> Arc<FrameTap> {
        Arc::new(FrameTap {
            every: Duration::from_secs(1) / fps.max(1),
            bounds,
            state: Mutex::default(),
            ready: Condvar::new(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether a frame finished at `now` is wanted. A few milliseconds early counts, so a 60 Hz
    /// screen gives every other frame to a 30 a second tap rather than every third.
    pub fn due(&self, now: Instant) -> bool {
        let slack = Duration::from_millis(4);
        self.lock()
            .last
            .is_none_or(|last| now + slack >= last + self.every)
    }

    /// Downscale on the GPU before the CPU reads the picture, keeping the source's shape and
    /// even dimensions for I420. Small canvases are not upscaled.
    pub(crate) fn capture_size(&self, (w, h): (u32, u32)) -> (u32, u32) {
        let (w, h) = (w & !1, h & !1);
        let (ow, oh) = self.bounds;
        if w <= ow && h <= oh {
            (w, h)
        } else if u64::from(w) * u64::from(oh) > u64::from(ow) * u64::from(h) {
            (
                ow,
                (u64::from(h) * u64::from(ow) / u64::from(w)) as u32 & !1,
            )
        } else {
            (
                (u64::from(w) * u64::from(oh) / u64::from(h)) as u32 & !1,
                oh,
            )
        }
    }

    /// Has `fill` write a `w` by `h` frame into a buffer, and leaves it for the encoder.
    pub fn put(&self, w: u32, h: u32, now: Instant, fill: impl FnOnce(&mut [u8]) -> bool) {
        let mut buf = {
            let mut s = self.lock();
            s.last = Some(now);
            match s.frame.take() {
                Some(Frame {
                    pixels: Pixels::Read(old),
                    ..
                }) => {
                    s.dropped += 1;
                    old
                }
                Some(_) => {
                    s.dropped += 1;
                    std::mem::take(&mut s.spare)
                }
                None => std::mem::take(&mut s.spare),
            }
        };
        buf.resize((w * h * 4) as usize, 0);
        let start = Instant::now();
        if !fill(&mut buf) {
            self.lock().spare = buf;
            return;
        }
        self.lock().frame = Some(Frame {
            w,
            h,
            pixels: Pixels::Read(buf),
            at: now,
            read: start.elapsed(),
            read_wait: Duration::ZERO,
        });
        self.ready.notify_all();
    }

    /// Leaves a `w` by `h` frame the GPU is still copying out, `read` having been spent on it.
    pub(crate) fn lend(&self, w: u32, h: u32, now: Instant, lent: Box<dyn Lent>, read: Duration) {
        if let Some(old) = self.publish(Frame {
            w,
            h,
            pixels: Pixels::Lent(lent),
            at: now,
            read,
            read_wait: Duration::ZERO,
        }) {
            self.give_back(old);
        }
    }

    fn publish(&self, frame: Frame) -> Option<Frame> {
        let mut s = self.lock();
        s.last = Some(frame.at);
        let old = s.frame.replace(frame);
        if old.is_some() {
            s.dropped += 1;
        }
        drop(s);
        self.ready.notify_all();
        old
    }

    /// Waits for GPU copies on a reader thread while the caller encodes the previous frame.
    /// Each stage keeps only its latest frame, never an unbounded queue of stale pictures.
    pub fn reader(self: &Arc<Self>) -> std::io::Result<FrameReader> {
        let ready = Arc::new(FrameTap {
            every: self.every,
            bounds: self.bounds,
            state: Mutex::default(),
            ready: Condvar::new(),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let (source, prepared, stopping) = (self.clone(), ready.clone(), stop.clone());
        let worker = std::thread::Builder::new()
            .name("slot-cast-read".into())
            .spawn(move || {
                while !stopping.load(Ordering::Relaxed) {
                    let Some(mut frame) = source.take(Duration::from_millis(100)) else {
                        continue;
                    };
                    frame.pixels();
                    if stopping.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(old) = prepared.publish(frame) {
                        source.give_back(old);
                    }
                }
            })?;
        Ok(FrameReader {
            source: self.clone(),
            ready,
            stop,
            worker: Some(worker),
        })
    }

    /// The latest frame, waiting up to `wait` for one.
    pub fn take(&self, wait: Duration) -> Option<Frame> {
        let s = self.lock();
        let (mut s, _) = self
            .ready
            .wait_timeout_while(s, wait, |s| s.frame.is_none())
            .unwrap_or_else(|e| e.into_inner());
        s.frame.take()
    }

    /// How many frames were replaced before the encoder took them, since the tap was made.
    pub fn dropped(&self) -> u32 {
        self.lock().dropped
    }

    /// Hands a taken frame's buffer back, for the next frame to be read into.
    pub fn give_back(&self, frame: Frame) {
        let Pixels::Read(v) = frame.pixels else {
            return;
        };
        let mut s = self.lock();
        if s.spare.capacity() < v.capacity() {
            s.spare = v;
        }
    }
}

static TAP: Mutex<Option<Arc<FrameTap>>> = Mutex::new(None);
static TAPPED: AtomicBool = AtomicBool::new(false);

/// A bounded GPU-readback stage in front of an encoder. Dropping it stops and joins its reader.
pub struct FrameReader {
    source: Arc<FrameTap>,
    ready: Arc<FrameTap>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl FrameReader {
    pub fn take(&self, wait: Duration) -> Option<Frame> {
        self.ready.take(wait)
    }

    pub fn give_back(&self, frame: Frame) {
        self.source.give_back(frame);
    }

    pub fn dropped(&self) -> u32 {
        self.source.dropped().wrapping_add(self.ready.dropped())
    }
}

impl Drop for FrameReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Starts reading the canvas into `tap`, or stops with None.
pub fn set_frame_tap(tap: Option<Arc<FrameTap>>) {
    let mut t = TAP.lock().unwrap_or_else(|e| e.into_inner());
    TAPPED.store(tap.is_some(), Ordering::Relaxed);
    *t = tap;
}

/// The tap, if one is set: an atomic load when none is.
pub(crate) fn frame_tap() -> Option<Arc<FrameTap>> {
    if !TAPPED.load(Ordering::Relaxed) {
        return None;
    }
    TAP.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PendingGpu {
        pixels: Vec<u8>,
        entered: Option<std::sync::mpsc::Sender<()>>,
        gate: Option<std::sync::mpsc::Receiver<()>>,
    }

    impl Lent for PendingGpu {
        fn pixels(&mut self) -> &[u8] {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
            }
            if let Some(gate) = self.gate.take() {
                gate.recv().unwrap();
            }
            &self.pixels
        }
    }

    #[test]
    fn a_ready_frame_is_available_while_the_next_gpu_copy_is_waiting() {
        let source = FrameTap::new(60, (640, 480));
        let reader = source.reader().unwrap();
        let (release, gate) = std::sync::mpsc::channel();
        let (seen, entered) = std::sync::mpsc::channel();
        source.lend(
            2,
            2,
            Instant::now(),
            Box::new(PendingGpu {
                pixels: vec![1; 16],
                entered: Some(seen.clone()),
                gate: None,
            }),
            Duration::ZERO,
        );
        entered.recv_timeout(Duration::from_secs(1)).unwrap();
        source.lend(
            2,
            2,
            Instant::now(),
            Box::new(PendingGpu {
                pixels: vec![2; 16],
                entered: Some(seen),
                gate: Some(gate),
            }),
            Duration::ZERO,
        );
        entered.recv_timeout(Duration::from_secs(1)).unwrap();
        // The reader is now blocked on the second GPU copy, not the encoder's first frame.
        let frame = reader.take(Duration::ZERO);
        release.send(()).unwrap();
        let mut frame = frame.expect("the first frame was prepared before the next GPU wait");
        assert_eq!(frame.pixels(), &[1; 16]);
    }

    #[test]
    fn prepared_frames_replace_stale_ones_instead_of_building_a_queue() {
        let source = FrameTap::new(60, (640, 480));
        let reader = source.reader().unwrap();
        let (release, gate) = std::sync::mpsc::channel();
        let (seen, entered) = std::sync::mpsc::channel();
        for value in [1, 2] {
            source.lend(
                2,
                2,
                Instant::now(),
                Box::new(PendingGpu {
                    pixels: vec![value; 16],
                    entered: Some(seen.clone()),
                    gate: None,
                }),
                Duration::ZERO,
            );
            entered.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        source.lend(
            2,
            2,
            Instant::now(),
            Box::new(PendingGpu {
                pixels: vec![3; 16],
                entered: Some(seen),
                gate: Some(gate),
            }),
            Duration::ZERO,
        );
        entered.recv_timeout(Duration::from_secs(1)).unwrap();
        let frame = reader.take(Duration::ZERO);
        let dropped = reader.dropped();
        release.send(()).unwrap();
        let mut frame = frame.expect("the latest completed copy is available");
        assert_eq!(frame.pixels(), &[2; 16]);
        assert_eq!(dropped, 1);
    }

    #[test]
    fn capture_is_bounded_without_stretching_or_upscaling() {
        let cast = FrameTap::new(60, (854, 480));
        assert_eq!(cast.capture_size((720, 720)), (480, 480));
        assert_eq!(cast.capture_size((1920, 1080)), (852, 480));
        assert_eq!(cast.capture_size((320, 240)), (320, 240));
        assert_eq!(cast.capture_size((321, 241)), (320, 240));
        let miracast = FrameTap::new(60, (640, 480));
        assert_eq!(miracast.capture_size((720, 480)), (640, 426));
    }

    #[test]
    fn a_frame_not_taken_is_replaced_by_the_next() {
        let tap = FrameTap::new(30, (640, 480));
        let t0 = Instant::now();
        assert!(tap.due(t0));
        tap.put(2, 1, t0, |b| {
            b.fill(1);
            true
        });
        // Every other frame of a 60 Hz screen.
        assert!(!tap.due(t0 + Duration::from_millis(16)));
        assert!(tap.due(t0 + Duration::from_millis(32)));
        tap.put(2, 1, t0 + Duration::from_millis(33), |b| {
            b.fill(2);
            true
        });
        let mut f = tap.take(Duration::ZERO).expect("a frame");
        assert_eq!((f.w, f.h, f.pixels()), (2, 1, &[2u8; 8][..]));
        assert!(tap.take(Duration::from_millis(1)).is_none());
        assert_eq!(tap.dropped(), 1);
    }

    struct Gpu(Vec<u8>, std::sync::Arc<AtomicBool>);

    impl Lent for Gpu {
        fn pixels(&mut self) -> &[u8] {
            &self.0
        }
    }

    impl Drop for Gpu {
        fn drop(&mut self) {
            self.1.store(true, Ordering::Relaxed);
        }
    }

    #[test]
    fn a_frame_still_on_the_gpu_is_handed_on_and_let_go_when_replaced() {
        let tap = FrameTap::new(60, (640, 480));
        let t0 = Instant::now();
        let first_gone = std::sync::Arc::new(AtomicBool::new(false));
        let gpu = Gpu(vec![1; 8], first_gone.clone());
        tap.lend(2, 1, t0, Box::new(gpu), Duration::ZERO);
        let gpu = Gpu(vec![3; 8], std::sync::Arc::default());
        tap.lend(
            2,
            1,
            t0 + Duration::from_millis(17),
            Box::new(gpu),
            Duration::ZERO,
        );
        assert!(
            first_gone.load(Ordering::Relaxed),
            "its buffer back for the GPU"
        );
        let mut f = tap.take(Duration::ZERO).expect("a frame");
        assert_eq!(f.pixels(), &[3u8; 8][..]);
        tap.give_back(f);
    }

    #[test]
    fn a_frame_that_could_not_be_read_is_not_handed_on() {
        let tap = FrameTap::new(30, (640, 480));
        tap.put(2, 2, Instant::now(), |_| false);
        assert!(tap.take(Duration::ZERO).is_none());
    }
}
