use std::path::{Path, PathBuf};

use slot_gfx::{OUT_H, OUT_W};
use slot_store::Platform;

pub const BEZELS: &str = "System/bezels";
pub const GAME_BEZELS: &str = "Bezels";

pub fn list(root: &Path, platform: Platform) -> Vec<String> {
    let mut files = pngs(&root.join(GAME_BEZELS).join(platform.dir_name()));
    sort(&mut files);
    files
}

fn pngs(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !slot_store::is_hidden(p))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .filter_map(|p| Some(p.file_name()?.to_str()?.to_owned()))
        .collect()
}

fn sort(files: &mut [String]) {
    files.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
}

fn dims(path: &Path) -> Option<(u32, u32)> {
    if !path.extension()?.eq_ignore_ascii_case("png") {
        return None;
    }
    let (w, h) = path.file_stem()?.to_str()?.split_once('x')?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    if w > 0 && h > 0 {
        Some((w, h))
    } else {
        None
    }
}

pub fn pick(root: &Path, panel: (u32, u32)) -> Option<PathBuf> {
    let mut fits: Vec<((u32, u32), PathBuf)> = std::fs::read_dir(root.join(BEZELS))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| !slot_store::is_hidden(p))
        .filter_map(|p| Some((dims(&p)?, p)))
        .filter(|((w, h), _)| {
            u64::from(*w) * u64::from(panel.1) == u64::from(*h) * u64::from(panel.0)
        })
        .collect();
    fits.sort_by_key(|((w, _), p)| (w.abs_diff(panel.0), p.clone()));
    fits.into_iter().next().map(|(_, p)| p)
}

/// The bezels a build ships, a folder per screen size, `WxH`, of a folder per system.
pub const OVERLAYS: &str = "System/overlays";

/// Below this alpha a bezel's pixel lets the game through.
const CLEAR_BELOW: u8 = 16;

/// A bezel to lay over the whole screen, and the window in it the game is drawn in, as x, y, w,
/// h on the picture's own pixels.
pub struct Overlay {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub window: [f32; 4],
}

/// The shipped overlays drawn for this panel: its own size, else the nearest of its shape, else
/// the 720 by 480 ones.
pub fn overlays(root: &Path, panel: (u32, u32)) -> Option<PathBuf> {
    let sizes: Vec<((u32, u32), PathBuf)> = std::fs::read_dir(root.join(OVERLAYS))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let (w, h) = p.file_name()?.to_str()?.split_once('x')?;
            Some(((w.parse().ok()?, h.parse().ok()?), p))
        })
        .collect();
    let shaped = |(w, h): (u32, u32)| u64::from(w) * u64::from(panel.1) == u64::from(h) * u64::from(panel.0);
    sizes
        .iter()
        .filter(|(size, _)| shaped(*size))
        .min_by_key(|((w, _), p)| (w.abs_diff(panel.0), p.clone()))
        .or_else(|| sizes.iter().find(|(size, _)| *size == (OUT_W, OUT_H)))
        .map(|(_, p)| p.clone())
}

/// The bezels to pick from for a system: the card's own in Bezels, then the shipped ones for
/// this panel, each name once.
pub fn choices(root: &Path, platform: Platform, panel: (u32, u32)) -> Vec<String> {
    let mut files = list(root, platform);
    if let Some(dir) = overlays(root, panel) {
        files.extend(pngs(&dir.join(platform.dir_name())));
    }
    sort(&mut files);
    files.dedup();
    files
}

/// Where a bezel picked by name is: the card's own wins over a shipped one of the same name.
pub fn find(root: &Path, platform: Platform, name: &str, panel: (u32, u32)) -> Option<PathBuf> {
    let own = root.join(GAME_BEZELS).join(platform.dir_name()).join(name);
    if own.is_file() {
        return Some(own);
    }
    let shipped = overlays(root, panel)?.join(platform.dir_name()).join(name);
    shipped.is_file().then_some(shipped)
}

/// A bezel and its window: from the `.info` beside it, as Batocera and KNULLI write them, else
/// the clear run through the picture's middle.
pub fn load(path: &Path) -> Option<Overlay> {
    let (rgba, w, h) = decode(&std::fs::read(path).ok()?)?;
    let window = std::fs::read_to_string(path.with_extension("info"))
        .ok()
        .and_then(|text| info_window(&text, (w, h)))
        .or_else(|| clear_window(&rgba, w, h))?;
    Some(Overlay { w, h, rgba, window })
}

/// The game's window from a `.info`: its borders, on a picture `width` by `height`, scaled to
/// the picture's real size.
pub fn info_window(text: &str, (w, h): (u32, u32)) -> Option<[f32; 4]> {
    let field = |key: &str| -> Option<f32> {
        let at = text.find(&format!("\"{key}\""))? + key.len() + 2;
        let rest = text[at..].trim_start().strip_prefix(':')?.trim_start();
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .unwrap_or(rest.len());
        rest[..end].parse().ok()
    };
    let width = field("width").unwrap_or(w as f32);
    let height = field("height").unwrap_or(h as f32);
    let (left, top) = (field("left")?, field("top")?);
    let (right, bottom) = (field("right")?, field("bottom")?);
    let (sx, sy) = (w as f32 / width, h as f32 / height);
    let window = [
        left * sx,
        top * sy,
        (width - left - right) * sx,
        (height - top - bottom) * sy,
    ];
    (width > 0.0 && height > 0.0 && window[2] > 0.0 && window[3] > 0.0).then_some(window)
}

/// The clear part of a bezel that holds its middle, measured across its middle row and down its
/// middle column.
pub fn clear_window(rgba: &[u8], w: u32, h: u32) -> Option<[f32; 4]> {
    let clear = |x: u32, y: u32| rgba[((y * w + x) * 4 + 3) as usize] < CLEAR_BELOW;
    let (cx, cy) = (w / 2, h / 2);
    if w == 0 || h == 0 || !clear(cx, cy) {
        return None;
    }
    let x0 = (0..cx).rev().find(|&x| !clear(x, cy)).map_or(0, |x| x + 1);
    let x1 = (cx..w).find(|&x| !clear(x, cy)).unwrap_or(w);
    let y0 = (0..cy).rev().find(|&y| !clear(cx, y)).map_or(0, |y| y + 1);
    let y1 = (cy..h).find(|&y| !clear(cx, y)).unwrap_or(h);
    Some([x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32])
}

fn decode(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let src = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => src.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => src.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => src.iter().flat_map(|&l| [l, l, l, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((rgba, info.width, info.height))
}
