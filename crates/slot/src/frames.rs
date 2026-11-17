use std::os::fd::OwnedFd;
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

/// A picture as it goes from the emulator's thread to the frontend's: the pixels copied out of
/// the core's buffer, or the buffer itself when the core's own process has the frame in memory
/// another process can map.
pub struct Picture {
    bytes: Option<Vec<u8>>,
    shared: Option<SharedPicture>,
}

/// A picture left where the core's GPU put it, in memory the frontend can map and draw from:
/// which set of buffers it is in, which of them, its size, and the buffers themselves when this
/// is the first picture of that set to reach the frontend.
#[derive(Debug)]
pub struct SharedPicture {
    /// Which set of buffers the frame is in, counted up by the core each time the buffers change.
    pub generation: u64,
    /// Which of the set's buffers holds it.
    pub index: usize,
    pub width: u32,
    pub height: u32,
    /// The buffers of the set, XRGB8888 top row first, one frame each, when they are new here.
    pub fds: Option<Vec<OwnedFd>>,
}

impl Picture {
    /// The pixels, copied out of the core's buffer.
    pub fn bytes(bytes: Vec<u8>) -> Picture {
        Picture {
            bytes: Some(bytes),
            shared: None,
        }
    }

    /// The core's own buffer, left where it is.
    pub fn shared(shared: SharedPicture) -> Picture {
        Picture {
            bytes: None,
            shared: Some(shared),
        }
    }

    /// Both, for a frontend that has not said whether it can draw the buffer itself: it draws
    /// whichever it can, and the pixels are there for it not to lose the frame learning that.
    pub fn both(bytes: Vec<u8>, shared: SharedPicture) -> Picture {
        Picture {
            bytes: Some(bytes),
            shared: Some(shared),
        }
    }

    pub fn pixels(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }

    pub fn buffer(&self) -> Option<&SharedPicture> {
        self.shared.as_ref()
    }

    /// The pixels out of it, for the pool they came from.
    fn take_bytes(&mut self) -> Option<Vec<u8>> {
        self.bytes.take()
    }
}

struct Inner {
    ready: Option<(Picture, Shape, u64)>,
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

    /// Makes `picture` the one to show next, and says which picture it is, counting from 1.
    pub fn publish(&self, picture: Picture, shape: Shape) -> u64 {
        let mut i = self.lock();
        i.seq += 1;
        let seq = i.seq;
        if let Some((mut dropped, _, lost)) = i.ready.replace((picture, shape, seq)) {
            ft::instant(ev::FRAMES_OVERWRITTEN, lost as i64);
            if let Some(buf) = dropped.take_bytes() {
                i.spare.push(buf);
            }
        }
        seq
    }

    pub fn latest(self: &Arc<Self>) -> Option<FrameRef> {
        let (picture, shape, seq) = self.lock().ready.take()?;
        self.taken.fetch_add(1, Ordering::Relaxed);
        Some(FrameRef {
            frames: self.clone(),
            picture,
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
    picture: Picture,
    pub shape: Shape,
    /// Which published picture this is.
    pub seq: u64,
}

impl FrameRef {
    /// The picture's pixels, when they were copied out of the core's buffer.
    pub fn pixels(&self) -> Option<&[u8]> {
        self.picture.pixels()
    }

    /// The buffer the picture is left in, when the core's own process shares one.
    pub fn buffer(&self) -> Option<&SharedPicture> {
        self.picture.buffer()
    }
}

impl Drop for FrameRef {
    fn drop(&mut self) {
        if let Some(buf) = self.picture.take_bytes() {
            self.frames.recycle(buf);
        }
    }
}
