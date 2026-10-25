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

/// A pack that checked out: the version it says it is and the update inside it.
#[derive(Debug)]
pub struct Pak {
    pub version: String,
    pub zip: Vec<u8>,
}

/// The name a release's pack goes by.
pub fn file_name(version: &str) -> String {
    format!("unslot-{version}.pak")
}

/// Reads the pack at `path` and checks it against unslot's release key.
pub fn open(path: &Path) -> Result<Pak, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    check(&bytes, &RELEASE_KEY)
}

/// Checks a pack's bytes against `key`: its note is signed by it, says what it is, and names
/// the zip inside by a checksum the zip matches.
pub fn check(bytes: &[u8], key: &[u8; 32]) -> Result<Pak, String> {
    let entries = stored_entries(bytes).ok_or("this is not an unslot pack")?;
    let entry = |name: &str| {
        entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, data)| *data)
            .ok_or_else(|| format!("the pack has no {name}"))
    };
    let note = entry("pak")?;
    let sig = entry("pak.sig")?;
    verify("the pack", note, std::str::from_utf8(sig).unwrap_or_default(), key)?;

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
    let zip = entry(ZIP)?;
    let got: String = Sha256::digest(zip)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !got.eq_ignore_ascii_case(want) {
        return Err(format!("{version}'s zip is not the one it was signed with"));
    }
    Ok(Pak {
        version: version.to_string(),
        zip: zip.to_vec(),
    })
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

/// A zip's entries, by name, when every one is stored rather than compressed, as a pack's are.
fn stored_entries(b: &[u8]) -> Option<Vec<(String, &[u8])>> {
    // The end of the central directory is the last record, after at most a 64 KiB comment.
    let end = (0..=b.len().checked_sub(22)?)
        .rev()
        .take(65_536 + 22)
        .find(|&i| b[i..].starts_with(&[0x50, 0x4b, 0x05, 0x06]))?;
    let (count, mut at) = (u16_at(b, end + 10)?, u32_at(b, end + 16)?);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if !b.get(at..)?.starts_with(&[0x50, 0x4b, 0x01, 0x02]) || u16_at(b, at + 10)? != 0 {
            return None;
        }
        let size = u32_at(b, at + 20)?;
        let (name_len, extra, comment) = (
            u16_at(b, at + 28)?,
            u16_at(b, at + 30)?,
            u16_at(b, at + 32)?,
        );
        let local = u32_at(b, at + 42)?;
        let name = String::from_utf8(b.get(at + 46..at + 46 + name_len)?.to_vec()).ok()?;
        if !b.get(local..)?.starts_with(&[0x50, 0x4b, 0x03, 0x04]) {
            return None;
        }
        let data = local + 30 + u16_at(b, local + 26)? + u16_at(b, local + 28)?;
        out.push((name, b.get(data..data + size)?));
        at += 46 + name_len + extra + comment;
    }
    Some(out)
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
        let mut note = format!("{FORMAT}\nversion={version}\nzip={ZIP}\nsha256={sha}\n");
        let sig = SigningKey::from_bytes(&seed).sign(note.as_bytes());
        tamper(&mut note);
        let sig: String = sig.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        stored_zip(&[
            ("pak", note.as_bytes()),
            ("pak.sig", sig.as_bytes()),
            (ZIP, zip),
        ])
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
