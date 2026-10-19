
/// How much sound the ring holds, in frames, for a device at `sample_rate`.
pub fn drc_target(capacity_frames: usize) -> usize {
    capacity_frames / 2
}

/// Holds the sound queue at its target by steering how much the sound is stretched: a queue
/// running low is played slower (at the same pitch) so the core doesn't run dry, and one running
/// high is played faster so the delay doesn't grow and nothing overflows.
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

    /// How far the learned part may take the rate; a fifth short or a tenth ahead.
    const LEARNED_MAX: f64 = 0.22;
    const LEARNED_MIN: f64 = -0.12;

    /// The most the rate may be moved either way, before the resampler takes over.
    const MIN: f64 = 0.5;
    const MAX: f64 = 1.5;

    /// Takes `dt` since the last update and the queue's level, and gives the rate to play at.
    pub fn push(&mut self, dt: f64, queued_frames: usize, target_frames: usize) -> f64 {
        if target_frames == 0 {
            return 1.0;
        }
        let error = (target_frames as f64 - queued_frames as f64) / target_frames as f64;
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.1) } else { 0.0 };
        self.learned = (self.learned + Self::LEARN * error * dt)
            .clamp(Self::LEARNED_MIN, Self::LEARNED_MAX);
        (1.0 + Self::PROPORTION * error + self.learned).clamp(Self::MIN, Self::MAX)
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
        let queued = settle(&mut level, 3200, 3200, 800.0 * 48000.0 / 44100.0, 800.0 * 48000.0 / 44100.0);
        assert!(queued.abs_diff(3200) < 200, "settled at {queued}");
    }

    #[test]
    fn a_core_running_short_is_slowed_until_the_queue_holds() {
        let mut level = Level::new();
        let queued = settle(&mut level, 3200, 3200, 0.9 * 800.0 * 48000.0 / 44100.0, 800.0 * 48000.0 / 44100.0);
        assert!(queued.abs_diff(3200) < 400, "settled at {queued}");
        let rate = level.push(1.0 / 60.0, 3200, 3200);
        assert!(rate > 1.05, "a short core settled at {rate}");
    }

    #[test]
    fn a_core_running_ahead_is_quickened_until_the_queue_holds() {
        let mut level = Level::new();
        let queued = settle(&mut level, 3200, 3200, 1.1 * 800.0 * 48000.0 / 44100.0, 800.0 * 48000.0 / 44100.0);
        assert!(queued.abs_diff(3200) < 400, "settled at {queued}");
        let rate = level.push(1.0 / 60.0, 3200, 3200);
        assert!(rate < 0.95, "a fast core settled at {rate}");
    }

    #[test]
    fn a_full_queue_is_drained_and_an_empty_one_filled() {
        let mut level = Level::new();
        assert!(level.push(1.0 / 60.0, 0, 3200) > 1.0);
        assert!(level.push(1.0 / 60.0, 6400, 3200) < 1.0);
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
                    (Level::MIN..=Level::MAX).contains(&rate),
                    "rate {rate} out of bounds for queued={queued}"
                );
            }
        }
    }
}
