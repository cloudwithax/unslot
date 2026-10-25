use slot_store::{
    core_for, core_for_platform, core_for_system, read_selected_cores, write_game_core,
    write_selected_core, write_system_core, Core, Platform, CORES_DIR,
};
use tempfile::tempdir;

fn root_with(ini: Option<&str>) -> tempfile::TempDir {
    let d = tempdir().unwrap();
    std::fs::create_dir(d.path().join("Config")).unwrap();
    if let Some(text) = ini {
        std::fs::write(d.path().join("Config/selected_core.txt"), text).unwrap();
    }
    d
}

#[test]
fn absent_file_means_everything_defaults() {
    let d = root_with(None);
    assert!(read_selected_cores(d.path()).is_empty());
    assert_eq!(core_for(d.path(), "Emerald"), Core::Mgba);
}

#[test]
fn a_listed_stem_gets_its_core() {
    let d = root_with(Some("Pokemon - Emerald Version (USA, Europe) = gpsp\n"));
    assert_eq!(
        core_for(d.path(), "Pokemon - Emerald Version (USA, Europe)"),
        Core::Gpsp
    );
}

#[test]
fn an_unlisted_stem_defaults_to_mgba() {
    let d = root_with(Some("Emerald = gpsp\n"));
    assert_eq!(core_for(d.path(), "Metroid Fusion"), Core::Mgba);
}

#[test]
fn malformed_lines_are_ignored_rather_than_fatal() {
    let d = root_with(Some(concat!(
        "\n",
        "# a comment\n",
        "; another comment\n",
        "[cores]\n",
        "no equals sign here\n",
        "Emerald = notacore\n",
        "  Spaced Out   =   gpsp  \n",
        "= gpsp\n",
        "Trailing =\n",
    )));
    let map = read_selected_cores(d.path());
    assert_eq!(
        map.get("Spaced Out"),
        Some(&Core::Gpsp),
        "whitespace not trimmed"
    );
    assert_eq!(
        map.get("Emerald"),
        None,
        "an unknown core name must not be stored"
    );
    assert_eq!(core_for(d.path(), "Emerald"), Core::Mgba);
    assert_eq!(map.len(), 1, "only the one good line should survive");
}

#[test]
fn a_later_duplicate_wins() {
    let d = root_with(Some("Emerald = mgba\nEmerald = gpsp\n"));
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
}

#[test]
fn the_last_line_for_a_cart_wins_even_when_it_is_the_typo() {
    let d = root_with(Some("Emerald = gpsp\nEmerald = notacore\n"));
    assert_eq!(
        core_for(d.path(), "Emerald"),
        Core::Mgba,
        "a line the later one superseded is still in force"
    );
    assert_eq!(
        read_selected_cores(d.path()).get("Emerald"),
        None,
        "the map and core_for read the same two lines differently"
    );

    let d = root_with(Some("Emerald = notacore\nEmerald = gpsp\n"));
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
    assert_eq!(
        read_selected_cores(d.path()).get("Emerald"),
        Some(&Core::Gpsp)
    );
}

#[test]
fn core_names_round_trip() {
    for c in Core::ALL {
        assert_eq!(Core::parse(c.as_str()), Some(c));
    }
    assert_eq!(
        Core::parse("MGBA"),
        Some(Core::Mgba),
        "case is not the user's problem"
    );
    assert_eq!(Core::parse("nonsense"), None);
    assert_eq!(Core::default(), Core::Mgba);
}

#[test]
fn writing_a_core_creates_the_file_when_absent() {
    let d = root_with(None);
    slot_store::write_selected_core(d.path(), "Emerald", Core::Gpsp).unwrap();
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
}

#[test]
fn writing_a_core_replaces_that_carts_line_and_leaves_the_rest_alone() {
    let d = root_with(Some(concat!(
        "# my notes\n",
        "\n",
        "Emerald = mgba\n",
        "Metroid Fusion = gpsp\n",
    )));
    slot_store::write_selected_core(d.path(), "Emerald", Core::Gpsp).unwrap();

    let text = std::fs::read_to_string(d.path().join("Config/selected_core.txt")).unwrap();
    assert!(
        text.contains("# my notes"),
        "a hand-written comment was destroyed"
    );
    assert!(
        text.contains("Metroid Fusion = gpsp"),
        "another cart's entry was lost"
    );
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
    assert_eq!(core_for(d.path(), "Metroid Fusion"), Core::Gpsp);
    assert_eq!(
        text.matches("Emerald").count(),
        1,
        "the old line was left behind"
    );
}

#[test]
fn writing_a_core_appends_a_cart_the_file_has_never_seen() {
    let d = root_with(Some("Emerald = gpsp\n"));
    slot_store::write_selected_core(d.path(), "Drill Dozer", Core::Gpsp).unwrap();
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
    assert_eq!(core_for(d.path(), "Drill Dozer"), Core::Gpsp);
}

#[test]
fn writing_the_default_still_records_it() {
    let d = root_with(Some("Emerald = gpsp\n"));
    slot_store::write_selected_core(d.path(), "Emerald", Core::Mgba).unwrap();
    assert_eq!(core_for(d.path(), "Emerald"), Core::Mgba);
    let text = std::fs::read_to_string(d.path().join("Config/selected_core.txt")).unwrap();
    assert!(text.contains("Emerald = mgba"));
}

#[test]
fn every_platform_defaults_to_mgba() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        core_for_platform(d.path(), "Tetris", Platform::Gb),
        Core::Mgba
    );
    assert_eq!(
        core_for_platform(d.path(), "Tetris Chromatic", Platform::Gbc),
        Core::Mgba
    );
    assert_eq!(
        core_for_platform(d.path(), "Emerald", Platform::Gba),
        Core::Mgba
    );
}

#[test]
fn a_cart_can_ask_for_a_non_default_core_by_hand() {
    let d = tempfile::tempdir().unwrap();
    write_selected_core(d.path(), "Emerald", Core::Gpsp).unwrap();
    assert_ne!(Core::default_for(Platform::Gba), Core::Gpsp);
    assert_eq!(
        core_for_platform(d.path(), "Emerald", Platform::Gba),
        Core::Gpsp
    );
}

#[test]
fn a_line_naming_a_core_the_platform_cannot_run_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    write_selected_core(d.path(), "Tetris", Core::Gpsp).unwrap();
    assert_eq!(
        core_for_platform(d.path(), "Tetris", Platform::Gb),
        Core::Mgba
    );
    assert_eq!(
        core_for_platform(d.path(), "Tetris", Platform::Gbc),
        Core::Mgba
    );
}

#[test]
fn the_picker_boards_two_sockets_are_the_gbas_own_cores() {
    // Past these the GBA's cores are downloaded, and once one is it picks from a list instead.
    let cores = Platform::Gba.spec().cores;
    assert_eq!(cores[..2], [Core::Mgba, Core::Gpsp]);
    assert!(cores[2..].iter().all(|c| c.download_only()));
}

#[test]
fn every_systems_default_core_comes_with_unslot() {
    for p in Platform::ALL {
        assert!(!Core::default_for(p).download_only(), "{p:?}'s default is downloaded");
    }
}

#[test]
fn a_core_not_downloaded_yet_gives_way_to_the_default() {
    let d = root_with(Some("Mario = nestopia\n"));
    std::fs::write(d.path().join("Config/system_core.txt"), "nes = quicknes\n").unwrap();
    assert_eq!(core_for_system(d.path(), Platform::Nes), Core::Fceumm);
    assert_eq!(core_for_platform(d.path(), "Mario", Platform::Nes), Core::Fceumm);

    std::fs::create_dir(d.path().join(CORES_DIR)).unwrap();
    std::fs::write(d.path().join(CORES_DIR).join(Core::Nestopia.file_name()), "").unwrap();
    assert!(Core::Nestopia.installed(d.path()));
    assert!(!Core::Quicknes.installed(d.path()));
    assert_eq!(core_for_platform(d.path(), "Mario", Platform::Nes), Core::Nestopia);
    assert_eq!(core_for_system(d.path(), Platform::Nes), Core::Fceumm);
}

#[test]
fn every_system_has_a_core_and_no_core_twice() {
    for p in Platform::ALL {
        let cores = p.spec().cores;
        assert!(!cores.is_empty(), "{p:?} has no core");
        for (i, c) in cores.iter().enumerate() {
            assert!(!cores[i + 1..].contains(c), "{p:?} lists {c:?} twice");
        }
    }
}

#[test]
fn a_game_with_no_line_runs_its_systems_core() {
    let d = root_with(None);
    assert_eq!(core_for_system(d.path(), Platform::Gba), Core::Mgba);
    write_system_core(d.path(), Platform::Gba, Core::Gpsp).unwrap();
    assert_eq!(core_for_system(d.path(), Platform::Gba), Core::Gpsp);
    assert_eq!(
        core_for_platform(d.path(), "Emerald", Platform::Gba),
        Core::Gpsp
    );
    assert_eq!(
        core_for_system(d.path(), Platform::Arcade),
        Core::Fbneo,
        "the GBA's core reached another system"
    );
}

#[test]
fn a_games_own_core_wins_over_its_systems() {
    let d = root_with(Some("Emerald = mgba\n"));
    write_system_core(d.path(), Platform::Gba, Core::Gpsp).unwrap();
    assert_eq!(
        core_for_platform(d.path(), "Emerald", Platform::Gba),
        Core::Mgba
    );
}

#[test]
fn a_system_core_the_system_cannot_run_is_ignored() {
    let d = root_with(None);
    std::fs::write(d.path().join("Config/system_core.txt"), "arcade = gpsp\n").unwrap();
    assert_eq!(core_for_system(d.path(), Platform::Arcade), Core::Fbneo);
}

#[test]
fn giving_a_game_its_systems_core_lets_it_follow_the_system() {
    let d = root_with(None);
    write_game_core(d.path(), "Emerald", Platform::Gba, Core::Gpsp).unwrap();
    assert_eq!(
        read_selected_cores(d.path()).get("Emerald"),
        Some(&Core::Gpsp)
    );

    write_game_core(d.path(), "Emerald", Platform::Gba, Core::Mgba).unwrap();
    assert!(read_selected_cores(d.path()).is_empty());
    write_system_core(d.path(), Platform::Gba, Core::Gpsp).unwrap();
    assert_eq!(
        core_for_platform(d.path(), "Emerald", Platform::Gba),
        Core::Gpsp
    );
}
