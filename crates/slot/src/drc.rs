use std::time::{Duration, Instant};

const MAX_CORRECTION: f64 = 0.005;

pub fn drc_ratio(queued_frames: usize, target_frames: usize) -> f64 {
    if target_frames == 0 {
        return 1.0;
    }
    let error = target_frames as f64 - queued_frames as f64;
    let ratio = 1.0 + MAX_CORRECTION * error / target_frames as f64;
    ratio.clamp(1.0 - MAX_CORRECTION, 1.0 + MAX_CORRECTION)
}

pub fn drc_target(capacity_frames: usize) -> usize {
    capacity_frames / 2
}

/// How much longer than the sound a core hands over lasts it takes to arrive, for a core that
/// can't keep its full rate: its sound is played that much slower rather than run dry.
pub struct Stretch {
    window: Option<(Instant, f64)>,
    last: Option<Instant>,
    target: f64,
    ratio: f64,
}

/// The most the sound is slowed, about what a core a few frames a second short needs.
pub const MAX_STRETCH: f64 = 1.08;

/// How long the arrival rate is measured over before the stretch heads for it.
const STRETCH_WINDOW: Duration = Duration::from_millis(500);

/// How long the stretch takes to get most of the way to a new rate, so the pitch glides.
const STRETCH_GLIDE: Duration = Duration::from_secs(2);

/// A wait between hand-overs this long is a pause, not the core running slow.
const STRETCH_GAP: Duration = Duration::from_millis(100);

impl Default for Stretch {
    fn default() -> Self {
        Stretch {
            window: None,
            last: None,
            target: 1.0,
            ratio: 1.0,
        }
    }
}

impl Stretch {
    /// Takes `sound` seconds of playback handed over at `now` and gives how much to stretch it.
    pub fn push(&mut self, now: Instant, sound: f64) -> f64 {
        let since = self.last.map(|last| now.saturating_duration_since(last));
        self.last = Some(now);
        let Some(since) = since.filter(|&d| d < STRETCH_GAP) else {
            self.window = Some((now, sound));
            return self.ratio;
        };
        match self.window {
            Some((start, heard)) if now - start >= STRETCH_WINDOW && heard > 0.0 => {
                let took = (now - start).as_secs_f64() / heard;
                self.target = took.clamp(1.0, MAX_STRETCH);
                self.window = Some((now, sound));
            }
            Some((start, heard)) => self.window = Some((start, heard + sound)),
            None => self.window = Some((now, sound)),
        }
        let step = (since.as_secs_f64() / STRETCH_GLIDE.as_secs_f64()).min(1.0);
        self.ratio += (self.target - self.ratio) * step;
        self.ratio
    }

    /// Forgets the arrival rate being measured, for when the core stops running at its own pace.
    pub fn reset(&mut self) {
        self.window = None;
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(stretch: &mut Stretch, start: Instant, every: Duration, sound: f64, n: u32) -> f64 {
        let mut ratio = 1.0;
        for i in 0..n {
            ratio = stretch.push(start + every * i, sound);
        }
        ratio
    }

    #[test]
    fn a_core_at_full_rate_is_not_stretched() {
        let mut stretch = Stretch::default();
        let ratio = run(
            &mut stretch,
            Instant::now(),
            Duration::from_micros(16_667),
            0.016_667,
            600,
        );
        assert!((ratio - 1.0).abs() < 1e-3, "{ratio}");
    }

    #[test]
    fn a_core_running_short_is_slowed_to_match_it() {
        let mut stretch = Stretch::default();
        let ratio = run(
            &mut stretch,
            Instant::now(),
            Duration::from_micros(17_857),
            0.016_667,
            900,
        );
        assert!((ratio - 56.0f64.recip() * 60.0).abs() < 2e-3, "{ratio}");
    }

    #[test]
    fn the_stretch_glides_rather_than_jumps() {
        let mut stretch = Stretch::default();
        let start = Instant::now();
        let every = Duration::from_millis(20);
        let mut last = 1.0;
        for i in 0..300 {
            let ratio = stretch.push(start + every * i, 0.016_667);
            assert!((ratio - last).abs() < 0.002, "jumped {last} to {ratio}");
            last = ratio;
        }
        assert!(last > 1.05);
    }

    #[test]
    fn a_core_far_behind_is_stretched_no_further_than_the_cap() {
        let mut stretch = Stretch::default();
        let ratio = run(
            &mut stretch,
            Instant::now(),
            Duration::from_millis(33),
            0.016_667,
            900,
        );
        assert!((ratio - MAX_STRETCH).abs() < 1e-3, "{ratio}");
    }

    #[test]
    fn a_pause_is_not_taken_for_a_slow_core() {
        let mut stretch = Stretch::default();
        let start = Instant::now();
        let every = Duration::from_micros(16_667);
        run(&mut stretch, start, every, 0.016_667, 60);
        let back = start + Duration::from_secs(5);
        let ratio = run(&mut stretch, back, every, 0.016_667, 600);
        assert!((ratio - 1.0).abs() < 1e-3, "{ratio}");
    }
}
