use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const CARD_LOGO: &str = "System/bootlogo.bmp";
pub const USER_LOGO: &str = "Config/bootlogo.bmp";
const LOGO: &str = "bootlogo.bmp";
const ORIGINAL: &str = "bootlogo.baseos.bmp";
const PARTITION: &str = "boot-resource";
const MOUNT: &str = "/tmp/slot-boot-resource";
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub const OWN: &str = "Bootlogos";

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    Same,
    Installed,
    Invalid,
}

pub struct Bmp {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<[u8; 3]>,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

pub fn size(bmp: &[u8]) -> Option<(usize, usize)> {
    if bmp.get(0..2)? != b"BM" || u32_at(bmp, 14)? < 40 {
        return None;
    }
    let w = u32_at(bmp, 18)? as i32;
    let h = u32_at(bmp, 22)? as i32;
    let (w, h) = (w.unsigned_abs() as usize, h.unsigned_abs() as usize);
    if (1..=4096).contains(&w) && (1..=4096).contains(&h) {
        Some((w, h))
    } else {
        None
    }
}

pub fn decode(bmp: &[u8]) -> Option<Bmp> {
    let (width, height) = size(bmp)?;
    if u16_at(bmp, 28)? != 24 || u32_at(bmp, 30)? != 0 {
        return None;
    }
    let top_down = (u32_at(bmp, 22)? as i32) < 0;
    let offset = u32_at(bmp, 10)? as usize;
    let stride = (width * 3 + 3) & !3;
    let data = bmp.get(offset..offset + stride * height)?;
    let mut rgb = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = if top_down { y } else { height - 1 - y };
        for px in data[row * stride..].chunks_exact(3).take(width) {
            rgb.push([px[2], px[1], px[0]]);
        }
    }
    Some(Bmp { width, height, rgb })
}

pub fn encode(img: &Bmp) -> Vec<u8> {
    let stride = (img.width * 3 + 3) & !3;
    let data = stride * img.height;
    let mut out = Vec::with_capacity(54 + data);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + data) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(img.width as i32).to_le_bytes());
    out.extend_from_slice(&(img.height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(data as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..img.height).rev() {
        for c in &img.rgb[y * img.width..(y + 1) * img.width] {
            out.extend_from_slice(&[c[2], c[1], c[0]]);
        }
        out.resize(out.len() + stride - img.width * 3, 0);
    }
    out
}

fn turned(img: &Bmp) -> Bmp {
    let (w, h) = (img.height, img.width);
    let mut rgb = vec![[0; 3]; w * h];
    for y in 0..img.height {
        for x in 0..img.width {
            rgb[(img.width - 1 - x) * w + y] = img.rgb[y * img.width + x];
        }
    }
    Bmp {
        width: w,
        height: h,
        rgb,
    }
}

fn landscape(img: Bmp) -> Bmp {
    if img.width >= img.height {
        return img;
    }
    let (width, height) = (img.height, img.width);
    let mut rgb = vec![[0; 3]; width * height];
    for y in 0..img.height {
        for x in 0..img.width {
            rgb[x * width + img.height - 1 - y] = img.rgb[y * img.width + x];
        }
    }
    Bmp { width, height, rgb }
}

pub fn compose(wordmark: &Bmp, (width, height): (usize, usize)) -> Option<Bmp> {
    let turned_mark;
    let mark = if height > width {
        turned_mark = turned(wordmark);
        &turned_mark
    } else {
        wordmark
    };
    if mark.width > width || mark.height > height {
        return None;
    }
    let mut rgb = vec![*mark.rgb.first()?; width * height];
    let (x0, y0) = ((width - mark.width) / 2, (height - mark.height) / 2);
    for y in 0..mark.height {
        let at = (y0 + y) * width + x0;
        rgb[at..at + mark.width].copy_from_slice(&mark.rgb[y * mark.width..(y + 1) * mark.width]);
    }
    Some(Bmp { width, height, rgb })
}

pub fn install(wordmark: &[u8], dir: &Path) -> io::Result<Outcome> {
    let Some(mark) = decode(wordmark) else {
        return Ok(Outcome::Invalid);
    };
    let current = fs::read(dir.join(LOGO)).ok();
    let original = fs::read(dir.join(ORIGINAL)).ok();
    let Some(panel) = original.as_deref().or(current.as_deref()).and_then(size) else {
        return Ok(Outcome::Invalid);
    };
    let Some(logo) = compose(&mark, panel).map(|b| encode(&b)) else {
        return Ok(Outcome::Invalid);
    };
    if current.as_deref() == Some(&logo[..]) {
        return Ok(Outcome::Same);
    }
    if let Some(current) = &current {
        if original.is_none() {
            replace(dir, ORIGINAL, current)?;
        }
        if current.len() == logo.len() {
            overwrite(dir, LOGO, &logo)?;
            return Ok(Outcome::Installed);
        }
    }
    replace(dir, LOGO, &logo)?;
    Ok(Outcome::Installed)
}

fn overwrite(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let mut f = fs::OpenOptions::new().write(true).open(dir.join(name))?;
    f.write_all(bytes)?;
    f.sync_all()
}

fn replace(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let tmp = dir.join("bootlogo.tmp");
    let mut f = File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, dir.join(name))?;
    File::open(dir)?.sync_all()
}

pub fn chosen(card: &Path) -> Option<Vec<u8>> {
    for name in [USER_LOGO, CARD_LOGO] {
        let Ok(logo) = fs::read(card.join(name)) else {
            continue;
        };
        if decode(&logo).is_some() {
            return Some(logo);
        }
        eprintln!("slot: bootlogo: {name} is not a 24-bit bmp, skipping it");
    }
    None
}

/// The card's boot logo as RGBA at `size`, for the first frame slot draws: the same picture
/// BaseOS shows while the kernel comes up, so the panel is never left black while the shelf is
/// built. Centred on the logo's own background, as the kernel's copy is.
pub fn splash_rgba(card: &Path, size: (usize, usize)) -> Option<Vec<u8>> {
    let mark = decode(&chosen(card)?)?;
    let img = compose(&mark, size)?;
    let mut rgba = Vec::with_capacity(img.width * img.height * 4);
    for px in &img.rgb {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    Some(rgba)
}

pub fn refresh(card: &Path) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(logo) = chosen(card) else {
        return;
    };
    let Some(device) = partition(PARTITION) else {
        eprintln!("slot: bootlogo: no {PARTITION} partition");
        return;
    };
    if fs::create_dir_all(MOUNT).is_err()
        || !run("mount", &["-t", "vfat", "-o", "rw,noatime", &device, MOUNT])
    {
        eprintln!("slot: bootlogo: could not mount {device}");
        return;
    }
    match install(&logo, Path::new(MOUNT)) {
        Ok(Outcome::Installed) => eprintln!("slot: bootlogo: installed"),
        Ok(Outcome::Invalid) => eprintln!("slot: bootlogo: no logo fits this panel, left alone"),
        Ok(_) => {}
        Err(e) => eprintln!("slot: bootlogo: {e}"),
    }
    if !run("umount", &[MOUNT]) {
        eprintln!("slot: bootlogo: could not unmount {MOUNT}");
    }
}

fn partition(name: &str) -> Option<String> {
    for entry in fs::read_dir("/sys/class/block").ok()?.flatten() {
        let uevent = fs::read_to_string(entry.path().join("uevent")).unwrap_or_default();
        if uevent.lines().any(|l| l == format!("PARTNAME={name}")) {
            return Some(
                PathBuf::from("/dev")
                    .join(entry.file_name())
                    .display()
                    .to_string(),
            );
        }
    }
    None
}

fn run(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

pub fn choices(card: &Path) -> Vec<String> {
    let dir = card.join(OWN);
    let _ = fs::create_dir_all(&dir);
    let mut files: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let path = entry.path();
            let ext = path.extension()?.to_str()?;
            ["png", "jpg", "jpeg", "bmp"]
                .iter()
                .any(|x| ext.eq_ignore_ascii_case(x))
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect();
    files.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    let mut choices = vec!["unslot".into(), "BaseOS".into()];
    choices.extend(files);
    choices
}

pub fn label(choice: &str) -> &str {
    Path::new(choice)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(choice)
}

/// Dimensions are in the panel's landscape coordinate system; compose turns portrait panels.
pub fn fit(img: Bmp, panel: (usize, usize)) -> Bmp {
    let (pw, ph) = if panel.1 > panel.0 {
        (panel.1, panel.0)
    } else {
        panel
    };
    if img.width <= pw && img.height <= ph {
        return img;
    }
    let scale = (pw as f64 / img.width as f64).min(ph as f64 / img.height as f64);
    let width = ((img.width as f64 * scale) as usize).max(1);
    let height = ((img.height as f64 * scale) as usize).max(1);
    let mut rgb = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            rgb.push(img.rgb[(y * img.height / height) * img.width + x * img.width / width]);
        }
    }
    Bmp { width, height, rgb }
}

fn picture(path: &Path) -> io::Result<Bmp> {
    let bytes = fs::read(path)?;
    if let Some(img) = decode(&bytes) {
        return Ok(img);
    }
    let (rgba, width, height) = slot_ui::decode_picture(&bytes)
        .ok_or_else(|| io::Error::other("Could not read that boot logo"))?;
    Ok(Bmp {
        width: width as usize,
        height: height as usize,
        rgb: rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect(),
    })
}

/// Serialized with startup refresh: neither mount nor Config can race another selection.
pub fn select(card: &Path, choice: &str) -> io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let device = partition(PARTITION)
        .ok_or_else(|| io::Error::other("Could not find the boot partition"))?;
    fs::create_dir_all(MOUNT)?;
    if !run("mount", &["-t", "vfat", "-o", "rw,noatime", &device, MOUNT]) {
        return Err(io::Error::other("Could not mount the boot partition"));
    }
    let result = select_in(card, choice, Path::new(MOUNT));
    if !run("umount", &[MOUNT]) {
        return Err(io::Error::other("Could not unmount the boot partition"));
    }
    result
}

/// Selects and installs a logo with the boot-resource partition already mounted at `dir`.
pub fn select_in(card: &Path, choice: &str, dir: &Path) -> io::Result<()> {
    let original = fs::read(dir.join(ORIGINAL)).or_else(|_| fs::read(dir.join(LOGO)))?;
    let panel = size(&original).ok_or_else(|| io::Error::other("Could not read the panel size"))?;
    let bytes = match choice {
        "unslot" => fs::read(card.join(CARD_LOGO))?,
        "BaseOS" => {
            let img = decode(&original)
                .ok_or_else(|| io::Error::other("Could not read the BaseOS boot logo"))?;
            encode(&landscape(img))
        }
        _ => {
            if !choices(card).iter().any(|c| c == choice) {
                return Err(io::Error::other("No such boot logo"));
            }
            encode(&fit(picture(&card.join(OWN).join(choice))?, panel))
        }
    };
    fs::create_dir_all(card.join("Config"))?;
    let path = card.join(USER_LOGO);
    let previous = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let store = |bytes: Option<&[u8]>| -> io::Result<()> {
        match bytes {
            Some(bytes) => slot_store::atomic_write(&path, bytes),
            None => match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
        }
    };
    store((choice != "unslot").then_some(bytes.as_slice()))?;
    let result = install(&bytes, dir).and_then(|outcome| match outcome {
        Outcome::Invalid => Err(io::Error::other("That boot logo does not fit the panel")),
        _ => Ok(()),
    });
    if let Err(e) = result {
        store(previous.as_deref())?;
        return Err(e);
    }
    Ok(())
}
