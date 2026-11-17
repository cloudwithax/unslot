/// How much sound the ring holds, in frames, for a device at `sample_rate`.
pub fn drc_target(capacity_frames: usize) -> usize {
    capacity_frames / 2
}

/// Holds the sound queue at its target by steering how much the sound is stretched: a queue
/// running low is played slower so the core doesn't run dry, and one running high is played
/// faster so the delay doesn't grow and nothing overflows.
///
/// Stretching moves the pitch, so it is kept to 1%, too little to hear, but for a queue about to
/// overflow (see `FULL_MIN`). A core that falls further
/// behind than that runs the queue dry instead: a short gap is gone at once, where a pitch dropped
/// to match it is heard for as long as the core is slow and glides back after.
///
/// The rate the sound arrives at is not used. A core whose hand-over is batched, or whose thread
/// waits when the queue is full, delivers unevenly without being slow, and measuring that would
/// slow the sound down just when the queue is already full.
pub struct Level {
    learned: f64,
}

impl Default for Level {
    fn default() -> Self {
        Level { learned: 0.0 }
    }
}

impl Level {
    pub fn new() -> Self {
        Self::default()
    }

    /// What the queue being off its target should move the rate by. Small, so the rate the sound
    /// is played at doesn't swing with the queue's own jitter.
    const PROPORTION: f64 = 0.05;

    /// How fast the part a core that never keeps its rate needs is learned.
    const LEARN: f64 = 0.12;

    /// How far the learned part may take the rate, leaving the rest for the queue's own jitter.
    const LEARNED_MAX: f64 = 0.005;
    const LEARNED_MIN: f64 = -0.005;

    /// The most the rate may be moved either way, so the pitch never moves enough to hear.
    pub const MIN: f64 = 0.99;
    pub const MAX: f64 = 1.01;

    /// How far it may be quickened once the queue is close to full. Past that the thread running
    /// the core waits for room and the next frame is late, which is seen, where this much pitch
    /// for the moment it takes to drain is hardly heard.
    pub const FULL_MIN: f64 = 0.95;
    /// How far over its target, as a part of it, the queue is when that starts and when it is all
    /// allowed.
    const FULL_FROM: f64 = 0.25;
    const FULL_BY: f64 = 0.75;

    /// Takes `dt` since the last update and the queue's level, and gives the rate to play at.
    pub fn push(&mut self, dt: f64, queued_frames: usize, target_frames: usize) -> f64 {
        if target_frames == 0 {
            return 1.0;
        }
        let error = (target_frames as f64 - queued_frames as f64) / target_frames as f64;
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        self.learned =
            (self.learned + Self::LEARN * error * dt).clamp(Self::LEARNED_MIN, Self::LEARNED_MAX);
        let full = ((-error - Self::FULL_FROM) / (Self::FULL_BY - Self::FULL_FROM)).clamp(0.0, 1.0);
        let min = Self::MIN - (Self::MIN - Self::FULL_MIN) * full;
        (1.0 + Self::PROPORTION * error + self.learned).clamp(min, Self::MAX)
    }

    /// Forgets the learned rate, for when the core stops running at its own pace.
    pub fn reset(&mut self) {
        self.learned = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A queue that rises by `produce` frames a step and drains `consume` frames a step.
    fn settle(level: &mut Level, start: usize, target: usize, produce: f64, consume: f64) -> usize {
        let frames = target * 2;
        let mut queued = start;
        let dt = 1.0 / 60.0;
        for _ in 0..600 {
            let rate = level.push(dt, queued, target);
            let added = produce * rate;
            queued = ((queued as f64 + added - consume).max(0.0) as usize).min(frames);
        }
        queued
    }

    #[test]
    fn a_core_at_the_device_rate_holds_the_queue() {
        let mut level = Level::new();
        let queued = settle(
            &mut level,
            3200,
            3200,
            800.0 * 48000.0 / 44100.0,
            800.0 * 48000.0 / 44100.0,
        );
        assert!(queued.abs_diff(3200) < 200, "settled at {queued}");
    }

    #[test]
    fn a_core_a_little_ahead_is_quickened_until_the_queue_holds() {
        let mut level = Level::new();
        let queued = settle(
            &mut level,
            3200,
            3200,
            1.003 * 800.0 * 48000.0 / 44100.0,
            800.0 * 48000.0 / 44100.0,
        );
        assert!(queued.abs_diff(3200) < 400, "settled at {queued}");
    }

    #[test]
    fn a_core_running_short_is_never_slowed_enough_to_hear() {
        let mut level = Level::new();
        let mut queued = 3200;
        for _ in 0..600 {
            let rate = level.push(1.0 / 60.0, queued, 3200);
            assert!(rate <= 1.01, "a short core was stretched to {rate}");
            queued = (queued as f64 + 0.67 * 800.0 * rate - 800.0).max(0.0) as usize;
        }
    }

    #[test]
    fn a_full_queue_is_drained_and_an_empty_one_filled() {
        let mut level = Level::new();
        assert!(level.push(1.0 / 60.0, 0, 3200) > 1.0);
        assert!(level.push(1.0 / 60.0, 6400, 3200) < 1.0);
    }

    #[test]
    fn a_core_well_ahead_is_quickened_before_the_queue_fills() {
        // 2% ahead, as after presents that missed their refresh: more than the 1% that is never
        // heard can take, and left alone the queue fills and holds the core's thread up.
        let mut level = Level::new();
        let produce = 1.02 * 800.0 * 48000.0 / 44100.0;
        let consume = 800.0 * 48000.0 / 44100.0;
        let mut queued = 3200usize;
        for _ in 0..1200 {
            let rate = level.push(1.0 / 60.0, queued, 3200);
            assert!(rate >= Level::FULL_MIN, "quickened to {rate}");
            queued = (queued as f64 + produce * rate - consume).max(0.0) as usize;
            assert!(queued + 900 < 6400, "no room left for a frame's sound at {queued}");
        }
    }

    #[test]
    fn a_queue_near_its_target_is_never_quickened_enough_to_hear() {
        let mut level = Level::new();
        for queued in [3200, 3600, 4000] {
            for _ in 0..600 {
                assert!(level.push(1.0 / 60.0, queued, 3200) >= Level::MIN);
            }
        }
    }

    #[test]
    fn a_queue_with_no_target_is_left_alone() {
        let mut level = Level::new();
        assert_eq!(level.push(1.0 / 60.0, 0, 0), 1.0);
    }

    #[test]
    fn the_rate_stays_within_what_the_resampler_can_take() {
        let mut level = Level::new();
        for queued in [0, 100, 3200, 6400, 100_000] {
            for _ in 0..100 {
                let rate = level.push(1.0 / 60.0, queued, 3200);
                assert!(
                    (Level::FULL_MIN..=Level::MAX).contains(&rate),
                    "rate {rate} out of bounds for queued={queued}"
                );
            }
        }
    }
}
