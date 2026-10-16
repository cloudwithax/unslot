//! A d-pad keyboard for typing a Wi-Fi password or a network's name.
//!
//! Ten keys a row, with the digits always on top, the way muOS and NextUI lay theirs out. A types
//! the key under the cursor, B deletes, Y is space, X is shift, L1 and R1 swap letters and
//! symbols, Start is done, Select shows or hides a password, and Menu gives up. B never throws
//! the text away: the projects that made it cancel moved it off for that reason.

use crate::page::{label_into, over, rounded, DIM, INK, LIT, LIT_INK};
use crate::plate::UndoFace;
use crate::text;

pub const COLS: usize = 10;
const ROWS: usize = 5;

const LETTERS: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl-", "zxcvbnm_.@"];
const SYMBOLS: [&str; 4] = ["1234567890", "!#$%^&*(),", "=+[]{}\\|;:", "'\"<>?/`~-@"];

/// The bottom row's keys and the columns each spans: where it starts and how wide it is.
const BOTTOM: [(Special, usize, usize); 5] = [
    (Special::Shift, 0, 2),
    (Special::Page, 2, 1),
    (Special::Space, 3, 4),
    (Special::Delete, 7, 1),
    (Special::Done, 8, 2),
];

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Special {
    Shift,
    Page,
    Space,
    Delete,
    Done,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Special(Special),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Page {
    Lower,
    /// Upper case for the next letter only.
    Shifted,
    /// Upper case until shift is pressed again.
    Caps,
    Symbols,
}

/// What the player asked the keyboard to do.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KeyInput {
    Up,
    Down,
    Left,
    Right,
    /// A: whatever key the cursor is on.
    Pick,
    Delete,
    Space,
    Shift,
    Page,
    Done,
    Reveal,
    Cancel,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum KeyOutcome {
    Typing,
    Done(String),
    Cancelled,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Keyboard {
    title: String,
    text: String,
    row: usize,
    col: usize,
    page: Page,
    masked: bool,
    /// Whether the text is a secret at all, so Select has something to show or hide.
    secret: bool,
    min: usize,
    max: usize,
}

impl Keyboard {
    /// A keyboard asking for `title`, starting from `text`. A `secret` is shown as stars until
    /// Select reveals it. Done does nothing until there are `min` characters, and nothing past
    /// `max` is taken.
    pub fn new(title: &str, text: &str, secret: bool, min: usize, max: usize) -> Keyboard {
        Keyboard {
            title: title.to_string(),
            text: text.chars().take(max).collect(),
            row: 1,
            col: 0,
            page: Page::Lower,
            masked: secret,
            secret,
            min,
            max,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text as the field shows it: stars for a hidden secret.
    pub fn shown(&self) -> String {
        match self.masked {
            true => "*".repeat(self.text.chars().count()),
            false => self.text.clone(),
        }
    }

    pub fn ready(&self) -> bool {
        self.text.chars().count() >= self.min
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// The key under the cursor.
    pub fn key(&self) -> Key {
        self.key_at(self.row, self.col)
    }

    fn key_at(&self, row: usize, col: usize) -> Key {
        if row == ROWS - 1 {
            let (special, ..) = BOTTOM
                .iter()
                .copied()
                .find(|(_, at, w)| (*at..at + w).contains(&col))
                .unwrap_or(BOTTOM[0]);
            return Key::Special(special);
        }
        let rows = match self.page {
            Page::Symbols => SYMBOLS,
            _ => LETTERS,
        };
        let ch = rows[row].chars().nth(col).unwrap_or(' ');
        Key::Char(match self.page {
            Page::Shifted | Page::Caps => ch.to_ascii_uppercase(),
            _ => ch,
        })
    }

    pub fn press(&mut self, input: KeyInput) -> KeyOutcome {
        match input {
            KeyInput::Up => self.row = (self.row + ROWS - 1) % ROWS,
            KeyInput::Down => self.row = (self.row + 1) % ROWS,
            KeyInput::Left => self.step(false),
            KeyInput::Right => self.step(true),
            KeyInput::Pick => return self.activate(self.key()),
            KeyInput::Delete => {
                self.text.pop();
            }
            KeyInput::Space => self.type_char(' '),
            KeyInput::Shift => self.shift(),
            KeyInput::Page => self.flip_page(),
            KeyInput::Done => return self.finish(),
            KeyInput::Reveal => self.masked = self.secret && !self.masked,
            KeyInput::Cancel => return KeyOutcome::Cancelled,
        }
        KeyOutcome::Typing
    }

    fn step(&mut self, right: bool) {
        if self.row != ROWS - 1 {
            self.col = (self.col + if right { 1 } else { COLS - 1 }) % COLS;
            return;
        }
        let at = BOTTOM
            .iter()
            .position(|(_, start, w)| (*start..start + w).contains(&self.col))
            .unwrap_or(0);
        let to = (at + if right { 1 } else { BOTTOM.len() - 1 }) % BOTTOM.len();
        self.col = BOTTOM[to].1;
    }

    fn activate(&mut self, key: Key) -> KeyOutcome {
        match key {
            Key::Char(ch) => self.type_char(ch),
            Key::Special(Special::Shift) => self.shift(),
            Key::Special(Special::Page) => self.flip_page(),
            Key::Special(Special::Space) => self.type_char(' '),
            Key::Special(Special::Delete) => {
                self.text.pop();
            }
            Key::Special(Special::Done) => return self.finish(),
        }
        KeyOutcome::Typing
    }

    fn type_char(&mut self, ch: char) {
        if self.text.chars().count() >= self.max {
            return;
        }
        self.text.push(ch);
        if self.page == Page::Shifted {
            self.page = Page::Lower;
        }
    }

    fn shift(&mut self) {
        self.page = match self.page {
            Page::Lower => Page::Shifted,
            Page::Shifted => Page::Caps,
            Page::Caps => Page::Lower,
            Page::Symbols => Page::Symbols,
        };
    }

    fn flip_page(&mut self) {
        self.page = match self.page {
            Page::Symbols => Page::Lower,
            _ => Page::Symbols,
        };
    }

    fn finish(&self) -> KeyOutcome {
        match self.ready() {
            true => KeyOutcome::Done(self.text.clone()),
            false => KeyOutcome::Typing,
        }
    }

    fn special_label(&self, special: Special) -> &'static str {
        match special {
            Special::Shift => match self.page {
                Page::Caps => "CAPS",
                _ => "SHIFT",
            },
            Special::Page => match self.page {
                Page::Symbols => "abc",
                _ => "#+=",
            },
            Special::Space => "space",
            Special::Delete => "del",
            Special::Done => "OK",
        }
    }

    /// The whole keyboard as one picture: the title, the field with its caret, the keys with the
    /// cursor's lit, and a line of what the buttons do.
    pub fn face(&self) -> UndoFace {
        let mut rgba = vec![0u8; (FACE_W * FACE_H * 4) as usize];
        label_into(
            &mut rgba,
            FACE_W,
            (0, 0, FACE_W, TITLE_H),
            &self.title,
            DIM,
            20.0,
            true,
        );
        self.draw_field(&mut rgba);
        for row in 0..ROWS {
            let y = KEYS_Y + row as u32 * (KEY_H + GAP);
            let keys: Vec<(Key, usize, usize)> = match row == ROWS - 1 {
                true => BOTTOM
                    .iter()
                    .map(|(s, at, w)| (Key::Special(*s), *at, *w))
                    .collect(),
                false => (0..COLS).map(|c| (self.key_at(row, c), c, 1)).collect(),
            };
            for (key, at, span) in keys {
                let lit = row == self.row && (at..at + span).contains(&self.col);
                let x = at as u32 * (KEY_W + GAP);
                let w = span as u32 * KEY_W + (span as u32 - 1) * GAP;
                let lit_special = match key {
                    Key::Special(Special::Shift) => matches!(self.page, Page::Shifted | Page::Caps),
                    Key::Special(Special::Done) => self.ready(),
                    _ => false,
                };
                let (fill, ink) = match (lit, lit_special) {
                    (true, _) => (LIT, LIT_INK),
                    (false, true) => (ARMED, INK),
                    (false, false) => (KEY, INK),
                };
                rounded(&mut rgba, FACE_W, (x, y, w, KEY_H), RADIUS, fill);
                let label = match key {
                    Key::Char(ch) => ch.to_string(),
                    Key::Special(s) => self.special_label(s).to_string(),
                };
                label_into(
                    &mut rgba,
                    FACE_W,
                    (x, y, w, KEY_H),
                    &label,
                    ink,
                    KEY_PX,
                    false,
                );
            }
        }
        let show = match self.secret {
            true => "  SELECT show",
            false => "",
        };
        let hints = format!(
            "A type   B delete   Y space   X shift   L1/R1 symbols{show}   START done   MENU cancel"
        );
        label_into(
            &mut rgba,
            FACE_W,
            (0, FACE_H - HINTS_H, FACE_W, HINTS_H),
            &hints,
            DIM,
            15.0,
            false,
        );
        UndoFace {
            rgba,
            w: FACE_W,
            h: FACE_H,
        }
    }

    fn draw_field(&self, rgba: &mut [u8]) {
        let (x, y, w, h) = (0, FIELD_Y, FACE_W, FIELD_H);
        rounded(rgba, FACE_W, (x, y, w, h), RADIUS, FIELD);
        let Some(font) = text::label_font() else {
            return;
        };
        let room = (w - 2 * FIELD_PAD) as f32;
        // Keep the end of a long line in view: that is where the typing is.
        let mut shown = self.shown();
        while shown.chars().count() > 1
            && text::line_width(font, &shown, FIELD_PX, 0.0) > room - CARET_W as f32
        {
            shown.remove(0);
        }
        let layout = text::Layout {
            lines: vec![format!("{shown}|")],
            px: FIELD_PX,
            tracking: 0.0,
        };
        let mut band = vec![0u8; ((w - 2 * FIELD_PAD) * h * 4) as usize];
        let cov = text::coverage(w - 2 * FIELD_PAD, h, &layout);
        let text_w = text::line_width(font, &layout.lines[0], FIELD_PX, 0.0);
        // `coverage` centres the line; slide it to the left edge of the field.
        let shift = ((room - text_w) / 2.0).max(0.0) as u32;
        let bw = w - 2 * FIELD_PAD;
        for row in 0..h {
            for col in 0..bw {
                let from = col + shift;
                if from >= bw {
                    continue;
                }
                let a = cov[(row * bw + from) as usize];
                let at = ((row * bw + col) * 4) as usize;
                band[at..at + 4].copy_from_slice(&[INK[0], INK[1], INK[2], a]);
            }
        }
        over(rgba, FACE_W, &band, bw, h, x + FIELD_PAD, y);
    }
}

pub const FACE_W: u32 = 680;
pub const FACE_H: u32 = 440;
const TITLE_H: u32 = 32;
const FIELD_Y: u32 = 40;
const FIELD_H: u32 = 52;
const FIELD_PAD: u32 = 16;
const FIELD_PX: f32 = 26.0;
const CARET_W: u32 = 10;
const KEYS_Y: u32 = 108;
const KEY_W: u32 = 62;
const KEY_H: u32 = 52;
const GAP: u32 = 7;
const RADIUS: u32 = 8;
const KEY_PX: f32 = 22.0;
const HINTS_H: u32 = 30;

const KEY: [u8; 4] = [0x2c, 0x2c, 0x32, 0xf0];
const ARMED: [u8; 4] = [0x4a, 0x4a, 0x56, 0xf8];
const FIELD: [u8; 4] = [0x22, 0x22, 0x28, 0xf0];

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(k: &mut Keyboard, keys: &[KeyInput]) -> KeyOutcome {
        let mut last = KeyOutcome::Typing;
        for key in keys {
            last = k.press(*key);
        }
        last
    }

    #[test]
    fn it_opens_on_the_first_letter_and_types_it() {
        let mut k = Keyboard::new("Password", "", true, 0, 63);
        assert_eq!(k.key(), Key::Char('q'));
        k.press(KeyInput::Pick);
        assert_eq!(k.text(), "q");
    }

    #[test]
    fn the_cursor_wraps_round_every_edge() {
        let mut k = Keyboard::new("", "", false, 0, 63);
        k.press(KeyInput::Left);
        assert_eq!(
            k.key(),
            Key::Char('p'),
            "left off the first key did not wrap"
        );
        k.press(KeyInput::Up);
        k.press(KeyInput::Up);
        assert_eq!(
            k.cursor().0,
            4,
            "up off the digits did not wrap to the bottom row"
        );
        k.press(KeyInput::Down);
        assert_eq!(k.cursor().0, 0);
    }

    #[test]
    fn the_bottom_row_steps_key_by_key_and_keeps_the_column_going_up() {
        let mut k = Keyboard::new("", "", false, 0, 63);
        for _ in 0..4 {
            k.press(KeyInput::Right);
        }
        k.press(KeyInput::Down);
        k.press(KeyInput::Down);
        k.press(KeyInput::Down);
        assert_eq!(
            k.key(),
            Key::Special(Special::Space),
            "column 4 is not over space"
        );
        k.press(KeyInput::Right);
        assert_eq!(k.key(), Key::Special(Special::Delete));
        k.press(KeyInput::Right);
        assert_eq!(k.key(), Key::Special(Special::Done));
        k.press(KeyInput::Right);
        assert_eq!(
            k.key(),
            Key::Special(Special::Shift),
            "the bottom row did not wrap"
        );
        k.press(KeyInput::Up);
        assert_eq!(k.key(), Key::Char('z'));
    }

    #[test]
    fn shift_is_once_then_caps_then_off() {
        let mut k = Keyboard::new("", "", false, 0, 63);
        k.press(KeyInput::Shift);
        assert_eq!(k.key(), Key::Char('Q'));
        typed(&mut k, &[KeyInput::Pick, KeyInput::Pick]);
        assert_eq!(k.text(), "Qq", "shift did not drop back after one letter");
        typed(
            &mut k,
            &[
                KeyInput::Shift,
                KeyInput::Shift,
                KeyInput::Pick,
                KeyInput::Pick,
            ],
        );
        assert_eq!(k.text(), "QqQQ", "caps lock did not hold");
        k.press(KeyInput::Shift);
        k.press(KeyInput::Pick);
        assert_eq!(k.text(), "QqQQq");
    }

    #[test]
    fn the_symbols_page_holds_every_printable_symbol_with_the_letters_page() {
        let all: String = LETTERS.concat() + &SYMBOLS.concat();
        for ch in (0x21u8..0x7f)
            .map(char::from)
            .filter(|c| !c.is_ascii_alphanumeric())
        {
            assert!(all.contains(ch), "{ch:?} cannot be typed");
        }
        for row in LETTERS.iter().chain(SYMBOLS.iter()) {
            assert_eq!(row.chars().count(), COLS, "{row:?} is not a full row");
        }
    }

    #[test]
    fn b_deletes_and_never_cancels() {
        let mut k = Keyboard::new("", "abc", true, 0, 63);
        assert_eq!(k.press(KeyInput::Delete), KeyOutcome::Typing);
        assert_eq!(k.text(), "ab");
        typed(
            &mut k,
            &[KeyInput::Delete, KeyInput::Delete, KeyInput::Delete],
        );
        assert_eq!(k.text(), "", "delete past the start broke something");
        assert_eq!(k.press(KeyInput::Cancel), KeyOutcome::Cancelled);
    }

    #[test]
    fn done_waits_for_the_shortest_text_it_takes() {
        let mut k = Keyboard::new("", "short", true, 8, 63);
        assert_eq!(
            k.press(KeyInput::Done),
            KeyOutcome::Typing,
            "a 5 letter WPA key went"
        );
        typed(&mut k, &[KeyInput::Pick, KeyInput::Pick, KeyInput::Pick]);
        assert_eq!(k.press(KeyInput::Done), KeyOutcome::Done("shortqqq".into()));
    }

    #[test]
    fn nothing_is_typed_past_the_longest_text_it_takes() {
        let mut k = Keyboard::new("", "", false, 0, 3);
        for _ in 0..5 {
            k.press(KeyInput::Pick);
        }
        k.press(KeyInput::Space);
        assert_eq!(k.text(), "qqq");
    }

    #[test]
    fn a_secret_is_starred_until_select_shows_it() {
        let mut k = Keyboard::new("", "hunter22", true, 0, 63);
        assert_eq!(k.shown(), "********");
        k.press(KeyInput::Reveal);
        assert_eq!(k.shown(), "hunter22");
        let mut name = Keyboard::new("", "home", false, 0, 32);
        name.press(KeyInput::Reveal);
        assert_eq!(name.shown(), "home", "a name that is no secret was hidden");
    }

    #[test]
    fn the_face_is_drawn_with_its_cursor_lit() {
        let k = Keyboard::new("Password for home", "abc", true, 8, 63);
        let face = k.face();
        assert_eq!((face.w, face.h), (FACE_W, FACE_H));
        let at = |x: u32, y: u32| &face.rgba[((y * FACE_W + x) * 4) as usize..][..4];
        // The cursor starts on q, row 1 column 0; its middle is lit.
        let y = KEYS_Y + KEY_H + GAP + 4;
        assert_eq!(at(4, y), &LIT, "the lit key is not lit");
        assert_eq!(at(KEY_W + GAP + 4, y), &KEY, "the key beside it is lit too");
    }
}
