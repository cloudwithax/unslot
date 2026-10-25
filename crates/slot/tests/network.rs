//! The Network pages driven against the pretend radio every build off the device uses.

mod common;

use std::time::{Duration, Instant};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::{App, PageModel, Phase};
use slot::wifi::SimWifi;
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, Platform, SlotState};
use slot_ui::{ButtonLegend, ListPage, QuickRow};

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

/// What A says it does on the row the cursor is on, if anything.
fn action(a: &App) -> Option<String> {
    list(a)
        .legends
        .into_iter()
        .find(|l| l.buttons == [ButtonLegend::A])
        .map(|l| l.label)
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
    until(&mut a, |a| a.bt_view().devices.len() == 4);
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
        "the paired device is not first"
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
fn a_bluetooth_le_controller_that_never_bonds_says_so_instead_of_connected() {
    let (_d, mut a) = on_device_list();
    let at = list(&a)
        .rows
        .iter()
        .position(|r| r.label == "Xbox Wireless Controller")
        .expect("listed");
    go_to_row(&mut a, at);
    press(&mut a, Btn::A);
    until(&mut a, |a| a.bt_view().error.is_some());
    let page = list(&a);
    assert_eq!(
        page.status,
        "Xbox Wireless Controller would not pair. Turn Bluetooth off and on, then try again"
    );
    assert!(page.rows.iter().all(|r| r.detail != "Connected"));
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
fn syncthing_asks_for_a_password_then_x_shows_its_device_id() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = on_services(&d);
    go_to(&mut a, "Syncthing");
    press(&mut a, Btn::A);
    assert!(
        matches!(a.page_model(), Some(PageModel::Keys(_))),
        "Syncthing began with no password: {:?}",
        a.phase()
    );
    for _ in 0..4 {
        press(&mut a, Btn::A);
    }
    press(&mut a, Btn::Start);
    let saved = read_slot_state(d.path());
    assert!(
        saved.syncthing,
        "Syncthing did not come on after the password"
    );
    assert!(!saved.samba, "the password turned file sharing on too");
    assert_eq!(detail(&a, "Syncthing"), "On");
    until(&mut a, |a| a.services_view().syncthing_id.is_some());
    press(&mut a, Btn::X);
    let Some(PageModel::Qr(qr)) = a.page_model() else {
        panic!("no device ID to scan: {:?}", a.phase());
    };
    assert_eq!(qr.url, slot::services::SimServices::SYNCTHING_ID);
    press(&mut a, Btn::B);
    assert!(matches!(a.phase(), Phase::Services { .. }));
    assert_eq!(action(&a), Some("Turn Off".to_string()));
}

#[test]
fn tailscale_shows_a_code_to_sign_in_and_x_turns_it_off() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = on_services(&d);
    go_to(&mut a, "Tailscale");
    press(&mut a, Btn::A);
    until(&mut a, |a| detail(a, "Tailscale") == "Sign in");
    assert_eq!(action(&a), Some("Sign In".to_string()));
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
    let get = |v: &str| format!("{v} available");
    until(&mut a, |a| {
        detail(a, "unslot") == get(SimUpdates::UNSLOT)
            && detail(a, "BaseOS") == get(SimUpdates::BASEOS)
    });
    assert_eq!(list(&a).status, "unslot v2.0.0   BaseOS 1.4.0");
    assert_eq!(action(&a), Some(format!("Download {}", SimUpdates::UNSLOT)));
    press(&mut a, Btn::A);
    until(&mut a, |a| detail(a, "unslot") == "Ready to install");
    assert!(
        d.path()
            .join(slot::pak::file_name(SimUpdates::UNSLOT))
            .exists(),
        "the signed pack is not at the card's root"
    );
    assert_eq!(
        list(&a).status,
        "Installs when the device restarts. Keep the power connected"
    );
    assert_eq!(action(&a), Some("Restart to Install".to_string()));
    press(&mut a, Btn::A);
    assert!(a.restarting(), "A on a downloaded update did not restart");
}

#[test]
fn game_art_labels_what_it_finds_and_shows_it_without_a_restart() {
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
    assert_eq!(action(&a), Some("Find Missing Art".to_string()));
    let zero = (0..Platform::COUNT + 2)
        .flat_map(|s| (0..3).map(move |i| (s, i)))
        .find(|&(s, i)| a.face_cart(s, i).is_some_and(|c| c.stem == "Zero"))
        .expect("Zero is on a shelf");
    a.set_cart_face(zero.0, zero.1, slot_ui::TexId::from_raw(7));
    press(&mut a, Btn::A);
    until(&mut a, |a| {
        !a.scrape_view().running && a.scrape_view().done == 3
    });
    assert!(
        d.path().join("Labels/GBA/Zero.png").exists(),
        "no label on the card"
    );
    assert_eq!(detail(&a, "All systems"), "1 of 3");
    assert_eq!(list(&a).status, "Added 1 label to the shelves");
    // The label goes on the cart as soon as it is made, and its face is built again with it,
    // so the shelf shows it without slot starting again.
    assert_eq!(
        a.face_cart(zero.0, zero.1).and_then(|c| c.label.clone()),
        Some(d.path().join("Labels").join("GBA").join("Zero.png"))
    );
    assert_eq!(a.take_relabelled(), [zero]);
    press(&mut a, Btn::Start);
    assert!(
        !a.restarting(),
        "START restarted, which labels no longer need"
    );
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

#[test]
fn a_screenscraper_login_is_typed_in_on_game_art_and_saved_to_the_card() {
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
        if a.quick_menu() == Some(QuickRow::GameArt) {
            break;
        }
        press(&mut a, Btn::Down);
    }
    press(&mut a, Btn::A);
    assert_eq!(detail(&a, "ScreenScraper"), "Sign in");
    // Up from the first row wraps to the last, the sign in.
    press(&mut a, Btn::Up);
    assert_eq!(action(&a), Some("Sign In".to_string()));
    press(&mut a, Btn::A);
    let Some(PageModel::Keys(_)) = a.page_model() else {
        panic!("no keyboard for the username: {:?}", a.phase());
    };
    for _ in 0..2 {
        press(&mut a, Btn::A);
    }
    press(&mut a, Btn::Start);
    let Some(PageModel::Keys(_)) = a.page_model() else {
        panic!("no keyboard for the password: {:?}", a.phase());
    };
    for _ in 0..4 {
        press(&mut a, Btn::A);
    }
    press(&mut a, Btn::Start);
    assert!(
        matches!(a.phase(), Phase::GameArt { .. }),
        "{:?}",
        a.phase()
    );
    let conf = std::fs::read_to_string(d.path().join(slot::scrape::SS_CONF)).unwrap();
    let user = conf
        .lines()
        .find_map(|l| l.strip_prefix("ssid="))
        .unwrap_or_default();
    assert_eq!(user.chars().count(), 2, "{conf}");
    assert_eq!(detail(&a, "ScreenScraper"), user);
    assert_eq!(
        list(&a).status,
        "Signed in, but ScreenScraper needs unslot's developer ID"
    );
}

#[test]
fn games_syncthing_brings_go_on_the_shelves_as_they_land() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let set = SlotState {
        clock_set: true,
        syncthing: true,
        password: Some("root".into()),
        ..SlotState::default()
    };
    write_slot_state(d.path(), &set).expect("write slot.state");
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    let seated = a.selected_stem().map(str::to_string);
    let gb = d.path().join("Games").join(Platform::Gb.dir_name());
    std::fs::create_dir_all(&gb).unwrap();
    std::fs::write(gb.join("Tetris.gb"), [0u8; 8]).unwrap();
    std::fs::write(d.path().join("Games/GBA/Advance Wars.gba"), [0u8; 8]).unwrap();
    // The pretend Syncthing is looked in on as often as the real one, every few seconds.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut now = 0;
    let landed = |a: &App| {
        ["Tetris", "Advance Wars"]
            .iter()
            .all(|s| a.carts().any(|c| c.stem == *s))
    };
    while !landed(&a) && Instant::now() < deadline {
        now += 16;
        a.tick_ms(now);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(landed(&a), "the new games never reached the shelves");
    assert_eq!(
        a.selected_stem().map(str::to_string),
        seated,
        "the shelf left the cart it was on"
    );
}

#[test]
fn the_sync_line_says_how_far_how_much_and_how_long() {
    use slot::app::sync_line;
    use slot::services::SyncProgress;
    let gb = 1u64 << 30;
    let line = |need, total, secs_left| {
        sync_line(SyncProgress {
            need,
            total,
            secs_left,
        })
    };
    assert_eq!(
        line(gb * 3 / 2, gb * 3, Some(2100)),
        "Syncing 50% · 1.5 GB left · 35 min"
    );
    assert_eq!(
        line(5 << 20, 10 << 20, Some(30)),
        "Syncing 50% · 5 MB left · under a minute"
    );
    assert_eq!(
        line(gb, gb * 10, Some(7500)),
        "Syncing 90% · 1.0 GB left · 2 h 5 min"
    );
    assert_eq!(
        line(1, 1 << 40, None),
        "Syncing 99% · 1 MB left",
        "never 100% with some to go"
    );
}

#[test]
fn the_cores_page_downloads_a_core_and_removes_it() {
    use slot_store::{Core, CORES_DIR};
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
        if a.quick_menu() == Some(QuickRow::Cores) {
            break;
        }
        press(&mut a, Btn::Down);
    }
    assert_eq!(a.quick_menu(), Some(QuickRow::Cores), "no Cores row");
    press(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::CoreDownloads { .. }),
        "{:?}",
        a.phase()
    );
    let n = Core::ALL.iter().filter(|c| c.download_only()).count();
    until(&mut a, |a| list(a).status == format!("0 of {n} downloaded"));
    assert_eq!(detail(&a, "VBA Next"), "GBA");
    assert_eq!(action(&a), Some("Download".to_string()));

    press(&mut a, Btn::A);
    until(&mut a, |a| detail(a, "VBA Next") == "Downloaded");
    let file = d.path().join(CORES_DIR).join(Core::VbaNext.file_name());
    assert!(file.is_file(), "the core is not in {CORES_DIR}");
    assert_eq!(list(&a).status, format!("1 of {n} downloaded"));
    assert_eq!(action(&a), None, "A still offers a core already downloaded");

    press(&mut a, Btn::X);
    until(&mut a, |a| detail(a, "VBA Next") == "GBA");
    assert!(!file.exists(), "X left the core on the card");

    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::Cores));
}
