//! slot's volume keys while it has quit for a port. A port plays through ALSA itself, which slot
//! cannot scale as it does its own sound, so the level the keys leave, slot's own,
//! kept in slot.state, goes on the codec's lineout volume instead, which BaseOS leaves at its
//! loudest. What the control was is put back when the port ends, for slot's own sound.

use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

use slot_input::{Action, Gestures, InputSource, Millis, MUTE_CHORD_MS};
use slot_store::{read_slot_state, write_slot_state, SlotState, VOLUME_MAX};

use crate::app::VOLUME_STEP;

const CARD: &str = "audiocodec";
const CONTROL: &str = "name=lineout volume";
/// The control's steps: 1.5 dB each, the lowest -43.5 dB and the top 0 dB; 0 is silence.
const TOP: u8 = 31;
const DB_STEP: f32 = 1.5;
const HEADPHONES_EVERY: Duration = Duration::from_secs(1);

/// slot's volume and mute, for the speaker or the headphones, as the keys move them.
pub struct Volume {
    pub state: SlotState,
    pub headphones: bool,
    before: Vec<(u8, bool, Millis)>,
}

impl Volume {
    pub fn new(state: SlotState, headphones: bool) -> Self {
        Volume {
            state,
            headphones,
            before: Vec::new(),
        }
    }

    fn level_and_muted(&mut self) -> (&mut u8, &mut bool) {
        match self.headphones {
            true => (&mut self.state.volume_hp, &mut self.state.muted_hp),
            false => (&mut self.state.volume, &mut self.state.muted),
        }
    }

    /// The level the port plays at: nothing when muted.
    pub fn output(&mut self) -> u8 {
        let (level, muted) = self.level_and_muted();
        if *muted {
            0
        } else {
            *level
        }
    }

    /// Moves the volume as slot does for `action`, the mute chord putting back what its two
    /// presses moved; whether anything changed.
    pub fn act(&mut self, action: Action, now: Millis) -> bool {
        let before = self.state.clone();
        match action {
            Action::VolumeUp | Action::VolumeDown => {
                let (level, muted) = self.level_and_muted();
                let was = (*level, *muted);
                *level = match action {
                    Action::VolumeUp => level.saturating_add(VOLUME_STEP).min(VOLUME_MAX),
                    _ => level.saturating_sub(VOLUME_STEP),
                };
                *muted = false;
                if self.before.len() == 2 {
                    self.before.remove(0);
                }
                self.before.push((was.0, was.1, now));
            }
            Action::MuteToggle => {
                let undo = self
                    .before
                    .iter()
                    .find(|(_, _, at)| now.saturating_sub(*at) <= MUTE_CHORD_MS)
                    .copied();
                self.before.clear();
                let (level, muted) = self.level_and_muted();
                if let Some((was, was_muted, _)) = undo {
                    *level = was;
                    *muted = was_muted;
                }
                *muted = !*muted;
            }
            _ => return false,
        }
        self.state != before
    }
}

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

    #[test]
    fn the_keys_move_the_speakers_level_and_unmute_it() {
        let mut v = Volume::new(
            SlotState {
                volume: 50,
                muted: true,
                ..Default::default()
            },
            false,
        );
        assert_eq!(v.output(), 0);
        assert!(v.act(Action::VolumeUp, 0));
        assert_eq!((v.state.volume, v.state.muted, v.output()), (55, false, 55));
        assert!(v.act(Action::VolumeDown, 1000));
        assert_eq!(v.state.volume, 50);
    }

    #[test]
    fn the_headphones_keep_their_own_level() {
        let mut v = Volume::new(
            SlotState {
                volume: 50,
                volume_hp: 20,
                ..Default::default()
            },
            true,
        );
        v.act(Action::VolumeUp, 0);
        assert_eq!((v.state.volume, v.state.volume_hp), (50, 25));
    }

    #[test]
    fn the_mute_chord_mutes_where_the_volume_was() {
        let mut v = Volume::new(
            SlotState {
                volume: 50,
                ..Default::default()
            },
            false,
        );
        v.act(Action::VolumeUp, 1000);
        v.act(Action::VolumeDown, 1100);
        v.act(Action::MuteToggle, 1100);
        assert_eq!((v.state.volume, v.state.muted, v.output()), (50, true, 0));
        v.act(Action::MuteToggle, 5000);
        assert_eq!(v.output(), 50);
    }
}
