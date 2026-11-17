//! The A133P's CPU thermal trip while a Dreamcast game runs.
//!
//! TrimUI's firmware starts throttling the CPUs at 63 °C, the passive trip of the CPU thermal zone,
//! stepping them down to 1.2 GHz, and a Brick Pro sits near 51 °C at the shelf. Over two minutes
//! of a Crazy Taxi 2 drive the CPUs then averaged about 1450 MHz and 57% of the frames ran over
//! 16.7 ms; with the trip at 70 °C, 1730 MHz and 34%; at 75 °C, 1918 MHz and 25%, peaking at 77 °C.
//! While Nodosumi runs, that trip goes up to [`RAISED_C`], well under the 90 °C critical trip,
//! which is never touched, and comes back when the game ends. The value it had is kept in `/run`
//! first, so a slot that went down mid-game puts it back when it next starts.
//! `System/dc-thermal-stock.on` on the card leaves the firmware's trip alone.

use std::path::{Path, PathBuf};

use slot_store::Core;

/// The passive trip while a Dreamcast game runs, in degrees.
pub const RAISED_C: i64 = 75;
/// The opt-out, under the card's System.
pub const STOCK_FLAG: &str = "System/dc-thermal-stock.on";
const MARK: &str = "/run/slot-thermal-trip";
const THERMAL: &str = "/sys/class/thermal";

/// The trip that throttles the CPUs: the CPU zone's highest passive trip, with the zone's
/// critical one, both in millidegrees, from a zone's `trip_point_N_type`/`_temp` pairs.
pub fn passive_trip(trips: &[(String, i64)]) -> Option<(usize, i64, i64)> {
    let critical = trips
        .iter()
        .filter(|(t, _)| t == "critical")
        .map(|&(_, c)| c)
        .min()?;
    let (n, (_, temp)) = trips
        .iter()
        .enumerate()
        .filter(|(_, (t, c))| t == "passive" && *c < critical)
        .max_by_key(|(_, (_, c))| *c)?;
    Some((n, *temp, critical))
}

/// The temperature to set, in millidegrees, if raising the trip is called for at all: never
/// lower than it was and never within 10 °C of the critical trip.
pub fn raised(was: i64, critical: i64) -> Option<i64> {
    let to = (RAISED_C * 1000).min(critical - 10_000);
    (to > was).then_some(to)
}

fn cpu_zone() -> Option<PathBuf> {
    std::fs::read_dir(THERMAL).ok()?.flatten().map(|e| e.path()).find(|p| {
        std::fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "cpu_thermal_zone")
    })
}

fn trips(zone: &Path) -> Vec<(String, i64)> {
    (0..)
        .map_while(|n| {
            let kind = std::fs::read_to_string(zone.join(format!("trip_point_{n}_type"))).ok()?;
            let temp = std::fs::read_to_string(zone.join(format!("trip_point_{n}_temp"))).ok()?;
            Some((kind.trim().to_string(), temp.trim().parse().unwrap_or(i64::MAX)))
        })
        .collect()
}

/// Puts the trip a slot that went down mid-game left raised back as it was.
pub fn restore_left() {
    let Ok(mark) = std::fs::read_to_string(MARK) else {
        return;
    };
    if let Some((path, was)) = mark.trim().split_once(' ') {
        match std::fs::write(path, was) {
            Ok(()) => eprintln!("slot: thermal: {path} back to {was}, left raised by the last slot"),
            Err(e) => eprintln!("slot: thermal: {path} stays raised: {e}"),
        }
    }
    let _ = std::fs::remove_file(MARK);
}

/// The trip raised for a game, back as it was when this is dropped.
pub struct Raised {
    path: PathBuf,
    was: i64,
}

impl Drop for Raised {
    fn drop(&mut self) {
        if let Err(e) = std::fs::write(&self.path, self.was.to_string()) {
            eprintln!("slot: thermal: {} stays raised: {e}", self.path.display());
            return;
        }
        let _ = std::fs::remove_file(MARK);
    }
}

/// Raises the CPU zone's passive trip while `core` runs, on the A133P, for Nodosumi, unless the
/// card has the opt-out flag.
pub fn raise(root: &Path, core: Core) -> Option<Raised> {
    if crate::pak::CHIP != "a133p" || core != Core::Nodosumi || root.join(STOCK_FLAG).is_file() {
        return None;
    }
    restore_left();
    let zone = cpu_zone()?;
    let (n, was, critical) = passive_trip(&trips(&zone))?;
    let to = raised(was, critical)?;
    let path = zone.join(format!("trip_point_{n}_temp"));
    std::fs::write(MARK, format!("{} {was}", path.display())).ok()?;
    match std::fs::write(&path, to.to_string()) {
        Ok(()) => {
            eprintln!("slot: thermal: CPU throttling from {} °C, not {} °C, while the game runs", to / 1000, was / 1000);
            Some(Raised { path, was })
        }
        Err(e) => {
            eprintln!("slot: thermal: the CPU's trip stays at {} °C: {e}", was / 1000);
            let _ = std::fs::remove_file(MARK);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brick_pro() -> Vec<(String, i64)> {
        vec![
            ("passive".into(), 58_000),
            ("passive".into(), 63_000),
            ("critical".into(), 90_000),
        ]
    }

    #[test]
    fn the_throttling_trip_is_the_highest_passive_one_below_critical() {
        assert_eq!(passive_trip(&brick_pro()), Some((1, 63_000, 90_000)));
        assert_eq!(passive_trip(&[("passive".into(), 63_000)]), None);
    }

    #[test]
    fn the_trip_only_goes_up_and_keeps_ten_degrees_under_critical() {
        assert_eq!(raised(63_000, 90_000), Some(RAISED_C * 1000));
        assert_eq!(raised(80_000, 90_000), None);
        assert_eq!(raised(63_000, 75_000), Some(65_000));
        assert_eq!(raised(63_000, 70_000), None);
    }
}
