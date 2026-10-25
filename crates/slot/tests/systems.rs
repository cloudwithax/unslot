mod common;

use common::{app_booting_at, CLOCK_IS_SET};
use slot::app::{PageModel, Phase};
use slot::root::missing_bios;
use slot_input::{Action, Btn};
use slot_store::{core_for_platform, core_for_system, write_slot_state, Core, Platform, SlotState};

fn card_with(games: &[&str]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    slot::root::ensure(d.path());
    for g in games {
        std::fs::write(d.path().join("Games").join(g), b"PK").unwrap();
    }
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    d
}

fn press(a: &mut slot::app::App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

#[test]
fn start_on_an_arcade_game_picks_its_core_from_a_list() {
    let d = card_with(&["ARCADE/sf2ce.zip", "ARCADE/1941.zip"]);
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    let stem = a.selected_stem().expect("a game on the shelf").to_string();
    press(&mut a, Btn::Start);
    let Some(PageModel::List(page)) = a.page_model() else {
        panic!("no core list: {:?}", a.phase());
    };
    let rows: Vec<(&str, &str)> = page
        .rows
        .iter()
        .map(|r| (r.label.as_str(), r.detail.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("FinalBurn Neo", "In use"),
            ("MAME 2003-Plus", ""),
            ("MAME 2003", "Download"),
            ("MAME 2000", "Download"),
            ("MAME 2010", "Download"),
            ("FB Alpha 2012", "Download")
        ]
    );
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    assert_eq!(
        core_for_platform(d.path(), &stem, Platform::Arcade),
        Core::Mame2003Plus
    );
}

#[test]
fn y_on_the_core_list_sets_the_core_for_the_whole_system() {
    let d = card_with(&["ARCADE/sf2ce.zip", "ARCADE/1941.zip", "CPS1/ffight.zip"]);
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    let stem = a.selected_stem().expect("a game on the shelf").to_string();
    press(&mut a, Btn::Start);
    press(&mut a, Btn::Down);
    let Some(PageModel::List(page)) = a.page_model() else {
        panic!("no core list: {:?}", a.phase());
    };
    let legends: Vec<&str> = page.legends.iter().map(|l| l.label.as_str()).collect();
    assert!(legends.contains(&"All Arcade"), "{legends:?}");
    press(&mut a, Btn::Y);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());

    assert_eq!(
        core_for_system(d.path(), Platform::Arcade),
        Core::Mame2003Plus
    );
    for game in ["sf2ce", "1941"] {
        assert_eq!(
            core_for_platform(d.path(), game, Platform::Arcade),
            Core::Mame2003Plus
        );
    }
    assert_eq!(
        core_for_system(d.path(), Platform::Cps1),
        Core::Fbneo,
        "the arcade's core reached CPS-1"
    );

    // One game back on FinalBurn Neo keeps it, and the list says which is the system's.
    press(&mut a, Btn::Start);
    let Some(PageModel::List(page)) = a.page_model() else {
        panic!("no core list: {:?}", a.phase());
    };
    let rows: Vec<(&str, &str)> = page
        .rows
        .iter()
        .map(|r| (r.label.as_str(), r.detail.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("FinalBurn Neo", ""),
            ("MAME 2003-Plus", "In use"),
            ("MAME 2003", "Download"),
            ("MAME 2000", "Download"),
            ("MAME 2010", "Download"),
            ("FB Alpha 2012", "Download")
        ]
    );
    press(&mut a, Btn::Up);
    press(&mut a, Btn::A);
    assert_eq!(
        core_for_platform(d.path(), &stem, Platform::Arcade),
        Core::Fbneo
    );
    press(&mut a, Btn::Start);
    let Some(PageModel::List(page)) = a.page_model() else {
        panic!("no core list: {:?}", a.phase());
    };
    let rows: Vec<(&str, &str)> = page
        .rows
        .iter()
        .map(|r| (r.label.as_str(), r.detail.as_str()))
        .collect();
assert_eq!(
        rows,
        [
            ("FinalBurn Neo", "In use"),
            ("MAME 2003-Plus", "Default"),
            ("MAME 2003", "Download"),
            ("MAME 2000", "Download"),
            ("MAME 2010", "Download"),
            ("FB Alpha 2012", "Download")
        ]
    );
}

#[test]
fn a_bios_is_found_under_any_of_its_names_or_beside_the_games() {
    let d = card_with(&[]);
    let bios = d.path().join("BIOS");
    assert_eq!(
        missing_bios(d.path(), Platform::SegaCd),
        Some("bios_CD_U.bin|bios_CD_E.bin|bios_CD_J.bin")
    );
    std::fs::write(bios.join("bios_CD_J.bin"), b"").unwrap();
    assert_eq!(missing_bios(d.path(), Platform::SegaCd), None);

    assert!(missing_bios(d.path(), Platform::NeoGeo).is_some());
    std::fs::write(d.path().join("Games/NEOGEO/neogeo.zip"), b"PK").unwrap();
    assert_eq!(missing_bios(d.path(), Platform::NeoGeo), None);

    // Both of the Intellivision's are needed.
    std::fs::write(bios.join("exec.bin"), b"").unwrap();
    assert_eq!(
        missing_bios(d.path(), Platform::Intellivision),
        Some("grom.bin")
    );
}

#[test]
fn every_system_has_its_own_folder_on_the_card() {
    let d = card_with(&[]);
    for p in Platform::ALL {
        assert!(
            d.path().join("Games").join(p.dir_name()).is_dir(),
            "no Games/{}",
            p.dir_name()
        );
    }
    let dirs: Vec<&str> = Platform::ALL.iter().map(|p| p.dir_name()).collect();
    for (i, d) in dirs.iter().enumerate() {
        assert!(!dirs[i + 1..].contains(d), "two systems share {d}");
    }
}

/// Types `word` on the keyboard by walking to each key, which starts on `q`.
fn type_word(a: &mut slot::app::App, word: &str) {
    const ROWS: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl-", "zxcvbnm_.@"];
    let (mut row, mut col) = (1usize, 0usize);
    for ch in word.chars() {
        let r = ROWS
            .iter()
            .position(|r| r.contains(ch))
            .expect("a key for it");
        let c = ROWS[r].find(ch).unwrap();
        while row < r {
            press(a, Btn::Down);
            row += 1;
        }
        while row > r {
            press(a, Btn::Up);
            row -= 1;
        }
        while col < c {
            press(a, Btn::Right);
            col += 1;
        }
        while col > c {
            press(a, Btn::Left);
            col -= 1;
        }
        press(a, Btn::A);
    }
}

#[test]
fn x_finds_a_game_on_any_shelf_and_a_takes_the_slot_to_it() {
    let d = card_with(&[
        "ARCADE/mslug.zip",
        "ARCADE/1941.zip",
        "SNES/Super Metroid.sfc",
        "NES/Metroid.nes",
        "NES/Zelda.nes",
    ]);
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    press(&mut a, Btn::X);
    assert!(
        matches!(a.page_model(), Some(PageModel::Keys(_))),
        "X opened no keyboard: {:?}",
        a.phase()
    );
    type_word(&mut a, "met");
    press(&mut a, Btn::Start);
    let Some(PageModel::List(page)) = a.page_model() else {
        panic!("no results: {:?}", a.phase());
    };
    let found: Vec<(&str, &str)> = page
        .rows
        .iter()
        .map(|r| (r.label.as_str(), r.detail.as_str()))
        .collect();
    // The arcade set is found by its game's name; a name starting with the query comes first.
    assert_eq!(
        found,
        [
            ("Metroid", "NES"),
            ("Metal Slug Super Vehicle-001", "Arcade"),
            ("Super Metroid", "Super Nintendo"),
        ]
    );
    assert_eq!(page.status, "3 games match \u{201c}met\u{201d}");
    assert!(
        page.rows[0].icon.as_ref().is_some_and(|i| i.h > 0),
        "the first result has no picture"
    );
    press(&mut a, Btn::Down);
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    assert_eq!(a.selected_stem(), Some("Super Metroid"));

    // Y from the results searches again from what was typed; MENU goes back to them.
    press(&mut a, Btn::X);
    let Some(PageModel::Keys(keys)) = a.page_model() else {
        panic!("no keyboard");
    };
    assert_eq!(keys.text(), "met", "the last search was not kept");
}

#[test]
fn a_search_that_finds_nothing_says_so() {
    let d = card_with(&["NES/Zelda.nes", "NES/Metroid.nes"]);
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    press(&mut a, Btn::X);
    type_word(&mut a, "qq");
    press(&mut a, Btn::Start);
    let Some(PageModel::List(page)) = a.page_model() else {
        panic!("no results page");
    };
    assert!(page.rows.is_empty());
    assert_eq!(page.status, "No games match \u{201c}qq\u{201d}");
    press(&mut a, Btn::B);
    assert!(matches!(a.phase(), Phase::Shelf));
}

#[test]
fn a_on_a_core_not_downloaded_yet_downloads_it_into_cores() {
    let d = card_with(&["ARCADE/sf2ce.zip", "ARCADE/1941.zip"]);
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    let stem = a.selected_stem().expect("a game on the shelf").to_string();
    press(&mut a, Btn::Start);
    press(&mut a, Btn::Down);
    press(&mut a, Btn::Down);
    let Some(PageModel::List(page)) = a.page_model() else {
        panic!("no core list: {:?}", a.phase());
    };
    let legends: Vec<&str> = page.legends.iter().map(|l| l.label.as_str()).collect();
    assert!(legends.contains(&"Download"), "{legends:?}");
    press(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Cores { cursor: 2 }),
        "a core not on the card was chosen: {:?}",
        a.phase()
    );
    assert_eq!(
        core_for_platform(d.path(), &stem, Platform::Arcade),
        Core::Fbneo,
        "a core still downloading was set for the game"
    );

    let file = d.path().join(slot_store::CORES_DIR).join(Core::Mame2003.file_name());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !file.is_file() {
        assert!(std::time::Instant::now() < deadline, "the core never came");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Once it is on the card the list offers it like any other.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        a.update(0.016);
        let Some(PageModel::List(page)) = a.page_model() else {
            panic!("the core list closed: {:?}", a.phase());
        };
        if page.rows[2].detail.is_empty() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{:?}", page.rows[2]);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    assert_eq!(
        core_for_platform(d.path(), &stem, Platform::Arcade),
        Core::Mame2003
    );
}

#[test]
fn the_gba_board_gives_way_to_a_list_once_another_gba_core_is_downloaded() {
    let d = card_with(&["GBA/emerald.gba", "GBA/zzz.gba"]);
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    a.apply(Action::GbaDown(Btn::Start));
    assert!(a.core_picker().is_some(), "{:?}", a.phase());

    let d = card_with(&["GBA/emerald.gba", "GBA/zzz.gba"]);
    let cores = d.path().join(slot_store::CORES_DIR);
    std::fs::create_dir_all(&cores).unwrap();
    std::fs::write(cores.join(Core::VbaNext.file_name()), b"pretend").unwrap();
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    press(&mut a, Btn::Start);
    assert_eq!(a.core_picker(), None);
    let Some(PageModel::List(page)) = a.page_model() else {
        panic!("no core list: {:?}", a.phase());
    };
    let labels: Vec<&str> = page.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, ["mGBA", "gpSP", "VBA Next", "VBA-M"]);
}
