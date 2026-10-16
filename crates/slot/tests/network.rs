//! The Network pages driven against the pretend radio every build off the device uses.

mod common;

use std::time::{Duration, Instant};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::{App, PageModel, Phase};
use slot::wifi::SimWifi;
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, SlotState};
use slot_ui::{ListPage, QuickRow};

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

/// Ticks the app until `done` holds, so the radio's thread has had its say.
fn until(a: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut now = 0;
    while !done(a) && Instant::now() < deadline {
        now += 16;
        a.tick_ms(now);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(done(a), "it never settled: {:?}", a.phase());
}

fn list(a: &App) -> ListPage {
    match a.page_model() {
        Some(PageModel::List(list)) => list,
        other => panic!("not on the network list: {other:?}"),
    }
}

/// Opens the settings, turns the Wi-Fi on and opens the network list.
fn on_network_list() -> (tempfile::TempDir, App) {
    // Two games, or slot goes straight into the one there is.
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let set = SlotState {
        clock_set: true,
        ..SlotState::default()
    };
    write_slot_state(d.path(), &set).expect("write slot.state");
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::MAIN.len() {
        if a.quick_menu() == Some(QuickRow::Network) {
            break;
        }
        press(&mut a, Btn::Down);
    }
    assert_eq!(
        a.quick_menu(),
        Some(QuickRow::Network),
        "no Network row in the menu"
    );
    press(&mut a, Btn::A);
    assert_eq!(a.quick_menu(), Some(QuickRow::Wifi));
    assert!(
        a.hidden_rows().contains(&QuickRow::WifiNetworks),
        "the network list was offered with the Wi-Fi off"
    );
    press(&mut a, Btn::Right);
    assert!(
        read_slot_state(d.path()).wifi,
        "turning the Wi-Fi on was not saved"
    );
    press(&mut a, Btn::Down);
    assert_eq!(a.quick_menu(), Some(QuickRow::WifiNetworks));
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Networks { .. }));
    until(&mut a, |a| {
        list(a).rows.len() > 1 && !a.wifi_view().networks.is_empty()
    });
    (d, a)
}

fn row_of(a: &App, ssid: &str) -> usize {
    list(a)
        .rows
        .iter()
        .position(|r| r.label == ssid)
        .unwrap_or_else(|| panic!("{ssid} is not listed"))
}

fn go_to_row(a: &mut App, at: usize) {
    for _ in 0..at {
        press(a, Btn::Down);
    }
    assert_eq!(list(a).cursor, at);
}

#[test]
fn the_list_shows_every_network_in_range_and_a_way_to_a_hidden_one() {
    let (_d, a) = on_network_list();
    let page = list(&a);
    let labels: Vec<&str> = page.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Home-5G",
            "Cafe Guest",
            "Office-EAP",
            "Neighbour",
            "Other network..."
        ]
    );
    assert_eq!(page.rows[0].bars, Some(3));
    assert_eq!(page.rows[1].detail, "Open");
    assert_eq!(page.rows[2].detail, "Not supported");
}

#[test]
fn an_open_network_joins_with_one_press() {
    let (_d, mut a) = on_network_list();
    let at = row_of(&a, "Cafe Guest");
    go_to_row(&mut a, at);
    press(&mut a, Btn::A);
    until(&mut a, |a| a.wifi_view().connected.is_some());
    let page = list(&a);
    assert!(
        page.status.contains("Connected to Cafe Guest"),
        "{}",
        page.status
    );
    assert_eq!(page.rows[row_of(&a, "Cafe Guest")].detail, "Connected");
}

#[test]
fn a_secured_network_asks_for_its_password_and_says_when_it_is_wrong() {
    let (_d, mut a) = on_network_list();
    press(&mut a, Btn::A);
    let Some(PageModel::Keys(keys)) = a.page_model() else {
        panic!("Home-5G did not ask for a password: {:?}", a.phase());
    };
    assert_eq!(keys.shown(), "", "the field did not start empty");
    for _ in 0..8 {
        press(&mut a, Btn::A);
    }
    press(&mut a, Btn::Start);
    assert!(
        matches!(a.phase(), Phase::Networks { .. }),
        "Start did not send it"
    );
    until(&mut a, |a| a.wifi_view().error.is_some());
    assert_eq!(list(&a).status, "Wrong password");
    assert!(a.wifi_view().connected.is_none());
}

#[test]
fn the_keyboard_gives_up_on_menu_and_keeps_the_place_in_the_list() {
    let (_d, mut a) = on_network_list();
    let at = row_of(&a, "Neighbour");
    go_to_row(&mut a, at);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Keyboard { .. }));
    press(&mut a, Btn::B);
    assert!(
        matches!(a.phase(), Phase::Keyboard { .. }),
        "B threw the password away"
    );
    a.apply(Action::QuickMenu);
    assert!(matches!(a.phase(), Phase::Networks { cursor, .. } if *cursor == at));
}

#[test]
fn a_hidden_network_is_asked_for_by_name_then_password() {
    let (_d, mut a) = on_network_list();
    let at = list(&a).rows.len() - 1;
    go_to_row(&mut a, at);
    press(&mut a, Btn::A);
    let Some(PageModel::Keys(keys)) = a.page_model() else {
        panic!("no keyboard for the name");
    };
    assert_eq!(keys.shown(), "");
    press(&mut a, Btn::A);
    press(&mut a, Btn::Start);
    assert!(
        matches!(a.phase(), Phase::Keyboard { .. }),
        "the password was not asked for after the name"
    );
}

#[test]
fn an_unsupported_network_says_why_and_stays_put() {
    let (_d, mut a) = on_network_list();
    let at = row_of(&a, "Office-EAP");
    go_to_row(&mut a, at);
    press(&mut a, Btn::A);
    assert!(
        list(&a).status.contains("not supported"),
        "{}",
        list(&a).status
    );
}

#[test]
fn b_goes_back_to_the_network_settings() {
    let (_d, mut a) = on_network_list();
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::WifiNetworks));
}

#[test]
fn the_pretend_radio_takes_the_password_it_says_it_does() {
    assert_eq!(
        SimWifi::PASSWORD.len(),
        8,
        "the sim password is no valid WPA length"
    );
}

/// Opens the settings' Network page, turns Bluetooth on and opens its device list.
fn on_device_list() -> (tempfile::TempDir, App) {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let set = SlotState {
        clock_set: true,
        ..SlotState::default()
    };
    write_slot_state(d.path(), &set).expect("write slot.state");
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::MAIN.len() {
        if a.quick_menu() == Some(QuickRow::Network) {
            break;
        }
        press(&mut a, Btn::Down);
    }
    press(&mut a, Btn::A);
    for _ in 0..QuickRow::NETWORK.len() {
        if a.quick_menu() == Some(QuickRow::Bluetooth) {
            break;
        }
        press(&mut a, Btn::Down);
    }
    assert_eq!(
        a.quick_menu(),
        Some(QuickRow::Bluetooth),
        "no Bluetooth row"
    );
    assert!(a.hidden_rows().contains(&QuickRow::BtDevices));
    press(&mut a, Btn::Right);
    assert!(
        read_slot_state(d.path()).bluetooth,
        "turning Bluetooth on was not saved"
    );
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Devices { .. }),
        "{:?}",
        a.phase()
    );
    until(&mut a, |a| a.bt_view().devices.len() == 3);
    (d, a)
}

#[test]
fn bluetooth_searches_when_nothing_is_paired_and_pairs_headphones_with_a() {
    let (_d, mut a) = on_device_list();
    let at = list(&a)
        .rows
        .iter()
        .position(|r| r.label == "WH-1000XM4")
        .expect("the headphones were not found");
    assert_eq!(list(&a).rows[at].detail, "Headphones");
    go_to_row(&mut a, at);
    press(&mut a, Btn::A);
    until(&mut a, |a| a.bt_view().audio().is_some());
    let page = list(&a);
    assert_eq!(
        page.rows[0].label, "WH-1000XM4",
        "the connected device is not first"
    );
    assert_eq!(page.rows[0].detail, "Connected");
    assert_eq!(page.status, "1 device connected");
}

#[test]
fn a_device_that_will_not_pair_says_so() {
    let (_d, mut a) = on_device_list();
    let at = list(&a)
        .rows
        .iter()
        .position(|r| r.label == "Stubborn")
        .expect("listed");
    go_to_row(&mut a, at);
    press(&mut a, Btn::A);
    until(&mut a, |a| a.bt_view().error.is_some());
    assert_eq!(list(&a).status, "Could not connect to Stubborn");
}

#[test]
fn x_forgets_a_paired_device() {
    let (_d, mut a) = on_device_list();
    press(&mut a, Btn::A);
    until(&mut a, |a| a.bt_view().devices.iter().any(|d| d.paired));
    press(&mut a, Btn::X);
    until(&mut a, |a| !a.bt_view().devices.iter().any(|d| d.paired));
}
