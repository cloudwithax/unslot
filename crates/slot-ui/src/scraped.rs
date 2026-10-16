//! Labels made from art found online: a game's clear logo on a two-stop gradient coloured from
//! its box art, or the box art itself when there is no logo to be had.
//!
//! The logo style is Cart Studio's "Logo Only" (github.com/BrandonKowalski/slot-cart-studio,
//! GPL-3.0-or-later): its gradient, the brightness split that decides whether the ground goes
//! deep or pale, the logo's box and the box-art hue, ported from its `label.rs` and `hue.rs`.

use resvg::tiny_skia::{FilterQuality, IntSize, Pixmap, PixmapPaint, Transform};

use crate::art::decode_png_bytes;

/// The logo's box, as fractions of the label, so a label of any shape places it alike.
const BOX_W_OF: f32 = 920.0 / 1280.0;
const BOX_H_OF: f32 = 423.0 / 640.0;
/// Where a logo that fills the box's height starts; a wider one centres on `MID_ROW_OF`.
const TALL_TOP_OF: f32 = 70.0 / 640.0;
const MID_ROW_OF: f32 = 320.0 / 640.0;
/// Bounds ignore the faint fringe some rips carry round a logo.
const ALPHA_EDGE: u8 = 16;
/// How bright a logo is before the ground goes deep under it rather than pale.
const BRIGHT: f32 = 110.0;
/// Yellow has no deep form: taken dark it reads as olive, so a deep yellow turns to amber.
const OLIVE_LO: u16 = 45;
const OLIVE_HI: u16 = 75;
const AMBER: f32 = 38.0;

/// The label for a game, `w` by `h`, as a PNG: its logo on a ground coloured from its box art
/// (or from `fallback_hue` with no box art to go on), else the box art covered onto the label.
/// None when neither is a picture.
pub fn scraped_label(
    logo: Option<&[u8]>,
    boxart: Option<&[u8]>,
    fallback_hue: u16,
    w: u32,
    h: u32,
) -> Option<Vec<u8>> {
    if let Some((px, lw, lh)) = logo.and_then(decode_png_bytes) {
        if trim(&px, lw, lh).is_some() {
            let hue = boxart.and_then(box_hue).unwrap_or(fallback_hue);
            let (deep, pale) = stops(hue, luma(&px));
            return encode(&compose(&px, lw, lh, w, h, deep, pale), w, h);
        }
    }
    let rgba = crate::art::cover_bytes(boxart?, w, h)?;
    encode(&rgba, w, h)
}

/// The ground's two corners for a hue, given how bright the logo landing on it is.
fn stops(hue: u16, logo_luma: f32) -> ([u8; 3], [u8; 3]) {
    let h = hue as f32;
    if logo_luma < BRIGHT {
        (hsl_to_rgb(h, 0.55, 0.62), hsl_to_rgb(h, 0.50, 0.84))
    } else {
        let h = if (OLIVE_LO..=OLIVE_HI).contains(&hue) {
            AMBER
        } else {
            h
        };
        (hsl_to_rgb(h, 0.65, 0.24), hsl_to_rgb(h, 0.60, 0.46))
    }
}

/// Opaque RGBA between two corners, leaning on x so it reads as diagonal on a wide label.
fn gradient(lw: u32, lh: u32, deep: [u8; 3], pale: [u8; 3]) -> Vec<u8> {
    let mut px = Vec::with_capacity((lw * lh * 4) as usize);
    for y in 0..lh {
        for x in 0..lw {
            let t = 0.88 * x as f32 / lw as f32 + 0.12 * y as f32 / lh as f32;
            for c in 0..3 {
                px.push((deep[c] as f32 + (pale[c] as f32 - deep[c] as f32) * t).round() as u8);
            }
            px.push(255);
        }
    }
    px
}

/// The visible part of a logo as `(x0, y0, x1, y1)`, far edges exclusive.
fn trim(rgba: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..h {
        for x in 0..w {
            if rgba[((y * w + x) * 4 + 3) as usize] < ALPHA_EDGE {
                continue;
            }
            bounds = Some(match bounds {
                None => (x, y, x + 1, y + 1),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
            });
        }
    }
    bounds
}

/// Scale and top left for a trimmed logo: one factor that fits the label's box, centred across.
fn placement(lw: u32, lh: u32, w: u32, h: u32) -> (f32, f32, f32) {
    let by_w = BOX_W_OF * lw as f32 / w as f32;
    let by_h = BOX_H_OF * lh as f32 / h as f32;
    let scale = by_w.min(by_h);
    let x = (lw as f32 - w as f32 * scale) / 2.0;
    let y = if by_h <= by_w {
        TALL_TOP_OF * lh as f32
    } else {
        MID_ROW_OF * lh as f32 - h as f32 * scale / 2.0
    };
    (scale, x, y)
}

/// The logo over the gradient, as opaque RGBA `lw` by `lh`.
fn compose(logo: &[u8], w: u32, h: u32, lw: u32, lh: u32, deep: [u8; 3], pale: [u8; 3]) -> Vec<u8> {
    let ground = gradient(lw, lh, deep, pale);
    let Some((x0, y0, x1, y1)) = trim(logo, w, h) else {
        return ground;
    };
    let (tw, th) = (x1 - x0, y1 - y0);
    let mut cut = Vec::with_capacity((tw * th * 4) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * w + x) * 4) as usize;
            let a = logo[i + 3] as u32;
            for c in 0..3 {
                cut.push(((logo[i + c] as u32 * a + 127) / 255) as u8);
            }
            cut.push(a as u8);
        }
    }
    let Some(src) = IntSize::from_wh(tw, th).and_then(|s| Pixmap::from_vec(cut, s)) else {
        return ground;
    };
    let Some(mut dst) = IntSize::from_wh(lw, lh).and_then(|s| Pixmap::from_vec(ground.clone(), s))
    else {
        return ground;
    };
    let (scale, x, y) = placement(lw, lh, tw, th);
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    dst.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &paint,
        Transform::from_row(scale, 0.0, 0.0, scale, x, y),
        None,
    );
    // The ground is opaque everywhere, so premultiplied and straight are the same bytes.
    dst.take()
}

/// Mean luminance of a logo's opaque pixels, which decides whether its ground goes deep or pale.
fn luma(px: &[u8]) -> f32 {
    let (mut sum, mut n) = (0.0, 0usize);
    for p in px.chunks_exact(4) {
        if p[3] < 200 {
            continue;
        }
        sum += 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
        n += 1;
    }
    if n == 0 {
        255.0
    } else {
        sum / n as f32
    }
}

/// The box art's dominant saturated hue, edges cropped and dull or dark pixels ignored, so a
/// region's trade dress does not outvote the art.
pub fn box_hue(png: &[u8]) -> Option<u16> {
    let (px, w, h) = decode_png_bytes(png)?;
    let (mx, my) = ((w as f32 * 0.06) as u32, (h as f32 * 0.06) as u32);
    let mut bins = [0f32; 36];
    for y in my..h.saturating_sub(my) {
        for x in mx..w.saturating_sub(mx) {
            let i = ((y * w + x) * 4) as usize;
            if px[i + 3] < 128 {
                continue;
            }
            let (hue, s, v) = rgb_to_hsv(px[i], px[i + 1], px[i + 2]);
            if s < 0.35 || v < 0.25 {
                continue;
            }
            bins[((hue / 10.0) as usize).min(35)] += s * v;
        }
    }
    let (best, weight) =
        bins.iter().enumerate().fold(
            (0, 0.0),
            |acc, (i, w)| if *w > acc.1 { (i, *w) } else { acc },
        );
    (weight > 0.0).then_some(best as u16 * 10 + 5)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let sector = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let hue = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (hue, s, max)
}

/// The hue of a colour, for a game with no box art: the colour its text label has on the shelf.
pub fn hue_of(rgb: [u8; 3]) -> u16 {
    rgb_to_hsv(rgb[0], rgb[1], rgb[2]).0 as u16
}

fn encode(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().ok()?.write_image_data(rgba).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let data: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| px(x, y))
            .collect();
        encode(&data, w, h).unwrap()
    }

    fn at(png_bytes: &[u8], x: u32, y: u32) -> [u8; 4] {
        let (px, w, _) = decode_png_bytes(png_bytes).unwrap();
        let i = ((y * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    #[test]
    fn a_white_logo_lands_on_a_deep_ground_in_its_box_arts_hue() {
        // A white bar, clear around it, and a box that is all one red.
        let logo = png(40, 10, |x, _| match x {
            10..=29 => [255, 255, 255, 255],
            _ => [0, 0, 0, 0],
        });
        let boxart = png(20, 20, |_, _| [220, 30, 30, 255]);
        assert_eq!(box_hue(&boxart), Some(5));
        let label = scraped_label(Some(&logo), Some(&boxart), 200, 200, 100).unwrap();
        let corner = at(&label, 0, 0);
        assert!(
            corner[0] > corner[1] && corner[0] > corner[2],
            "the ground is not red: {corner:?}"
        );
        assert!(
            corner[0] < 120,
            "a white logo did not get a deep ground: {corner:?}"
        );
        assert_eq!(
            at(&label, 100, 50),
            [255, 255, 255, 255],
            "the logo is not in the middle"
        );
    }

    #[test]
    fn a_dark_logo_lands_on_a_pale_ground() {
        let logo = png(20, 20, |_, _| [10, 10, 40, 255]);
        let label = scraped_label(Some(&logo), None, 120, 160, 80).unwrap();
        let corner = at(&label, 0, 79);
        let luma =
            0.2126 * corner[0] as f32 + 0.7152 * corner[1] as f32 + 0.0722 * corner[2] as f32;
        assert!(luma > 150.0, "the ground is not pale: {corner:?}");
    }

    #[test]
    fn with_no_logo_the_box_art_covers_the_label() {
        let boxart = png(30, 30, |_, _| [10, 200, 40, 255]);
        let label = scraped_label(None, Some(&boxart), 0, 64, 32).unwrap();
        assert_eq!(at(&label, 32, 16), [10, 200, 40, 255]);
        let (_, w, h) = decode_png_bytes(&label).unwrap();
        assert_eq!((w, h), (64, 32));
    }

    #[test]
    fn nothing_to_draw_is_no_label() {
        let clear = png(8, 8, |_, _| [0, 0, 0, 0]);
        assert!(scraped_label(Some(&clear), None, 0, 64, 32).is_none());
        assert!(scraped_label(Some(b"not a png"), None, 0, 64, 32).is_none());
    }
}
