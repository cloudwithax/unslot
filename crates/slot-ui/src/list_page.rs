//! A page of rows slot fills at run time: the Wi-Fi networks in range, the Bluetooth devices
//! nearby. Drawn whole into one picture each time it changes, like the keyboard.

use crate::page::{label_into, label_left, rounded, DIM, INK, LIT, LIT_INK};
use crate::plate::UndoFace;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ListRow {
    pub label: String,
    /// What is said of it on the right: "Connected", "Saved".
    pub detail: String,
    /// Signal strength, 0 to 3 bars, for a row that has one.
    pub bars: Option<u8>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ListPage {
    pub title: String,
    /// A line under the title: what is happening, or what went wrong.
    pub status: String,
    pub rows: Vec<ListRow>,
    pub cursor: usize,
    pub hints: String,
}

pub const LIST_W: u32 = 680;
pub const LIST_H: u32 = 440;
/// How many rows are in view at once. A longer list scrolls to keep the cursor in view.
pub const LIST_ROWS: usize = 7;

const TITLE_H: u32 = 32;
const STATUS_Y: u32 = 34;
const STATUS_H: u32 = 26;
const ROWS_Y: u32 = 70;
const ROW_H: u32 = 46;
const ROW_PAD: u32 = 18;
const HINTS_H: u32 = 30;
const BARS_W: u32 = 34;
const DETAIL_W: u32 = 150;

impl ListPage {
    /// The first row in view: the window keeps the cursor as near its middle as the ends allow.
    pub fn first_shown(&self) -> usize {
        if self.rows.len() <= LIST_ROWS {
            return 0;
        }
        self.cursor
            .saturating_sub(LIST_ROWS / 2)
            .min(self.rows.len() - LIST_ROWS)
    }

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
        label_into(
            &mut rgba,
            LIST_W,
            (0, STATUS_Y, LIST_W, STATUS_H),
            &self.status,
            INK,
            18.0,
            false,
        );
        let first = self.first_shown();
        for (slot, (at, row)) in self
            .rows
            .iter()
            .enumerate()
            .skip(first)
            .take(LIST_ROWS)
            .enumerate()
        {
            let y = ROWS_Y + slot as u32 * ROW_H;
            let lit = at == self.cursor;
            let ink = match lit {
                true => {
                    rounded(&mut rgba, LIST_W, (0, y + 3, LIST_W, ROW_H - 6), 8, LIT);
                    LIT_INK
                }
                false => INK,
            };
            let right = LIST_W - ROW_PAD;
            let bars_x = right - BARS_W;
            let detail_x = bars_x - 8 - DETAIL_W;
            let label_w = detail_x - ROW_PAD - 8;
            label_left(
                &mut rgba,
                LIST_W,
                (ROW_PAD, y, label_w, ROW_H),
                &row.label,
                ink,
                22.0,
            );
            if !row.detail.is_empty() {
                let dim = if lit { LIT_INK } else { DIM };
                label_into(
                    &mut rgba,
                    LIST_W,
                    (detail_x, y, DETAIL_W, ROW_H),
                    &row.detail,
                    dim,
                    16.0,
                    false,
                );
            }
            if let Some(level) = row.bars {
                bars(&mut rgba, (bars_x, y, BARS_W, ROW_H), level, ink);
            }
        }
        if self.rows.len() > LIST_ROWS {
            let more = format!("{} of {}", self.cursor + 1, self.rows.len());
            label_into(
                &mut rgba,
                LIST_W,
                (0, ROWS_Y + LIST_ROWS as u32 * ROW_H, LIST_W, 10),
                &more,
                DIM,
                12.0,
                false,
            );
        }
        label_into(
            &mut rgba,
            LIST_W,
            (0, LIST_H - HINTS_H, LIST_W, HINTS_H),
            &self.hints,
            DIM,
            15.0,
            false,
        );
        UndoFace {
            rgba,
            w: LIST_W,
            h: LIST_H,
        }
    }
}

/// Three rising bars, `level` of them in ink and the rest faint.
fn bars(rgba: &mut [u8], (x, y, w, h): (u32, u32, u32, u32), level: u8, ink: [u8; 4]) {
    let bar_w = w / 4;
    let base = y + h - 12;
    for i in 0..3u32 {
        let tall = 8 + i * 7;
        let fill = match i < u32::from(level) {
            true => ink,
            false => [ink[0], ink[1], ink[2], 0x50],
        };
        let bx = x + i * (bar_w + bar_w / 3);
        rounded(rgba, LIST_W, (bx, base - tall, bar_w, tall), 1, fill);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<ListRow> {
        (0..n)
            .map(|i| ListRow {
                label: format!("Network {i}"),
                ..ListRow::default()
            })
            .collect()
    }

    #[test]
    fn a_short_list_never_scrolls() {
        let page = ListPage {
            rows: rows(5),
            cursor: 4,
            ..ListPage::default()
        };
        assert_eq!(page.first_shown(), 0);
    }

    #[test]
    fn a_long_list_keeps_the_cursor_in_view_and_stops_at_its_ends() {
        let mut page = ListPage {
            rows: rows(20),
            ..ListPage::default()
        };
        for cursor in 0..20 {
            page.cursor = cursor;
            let first = page.first_shown();
            assert!(
                (first..first + LIST_ROWS).contains(&cursor),
                "row {cursor} is out of view from {first}"
            );
            assert!(first + LIST_ROWS <= 20, "the window ran past the end");
        }
    }

    #[test]
    fn the_cursors_row_is_lit() {
        let page = ListPage {
            rows: rows(3),
            cursor: 1,
            ..ListPage::default()
        };
        let face = page.face();
        let at = |x: u32, y: u32| &face.rgba[((y * LIST_W + x) * 4) as usize..][..4];
        assert_eq!(
            at(LIST_W / 2, ROWS_Y + ROW_H + ROW_H / 2),
            &LIT,
            "row 1 is not lit"
        );
        assert_ne!(at(LIST_W / 2, ROWS_Y + ROW_H / 2), &LIT, "row 0 is lit too");
    }
}
