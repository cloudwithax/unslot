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

/// Where a core that keeps its own files in the BIOS folder finds them there: PPSSPP's fonts,
/// atlas and shaders, which ship in System/PPSSPP and are copied over the first time a PSP game
/// starts. An update can replace System/ but never touches BIOS/, so they go across again after
/// one only if they went missing.
pub fn seed_core_files(root: &Path, core: slot_store::Core) {
    let name = match core {
        slot_store::Core::Ppsspp => "PPSSPP",
        _ => return,
    };
    let (from, to) = (root.join("System").join(name), bios_dir(root).join(name));
    if to.join("ppge_atlas.zim").is_file() || !from.is_dir() {
        return;
    }
    if let Err(e) = copy_tree(&from, &to) {
        eprintln!("slot: {}: {e}", to.display());
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ppsspps_files_go_to_the_bios_folder_once() {
        let d = tempfile::tempdir().unwrap();
        let shipped = d.path().join("System/PPSSPP/flash0/font");
        std::fs::create_dir_all(&shipped).unwrap();
        std::fs::write(d.path().join("System/PPSSPP/ppge_atlas.zim"), b"atlas").unwrap();
        std::fs::write(shipped.join("jpn0.pgf"), b"font").unwrap();
        seed_core_files(d.path(), slot_store::Core::Ppsspp);
        let bios = d.path().join("BIOS/PPSSPP");
        assert_eq!(
            std::fs::read(bios.join("flash0/font/jpn0.pgf")).unwrap(),
            b"font"
        );
        // What the player changed there stays.
        std::fs::write(bios.join("ppge_atlas.zim"), b"theirs").unwrap();
        seed_core_files(d.path(), slot_store::Core::Ppsspp);
        assert_eq!(
            std::fs::read(bios.join("ppge_atlas.zim")).unwrap(),
            b"theirs"
        );
    }
}
