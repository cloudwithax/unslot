use std::collections::HashMap;
use std::time::Duration;

use slot::app::{App, PageModel, Phase};
use slot::scrape::{parse_esrb_dat, ss_esrb, EsrbScraper, Fetch};
use slot_input::{Action, Btn};
use slot_store::{
    write_manual_esrb, write_scraped_esrb, write_slot_state, Esrb, EsrbCap, Platform, SlotState,
};
use slot_ui::{KeyLayout, QuickRow};

fn press(app: &mut App, btn: Btn) {
    app.apply(Action::GbaDown(btn));
    app.apply(Action::GbaUp(btn));
}

fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        let Phase::Keyboard { keys, .. } = app.phase() else {
            panic!("not on keyboard: {:?}", app.phase());
        };
        let mut kb = keys.clone();
        let mut typed = false;
        let rows = kb.rows().len();
        'outer: for r in 0..rows {
            for c in 0..kb.rows()[r].len() {
                if kb.label(kb.rows()[r][c]) == ch.to_string() {
                    let mut probe = keys.clone();
                    while probe.row() != r {
                        press(app, if probe.row() < r { Btn::Down } else { Btn::Up });
                        let Phase::Keyboard { keys: k, .. } = app.phase() else {
                            unreachable!()
                        };
                        probe = k.clone();
                    }
                    while probe.col() != c {
                        press(
                            app,
                            if probe.col() < c {
                                Btn::Right
                            } else {
                                Btn::Left
                            },
                        );
                        let Phase::Keyboard { keys: k, .. } = app.phase() else {
                            unreachable!()
                        };
                        probe = k.clone();
                    }
                    press(app, Btn::A);
                    typed = true;
                    break 'outer;
                }
            }
        }
        assert!(typed, "character {ch:?} not directly reachable");
        let _ = &mut kb;
    }
    press(app, Btn::Start);
}

#[test]
fn kiosk_pin_and_password_persist_across_reboots_and_drop_menu_straight_to_prompt() {
    let d = tempfile::tempdir().unwrap();
    slot::root::ensure(d.path());
    std::fs::write(d.path().join("Games/GBA/Mario.gba"), b"GBA").unwrap();
    std::fs::write(d.path().join("Games/SNES/Zelda.sfc"), b"SFC").unwrap();
    std::fs::write(d.path().join("Games/MD/Sonic.bin"), b"MD").unwrap();
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    write_scraped_esrb(d.path(), Platform::Gba, "Mario", Esrb::Everyone).unwrap();
    write_scraped_esrb(d.path(), Platform::Snes, "Zelda", Esrb::Teen).unwrap();
    write_scraped_esrb(d.path(), Platform::Genesis, "Sonic", Esrb::Everyone).unwrap();

    let mut app = App::boot(d.path());
    assert_eq!(
        app.carts().filter(|c| c.platform != Platform::Apps).count(),
        3
    );

    // General -> Kiosk Mode
    app.apply(Action::QuickMenu);
    press(&mut app, Btn::A);
    while app.quick_menu() != Some(QuickRow::Kiosk) {
        press(&mut app, Btn::Down);
    }
    press(&mut app, Btn::A);
    let Some(PageModel::List(page)) = app.page_model() else {
        panic!("expected Kiosk main page, got {:?}", app.phase());
    };
    assert_eq!(page.title, "Kiosk Mode");

    // Configure Allowed Systems -> GBA only (toggle restrict via Right, open via A or toggle GBA).
    while app.page_model().is_some_and(|p| match p {
        PageModel::List(l) => l.rows[l.cursor].label != "Allowed Systems",
        _ => false,
    }) {
        press(&mut app, Btn::Down);
    }
    press(&mut app, Btn::A);
    while app.page_model().is_some_and(|p| match p {
        PageModel::List(l) => l.rows[l.cursor].label != Platform::Gba.name(),
        _ => false,
    }) {
        press(&mut app, Btn::Down);
    }
    press(&mut app, Btn::A);
    press(&mut app, Btn::B);

    // Enter Kiosk Mode with no PIN set yet -> prompts numeric PIN pad.
    while app.page_model().is_some_and(|p| match p {
        PageModel::List(l) => l.cursor != 0,
        _ => false,
    }) {
        press(&mut app, Btn::Up);
    }
    press(&mut app, Btn::A);
    let Phase::Keyboard { keys, .. } = app.phase() else {
        panic!("expected PIN setup keyboard");
    };
    assert_eq!(keys.layout(), KeyLayout::Numeric);
    assert!(keys.is_masked());
    type_text(&mut app, "1234");
    assert!(matches!(app.phase(), Phase::Shelf));

    // Only Mario (GBA) is visible now.
    let visible: Vec<(Platform, &str)> =
        app.carts().map(|c| (c.platform, c.stem.as_str())).collect();
    assert_eq!(visible, [(Platform::Gba, "Mario")]);

    // Survives reboot cleanly.
    let mut rebooted = App::boot(d.path());
    let visible_after: Vec<(Platform, &str)> = rebooted
        .carts()
        .map(|c| (c.platform, c.stem.as_str()))
        .collect();
    assert_eq!(visible_after, [(Platform::Gba, "Mario")]);

    // Menu drops straight into masked numeric PIN prompt; refuses wrong PIN, accepts 1234.
    rebooted.apply(Action::QuickMenu);
    let Phase::Keyboard { keys, .. } = rebooted.phase() else {
        panic!("expected direct PIN prompt, got {:?}", rebooted.phase());
    };
    assert_eq!(keys.layout(), KeyLayout::Numeric);
    assert!(keys.is_masked());
    type_text(&mut rebooted, "9999");
    let Phase::Keyboard { keys, .. } = rebooted.phase() else {
        panic!("wrong PIN left prompt");
    };
    assert!(keys.alert());
    press(&mut rebooted, Btn::Y);
    assert!(
        matches!(rebooted.phase(), Phase::Shelf),
        "{:?}",
        rebooted.phase()
    );

    rebooted.apply(Action::QuickMenu);
    type_text(&mut rebooted, "1234");
    assert_eq!(rebooted.quick_menu(), Some(QuickRow::Kiosk));
    assert_eq!(
        rebooted
            .carts()
            .filter(|c| c.platform != Platform::Apps)
            .count(),
        3
    );
}

#[test]
fn kiosk_esrb_cap_locks_unrated_by_default_and_manual_update_unlocks() {
    let d = tempfile::tempdir().unwrap();
    slot::root::ensure(d.path());
    std::fs::write(d.path().join("Games/GBA/Mario.gba"), b"GBA").unwrap();
    std::fs::write(d.path().join("Games/GBA/Mystery.gba"), b"GBA").unwrap();
    std::fs::write(d.path().join("Games/GBA/Dark.gba"), b"GBA").unwrap();
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    write_scraped_esrb(d.path(), Platform::Gba, "Mario", Esrb::Everyone).unwrap();
    write_scraped_esrb(d.path(), Platform::Gba, "Dark", Esrb::Teen).unwrap();
    // "Mystery" has no ESRB rating (unrated).

    let mut app = App::boot(d.path());
    app.apply(Action::QuickMenu);
    press(&mut app, Btn::A);
    while app.quick_menu() != Some(QuickRow::Kiosk) {
        press(&mut app, Btn::Down);
    }
    press(&mut app, Btn::A);

    // Switch Lock Type to Password, set password "kid", set ESRB Limit to Under T.
    press(&mut app, Btn::Down);
    press(&mut app, Btn::Right);
    press(&mut app, Btn::Down);
    press(&mut app, Btn::A);
    let Phase::Keyboard { keys, .. } = app.phase() else {
        panic!("expected password prompt");
    };
    assert_eq!(keys.layout(), KeyLayout::General);
    type_text(&mut app, "kid");

    while app.page_model().is_some_and(|p| match p {
        PageModel::List(l) => l.rows[l.cursor].label != "ESRB Limit",
        _ => false,
    }) {
        press(&mut app, Btn::Down);
    }
    while app.page_model().is_some_and(|p| match p {
        PageModel::List(l) => l.rows[l.cursor].detail != EsrbCap::UnderTeen.label(),
        _ => false,
    }) {
        press(&mut app, Btn::Right);
    }

    // Enter Kiosk Mode -> only Mario (E) is allowed; Dark (T) and Mystery (Unrated) are locked out.
    while app.page_model().is_some_and(|p| match p {
        PageModel::List(l) => l.cursor != 0,
        _ => false,
    }) {
        press(&mut app, Btn::Up);
    }
    press(&mut app, Btn::A);
    let stems: Vec<&str> = app
        .carts()
        .filter(|c| c.platform != Platform::Apps)
        .map(|c| c.stem.as_str())
        .collect();
    assert_eq!(stems, ["Mario"]);

    // Unlock using password prompt, open Game Rules & ESRB -> GBA, manually set Mystery to E10+!
    app.apply(Action::QuickMenu);
    let Phase::Keyboard { keys, .. } = app.phase() else {
        panic!("expected password prompt on unlock");
    };
    assert_eq!(keys.layout(), KeyLayout::General);
    type_text(&mut app, "kid");
    press(&mut app, Btn::A);
    while app.page_model().is_some_and(|p| match p {
        PageModel::List(l) => l.rows[l.cursor].label != "Game Rules & ESRB",
        _ => false,
    }) {
        press(&mut app, Btn::Down);
    }
    press(&mut app, Btn::A);
    press(&mut app, Btn::A);
    while app.page_model().is_some_and(|p| match p {
        PageModel::List(l) => !l.rows[l.cursor].label.contains("Mystery"),
        _ => false,
    }) {
        press(&mut app, Btn::Down);
    }
    // Right twice: Unrated -> E -> E10+!
    press(&mut app, Btn::Right);
    press(&mut app, Btn::Right);
    let Some(PageModel::List(page)) = app.page_model() else {
        panic!("expected game rules page");
    };
    assert!(
        page.rows[page.cursor].detail.contains("E10+*"),
        "{:?}",
        page.rows[page.cursor]
    );
    press(&mut app, Btn::B);
    press(&mut app, Btn::B);
    while app.page_model().is_some_and(|p| match p {
        PageModel::List(l) => l.cursor != 0,
        _ => false,
    }) {
        press(&mut app, Btn::Up);
    }
    press(&mut app, Btn::A);
    let mut unlocked: Vec<&str> = app
        .carts()
        .filter(|c| c.platform != Platform::Apps)
        .map(|c| c.stem.as_str())
        .collect();
    unlocked.sort();
    assert_eq!(unlocked, ["Mario", "Mystery"]);

    // Manual rating overrides even scraped T -> reset back to Mature hides again after manual edit.
    write_manual_esrb(d.path(), Platform::Gba, "Mystery", Some(Esrb::Mature)).unwrap();
    let rebooted = App::boot(d.path());
    let stems: Vec<&str> = rebooted
        .carts()
        .filter(|c| c.platform != Platform::Apps)
        .map(|c| c.stem.as_str())
        .collect();
    assert_eq!(stems, ["Mario"]);
}

struct MockEsrbFetch;
impl Fetch for MockEsrbFetch {
    fn get(&mut self, url: &str) -> Result<Option<Vec<u8>>, String> {
        if url.contains("metadat/esrb/") {
            return Ok(Some(
                b"game (\n\tname \"Alpha (USA)\"\n\tesrb_rating \"E10+ - Everyone 10+\"\n)\n"
                    .to_vec(),
            ));
        }
        Ok(None)
    }
}

#[test]
fn parses_esrb_dat_screenscraper_classifications_and_runs_esrb_scraper() {
    let dat = r#"
game (
	esrb_rating "T - Teen"
	name "Metroid Fusion (USA)"
	rom ( name "Metroid Fusion (USA).gba" size 8388608 crc 12345678 )
)
game (
	name "Mario Kart"
	esrb "E"
)
"#;
    assert_eq!(
        parse_esrb_dat(dat),
        vec![
            ("Metroid Fusion (USA)".into(), Esrb::Teen),
            ("Mario Kart".into(), Esrb::Everyone),
        ]
    );
    let ss_json = r#"{"response":{"jeu":{"classifications":[{"type":"PEGI","texte":"12"},{"type":"ESRB","texte":"E10+"}]}}}"#;
    assert_eq!(ss_esrb(ss_json), Some(Esrb::Everyone10Plus));

    let d = tempfile::tempdir().unwrap();
    slot::root::ensure(d.path());
    let rom = d.path().join("Games/GBA/Alpha.gba");
    std::fs::write(&rom, b"GBA").unwrap();
    let carts = slot_store::scan(d.path()).unwrap();
    let scraper = EsrbScraper::spawn(Box::new(MockEsrbFetch), d.path());
    scraper.fetch(carts, HashMap::new());
    for _ in 0..50 {
        if !scraper.view().running && scraper.view().done > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let view = scraper.view();
    assert!(!view.running);
    assert_eq!(view.rated, 1);
    let ratings = slot_store::read_esrb_map(d.path());
    assert_eq!(
        ratings.get(&(Platform::Gba, "Alpha".into())),
        Some(&Esrb::Everyone10Plus)
    );
}
