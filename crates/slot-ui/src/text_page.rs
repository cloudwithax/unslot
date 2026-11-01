//! A page of running text, for the books and manuals the Reader opens. Drawn whole into one
//! picture, like the lists.

use crate::list_page::{LIST_H, LIST_W};
use crate::page::{label_into, legends_into, over, Legend, DIM, INK};
use crate::plate::UndoFace;
use crate::text;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TextPage {
    pub title: String,
    /// Under the text: how far through it is.
    pub status: String,
    /// The page's lines, already wrapped to `TEXT_W` by `wrap`.
    pub lines: Vec<String>,
    pub legends: Vec<Legend>,
}

const TITLE_H: u32 = 32;
const MARGIN: u32 = 24;
const LINE_H: u32 = 24;
const STATUS_H: u32 = 24;
const HINTS_H: u32 = 40;
/// The text's size.
pub const TEXT_PX: f32 = 18.0;
/// How wide a line of text is.
pub const TEXT_W: u32 = LIST_W - 2 * MARGIN;
/// How many lines a page holds.
pub const TEXT_LINES: usize = ((LIST_H - TITLE_H - STATUS_H - HINTS_H) / LINE_H) as usize;

/// `text` in lines the page's width, to be cut into pages of `TEXT_LINES`.
pub fn wrap(text: &str) -> Vec<String> {
    match text::label_font() {
        Some(font) => text::wrap_as_written(font, text, TEXT_W as f32 - 4.0, TEXT_PX),
        None => text.lines().map(str::to_string).collect(),
    }
}

/// One wrapped line at the margin, at its full size: `wrap` has already made it fit.
fn line_into(rgba: &mut [u8], y: u32, line: &str) {
    let Some(font) = text::label_font() else {
        return;
    };
    let layout = text::fit_as_written(font, line, f32::MAX, TEXT_PX, TEXT_PX);
    let w = (text::line_width(font, line, layout.px, layout.tracking).ceil() as u32 + 2).min(TEXT_W);
    let mut band = vec![0u8; (w * LINE_H * 4) as usize];
    for (i, a) in text::coverage(w, LINE_H, &layout).into_iter().enumerate() {
        band[i * 4..i * 4 + 4].copy_from_slice(&[INK[0], INK[1], INK[2], a]);
    }
    over(rgba, LIST_W, &band, w, LINE_H, MARGIN, y);
}

impl TextPage {
    pub fn face(&self) -> UndoFace {
        let mut rgba = vec![0u8; (LIST_W * LIST_H * 4) as usize];
        label_into(
            &mut rgba,
            LIST_W,
            (0, 0, LIST_W, TITLE_H),
            &self.title,
            DIM,
            20.0,
            true,
        );
        for (i, line) in self.lines.iter().take(TEXT_LINES).enumerate() {
            if line.is_empty() {
                continue;
            }
            line_into(&mut rgba, TITLE_H + i as u32 * LINE_H, line);
        }
        let y = LIST_H - HINTS_H - STATUS_H;
        label_into(
            &mut rgba,
            LIST_W,
            (0, y, LIST_W, STATUS_H),
            &self.status,
            DIM,
            15.0,
            false,
        );
        legends_into(
            &mut rgba,
            LIST_W,
            (0, LIST_H - HINTS_H, LIST_W, HINTS_H),
            &self.legends,
            INK,
        );
        UndoFace {
            rgba,
            w: LIST_W,
            h: LIST_H,
        }
    }
}
