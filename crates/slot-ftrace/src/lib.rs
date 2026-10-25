//! A frame-path tracer for profiling, on only when `SLOT_FTRACE=<path>` is set.
//!
//! Every record is 24 bytes, little-endian, appended to `<path>`:
//! `u64 t_ns` (CLOCK_MONOTONIC), `u32 tid` (gettid), `u16 event`, `u8 phase`
//! (0 instant, 1 begin, 2 end, 3 counter), `u8 0`, `i64 arg`.
//! `<path>.names` maps event ids to names, `<path>.threads` maps `tid pid comm`.
//!
//! Each thread appends to a buffer of its own; a background thread writes them out about every
//! 250 ms, so no traced thread waits on a file and a killed process loses under a second. Off, an
//! event is one atomic load.
//!
//! A core host process (`SLOT_FTRACE_CHILD` set by slot when it spawns one) appends to the same
//! file without truncating it, so both processes share the one timeline.

mod events;

use std::cell::OnceCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use events::{ev, NAMES};

pub const VAR: &str = "SLOT_FTRACE";
pub const CHILD_VAR: &str = "SLOT_FTRACE_CHILD";

pub const INSTANT: u8 = 0;
pub const BEGIN: u8 = 1;
pub const END: u8 = 2;
pub const COUNTER: u8 = 3;

pub const RECORD: usize = 24;

const FLUSH_EVERY: Duration = Duration::from_millis(250);
/// Records one thread may hold between flushes, past which new ones are dropped.
const MAX_BUFFERED: usize = 1 << 20;
const FIRST_BUFFER: usize = 4096;

const UNSET: u8 = 0;
const OFF: u8 = 1;
const ON: u8 = 2;

static STATE: AtomicU8 = AtomicU8::new(UNSET);

type Buffer = Arc<Mutex<Vec<u8>>>;

struct Tracer {
    buffers: Mutex<Vec<(u32, Buffer)>>,
}

static TRACER: Mutex<Option<Arc<Tracer>>> = Mutex::new(None);

thread_local! {
    static LOCAL: OnceCell<(u32, Buffer)> = const { OnceCell::new() };
}

#[inline]
pub fn enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        ON => true,
        OFF => false,
        _ => init(),
    }
}

#[cold]
fn init() -> bool {
    let mut slot = TRACER.lock().unwrap_or_else(|e| e.into_inner());
    match STATE.load(Ordering::Relaxed) {
        ON => return true,
        OFF => return false,
        _ => {}
    }
    let path = std::env::var_os(VAR).filter(|p| !p.is_empty());
    let Some(path) = path else {
        STATE.store(OFF, Ordering::Relaxed);
        return false;
    };
    let child = std::env::var_os(CHILD_VAR).is_some();
    let path = std::path::PathBuf::from(path);
    let side = |ext: &str| {
        let mut p = path.clone().into_os_string();
        p.push(ext);
        std::path::PathBuf::from(p)
    };
    let open = |p: &std::path::Path| OpenOptions::new().create(true).append(true).open(p);
    if !child {
        let _ = File::create(&path);
        let _ = File::create(side(".threads"));
    }
    let Ok(file) = open(&path) else {
        eprintln!("slot: ftrace: cannot open {}", path.display());
        STATE.store(OFF, Ordering::Relaxed);
        return false;
    };
    if !child {
        let mut names = String::new();
        for (id, name) in NAMES {
            names.push_str(&format!("{id} {name}\n"));
        }
        let _ = std::fs::write(side(".names"), names);
    }
    let threads = open(&side(".threads")).ok();
    let tracer = Arc::new(Tracer {
        buffers: Mutex::new(Vec::new()),
    });
    let flusher = tracer.clone();
    let spawned = std::thread::Builder::new()
        .name("slot-ftrace".into())
        .spawn(move || flusher.run(file, threads));
    if spawned.is_err() {
        STATE.store(OFF, Ordering::Relaxed);
        return false;
    }
    *slot = Some(tracer);
    STATE.store(ON, Ordering::Relaxed);
    eprintln!("slot: ftrace: recording to {}", path.display());
    true
}

impl Tracer {
    fn run(&self, mut file: File, mut threads: Option<File>) {
        let mut spare: Vec<u8> = Vec::with_capacity(RECORD * FIRST_BUFFER);
        let mut known = 0;
        loop {
            std::thread::sleep(FLUSH_EVERY);
            let buffers: Vec<(u32, Buffer)> = self
                .buffers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(f) = threads.as_mut() {
                for (tid, _) in &buffers[known..] {
                    let comm = std::fs::read_to_string(format!("/proc/self/task/{tid}/comm"))
                        .unwrap_or_default();
                    let _ = writeln!(f, "{tid} {} {}", std::process::id(), comm.trim());
                }
                known = buffers.len();
            }
            for (_, buf) in &buffers {
                spare.clear();
                std::mem::swap(
                    &mut *buf.lock().unwrap_or_else(|e| e.into_inner()),
                    &mut spare,
                );
                if !spare.is_empty() {
                    let _ = file.write_all(&spare);
                }
            }
        }
    }
}

fn now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts is a valid timespec for the call to fill.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

#[cold]
fn register() -> (u32, Buffer) {
    // SAFETY: gettid takes no arguments and cannot fail.
    let tid = unsafe { libc::syscall(libc::SYS_gettid) } as u32;
    let buf: Buffer = Arc::new(Mutex::new(Vec::with_capacity(RECORD * FIRST_BUFFER)));
    if let Some(t) = TRACER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        t.buffers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((tid, buf.clone()));
    }
    (tid, buf)
}

#[inline]
pub fn record(event: u16, phase: u8, arg: i64) {
    if !enabled() {
        return;
    }
    let t = now_ns();
    let _ = LOCAL.try_with(|local| {
        let (tid, buf) = local.get_or_init(register);
        let mut rec = [0u8; RECORD];
        rec[0..8].copy_from_slice(&t.to_le_bytes());
        rec[8..12].copy_from_slice(&tid.to_le_bytes());
        rec[12..14].copy_from_slice(&event.to_le_bytes());
        rec[14] = phase;
        rec[16..24].copy_from_slice(&arg.to_le_bytes());
        let mut buf = buf.lock().unwrap_or_else(|e| e.into_inner());
        if buf.len() < MAX_BUFFERED * RECORD {
            buf.extend_from_slice(&rec);
        }
    });
}

#[inline]
pub fn instant(event: u16, arg: i64) {
    record(event, INSTANT, arg);
}

#[inline]
pub fn begin(event: u16, arg: i64) {
    record(event, BEGIN, arg);
}

#[inline]
pub fn end(event: u16, arg: i64) {
    record(event, END, arg);
}

#[inline]
pub fn counter(event: u16, value: i64) {
    record(event, COUNTER, value);
}

/// Begins `event` now and ends it, with the same `arg`, when dropped.
#[inline]
pub fn span(event: u16, arg: i64) -> Span {
    begin(event, arg);
    Span(event, arg)
}

#[must_use = "the span ends when this is dropped"]
pub struct Span(u16, i64);

impl Drop for Span {
    #[inline]
    fn drop(&mut self) {
        end(self.0, self.1);
    }
}
