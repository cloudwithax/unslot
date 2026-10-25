//! Frames handed from the GPU to the CPU through memory the CPU caches. A pixel buffer the Mali's
//! driver maps is not cached, and reading a 320x240 frame out of it takes the CPU 4.5 ms, a
//! quarter of a frame. Here the frame is in a buffer from the kernel's ION allocator, cached,
//! which the GPU draws into as an EGL image: the GPU copies the frame in, turning it top row first
//! and laid out as XRGB8888 on the way, the cache is made to forget the buffer, and the CPU takes
//! the frame in 0.3 ms.
//!
//! Three buffers take turns, the frame read the one before, so the CPU never waits on the GPU
//! drawing the frame it has just been given. The third is for a frame left in its buffer for
//! another process to copy out: it has until the frame after next before the GPU draws there.

use std::ffi::{c_void, CStr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;

type Ptr = *mut c_void;

const ION_IOC_ALLOC: u32 = 0xC020_4900;
const ION_IOC_FREE: u32 = 0xC004_4901;
const ION_IOC_SHARE: u32 = 0xC008_4904;
const ION_IOC_SYNC: u32 = 0xC008_4907;
const ION_FLAG_CACHED: u32 = 1;

const EGL_WIDTH: i32 = 0x3057;
const EGL_HEIGHT: i32 = 0x3056;
const EGL_NONE: i32 = 0x3038;
const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: i32 = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: i32 = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: i32 = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: i32 = 0x3274;
/// DRM's XRGB8888, bytes blue, green, red, unused: the layout slot keeps frames in.
const DRM_FORMAT_XRGB8888: i32 = i32::from_le_bytes(*b"XR24");

const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_TEXTURE_BINDING_2D: u32 = 0x8069;
const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_READ_FRAMEBUFFER: u32 = 0x8CA8;
const GL_DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
const GL_NEAREST: u32 = 0x2600;
const GL_SCISSOR_TEST: u32 = 0x0C11;
const GL_SYNC_GPU_COMMANDS_COMPLETE: u32 = 0x9117;
const GL_SYNC_FLUSH_COMMANDS_BIT: u32 = 1;
/// How many buffers take turns.
const BUFFERS: usize = 3;
/// How long to wait on the GPU for a frame it should have drawn a frame ago.
const WAIT_NS: u64 = 100_000_000;
const EGL_SYNC_FENCE_KHR: u32 = 0x30F9;

#[repr(C)]
struct IonAllocation {
    len: usize,
    align: usize,
    heap_id_mask: u32,
    flags: u32,
    handle: i32,
}

#[repr(C)]
struct IonFd {
    handle: i32,
    fd: i32,
}

struct Fns {
    create_image: unsafe extern "C" fn(Ptr, Ptr, u32, Ptr, *const i32) -> Ptr,
    destroy_image: unsafe extern "C" fn(Ptr, Ptr) -> u32,
    image_target_texture: unsafe extern "C" fn(u32, Ptr),
    gen_textures: unsafe extern "C" fn(i32, *mut u32),
    delete_textures: unsafe extern "C" fn(i32, *const u32),
    bind_texture: unsafe extern "C" fn(u32, u32),
    get_integerv: unsafe extern "C" fn(u32, *mut i32),
    gen_framebuffers: unsafe extern "C" fn(i32, *mut u32),
    delete_framebuffers: unsafe extern "C" fn(i32, *const u32),
    bind_framebuffer: unsafe extern "C" fn(u32, u32),
    framebuffer_texture: unsafe extern "C" fn(u32, u32, u32, u32, i32),
    check_framebuffer_status: unsafe extern "C" fn(u32) -> u32,
    blit: unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32, i32, u32, u32),
    is_enabled: unsafe extern "C" fn(u32) -> u8,
    enable: unsafe extern "C" fn(u32),
    disable: unsafe extern "C" fn(u32),
    flush: unsafe extern "C" fn(),
    fence_sync: unsafe extern "C" fn(u32, u32) -> Ptr,
    client_wait_sync: unsafe extern "C" fn(Ptr, u32, u64) -> u32,
    delete_sync: unsafe extern "C" fn(Ptr),
}

/// EGL's fences, which unlike GL's can be waited on from a thread with no context.
#[derive(Clone, Copy)]
struct EglSync {
    create: unsafe extern "C" fn(Ptr, u32, *const i32) -> Ptr,
    client_wait: unsafe extern "C" fn(Ptr, Ptr, i32, u64) -> i32,
    destroy: unsafe extern "C" fn(Ptr, Ptr) -> u32,
}

/// A buffer's memory as the CPU maps it, kept while anything may read it.
struct Mapping {
    fd: OwnedFd,
    map: *const u8,
    len: usize,
}

// SAFETY: the mapping is only read, and only once the GPU is done writing it.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: mapped once, by `buffer`, and unmapped here once.
        unsafe { libc::munmap(self.map as *mut c_void, self.len) };
    }
}

/// One buffer: the ION memory, the CPU's mapping of it, and the GPU's image of it to draw into.
struct Buffer {
    /// Shared with a frame lent out of it, until that is done with.
    mem: Arc<Mapping>,
    image: Ptr,
    texture: u32,
    fbo: u32,
    /// Signalled once the GPU has copied the frame in; None before it is first given one.
    fence: Option<Ptr>,
}

pub(crate) struct IonFrames {
    ion: Arc<OwnedFd>,
    display: Ptr,
    fns: Fns,
    sync: Option<EglSync>,
    size: (u32, u32),
    buffers: Vec<Buffer>,
    turn: usize,
    /// Set when the buffers are made, until `share` has handed them out.
    fresh: bool,
}

impl IonFrames {
    /// Ready for frames on the context current on this thread, or why not. `proc` gives GL and
    /// EGL functions by name.
    pub(crate) fn new(display: Ptr, proc: impl Fn(&CStr) -> *const c_void) -> Result<Self, String> {
        // SAFETY: each is named for the function its field types it as.
        unsafe fn fun<T>(p: *const c_void) -> T {
            std::mem::transmute_copy(&p)
        }
        let f = |name: &CStr| {
            let p = proc(name);
            match p.is_null() {
                true => Err(format!("no {}", name.to_string_lossy())),
                false => Ok(p),
            }
        };
        unsafe {
            let fns = Fns {
                create_image: fun(f(c"eglCreateImageKHR")?),
                destroy_image: fun(f(c"eglDestroyImageKHR")?),
                image_target_texture: fun(f(c"glEGLImageTargetTexture2DOES")?),
                gen_textures: fun(f(c"glGenTextures")?),
                delete_textures: fun(f(c"glDeleteTextures")?),
                bind_texture: fun(f(c"glBindTexture")?),
                get_integerv: fun(f(c"glGetIntegerv")?),
                gen_framebuffers: fun(f(c"glGenFramebuffers")?),
                delete_framebuffers: fun(f(c"glDeleteFramebuffers")?),
                bind_framebuffer: fun(f(c"glBindFramebuffer")?),
                framebuffer_texture: fun(f(c"glFramebufferTexture2D")?),
                check_framebuffer_status: fun(f(c"glCheckFramebufferStatus")?),
                blit: fun(f(c"glBlitFramebuffer")?),
                is_enabled: fun(f(c"glIsEnabled")?),
                enable: fun(f(c"glEnable")?),
                disable: fun(f(c"glDisable")?),
                flush: fun(f(c"glFlush")?),
                fence_sync: fun(f(c"glFenceSync")?),
                client_wait_sync: fun(f(c"glClientWaitSync")?),
                delete_sync: fun(f(c"glDeleteSync")?),
            };
            let sync = (|| {
                Ok::<_, String>(EglSync {
                    create: fun(f(c"eglCreateSyncKHR")?),
                    client_wait: fun(f(c"eglClientWaitSyncKHR")?),
                    destroy: fun(f(c"eglDestroySyncKHR")?),
                })
            })()
            .ok();
            let fd = libc::open(c"/dev/ion".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
            if fd < 0 {
                return Err(format!("/dev/ion: {}", std::io::Error::last_os_error()));
            }
            Ok(IonFrames {
                ion: Arc::new(OwnedFd::from_raw_fd(fd)),
                display,
                fns,
                sync,
                size: (0, 0),
                buffers: Vec::new(),
                turn: 0,
                fresh: false,
            })
        }
    }

    /// Ready for frames on the EGL context current on this thread, its display and functions
    /// found through libEGL and libGLESv2 themselves, as the compositor keeps neither.
    pub(crate) fn for_current() -> Result<Self, String> {
        let open = |names: [&str; 2]| {
            unsafe { libloading::Library::new(names[0]).or_else(|_| libloading::Library::new(names[1])) }
                .map_err(|e| e.to_string())
        };
        let egl = open(["libEGL.so.1", "libEGL.so"])?;
        let gles = open(["libGLESv2.so.2", "libGLESv2.so"])?;
        // SAFETY: each is named for the function it is typed as.
        let made = unsafe {
            let display: unsafe extern "C" fn() -> Ptr =
                *egl.get(b"eglGetCurrentDisplay").map_err(|e| e.to_string())?;
            let get_proc: unsafe extern "C" fn(*const std::ffi::c_char) -> *const c_void =
                *egl.get(b"eglGetProcAddress").map_err(|e| e.to_string())?;
            let display = display();
            if display.is_null() {
                return Err("no EGL display current".into());
            }
            IonFrames::new(display, |name| {
                match gles.get::<unsafe extern "C" fn()>(name.to_bytes()) {
                    Ok(f) => *f as *const c_void,
                    Err(_) => get_proc(name.as_ptr()),
                }
            })
        };
        // The context they serve keeps both loaded; letting go would only drop a reference.
        std::mem::forget(egl);
        std::mem::forget(gles);
        made
    }

    /// The `w` by `h` frame in framebuffer `from` into `out` as XRGB8888, top row first, turned
    /// over when its bottom row is first; the frame before it once there is one, as the pixel
    /// buffers give. Leaves `from` bound. False when the buffers cannot be had, the frame unread.
    pub(crate) fn read(&mut self, from: u32, w: u32, h: u32, flip: bool, out: &mut [u8]) -> bool {
        let Some(take) = self.turn_in(from, w, h, flip) else {
            return false;
        };
        let b = &self.buffers[take].mem;
        let n = out.len().min(b.len);
        // SAFETY: the mapping is `b.len` long, and the GPU is done with it.
        unsafe { std::ptr::copy_nonoverlapping(b.map, out.as_mut_ptr(), n) };
        true
    }

    /// Whether frames can be lent, to be waited on and read on another thread.
    pub(crate) fn can_lend(&self) -> bool {
        self.sync.is_some()
    }

    /// Has the GPU copy the frame in framebuffer `from` into a buffer not lent out, as `read`
    /// does, and lends it, for whoever has it to wait on and read while the GPU goes on. Leaves
    /// `from` bound. None when every buffer is lent out; Err when the buffers cannot be had.
    pub(crate) fn lend(
        &mut self,
        from: u32,
        w: u32,
        h: u32,
        flip: bool,
    ) -> Result<Option<LentFrame>, ()> {
        let sync = self.sync.ok_or(())?;
        if !self.ready_for(w, h) {
            return Err(());
        }
        let Some(i) = self.buffers.iter().position(|b| Arc::strong_count(&b.mem) == 1) else {
            return Ok(None);
        };
        self.copy_in(from, i, w, h, flip);
        // SAFETY: the display is live; the fence follows the copy, flushed for it to be done.
        let fence = unsafe {
            let fence = (sync.create)(self.display, EGL_SYNC_FENCE_KHR, std::ptr::null());
            (self.fns.flush)();
            (self.fns.bind_framebuffer)(GL_FRAMEBUFFER, from);
            fence
        };
        if fence.is_null() {
            return Ok(None);
        }
        Ok(Some(LentFrame {
            mem: self.buffers[i].mem.clone(),
            ion: self.ion.clone(),
            display: self.display,
            sync,
            fence,
            waited: false,
        }))
    }

    /// As `read`, leaving the frame in its buffer: which of them it is in, with the buffers to map
    /// when they are new since last asked. None when the buffers cannot be had.
    pub(crate) fn share(
        &mut self,
        from: u32,
        w: u32,
        h: u32,
        flip: bool,
    ) -> Option<(usize, Option<Vec<OwnedFd>>)> {
        let take = self.turn_in(from, w, h, flip)?;
        let fds = std::mem::take(&mut self.fresh)
            .then(|| self.buffers.iter().map(|b| b.mem.fd.try_clone()).collect())
            .and_then(|fds: std::io::Result<Vec<_>>| fds.ok());
        Some((take, fds))
    }

    /// Has the GPU copy the frame in framebuffer `from` into the next buffer, and gives the one
    /// holding the frame before, done and out of the cache, to read.
    fn turn_in(&mut self, from: u32, w: u32, h: u32, flip: bool) -> Option<usize> {
        if !self.ready_for(w, h) {
            return None;
        }
        self.copy_in(from, self.turn, w, h, flip);
        let f = &self.fns;
        unsafe {
            let this = &mut self.buffers[self.turn];
            if let Some(old) = this.fence.take() {
                (f.delete_sync)(old);
            }
            this.fence = Some((f.fence_sync)(GL_SYNC_GPU_COMMANDS_COMPLETE, 0));
            (f.flush)();
            (f.bind_framebuffer)(GL_FRAMEBUFFER, from);
            let last = (self.turn + BUFFERS - 1) % BUFFERS;
            let take = match self.buffers[last].fence {
                Some(_) => last,
                None => self.turn,
            };
            self.turn = (self.turn + 1) % BUFFERS;
            let b = &self.buffers[take];
            if let Some(fence) = b.fence {
                (f.client_wait_sync)(fence, GL_SYNC_FLUSH_COMMANDS_BIT, WAIT_NS);
            }
            // The GPU wrote behind the cache's back: have it drop what it holds of the buffer.
            let mut sync = IonFd {
                handle: 0,
                fd: b.mem.fd.as_raw_fd(),
            };
            libc::ioctl(self.ion.as_raw_fd(), ION_IOC_SYNC as _, &mut sync);
            Some(take)
        }
    }

    /// Buffers for `w` by `h` frames, made again when the size has changed. False when they
    /// cannot be had.
    fn ready_for(&mut self, w: u32, h: u32) -> bool {
        if self.size == (w, h) {
            return true;
        }
        self.release();
        for _ in 0..BUFFERS {
            match self.buffer(w, h) {
                Ok(b) => self.buffers.push(b),
                Err(e) => {
                    eprintln!("slot: gpu core: no cached frames {w}x{h}: {e}");
                    self.release();
                    return false;
                }
            }
        }
        self.size = (w, h);
        self.turn = 0;
        self.fresh = true;
        true
    }

    /// Has the GPU copy the `w` by `h` frame in framebuffer `from` into buffer `i`.
    fn copy_in(&self, from: u32, i: usize, w: u32, h: u32, flip: bool) {
        let f = &self.fns;
        unsafe {
            (f.bind_framebuffer)(GL_READ_FRAMEBUFFER, from);
            (f.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, self.buffers[i].fbo);
            // A blit is cut by the scissor, which a core may leave on.
            let scissor = (f.is_enabled)(GL_SCISSOR_TEST) != 0;
            if scissor {
                (f.disable)(GL_SCISSOR_TEST);
            }
            let (w, h) = (w as i32, h as i32);
            let (y0, y1) = if flip { (h, 0) } else { (0, h) };
            (f.blit)(0, 0, w, h, 0, y0, w, y1, GL_COLOR_BUFFER_BIT, GL_NEAREST);
            if scissor {
                (f.enable)(GL_SCISSOR_TEST);
            }
        }
    }

    fn buffer(&self, w: u32, h: u32) -> Result<Buffer, String> {
        let len = (w * h * 4) as usize;
        let f = &self.fns;
        let os = || std::io::Error::last_os_error().to_string();
        unsafe {
            let mut alloc = IonAllocation {
                len,
                align: 4096,
                heap_id_mask: 0,
                flags: ION_FLAG_CACHED,
                handle: 0,
            };
            // The heaps are numbered as each kernel likes; the first that gives cached memory.
            let found = (0..16).any(|bit| {
                alloc.heap_id_mask = 1 << bit;
                libc::ioctl(self.ion.as_raw_fd(), ION_IOC_ALLOC as _, &mut alloc) == 0
            });
            if !found {
                return Err(format!("ION alloc: {}", os()));
            }
            let mut share = IonFd {
                handle: alloc.handle,
                fd: -1,
            };
            let shared = libc::ioctl(self.ion.as_raw_fd(), ION_IOC_SHARE as _, &mut share);
            // The shared fd holds the memory from here on.
            let mut free = alloc.handle;
            libc::ioctl(self.ion.as_raw_fd(), ION_IOC_FREE as _, &mut free);
            if shared != 0 {
                return Err(format!("ION share: {}", os()));
            }
            let fd = OwnedFd::from_raw_fd(share.fd);
            let map = libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            );
            if map == libc::MAP_FAILED {
                return Err(format!("mmap: {}", os()));
            }
            let attrs = [
                EGL_WIDTH,
                w as i32,
                EGL_HEIGHT,
                h as i32,
                EGL_LINUX_DRM_FOURCC_EXT,
                DRM_FORMAT_XRGB8888,
                EGL_DMA_BUF_PLANE0_FD_EXT,
                fd.as_raw_fd(),
                EGL_DMA_BUF_PLANE0_OFFSET_EXT,
                0,
                EGL_DMA_BUF_PLANE0_PITCH_EXT,
                (w * 4) as i32,
                EGL_NONE,
            ];
            let image = (f.create_image)(
                self.display,
                std::ptr::null_mut(),
                EGL_LINUX_DMA_BUF_EXT,
                std::ptr::null_mut(),
                attrs.as_ptr(),
            );
            let mut b = Buffer {
                mem: Arc::new(Mapping {
                    fd,
                    map: map as *const u8,
                    len,
                }),
                image,
                texture: 0,
                fbo: 0,
                fence: None,
            };
            if image.is_null() {
                self.free(&mut b);
                return Err("eglCreateImageKHR".into());
            }
            // The core's texture stays bound as it was.
            let mut bound = 0;
            (f.get_integerv)(GL_TEXTURE_BINDING_2D, &mut bound);
            (f.gen_textures)(1, &mut b.texture);
            (f.bind_texture)(GL_TEXTURE_2D, b.texture);
            (f.image_target_texture)(GL_TEXTURE_2D, image);
            (f.bind_texture)(GL_TEXTURE_2D, bound as u32);
            (f.gen_framebuffers)(1, &mut b.fbo);
            (f.bind_framebuffer)(GL_FRAMEBUFFER, b.fbo);
            (f.framebuffer_texture)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                b.texture,
                0,
            );
            let status = (f.check_framebuffer_status)(GL_FRAMEBUFFER);
            if status != GL_FRAMEBUFFER_COMPLETE {
                self.free(&mut b);
                return Err(format!("framebuffer incomplete ({status:#x})"));
            }
            Ok(b)
        }
    }

    fn free(&self, b: &mut Buffer) {
        let f = &self.fns;
        unsafe {
            if let Some(fence) = b.fence.take() {
                (f.delete_sync)(fence);
            }
            if b.fbo != 0 {
                (f.delete_framebuffers)(1, &b.fbo);
            }
            if b.texture != 0 {
                (f.delete_textures)(1, &b.texture);
            }
            if !b.image.is_null() {
                (f.destroy_image)(self.display, b.image);
            }
        }
    }

    fn release(&mut self) {
        for mut b in std::mem::take(&mut self.buffers) {
            self.free(&mut b);
        }
        self.size = (0, 0);
    }
}

impl Drop for IonFrames {
    fn drop(&mut self) {
        self.release();
    }
}

/// A frame lent out of its buffer, which goes back to the GPU once this is dropped.
pub(crate) struct LentFrame {
    mem: Arc<Mapping>,
    ion: Arc<OwnedFd>,
    display: Ptr,
    sync: EglSync,
    fence: Ptr,
    waited: bool,
}

// SAFETY: EGL's fences are the display's, waited on and let go from any thread.
unsafe impl Send for LentFrame {}

impl crate::cast_tap::Lent for LentFrame {
    fn pixels(&mut self) -> &[u8] {
        if !std::mem::replace(&mut self.waited, true) {
            // SAFETY: the fence and the display are live; the flush was done when it was made.
            unsafe {
                (self.sync.client_wait)(self.display, self.fence, 0, WAIT_NS);
                // The GPU wrote behind the cache's back: have it drop what it holds of the buffer.
                let mut sync = IonFd {
                    handle: 0,
                    fd: self.mem.fd.as_raw_fd(),
                };
                libc::ioctl(self.ion.as_raw_fd(), ION_IOC_SYNC as _, &mut sync);
            }
        }
        // SAFETY: the mapping is that long, kept while this has it, and the GPU is done with it.
        unsafe { std::slice::from_raw_parts(self.mem.map, self.mem.len) }
    }
}

impl Drop for LentFrame {
    fn drop(&mut self) {
        // SAFETY: made for this frame, let go once.
        unsafe { (self.sync.destroy)(self.display, self.fence) };
    }
}
