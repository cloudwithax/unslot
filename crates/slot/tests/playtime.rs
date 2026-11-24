mod common;

use std::time::{Duration, Instant};

use common::tmp_root_with_carts;
use slot::app::{App, PageModel, Phase, RetroshelfPage, EJECT_S, INSERT_S};
use slot::playtime::{now_unix, PlaytimeStore, Sort};
use slot::retroshelf::{self, SimRetroshelf};
use slot_input::{Action, Btn};
use slot_store::{write_slot_state, Platform, SlotState};
use slot_ui::{AppPage, QuickRow};

fn press(a: &mut App, b: Btn) {
    a.apply(Action::GbaDown(b));
}

fn open_at(a: &mut App, row: QuickRow) {
    a.apply(Action::QuickMenu);
    let mut path = vec![row];
    while let Some(parent) = path.last().and_then(|r| r.parent()) {
        path.push(parent);
    }
    for (depth, step) in path.iter().rev().enumerate() {
        if depth > 0 {
            press(a, Btn::A);
        }
        for _ in 0..step.position(&a.hidden_rows()) {
            press(a, Btn::Down);
        }
    }
    assert_eq!(a.quick_menu(), Some(row), "the bar never reached {row:?}");
}

fn seat_and_play(a: &mut App) {
    a.apply(Action::Insert);
    a.on_core_ready();
    a.update(INSERT_S + 0.1);
    assert!(
        matches!(a.phase(), Phase::Playing { .. }),
        "expected Playing, got {:?}",
        a.phase()
    );
}

#[test]
fn active_play_tracks_playtime_pauses_in_menu_and_doze_and_persists() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            show_titles: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    let mut a = App::boot(d.path());
    assert_eq!(a.title_text().as_deref(), Some("Emerald"));

    // Start playing Emerald and tick 70 seconds -> 60s checkpoint hits disk.
    seat_and_play(&mut a);
    for _ in 0..70 {
        a.update(1.0);
    }
    assert_eq!(
        PlaytimeStore::read(d.path()).game_secs(Platform::Gba, "Emerald"),
        60,
        "expected 60s checkpoint written to Config/playtime.json"
    );

    // Open in-game menu: ticking must not advance playtime while paused.
    a.apply(Action::QuickMenu);
    for _ in 0..30 {
        a.update(1.0);
    }
    press(&mut a, Btn::B);

    // Play another 20s (session total = 90s), then doze -> finishes & persists session.
    for _ in 0..20 {
        a.update(1.0);
    }
    a.apply(Action::PowerTap);
    assert!(matches!(a.phase(), Phase::Doze { .. }));
    let saved = PlaytimeStore::read(d.path());
    assert_eq!(saved.game_secs(Platform::Gba, "Emerald"), 90);
    assert_eq!(saved.total_sessions(), 1);
    assert_eq!(saved.pending_count(), 1);

    // Time spent asleep must not count.
    for _ in 0..60 {
        a.update(1.0);
    }
    assert_eq!(
        PlaytimeStore::read(d.path()).game_secs(Platform::Gba, "Emerald"),
        90
    );

    // Wake back into the game, play 35s, then eject -> 125s total (2m), 2 sessions.
    a.apply(Action::PowerTap);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    for _ in 0..35 {
        a.update(1.0);
    }
    a.apply(Action::Eject);
    for _ in 0..40 {
        a.update((EJECT_S + 1.0) / 40.0);
    }
    assert!(matches!(a.phase(), Phase::Shelf));

    let saved = PlaytimeStore::read(d.path());
    assert_eq!(saved.game_secs(Platform::Gba, "Emerald"), 125);
    assert_eq!(saved.total_sessions(), 2);
    assert_eq!(saved.pending_count(), 2);

    // Shelf shows duration next to played game and keeps unplayed game clean.
    assert_eq!(a.title_text().as_deref(), Some("Emerald · 2m"));
    press(&mut a, Btn::Right);
    assert_eq!(a.title_text().as_deref(), Some("Fusion"));

    // Reloading from disk preserves playtime and shelf display.
    let mut b = App::boot(d.path());
    assert_eq!(b.playtime().game_secs(Platform::Gba, "Emerald"), 125);
    // Return to Emerald on shelf.
    if b.selected_stem() != Some("Emerald") {
        press(&mut b, Btn::Left);
    }
    assert_eq!(b.title_text().as_deref(), Some("Emerald · 2m"));
}

#[test]
fn playtime_screen_ranks_sorts_renders_and_jumps_to_game_on_shelf() {
    let d = tmp_root_with_carts(&["Zelda"]);
    let snes = d.path().join("Games/SNES/Chrono.sfc");
    std::fs::create_dir_all(snes.parent().unwrap()).unwrap();
    std::fs::write(&snes, vec![0u8; 0x200]).unwrap();
    let gb = d.path().join("Games/GB/Tetris.gb");
    std::fs::create_dir_all(gb.parent().unwrap()).unwrap();
    std::fs::write(&gb, vec![0u8; 0x200]).unwrap();
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();

    let now = now_unix();
    let mut store = PlaytimeStore::default();
    store.finish_session(
        Platform::Gba,
        "Zelda",
        &d.path().join("Games/GBA/Zelda.gba"),
        d.path(),
        300,
        300,
        now - 3600,
        now - 3300,
    );
    store.finish_session(
        Platform::Snes,
        "Chrono",
        &snes,
        d.path(),
        900,
        900,
        now - 90_000,
        now - 89_100,
    );
    store.finish_session(
        Platform::Gb,
        "Tetris",
        &gb,
        d.path(),
        60,
        60,
        now - 120,
        now - 60,
    );
    store.write(d.path()).unwrap();

    let mut a = App::boot(d.path());
    open_at(&mut a, QuickRow::Playtime);
    press(&mut a, Btn::A);
    assert!(matches!(
        a.phase(),
        Phase::Playtime {
            cursor: 0,
            sort: Sort::MostPlayed
        }
    ));

    let page = match a.page_model() {
        Some(PageModel::App(AppPage::Playtime(p))) => p,
        other => panic!("expected PlaytimePage, got {other:?}"),
    };
    assert_eq!(page.sort, "Most played");
    assert_eq!(page.summary, "Total 21m · 3 games · 3 plays");
    assert!(
        page.sync_badge.is_empty(),
        "unlinked user should see no RetroShelf badge"
    );
    let names: Vec<&str> = page.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Chrono", "Zelda", "Tetris"]);
    assert_eq!(page.rows[0].system, "SNES");
    assert_eq!(page.rows[0].duration, "15m");

    // Rendering the PlaytimePage produces a full 680x440 page face with visible ink.
    let face = AppPage::Playtime(page).face();
    assert_eq!((face.w, face.h), (680, 440));
    assert!(face.rgba.chunks(4).any(|p| p[1] > 180));

    // Y cycles to Recent -> [Tetris, Zelda, Chrono].
    press(&mut a, Btn::Y);
    let page = match a.page_model() {
        Some(PageModel::App(AppPage::Playtime(p))) => p,
        other => panic!("expected PlaytimePage, got {other:?}"),
    };
    assert_eq!(page.sort, "Recent");
    let names: Vec<&str> = page.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Tetris", "Zelda", "Chrono"]);

    // Y cycles to A-Z -> [Chrono, Tetris, Zelda].
    press(&mut a, Btn::Y);
    let page = match a.page_model() {
        Some(PageModel::App(AppPage::Playtime(p))) => p,
        other => panic!("expected PlaytimePage, got {other:?}"),
    };
    assert_eq!(page.sort, "A-Z");
    let names: Vec<&str> = page.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Chrono", "Tetris", "Zelda"]);

    // Move to Tetris (row 1) and press A -> jumps to GB shelf with Tetris selected.
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Shelf));
    assert_eq!(a.selected_stem(), Some("Tetris"));
}

#[test]
fn retroshelf_links_via_qr_syncs_pending_sessions_and_unlinks() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    let now = now_unix();
    let mut store = PlaytimeStore::default();
    store.finish_session(
        Platform::Gba,
        "Emerald",
        &d.path().join("Games/GBA/Emerald.gba"),
        d.path(),
        420,
        420,
        now - 500,
        now - 80,
    );
    store.write(d.path()).unwrap();

    let mut a = App::boot(d.path());
    assert_eq!(a.playtime().pending_count(), 1);
    open_at(&mut a, QuickRow::Retroshelf);
    press(&mut a, Btn::A);
    assert!(matches!(
        a.phase(),
        Phase::Retroshelf {
            page: RetroshelfPage::SignIn | RetroshelfPage::Main { .. },
            ..
        }
    ));

    // Row 0 is Account ("Not linked"). Press A to start QR pairing with SimRetroshelf.
    press(&mut a, Btn::A);
    assert!(matches!(
        a.phase(),
        Phase::Retroshelf {
            page: RetroshelfPage::SignIn,
            ..
        }
    ));

    // Wait for SimRetroshelf to surface the QR code and then approve + link + sync.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_qr = false;
    while Instant::now() < deadline {
        a.update(0.05);
        if let Some(PageModel::Qr(qr)) = a.page_model() {
            if qr.url.contains(SimRetroshelf::CODE) {
                saw_qr = true;
            }
        }
        if a.retroshelf_conf().linked() && a.playtime().pending_count() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(saw_qr, "expected RetroShelf link QR with user code");
    assert!(
        a.retroshelf_conf().linked(),
        "expected RetroShelf to finish linking"
    );
    assert_eq!(a.retroshelf_conf().client_id, SimRetroshelf::CLIENT_ID);
    assert_eq!(
        a.playtime().pending_count(),
        0,
        "expected pending session to sync right after linking"
    );
    assert!(matches!(
        a.phase(),
        Phase::Retroshelf {
            page: RetroshelfPage::Main { cursor: 0 },
            ..
        }
    ));
    let fp = a.retroshelf_conf().fingerprint.clone();
    assert!(!fp.is_empty());

    // Open Playtime row (row 2) from RetroShelf -> shows synced badge.
    press(&mut a, Btn::Down);
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    let page = match a.page_model() {
        Some(PageModel::App(AppPage::Playtime(p))) => p,
        other => panic!("expected PlaytimePage, got {other:?}"),
    };
    assert_eq!(page.sync_badge, "RetroShelf · Synced");

    // X from Playtime jumps back to RetroShelf; A on Account unlinks while preserving fingerprint.
    press(&mut a, Btn::X);
    press(&mut a, Btn::A);
    assert!(!a.retroshelf_conf().linked());
    assert_eq!(retroshelf::read(d.path()).fingerprint, fp);
}
