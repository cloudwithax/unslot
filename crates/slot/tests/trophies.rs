use slot::app::{App, PageModel, Phase};
use slot::trophies::{TrophyIntro, TrophyViewMode};
use slot_input::{Action, Btn};
use slot_store::{write_slot_state, SlotState};
use slot_ui::{QuickRow, TrophyTier};

fn press(app: &mut App, btn: Btn) {
    app.apply(Action::GbaDown(btn));
    app.apply(Action::GbaUp(btn));
}

fn open_trophy_room(app: &mut App) {
    app.apply(Action::QuickMenu);
    while app.quick_menu() != Some(QuickRow::Online) {
        press(app, Btn::Down);
    }
    press(app, Btn::A);
    while app.quick_menu() != Some(QuickRow::TrophyRoom) {
        press(app, Btn::Down);
    }
    press(app, Btn::A);
    assert!(matches!(app.phase(), Phase::Trophies(_)));
}

fn seed_ra_catalog(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("Config/retroachievements/cache")).unwrap();
    std::fs::create_dir_all(root.join("Config/retroachievements/unlocks")).unwrap();
    std::fs::create_dir_all(root.join("Config/retroachievements/queue")).unwrap();
    std::fs::write(
        root.join("Config/retroachievements.conf"),
        "user = Player\ntoken = tok123\n",
    )
    .unwrap();

    // GBA (ConsoleId 5): Metroid Fusion (501) -> 2/2 Platinum; Aria of Sorrow (502) -> 1/2 Silver.
    std::fs::write(
        root.join("Config/retroachievements/cache/achievementsets-501.json"),
        r#"{"Success":true,"GameId":501,"Title":"Metroid Fusion","ConsoleId":5,"Sets":[{"Type":"core","Achievements":[{"ID":1001,"Title":"Wake Up","Description":"Recover from X parasite","Flags":3,"Points":50,"BadgeName":"1001"},{"ID":1002,"Title":"SA-X Down","Description":"Escape BSL station","Flags":3,"Points":25,"BadgeName":"1002"}]}]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("Config/retroachievements/cache/achievementsets-502.json"),
        r#"{"Success":true,"GameId":502,"Title":"Aria of Sorrow","ConsoleId":5,"Sets":[{"Type":"core","Achievements":[{"ID":1003,"Title":"Castle Gate","Description":"Defeat Creaking Skull","Flags":3,"Points":10,"BadgeName":"1003"},{"ID":1004,"Title":"Chaos","Description":"Best ending","Flags":3,"Points":50,"BadgeName":"1004"}]}]}"#,
    )
    .unwrap();

    // SNES (ConsoleId 3): Chrono Trigger (301) -> 0/2 Locked; Super Metroid (302) -> 1/4 Bronze (queued).
    std::fs::write(
        root.join("Config/retroachievements/cache/achievementsets-301.json"),
        r#"{"Success":true,"GameId":301,"Title":"Chrono Trigger","ConsoleId":3,"Sets":[{"Type":"core","Achievements":[{"ID":2001,"Title":"Millennial Fair","Description":"Meet Marle","Flags":3,"Points":5,"BadgeName":"2001"},{"ID":2002,"Title":"Lavos","Description":"Save the future","Flags":3,"Points":50,"BadgeName":"2002"}]}]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("Config/retroachievements/cache/achievementsets-302.json"),
        r#"{"Success":true,"GameId":302,"Title":"Super Metroid","ConsoleId":3,"Sets":[{"Type":"core","Achievements":[{"ID":2003,"Title":"Morph Ball","Description":"Grab the Morphing Ball","Flags":3,"Points":5,"BadgeName":"2003"},{"ID":2004,"Title":"Kraid","Description":"Defeat Kraid","Flags":3,"Points":10,"BadgeName":"2004"},{"ID":2005,"Title":"Phantoon","Description":"Defeat Phantoon","Flags":3,"Points":10,"BadgeName":"2005"},{"ID":2006,"Title":"Mother Brain","Description":"Escape Zebes","Flags":3,"Points":25,"BadgeName":"2006"}]}]}"#,
    )
    .unwrap();

    std::fs::write(
        root.join("Config/retroachievements/unlocks/Player.txt"),
        "1001 1700000001 h\n1002 1700000002\n1003 1700000003 h\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Config/retroachievements/queue/00001-0-0.txt"),
        "1700000010\nhttps://retroachievements.org/dorequest.php\nr=awardachievement&u=Player&a=2003&h=0\n",
    )
    .unwrap();
}

/// Puts a cart on the card for each seeded set, named as RetroAchievements names it.
fn seed_carts(root: &std::path::Path) {
    for (dir, name) in [
        ("GBA", "Aria of Sorrow (USA).gba"),
        ("GBA", "Metroid Fusion (USA).gba"),
        ("SNES", "Chrono Trigger (USA).sfc"),
        ("SNES", "Super Metroid (USA).sfc"),
    ] {
        let dir = root.join("Games").join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), b"rom").unwrap();
    }
}

#[test]
fn trophy_room_carousel_rises_from_the_shelf_then_drills_into_games() {
    let d = tempfile::tempdir().unwrap();
    slot::root::ensure(d.path());
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    seed_ra_catalog(d.path());
    seed_carts(d.path());

    let mut app = App::boot(d.path());
    open_trophy_room(&mut app);

    // 1. The way in: the shelf first for a beat, then the carts go down and the room comes up.
    let Phase::Trophies(t) = app.phase() else {
        panic!("not in the room");
    };
    assert_eq!(t.intro, TrophyIntro::Beat);
    assert_eq!(t.brass(), 0.0);
    app.update(0.6);
    let Phase::Trophies(t) = app.phase() else {
        panic!()
    };
    assert_eq!(t.intro, TrophyIntro::Duck);
    assert!(t.sink(300.0) > 0.0, "the carts are not going down");
    app.update(0.4);
    let Phase::Trophies(t) = app.phase() else {
        panic!()
    };
    assert_eq!(t.intro, TrophyIntro::Rise);
    app.update(0.5);
    let Phase::Trophies(t) = app.phase() else {
        panic!()
    };
    assert_eq!(t.intro, TrophyIntro::Done);
    assert_eq!(t.sink(300.0), 0.0);
    assert_eq!(t.brass(), 1.0);

    // 2. The carousel is the shelf's: the card's carts, system first then game, each with its
    // trophies, starting with the one in the middle, named where the shelf names its own.
    assert_eq!(app.title_text().unwrap(), "Aria of Sorrow · 1/2");
    let detail = app.slot_text().unwrap();
    assert!(
        detail.contains("1 OF 4") && detail.contains("ALL SYSTEMS"),
        "{detail}"
    );
    assert_eq!(app.note_text(), None);

    // Right walks the carousel, and the name follows the cart.
    press(&mut app, Btn::Right);
    assert!(app.title_text().unwrap().starts_with("Metroid Fusion · "));

    // 3. A goes a step further: the game's own trophies, a carousel of them in the same light.
    press(&mut app, Btn::A);
    let Phase::Trophies(t) = app.phase() else {
        panic!()
    };
    assert!(matches!(t.mode, TrophyViewMode::Game(501)));
    assert_eq!(t.strip_tiers().len(), 2);
    assert_eq!(t.strip_tiers()[0], TrophyTier::Platinum);
    assert_eq!(app.note_text().unwrap(), "METROID FUSION · 1 OF 2");
    assert_eq!(app.slot_text().unwrap(), "Recover from X parasite");
    assert!(app.title_text().unwrap().starts_with("Wake Up · "));
    // Right moves to the next trophy, which the carousel follows, and stops at the last.
    press(&mut app, Btn::Right);
    press(&mut app, Btn::Right);
    assert_eq!(app.note_text().unwrap(), "METROID FUSION · 2 OF 2");
    app.update(0.1);
    let Phase::Trophies(t) = app.phase() else {
        panic!()
    };
    assert!(t.scroll() > 0.0 && t.scroll() < 1.0, "{}", t.scroll());
    app.update(1.0);
    let Phase::Trophies(t) = app.phase() else {
        panic!()
    };
    assert_eq!(t.scroll(), 1.0);

    // B comes back to the carousel of games, on the same cart.
    press(&mut app, Btn::B);
    assert!(app.title_text().unwrap().starts_with("Metroid Fusion · "));
    assert_eq!(app.note_text(), None);

    // 4. X narrows to one system: only that system's carts are left, and the cart in hand is
    // still in hand.
    press(&mut app, Btn::X);
    let detail = app.slot_text().unwrap();
    assert!(
        detail.contains("GAME BOY ADVANCE") && detail.contains("2 OF 2"),
        "{detail}"
    );
    assert!(app.title_text().unwrap().starts_with("Metroid Fusion · "));

    // 5. Select opens the game set, and what is ticked is what the carousel shows.
    press(&mut app, Btn::Select);
    let Some(PageModel::List(picker)) = app.page_model() else {
        panic!("expected the Game Set picker");
    };
    assert_eq!(picker.title, "Trophy Game Set");
    assert_eq!(picker.rows.len(), 4);
    press(&mut app, Btn::A);
    press(&mut app, Btn::Down);
    press(&mut app, Btn::Down);
    press(&mut app, Btn::Down);
    press(&mut app, Btn::A);
    press(&mut app, Btn::B);
    assert!(app.page_model().is_none());
    let detail = app.slot_text().unwrap();
    assert!(
        detail.contains("GAME SET") && detail.contains("1 OF 2"),
        "{detail}"
    );
    assert!(app.title_text().unwrap().starts_with("Aria of Sorrow · "));
    press(&mut app, Btn::Right);
    assert!(app.title_text().unwrap().starts_with("Super Metroid · "));

    // 6. Start has a carousel of every trophy in the set, and Start again goes back.
    press(&mut app, Btn::Start);
    let Phase::Trophies(t) = app.phase() else {
        panic!()
    };
    assert_eq!(t.strip_tiers().len(), 6);
    assert!(app.note_text().unwrap().ends_with("1 OF 6"));
    press(&mut app, Btn::Start);
    assert_eq!(app.note_text(), None);
    assert!(app.title_text().unwrap().starts_with("Super Metroid · "));

    // 7. X goes on to every game, and a reboot keeps the set that was ticked.
    press(&mut app, Btn::X);
    let detail = app.slot_text().unwrap();
    assert!(
        detail.contains("ALL GAMES") && detail.contains("OF 4"),
        "{detail}"
    );

    drop(app);
    let mut rebooted = App::boot(d.path());
    open_trophy_room(&mut rebooted);
    press(&mut rebooted, Btn::B); // settles the way in
    assert!(matches!(rebooted.phase(), Phase::Trophies(_)));
    press(&mut rebooted, Btn::B); // and leaves
    assert_eq!(rebooted.quick_menu(), Some(QuickRow::TrophyRoom));

    open_trophy_room(&mut rebooted);
    rebooted.update(2.0);
    press(&mut rebooted, Btn::X); // SingleSystem
    press(&mut rebooted, Btn::X); // GameSet
    let detail = rebooted.slot_text().unwrap();
    assert!(
        detail.contains("GAME SET") && detail.contains("1 OF 2"),
        "{detail}"
    );
}
