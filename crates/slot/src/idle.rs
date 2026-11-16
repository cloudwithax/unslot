use slot_input::Millis;

#[derive(Default)]
pub struct IdleTimer {
    last_input: Millis,
}

#[derive(Default)]
pub struct Activity {
    pub link: bool,
    pub video: bool,
    pub music: bool,
    pub casting: bool,
    pub downloading: bool,
    pub asleep: bool,
    pub powering_off: bool,
}

impl IdleTimer {
    pub fn input(&mut self, now: Millis) {
        self.last_input = self.last_input.max(now);
    }

    /// Busy time is not idle time: finishing playback or a transfer starts a fresh interval.
    pub fn tick(&mut self, now: Millis, minutes: u8, activity: Activity) -> bool {
        if activity.link
            || activity.video
            || activity.music
            || activity.casting
            || activity.downloading
            || activity.asleep
            || activity.powering_off
        {
            self.input(now);
            return false;
        }
        minutes != 0 && now.saturating_sub(self.last_input) >= u64::from(minutes) * 60_000
    }
}
