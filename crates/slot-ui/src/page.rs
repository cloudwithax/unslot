//! Drawing for the pages slot lays out itself, a picture at a time: the keyboard and the lists of
//! networks and devices.

use crate::bitmaps::{button_legend, ButtonLegend};
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

/// What a button does on a page: its legend, or two for a pair such as L1 and R1, and the word
/// for it, drawn as the quick menu's footer draws its own.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Legend {
    pub buttons: Vec<ButtonLegend>,
    pub label: String,
}

impl Legend {
    pub fn new(button: ButtonLegend, label: &str) -> Legend {
        Legend {
            buttons: vec![button],
            label: label.into(),
        }
    }

    pub fn pair(first: ButtonLegend, second: ButtonLegend, label: &str) -> Legend {
        Legend {
            buttons: vec![first, second],
            label: label.into(),
        }
    }
}

/// The legend pictures are the footer's height; a page draws them this tall.
const LEGEND_ICON: f32 = 34.0;
const LEGEND_PX: f32 = 16.0;
const LEGEND_WORD_GAP: f32 = 4.0;
const LEGEND_SPACE: f32 = 20.0;
/// The most a row of legends shrinks to fit before it is cut off.
const LEGEND_MIN_SCALE: f32 = 0.6;

/// `legends` in a row centred in the box, each button then its word. A row too wide for the box
/// shrinks, all of it alike.
pub fn legends_into(
    rgba: &mut [u8],
    face_w: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    legends: &[Legend],
    ink: [u8; 4],
) {
    let Some(font) = text::label_font() else {
        return;
    };
    let mut scale = 1.0;
    while scale > LEGEND_MIN_SCALE && row_width(font, legends, scale) > w as f32 {
        scale -= 0.05;
    }
    let icon_h = (LEGEND_ICON * scale).round() as u32;
    let px = (LEGEND_PX * scale).round();
    let total = row_width(font, legends, scale).min(w as f32);
    let mut at = x as f32 + (w as f32 - total) / 2.0;
    let right = (x + w) as f32;
    for (i, legend) in legends.iter().enumerate() {
        if i > 0 {
            at += LEGEND_SPACE * scale;
        }
        for &button in &legend.buttons {
            let (icon, iw) = shrunk(button, icon_h);
            if at + iw as f32 > right {
                return;
            }
            over(
                rgba,
                face_w,
                &icon,
                iw,
                icon_h,
                at.round() as u32,
                y + (h - icon_h) / 2,
            );
            at += iw as f32;
        }
        at += LEGEND_WORD_GAP * scale;
        let word = word_width(font, &legend.label, px);
        if at + word > right {
            return;
        }
        // label_into keeps 4 px clear either side of the box.
        label_into(
            rgba,
            face_w,
            (at.round() as u32 - 4, y, word.ceil() as u32 + 8, h),
            &legend.label,
            ink,
            px,
            true,
        );
        at += word;
    }
}

fn word_width(font: &fontdue::Font, label: &str, px: f32) -> f32 {
    let layout = text::fit(font, label, f32::INFINITY, 1, px, px);
    layout
        .lines
        .iter()
        .map(|l| text::line_width(font, l, layout.px, layout.tracking))
        .fold(0.0, f32::max)
        .ceil()
}

fn row_width(font: &fontdue::Font, legends: &[Legend], scale: f32) -> f32 {
    let icon_h = (LEGEND_ICON * scale).round();
    let px = (LEGEND_PX * scale).round();
    let gaps = LEGEND_SPACE * scale * legends.len().saturating_sub(1) as f32;
    gaps + legends
        .iter()
        .map(|l| {
            let icons: f32 = l
                .buttons
                .iter()
                .map(|&b| {
                    let (_, w, h) = button_legend(b);
                    (w as f32 * icon_h / h as f32).round()
                })
                .sum();
            icons + LEGEND_WORD_GAP * scale + word_width(font, &l.label, px)
        })
        .sum::<f32>()
}

/// A button's legend scaled down to `h` tall, each pixel the average of the ones it covers.
fn shrunk(button: ButtonLegend, h: u32) -> (Vec<u8>, u32) {
    const TAPS: u32 = 4;
    let (src, sw, sh) = button_legend(button);
    let w = ((sw * h) as f32 / sh as f32).round().max(1.0) as u32;
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for ty in 0..TAPS {
                for tx in 0..TAPS {
                    let sx = ((x * TAPS + tx) * sw / (w * TAPS)).min(sw - 1);
                    let sy = ((y * TAPS + ty) * sh / (h * TAPS)).min(sh - 1);
                    let p = ((sy * sw + sx) * 4) as usize;
                    let a = u32::from(src[p + 3]);
                    for c in 0..3 {
                        sum[c] += u32::from(src[p + c]) * a;
                    }
                    sum[3] += a;
                }
            }
            let at = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                out[at + c] = sum[c].checked_div(sum[3]).unwrap_or(0) as u8;
            }
            out[at + 3] = (sum[3] / (TAPS * TAPS)) as u8;
        }
    }
    (out, w)
}
