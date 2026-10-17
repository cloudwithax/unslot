mod bezel;
mod draw;
mod fbdev;
mod fbo;
mod gl;
mod grade;
#[cfg(target_os = "macos")]
mod headless;
#[cfg(target_os = "linux")]
#[path = "headless_egl.rs"]
mod headless;
#[cfg(feature = "host")]
mod host;
mod layout;
mod lcd3x;
#[cfg(target_os = "linux")]
mod offscreen;
mod pipeline;
mod power;
mod quad;
mod shaders;
mod surface;

pub use bezel::{framed, lifted_rect, screen_rect};
pub use draw::{Draw, TexId};
pub use fbdev::{egl_error, panel_mode, panel_size, FbdevSurface};
pub use fbo::{canvas_size, Compositor, BACKDROP};
pub use grade::{blue_light_gain, BLUE_LIGHT_MAX};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use headless::HeadlessSurface;
#[cfg(feature = "host")]
pub use host::HostSurface;
pub use layout::{Canvas, Layout};
pub use lcd3x::{lcd3x_factors, lcd3x_mask, ScreenEffect};
#[cfg(target_os = "linux")]
pub use offscreen::OffscreenGl;
pub use pipeline::{fit_aspect, SCALE, SRC_H, SRC_W, WHOLE_TEXTURE};
pub use power::{screen_brightness, screen_rect_above, screen_scale, screen_width};
pub use surface::{blit_rect, blit_rect_fit, fit_rect, fit_scale, GfxError, Surface, OUT_H, OUT_W};
