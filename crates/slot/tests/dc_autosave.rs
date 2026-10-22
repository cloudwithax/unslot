mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::tmp_root_with_carts;
use slot::app::{App, Phase};
use slot::persist::Snapshot;
use slot_input::Action;
use slot_store::{write_slot_state, Platform, SlotState};

struct CountingSnapshot {
    states: Arc<AtomicUsize>,
}

impl Snapshot for CountingSnapshot {
    fn state(&self) -> Option<Vec<u8>> {
        self.states.fetch_add(1, Ordering::Relaxed);
        Some(vec![0; 64])
    }

    fn save_ram(&self) -> Option<Vec<u8>> {
        None
    }

    fn thumb(&self) -> Option<Vec<u8>> {
        None
    }

    fn load(&self, _state: Vec<u8>) {}
}

#[test]
fn a_dreamcast_game_takes_no_autosave_but_still_saves_on_sleep() {
    let d = tmp_root_with_carts(&[]);
    std::fs::create_dir_all(d.path().join("Games/DC")).unwrap();
    std::fs::write(d.path().join("Games/DC/Crazy Taxi 2.chd"), [0u8; 16]).unwrap();
    write_slot_state(
        d.path(),
        &SlotState {
            cart: Some("Crazy Taxi 2".into()),
            cart_platform: Some(Platform::Dreamcast),
            clock_set: true,
            ..Default::default()
        },
    )
    .unwrap();
    let states = Arc::new(AtomicUsize::new(0));
    let mut a = App::boot(d.path());
    a.set_snapshot(Box::new(CountingSnapshot {
        states: states.clone(),
    }));
    while !a.may_spawn_core() {
        a.update(1.0 / 60.0);
    }
    a.set_platform(Platform::Dreamcast);
    a.on_core_ready();
    a.update(1.0 / 60.0);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    a.tick_ms(900_000);
    assert_eq!(
        states.load(Ordering::Relaxed),
        0,
        "a Dreamcast game was autosaved"
    );
    a.apply(Action::LidClose);
    assert_eq!(states.load(Ordering::Relaxed), 1, "sleep saved no state");
}
