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

/// Opens the services page under Network.
fn on_services(d: &tempfile::TempDir) -> App {
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
        if a.quick_menu() == Some(QuickRow::Services) {
            break;
        }
        press(&mut a, Btn::Down);
    }
    assert_eq!(a.quick_menu(), Some(QuickRow::Services), "no Services row");
    press(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Services { .. }),
        "{:?}",
        a.phase()
    );
    a
}

fn detail(a: &App, label: &str) -> String {
    list(a)
        .rows
        .iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no {label} row"))
        .detail
        .clone()
}

fn go_to(a: &mut App, label: &str) {
    let at = row_of(a, label);
    go_to_row(a, at);
}

#[test]
fn ssh_starts_on_and_turning_it_off_is_kept() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = on_services(&d);
    assert_eq!(detail(&a, "SSH"), "On");
    assert_eq!(detail(&a, "Password"), "BaseOS's");
    press(&mut a, Btn::A);
    assert_eq!(detail(&a, "SSH"), "Off");
    assert!(
        !read_slot_state(d.path()).ssh,
        "turning SSH off was not saved"
    );
}

#[test]
fn file_sharing_asks_for_a_password_first_then_shares() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = on_services(&d);
    go_to(&mut a, "File sharing");
    press(&mut a, Btn::A);
    assert!(
        matches!(a.page_model(), Some(PageModel::Keys(_))),
        "sharing began with no password: {:?}",
        a.phase()
    );
    for _ in 0..4 {
        press(&mut a, Btn::A);
    }
    press(&mut a, Btn::Start);
    assert!(
        matches!(a.phase(), Phase::Services { .. }),
        "{:?}",
        a.phase()
    );
    let saved = read_slot_state(d.path());
    assert!(saved.samba, "sharing did not come on after the password");
    assert_eq!(saved.password.map(|p| p.len()), Some(4));
    assert_eq!(detail(&a, "File sharing"), "On");
    assert_eq!(detail(&a, "Password"), "Set");
}

#[test]
fn tailscale_shows_a_code_to_sign_in_and_x_turns_it_off() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = on_services(&d);
    go_to(&mut a, "Tailscale");
    press(&mut a, Btn::A);
    until(&mut a, |a| detail(a, "Tailscale") == "Sign in");
    assert_eq!(list(&a).hints, "A sign in   B back");
    press(&mut a, Btn::A);
    let Some(PageModel::Qr(qr)) = a.page_model() else {
        panic!("no code to scan: {:?}", a.phase());
    };
    assert_eq!(qr.url, slot::services::SimServices::URL);
    press(&mut a, Btn::X);
    assert!(matches!(a.phase(), Phase::Services { .. }));
    assert!(!read_slot_state(d.path()).tailscale);
    until(&mut a, |a| detail(a, "Tailscale") == "Off");
}

#[test]
fn the_vpn_steps_through_the_cards_tunnels_and_back_to_off() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = on_services(&d);
    go_to(&mut a, "VPN");
    press(&mut a, Btn::A);
    assert_eq!(
        list(&a).status,
        "Put WireGuard configs in Config/wireguard on the card"
    );
    let dir = d.path().join(slot::wireguard::WG_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    let conf = "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.0.0.2/32\n[Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nAllowedIPs = 10.0.0.0/24\n";
    for name in ["home", "work"] {
        std::fs::write(dir.join(format!("{name}.conf")), conf).unwrap();
    }
    press(&mut a, Btn::A);
    until(&mut a, |a| detail(a, "VPN") == "home");
    press(&mut a, Btn::A);
    until(&mut a, |a| detail(a, "VPN") == "work");
    assert_eq!(read_slot_state(d.path()).vpn.as_deref(), Some("work"));
    press(&mut a, Btn::A);
    assert_eq!(detail(&a, "VPN"), "Off");
    assert_eq!(read_slot_state(d.path()).vpn, None);
}

/// Opens the updates page under Network, which looks for releases as it opens.
fn on_updates(d: &tempfile::TempDir) -> App {
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
        if a.quick_menu() == Some(QuickRow::Updates) {
            break;
        }
        press(&mut a, Btn::Down);
    }
    assert_eq!(a.quick_menu(), Some(QuickRow::Updates), "no Updates row");
    press(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Updates { .. }),
        "{:?}",
        a.phase()
    );
    a
}

#[test]
fn updates_offer_the_newer_releases_and_a_downloaded_one_restarts_to_install() {
    use slot::update::SimUpdates;
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::write(d.path().join("System/version"), "v2.0.0\n").unwrap();
    let mut a = on_updates(&d);
    let get = |v: &str| format!("Get {v}");
    until(&mut a, |a| {
        detail(a, "unslot") == get(SimUpdates::UNSLOT)
            && detail(a, "BaseOS") == get(SimUpdates::BASEOS)
    });
    assert_eq!(list(&a).status, "unslot v2.0.0   BaseOS 1.4.0");
    press(&mut a, Btn::A);
    until(&mut a, |a| detail(a, "unslot") == "Restart");
    assert!(
        d.path().join(slot::update::PACK).exists(),
        "the pack is not where BaseOS looks"
    );
    assert_eq!(
        list(&a).status,
        "Restart to install, with the power connected"
    );
    press(&mut a, Btn::A);
    assert!(a.restarting(), "A on a downloaded update did not restart");
}

#[test]
fn game_art_labels_what_it_finds_and_restarts_to_show_it() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    // Eight zero bytes: the game the pretend database knows by its CRC.
    std::fs::write(d.path().join("Games/GBA/Zero.gba"), [0u8; 8]).unwrap();
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
        if a.quick_menu() == Some(QuickRow::GameArt) {
            break;
        }
        press(&mut a, Btn::Down);
    }
    assert_eq!(a.quick_menu(), Some(QuickRow::GameArt), "no Game Art row");
    press(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::GameArt { .. }),
        "{:?}",
        a.phase()
    );
    assert_eq!(detail(&a, "All systems"), "0 of 3");
    assert_eq!(detail(&a, "Game Boy Advance"), "0 of 3");
    press(&mut a, Btn::A);
    until(&mut a, |a| {
        !a.scrape_view().running && a.scrape_view().done == 3
    });
    assert!(
        d.path().join("Labels/GBA/Zero.png").exists(),
        "no label on the card"
    );
    assert_eq!(detail(&a, "All systems"), "1 of 3");
    assert_eq!(
        list(&a).status,
        "1 new label. Press START to restart and see them"
    );
    press(&mut a, Btn::A);
    until(&mut a, |a| !a.scrape_view().running);
    press(&mut a, Btn::Start);
    assert!(a.restarting(), "START did not restart to show the labels");
}

#[test]
fn the_footer_shows_wi_fi_once_it_has_joined_a_network() {
    let (_d, mut a) = on_network_list();
    let (wifi, bt) = (slot_ui::TexId::from_raw(901), slot_ui::TexId::from_raw(902));
    a.set_net_faces([wifi, bt]);
    assert!(a.net_icons().is_empty(), "an icon before joining anything");
    let at = row_of(&a, "Cafe Guest");
    go_to_row(&mut a, at);
    press(&mut a, Btn::A);
    until(&mut a, |a| a.wifi_view().connected.is_some());
    assert_eq!(a.net_icons(), [wifi], "no Wi-Fi icon once joined");
}
