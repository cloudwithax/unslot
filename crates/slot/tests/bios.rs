mod common;

use std::path::PathBuf;

use common::core_lock;
use slot::core::open_core_for;
use slot_retro::{ButtonMask, LibretroCore};
use slot_store::Core;
use tempfile::tempdir;

fn vendored_core_paths() -> Vec<PathBuf> {
    common::vendored_core().into_iter().collect()
}

#[test]
fn a_missing_bios_folder_still_boots_a_core() {
    let _g = core_lock();
    let d = common::tmp_root_with_real_carts(&["Emerald"]);
    std::fs::remove_dir_all(d.path().join("BIOS")).ok();
    let mut core = open_core_for(d.path(), Core::Mgba, "auto", false, &vendored_core_paths());
    core.load(&d.path().join("Games/GBA/Emerald.gba")).unwrap();
    core.run_frame(ButtonMask::default());
}

#[test]
fn an_empty_bios_folder_still_boots_a_core() {
    let _g = core_lock();
    let d = common::tmp_root_with_real_carts(&["Emerald"]);
    std::fs::create_dir_all(d.path().join("BIOS")).unwrap();
    let mut core = open_core_for(d.path(), Core::Mgba, "auto", false, &vendored_core_paths());
    core.load(&d.path().join("Games/GBA/Emerald.gba")).unwrap();
    core.run_frame(ButtonMask::default());
}

#[test]
fn the_core_is_told_the_bios_folder_not_the_dylib_folder() {
    let _g = core_lock();
    let d = tempdir().unwrap();
    let bios = d.path().join("BIOS");
    let saves = d.path().join("Saves");
    std::fs::create_dir_all(&bios).unwrap();
    std::fs::create_dir_all(&saves).unwrap();
    let Some(dylib) = common::vendored_core() else {
        return;
    };
    let core = LibretroCore::open_with(&dylib, &bios, &saves).unwrap();
    assert_eq!(core.reported_system_dir(), bios.to_string_lossy());
    assert_ne!(
        core.reported_system_dir(),
        dylib.parent().unwrap().to_string_lossy()
    );
}

#[test]
fn the_core_is_told_the_saves_folder_too() {
    let _g = core_lock();
    let d = tempdir().unwrap();
    let bios = d.path().join("BIOS");
    let saves = d.path().join("Saves");
    std::fs::create_dir_all(&bios).unwrap();
    std::fs::create_dir_all(&saves).unwrap();
    let Some(dylib) = common::vendored_core() else {
        return;
    };
    let core = LibretroCore::open_with(&dylib, &bios, &saves).unwrap();
    assert_eq!(core.reported_save_dir(), saves.to_string_lossy());
}

#[test]
fn only_a_real_bios_image_turns_the_boot_splash_on() {
    let d = common::tmp_root_with_carts(&["Emerald"]);
    let bios = d.path().join("BIOS").join("gba_bios.bin");

    assert!(
        !slot::root::has_real_bios(d.path()),
        "an empty BIOS folder counted as a BIOS"
    );

    for (bytes, what) in [
        (vec![], "a zero byte file"),
        (vec![0x18u8; 1024], "a truncated image, right first byte"),
        (vec![0x18u8; 16 * 1024 - 1], "one byte short of an image"),
        (vec![0x18u8; 16 * 1024 + 1], "one byte past an image"),
        (vec![0u8; 16 * 1024], "16 KB that starts like nothing"),
        (vec![0xffu8; 16 * 1024], "16 KB of erased flash"),
    ] {
        std::fs::write(&bios, &bytes).unwrap();
        assert!(
            !slot::root::has_real_bios(d.path()),
            "{what} counted as a BIOS: gpSP would fall back to its built-in one and boot \
             through a splash that does not exist"
        );
    }

    let mut real = vec![0u8; 16 * 1024];
    real[0] = 0x18;
    std::fs::write(&bios, &real).unwrap();
    assert!(
        slot::root::has_real_bios(d.path()),
        "a 16 KB image starting 0x18 is what gpSP itself accepts, and it was refused"
    );
}

#[test]
fn a_missing_bios_folder_turns_the_boot_splash_off() {
    let d = common::tmp_root_with_carts(&["Emerald"]);
    std::fs::remove_dir_all(d.path().join("BIOS")).unwrap();
    assert!(
        !slot::root::has_real_bios(d.path()),
        "a missing BIOS folder counted as a BIOS"
    );
}

#[test]
fn boot_creates_every_content_folder() {
    let d = tempdir().unwrap();
    let _ = slot::app::App::boot(d.path());
    for sub in slot::root::dirs() {
        assert!(d.path().join(&sub).is_dir(), "{sub} was not created");
    }
}

#[test]
fn the_content_root_has_no_art_directory() {
    assert!(slot::root::dirs().iter().any(|d| d == "Labels"));
    assert!(
        !slot::root::dirs().iter().any(|d| d == "Art"),
        "Art survived the rename"
    );
}
