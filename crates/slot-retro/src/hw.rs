//! Cores that draw with the GPU (Nintendo 64, Dreamcast, PSP) ask for an OpenGL ES context in
//! place of handing over pixels. They get one of their own on the thread that runs them, draw
//! into a framebuffer slot keeps in it, and each finished frame is read back into the same
//! buffer a software core fills, so everything past the core is the same for both.

use std::ffi::{c_char, c_uint, c_void, CStr};
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use libloading::Library;

pub const SET_HW_RENDER: c_uint = 14;
pub const GET_PREFERRED_HW_RENDER: c_uint = 56;
pub const SET_HW_SHARED_CONTEXT: c_uint = 44 | 0x10000;

pub const CONTEXT_OPENGLES2: c_uint = 2;
pub const CONTEXT_OPENGLES3: c_uint = 4;
pub const CONTEXT_OPENGLES_VERSION: c_uint = 5;

/// What `video_refresh` is handed in place of pixels when the frame is in the framebuffer.
pub const FRAME_IN_FRAMEBUFFER: usize = usize::MAX;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HwRenderCallback {
    pub context_type: c_uint,
    pub context_reset: Option<unsafe extern "C" fn()>,
    pub get_current_framebuffer: Option<unsafe extern "C" fn() -> usize>,
    pub get_proc_address: Option<unsafe extern "C" fn(*const c_char) -> *const c_void>,
    pub depth: bool,
    pub stencil: bool,
    pub bottom_left_origin: bool,
    pub version_major: c_uint,
    pub version_minor: c_uint,
    pub cache_context: bool,
    pub context_destroy: Option<unsafe extern "C" fn()>,
    pub debug_context: bool,
}

/// An OpenGL ES context current on the thread that made it, with a framebuffer for a core. It is
/// made while the core loads, on the thread that goes on to run it, and used on no other; `Send`
/// is only so the core can be handed to that thread before.
pub trait GlContext: Send {
    /// The framebuffer the core draws into, holding at least `w` by `h`, made afresh when it
    /// does not.
    fn framebuffer(&mut self, w: u32, h: u32, depth: bool, stencil: bool) -> u32;
    /// The bottom-left `w` by `h` of the framebuffer as RGBA, its bottom row first.
    fn read(&mut self, w: u32, h: u32, out: &mut Vec<u8>);
    /// The same frame straight into `out` as XRGB8888, top row first, turned over when
    /// `bottom_left_origin`, when the context can have the GPU lay it out so; false leaves it to
    /// `read`.
    fn read_xrgb(&mut self, _w: u32, _h: u32, _bottom_left_origin: bool, _out: &mut [u8]) -> bool {
        false
    }
    /// As `read_xrgb`, leaving the frame where the GPU put it, in memory another process can
    /// map: which of the buffers it is in, with the buffers when they are new since last asked.
    /// None leaves it to `read_xrgb`.
    fn share_xrgb(
        &mut self,
        _w: u32,
        _h: u32,
        _bottom_left_origin: bool,
    ) -> Option<(usize, Option<Vec<OwnedFd>>)> {
        None
    }
}

/// Makes a context on the calling thread: GLES 3 when asked and to be had, else GLES 2.
pub type GlFactory = fn(gles3: bool) -> Option<Box<dyn GlContext>>;

static FACTORY: OnceLock<GlFactory> = OnceLock::new();

/// Where GPU cores get their context. Without one they are refused, as on a desktop build.
pub fn set_gl_factory(factory: GlFactory) {
    let _ = FACTORY.set(factory);
}

pub(crate) fn factory() -> Option<GlFactory> {
    FACTORY.get().copied()
}

/// The framebuffer the running GPU core draws into. A core may ask from a thread of its own, so
/// it is kept here rather than with the host, which only the thread calling the core can see.
static FRAMEBUFFER: AtomicU32 = AtomicU32::new(0);

pub(crate) fn current_framebuffer() -> usize {
    FRAMEBUFFER.load(Ordering::Acquire) as usize
}

type GetProc = unsafe extern "C" fn(*const c_char) -> *const c_void;

/// A GL function for a core, from any thread: from the GLES library first, as some drivers'
/// eglGetProcAddress gives only extensions, then from EGL.
pub(crate) fn proc_address(name: &CStr) -> *const c_void {
    static LIBS: OnceLock<Option<(Library, Library)>> = OnceLock::new();
    let libs = LIBS.get_or_init(|| {
        let open = |names: [&str; 2]| names.iter().find_map(|n| unsafe { Library::new(n) }.ok());
        Some((
            open(["libGLESv2.so.2", "libGLESv2.so"])?,
            open(["libEGL.so.1", "libEGL.so"])?,
        ))
    });
    let Some((gles, egl)) = libs else {
        return std::ptr::null();
    };
    unsafe {
        if let Ok(f) = gles.get::<unsafe extern "C" fn()>(name.to_bytes()) {
            return *f as *const c_void;
        }
        match egl.get::<GetProc>(b"eglGetProcAddress") {
            Ok(get) => get(name.as_ptr()),
            Err(_) => std::ptr::null(),
        }
    }
}

/// A GPU core's context and what it asked for.
pub(crate) struct Hw {
    pub callback: HwRenderCallback,
    pub gl: Box<dyn GlContext>,
    pub fbo: u32,
    pub readback: Vec<u8>,
    /// Frames read back, for SLOT_GL_DEBUG's occasional word on them.
    pub frames: u64,
    /// Set once frames are to be left where the GPU puts them, for another process to map.
    pub share: bool,
    /// Which buffer the last frame was left in, while they are.
    pub shared: Option<usize>,
    /// The buffers, when they are new since last handed out.
    pub buffers: Option<Vec<OwnedFd>>,
}

impl Hw {
    /// Sizes the framebuffer for the largest picture the core says it will draw.
    pub fn fit(&mut self, w: u32, h: u32) {
        let c = &self.callback;
        self.fbo = self.gl.framebuffer(w.max(1), h.max(1), c.depth, c.stencil);
        FRAMEBUFFER.store(self.fbo, Ordering::Release);
    }

    /// The core's last frame into `video` as XRGB8888, top row first, the way a software core's
    /// frames are kept.
    pub fn read_into(&mut self, w: u32, h: u32, video: &mut [u8]) {
        self.frames += 1;
        if self.share {
            let origin = self.callback.bottom_left_origin;
            if let Some((index, buffers)) = self.gl.share_xrgb(w, h, origin) {
                self.shared = Some(index);
                if buffers.is_some() {
                    self.buffers = buffers;
                }
                return;
            }
            self.share = false;
        }
        self.shared = None;
        let size = (w * h * 4) as usize;
        if self
            .gl
            .read_xrgb(w, h, self.callback.bottom_left_origin, &mut video[..size])
        {
            return;
        }
        self.gl.read(w, h, &mut self.readback);
        if self.frames % 120 == 1 && std::env::var_os("SLOT_GL_DEBUG").is_some() {
            let lit: u64 = self.readback.iter().map(|&b| u64::from(b)).sum();
            eprintln!(
                "slot-retro: gpu core: frame {} {w}x{h}, mean {}",
                self.frames,
                lit / self.readback.len().max(1) as u64
            );
        }
        let row = (w * 4) as usize;
        for y in 0..h as usize {
            let from = match self.callback.bottom_left_origin {
                true => h as usize - 1 - y,
                false => y,
            };
            let src = &self.readback[from * row..from * row + row];
            let dst = &mut video[y * row..y * row + row];
            for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d.copy_from_slice(&[s[2], s[1], s[0], 0]);
            }
        }
    }
}

/// Whether slot can give a core the context `c` asks for, and if so whether it is GLES 3. A core
/// asking for GLES 2 is given 3 all the same, as RetroArch built for GLES 3 does: GLES 3 is GLES
/// 2 and more, the cores look at what they got to use the more, and on the Mali the RGBA8
/// framebuffer it brings reads back four times as fast as GLES 2's RGB565 one. N64 went from 28
/// fps to 101, PSP from 14 to 68.
pub(crate) fn wants_gles3(c: &HwRenderCallback) -> Option<bool> {
    match c.context_type {
        CONTEXT_OPENGLES2 | CONTEXT_OPENGLES3 | CONTEXT_OPENGLES_VERSION => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        frame: Vec<u8>,
    }

    impl GlContext for Fake {
        fn framebuffer(&mut self, _w: u32, _h: u32, _depth: bool, _stencil: bool) -> u32 {
            7
        }
        fn read(&mut self, _w: u32, _h: u32, out: &mut Vec<u8>) {
            out.clone_from(&self.frame);
        }
    }

    fn callback(bottom_left_origin: bool) -> HwRenderCallback {
        HwRenderCallback {
            context_type: CONTEXT_OPENGLES2,
            context_reset: None,
            get_current_framebuffer: None,
            get_proc_address: None,
            depth: true,
            stencil: true,
            bottom_left_origin,
            version_major: 0,
            version_minor: 0,
            cache_context: false,
            context_destroy: None,
            debug_context: false,
        }
    }

    #[test]
    fn a_gl_frame_comes_back_top_row_first_as_xrgb() {
        // Two rows of one pixel: red at the bottom, as GL reads it first, then blue.
        let mut hw = Hw {
            callback: callback(true),
            gl: Box::new(Fake {
                frame: vec![255, 0, 0, 255, 0, 0, 255, 255],
            }),
            fbo: 0,
            readback: Vec::new(),
            frames: 0,
            share: false,
            shared: None,
            buffers: None,
        };
        let mut video = vec![0u8; 8];
        hw.read_into(1, 2, &mut video);
        assert_eq!(video, [255, 0, 0, 0, 0, 0, 255, 0], "blue on top, then red");
    }

    #[test]
    fn only_gles_is_offered() {
        let mut c = callback(true);
        assert_eq!(wants_gles3(&c), Some(true), "GLES 2 is given 3");
        c.context_type = CONTEXT_OPENGLES3;
        assert_eq!(wants_gles3(&c), Some(true));
        c.context_type = CONTEXT_OPENGLES_VERSION;
        c.version_major = 2;
        assert_eq!(wants_gles3(&c), Some(true));
        // Desktop GL, its core profile and Vulkan are not to be had on the device.
        for t in [1, 3, 6] {
            c.context_type = t;
            assert_eq!(wants_gles3(&c), None, "context type {t}");
        }
    }
}
