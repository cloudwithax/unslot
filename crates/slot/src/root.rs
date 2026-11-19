use std::path::{Path, PathBuf};

use slot_store::Platform;

/// Every folder slot keeps on the card, each parent before the folders in it.
pub fn dirs() -> Vec<String> {
    let mut dirs: Vec<String> = [
        "BIOS",
        "Books",
        "Bootlogos",
        "Config",
        "Logs",
        "Music",
        "Pictures",
        "Ports",
        "Shaders",
        "System",
        "Wallpapers",
    ]
    .map(String::from)
    .to_vec();
    for top in ["Bezels", "Games", "Labels", "Manuals", "Saves", "States"] {
        dirs.push(top.to_string());
        dirs.extend(
            Platform::ALL
                .into_iter()
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

/// Clears what a run that went down mid-write left in the folders slot writes to.
pub fn sweep_temps(root: &Path) {
    for sub in ["Config", "Saves", "States"] {
        slot_store::sweep_temps(&root.join(sub));
    }
}

pub fn bios_dir(root: &Path) -> PathBuf {
    root.join("BIOS")
}

/// The first of a platform's BIOS files that is not on the card, as its spec names it. An entry
/// of `a|b` is met by either; a name starting `@` is looked for in the system's own Games folder,
/// where FinalBurn Neo also looks for neogeo.zip, rather than in BIOS.
pub fn missing_bios(root: &Path, platform: Platform) -> Option<&'static str> {
    let bios = bios_dir(root);
    let games = root.join("Games").join(platform.dir_name());
    platform.spec().bios.iter().copied().find(|entry| {
        !entry.split('|').any(|name| match name.strip_prefix('@') {
            Some(name) => games.join(name).is_file(),
            None => bios.join(name).is_file(),
        })
    })
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
