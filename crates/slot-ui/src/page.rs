//! Drawing for the pages slot lays out itself, a picture at a time: the keyboard and the lists of
//! networks and devices.

use crate::text;

pub const INK: [u8; 4] = [0xf6, 0xf4, 0xef, 0xff];
pub const DIM: [u8; 4] = [0x9a, 0x9a, 0xa4, 0xff];
pub const LIT: [u8; 4] = [0xf6, 0xf4, 0xef, 0xff];
pub const LIT_INK: [u8; 4] = [0x1a, 0x18, 0x16, 0xff];

/// A filled rectangle with its corners rounded off, on a picture `face_w` wide.
pub fn rounded(
    rgba: &mut [u8],
    face_w: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    r: u32,
    fill: [u8; 4],
) {
    let (r, w, h) = (r as i32, w as i32, h as i32);
    for py in 0..h {
        for px in 0..w {
            // How far the pixel is past the inner rectangle the corners are drawn round.
            let dx = px - px.clamp(r, w - 1 - r);
            let dy = py - py.clamp(r, h - 1 - r);
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let at = (((y as i32 + py) as u32 * face_w + x + px as u32) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&fill);
        }
    }
}

/// `label` centred in the box, as written or upper-cased to the label font's style.
pub fn label_into(
    rgba: &mut [u8],
    face_w: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    label: &str,
    ink: [u8; 4],
    px: f32,
    upper: bool,
) {
    let Some(font) = text::label_font() else {
        return;
    };
    let layout = match upper {
        true => text::fit(font, label, w as f32 - 8.0, 1, px, 10.0),
        false => text::fit_as_written(font, label, w as f32 - 8.0, px, 10.0),
    };
    let cov = text::coverage(w, h, &layout);
    let mut band = vec![0u8; (w * h * 4) as usize];
    for (i, a) in cov.into_iter().enumerate() {
        band[i * 4..i * 4 + 4].copy_from_slice(&[ink[0], ink[1], ink[2], a]);
    }
    over(rgba, face_w, &band, w, h, x, y);
}

/// `label` from the left of the box, as written, shrunk to fit its width.
pub fn label_left(
    rgba: &mut [u8],
    face_w: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    label: &str,
    ink: [u8; 4],
    px: f32,
) {
    let Some(font) = text::label_font() else {
        return;
    };
    let layout = text::fit_as_written(font, label, w as f32, px, 12.0);
    let set = text::line_width(font, label, layout.px, layout.tracking).ceil() as u32 + 2;
    let set = set.min(w);
    label_into(rgba, face_w, (x, y, set, h), label, ink, layout.px, false);
}

/// Lays `src`, `w` by `h`, over the picture at `x`, `y` with its alpha.
pub fn over(dst: &mut [u8], face_w: u32, src: &[u8], w: u32, h: u32, x: u32, y: u32) {
    for row in 0..h {
        for col in 0..w {
            let s = ((row * w + col) * 4) as usize;
            let a = u32::from(src[s + 3]);
            if a == 0 {
                continue;
            }
            let d = (((y + row) * face_w + x + col) * 4) as usize;
            for c in 0..3 {
                dst[d + c] =
                    ((u32::from(src[s + c]) * a + u32::from(dst[d + c]) * (255 - a)) / 255) as u8;
            }
            dst[d + 3] = (a + u32::from(dst[d + 3]) * (255 - a) / 255) as u8;
        }
    }
}
