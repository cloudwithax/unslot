use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const TITLE_OFF: u64 = 0xa0;
const TITLE_LEN: usize = 12;

const CODE_OFF: u64 = 0xac;
const CODE_LEN: usize = 4;

pub fn title(rom: &Path) -> Option<String> {
    field(rom, TITLE_OFF, &mut [0u8; TITLE_LEN])
}

pub fn code(rom: &Path) -> Option<String> {
    field(rom, CODE_OFF, &mut [0u8; CODE_LEN])
}

pub fn title_code(rom: &Path) -> Option<(String, String)> {
    let mut f = File::open(rom).ok()?;
    f.seek(SeekFrom::Start(TITLE_OFF)).ok()?;
    let mut head = [0u8; (CODE_OFF - TITLE_OFF) as usize + CODE_LEN];
    f.read_exact(&mut head).ok()?;
    let (t, c) = head.split_at(TITLE_LEN);
    Some((text(t).unwrap_or_default(), text(c).unwrap_or_default()))
}

pub fn header_clean(rom: &Path) -> bool {
    let mut f = match File::open(rom) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let meta = match f.metadata() {
        Ok(m) => m,
        Err(_) => return false,
    };
    let mut head = [0u8; 0xB3];
    f.read_exact(&mut head).is_ok()
        && head[3] == 0xEA
        && head[0xB2] == 0x96
        && meta.len() <= 16 * 1024 * 1024
}

fn field(rom: &Path, off: u64, buf: &mut [u8]) -> Option<String> {
    let mut f = File::open(rom).ok()?;
    f.seek(SeekFrom::Start(off)).ok()?;
    f.read_exact(buf).ok()?;
    text(buf)
}

fn text(buf: &[u8]) -> Option<String> {
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    let s = std::str::from_utf8(&buf[..end]).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::text;

    #[test]
    fn padding_is_trimmed_but_interior_spaces_are_kept() {
        let full = text(b"POKEMON EMER");
        assert_eq!(full.as_deref(), Some("POKEMON EMER"));
        assert_eq!(text(b"ADVANCEWARS\0").as_deref(), Some("ADVANCEWARS"));
        assert_eq!(text(b"KIRBY      \0").as_deref(), Some("KIRBY"));
        assert_eq!(text(&[0u8; 12]), None);
    }
}
