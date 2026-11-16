mod common;

use slot::baseos::{configured_hostname, rewrite_hostname, set_hostname, valid_hostname};
use slot::bootlogo::{choices, compose, fit, Bmp};
use slot::idle::{Activity, IdleTimer};
use slot::upload::{place, Landed};
use slot_input::{Action, Btn};
use slot_store::SlotState;

#[test]
fn idle_sleep_obeys_every_duration_and_input_reset() {
    for minutes in [1, 3, 5, 10] {
        let mut timer = IdleTimer::default();
        let timeout = u64::from(minutes) * 60_000;
        assert!(!timer.tick(timeout - 1, minutes, Activity::default()));
        timer.input(timeout - 1);
        assert!(!timer.tick(timeout, minutes, Activity::default()));
        assert!(timer.tick(timeout * 2 - 1, minutes, Activity::default()));
    }
    assert!(!IdleTimer::default().tick(u64::MAX, 0, Activity::default()));
}

#[test]
fn each_exception_starts_a_fresh_idle_interval() {
    let exceptions = [
        Activity {
            link: true,
            ..Activity::default()
        },
        Activity {
            video: true,
            ..Activity::default()
        },
        Activity {
            music: true,
            ..Activity::default()
        },
        Activity {
            casting: true,
            ..Activity::default()
        },
        Activity {
            downloading: true,
            ..Activity::default()
        },
        Activity {
            asleep: true,
            ..Activity::default()
        },
        Activity {
            powering_off: true,
            ..Activity::default()
        },
    ];
    for activity in exceptions {
        let mut timer = IdleTimer::default();
        assert!(!timer.tick(120_000, 1, activity));
        assert!(!timer.tick(179_999, 1, Activity::default()));
        assert!(timer.tick(180_000, 1, Activity::default()));
    }
}

#[test]
fn hostname_validation_matches_baseos() {
    for name in ["a", "0", "RG-CubeXX", &"a".repeat(63)] {
        assert!(valid_hostname(name), "{name}");
    }
    for name in [
        "",
        "-a",
        "a-",
        "a_b",
        "a.b",
        "a b",
        "é",
        "a\n",
        &"a".repeat(64),
    ] {
        assert!(!valid_hostname(name), "{name:?}");
    }
}

#[test]
fn hostname_rewrites_preserve_other_keys_comments_and_line_endings() {
    for (before, after) in [
        ("hostname=old\nmdns=true\n", "hostname=new\nmdns=true\n"),
        ("# hostname=old\n# note\n", "hostname=new\n# note\n"),
        ("mdns=false\n", "mdns=false\nhostname=new\n"),
        ("mdns=false", "mdns=false\nhostname=new\n"),
        ("", "hostname=new\n"),
        (
            "# hostname=old\r\nssh_password=secret\r\n",
            "hostname=new\r\nssh_password=secret\r\n",
        ),
        (
            "hostname=old\r\nmdns=true\r\n",
            "hostname=new\r\nmdns=true\r\n",
        ),
        (
            "# hostname=example\nhostname=old\n",
            "# hostname=example\nhostname=new\n",
        ),
        ("hostname = old # note\n", "hostname=new # note\n"),
        (
            "# hostname=old # note\r\nmdns=true\r\n",
            "hostname=new # note\r\nmdns=true\r\n",
        ),
    ] {
        assert_eq!(rewrite_hostname(before, "new"), after);
    }
    let dir = tempfile::tempdir().unwrap();
    set_hostname(dir.path(), "new").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("baseos.conf")).unwrap(),
        "hostname=new\n"
    );
    assert_eq!(configured_hostname(dir.path()), "new");
    assert!(set_hostname(dir.path(), "-bad").is_err());
    assert_eq!(configured_hostname(dir.path()), "new");
}

#[test]
fn boot_logo_choices_and_scaling_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(choices(dir.path()), ["unslot", "BaseOS"]);
    for name in ["z.JPG", "Alpha.png", "beta.jpeg", "gamma.BMP", "skip.gif"] {
        std::fs::write(dir.path().join("Bootlogos").join(name), []).unwrap();
    }
    assert_eq!(
        choices(dir.path()),
        [
            "unslot",
            "BaseOS",
            "Alpha.png",
            "beta.jpeg",
            "gamma.BMP",
            "z.JPG"
        ]
    );
    let image = || Bmp {
        width: 1200,
        height: 600,
        rgb: vec![[2, 3, 4]; 1200 * 600],
    };
    let fitted = fit(image(), (640, 480));
    assert_eq!((fitted.width, fitted.height), (640, 320));
    let portrait = fit(image(), (480, 640));
    let composed = compose(&portrait, (480, 640)).unwrap();
    assert_eq!((composed.width, composed.height), (480, 640));
    let small = fit(
        Bmp {
            width: 2,
            height: 1,
            rgb: vec![[0; 3]; 2],
        },
        (640, 480),
    );
    assert_eq!((small.width, small.height), (2, 1));
}

#[test]
fn upload_boot_logo_accepts_only_images_under_eight_mib() {
    let root = std::path::Path::new("/card");
    for name in ["test.png", "test.JPG", "test.jpeg", "test.bmp"] {
        assert_eq!(
            place(root, "bootlogo", name, 8 << 20).unwrap(),
            (
                root.join("Bootlogos").join(name),
                Landed::BootLogo(name.into())
            )
        );
    }
    assert_eq!(
        place(root, "bootlogo", "test.gif", 1).unwrap_err(),
        "A boot logo is a PNG, a JPEG or a BMP"
    );
    assert_eq!(
        place(root, "bootlogo", "test.png", (8 << 20) + 1).unwrap_err(),
        "That picture is too big"
    );
    assert!(place(root, "bootlogo", "../test.png", 1).is_err());
}

#[test]
fn idle_sleep_saves_and_keeps_the_game_seated() {
    let dir = common::tmp_root_with_carts(&["Emerald"]);
    let mut app = common::app_playing_with_state(
        dir.path(),
        SlotState {
            cart: Some("Emerald".into()),
            clock_set: true,
            sleep_after: 1,
            ..SlotState::default()
        },
    );
    app.apply_at(Action::GbaDown(Btn::A), 30_000);
    app.tick_ms(89_999);
    assert!(!matches!(app.phase(), slot::app::Phase::Doze { .. }));
    app.tick_ms(90_000);
    assert!(matches!(
        app.phase(),
        slot::app::Phase::Doze { cart: Some(_) }
    ));
    assert_eq!(
        slot_store::read_slot_state(dir.path()).cart.as_deref(),
        Some("Emerald")
    );
}

#[test]
fn never_stays_asleep_until_woken() {
    let dir = common::tmp_root_with_carts(&["Emerald"]);
    let mut app = common::app_playing_with_state(
        dir.path(),
        SlotState {
            cart: Some("Emerald".into()),
            clock_set: true,
            power_off_after: 0,
            ..SlotState::default()
        },
    );
    app.apply(Action::LidClose);
    app.tick_ms(24 * 60 * 60 * 1000);
    assert!(!app.powering_off());
    app.on_doze_timeout();
    assert!(!app.powering_off());
    assert!(matches!(app.phase(), slot::app::Phase::Doze { .. }));
    app.apply(Action::LidOpen);
    assert!(matches!(app.phase(), slot::app::Phase::Playing { .. }));
}

#[test]
fn raw_buttons_and_normalized_sticks_reset_idle_even_without_a_gesture() {
    use slot_input::RawEvent;
    for button in [Btn::A, Btn::Down, Btn::VolUp, Btn::VolDown, Btn::Select] {
        let dir = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
        slot_store::write_slot_state(
            dir.path(),
            &SlotState {
                sleep_after: 1,
                clock_set: true,
                ..SlotState::default()
            },
        )
        .unwrap();
        let (mut session, _) = common::session_with_platform(dir.path());
        session.feed([RawEvent::Down(button)], 59_999);
        session.app_mut().tick_ms(60_000);
        assert!(
            !matches!(session.app().phase(), slot::app::Phase::Doze { .. }),
            "{button:?}"
        );
        session.feed([RawEvent::Up(button)], 60_001);
        session.app_mut().tick_ms(120_000);
        assert!(
            !matches!(session.app().phase(), slot::app::Phase::Doze { .. }),
            "{button:?}"
        );
        session.app_mut().tick_ms(120_001);
        assert!(
            matches!(session.app().phase(), slot::app::Phase::Doze { .. }),
            "{button:?}"
        );
    }
    let dir = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    slot_store::write_slot_state(
        dir.path(),
        &SlotState {
            sleep_after: 1,
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    let (mut session, _) = common::session_with_platform(dir.path());
    session.set_sticks([1, 0, 0, 0]); // InputSource already applies the handheld's radial deadzone.
    session.feed([], 59_999);
    session.set_sticks([0; 4]);
    session.app_mut().tick_ms(119_998);
    assert!(!matches!(
        session.app().phase(),
        slot::app::Phase::Doze { .. }
    ));
    session.app_mut().tick_ms(119_999);
    assert!(matches!(
        session.app().phase(),
        slot::app::Phase::Doze { .. }
    ));
}

#[test]
fn the_new_settings_round_trip_without_affecting_defaults() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Config")).unwrap();
    let state = SlotState {
        sleep_after: 10,
        power_off_after: 0,
        boot_logo: "A picture.jpg".into(),
        ..SlotState::default()
    };
    slot_store::write_slot_state(dir.path(), &state).unwrap();
    assert_eq!(slot_store::read_slot_state(dir.path()), state);
}

#[test]
fn holding_a_button_is_not_idle_time() {
    use slot_input::RawEvent;
    let dir = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    slot_store::write_slot_state(
        dir.path(),
        &SlotState {
            sleep_after: 1,
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    let (mut session, _) = common::session_with_platform(dir.path());
    session.feed([RawEvent::Down(Btn::A)], 0);
    session.feed([], 120_000);
    session.app_mut().tick_ms(120_000);
    assert!(!matches!(
        session.app().phase(),
        slot::app::Phase::Doze { .. }
    ));
    session.feed([RawEvent::Up(Btn::A)], 120_001);
    session.app_mut().tick_ms(180_000);
    assert!(!matches!(
        session.app().phase(),
        slot::app::Phase::Doze { .. }
    ));
    session.app_mut().tick_ms(180_001);
    assert!(matches!(
        session.app().phase(),
        slot::app::Phase::Doze { .. }
    ));
}
