//! The OpenGL ES context a core that draws with the GPU is given: one of slot-gfx's offscreen
//! contexts, made on the core's thread when it asks. Only on Linux, the device and the tests
//! run under WSL; elsewhere such a core is refused.

#[cfg(target_os = "linux")]
struct Offscreen(slot_gfx::OffscreenGl);

#[cfg(target_os = "linux")]
impl slot_retro::GlContext for Offscreen {
    fn framebuffer(&mut self, w: u32, h: u32, depth: bool, stencil: bool) -> u32 {
        self.0.framebuffer(w, h, depth, stencil)
    }

    fn read(&mut self, w: u32, h: u32, out: &mut Vec<u8>) {
        self.0.read(w, h, out)
    }

    fn read_xrgb(&mut self, w: u32, h: u32, bottom_left_origin: bool, out: &mut [u8]) -> bool {
        self.0.read_xrgb(w, h, bottom_left_origin, out)
    }

    fn share_xrgb(
        &mut self,
        w: u32,
        h: u32,
        bottom_left_origin: bool,
    ) -> Option<(usize, Option<Vec<std::os::fd::OwnedFd>>)> {
        self.0.share_xrgb(w, h, bottom_left_origin)
    }
}

/// Lets GPU cores have a context. Safe to call again; the first call is the one that counts.
pub fn install() {
    #[cfg(target_os = "linux")]
    {
        static GLOBAL: std::sync::Once = std::sync::Once::new();
        GLOBAL.call_once(load_gl_globally);
    }
    #[cfg(target_os = "linux")]
    slot_retro::set_gl_factory(|gles3| match slot_gfx::OffscreenGl::new(gles3) {
        Ok(gl) => Some(Box::new(Offscreen(gl)) as Box<dyn slot_retro::GlContext>),
        Err(e) => {
            eprintln!("slot: gpu core: no GL context: {e}");
            None
        }
    });
}

/// Loads GLES and EGL into the global namespace before any core, as RetroArch, which links them,
/// has them. Builds of PPSSPP and Flycast call GL by name without naming the library they come
/// from, and would not load else. The libraries stay loaded for as long as slot runs.
#[cfg(target_os = "linux")]
fn load_gl_globally() {
    for names in [
        ["libGLESv2.so.2", "libGLESv2.so"],
        ["libEGL.so.1", "libEGL.so"],
    ] {
        let loaded = names.iter().any(|name| {
            let Ok(c) = std::ffi::CString::new(*name) else {
                return false;
            };
            // SAFETY: dlopen with a valid C string; the handle is kept open on purpose.
            !unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) }.is_null()
        });
        if !loaded {
            eprintln!("slot: gpu core: {} could not be loaded", names[0]);
        }
    }
}
