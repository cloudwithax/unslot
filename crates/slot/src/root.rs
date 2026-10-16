use std::path::{Path, PathBuf};

use slot_store::Platform;

/// Every folder slot keeps on the card, each parent before the folders in it.
pub fn dirs() -> Vec<String> {
    let mut dirs: Vec<String> = ["BIOS", "Config", "Logs", "System", "Wallpapers"]
        .map(String::from)
        .to_vec();
    for (top, only_with_bezels) in [
        ("Bezels", true),
        ("Games", false),
        ("Labels", false),
        ("Saves", false),
        ("States", false),
    ] {
        dirs.push(top.to_string());
        dirs.extend(
            Platform::ALL
                .into_iter()
                .filter(|p| !only_with_bezels || p.spec().bezels)
                .map(|p| format!("{top}/{}", p.dir_name())),
        );
    }
    dirs
}

pub fn ensure(root: &Path) {
    for sub in dirs() {
        let _ = std::fs::create_dir_all(root.join(sub));
    }
    if let Err(e) = slot_store::move_config(root) {
        eprintln!("slot: config: {e}");
    }
}

pub fn bios_dir(root: &Path) -> PathBuf {
    root.join("BIOS")
}

const BIOS_FILE: &str = "gba_bios.bin";

const BIOS_BYTES: u64 = 16 * 1024;
const BIOS_FIRST_BYTE: u8 = 0x18;

pub fn has_real_bios(root: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(bios_dir(root).join(BIOS_FILE)) else {
        return false;
    };
    if !f.metadata().is_ok_and(|m| m.len() == BIOS_BYTES) {
        return false;
    }
    let mut first = [0u8; 1];
    std::io::Read::read_exact(&mut f, &mut first).is_ok() && first[0] == BIOS_FIRST_BYTE
}

pub fn logs_dir(root: &Path) -> PathBuf {
    root.join("Logs")
}

pub fn saves_dir(root: &Path) -> PathBuf {
    root.join("Saves")
}
