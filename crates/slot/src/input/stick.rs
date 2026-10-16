//! The analog sticks of the H700 handhelds that have them. Anbernic's own kernel reads them
//! through one ADC and a mux and reports them as four absolute axes on the same evdev node as
//! the buttons, so all slot does is find which axes, scale them to libretro's range and keep a
//! resting stick from drifting.

use slot_retro::Sticks;

use super::evdev::{has_bit, Ev, EV_ABS};

pub const ABS_Y: u16 = 0x01;
pub const ABS_Z: u16 = 0x02;
pub const ABS_RX: u16 = 0x03;
pub const ABS_RY: u16 = 0x04;
pub const ABS_RZ: u16 = 0x05;

/// How far from rest a stick has to go before it counts, as a share of its throw: 128 of the
/// 1800 ROCKNIX's driver reports, about 7%.
const DEADZONE: f32 = 0.07;

/// The axes that carry left x, left y, right x and right y on a node, from its `abs` capability
/// bitmap, or None for a node with no sticks.
///
/// Anbernic's stock kernel puts them on Z, RX, RY and RZ. A stickless board advertises RX, RY
/// and RZ too, which never move, but never Z. muOS's device tree moves the left stick to Y and Z
/// and has no RX, so that layout is told apart by it.
pub fn layout(abs: &str) -> Option<[u16; 4]> {
    let has = |code| has_bit(abs, code);
    if has(ABS_Z) && has(ABS_RX) && has(ABS_RY) && has(ABS_RZ) {
        return Some([ABS_Z, ABS_RX, ABS_RY, ABS_RZ]);
    }
    if has(ABS_Y) && has(ABS_Z) && !has(ABS_RX) && has(ABS_RY) && has(ABS_RZ) {
        return Some([ABS_Y, ABS_Z, ABS_RY, ABS_RZ]);
    }
    None
}

/// One axis's travel as the kernel reports it, and where it rests.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Range {
    pub min: i32,
    pub max: i32,
    pub centre: i32,
}

impl Range {
    /// The range the kernel gives, resting where the axis read when the node was opened. A
    /// reading far from the middle means the stick was held then, so the middle is used instead.
    pub fn new(min: i32, max: i32, value: i32) -> Range {
        let (min, max) = (min.min(max), min.max(max));
        let mid = min + (max - min) / 2;
        let centre = match (value - mid).abs() <= (max - min) / 8 {
            true => value,
            false => mid,
        };
        Range { min, max, centre }
    }

    /// The reading as a share of the way to either end, -1 to 1.
    fn share(self, value: i32) -> f32 {
        let off = value - self.centre;
        let reach = match off < 0 {
            true => self.centre - self.min,
            false => self.max - self.centre,
        };
        if reach <= 0 {
            return 0.0;
        }
        (off as f32 / reach as f32).clamp(-1.0, 1.0)
    }
}

/// The sticks on one node: which axes they are, how far each travels, and where each was last.
pub struct StickNode {
    axes: [u16; 4],
    ranges: [Range; 4],
    raw: [i32; 4],
    right: bool,
}

impl StickNode {
    /// `right` is false on a model with one stick, whose second pair of channels reads nothing.
    pub fn new(axes: [u16; 4], ranges: [Range; 4], right: bool) -> StickNode {
        StickNode {
            axes,
            ranges,
            raw: ranges.map(|r| r.centre),
            right,
        }
    }

    /// Takes the event if it moves one of the sticks.
    pub fn feed(&mut self, ev: Ev) -> bool {
        if ev.kind != EV_ABS {
            return false;
        }
        let Some(at) = self.axes.iter().position(|a| *a == ev.code) else {
            return false;
        };
        self.raw[at] = ev.value;
        true
    }

    /// Both sticks in libretro's terms: left x, left y, right x, right y, each -32767 to 32767
    /// with left and up negative.
    pub fn values(&self) -> Sticks {
        let share = |i: usize| self.ranges[i].share(self.raw[i]);
        let (lx, ly) = deadzone(share(0), share(1));
        let (rx, ry) = match self.right {
            true => deadzone(share(2), share(3)),
            false => (0.0, 0.0),
        };
        [lx, ly, rx, ry].map(|v| (v * f32::from(i16::MAX)).round() as i16)
    }
}

/// A radial deadzone: inside it the stick reads as rest, and past it the travel left is
/// stretched back out to the full throw, so the edge of the deadzone is not a jump.
pub fn deadzone(x: f32, y: f32) -> (f32, f32) {
    let m = x.hypot(y);
    if m <= DEADZONE {
        return (0.0, 0.0);
    }
    let k = ((m - DEADZONE) / (1.0 - DEADZONE)).min(1.0) / m;
    (x * k, y * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: u16, value: i32) -> Ev {
        Ev {
            kind: EV_ABS,
            code,
            value,
        }
    }

    #[test]
    fn the_stock_layout_is_read_off_a_stick_models_bitmap() {
        // An RG34XXSP: Z, RX, RY, RZ and the hat.
        assert_eq!(layout("3003c"), Some([ABS_Z, ABS_RX, ABS_RY, ABS_RZ]));
    }

    #[test]
    fn a_stickless_board_that_still_advertises_rx_ry_and_rz_has_no_sticks() {
        // An RG SP: RX, RY, RZ and the hat, but no Z.
        assert_eq!(layout("30038"), None);
        assert_eq!(layout("30000"), None, "a hat alone is not a stick");
    }

    #[test]
    fn muoss_device_tree_layout_is_told_apart_by_its_missing_rx() {
        assert_eq!(layout("30036"), Some([ABS_Y, ABS_Z, ABS_RY, ABS_RZ]));
    }

    #[test]
    fn a_held_stick_at_open_does_not_become_its_rest() {
        assert_eq!(
            Range::new(-4096, 4096, 30).centre,
            30,
            "a small offset is the rest"
        );
        assert_eq!(
            Range::new(-4096, 4096, 3000).centre,
            0,
            "a held stick is not"
        );
        assert_eq!(Range::new(0, 4095, 2100).centre, 2100);
    }

    #[test]
    fn full_travel_either_way_reads_as_full_scale_around_an_off_centre_rest() {
        let ranges = [Range::new(0, 4000, 2200); 4];
        let mut s = StickNode::new([ABS_Z, ABS_RX, ABS_RY, ABS_RZ], ranges, true);
        assert_eq!(s.values(), [0; 4], "a resting stick moved");
        assert!(s.feed(ev(ABS_Z, 0)));
        assert!(s.feed(ev(ABS_RX, 4000)));
        let [lx, ly, ..] = s.values();
        // Pushed to a corner, each axis reads its share of the radial throw.
        assert!(lx < -20_000 && ly > 20_000, "{lx} {ly}");
        assert!(s.feed(ev(ABS_RX, 2200)));
        assert_eq!(s.values()[0], -i16::MAX, "full left is not full scale");
    }

    #[test]
    fn a_stick_resting_a_little_off_centre_reads_as_rest() {
        let ranges = [Range::new(-1800, 1800, 0); 4];
        let mut s = StickNode::new([ABS_Z, ABS_RX, ABS_RY, ABS_RZ], ranges, true);
        s.feed(ev(ABS_Z, 90));
        s.feed(ev(ABS_RX, -60));
        assert_eq!(
            s.values(),
            [0; 4],
            "drift inside the deadzone reached the game"
        );
    }

    #[test]
    fn the_deadzones_edge_is_not_a_jump() {
        let (x, _) = deadzone(DEADZONE + 0.001, 0.0);
        assert!(x > 0.0 && x < 0.01, "{x}");
        assert_eq!(deadzone(1.0, 0.0), (1.0, 0.0));
    }

    #[test]
    fn a_one_stick_model_keeps_its_right_stick_at_rest() {
        let ranges = [Range::new(-1800, 1800, 0); 4];
        let mut s = StickNode::new([ABS_Z, ABS_RX, ABS_RY, ABS_RZ], ranges, false);
        s.feed(ev(ABS_RY, 1800));
        s.feed(ev(ABS_RZ, -1800));
        assert_eq!(s.values()[2..], [0, 0]);
    }

    #[test]
    fn events_that_are_not_the_sticks_are_left_for_the_hat_and_buttons() {
        let ranges = [Range::new(-1800, 1800, 0); 4];
        let mut s = StickNode::new([ABS_Z, ABS_RX, ABS_RY, ABS_RZ], ranges, true);
        assert!(!s.feed(ev(0x10, 1)), "the hat was taken as a stick");
        assert!(!s.feed(Ev {
            kind: super::super::evdev::EV_KEY,
            code: ABS_Z,
            value: 1
        }));
    }
}
