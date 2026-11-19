//! Unpacking a zip onto the card, for the handhelds whose firmware has no `unzip`: TrimUI's
//! A133P firmware ships none, and an update pack is a zip. Stored and deflated entries, which is
//! all `zip` writes, one entry in memory at a time.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

const END: u32 = 0x0605_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;
/// What zip writes for a size or offset too big for its 32 bits, with the real one in zip64's.
const ZIP64: u32 = 0xffff_ffff;
/// The most one entry may inflate to; nothing an update ships comes near it.
const ENTRY_MAX: usize = 512 << 20;

struct Entry {
    name: String,
    method: u16,
    packed: u64,
    size: u64,
    local: u64,
    mode: Option<u32>,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// The zip's entries, from its central directory.
fn entries(file: &mut File) -> Result<Vec<Entry>, String> {
    let len = file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    // The end record is the last 22 bytes, or before a comment of up to 64 KiB.
    let tail_len = len.min(65_557) as usize;
    let mut tail = vec![0u8; tail_len];
    file.seek(SeekFrom::Start(len - tail_len as u64))
        .and_then(|_| file.read_exact(&mut tail))
        .map_err(|e| e.to_string())?;
    let end = (0..tail_len.saturating_sub(21))
        .rev()
        .find(|&i| u32_at(&tail, i) == Some(END))
        .ok_or("not a zip")?;
    let count = u16_at(&tail, end + 10).ok_or("not a zip")? as usize;
    let dir_len = u32_at(&tail, end + 12).ok_or("not a zip")?;
    let dir_at = u32_at(&tail, end + 16).ok_or("not a zip")?;
    if count == 0xffff || dir_len == ZIP64 || dir_at == ZIP64 {
        return Err("a zip64 archive, which this cannot read".into());
    }
    let mut dir = vec![0u8; dir_len as usize];
    file.seek(SeekFrom::Start(dir_at.into()))
        .and_then(|_| file.read_exact(&mut dir))
        .map_err(|e| e.to_string())?;
    let bad = || "its directory is damaged".to_string();
    let mut out = Vec::with_capacity(count);
    let mut at = 0;
    for _ in 0..count {
        if u32_at(&dir, at) != Some(CENTRAL) {
            return Err(bad());
        }
        let made_by = u16_at(&dir, at + 4).ok_or_else(bad)?;
        let method = u16_at(&dir, at + 10).ok_or_else(bad)?;
        let packed = u32_at(&dir, at + 20).ok_or_else(bad)?;
        let size = u32_at(&dir, at + 24).ok_or_else(bad)?;
        let name_len = u16_at(&dir, at + 28).ok_or_else(bad)? as usize;
        let extra = u16_at(&dir, at + 30).ok_or_else(bad)? as usize;
        let comment = u16_at(&dir, at + 32).ok_or_else(bad)? as usize;
        let external = u32_at(&dir, at + 38).ok_or_else(bad)?;
        let local = u32_at(&dir, at + 42).ok_or_else(bad)?;
        if packed == ZIP64 || size == ZIP64 || local == ZIP64 {
            return Err("a zip64 archive, which this cannot read".into());
        }
        let name = dir.get(at + 46..at + 46 + name_len).ok_or_else(bad)?;
        // Unix keeps a file's mode in the high half of the external attributes.
        let mode = (made_by >> 8 == 3).then_some(external >> 16 & 0o777);
        out.push(Entry {
            name: String::from_utf8_lossy(name).into_owned(),
            method,
            packed: packed.into(),
            size: size.into(),
            local: local.into(),
            mode,
        });
        at += 46 + name_len + extra + comment;
    }
    Ok(out)
}

/// Where an entry goes under `dest`, or none for a name that would climb out of it.
fn target(dest: &Path, name: &str) -> Option<PathBuf> {
    let rel = Path::new(name.trim_end_matches('/'));
    let mut out = dest.to_path_buf();
    for part in rel.components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (out != dest).then_some(out)
}

/// Unpacks the entries of `zip` whose names start with `prefix` under `dest`, keeping the modes
/// zip noted. Answers how many files it wrote.
pub fn extract(zip: &Path, dest: &Path, prefix: &str) -> Result<usize, String> {
    let mut file = File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut written = 0;
    for entry in entries(&mut file)? {
        if !entry.name.starts_with(prefix) {
            continue;
        }
        let path = target(dest, &entry.name)
            .ok_or_else(|| format!("{} would land outside {}", entry.name, dest.display()))?;
        if entry.name.ends_with('/') {
            std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            continue;
        }
        if entry.size as usize > ENTRY_MAX {
            return Err(format!("{} is too big to unpack", entry.name));
        }
        let mut header = [0u8; 30];
        file.seek(SeekFrom::Start(entry.local))
            .and_then(|_| file.read_exact(&mut header))
            .map_err(|e| e.to_string())?;
        if u32_at(&header, 0) != Some(LOCAL) {
            return Err(format!("{} is damaged", entry.name));
        }
        let skip = u64::from(u16_at(&header, 26).unwrap_or(0))
            + u64::from(u16_at(&header, 28).unwrap_or(0));
        let mut packed = vec![0u8; entry.packed as usize];
        file.seek(SeekFrom::Current(skip as i64))
            .and_then(|_| file.read_exact(&mut packed))
            .map_err(|e| e.to_string())?;
        let data = match entry.method {
            0 => packed,
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, entry.size as usize)
                .map_err(|_| format!("{} is damaged", entry.name))?,
            m => {
                return Err(format!(
                    "{} is packed in a way this cannot read ({m})",
                    entry.name
                ))
            }
        };
        if data.len() as u64 != entry.size {
            return Err(format!("{} is damaged", entry.name));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, &data).map_err(|e| format!("{}: {e}", path.display()))?;
        #[cfg(unix)]
        if let Some(mode) = entry.mode.filter(|m| *m != 0) {
            use std::os::unix::fs::PermissionsExt;
            // FAT has no modes to keep; the file is there all the same.
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode));
        }
        written += 1;
    }
    Ok(written)
}
