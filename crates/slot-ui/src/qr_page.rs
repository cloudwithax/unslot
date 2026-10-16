//! A page with a QR code on it, for signing in on a phone: Tailscale's login link. Drawn whole
//! into one picture, like the lists.

use crate::list_page::{LIST_H, LIST_W};
use crate::page::{label_into, DIM, INK};
use crate::plate::UndoFace;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct QrPage {
    pub title: String,
    pub status: String,
    /// What the code holds, written under it too for anyone without a camera.
    pub url: String,
    pub hints: String,
}

const TITLE_H: u32 = 32;
const STATUS_Y: u32 = 34;
const STATUS_H: u32 = 26;
const CODE_Y: u32 = 66;
const CODE_MAX: u32 = 290;
const URL_H: u32 = 30;
const HINTS_H: u32 = 30;
/// The light margin a reader needs round the code, in modules.
const QUIET: u32 = 3;
const PAPER: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
const DARK: [u8; 4] = [0x00, 0x00, 0x00, 0xff];

impl QrPage {
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
        let mut bottom = CODE_Y;
        if let Ok(code) = qrcode::QrCode::new(self.url.as_bytes()) {
            let n = code.width() as u32;
            let module = (CODE_MAX / (n + 2 * QUIET)).max(1);
            let side = module * (n + 2 * QUIET);
            let x0 = (LIST_W - side) / 2;
            fill(&mut rgba, (x0, CODE_Y, side, side), PAPER);
            for (i, colour) in code.to_colors().into_iter().enumerate() {
                if colour != qrcode::Color::Dark {
                    continue;
                }
                let (cx, cy) = (i as u32 % n + QUIET, i as u32 / n + QUIET);
                fill(
                    &mut rgba,
                    (x0 + cx * module, CODE_Y + cy * module, module, module),
                    DARK,
                );
            }
            bottom = CODE_Y + side;
        }
        label_into(
            &mut rgba,
            LIST_W,
            (0, bottom + 6, LIST_W, URL_H),
            &self.url,
            INK,
            16.0,
            false,
        );
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

fn fill(rgba: &mut [u8], (x, y, w, h): (u32, u32, u32, u32), colour: [u8; 4]) {
    for row in y..y + h {
        for col in x..x + w {
            let at = ((row * LIST_W + col) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_is_drawn_dark_on_a_light_square_in_the_middle() {
        let page = QrPage {
            url: "https://login.tailscale.com/a/0123456789ab".into(),
            ..QrPage::default()
        };
        let face = page.face();
        let at = |x: u32, y: u32| &face.rgba[((y * LIST_W + x) * 4) as usize..][..4];
        let code = qrcode::QrCode::new(page.url.as_bytes()).unwrap();
        let n = code.width() as u32;
        let module = CODE_MAX / (n + 2 * QUIET);
        let x0 = (LIST_W - module * (n + 2 * QUIET)) / 2;
        // The top-left finder: dark at its corner, light just outside it.
        let corner = (x0 + QUIET * module, CODE_Y + QUIET * module);
        assert_eq!(at(corner.0 + module / 2, corner.1 + module / 2), &DARK);
        assert_eq!(at(corner.0 - module / 2, corner.1 + module / 2), &PAPER);
        assert!(
            module >= 5,
            "{module}px modules are too small to scan off the screen"
        );
    }

    #[test]
    fn a_long_url_still_fits() {
        let page = QrPage {
            url: format!("https://login.tailscale.com/a/{}", "x".repeat(200)),
            ..QrPage::default()
        };
        assert_eq!(page.face().rgba.len(), (LIST_W * LIST_H * 4) as usize);
    }
}
