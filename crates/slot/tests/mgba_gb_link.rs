mod common;

use std::path::{Path, PathBuf};

use slot_retro::{ButtonMask, LibretroCore, RetroCore};

fn vendored() -> Option<PathBuf> {
    let p = common::repo_root().join(format!(
        "vendor/mgba_libretro.{}",
        std::env::consts::DLL_EXTENSION
    ));
    if p.exists() { Some(p) } else { None }
}

fn card_cart() -> Option<PathBuf> {
    let p = common::repo_root().join("sdcard/Games/GB/Tetris Rosy Retrospection.gb");
    if p.exists() { Some(p) } else { None }
}

fn link_core(dylib: &Path, player: u8) -> LibretroCore {
    let mut core = LibretroCore::open(dylib).expect("vendored core would not open");
    core.set_option("mgba_link", "on");
    core.set_option("mgba_link_player", &player.to_string());
    core
}

fn lit(px: &[u8]) -> usize {
    px.chunks_exact(4).filter(|p| p[0..3] != [0, 0, 0]).count()
}

#[test]
fn link_mode_accepts_a_game_boy_cart() {
    let _g = common::core_lock();
    let (Some(dylib), Some(rom)) = (vendored(), card_cart()) else {
        eprintln!("no vendored core or no Game Boy cart on this machine, skipping");
        return;
    };
    let mut pair = link_core(&dylib, 0);
    pair.load(&rom).expect("link mode refused a Game Boy rom");
}

#[test]
fn a_linked_game_boy_pair_runs_and_paints() {
    let _g = common::core_lock();
    let (Some(dylib), Some(rom)) = (vendored(), card_cart()) else {
        eprintln!("no vendored core or no Game Boy cart on this machine, skipping");
        return;
    };
    let mut pair = link_core(&dylib, 0);
    pair.load(&rom).expect("link mode refused a Game Boy rom");

    let idle = ButtonMask::default();
    for _ in 0..600 {
        pair.run_frame_linked(idle, idle);
    }
    assert!(
        lit(pair.video_xrgb8888()) > 1_000,
        "the panel is dark after 600 linked frames: nothing was stepped"
    );
}

#[test]
fn a_linked_game_boy_pair_is_deterministic() {
    let _g = common::core_lock();
    let (Some(dylib), Some(rom)) = (vendored(), card_cart()) else {
        eprintln!("no vendored core or no Game Boy cart on this machine, skipping");
        return;
    };
    let run = || {
        let mut pair = link_core(&dylib, 0);
        pair.load(&rom).expect("link mode refused a Game Boy rom");
        let idle = ButtonMask::default();
        for _ in 0..400 {
            pair.run_frame_linked(idle, idle);
        }
        pair.video_xrgb8888().to_vec()
    };
    assert_eq!(run(), run(), "two identical runs painted different frames");
}
