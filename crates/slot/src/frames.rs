use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use slot_ftrace::{self as ft, ev};
use slot_retro::{GBA_H, GBA_W};

/// The size of a published picture, and how wide it should look against its height. The
/// aspect is None for the Game Boy family, whose 240x160 canvas the frontend places itself.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shape {
    pub width: u32,
    pub height: u32,
    pub aspect: Option<f32>,
    /// Quarter turns counter-clockwise the picture is shown at.
    pub rotation: u32,
}

impl Default for Shape {
    fn default() -> Self {
        Shape {
            width: GBA_W,
            height: GBA_H,
            aspect: None,
            rotation: 0,
        }
    }
}

pub struct Frames {
    inner: Mutex<Inner>,
    size: usize,
    taken: AtomicU64,
}

struct Inner {
    ready: Option<(Vec<u8>, Shape, u64)>,
    /// Pictures published so far, which numbers each one for the trace.
    seq: u64,
    spare: Vec<Vec<u8>>,
    allocated: usize,
}

impl Frames {
    pub fn new(size: usize) -> Arc<Self> {
        Arc::new(Frames {
            inner: Mutex::new(Inner {
                ready: None,
                seq: 0,
                spare: Vec::new(),
                allocated: 0,
            }),
            size,
            taken: AtomicU64::new(0),
        })
    }

    pub fn take_write(&self) -> Vec<u8> {
        let mut i = self.lock();
        match i.spare.pop() {
            Some(buf) => buf,
            None => {
                i.allocated += 1;
                Vec::with_capacity(self.size)
            }
        }
    }

    /// Makes `buf` the picture to show next, and says which picture it is, counting from 1.
    pub fn publish(&self, buf: Vec<u8>, shape: Shape) -> u64 {
        let mut i = self.lock();
        i.seq += 1;
        let seq = i.seq;
        if let Some((dropped, _, lost)) = i.ready.replace((buf, shape, seq)) {
            ft::instant(ev::FRAMES_OVERWRITTEN, lost as i64);
            i.spare.push(dropped);
        }
        seq
    }

    pub fn latest(self: &Arc<Self>) -> Option<FrameRef> {
        let (buf, shape, seq) = self.lock().ready.take()?;
        self.taken.fetch_add(1, Ordering::Relaxed);
        Some(FrameRef {
            frames: self.clone(),
            buf,
            shape,
            seq,
        })
    }

    pub fn is_ready(&self) -> bool {
        self.lock().ready.is_some()
    }

    pub fn taken(&self) -> u64 {
        self.taken.load(Ordering::Relaxed)
    }

    pub fn allocated(&self) -> usize {
        self.lock().allocated
    }

    fn recycle(&self, buf: Vec<u8>) {
        self.lock().spare.push(buf);
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct FrameRef {
    frames: Arc<Frames>,
    buf: Vec<u8>,
    pub shape: Shape,
    /// Which published picture this is.
    pub seq: u64,
}

impl Deref for FrameRef {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.buf
    }
}

impl Drop for FrameRef {
    fn drop(&mut self) {
        self.frames.recycle(std::mem::take(&mut self.buf));
    }
}
