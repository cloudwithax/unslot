//! Unpacking an update pack where the firmware has no unzip.

use std::path::Path;

/// A zip of `files` as zip writes one: each stored or deflated, with its Unix mode.
fn zip_of(files: &[(&str, &[u8], bool, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut dir = Vec::new();
    for (name, data, deflate, mode) in files {
        let packed = match deflate {
            true => miniz_oxide::deflate::compress_to_vec(data, 6),
            false => data.to_vec(),
        };
        let method: u16 = if *deflate { 8 } else { 0 };
        let local = out.len() as u32;
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend(20u16.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(method.to_le_bytes());
        out.extend([0u8; 8]); // time, date, crc: not checked
        out.extend((packed.len() as u32).to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name.as_bytes());
        out.extend(&packed);
        dir.extend(0x0201_4b50u32.to_le_bytes());
        dir.extend((3u16 << 8 | 20).to_le_bytes());
        dir.extend(20u16.to_le_bytes());
        dir.extend(0u16.to_le_bytes());
        dir.extend(method.to_le_bytes());
        dir.extend([0u8; 8]);
        dir.extend((packed.len() as u32).to_le_bytes());
        dir.extend((data.len() as u32).to_le_bytes());
        dir.extend((name.len() as u16).to_le_bytes());
        dir.extend([0u8; 8]); // extra, comment, disk, internal attributes
        dir.extend(((0o100000 | mode) << 16).to_le_bytes());
        dir.extend(local.to_le_bytes());
        dir.extend(name.as_bytes());
    }
    let dir_at = out.len() as u32;
    out.extend(&dir);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((dir.len() as u32).to_le_bytes());
    out.extend(dir_at.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

fn unzip(zip: &Path, dest: &Path, prefix: &str) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_slot"))
        .arg("--unzip")
        .arg(zip)
        .arg(dest)
        .arg(prefix)
        .output()
        .unwrap()
}

#[test]
fn an_update_pack_unpacks_its_installer_and_system_without_unzip() {
    let d = tempfile::tempdir().unwrap();
    let big: Vec<u8> = (0..200_000u32)
        .flat_map(|i| (i % 251).to_le_bytes())
        .collect();
    let zip = d.path().join("pak.zip");
    std::fs::write(
        &zip,
        zip_of(&[
            (".tmp_update/unslot/System/slot", &big, true, 0o755),
            (
                ".tmp_update/unslot/manifest",
                b"version=v9.9.9\n",
                false,
                0o644,
            ),
            (".tmp_update/a133p.sh", b"#!/bin/sh\n", true, 0o755),
            ("README.txt", b"not ours", false, 0o644),
        ]),
    )
    .unwrap();
    let card = d.path().join("card");
    let out = unzip(&zip, &card, ".tmp_update/");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(card.join(".tmp_update/unslot/System/slot")).unwrap(),
        big
    );
    assert_eq!(
        std::fs::read_to_string(card.join(".tmp_update/unslot/manifest")).unwrap(),
        "version=v9.9.9\n"
    );
    assert!(card.join(".tmp_update/a133p.sh").is_file());
    assert!(
        !card.join("README.txt").exists(),
        "took what was not under the prefix"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(card.join(".tmp_update/a133p.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755, "the installer lost its mode");
    }
}

#[test]
fn an_entry_that_climbs_out_of_the_card_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let zip = d.path().join("evil.zip");
    std::fs::write(
        &zip,
        zip_of(&[(".tmp_update/../../escaped", b"x", false, 0o644)]),
    )
    .unwrap();
    let card = d.path().join("a/card");
    let out = unzip(&zip, &card, ".tmp_update/");
    assert!(!out.status.success());
    assert!(!d.path().join("escaped").exists());
}

#[test]
fn what_is_not_a_zip_fails_rather_than_unpacking_nothing() {
    let d = tempfile::tempdir().unwrap();
    let zip = d.path().join("bad.zip");
    std::fs::write(&zip, b"definitely not a zip").unwrap();
    let out = unzip(&zip, d.path(), "");
    assert!(!out.status.success());
}
