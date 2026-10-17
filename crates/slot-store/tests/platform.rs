use slot_store::Platform;

#[test]
fn every_platform_has_a_directory() {
    let names: Vec<&str> = Platform::ALL.iter().map(|p| p.dir_name()).collect();
    // The first systems keep the folders they had; the rest follow.
    assert_eq!(
        names[..13],
        ["GBA", "GB", "GBC", "NES", "SNES", "MD", "SMS", "GG", "PCE", "NGP", "WS", "LYNX", "PS"]
    );
    assert_eq!(names.len(), Platform::COUNT);
    assert!(names.iter().all(|n| !n.is_empty()));
}

#[test]
fn every_platform_is_a_shelf_of_its_own() {
    assert_eq!(
        Platform::ALL[..3],
        [Platform::Gba, Platform::Gb, Platform::Gbc],
        "the Game Boy family's shelves come first, as they always have"
    );
    for (i, p) in Platform::ALL.iter().enumerate() {
        assert!(
            !Platform::ALL[..i].contains(p),
            "{p:?} is named twice in the ring, so two shelves would hold one platform"
        );
    }
}

#[test]
fn each_platform_takes_only_its_own_extensions() {
    assert!(Platform::Gba.accepts(std::path::Path::new("Metroid Fusion.gba")));
    assert!(!Platform::Gba.accepts(std::path::Path::new("Tetris.gb")));
    assert!(Platform::Gb.accepts(std::path::Path::new("Tetris.gb")));
    assert!(Platform::Gb.accepts(std::path::Path::new("Tetris.gbc")));
    assert!(!Platform::Gb.accepts(std::path::Path::new("Metroid Fusion.gba")));
    assert!(Platform::Gba.accepts(std::path::Path::new("Shrek.GBA")));
}

#[test]
fn a_disc_platform_takes_a_playlist_and_a_disc_image_but_not_its_tracks() {
    let ps = Platform::PlayStation;
    for name in ["Game.cue", "Game.chd", "Game.m3u", "Game.pbp"] {
        assert!(ps.accepts(std::path::Path::new(name)), "{name}");
    }
    assert!(!ps.accepts(std::path::Path::new("Game (Track 1).bin")));
}
