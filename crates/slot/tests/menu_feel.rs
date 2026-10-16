mod common;

use std::time::{Duration, Instant};

use common::{app_booting_at, session_with_platform, tmp_root_with_carts, Clock, CLOCK_IS_SET};
use slot::app::{App, Phase};
use slot::audio::Sfx;
use slot::session::Session;
use slot_input::{Action, Btn, Millis, RawEvent};
use slot_store::{write_slot_state, SlotState};
use slot_ui::QuickRow;

const FRAME_MS: Millis = 16;
const DT: f32 = 1.0 / 60.0;

fn on_shelf(set: SlotState) -> (tempfile::TempDir, App, Clock) {
    // Two games, or slot goes straight into the one there is.
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..set
        },
    )
    .expect("write slot.state");
    let (mut a, clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    a.take_sfx();
    a.take_buzz();
    (d, a, clock)
}

fn press(a: &mut App, btn: Btn) -> Option<Sfx> {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
    a.take_sfx()
}

#[test]
fn the_menus_click_for_what_each_press_did() {
    let (_d, mut a, _clock) = on_shelf(SlotState::default());
    a.apply(Action::QuickMenu);
    assert_eq!(a.take_sfx(), Some(Sfx::Open), "opening the menu");
    assert_eq!(
        press(&mut a, Btn::Down),
        Some(Sfx::Move),
        "a step down the menu"
    );
    while a.quick_menu() != Some(QuickRow::General) {
        press(&mut a, Btn::Down);
    }
    assert_eq!(press(&mut a, Btn::A), Some(Sfx::Open), "into General");
    while a.quick_menu() != Some(QuickRow::Clock) {
        press(&mut a, Btn::Down);
    }
    assert_eq!(
        press(&mut a, Btn::Right),
        Some(Sfx::Toggle),
        "the clock's hours"
    );
    assert!(a.take_buzz().is_some(), "a setting changed without a nudge");
    assert_eq!(press(&mut a, Btn::B), Some(Sfx::Back), "out of General");
    a.apply(Action::QuickMenu);
    assert_eq!(a.take_sfx(), Some(Sfx::Back), "closing the menu");
}

#[test]
fn the_shelves_click_as_they_turn_and_change() {
    let (_d, mut a, _clock) = on_shelf(SlotState::default());
    a.apply(Action::ShelfRight);
    assert_eq!(a.take_sfx(), Some(Sfx::Move), "the next cart");
    a.apply(Action::GbaUp(Btn::Right));
    a.apply(Action::GbaDown(Btn::Y));
    assert_eq!(a.take_sfx(), Some(Sfx::Toggle), "a favourite");
}

#[test]
fn with_menu_sounds_off_the_menus_are_quiet_and_still_nudge() {
    let (_d, mut a, _clock) = on_shelf(SlotState {
        menu_sounds: false,
        ..SlotState::default()
    });
    a.apply(Action::QuickMenu);
    assert_eq!(press(&mut a, Btn::Down), None);
    a.apply(Action::QuickMenu);
    a.apply(Action::GbaDown(Btn::Y));
    assert_eq!(a.take_sfx(), None);
    assert!(a.take_buzz().is_some(), "Haptics is still on");
}

#[test]
fn with_haptics_off_nothing_nudges_the_motor() {
    let (_d, mut a, _clock) = on_shelf(SlotState {
        haptics: false,
        ..SlotState::default()
    });
    a.apply(Action::GbaDown(Btn::Y));
    assert_eq!(a.take_sfx(), Some(Sfx::Toggle));
    assert_eq!(a.take_buzz(), None);
    a.refuse();
    assert_eq!(a.take_buzz(), None);
}

#[test]
fn a_refusal_thunks_and_nudges() {
    let (_d, mut a, _clock) = on_shelf(SlotState::default());
    a.refuse();
    assert_eq!(a.take_sfx(), Some(Sfx::Error));
    assert!(a.take_buzz().is_some());
}

#[test]
fn the_menu_clips_start_on_their_sound_and_are_short() {
    for s in Sfx::MENU {
        let v = s.render(48_000);
        let peak_at = v
            .chunks_exact(2)
            .enumerate()
            .max_by_key(|(_, c)| c[0].unsigned_abs())
            .map(|(i, _)| i as f32 / 48_000.0)
            .unwrap();
        assert!(peak_at < 0.03, "{s:?} peaks {:.0} ms in", peak_at * 1000.0);
        assert!(
            s.tail() <= 0.2,
            "{s:?} rings for {:.0} ms",
            s.tail() * 1000.0
        );
        assert!(
            v.chunks_exact(2).all(|c| c[0] == c[1]),
            "{s:?} is not the mono recording"
        );
    }
}

fn step(s: &mut Session, now: &mut Millis, ev: Option<RawEvent>) {
    *now += FRAME_MS;
    s.feed(ev, *now);
    s.update(DT);
}

#[test]
fn the_motor_knocks_once_as_the_cart_goes_home() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let (mut s, motor) = session_with_platform(d.path());
    let mut now = 0;
    step(&mut s, &mut now, Some(RawEvent::Down(Btn::A)));
    step(&mut s, &mut now, Some(RawEvent::Up(Btn::A)));
    let (mut ran, mut stopped) = (0, false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while matches!(s.app().phase(), Phase::Inserting { .. }) {
        assert!(Instant::now() < deadline, "the cart never seated");
        step(&mut s, &mut now, None);
        match motor.last() {
            0 if ran > 0 => stopped = true,
            0 => {}
            _ => {
                assert!(!stopped, "the motor started a second time");
                ran += 1;
            }
        }
    }
    assert!(ran > 0, "the cart went home without a knock");
    assert!(ran * FRAME_MS <= 120, "the knock ran {} ms", ran * FRAME_MS);
}
