use std::path::{Path, PathBuf};

use slot_input::{Btn, RawEvent};

pub const EVENT_BYTES: usize = 24;

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_SW: u16 = 0x05;
pub const SW_LID: u16 = 0x00;

pub const EV_ABS: u16 = 0x03;
pub const ABS_HAT0X: u16 = 0x10;
pub const ABS_HAT0Y: u16 = 0x11;
pub const ABS_GAS: u16 = 0x09;
pub const ABS_BRAKE: u16 = 0x0a;

const BUS_USB: u16 = 0x03;
const BUS_BLUETOOTH: u16 = 0x05;

#[derive(Default)]
pub struct Hat {
    x: i32,
    y: i32,
}

impl Hat {
    pub fn feed(&mut self, ev: Ev) -> Vec<RawEvent> {
        let (held, ends) = match ev.code {
            ABS_HAT0X => (&mut self.x, [Btn::Left, Btn::Right]),
            ABS_HAT0Y => (&mut self.y, [Btn::Up, Btn::Down]),
            _ => return Vec::new(),
        };
        let (was, now) = (*held, ev.value.signum());
        if now == was {
            return Vec::new();
        }
        *held = now;
        let btn = |v: i32| ends[usize::from(v > 0)];
        let mut out = Vec::new();
        if was != 0 {
            out.push(RawEvent::Up(btn(was)));
        }
        if now != 0 {
            out.push(RawEvent::Down(btn(now)));
        }
        out
    }
}

/// A pad's analog triggers, LT on brake and RT on gas as the Xbox Wireless Controller has them,
/// as L2 and R2, held past half their pull.
pub struct Triggers {
    half: [i32; 2],
    held: [bool; 2],
}

impl Triggers {
    /// From how far each pulls: brake's then gas's.
    pub fn new(max: [i32; 2]) -> Triggers {
        Triggers {
            half: max.map(|m| m / 2),
            held: [false; 2],
        }
    }

    pub fn feed(&mut self, ev: Ev) -> Option<RawEvent> {
        let (i, btn) = match ev.code {
            ABS_BRAKE => (0, Btn::L2),
            ABS_GAS => (1, Btn::R2),
            _ => return None,
        };
        let now = ev.value > self.half[i];
        if now == self.held[i] {
            return None;
        }
        self.held[i] = now;
        Some(if now { RawEvent::Down(btn) } else { RawEvent::Up(btn) })
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Ev {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

pub fn decode(bytes: &[u8]) -> Option<Ev> {
    let packet: &[u8; EVENT_BYTES] = bytes.get(..EVENT_BYTES)?.try_into().ok()?;
    Some(Ev {
        kind: u16::from_le_bytes([packet[16], packet[17]]),
        code: u16::from_le_bytes([packet[18], packet[19]]),
        value: i32::from_le_bytes([packet[20], packet[21], packet[22], packet[23]]),
    })
}

pub fn to_raw(ev: Ev) -> Option<RawEvent> {
    let btn = match ev.kind {
        EV_KEY => code_to_btn(ev.code)?,
        EV_SW if ev.code == SW_LID => Btn::Lid,
        _ => return None,
    };
    match ev.value {
        0 => Some(RawEvent::Up(btn)),
        1 => Some(RawEvent::Down(btn)),
        _ => None,
    }
}

pub fn code_to_btn(code: u16) -> Option<Btn> {
    Some(match code {
        0x130 => Btn::A,
        0x131 => Btn::B,
        0x132 => Btn::Y,
        0x133 => Btn::X,
        0x134 => Btn::L1,
        0x135 => Btn::R1,
        0x136 => Btn::Select,
        0x137 => Btn::Start,
        0x138 => Btn::Menu,
        0x13a => Btn::L2,
        0x13b => Btn::R2,
        // The stick clicks on the stick models, under codes the kernel header names otherwise.
        0x139 => Btn::L3,
        0x13c => Btn::R3,
        115 => Btn::VolUp,
        114 => Btn::VolDown,
        116 => Btn::Power,
        _ => return None,
    })
}

/// `ev` as a button of a Bluetooth or USB pad.
pub fn pad_to_raw(ev: Ev) -> Option<RawEvent> {
    if ev.kind != EV_KEY {
        return None;
    }
    let btn = pad_code_to_btn(ev.code)?;
    match ev.value {
        0 => Some(RawEvent::Up(btn)),
        1 => Some(RawEvent::Down(btn)),
        _ => None,
    }
}

/// A Bluetooth or USB pad's button, by the codes Linux gives a gamepad. The face buttons go by
/// the letters an Xbox pad prints on them, so its bottom button is A.
pub fn pad_code_to_btn(code: u16) -> Option<Btn> {
    Some(match code {
        0x130 => Btn::A,
        0x131 => Btn::B,
        0x133 => Btn::X,
        0x134 => Btn::Y,
        0x136 => Btn::L1,
        0x137 => Btn::R1,
        0x138 => Btn::L2,
        0x139 => Btn::R2,
        0x13a => Btn::Select,
        0x13b => Btn::Start,
        0x13c => Btn::Menu,
        0x13d => Btn::L3,
        0x13e => Btn::R3,
        _ => return None,
    })
}

/// Whether `node` is a pad paired or plugged in, not the handheld's own buttons, which are on
/// the host bus.
pub fn is_pad(sys: &Path, node: &Path) -> bool {
    node.file_name()
        .and_then(|n| std::fs::read_to_string(sys.join(n).join("device/id/bustype")).ok())
        .and_then(|bus| u16::from_str_radix(bus.trim(), 16).ok())
        .is_some_and(|bus| bus == BUS_USB || bus == BUS_BLUETOOTH)
}

const WANTED: [u16; 15] = [
    0x130, 0x131, 0x132, 0x133, 0x134, 0x135, 0x136, 0x137, 0x138, 0x13a, 0x13b, 0x162, 115, 114,
    116,
];

pub use slot_power::has_bit;

pub fn pick_devices(dev: &Path, sys: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dev) else {
        return Vec::new();
    };
    let mut nodes: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("event"))
        })
        .collect();
    nodes.sort();
    nodes.retain(|node| {
        let Some(name) = node.file_name() else {
            return false;
        };
        wanted_node(&sys.join(name).join("device"))
    });
    nodes
}

fn wanted_node(device: &Path) -> bool {
    let cap = |file: &str| std::fs::read_to_string(device.join("capabilities").join(file));
    let keys = cap("key").unwrap_or_default();
    let lid = cap("sw").is_ok_and(|sw| has_bit(&sw, SW_LID));
    lid || WANTED.iter().any(|bit| has_bit(&keys, *bit))
}

pub fn device_name(sys: &Path, node: &Path) -> String {
    node.file_name()
        .and_then(|n| std::fs::read_to_string(sys.join(n).join("device/name")).ok())
        .map(|n| n.trim().to_string())
        .unwrap_or_else(|| node.display().to_string())
}
