//! unslot's update packs, `unslot-<version>.pak`: the `MinUI.zip` BaseOS installs, untouched,
//! beside a note of its version and checksum that unslot's release signed. A pack whose note was
//! changed, whose zip does not match the note, or that unslot did not sign is not installed, so
//! nothing but a release unslot made goes in through the Updates page or the card's root.
//!
//! The pack is a zip whose entries are stored, not compressed:
//!
//!   pak        format=unslot-pak/1, version=<tag>, zip=MinUI.zip, sha256=<hex of the zip>
//!   pak.sig    the ed25519 signature of `pak`, in hex
//!   MinUI.zip  the update itself, as tools/update/pack.sh made it

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

/// unslot's releases are signed with the key this is the public half of. The private half is
/// the repository's `UNSLOT_PAK_KEY` secret.
pub const RELEASE_KEY: [u8; 32] = [
    0xab, 0xf3, 0xe4, 0x7a, 0x8f, 0x83, 0x60, 0x3f, 0x2e, 0xef, 0x8c, 0x43, 0x5b, 0xd5, 0x95, 0x24,
    0x46, 0x28, 0xff, 0x84, 0xad, 0x5c, 0x29, 0xc2, 0xd4, 0xd2, 0x3d, 0x47, 0x09, 0xa6, 0x4a, 0x9a,
];

const FORMAT: &str = "format=unslot-pak/1";
const ZIP: &str = "MinUI.zip";

/// The name a release's pack goes by.
pub fn file_name(version: &str) -> String {
    format!("unslot-{version}.pak")
}

/// Checks a pack on disk without keeping its update in memory.
pub fn check_file(path: &Path, key: &[u8; 32]) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    check(&mut file, key)
}

/// Checks a seekable pack against `key`, discarding the verified update.
pub fn check(reader: &mut (impl Read + Seek), key: &[u8; 32]) -> Result<String, String> {
    read_pack(reader, key, &mut std::io::sink())
}

/// Copies and checks the update in one pass; a failed copy is never left to install.
pub fn unpack(path: &Path, out: &Path, key: &[u8; 32]) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut output = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let result = read_pack(&mut file, key, &mut output);
    drop(output);
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

fn read_pack(
    reader: &mut (impl Read + Seek),
    key: &[u8; 32],
    output: &mut impl Write,
) -> Result<String, String> {
    let entries = regions(reader).map_err(|_| "this is not an unslot pack")?;
    let entry = |index: usize| {
        entries[index].ok_or_else(|| format!("the pack has no {}", ["pak", "pak.sig", ZIP][index]))
    };
    let mut small = |index| -> Result<Vec<u8>, String> {
        let (at, size) = entry(index)?;
        if size > 65_536 {
            return Err("the pack's note or signature is too large".into());
        }
        let mut bytes = vec![0; size as usize];
        reader
            .seek(SeekFrom::Start(at))
            .map_err(|e| e.to_string())?;
        reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    };
    let note = small(0)?;
    let sig = small(1)?;
    let (version, want) = check_note(&note, &sig, key)?;
    let (at, mut remaining) = entry(2)?;
    reader
        .seek(SeekFrom::Start(at))
        .map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65_536];
    while remaining != 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        reader
            .read_exact(&mut buffer[..count])
            .map_err(|e| e.to_string())?;
        hash.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        remaining -= count as u64;
    }
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !got.eq_ignore_ascii_case(want) {
        return Err(format!("{version}'s zip is not the one it was signed with"));
    }
    Ok(version.to_string())
}

fn check_note<'a>(
    note: &'a [u8],
    sig: &[u8],
    key: &[u8; 32],
) -> Result<(&'a str, &'a str), String> {
    verify(
        "the pack",
        note,
        std::str::from_utf8(sig).unwrap_or_default(),
        key,
    )?;

    let note = std::str::from_utf8(note).map_err(|_| "the pack's note is not text")?;
    let field = |key: &str| {
        note.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
    };
    if !note.lines().any(|l| l.trim() == FORMAT) || field("zip") != Some(ZIP) {
        return Err("the pack is not one this unslot can install".into());
    }
    let version = field("version")
        .filter(|v| !v.is_empty())
        .ok_or("the pack does not say what version it is")?;
    let want = field("sha256").ok_or("the pack does not say what its zip is")?;
    Ok((version, want))
}

/// Checks that `signed`, which complaints call `what`, is what `key` signed, by its signature
/// in hex.
pub fn verify(what: &str, signed: &[u8], sig: &str, key: &[u8; 32]) -> Result<(), String> {
    let sig = hex(sig.trim())
        .and_then(|b| <[u8; 64]>::try_from(b).ok())
        .ok_or(format!("{what}'s signature is not one"))?;
    let key = VerifyingKey::from_bytes(key).map_err(|_| "the release key is not one")?;
    key.verify_strict(signed, &Signature::from_bytes(&sig))
        .map_err(|_| format!("{what} was not signed by unslot, or was changed since"))
}

fn hex(s: &str) -> Option<Vec<u8>> {
    s.len()
        .is_multiple_of(2)
        .then(|| {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
                .collect()
        })
        .flatten()
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

type Regions = [Option<(u64, u64)>; 3];

// Read directory records individually: even an archive full of extras stays bounded.
fn regions(reader: &mut (impl Read + Seek)) -> std::io::Result<Regions> {
    let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidData);
    let len = reader.seek(SeekFrom::End(0))?;
    let tail_len = len.min(65_536 + 22) as usize;
    let mut tail = vec![0; tail_len];
    reader.seek(SeekFrom::Start(len - tail_len as u64))?;
    reader.read_exact(&mut tail)?;
    let end = (0..=tail_len.checked_sub(22).ok_or_else(invalid)?)
        .rev()
        .find(|&i| tail[i..].starts_with(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or_else(invalid)?;
    let count = u16_at(&tail, end + 10).ok_or_else(invalid)?;
    let mut at = u32_at(&tail, end + 16).ok_or_else(invalid)? as u64;
    drop(tail);
    let mut entries = [None; 3];
    for _ in 0..count {
        reader.seek(SeekFrom::Start(at))?;
        let mut header = [0; 46];
        reader.read_exact(&mut header)?;
        if !header.starts_with(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(invalid());
        }
        let name_len = u16_at(&header, 28).ok_or_else(invalid)?;
        at += (46
            + name_len
            + u16_at(&header, 30).ok_or_else(invalid)?
            + u16_at(&header, 32).ok_or_else(invalid)?) as u64;
        if u16_at(&header, 10) != Some(0) {
            continue;
        }
        let mut name = vec![0; name_len];
        reader.read_exact(&mut name)?;
        let Some(index) = [b"pak".as_slice(), b"pak.sig", ZIP.as_bytes()]
            .iter()
            .position(|&n| n == name)
        else {
            continue;
        };
        if entries[index].is_some() {
            continue;
        }
        let local = u32_at(&header, 42).ok_or_else(invalid)? as u64;
        let size = u32_at(&header, 20).ok_or_else(invalid)? as u64;
        reader.seek(SeekFrom::Start(local))?;
        let mut header = [0; 30];
        reader.read_exact(&mut header)?;
        if !header.starts_with(&[0x50, 0x4b, 0x03, 0x04]) || u16_at(&header, 8) != Some(0) {
            return Err(invalid());
        }
        let data = local
            + 30
            + u16_at(&header, 26).ok_or_else(invalid)? as u64
            + u16_at(&header, 28).ok_or_else(invalid)? as u64;
        if data + size > len {
            return Err(invalid());
        }
        entries[index] = Some((data, size));
    }
    Ok(entries)
}

/// Packs made here, for the pretend updater on the desktop and for the tests.
pub mod build {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// The pretend releases' key, which unslot's own does not take.
    pub const TEST_SEED: [u8; 32] = [7; 32];

    pub fn test_key() -> [u8; 32] {
        public_key(TEST_SEED)
    }

    /// A release's pack: `zip`, untouched, with a note of `version` and its checksum signed by
    /// the key `seed` is the secret half of.
    pub fn signed(version: &str, zip: &[u8], seed: [u8; 32]) -> Vec<u8> {
        pack(version, zip, seed, |_| {})
    }

    /// `bytes`' signature by the key `seed` is the secret half of, in hex, as `verify` reads it.
    pub fn sign(bytes: &[u8], seed: [u8; 32]) -> String {
        let sig = SigningKey::from_bytes(&seed).sign(bytes);
        sig.to_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The public half of the key `seed` is the secret half of.
    pub fn public_key(seed: [u8; 32]) -> [u8; 32] {
        SigningKey::from_bytes(&seed).verifying_key().to_bytes()
    }

    /// A pack as `make_pak` writes one, its note changed by `tamper` once it is signed.
    pub fn pack(
        version: &str,
        zip: &[u8],
        seed: [u8; 32],
        tamper: impl Fn(&mut String),
    ) -> Vec<u8> {
        let sha: String = Sha256::digest(zip)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut note = note(version, &sha);
        let sig = SigningKey::from_bytes(&seed).sign(note.as_bytes());
        tamper(&mut note);
        let sig: String = sig.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        stored_zip(&[
            ("pak", note.as_bytes()),
            ("pak.sig", sig.as_bytes()),
            (ZIP, zip),
        ])
    }

    pub(super) fn note(version: &str, sha: &str) -> String {
        format!("{FORMAT}\nversion={version}\nzip={ZIP}\nsha256={sha}\n")
    }

    /// A zip of stored entries, with no extras, as `zip -0 -X` makes.
    pub fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for (name, data) in entries {
            let offset = out.len() as u32;
            let crc = crc32fast::hash(data);
            let header = |sig: [u8; 4], central_dir: bool| {
                let mut h = sig.to_vec();
                if central_dir {
                    h.extend_from_slice(&20u16.to_le_bytes());
                }
                h.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
                h.extend_from_slice(&crc.to_le_bytes());
                h.extend_from_slice(&(data.len() as u32).to_le_bytes());
                h.extend_from_slice(&(data.len() as u32).to_le_bytes());
                h.extend_from_slice(&(name.len() as u16).to_le_bytes());
                h.extend_from_slice(&0u16.to_le_bytes());
                if central_dir {
                    // Comment length, disk, internal and external attributes.
                    h.extend_from_slice(&[0; 10]);
                    h.extend_from_slice(&offset.to_le_bytes());
                }
                h.extend_from_slice(name.as_bytes());
                h
            };
            out.extend(header([0x50, 0x4b, 0x03, 0x04], false));
            out.extend_from_slice(data);
            central.extend(header([0x50, 0x4b, 0x01, 0x02], true));
        }
        let start = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::build::*;
    use super::*;
    use std::io::Cursor;

    #[derive(Debug)]
    struct Checked {
        version: String,
        zip: Vec<u8>,
    }

    fn check(bytes: &[u8], key: &[u8; 32]) -> Result<Checked, String> {
        let mut zip = Vec::new();
        let version = read_pack(&mut Cursor::new(bytes), key, &mut zip)?;
        Ok(Checked { version, zip })
    }

    fn stored_entries(bytes: &[u8]) -> Option<Vec<(String, &[u8])>> {
        let entries = regions(&mut Cursor::new(bytes)).ok()?;
        ["pak", "pak.sig", ZIP]
            .into_iter()
            .zip(entries)
            .map(|(name, region)| {
                let (at, size) = region?;
                Some((
                    name.to_string(),
                    bytes.get(at as usize..(at + size) as usize)?,
                ))
            })
            .collect()
    }

    #[test]
    fn unpack_copies_the_zip_and_removes_a_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.pak");
        let out = dir.path().join("MinUI.zip");
        let zip = vec![42; 200_000];
        let good = pack("v2.11.0", &zip, TEST_SEED, |_| {});
        std::fs::write(&path, &good).unwrap();
        assert_eq!(unpack(&path, &out, &test_key()).unwrap(), "v2.11.0");
        assert_eq!(std::fs::read(&out).unwrap(), zip);
        let entries = stored_entries(&good).unwrap();
        let swapped = stored_zip(&[
            ("pak", entries[0].1),
            ("pak.sig", entries[1].1),
            (ZIP, b"swapped"),
        ]);
        std::fs::write(&path, swapped).unwrap();
        assert!(unpack(&path, &out, &test_key())
            .unwrap_err()
            .contains("not the one it was signed with"));
        assert!(!out.exists());
    }

    // The signed note from the published v2.11.0 pack, fetched with curl -L -r 0-4095.
    const RELEASE_NOTE: &str = "format=unslot-pak/1\nversion=v2.11.0\nzip=MinUI.zip\nsha256=402d2a611817d08393764f4203c1ec42813249ddfbbd682148beaec5e94167b1\n";
    const RELEASE_SIG: &str = "332398137cc5e0b1c89f59122ab3fcd5d16c6cf44ee812bb3d94d4dd5f582a6f94a7c19f7ace5f0abaff03503c8dbddc680b896e1b2daaa367b472696716210d";
    const RELEASE_SHA: &str = "402d2a611817d08393764f4203c1ec42813249ddfbbd682148beaec5e94167b1";

    #[test]
    fn the_published_release_note_verifies() {
        assert_eq!(
            check_note(
                RELEASE_NOTE.as_bytes(),
                RELEASE_SIG.as_bytes(),
                &RELEASE_KEY
            )
            .unwrap(),
            ("v2.11.0", RELEASE_SHA)
        );
    }

    #[test]
    fn the_writer_keeps_the_published_note_format() {
        assert_eq!(build::note("v2.11.0", RELEASE_SHA), RELEASE_NOTE);
    }

    #[test]
    fn signed_unknown_lines_and_crlf_are_accepted() {
        let bytes = pack("v2.11.0", b"the update", TEST_SEED, |_| {});
        let entries = stored_entries(&bytes).unwrap();
        let note = format!(
            "future=before\r\n{}future=after\r\n",
            std::str::from_utf8(entries[0].1)
                .unwrap()
                .replace('\n', "\r\n")
        );
        let sig = sign(note.as_bytes(), TEST_SEED);
        let bytes = stored_zip(&[
            ("pak", note.as_bytes()),
            ("pak.sig", sig.as_bytes()),
            (ZIP, b"the update"),
        ]);
        let pak = check(&bytes, &test_key()).unwrap();
        assert_eq!(pak.version, "v2.11.0");
        assert_eq!(pak.zip, b"the update");
    }

    #[test]
    fn a_deflated_extra_is_ignored() {
        let good = pack("v2.11.0", b"the update", TEST_SEED, |_| {});
        let entries = stored_entries(&good).unwrap();
        let extra = b"future metadata";
        let deflated = miniz_oxide::deflate::compress_to_vec(extra, 6);
        let mut bytes = stored_zip(&[
            ("pak", entries[0].1),
            ("pak.sig", entries[1].1),
            (ZIP, entries[2].1),
            ("extra", &deflated),
        ]);
        let end = bytes.len() - 22;
        let mut at = u32_at(&bytes, end + 16).unwrap();
        for _ in 0..3 {
            at += 46 + u16_at(&bytes, at + 28).unwrap();
        }
        let local = u32_at(&bytes, at + 42).unwrap();
        bytes[at + 10..at + 12].copy_from_slice(&8u16.to_le_bytes());
        bytes[local + 8..local + 10].copy_from_slice(&8u16.to_le_bytes());
        for (crc, size) in [(at + 16, at + 24), (local + 14, local + 22)] {
            bytes[crc..crc + 4].copy_from_slice(&crc32fast::hash(extra).to_le_bytes());
            bytes[size..size + 4].copy_from_slice(&(extra.len() as u32).to_le_bytes());
        }
        let pak = check(&bytes, &test_key()).unwrap();
        assert_eq!(pak.version, "v2.11.0");
        assert_eq!(pak.zip, b"the update");
    }

    #[test]
    fn required_entries_must_be_stored_and_present() {
        let good = pack("v2.11.0", b"the update", TEST_SEED, |_| {});
        let entries = stored_entries(&good).unwrap();
        for index in 0..3 {
            let mut bytes = good.clone();
            let mut at = u32_at(&bytes, bytes.len() - 6).unwrap();
            for _ in 0..index {
                at += 46 + u16_at(&bytes, at + 28).unwrap();
            }
            bytes[at + 10..at + 12].copy_from_slice(&8u16.to_le_bytes());
            assert!(check(&bytes, &test_key()).is_err());
            let remaining: Vec<_> = entries
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .map(|(_, (name, data))| (name.as_str(), *data))
                .collect();
            assert!(check(&stored_zip(&remaining), &test_key()).is_err());
        }
    }

    #[test]
    fn a_signed_pack_gives_up_its_zip_untouched() {
        let pak = check(
            &pack("v2.2.0", b"the update", TEST_SEED, |_| {}),
            &test_key(),
        )
        .unwrap();
        assert_eq!(pak.version, "v2.2.0");
        assert_eq!(pak.zip, b"the update");
    }

    #[test]
    fn a_changed_version_is_refused() {
        let bytes = pack("v2.2.0", b"the update", TEST_SEED, |n| {
            *n = n.replace("v2.2.0", "v9.9.9");
        });
        let e = check(&bytes, &test_key()).unwrap_err();
        assert!(e.contains("not signed by unslot"), "{e}");
    }

    #[test]
    fn a_swapped_zip_is_refused() {
        let good = pack("v2.2.0", b"the update", TEST_SEED, |_| {});
        let entries = stored_entries(&good).unwrap();
        let (note, sig) = (entries[0].1, entries[1].1);
        let swapped = stored_zip(&[("pak", note), ("pak.sig", sig), (ZIP, b"something else")]);
        let e = check(&swapped, &test_key()).unwrap_err();
        assert!(e.contains("not the one it was signed with"), "{e}");
    }

    #[test]
    fn a_pack_signed_with_another_key_is_refused() {
        let bytes = pack("v2.2.0", b"the update", [9; 32], |_| {});
        assert!(check(&bytes, &test_key()).is_err());
        // Nor does unslot's own key take a test pack.
        assert!(check(&pack("v2.2.0", b"x", TEST_SEED, |_| {}), &RELEASE_KEY).is_err());
    }

    #[test]
    fn a_plain_zip_is_not_a_pack() {
        assert!(check(b"PK not really", &test_key()).is_err());
        let zip = stored_zip(&[(ZIP, b"the update")]);
        assert!(check(&zip, &test_key()).unwrap_err().contains("no pak"));
    }

    #[test]
    fn a_pack_is_named_for_its_version() {
        assert_eq!(file_name("v2.2.0"), "unslot-v2.2.0.pak");
    }
}
