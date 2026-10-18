//! An OpenGL ES context with no window, for a core that draws with the GPU on its own thread.
//! It has a framebuffer the core draws into and reads each frame back as RGBA. Its GL calls go
//! through pointers of its own, not the `gl` crate's, which belong to the compositor's context.

use std::ffi::{c_char, c_void, CStr};
use std::os::fd::OwnedFd;

use libloading::Library;

use crate::ion::IonFrames;
use crate::surface::GfxError;

const EGL_NONE: i32 = 0x3038;
const EGL_RED_SIZE: i32 = 0x3024;
const EGL_GREEN_SIZE: i32 = 0x3023;
const EGL_BLUE_SIZE: i32 = 0x3022;
const EGL_ALPHA_SIZE: i32 = 0x3021;
const EGL_DEPTH_SIZE: i32 = 0x3025;
const EGL_STENCIL_SIZE: i32 = 0x3026;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_WIDTH: i32 = 0x3057;
const EGL_HEIGHT: i32 = 0x3056;
const EGL_PBUFFER_BIT: i32 = 0x0001;
const EGL_OPENGL_ES2_BIT: i32 = 0x0004;
const EGL_OPENGL_ES3_BIT: i32 = 0x0040;
const EGL_OPENGL_ES_API: u32 = 0x30A0;
const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
const EGL_PLATFORM_SURFACELESS_MESA: u32 = 0x31DD;

const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_RENDERBUFFER: u32 = 0x8D41;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_DEPTH_ATTACHMENT: u32 = 0x8D00;
const GL_STENCIL_ATTACHMENT: u32 = 0x8D20;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_RGBA8: u32 = 0x8058;
const GL_RGB565: u32 = 0x8D62;
const GL_DEPTH24_STENCIL8: u32 = 0x88F0;
const GL_DEPTH_COMPONENT16: u32 = 0x81A5;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_PACK_ALIGNMENT: u32 = 0x0D05;
const GL_PIXEL_PACK_BUFFER: u32 = 0x88EB;
const GL_STREAM_READ: u32 = 0x88E1;
const GL_MAP_READ_BIT: u32 = 0x0001;

type Ptr = *mut c_void;

struct Gl {
    gen_framebuffers: unsafe extern "C" fn(i32, *mut u32),
    delete_framebuffers: unsafe extern "C" fn(i32, *const u32),
    bind_framebuffer: unsafe extern "C" fn(u32, u32),
    gen_renderbuffers: unsafe extern "C" fn(i32, *mut u32),
    delete_renderbuffers: unsafe extern "C" fn(i32, *const u32),
    bind_renderbuffer: unsafe extern "C" fn(u32, u32),
    renderbuffer_storage: unsafe extern "C" fn(u32, u32, i32, i32),
    framebuffer_renderbuffer: unsafe extern "C" fn(u32, u32, u32, u32),
    check_framebuffer_status: unsafe extern "C" fn(u32) -> u32,
    read_pixels: unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut c_void),
    pixel_storei: unsafe extern "C" fn(u32, i32),
    clear_color: unsafe extern "C" fn(f32, f32, f32, f32),
    clear: unsafe extern "C" fn(u32),
    pack: Option<Pack>,
}

/// GLES 3's pixel buffers, which a frame is read into by the GPU while the CPU takes the one
/// before. Reading the framebuffer straight into client memory instead makes the CPU wait for the
/// GPU to finish first: on the Mali a 320x240 frame takes 5 ms this way and 10 that, 640x480 29
/// and 37, most of it the CPU reading memory the driver does not cache.
struct Pack {
    gen_buffers: unsafe extern "C" fn(i32, *mut u32),
    delete_buffers: unsafe extern "C" fn(i32, *const u32),
    bind_buffer: unsafe extern "C" fn(u32, u32),
    buffer_data: unsafe extern "C" fn(u32, isize, *const c_void, u32),
    map_buffer_range: unsafe extern "C" fn(u32, isize, isize, u32) -> *mut c_void,
    unmap_buffer: unsafe extern "C" fn(u32) -> u8,
}

/// A GLES context current on the thread that made it, with no surface to show.
pub struct OffscreenGl {
    display: Ptr,
    surface: Ptr,
    context: Ptr,
    gles3: bool,
    make_current: unsafe extern "C" fn(Ptr, Ptr, Ptr, Ptr) -> u32,
    get_proc_address: unsafe extern "C" fn(*const c_char) -> *const c_void,
    destroy_context: unsafe extern "C" fn(Ptr, Ptr) -> u32,
    destroy_surface: unsafe extern "C" fn(Ptr, Ptr) -> u32,
    gl: Gl,
    fbo: u32,
    buffers: [u32; 2],
    size: (u32, u32),
    /// Two pixel buffers taking turns, and the size of the frame read into each, if any.
    pbos: [u32; 2],
    pending: [Option<(u32, u32)>; 2],
    turn: usize,
    /// Cached buffers frames go out through when the kernel and driver have them, made at the
    /// first frame; `ion_tried` once that has been tried.
    ion: Option<IonFrames>,
    ion_tried: bool,
    gles: Library,
    _egl: Library,
}

// The context is made and used on one thread, the core's; it is only moved before it is made.
unsafe impl Send for OffscreenGl {}

fn open(name: &str) -> Result<Library, GfxError> {
    unsafe { Library::new(name) }.map_err(|e| GfxError::Context(format!("{name}: {e}")))
}

unsafe fn sym<T: Copy>(lib: &Library, name: &str) -> Result<T, GfxError> {
    lib.get::<T>(name.as_bytes())
        .map(|s| *s)
        .map_err(|e| GfxError::Context(format!("{name}: {e}")))
}

impl OffscreenGl {
    /// A GLES 3 context when `gles3` and the driver has one, else GLES 2, current on this thread.
    pub fn new(gles3: bool) -> Result<Self, GfxError> {
        let egl = open("libEGL.so.1").or_else(|_| open("libEGL.so"))?;
        let gles = open("libGLESv2.so.2").or_else(|_| open("libGLESv2.so"))?;
        unsafe {
            let get_display: unsafe extern "C" fn(Ptr) -> Ptr = sym(&egl, "eglGetDisplay")?;
            let initialize: unsafe extern "C" fn(Ptr, *mut i32, *mut i32) -> u32 =
                sym(&egl, "eglInitialize")?;
            let bind_api: unsafe extern "C" fn(u32) -> u32 = sym(&egl, "eglBindAPI")?;
            let choose_config: unsafe extern "C" fn(
                Ptr,
                *const i32,
                *mut Ptr,
                i32,
                *mut i32,
            ) -> u32 = sym(&egl, "eglChooseConfig")?;
            let create_pbuffer: unsafe extern "C" fn(Ptr, Ptr, *const i32) -> Ptr =
                sym(&egl, "eglCreatePbufferSurface")?;
            let create_context: unsafe extern "C" fn(Ptr, Ptr, Ptr, *const i32) -> Ptr =
                sym(&egl, "eglCreateContext")?;
            let get_error: unsafe extern "C" fn() -> i32 = sym(&egl, "eglGetError")?;
            let fail = |what: &str| crate::egl_error(what, get_error());

            // The device's driver has one display, the default. Mesa, which the tests run on, has
            // none without a window system but for its surfaceless platform.
            let mut display = get_display(std::ptr::null_mut());
            if display.is_null()
                || initialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0
            {
                let platform: Result<unsafe extern "C" fn(u32, Ptr, *const isize) -> Ptr, _> =
                    sym(&egl, "eglGetPlatformDisplay");
                display = match platform {
                    Ok(get) => get(
                        EGL_PLATFORM_SURFACELESS_MESA,
                        std::ptr::null_mut(),
                        std::ptr::null(),
                    ),
                    Err(_) => std::ptr::null_mut(),
                };
                if display.is_null()
                    || initialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0
                {
                    return Err(fail("eglInitialize"));
                }
            }
            if bind_api(EGL_OPENGL_ES_API) == 0 {
                return Err(fail("eglBindAPI"));
            }
            let config_for = |bit: i32| {
                let attrs = [
                    EGL_SURFACE_TYPE,
                    EGL_PBUFFER_BIT,
                    EGL_RED_SIZE,
                    8,
                    EGL_GREEN_SIZE,
                    8,
                    EGL_BLUE_SIZE,
                    8,
                    EGL_ALPHA_SIZE,
                    8,
                    EGL_DEPTH_SIZE,
                    16,
                    EGL_STENCIL_SIZE,
                    8,
                    EGL_RENDERABLE_TYPE,
                    bit,
                    EGL_NONE,
                ];
                let mut config: Ptr = std::ptr::null_mut();
                let mut found = 0;
                let ok = choose_config(display, attrs.as_ptr(), &mut config, 1, &mut found);
                (ok != 0 && found > 0).then_some(config)
            };
            let size = [EGL_WIDTH, 16, EGL_HEIGHT, 16, EGL_NONE];
            let make = |version: i32, bit: i32| {
                let config = config_for(bit)?;
                let surface = create_pbuffer(display, config, size.as_ptr());
                if surface.is_null() {
                    return None;
                }
                let attrs = [EGL_CONTEXT_CLIENT_VERSION, version, EGL_NONE];
                let context = create_context(display, config, std::ptr::null_mut(), attrs.as_ptr());
                (!context.is_null()).then_some((surface, context))
            };
            let made = match gles3 {
                true => make(3, EGL_OPENGL_ES3_BIT).map(|m| (m, true)),
                false => None,
            }
            .or_else(|| make(2, EGL_OPENGL_ES2_BIT).map(|m| (m, false)));
            let Some(((surface, context), gles3)) = made else {
                return Err(fail("eglCreateContext"));
            };
            let gl = Gl {
                gen_framebuffers: sym(&gles, "glGenFramebuffers")?,
                delete_framebuffers: sym(&gles, "glDeleteFramebuffers")?,
                bind_framebuffer: sym(&gles, "glBindFramebuffer")?,
                gen_renderbuffers: sym(&gles, "glGenRenderbuffers")?,
                delete_renderbuffers: sym(&gles, "glDeleteRenderbuffers")?,
                bind_renderbuffer: sym(&gles, "glBindRenderbuffer")?,
                renderbuffer_storage: sym(&gles, "glRenderbufferStorage")?,
                framebuffer_renderbuffer: sym(&gles, "glFramebufferRenderbuffer")?,
                check_framebuffer_status: sym(&gles, "glCheckFramebufferStatus")?,
                read_pixels: sym(&gles, "glReadPixels")?,
                pixel_storei: sym(&gles, "glPixelStorei")?,
                clear_color: sym(&gles, "glClearColor")?,
                clear: sym(&gles, "glClear")?,
                pack: match gles3 {
                    true => (|| {
                        Ok::<_, GfxError>(Pack {
                            gen_buffers: sym(&gles, "glGenBuffers")?,
                            delete_buffers: sym(&gles, "glDeleteBuffers")?,
                            bind_buffer: sym(&gles, "glBindBuffer")?,
                            buffer_data: sym(&gles, "glBufferData")?,
                            map_buffer_range: sym(&gles, "glMapBufferRange")?,
                            unmap_buffer: sym(&gles, "glUnmapBuffer")?,
                        })
                    })()
                    .ok(),
                    false => None,
                },
            };
            let made = OffscreenGl {
                display,
                surface,
                context,
                gles3,
                make_current: sym(&egl, "eglMakeCurrent")?,
                get_proc_address: sym(&egl, "eglGetProcAddress")?,
                destroy_context: sym(&egl, "eglDestroyContext")?,
                destroy_surface: sym(&egl, "eglDestroySurface")?,
                gl,
                fbo: 0,
                buffers: [0; 2],
                size: (0, 0),
                pbos: [0; 2],
                pending: [None; 2],
                turn: 0,
                ion: None,
                ion_tried: false,
                gles,
                _egl: egl,
            };
            if (made.make_current)(display, surface, surface, context) == 0 {
                return Err(fail("eglMakeCurrent"));
            }
            Ok(made)
        }
    }

    pub fn is_gles3(&self) -> bool {
        self.gles3
    }

    /// A GL function by name: from the library first, as some drivers' eglGetProcAddress gives
    /// only extensions, then from EGL.
    pub fn proc_address(&self, name: &CStr) -> *const c_void {
        match unsafe { self.gles.get::<unsafe extern "C" fn()>(name.to_bytes()) } {
            Ok(f) => *f as *const c_void,
            Err(_) => unsafe { (self.get_proc_address)(name.as_ptr()) },
        }
    }

    /// A framebuffer of at least `w` by `h`, with depth and stencil when asked, made again only
    /// when it is too small.
    pub fn framebuffer(&mut self, w: u32, h: u32, depth: bool, stencil: bool) -> u32 {
        if self.fbo != 0 && self.size.0 >= w && self.size.1 >= h {
            return self.fbo;
        }
        let gl = &self.gl;
        unsafe {
            if self.fbo != 0 {
                (gl.delete_framebuffers)(1, &self.fbo);
                (gl.delete_renderbuffers)(2, self.buffers.as_ptr());
            }
            let (w, h) = (w.max(self.size.0), h.max(self.size.1));
            (gl.gen_framebuffers)(1, &mut self.fbo);
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (gl.gen_renderbuffers)(2, self.buffers.as_mut_ptr());
            (gl.bind_renderbuffer)(GL_RENDERBUFFER, self.buffers[0]);
            // GLES 2 has no RGBA8 renderbuffer without an extension; RGB565 is always there.
            let colour = if self.gles3 { GL_RGBA8 } else { GL_RGB565 };
            (gl.renderbuffer_storage)(GL_RENDERBUFFER, colour, w as i32, h as i32);
            (gl.framebuffer_renderbuffer)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_RENDERBUFFER,
                self.buffers[0],
            );
            if depth || stencil {
                (gl.bind_renderbuffer)(GL_RENDERBUFFER, self.buffers[1]);
                let packed = self.gles3 || stencil;
                let format = if packed {
                    GL_DEPTH24_STENCIL8
                } else {
                    GL_DEPTH_COMPONENT16
                };
                (gl.renderbuffer_storage)(GL_RENDERBUFFER, format, w as i32, h as i32);
                (gl.framebuffer_renderbuffer)(
                    GL_FRAMEBUFFER,
                    GL_DEPTH_ATTACHMENT,
                    GL_RENDERBUFFER,
                    self.buffers[1],
                );
                if packed {
                    (gl.framebuffer_renderbuffer)(
                        GL_FRAMEBUFFER,
                        GL_STENCIL_ATTACHMENT,
                        GL_RENDERBUFFER,
                        self.buffers[1],
                    );
                }
            }
            // With SLOT_GL_DEBUG the framebuffer starts magenta, so a frame read back from one
            // the core never drew into says so rather than passing for a black picture.
            if std::env::var_os("SLOT_GL_DEBUG").is_some() {
                (gl.clear_color)(1.0, 0.0, 1.0, 1.0);
                (gl.clear)(0x4000);
            }
            let status = (gl.check_framebuffer_status)(GL_FRAMEBUFFER);
            if status != GL_FRAMEBUFFER_COMPLETE {
                eprintln!("slot: gpu core: framebuffer {w}x{h} incomplete ({status:#x})");
            }
            self.size = (w, h);
        }
        self.fbo
    }

    /// The bottom-left `w` by `h` of the framebuffer as RGBA rows, the bottom one first. With
    /// GLES 3 it is the frame before this one, as the GPU copies this one out while the CPU
    /// takes that; the first frame, and one of a new size, is read as it is.
    pub fn read(&mut self, w: u32, h: u32, out: &mut Vec<u8>) {
        out.resize((w * h * 4) as usize, 0);
        let gl = &self.gl;
        unsafe {
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (gl.pixel_storei)(GL_PACK_ALIGNMENT, 1);
            let Some(pack) = &gl.pack else {
                (gl.read_pixels)(
                    0,
                    0,
                    w as i32,
                    h as i32,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    out.as_mut_ptr() as *mut c_void,
                );
                return;
            };
            if self.pbos[0] == 0 {
                (pack.gen_buffers)(2, self.pbos.as_mut_ptr());
            }
            let len = out.len() as isize;
            let this = self.turn;
            (pack.bind_buffer)(GL_PIXEL_PACK_BUFFER, self.pbos[this]);
            (pack.buffer_data)(GL_PIXEL_PACK_BUFFER, len, std::ptr::null(), GL_STREAM_READ);
            (gl.read_pixels)(
                0,
                0,
                w as i32,
                h as i32,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null_mut(),
            );
            self.pending[this] = Some((w, h));
            let last = 1 - this;
            let from = match self.pending[last] {
                Some(size) if size == (w, h) => last,
                _ => this,
            };
            (pack.bind_buffer)(GL_PIXEL_PACK_BUFFER, self.pbos[from]);
            let mapped = (pack.map_buffer_range)(GL_PIXEL_PACK_BUFFER, 0, len, GL_MAP_READ_BIT);
            if !mapped.is_null() {
                std::ptr::copy_nonoverlapping(mapped as *const u8, out.as_mut_ptr(), out.len());
                (pack.unmap_buffer)(GL_PIXEL_PACK_BUFFER);
            }
            (pack.bind_buffer)(GL_PIXEL_PACK_BUFFER, 0);
            self.turn = last;
        }
    }

    /// The bottom-left `w` by `h` of the framebuffer into `out` as XRGB8888, top row first, turned
    /// over when `flip`, the GPU laying it out and the CPU reading it from cached memory; the
    /// frame before this one, as `read` gives. False when that cannot be had here, and `read` is
    /// the way.
    pub fn read_xrgb(&mut self, w: u32, h: u32, flip: bool, out: &mut [u8]) -> bool {
        let fbo = self.fbo;
        let Some(ion) = self.ion() else {
            return false;
        };
        if ion.read(fbo, w, h, flip, out) {
            return true;
        }
        self.ion = None;
        false
    }

    /// As `read_xrgb`, leaving the frame where the GPU put it, in memory another process can map:
    /// which of the buffers it is in, with the buffers' fds when they are new since last asked.
    /// None when that cannot be had here.
    pub fn share_xrgb(
        &mut self,
        w: u32,
        h: u32,
        flip: bool,
    ) -> Option<(usize, Option<Vec<OwnedFd>>)> {
        let fbo = self.fbo;
        let shared = self.ion()?.share(fbo, w, h, flip);
        if shared.is_none() {
            self.ion = None;
        }
        shared
    }

    /// The cached frames, made the first time they are asked for; None on GLES 2 or if they
    /// cannot be had.
    fn ion(&mut self) -> Option<&mut IonFrames> {
        if !self.gles3 {
            return None;
        }
        if !self.ion_tried {
            self.ion_tried = true;
            match IonFrames::new(self.display, |name| self.proc_address(name)) {
                Ok(ion) => self.ion = Some(ion),
                Err(e) => eprintln!("slot: gpu core: no cached frames: {e}"),
            }
        }
        self.ion.as_mut()
    }

    /// Makes the context current again, for a thread that has used another since.
    pub fn make_current(&self) -> bool {
        unsafe { (self.make_current)(self.display, self.surface, self.surface, self.context) != 0 }
    }
}

impl Drop for OffscreenGl {
    fn drop(&mut self) {
        self.ion = None;
        unsafe {
            if self.fbo != 0 {
                (self.gl.delete_framebuffers)(1, &self.fbo);
                (self.gl.delete_renderbuffers)(2, self.buffers.as_ptr());
            }
            if let (Some(pack), true) = (&self.gl.pack, self.pbos[0] != 0) {
                (pack.delete_buffers)(2, self.pbos.as_ptr());
            }
            (self.make_current)(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            (self.destroy_context)(self.display, self.context);
            (self.destroy_surface)(self.display, self.surface);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GL_COLOR_BUFFER_BIT: u32 = 0x4000;

    /// Draws a colour into the framebuffer with the context's own GL and reads it back. Runs only
    /// where there is an EGL to be had (Linux with Mesa, the device), and says so when there is not.
    #[test]
    fn a_core_drawing_into_the_framebuffer_reads_back() {
        let Ok(mut gl) = OffscreenGl::new(true) else {
            eprintln!("no EGL here, skipping");
            return;
        };
        let fbo = gl.framebuffer(8, 4, true, true);
        assert_ne!(fbo, 0);
        unsafe {
            let clear_color: unsafe extern "C" fn(f32, f32, f32, f32) =
                std::mem::transmute(gl.proc_address(c"glClearColor"));
            let clear: unsafe extern "C" fn(u32) = std::mem::transmute(gl.proc_address(c"glClear"));
            (gl.gl.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            clear_color(1.0, 0.0, 0.0, 1.0);
            clear(GL_COLOR_BUFFER_BIT);
        }
        let mut out = Vec::new();
        gl.read(8, 4, &mut out);
        assert_eq!(out.len(), 8 * 4 * 4);
        assert!(
            out.chunks_exact(4)
                .all(|p| p[0] > 200 && p[1] < 50 && p[2] < 50),
            "the framebuffer did not come back red: {:?}",
            &out[..4]
        );
        // Asking for no more than it has keeps the same framebuffer.
        assert_eq!(gl.framebuffer(4, 2, true, true), fbo);
    }
}
