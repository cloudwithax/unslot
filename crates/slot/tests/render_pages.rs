#![cfg(any(target_os = "macos", target_os = "linux"))]

//! Every page the quick menu can open gets drawn: the pages that are not covered by a picture
//! test of their own are walked here and checked for ink, so a surface that draws nothing (or
//! only its background) is caught.

mod common;

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

use common::{clocked, tmp_root_with_carts};
use slot::app::Phase;
use slot::frontend::Frontend;
use slot_gfx::{Compositor, HeadlessSurface, OUT_H, OUT_W};
use slot_input::{Btn, InputSource, Millis, RawEvent};
use slot_power::SimPlatform;
use slot_ui::QuickRow;

static GL: Mutex<()> = Mutex::new(());

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

fn tap(f: &mut Frontend, input: &mut Script, btn: Btn) {
    input.0.push_back(vec![RawEvent::Down(btn)]);
    f.advance(input);
    input.0.push_back(vec![RawEvent::Up(btn)]);
    f.advance(input);
}

fn composed(f: &mut Frontend, c: &mut Compositor) -> Vec<u8> {
    f.compose(c);
    c.read_frame()
}

/// Bright pixels in the page body, away from the top bar and the footer, so a page that draws
/// only its chrome fails.
fn body_ink(px: &[u8]) -> usize {
    let mut n = 0;
    for y in 70..OUT_H as usize - 90 {
        for x in (20..OUT_W as usize - 20).step_by(2) {
            let o = (y * OUT_W as usize + x) * 4;
            if px[o] > 0x80 && px[o + 1] > 0x80 && px[o + 2] > 0x80 {
                n += 1;
            }
        }
    }
    n
}

#[allow(clippy::type_complexity)]
fn boot() -> Option<(
    MutexGuard<'static, ()>,
    HeadlessSurface,
    Compositor,
    tempfile::TempDir,
    Frontend,
    Script,
)> {
    let guard = GL.lock().unwrap_or_else(PoisonError::into_inner);
    let surface = HeadlessSurface::new().ok()?;
    let mut c = Compositor::new(&surface).ok()?;
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    f.advance(&mut input);
    Some((guard, surface, c, d, f, input))
}

fn go(f: &mut Frontend, input: &mut Script, row: QuickRow) {
    let hidden = f.app().hidden_rows();
    let Some(current) = f.app().quick_menu() else {
        panic!("not on a quick menu page when asked for {row:?}");
    };
    let (from, to) = (current.position(&hidden), row.position(&hidden));
    for _ in from..to {
        tap(f, input, Btn::Down);
    }
    for _ in to..from {
        tap(f, input, Btn::Up);
    }
}

fn open_page(f: &mut Frontend, input: &mut Script, c: &mut Compositor, row: QuickRow, name: &str) {
    go(f, input, row);
    tap(f, input, Btn::A);
    let px = composed(f, c);
    if let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") {
        let path = format!("{dir}/page-{name}.png");
        let file = std::fs::File::create(&path).expect("create png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(file), OUT_W, OUT_H);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()
            .expect("png header")
            .write_image_data(&px)
            .expect("png data");
    }
    let ink = body_ink(&px);
    assert!(
        ink > 40,
        "{name} drew almost nothing ({ink} bright samples): {:?}",
        f.app().phase()
    );
}

#[test]
fn the_network_pages_draw_their_own_rows() {
    let Some((_g, _s, mut c, _d, mut f, mut input)) = boot() else {
        return;
    };
    tap(&mut f, &mut input, Btn::Menu);
    open_page(&mut f, &mut input, &mut c, QuickRow::Network, "network");

    open_page(&mut f, &mut input, &mut c, QuickRow::Services, "services");
    assert!(
        matches!(f.app().phase(), Phase::Services { .. }),
        "services opened {:?}",
        f.app().phase()
    );
    tap(&mut f, &mut input, Btn::B);

    open_page(&mut f, &mut input, &mut c, QuickRow::Updates, "updates");
    assert!(
        matches!(f.app().phase(), Phase::Updates { .. }),
        "updates opened {:?}",
        f.app().phase()
    );
    tap(&mut f, &mut input, Btn::B);

    open_page(&mut f, &mut input, &mut c, QuickRow::GameArt, "game art");
    assert!(
        matches!(f.app().phase(), Phase::GameArt { .. }),
        "game art opened {:?}",
        f.app().phase()
    );
}

#[test]
fn the_settings_pages_draw_their_own_rows() {
    let Some((_g, _s, mut c, _d, mut f, mut input)) = boot() else {
        return;
    };
    tap(&mut f, &mut input, Btn::Menu);

    open_page(&mut f, &mut input, &mut c, QuickRow::General, "general");
    tap(&mut f, &mut input, Btn::B);

    open_page(&mut f, &mut input, &mut c, QuickRow::Screen, "screen");
    // The screen page lists one row per platform; open the first platform's page too.
    tap(&mut f, &mut input, Btn::A);
    let px = composed(&mut f, &mut c);
    assert!(body_ink(&px) > 40, "a platform's screen page drew almost nothing");
    tap(&mut f, &mut input, Btn::B);
    tap(&mut f, &mut input, Btn::B);

    open_page(&mut f, &mut input, &mut c, QuickRow::Game, "game");
    tap(&mut f, &mut input, Btn::B);

    open_page(&mut f, &mut input, &mut c, QuickRow::About, "about");
    assert!(
        matches!(f.app().phase(), Phase::About),
        "about opened {:?}",
        f.app().phase()
    );
}
