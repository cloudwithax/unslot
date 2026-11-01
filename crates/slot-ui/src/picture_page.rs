//! A picture from the card shown whole on a page: a photo, or a page of a PDF the Reader has
//! drawn out. Drawn whole into one picture, like the lists.

use std::path::PathBuf;

use crate::list_page::{LIST_H, LIST_W};
use crate::page::{label_into, legends_into, over, Legend, DIM, INK};
use crate::plate::UndoFace;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PicturePage {
    pub title: String,
    /// Under the picture: which of how many, or what went wrong.
    pub status: String,
    pub path: PathBuf,
    pub legends: Vec<Legend>,
}

const TITLE_H: u32 = 32;
const STATUS_H: u32 = 24;
const HINTS_H: u32 = 40;
/// The room the picture is fitted in.
pub const PICTURE_W: u32 = LIST_W;
pub const PICTURE_H: u32 = LIST_H - TITLE_H - STATUS_H - HINTS_H;

impl PicturePage {
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
        let mut status = self.status.clone();
        match crate::art::contain(&self.path, PICTURE_W, PICTURE_H) {
            Some(picture) => over(
                &mut rgba, LIST_W, &picture, PICTURE_W, PICTURE_H, 0, TITLE_H,
            ),
            None if status.is_empty() => status = "This picture will not open".into(),
            None => {}
        }
        label_into(
            &mut rgba,
            LIST_W,
            (0, LIST_H - HINTS_H - STATUS_H, LIST_W, STATUS_H),
            &status,
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
