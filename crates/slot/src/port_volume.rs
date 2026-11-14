//! slot's volume keys while it has quit for a port. A port plays through ALSA itself, which slot
//! cannot scale as it does its own sound or DraStic's, so the level the keys leave, slot's own,
//! kept in slot.state, goes on the codec's lineout volume instead, which BaseOS leaves at its
//! loudest. What the control was is put back when the port ends, for slot's own sound.

use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

use slot_input::{Gestures, InputSource, Millis};
use slot_store::{read_slot_state, write_slot_state};

use crate::drastic::Volume;

const CARD: &str = "audiocodec";
const CONTROL: &str = "name=lineout volume";
/// The control's steps: 1.5 dB each, the lowest -43.5 dB and the top 0 dB; 0 is silence.
const TOP: u8 = 31;
const DB_STEP: f32 = 1.5;
const HEADPHONES_EVERY: Duration = Duration::from_secs(1);

/// The control's step for slot's `level`: slot scales its sound by the level squared, which is
/// 40·log10(level / 100) dB, taken to the nearest step. Anything quieter than the lowest step is
/// the lowest step, and 0 is silence.
pub fn step(level: u8) -> u8 {
    if level == 0 {
        return 0;
    }
    let db = 40.0 * (f32::from(level.min(100)) / 100.0).log10();
    let below = (-db / DB_STEP).round() as i32;
    (i32::from(TOP) - below).clamp(1, i32::from(TOP)) as u8
}

fn read_control() -> Option<u8> {
    let out = Command::new("amixer")
        .args(["-c", CARD, "cget", CONTROL])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find_map(|l| l.trim().strip_prefix(": values="))?;
    line.split(',').next()?.trim().parse().ok()
}

fn set_control(step: u8) {
    let set = Command::new("amixer")
        .args(["-q", "-c", CARD, "cset", CONTROL, &step.to_string()])
        .status();
    match set {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("slot: port volume: amixer {s}"),
        Err(e) => eprintln!("slot: port volume: amixer: {e}"),
    }
}

/// Restores the mixer on every exit path, including a port that cannot start.
struct RestoreMixer(Option<u8>);

impl Drop for RestoreMixer {
    fn drop(&mut self) {
        if let Some(before) = self.0 {
            set_control(before);
        }
    }
}

/// Owns the port process, restoring the mixer before its caller starts slot again. Even a port
/// that exits immediately is reaped here; there is no background companion to become orphaned.
pub fn run(root: &Path, command: &mut Command) -> std::io::Result<ExitStatus> {
    use slot_power::Platform;
    let power = slot_power::DevicePlatform::new(root.to_path_buf());
    let mut input = crate::input::DeviceInput::open(root);
    let mut gestures = Gestures::new();
    let mut volume = Volume::new(read_slot_state(root), power.headphones());
    let _restore = RestoreMixer(read_control());
    let mut written = step(volume.output());
    set_control(written);
    let mut port = command.spawn()?;
    let start = Instant::now();
    let mut next_jack = start + HEADPHONES_EVERY;
    loop {
        if let Some(status) = port.try_wait()? {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(10));
        let now = start.elapsed().as_millis() as Millis;
        let mut actions = gestures.tick(now);
        for ev in input.poll(now) {
            actions.extend(gestures.feed(ev, now));
        }
        for a in actions {
            if volume.act(a, now) {
                if let Err(e) = write_slot_state(root, &volume.state) {
                    eprintln!("slot: port volume: {e}");
                }
            }
        }
        if Instant::now() >= next_jack {
            next_jack += HEADPHONES_EVERY;
            volume.headphones = power.headphones();
        }
        let want = step(volume.output());
        if want != written {
            written = want;
            set_control(written);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lineout_sounds_as_slot_would_at_the_level() {
        assert_eq!(step(100), TOP);
        // Half way is a quarter as loud, -12 dB: eight steps down.
        assert_eq!(step(50), TOP - 8);
        assert_eq!(step(0), 0);
    }

    #[test]
    fn levels_quieter_than_the_lowest_step_still_play() {
        assert_eq!(step(5), 1);
        assert_eq!(step(1), 1);
    }
}
