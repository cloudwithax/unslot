use crate::bezel::framed;
use crate::surface::{OUT_H, OUT_W};

/// A square panel, as the RG CubeXX has: the menus fill it at 720 by 720.
pub fn square(panel: (u32, u32)) -> bool {
    panel.0 > 0 && panel.0 == panel.1
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Native,
    Framed,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Canvas {
    pub w: f32,
    pub h: f32,
}

impl Canvas {
    pub const FRAMED: Canvas = Canvas {
        w: OUT_W as f32,
        h: OUT_H as f32,
    };

    pub fn of(layout: Layout, panel: (u32, u32)) -> Canvas {
        match layout {
            Layout::Native if framed(panel) => Canvas {
                w: (OUT_H * panel.0 / panel.1) as f32,
                h: OUT_H as f32,
            },
            Layout::Native if square(panel) => Canvas {
                w: OUT_W as f32,
                h: OUT_W as f32,
            },
            _ => Canvas::FRAMED,
        }
    }

    /// How much taller than the 480 the menus were drawn for, the room a square panel has
    /// above and below them.
    pub fn extra(self) -> f32 {
        self.h - OUT_H as f32
    }

    pub fn centred(self, w: f32) -> f32 {
        (self.w - w) / 2.0
    }

    pub fn window(self, tex_w: f32) -> [f32; 4] {
        if tex_w <= self.w {
            return [0.0, 0.0, 1.0, 1.0];
        }
        [(tex_w - self.w) / 2.0 / tex_w, 0.0, self.w / tex_w, 1.0]
    }

    /// The part of a whole-screen picture, `tex_w` by 480, that fills the canvas uncropped
    /// in height: on a square one, its middle.
    pub fn cover(self, tex_w: f32) -> [f32; 4] {
        let seen = Canvas {
            w: self.w * OUT_H as f32 / self.h,
            h: OUT_H as f32,
        };
        seen.window(tex_w)
    }
}
