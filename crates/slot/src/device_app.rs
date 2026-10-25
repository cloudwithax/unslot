use std::path::PathBuf;
use std::time::{Duration, Instant};

use slot::frontend::Frontend;
use slot::input::DeviceInput;
use slot::panel_turn::{panel_turn, PanelTurn};
use slot_ftrace::{self as ft, ev};
use slot_gfx::hdmi::{self, CableWatch};
use slot_gfx::{Compositor, FbdevSurface, Surface};
use slot_power::{trace_first_frame, DevicePlatform};

const CARD: &str = "/mnt/sdcard";

const MIN_FRAME: Duration = Duration::from_millis(12);

const STEP_TIMEOUT: Duration = Duration::from_millis(12);

const MARGIN: Duration = Duration::from_micros(1500);

struct Pacer {
    last_return: Option<Instant>,
    period: Duration,
    work: Duration,
    works: [Duration; WORK_WINDOW],
    next: usize,
}

const WORK_WINDOW: usize = 32;

impl Pacer {
    fn new() -> Self {
        Pacer {
            last_return: None,
            period: Duration::from_micros(16_760),
            work: Duration::from_millis(4),
            works: [Duration::from_millis(4); WORK_WINDOW],
            next: 0,
        }
    }

    fn wait(&self) {
        let Some(last) = self.last_return else {
            return;
        };
        let delay = self.period.saturating_sub(self.work + MARGIN);
        if let Some(left) = (last + delay).checked_duration_since(Instant::now()) {
            std::thread::sleep(left);
        }
    }

    fn swapped(&mut self, work: Duration, blocked: bool) -> bool {
        let now = Instant::now();
        let mut missed = false;
        if let Some(last) = self.last_return {
            let seen = now - last;
            missed = seen > self.period.mul_f64(1.5);
            ft::counter(ev::PRESENT_SEEN_NS, seen.as_nanos() as i64);
            if missed {
                ft::instant(ev::PRESENT_MISSED, seen.as_nanos() as i64);
            }
            if blocked && seen > Duration::from_millis(12) && seen < Duration::from_millis(22) {
                self.period = self.period.mul_f64(0.95) + seen.mul_f64(0.05);
            }
        }
        self.works[self.next] = work;
        self.next = (self.next + 1) % WORK_WINDOW;
        self.work = self.works.iter().copied().max().unwrap_or(work);
        ft::counter(ev::PRESENT_PERIOD_NS, self.period.as_nanos() as i64);
        ft::counter(ev::PRESENT_WORK_MAX_NS, self.work.as_nanos() as i64);
        self.last_return = Some(now);
        missed
    }
}

pub fn run() {
    slot::boot_time::mark("start");
    let _ = std::thread::Builder::new()
        .name("slot-fonts".into())
        .spawn(slot_ui::warm_fonts);
    // The picture goes to a TV if one is plugged in, before the screen opens: its size is fixed
    // from then on. The sound follows it, so this comes before the sound opens too.
    let hdmi_paths = hdmi::Paths::default();
    let switchable = hdmi_paths.switchable();
    let synced = hdmi::sync(
        &hdmi_paths,
        hdmi::panel_for(slot::baseos::var("BASEOS_TARGET").as_deref()),
    );
    slot::audio::set_hdmi(synced.on_tv);
    let audio = std::thread::Builder::new()
        .name("slot-audio-open".into())
        .spawn(slot::audio::opened_sink);
    let root = std::env::var_os("SLOT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(CARD));
    let opened = match synced.fb {
        Some(size) => FbdevSurface::sized(size),
        None => FbdevSurface::new(),
    };
    let mut surface = match opened {
        Ok(s) => s,
        Err(e) => {
            eprintln!("slot: {e}");
            return;
        }
    };
    slot::boot_time::mark("surface");
    let mut compositor = match Compositor::new(&surface) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("slot: {e}");
            return;
        }
    };
    slot::boot_time::mark("compositor");
    let panel = surface.window_size();
    let env = |key| std::env::var(key).ok();
    // A TV is never mounted on its side, whichever way the panel is.
    let turn = if synced.on_tv {
        PanelTurn::None
    } else {
        panel_turn(
            panel,
            slot::baseos::var("BASEOS_PANEL_ROTATION_CCW").as_deref(),
            env("SLOT_ROTATE").as_deref(),
        )
    };
    let turned = turn != PanelTurn::None;
    let window = if turned { (panel.1, panel.0) } else { panel };
    eprintln!(
        "slot: baseos target {}, model {}, rotation ccw {}",
        slot::baseos::var("BASEOS_TARGET").unwrap_or_default(),
        slot::baseos::var("BASEOS_MODEL").unwrap_or_default(),
        slot::baseos::var("BASEOS_PANEL_ROTATION_CCW").unwrap_or_default()
    );
    eprintln!(
        "slot: panel {}x{}, canvas {}x{}, turn {turn:?}",
        panel.0, panel.1, window.0, window.1
    );
    if turned {
        if let Err(e) = compositor.set_panel_turn(panel, turn == PanelTurn::Ccw) {
            eprintln!("slot: {e}");
            return;
        }
    }
    // Draw the boot logo before the shelf is built, so the panel is not left black (or on a stale
    // picture) for the second or so the card takes to read.
    if slot::frontend::draw_boot_splash(&mut compositor, &root, window) {
        if let Err(e) = surface.swap() {
            eprintln!("slot: boot logo: {e}");
        }
    }
    let platform = DevicePlatform::new(root.clone());
    eprintln!("slot: {}", platform.report());
    platform.trace_boot();
    slot::boot_time::mark("platform");
    let sink = match audio.map(|h| h.join()) {
        Ok(Ok(sink)) => sink,
        _ => slot::audio::opened_sink(),
    };
    slot::boot_time::mark("audio");
    let mut frontend = Frontend::boot_with(Box::new(platform), sink);
    frontend.set_external_display(synced.on_tv);
    let mut cable = switchable.then(|| CableWatch::new(hdmi_paths, synced.cable));
    slot::boot_time::mark("frontend");
    frontend.upload_boot_faces(&mut compositor);
    slot::boot_time::mark("faces");
    frontend.upload_bezel(&mut compositor, window);
    slot::boot_time::mark("bezel");
    let mut input = DeviceInput::open(&root);
    slot::boot_time::mark("input");
    let card = root.clone();
    let _ = std::thread::Builder::new()
        .name("slot-bootlogo".into())
        .spawn(move || slot::bootlogo::refresh(&card));
    let mut drawn = false;
    frontend.drive_emulator();
    let mut pacer = Pacer::new();
    let (mut frames, mut missed, mut early) = (0u32, 0u32, 0u32);
    let mut present_no = 0i64;
    loop {
        present_no += 1;
        let _iter = ft::span(ev::PRESENT_ITER, present_no);
        {
            let _s = ft::span(ev::PRESENT_PACER_WAIT, pacer.period.as_nanos() as i64);
            pacer.wait();
        }
        let began = Instant::now();
        let advance = ft::span(ev::PRESENT_ADVANCE, 0);
        frontend.advance(&mut input);
        drop(advance);
        if frontend.restarting() {
            frontend.restart();
        }
        if frontend.powering_off() {
            frontend.poweroff();
            return;
        }
        if frontend.reloading() {
            // init starts the frontend again once slot is gone.
            frontend.reload();
            return;
        }
        if cable.as_mut().is_some_and(|c| c.changed(began)) {
            // The same: slot starts again, and the picture goes where the cable now says.
            eprintln!("slot: hdmi cable changed, starting again");
            frontend.switch_display();
            return;
        }
        {
            let _s = ft::span(ev::PRESENT_STEP, 0);
            frontend.step_emulator(pacer.period, STEP_TIMEOUT);
        }
        {
            let _s = ft::span(ev::PRESENT_RENDER, 0);
            frontend.render(&mut compositor, window);
        }
        let swap = Instant::now();
        let work = swap - began;
        ft::begin(ev::PRESENT_SWAP, 0);
        let swapped = surface.swap();
        let swap_took = swap.elapsed();
        ft::end(ev::PRESENT_SWAP, swap_took.as_nanos() as i64);
        ft::counter(ev::PRESENT_WORK_NS, work.as_nanos() as i64);
        if let Err(e) = swapped {
            eprintln!("slot: {e}");
            return;
        }
        slot::latency::swapped(swap_took.as_secs_f64() * 1000.0);
        let dropped = pacer.swapped(work, swap_took > Duration::from_millis(1));
        if slot::latency::tracing() {
            frames += 1;
            missed += u32::from(dropped);
            early += u32::from(swap_took > pacer.period / 2);
            if frames == 600 {
                eprintln!(
                    "slot: pace: 600 frames, {missed} missed a latch, {early} drawn early, work {:.1} ms, period {:.2} ms",
                    pacer.work.as_secs_f64() * 1e3,
                    pacer.period.as_secs_f64() * 1e3
                );
                (frames, missed, early) = (0, 0, 0);
            }
        }
        if !drawn {
            drawn = true;
            slot::boot_time::mark("first frame");
            slot::update::confirm(&root);
            trace_first_frame();
        }
        if let Some(left) = MIN_FRAME.checked_sub(began.elapsed()) {
            let _s = ft::span(ev::PRESENT_MIN_SLEEP, left.as_nanos() as i64);
            std::thread::sleep(left);
        }
    }
}
