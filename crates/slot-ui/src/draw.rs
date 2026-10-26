pub use slot_gfx::{Canvas, Draw, Layout, TexId, OUT_H, OUT_W};

pub fn shift_down(draws: &mut [Draw], dy: f32) {
    if dy == 0.0 {
        return;
    }
    for draw in draws {
        match draw {
            Draw::Rect { y, .. }
            | Draw::Tex { y, .. }
            | Draw::Part { y, .. }
            | Draw::Turned { y, .. } => *y += dy,
            Draw::Game | Draw::Shot { .. } => {}
        }
    }
}
