mod common;

use common::{app_playing_in, boot, tmp_root_with_carts};
use slot::app::{App, PageModel};
use slot::input::remap::{self, Remap, Scope};
use slot_input::{Action, Btn};
use slot_retro::ButtonMask;
use slot_store::Platform;
use slot_ui::{Draw, ListPage, QuickRow, TexId};

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn page(a: &App) -> ListPage {
    match a.page_model() {
        Some(PageModel::List(page)) => page,
        _ => panic!("no list page is open"),
    }
}

fn detail(a: &App, label: &str) -> String {
    let page = page(a);
    let row = page.rows.iter().find(|r| r.label == label).expect(label);
    row.detail.clone()
}

/// Moves the Controls page's cursor onto the row called `label`.
fn down_to(a: &mut App, label: &str) {
    for _ in 0..20 {
        let page = page(a);
        if page.rows[page.cursor].label == label {
            return;
        }
        press(a, Btn::Down);
    }
    panic!("never reached {label}");
}

#[test]
fn a_map_of_nothing_hands_the_game_the_buttons_as_held() {
    let held = ButtonMask(ButtonMask::A | ButtonMask::UP);
    assert_eq!(Remap::default().apply(held), held);
}

#[test]
fn a_swapped_pair_presses_each_others_button() {
    let remap = Remap::parse("A=B\nB=A\n");
    assert_eq!(
        remap.apply(ButtonMask(ButtonMask::B)),
        ButtonMask(ButtonMask::A)
    );
    assert_eq!(
        remap.apply(ButtonMask(ButtonMask::A)),
        ButtonMask(ButtonMask::B)
    );
    assert_eq!(
        remap.apply(ButtonMask(ButtonMask::START)),
        ButtonMask(ButtonMask::START)
    );
}

#[test]
fn a_button_mapped_to_none_never_reaches_the_game_and_one_button_can_press_two() {
    let remap = Remap::parse("Select=none\nR=L\n");
    assert_eq!(remap.apply(ButtonMask(ButtonMask::SELECT)), ButtonMask(0));
    assert_eq!(
        remap.apply(ButtonMask(ButtonMask::L)),
        ButtonMask(ButtonMask::L | ButtonMask::R)
    );
}

#[test]
fn a_map_reads_back_as_it_was_written_and_skips_lines_it_cannot_read() {
    let remap = Remap::parse("a=y\nX=None\nbogus\nStart=Nothing\n");
    assert_eq!(Remap::parse(&remap.text()), remap);
    assert_eq!(remap.0[ButtonMask::START.trailing_zeros() as usize], 3);
}

#[test]
fn a_game_takes_its_own_map_else_its_systems_else_every_games() {
    let d = tempfile::tempdir().unwrap();
    let game = Scope::Game(Platform::Gba, "Emerald".into());
    assert_eq!(remap::resolve(d.path(), &game), Remap::default());
    let all = Remap::parse("A=B\n");
    remap::write(d.path(), &Scope::All, &all);
    assert_eq!(remap::resolve(d.path(), &game), all);
    let gba = Remap::parse("A=X\n");
    remap::write(d.path(), &Scope::System(Platform::Gba), &gba);
    assert_eq!(remap::resolve(d.path(), &game), gba);
    assert_eq!(
        remap::resolve(d.path(), &Scope::System(Platform::Snes)),
        all,
        "another system took the GBA's"
    );
    let own = Remap::parse("A=Y\n");
    remap::write(d.path(), &game, &own);
    assert_eq!(remap::resolve(d.path(), &game), own);
    remap::clear(d.path(), &game);
    assert_eq!(remap::resolve(d.path(), &game), gba);
    assert!(d.path().join("Config/Remaps/GBA.txt").is_file());
}

#[test]
fn controls_over_a_game_open_on_its_system_and_a_change_reaches_the_game_at_once() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::QuickMenu);
    for step in [
        QuickRow::Settings,
        QuickRow::PlayGameplay,
        QuickRow::Controls,
    ] {
        for _ in 0..12 {
            if a.in_game_menu() == Some(step) {
                break;
            }
            press(&mut a, Btn::Down);
        }
        assert_eq!(a.in_game_menu(), Some(step), "never reached {step:?}");
        press(&mut a, Btn::A);
    }
    assert_eq!(page(&a).title, "Controls");
    assert_eq!(detail(&a, "Applies to"), Platform::Gba.name());
    assert!(a.game_page_open(), "the game would still read the buttons");
    assert_eq!(
        page(&a).status,
        "The same as a wider choice, until you change one"
    );

    down_to(&mut a, "A");
    press(&mut a, Btn::Right);
    assert_eq!(detail(&a, "A"), "B");
    assert_eq!(
        page(&a).status,
        format!("Buttons for {}", Platform::Gba.name())
    );
    assert_ne!(
        a.remap().apply(ButtonMask(ButtonMask::B)).0 & ButtonMask::A,
        0,
        "the game did not take the new map"
    );
    let saved = std::fs::read_to_string(d.path().join("Config/Remaps/GBA.txt")).unwrap();
    assert!(saved.contains("A=B\n"), "{saved}");

    press(&mut a, Btn::B);
    assert!(!a.game_page_open());
    assert_eq!(a.in_game_menu(), Some(QuickRow::Controls));
}

#[test]
fn a_game_of_its_own_wins_over_its_system_and_reset_hands_it_back() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    remap::write(
        d.path(),
        &Scope::System(Platform::Gba),
        &Remap::parse("A=X\n"),
    );
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::QuickMenu);
    for step in [
        QuickRow::Settings,
        QuickRow::PlayGameplay,
        QuickRow::Controls,
    ] {
        for _ in 0..12 {
            if a.in_game_menu() == Some(step) {
                break;
            }
            press(&mut a, Btn::Down);
        }
        press(&mut a, Btn::A);
    }
    assert_eq!(detail(&a, "Applies to"), Platform::Gba.name());
    press(&mut a, Btn::Right);
    assert_eq!(detail(&a, "Applies to"), "This Game");
    assert_eq!(
        detail(&a, "A"),
        "X",
        "the game did not start from its system's"
    );
    down_to(&mut a, "A");
    press(&mut a, Btn::Right);
    assert_eq!(detail(&a, "A"), "Y");
    assert!(d.path().join("Config/Remaps/GBA/Emerald.txt").is_file());
    assert_ne!(
        a.remap().apply(ButtonMask(ButtonMask::Y)).0 & ButtonMask::A,
        0
    );

    down_to(&mut a, "Reset");
    press(&mut a, Btn::A);
    assert!(!d.path().join("Config/Remaps/GBA/Emerald.txt").exists());
    assert_eq!(detail(&a, "A"), "X");
    assert_ne!(
        a.remap().apply(ButtonMask(ButtonMask::X)).0 & ButtonMask::A,
        0
    );
}

#[test]
fn controls_from_the_menu_set_every_games_buttons() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = boot(d.path());
    a.apply(Action::QuickMenu);
    for step in [QuickRow::Game, QuickRow::Controls] {
        for _ in 0..12 {
            if a.quick_menu() == Some(step) {
                break;
            }
            press(&mut a, Btn::Down);
        }
        assert_eq!(a.quick_menu(), Some(step), "never reached {step:?}");
        press(&mut a, Btn::A);
    }
    assert_eq!(detail(&a, "Applies to"), "All Games");
    assert_eq!(page(&a).status, "Buttons for all games");
    a.set_page_face(TexId::from_raw(91), 100, 100);
    let mut out = Vec::new();
    a.draw(&mut out);
    assert!(
        out.iter()
            .any(|d| matches!(d, Draw::Tex { tex, .. } if *tex == TexId::from_raw(91))),
        "the page was not drawn over the menu"
    );
    down_to(&mut a, "Select");
    press(&mut a, Btn::Right);
    assert_eq!(detail(&a, "Select"), "None");
    let saved = std::fs::read_to_string(d.path().join("Config/Remaps/All.txt")).unwrap();
    assert!(saved.contains("Select=None\n"), "{saved}");
    press(&mut a, Btn::B);
    assert_eq!(a.page_model().map(|_| ()), None);
    assert_eq!(a.quick_menu(), Some(QuickRow::Controls));
}
