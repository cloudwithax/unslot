use std::fs;
use std::path::Path;

use slot::input::evdev::{decode, has_bit, pick_devices, to_raw, EVENT_BYTES};
use slot_input::{Btn, InputSource, RawEvent};

fn packet(kind: u16, code: u16, value: i32) -> [u8; EVENT_BYTES] {
    let mut b = [0x11u8; EVENT_BYTES];
    b[16] = kind as u8;
    b[17] = (kind >> 8) as u8;
    b[18] = code as u8;
    b[19] = (code >> 8) as u8;
    b[20..24].copy_from_slice(&value.to_le_bytes());
    b
}

const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const EV_SW: u16 = 0x05;
const EV_SYN: u16 = 0x00;
const CODE_R1: u16 = 0x135;
const KEY_VOLUMEUP: u16 = 115;
const SW_LID: u16 = 0x00;

#[test]
fn a_key_press_decodes_from_the_evdev_byte_layout() {
    let down = decode(&packet(EV_KEY, CODE_R1, 1)).and_then(to_raw);
    assert_eq!(down, Some(RawEvent::Down(Btn::R1)));
    let up = decode(&packet(EV_KEY, CODE_R1, 0)).and_then(to_raw);
    assert_eq!(up, Some(RawEvent::Up(Btn::R1)));
    let vol = decode(&packet(EV_KEY, KEY_VOLUMEUP, 1)).and_then(to_raw);
    assert_eq!(vol, Some(RawEvent::Down(Btn::VolUp)));
}

#[test]
fn autorepeat_is_not_a_press() {
    assert_eq!(decode(&packet(EV_KEY, CODE_R1, 2)).and_then(to_raw), None);
}

#[test]
fn the_lid_arrives_as_a_switch_rather_than_a_key() {
    let shut = decode(&packet(EV_SW, SW_LID, 1)).and_then(to_raw);
    assert_eq!(shut, Some(RawEvent::Down(Btn::Lid)));
    let open = decode(&packet(EV_SW, SW_LID, 0)).and_then(to_raw);
    assert_eq!(open, Some(RawEvent::Up(Btn::Lid)));
    assert_ne!(
        decode(&packet(EV_KEY, SW_LID, 1)).and_then(to_raw),
        Some(RawEvent::Down(Btn::Lid))
    );
}

#[test]
fn frame_markers_and_unmapped_codes_are_dropped() {
    assert_eq!(decode(&packet(EV_SYN, 0, 0)).and_then(to_raw), None);
    assert_eq!(decode(&packet(EV_KEY, 0x2ff, 1)).and_then(to_raw), None);
}

#[test]
fn a_short_read_decodes_to_nothing_rather_than_panicking() {
    assert!(decode(&[0u8; EVENT_BYTES - 1]).is_none());
    assert!(decode(&[]).is_none());
}

#[test]
fn a_capability_bitmask_is_indexed_from_the_last_word() {
    assert!(has_bit("10 0", 68));
    assert!(!has_bit("10 0", 4));
    assert!(has_bit("8", 3));
    assert!(!has_bit("", 0));
    assert!(!has_bit("0 0 0", 300));
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn input_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let (dev, sys) = (d.path().join("dev/input"), d.path().join("sys/class/input"));
    for node in ["event0", "event1", "event2", "mice"] {
        write(&dev.join(node), "");
    }
    write(&sys.join("event0/device/name"), "axp2202-pek\n");
    write(&sys.join("event0/device/capabilities/key"), "0\n");
    write(&sys.join("event1/device/name"), "gpio-keys\n");
    write(
        &sys.join("event1/device/capabilities/key"),
        "1000000000000 0 0 0 0\n",
    );
    write(&sys.join("event2/device/name"), "gpio-keys-lid\n");
    write(&sys.join("event2/device/capabilities/key"), "0\n");
    write(&sys.join("event2/device/capabilities/sw"), "1\n");
    d
}

#[test]
fn devices_are_picked_by_capability_and_never_by_position() {
    let d = input_tree();
    let picked = pick_devices(
        &d.path().join("dev/input"),
        &d.path().join("sys/class/input"),
    );
    let names: Vec<_> = picked
        .iter()
        .filter_map(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["event1", "event2"]);
}

#[test]
fn an_input_node_with_no_sysfs_entry_is_skipped_rather_than_opened() {
    let d = tempfile::tempdir().unwrap();
    write(&d.path().join("dev/input/event0"), "");
    let picked = pick_devices(
        &d.path().join("dev/input"),
        &d.path().join("sys/class/input"),
    );
    assert!(picked.is_empty(), "{picked:?}");
}

#[test]
fn the_survey_reports_the_nodes_that_were_not_opened_too() {
    let d = input_tree();
    let lines = slot::input::trace::survey(
        &d.path().join("dev/input"),
        &d.path().join("sys/class/input"),
    );
    let line = |node: &str| {
        lines
            .iter()
            .find(|l| l.contains(node))
            .unwrap_or_else(|| panic!("{node} was left out of the survey: {lines:?}"))
            .clone()
    };
    assert!(line("event0").contains("skipped"), "{}", line("event0"));
    assert!(
        line("event0").contains("axp2202-pek"),
        "the survey did not name the node: {}",
        line("event0")
    );
    assert!(line("event1").contains("opened"), "{}", line("event1"));
    assert!(line("event2").contains("opened"), "{}", line("event2"));
}

#[test]
fn an_unmapped_code_is_traced_by_number_rather_than_dropped() {
    let ev = |code| slot::input::evdev::Ev {
        kind: EV_KEY,
        code,
        value: 1,
    };
    let mapped = slot::input::trace::event_line("event1", 12, ev(CODE_R1));
    assert!(mapped.contains("R1"), "{mapped}");
    let unmapped = slot::input::trace::event_line("event1", 13, ev(0x2ff));
    assert!(unmapped.contains("0x2ff"), "{unmapped}");
    assert!(
        unmapped.contains("unmapped"),
        "an unknown code has to say so, or it looks like nothing arrived: {unmapped}"
    );
}

#[test]
fn a_trace_writes_the_survey_to_the_card() {
    let d = input_tree();
    let card = tempfile::tempdir().unwrap();
    let input = slot::input::DeviceInput::open_in(
        &d.path().join("dev/input"),
        &d.path().join("sys/class/input"),
        card.path(),
        true,
    );
    drop(input);
    let log = std::fs::read_to_string(card.path().join("Logs/input-trace.log"))
        .expect("no trace file reached the card");
    assert!(log.contains("event0 skipped"), "{log}");
    assert!(log.contains("event1 opened"), "{log}");
    assert!(log.contains("gpio-keys-lid"), "{log}");
}

#[test]
fn no_trace_file_is_written_unless_one_was_asked_for() {
    let d = input_tree();
    let card = tempfile::tempdir().unwrap();
    let input = slot::input::DeviceInput::open_in(
        &d.path().join("dev/input"),
        &d.path().join("sys/class/input"),
        card.path(),
        false,
    );
    drop(input);
    assert!(!card.path().join("Logs/input-trace.log").exists());
}

#[test]
fn the_pad_is_polled_as_fast_as_the_kernel_tick_allows() {
    let d = input_tree();
    let sys = d.path().join("sys/class/input");
    write(&sys.join("event1/device/poll"), "20\n");
    write(&sys.join("event0/device/poll"), "20\n");
    let card = tempfile::tempdir().unwrap();
    let input =
        slot::input::DeviceInput::open_in(&d.path().join("dev/input"), &sys, card.path(), false);
    drop(input);
    let poll = |node: &str| std::fs::read_to_string(sys.join(node).join("device/poll")).unwrap();
    assert_eq!(
        poll("event1").trim(),
        "10",
        "the pad is still polled at 20 ms"
    );
    assert_eq!(
        poll("event0").trim(),
        "20",
        "a node slot does not read was changed"
    );
}

#[test]
fn a_pad_already_polled_faster_is_left_alone() {
    let d = input_tree();
    let sys = d.path().join("sys/class/input");
    write(&sys.join("event1/device/poll"), "5\n");
    let card = tempfile::tempdir().unwrap();
    drop(slot::input::DeviceInput::open_in(
        &d.path().join("dev/input"),
        &sys,
        card.path(),
        false,
    ));
    let poll = std::fs::read_to_string(sys.join("event1/device/poll")).unwrap();
    assert_eq!(poll.trim(), "5");
}

#[test]
fn the_pad_maps_by_the_codes_the_device_sends() {
    let expected = [
        (0x130, Btn::A),
        (0x131, Btn::B),
        (0x132, Btn::Y),
        (0x133, Btn::X),
        (0x134, Btn::L1),
        (0x135, Btn::R1),
        (0x136, Btn::Select),
        (0x137, Btn::Start),
        (0x138, Btn::Menu),
        (0x13a, Btn::L2),
        (0x13b, Btn::R2),
        (115, Btn::VolUp),
        (114, Btn::VolDown),
        (116, Btn::Power),
    ];
    for (code, want) in expected {
        assert_eq!(
            slot::input::evdev::code_to_btn(code),
            Some(want),
            "code {code:#x}"
        );
    }
}

#[test]
fn a_bluetooth_pad_maps_its_buttons_by_their_xbox_letters() {
    let expected = [
        (0x130, Btn::A),
        (0x131, Btn::B),
        (0x133, Btn::X),
        (0x134, Btn::Y),
        (0x136, Btn::L1),
        (0x137, Btn::R1),
        (0x13a, Btn::Select),
        (0x13b, Btn::Start),
        (0x13c, Btn::Menu),
        (0x13d, Btn::L3),
        (0x13e, Btn::R3),
    ];
    for (code, want) in expected {
        assert_eq!(
            decode(&packet(EV_KEY, code, 1)).and_then(slot::input::evdev::pad_to_raw),
            Some(RawEvent::Down(want)),
            "code {code:#x}"
        );
    }
}

#[test]
fn a_pads_analog_triggers_are_l2_and_r2_past_half_their_pull() {
    let mut triggers = slot::input::evdev::Triggers::new([1023, 1023]);
    let ev = |code, value| slot::input::evdev::Ev {
        kind: slot::input::evdev::EV_ABS,
        code,
        value,
    };
    assert_eq!(triggers.feed(ev(0x0a, 200)), None);
    assert_eq!(triggers.feed(ev(0x0a, 900)), Some(RawEvent::Down(Btn::L2)));
    assert_eq!(triggers.feed(ev(0x0a, 1023)), None);
    assert_eq!(triggers.feed(ev(0x09, 1023)), Some(RawEvent::Down(Btn::R2)));
    assert_eq!(triggers.feed(ev(0x0a, 0)), Some(RawEvent::Up(Btn::L2)));
    assert_eq!(triggers.feed(ev(0x00, 1023)), None);
}

#[test]
fn the_menu_buttons_second_code_is_swallowed() {
    assert_eq!(slot::input::evdev::code_to_btn(0x162), None);
}

#[test]
fn the_d_pad_arrives_as_a_hat_rather_than_as_keys() {
    let mut hat = slot::input::evdev::Hat::default();
    let ev = |code, value| slot::input::evdev::Ev {
        kind: slot::input::evdev::EV_ABS,
        code,
        value,
    };
    assert_eq!(hat.feed(ev(0x11, -1)), vec![RawEvent::Down(Btn::Up)]);
    assert_eq!(hat.feed(ev(0x11, 0)), vec![RawEvent::Up(Btn::Up)]);
    assert_eq!(hat.feed(ev(0x11, 1)), vec![RawEvent::Down(Btn::Down)]);
    assert_eq!(hat.feed(ev(0x11, 0)), vec![RawEvent::Up(Btn::Down)]);
    assert_eq!(hat.feed(ev(0x10, -1)), vec![RawEvent::Down(Btn::Left)]);
    assert_eq!(hat.feed(ev(0x10, 0)), vec![RawEvent::Up(Btn::Left)]);
    assert_eq!(hat.feed(ev(0x10, 1)), vec![RawEvent::Down(Btn::Right)]);
    assert_eq!(hat.feed(ev(0x10, 0)), vec![RawEvent::Up(Btn::Right)]);
}

#[test]
fn a_hat_swung_end_to_end_releases_before_it_presses() {
    let mut hat = slot::input::evdev::Hat::default();
    let ev = |code, value| slot::input::evdev::Ev {
        kind: slot::input::evdev::EV_ABS,
        code,
        value,
    };
    assert_eq!(hat.feed(ev(0x10, -1)), vec![RawEvent::Down(Btn::Left)]);
    assert_eq!(
        hat.feed(ev(0x10, 1)),
        vec![RawEvent::Up(Btn::Left), RawEvent::Down(Btn::Right)]
    );
    assert!(hat.feed(ev(0x10, 1)).is_empty());
}

#[test]
fn the_node_carrying_only_the_power_key_is_opened() {
    let d = tempfile::tempdir().unwrap();
    let (dev, sys) = (d.path().join("dev/input"), d.path().join("sys/class/input"));
    write(&dev.join("event0"), "");
    write(&sys.join("event0/device/name"), "axp2202-pek\n");
    write(
        &sys.join("event0/device/capabilities/key"),
        "10000000000000 0\n",
    );
    assert_eq!(
        pick_devices(&dev, &sys).len(),
        1,
        "the power node was skipped"
    );
}

#[test]
fn a_hat_axis_is_traced_as_the_axis_it_is() {
    let line = slot::input::trace::event_line(
        "event1",
        9,
        slot::input::evdev::Ev {
            kind: slot::input::evdev::EV_ABS,
            code: 0x11,
            value: -1,
        },
    );
    assert!(line.contains("hat0y"), "{line}");
    assert!(!line.contains("ignored"), "{line}");
}

#[test]
fn a_stick_models_sticks_and_clicks_come_off_the_button_node() {
    let d = input_tree();
    let (dev, sys) = (d.path().join("dev/input"), d.path().join("sys/class/input"));
    // An RG34XXSP: Z, RX, RY, RZ and the hat on the same node as the buttons.
    write(&sys.join("event1/device/capabilities/abs"), "3003c\n");
    let (abs_z, abs_rx, abs_ry) = (0x02, 0x03, 0x04);
    let mut stream = Vec::new();
    for (kind, code, value) in [
        (EV_ABS, abs_z, -1800),
        (EV_ABS, abs_rx, 0),
        (EV_ABS, abs_ry, 1800),
        (EV_KEY, 0x139, 1),
    ] {
        stream.extend_from_slice(&packet(kind, code, value));
    }
    fs::write(dev.join("event1"), stream).unwrap();

    let card = tempfile::tempdir().unwrap();
    let mut input = slot::input::DeviceInput::open_in(&dev, &sys, card.path(), false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut events = Vec::new();
    while std::time::Instant::now() < deadline
        && (input.sticks() == [0; 4] || !events.contains(&RawEvent::Down(Btn::L3)))
    {
        events.extend(input.poll(0));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        input.sticks(),
        [-i16::MAX, 0, i16::MAX, 0],
        "full left and full right did not reach full scale"
    );
    assert!(
        events.contains(&RawEvent::Down(Btn::L3)),
        "the left stick's click was lost: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, RawEvent::Down(Btn::Left | Btn::Right))),
        "a stick axis was read as the d-pad: {events:?}"
    );
}
