use slot_gfx::{Canvas, Draw, TexId, OUT_H};
use slot_power::Battery;

use crate::battery::{draw_gauge, GAUGE_H};
use crate::hud::{HUD_INK, PLATE_H};
use crate::plate::HINT_H;
use crate::slot_chrome::MOUTH_H;

pub const FOOTER_Y: f32 = OUT_H as f32 - MOUTH_H + (MOUTH_H - HINT_H as f32) / 2.0;
const FOOTER_MARGIN: f32 = 24.0;
const CORNER_MARGIN: f32 = 16.0;

#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct Printed {
    pub face: Option<TexId>,
    pub w: u32,
}

impl Printed {
    pub fn new(face: TexId, w: u32) -> Self {
        Printed {
            face: Some(face),
            w,
        }
    }
}

/// How big the network icons are drawn, and the room between them and the clock.
const NET_PX: f32 = 16.0;
const NET_GAP: f32 = 8.0;

/// The battery on the left; on the right the clock, and left of it an icon for each network
/// that is up, Wi-Fi then Bluetooth.
#[allow(clippy::too_many_arguments)]
pub fn draw_footer(
    canvas: Canvas,
    battery: Option<Battery>,
    percent: Printed,
    bolt: Option<TexId>,
    clock: Printed,
    net: &[TexId],
    ink: [u8; 3],
    out: &mut Vec<Draw>,
) {
    let y = FOOTER_Y + (HINT_H as f32 - GAUGE_H) / 2.0;
    draw_gauge(FOOTER_MARGIN, y, battery, percent, bolt, ink, out);
    let clock_x = canvas.w - FOOTER_MARGIN - clock.w as f32;
    printed(clock_x, clock, out);
    let mut x = clock_x - NET_GAP;
    for &tex in net.iter().rev() {
        x -= NET_PX;
        out.push(Draw::Tex {
            x,
            y: FOOTER_Y + (HINT_H as f32 - NET_PX) / 2.0,
            w: NET_PX,
            h: NET_PX,
            tex,
            alpha: 1.0,
        });
        x -= NET_GAP / 2.0;
    }
}

pub fn draw_corners(
    canvas: Canvas,
    battery: Option<Battery>,
    percent: Printed,
    bolt: Option<TexId>,
    clock: Printed,
    out: &mut Vec<Draw>,
) {
    let y = (PLATE_H - GAUGE_H) / 2.0;
    draw_gauge(CORNER_MARGIN, y, battery, percent, bolt, HUD_INK, out);
    let x = canvas.w - CORNER_MARGIN - clock.w as f32;
    draw_printed(x, (PLATE_H - HINT_H as f32) / 2.0, clock, out);
}

pub(crate) fn draw_printed(x: f32, y: f32, p: Printed, out: &mut Vec<Draw>) {
    if p.w == 0 {
        return;
    }
    let (w, h) = (p.w as f32, HINT_H as f32);
    out.push(match p.face {
        Some(tex) => Draw::Tex {
            x,
            y,
            w,
            h,
            tex,
            alpha: 1.0,
        },
        None => Draw::Rect {
            x,
            y,
            w,
            h,
            colour: [1.0, 1.0, 1.0, 0.08],
        },
    });
}

fn printed(x: f32, p: Printed, out: &mut Vec<Draw>) {
    draw_printed(x, FOOTER_Y, p, out);
}
