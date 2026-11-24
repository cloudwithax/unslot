//! The built-in apps' own screens: a file manager, a gallery and its viewer, a library of books
//! and its reader, and a music player. Each is drawn whole, a window of its own over the
//! wallpaper with its name, its colour and its icons, each time what it shows changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::list_page::{LIST_H, LIST_W};
use crate::page::{legends_into, over, Legend, DIM, INK};
use crate::plate::UndoFace;
use crate::text;

const W: u32 = LIST_W;
const H: u32 = LIST_H;
const HEAD_H: u32 = 56;
const FOOT_H: u32 = 44;
const PAD: u32 = 16;

const WINDOW: [u8; 4] = [0x16, 0x16, 0x1b, 0xf6];
const PANEL: [u8; 4] = [0x22, 0x22, 0x2a, 0xff];
const RULE: [u8; 4] = [0x2e, 0x2e, 0x38, 0xff];
const FAINT: [u8; 4] = [0x6a, 0x6a, 0x76, 0xff];
const DARK: [u8; 4] = [0x14, 0x12, 0x10, 0xff];
const PAPER: [u8; 4] = [0xf2, 0xec, 0xdf, 0xff];
const PAPER_INK: [u8; 4] = [0x2b, 0x26, 0x20, 0xff];

/// Each app's colour.
pub const FILES_ACCENT: [u8; 3] = [0xf0, 0xa2, 0x3a];
pub const GALLERY_ACCENT: [u8; 3] = [0x4c, 0xa8, 0xf0];
pub const MUSIC_ACCENT: [u8; 3] = [0xe8, 0x47, 0x3f];
pub const BOOKS_ACCENT: [u8; 3] = [0x5f, 0xc8, 0x7a];
pub const MANUALS_ACCENT: [u8; 3] = [0xa8, 0x80, 0xf0];

/// The apps' icons, from the symbols font.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Glyph {
    Folder,
    FolderOpen,
    File,
    Picture,
    Video,
    Audio,
    Text,
    Pdf,
    Book,
    Script,
    Archive,
    Game,
    Play,
    Pause,
    Next,
    Previous,
    Card,
    Copy,
    Cut,
    Paste,
    Delete,
    Rename,
    NewFolder,
    Album,
    Check,
    Chevron,
    PlayCircle,
    Film,
    Pictures,
    Music,
    Shelf,
    UpFolder,
    Alert,
    Close,
}

impl Glyph {
    fn char(self) -> char {
        match self {
            Glyph::Folder => '\u{f024b}',
            Glyph::FolderOpen => '\u{f0770}',
            Glyph::File => '\u{f0214}',
            Glyph::Picture => '\u{f02e9}',
            Glyph::Video => '\u{f022b}',
            Glyph::Audio => '\u{f0223}',
            Glyph::Text => '\u{f0219}',
            Glyph::Pdf => '\u{f0226}',
            Glyph::Book => '\u{f05da}',
            Glyph::Script => '\u{f018d}',
            Glyph::Archive => '\u{f05c4}',
            Glyph::Game => '\u{f0297}',
            Glyph::Play => '\u{f040a}',
            Glyph::Pause => '\u{f03e4}',
            Glyph::Next => '\u{f04ad}',
            Glyph::Previous => '\u{f04ae}',
            Glyph::Card => '\u{f0479}',
            Glyph::Copy => '\u{f018f}',
            Glyph::Cut => '\u{f0190}',
            Glyph::Paste => '\u{f0192}',
            Glyph::Delete => '\u{f01b4}',
            Glyph::Rename => '\u{f03eb}',
            Glyph::NewFolder => '\u{f0257}',
            Glyph::Album => '\u{f0025}',
            Glyph::Check => '\u{f012c}',
            Glyph::Chevron => '\u{f0142}',
            Glyph::PlayCircle => '\u{f040c}',
            Glyph::Film => '\u{f0230}',
            Glyph::Pictures => '\u{f02f9}',
            Glyph::Music => '\u{f075a}',
            Glyph::Shelf => '\u{f125f}',
            Glyph::UpFolder => '\u{f19f0}',
            Glyph::Alert => '\u{f05d6}',
            Glyph::Close => '\u{f0156}',
        }
    }
}

/// What a file is, for its icon and its colour.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum FileIcon {
    Folder,
    Picture,
    Video,
    Audio,
    Text,
    Pdf,
    Book,
    Script,
    Archive,
    Game,
    #[default]
    Other,
}

impl FileIcon {
    pub fn glyph(self) -> Glyph {
        match self {
            FileIcon::Folder => Glyph::Folder,
            FileIcon::Picture => Glyph::Picture,
            FileIcon::Video => Glyph::Video,
            FileIcon::Audio => Glyph::Audio,
            FileIcon::Text => Glyph::Text,
            FileIcon::Pdf => Glyph::Pdf,
            FileIcon::Book => Glyph::Book,
            FileIcon::Script => Glyph::Script,
            FileIcon::Archive => Glyph::Archive,
            FileIcon::Game => Glyph::Game,
            FileIcon::Other => Glyph::File,
        }
    }

    pub fn tint(self) -> [u8; 3] {
        match self {
            FileIcon::Folder => FILES_ACCENT,
            FileIcon::Picture => GALLERY_ACCENT,
            FileIcon::Video => [0xb0, 0x88, 0xf0],
            FileIcon::Audio => MUSIC_ACCENT,
            FileIcon::Text => [0x9a, 0xb0, 0xc8],
            FileIcon::Pdf => [0xf0, 0x64, 0x54],
            FileIcon::Book => BOOKS_ACCENT,
            FileIcon::Script => [0xb8, 0xe0, 0x50],
            FileIcon::Archive => [0xd8, 0xc0, 0x80],
            FileIcon::Game => [0xf0, 0x80, 0xb8],
            FileIcon::Other => [0x9a, 0x9a, 0xa4],
        }
    }
}

/// A row of the file manager.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FileRow {
    pub name: String,
    pub icon: FileIcon,
    /// "1.2 MB", or how many things a folder holds.
    pub size: String,
    pub date: String,
    /// Cut, waiting to be pasted somewhere else.
    pub faded: bool,
}

/// A small list over the page of what can be done: the file manager's actions.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Menu {
    pub title: String,
    /// Each action's icon, its name, and whether it can be done here.
    pub rows: Vec<(Glyph, String, bool)>,
    pub cursor: usize,
}

/// A box over the page asking before something is done, or showing it being done.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Dialog {
    pub title: String,
    pub body: String,
    /// How far along, for one under way.
    pub progress: Option<f32>,
    pub legends: Vec<Legend>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct FilesPage {
    pub title: String,
    /// Where it is, from the card down: "Card", "Games", "GBA".
    pub crumbs: Vec<String>,
    pub rows: Vec<FileRow>,
    pub cursor: usize,
    /// Under the list on the left: the room left, or what was copied.
    pub status: String,
    /// In the header on the right: how many things are here.
    pub meta: String,
    pub legends: Vec<Legend>,
    pub menu: Option<Menu>,
    pub dialog: Option<Dialog>,
}

/// A tile of the gallery or the library: a picture to show on it, or else its icon.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Tile {
    pub name: String,
    pub icon: FileIcon,
    pub picture: Option<PathBuf>,
    /// Under a book: its kind and how far into it.
    pub detail: String,
    /// How much of a book has been read.
    pub read: Option<u8>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct GalleryPage {
    pub title: String,
    pub meta: String,
    pub tiles: Vec<Tile>,
    pub cursor: usize,
    pub status: String,
    pub legends: Vec<Legend>,
}

/// A picture on its own, as large as the screen allows.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ViewerPage {
    pub name: String,
    pub path: PathBuf,
    /// Which of how many: "2 / 3".
    pub place: String,
    /// The name and the buttons over the picture; A hides them.
    pub chrome: bool,
    pub legends: Vec<Legend>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct LibraryPage {
    pub title: String,
    pub accent: [u8; 3],
    pub meta: String,
    pub tiles: Vec<Tile>,
    pub cursor: usize,
    pub status: String,
    pub legends: Vec<Legend>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct ReaderPage {
    pub title: String,
    pub accent: [u8; 3],
    pub page: usize,
    pub pages: usize,
    /// A page of text, or the picture of a PDF's page, or why it will not show.
    pub lines: Vec<String>,
    pub picture: Option<PathBuf>,
    pub error: Option<String>,
    pub legends: Vec<Legend>,
}

/// A track's cover: from the file itself, or a picture beside it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Cover {
    Bytes(Arc<Vec<u8>>),
    File(PathBuf),
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct MusicPage {
    pub title: String,
    pub artist: String,
    pub cover: Option<Cover>,
    pub secs: u32,
    pub total: Option<u32>,
    /// Whether a track is in, and whether it is going.
    pub loaded: bool,
    pub paused: bool,
    /// The heading over the tracks: "Up next", "All music".
    pub heading: String,
    pub tracks: Vec<String>,
    /// The one playing among them.
    pub now: Option<usize>,
    pub cursor: usize,
    pub status: String,
    pub legends: Vec<Legend>,
}

/// What a video shows over its picture while a button has been pressed lately, or it is paused.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct VideoOsd {
    pub title: String,
    pub secs: u32,
    pub total: Option<u32>,
    pub paused: bool,
    pub legends: Vec<Legend>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum AppPage {
    Files(FilesPage),
    Gallery(GalleryPage),
    Viewer(ViewerPage),
    Library(LibraryPage),
    Reader(ReaderPage),
    Music(MusicPage),
}

impl AppPage {
    pub fn face(&self) -> UndoFace {
        let mut s = Sheet::new(W, H);
        match self {
            AppPage::Files(p) => p.draw(&mut s),
            AppPage::Gallery(p) => p.draw(&mut s),
            AppPage::Viewer(p) => p.draw(&mut s),
            AppPage::Library(p) => p.draw(&mut s),
            AppPage::Reader(p) => p.draw(&mut s),
            AppPage::Music(p) => p.draw(&mut s),
        }
        s.face()
    }
}

/// How a reader's page of text is laid out, for the Reader to cut a book into pages.
pub const READER_PX: f32 = 17.0;
const READER_LINE_H: u32 = 23;
const PAPER_RECT: (u32, u32, u32, u32) = (PAD, HEAD_H - 4, W - 2 * PAD, H - HEAD_H - FOOT_H + 4);
const PAPER_MARGIN: u32 = 26;
pub const READER_TEXT_W: u32 = PAPER_RECT.2 - 2 * PAPER_MARGIN;
pub const READER_LINES: usize = ((PAPER_RECT.3 - 2 * 14) / READER_LINE_H) as usize;
/// The room a PDF's page is drawn to fit.
pub const READER_PAGE_W: u32 = PAPER_RECT.2;
pub const READER_PAGE_H: u32 = PAPER_RECT.3;

/// `text` in lines a reader's page wide.
pub fn wrap_reader(text: &str) -> Vec<String> {
    match text::label_font() {
        Some(font) => text::wrap_as_written(font, text, READER_TEXT_W as f32 - 4.0, READER_PX),
        None => text.lines().map(str::to_string).collect(),
    }
}

// The window every app is drawn in: its icon and name across the top, and a rule over its
// footer.
fn chrome(s: &mut Sheet, accent: [u8; 3], glyph: Glyph, title: &str, meta: &str) {
    s.round((0, 0, W, H), 16, WINDOW);
    s.round((PAD, 10, 36, 36), 10, opaque(accent));
    s.glyph(glyph, PAD + 18, 28, 22.0, DARK);
    s.text((PAD + 48, 8, 360, 40), title, INK, 20.0, Align::Left);
    s.text((W - PAD - 240, 8, 240, 40), meta, DIM, 14.0, Align::Right);
    s.fill((0, HEAD_H, W, 1), RULE);
}

fn footer(s: &mut Sheet, status: &str, legends: &[Legend]) {
    let y = H - FOOT_H;
    s.fill((0, y, W, 1), RULE);
    s.text((PAD, y, 220, FOOT_H), status, DIM, 14.0, Align::Left);
    legends_into(&mut s.px, W, (240, y + 2, W - 240 - PAD, FOOT_H - 2), legends, INK);
}

fn opaque(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 0xff]
}

fn tinted(c: [u8; 3], a: u8) -> [u8; 4] {
    [c[0], c[1], c[2], a]
}

/// The first of `count` things in view, `shown` at a time, keeping `cursor` in view and as near
/// the middle as the ends allow; in steps of `step`, for a grid's rows.
fn first_shown(count: usize, cursor: usize, shown: usize, step: usize) -> usize {
    let (rows, at, shown_rows) = (count.div_ceil(step), cursor / step, shown / step);
    if rows <= shown_rows {
        return 0;
    }
    at.saturating_sub(shown_rows.saturating_sub(1) / 2).min(rows - shown_rows) * step
}

const FILE_ROW_H: u32 = 42;
const FILE_ROWS: usize = 7;
const CRUMB_H: u32 = 36;

impl FilesPage {
    pub fn first_shown(&self) -> usize {
        first_shown(self.rows.len(), self.cursor, FILE_ROWS, 1)
    }

    fn draw(&self, s: &mut Sheet) {
        chrome(s, FILES_ACCENT, Glyph::Folder, &self.title, &self.meta);
        self.crumbs(s);
        let top = HEAD_H + CRUMB_H + 4;
        if self.rows.is_empty() {
            s.glyph(Glyph::FolderOpen, W / 2, top + 100, 64.0, FAINT);
            s.text((0, top + 140, W, 30), "This folder is empty", DIM, 16.0, Align::Centre);
        }
        let first = self.first_shown();
        for (slot, (at, row)) in self.rows.iter().enumerate().skip(first).take(FILE_ROWS).enumerate() {
            let y = top + slot as u32 * FILE_ROW_H;
            if at == self.cursor {
                s.round((8, y + 1, W - 30, FILE_ROW_H - 2), 9, tinted(FILES_ACCENT, 0x30));
                s.round((8, y + 10, 4, FILE_ROW_H - 20), 2, opaque(FILES_ACCENT));
            }
            let ink = if row.faded { FAINT } else { INK };
            let tint = if row.faded { FAINT } else { opaque(row.icon.tint()) };
            s.glyph(row.icon.glyph(), 36, y + FILE_ROW_H / 2, 22.0, tint);
            s.text((60, y, 360, FILE_ROW_H), &row.name, ink, 16.0, Align::Left);
            s.text((430, y, 96, FILE_ROW_H), &row.size, DIM, 13.0, Align::Right);
            s.text((532, y, 110, FILE_ROW_H), &row.date, DIM, 13.0, Align::Right);
        }
        s.scrollbar(
            (W - 14, top + 4, 4, FILE_ROWS as u32 * FILE_ROW_H - 8),
            first,
            FILE_ROWS,
            self.rows.len(),
            FILES_ACCENT,
        );
        footer(s, &self.status, &self.legends);
        if let Some(menu) = &self.menu {
            menu.draw(s, FILES_ACCENT);
        }
        if let Some(dialog) = &self.dialog {
            dialog.draw(s, FILES_ACCENT);
        }
    }

    /// The path as a row of folder names with arrows between, the start dropped for one too
    /// long to fit.
    fn crumbs(&self, s: &mut Sheet) {
        let (y, px) = (HEAD_H + 1, 15.0);
        s.fill((0, y, W, CRUMB_H), [0x1b, 0x1b, 0x21, 0xff]);
        s.fill((0, y + CRUMB_H, W, 1), RULE);
        s.glyph(Glyph::Card, PAD + 8, y + CRUMB_H / 2, 16.0, DIM);
        let room = (W - 2 * PAD - 30) as f32;
        let width = |c: &str| s.text_width(c, px) + 26.0;
        let mut from = 0;
        while from + 1 < self.crumbs.len()
            && self.crumbs[from..].iter().map(|c| width(c)).sum::<f32>() + 30.0 > room
        {
            from += 1;
        }
        let mut x = PAD as f32 + 24.0;
        if from > 0 {
            s.text((x as u32, y, 30, CRUMB_H), "...", DIM, px, Align::Left);
            x += 26.0;
        }
        for (i, crumb) in self.crumbs.iter().enumerate().skip(from) {
            if i > from || from > 0 {
                s.glyph(Glyph::Chevron, x as u32 + 6, y + CRUMB_H / 2, 14.0, FAINT);
                x += 14.0;
            }
            let last = i + 1 == self.crumbs.len();
            let w = (s.text_width(crumb, px) + 12.0).min(W as f32 - PAD as f32 - x).max(0.0);
            let ink = if last { INK } else { DIM };
            s.text((x as u32, y, w as u32, CRUMB_H), crumb, ink, px, Align::Left);
            x += w + 4.0;
        }
    }
}

impl Menu {
    fn draw(&self, s: &mut Sheet, accent: [u8; 3]) {
        const ROW: u32 = 34;
        let (w, h) = (236, 46 + self.rows.len() as u32 * ROW);
        let (x, y) = (W - PAD - w, HEAD_H + 10);
        s.round((x - 1, y - 1, w + 2, h + 2), 12, RULE);
        s.round((x, y, w, h), 11, PANEL);
        s.text((x + 16, y + 4, w - 32, 34), &self.title, DIM, 13.0, Align::Left);
        for (i, (glyph, label, can)) in self.rows.iter().enumerate() {
            let ry = y + 40 + i as u32 * ROW;
            if i == self.cursor {
                s.round((x + 6, ry, w - 12, ROW), 8, tinted(accent, 0x38));
            }
            let ink = if *can { INK } else { FAINT };
            let tint = if *can { opaque(accent) } else { FAINT };
            s.glyph(*glyph, x + 28, ry + ROW / 2, 18.0, tint);
            s.text((x + 46, ry, w - 56, ROW), label, ink, 15.0, Align::Left);
        }
    }
}

impl Dialog {
    fn draw(&self, s: &mut Sheet, accent: [u8; 3]) {
        s.fill((0, 0, W, H), [0, 0, 0, 0x90]);
        let (w, h) = (440, 190);
        let (x, y) = ((W - w) / 2, (H - h) / 2);
        s.round((x - 1, y - 1, w + 2, h + 2), 15, RULE);
        s.round((x, y, w, h), 14, PANEL);
        s.text((x + 24, y + 14, w - 48, 34), &self.title, INK, 19.0, Align::Left);
        s.wrapped((x + 24, y + 52, w - 48, 66), &self.body, DIM, 15.0, 3);
        if let Some(done) = self.progress {
            s.bar((x + 24, y + 124, w - 48, 6), done, accent);
        }
        legends_into(&mut s.px, W, (x, y + h - 48, w, 40), &self.legends, INK);
    }
}

const TILE_W: u32 = 152;
const TILE_H: u32 = 98;
const TILE_GAP: u32 = 12;
const TILE_COLS: usize = 4;
const TILE_ROWS: usize = 3;

impl GalleryPage {
    fn draw(&self, s: &mut Sheet) {
        chrome(s, GALLERY_ACCENT, Glyph::Pictures, &self.title, &self.meta);
        if self.tiles.is_empty() {
            s.glyph(Glyph::Pictures, W / 2, 190, 64.0, FAINT);
            s.text((0, 232, W, 30), "No pictures or videos here", DIM, 16.0, Align::Centre);
        }
        let shown = TILE_COLS * TILE_ROWS;
        let first = first_shown(self.tiles.len(), self.cursor, shown, TILE_COLS);
        for (slot, (at, tile)) in self.tiles.iter().enumerate().skip(first).take(shown).enumerate() {
            let (col, row) = ((slot % TILE_COLS) as u32, (slot / TILE_COLS) as u32);
            let x = PAD + col * (TILE_W + TILE_GAP);
            let y = HEAD_H + 12 + row * (TILE_H + TILE_GAP);
            if at == self.cursor {
                s.round((x - 4, y - 4, TILE_W + 8, TILE_H + 8), 13, opaque(GALLERY_ACCENT));
            }
            s.round((x, y, TILE_W, TILE_H), 10, PANEL);
            let thumb = tile.picture.as_deref().and_then(|p| thumb(p, TILE_W, TILE_H));
            match &thumb {
                Some(px) => s.image_round(px, TILE_W, TILE_H, x, y, 10),
                None => s.glyph(tile.icon.glyph(), x + TILE_W / 2, y + TILE_H / 2 - 10, 40.0, opaque(tile.icon.tint())),
            }
            match tile.icon {
                FileIcon::Video => {
                    s.round((x + TILE_W - 34, y + 6, 28, 28), 14, [0, 0, 0, 0xa0]);
                    s.glyph(Glyph::Play, x + TILE_W - 20, y + 20, 18.0, INK);
                }
                FileIcon::Folder if thumb.is_some() => {
                    s.round((x + 6, y + 6, 28, 28), 8, [0, 0, 0, 0xa0]);
                    s.glyph(Glyph::Folder, x + 20, y + 20, 18.0, opaque(FILES_ACCENT));
                }
                _ => {}
            }
            s.shade_in((x, y + TILE_H - 30, TILE_W, 30), (x, y, TILE_W, TILE_H), 0, 0xc8, 10);
            s.text((x + 8, y + TILE_H - 28, TILE_W - 16, 26), &tile.name, INK, 13.0, Align::Left);
        }
        let rows = self.tiles.len().div_ceil(TILE_COLS);
        s.scrollbar(
            (W - 7, HEAD_H + 12, 3, TILE_ROWS as u32 * (TILE_H + TILE_GAP) - TILE_GAP),
            first / TILE_COLS,
            TILE_ROWS,
            rows,
            GALLERY_ACCENT,
        );
        footer(s, &self.status, &self.legends);
    }
}

impl ViewerPage {
    fn draw(&self, s: &mut Sheet) {
        s.round((0, 0, W, H), 16, [0x08, 0x08, 0x0a, 0xff]);
        let shown = crate::art::decode_near(&self.path, W, H)
            .and_then(|(src, sw, sh)| crate::art::fit_rgba(&src, sw, sh, W, H));
        match shown {
            Some((px, fw, fh)) => s.image(&px, fw, fh, (W - fw) / 2, (H - fh) / 2),
            None => {
                s.glyph(Glyph::Alert, W / 2, H / 2 - 20, 48.0, FAINT);
                s.text((0, H / 2 + 16, W, 30), "This picture will not open", DIM, 16.0, Align::Centre);
            }
        }
        if !self.chrome {
            return;
        }
        s.shade((0, 0, W, 64), 0xc0, 0, 16);
        s.text((PAD + 4, 6, W - 160, 40), &self.name, INK, 18.0, Align::Left);
        s.text((W - PAD - 130, 6, 126, 40), &self.place, DIM, 15.0, Align::Right);
        s.shade((0, H - 60, W, 60), 0, 0xc8, 16);
        legends_into(&mut s.px, W, (0, H - FOOT_H, W, FOOT_H - 4), &self.legends, INK);
    }
}

const BOOK_W: u32 = 92;
const BOOK_H: u32 = 122;
const BOOK_CAPTION: u32 = 36;
const BOOK_COLS: usize = 6;
const BOOK_ROWS: usize = 2;

impl LibraryPage {
    fn draw(&self, s: &mut Sheet) {
        chrome(s, self.accent, Glyph::Shelf, &self.title, &self.meta);
        if self.tiles.is_empty() {
            s.glyph(Glyph::Shelf, W / 2, 190, 64.0, FAINT);
            s.text((0, 232, W, 30), "Nothing to read here yet", DIM, 16.0, Align::Centre);
        }
        let gap = (W - 2 * PAD - BOOK_COLS as u32 * BOOK_W) / (BOOK_COLS as u32 - 1);
        let shown = BOOK_COLS * BOOK_ROWS;
        let first = first_shown(self.tiles.len(), self.cursor, shown, BOOK_COLS);
        for (slot, (at, tile)) in self.tiles.iter().enumerate().skip(first).take(shown).enumerate() {
            let (col, row) = ((slot % BOOK_COLS) as u32, (slot / BOOK_COLS) as u32);
            let x = PAD + col * (BOOK_W + gap);
            let y = HEAD_H + 10 + row * (BOOK_H + BOOK_CAPTION + 6);
            if at == self.cursor {
                s.round((x - 4, y - 4, BOOK_W + 8, BOOK_H + 8), 9, opaque(self.accent));
            }
            match tile.icon {
                FileIcon::Folder => {
                    s.round((x, y, BOOK_W, BOOK_H), 6, PANEL);
                    s.glyph(Glyph::Folder, x + BOOK_W / 2, y + BOOK_H / 2, 44.0, opaque(FILES_ACCENT));
                }
                _ => match tile.picture.as_deref().and_then(|p| thumb(p, BOOK_W, BOOK_H)) {
                    Some(px) => s.image_round(&px, BOOK_W, BOOK_H, x, y, 5),
                    None => s.made_cover((x, y, BOOK_W, BOOK_H), &tile.name),
                },
            }
            if let Some(read) = tile.read {
                s.round((x + 6, y + BOOK_H - 9, BOOK_W - 12, 4), 2, [0, 0, 0, 0xb0]);
                let fill = ((BOOK_W - 12) * u32::from(read.min(100)) / 100).max(4);
                s.round((x + 6, y + BOOK_H - 9, fill, 4), 2, opaque(self.accent));
            }
            let ink = if at == self.cursor { INK } else { [0xd6, 0xd4, 0xcf, 0xff] };
            s.text((x - 4, y + BOOK_H + 3, BOOK_W + 8, 18), &tile.name, ink, 13.0, Align::Centre);
            s.text((x - 4, y + BOOK_H + 19, BOOK_W + 8, 14), &tile.detail, DIM, 11.0, Align::Centre);
        }
        let rows = self.tiles.len().div_ceil(BOOK_COLS);
        s.scrollbar(
            (W - 7, HEAD_H + 10, 3, BOOK_ROWS as u32 * (BOOK_H + BOOK_CAPTION + 6) - 6),
            first / BOOK_COLS,
            BOOK_ROWS,
            rows,
            self.accent,
        );
        footer(s, &self.status, &self.legends);
    }
}

impl ReaderPage {
    fn draw(&self, s: &mut Sheet) {
        s.round((0, 0, W, H), 16, WINDOW);
        s.text((PAD + 4, 4, W - 220, 40), &self.title, INK, 17.0, Align::Left);
        let place = format!("Page {} of {}", self.page + 1, self.pages.max(1));
        s.text((W - PAD - 190, 4, 186, 40), &place, DIM, 14.0, Align::Right);
        let (x, y, w, h) = PAPER_RECT;
        match (&self.error, &self.picture) {
            (Some(e), _) => {
                s.glyph(Glyph::Alert, W / 2, y + h / 2 - 24, 48.0, FAINT);
                s.wrapped((x + 40, y + h / 2 + 8, w - 80, 60), e, DIM, 15.0, 2);
            }
            (None, Some(path)) => {
                let shown = crate::art::decode_near(path, w, h)
                    .and_then(|(src, sw, sh)| crate::art::fit_rgba(&src, sw, sh, w, h));
                if let Some((px, fw, fh)) = shown {
                    s.image(&px, fw, fh, x + (w - fw) / 2, y + (h - fh) / 2);
                }
            }
            (None, None) => {
                s.round((x, y, w, h), 6, PAPER);
                for (i, line) in self.lines.iter().take(READER_LINES).enumerate() {
                    let ly = y + 14 + i as u32 * READER_LINE_H;
                    s.text((x + PAPER_MARGIN, ly, READER_TEXT_W + 8, READER_LINE_H), line, PAPER_INK, READER_PX, Align::Left);
                }
            }
        }
        let fy = H - FOOT_H;
        let done = (self.page + 1) as f32 / self.pages.max(1) as f32;
        s.bar((PAD, fy + 4, W - 2 * PAD, 4), done, self.accent);
        s.text((PAD, fy + 8, 120, FOOT_H - 8), &format!("{}%", (done * 100.0).round()), DIM, 14.0, Align::Left);
        legends_into(&mut s.px, W, (140, fy + 8, W - 280, FOOT_H - 8), &self.legends, INK);
    }
}

const ART: u32 = 196;
const TRACK_ROW_H: u32 = 38;
const TRACK_ROWS: usize = 7;

impl MusicPage {
    pub fn first_shown(&self) -> usize {
        first_shown(self.tracks.len(), self.cursor, TRACK_ROWS, 1)
    }

    fn draw(&self, s: &mut Sheet) {
        chrome(s, MUSIC_ACCENT, Glyph::Music, "Music", &self.meta());
        let pane = 300;
        s.fill((pane, HEAD_H + 1, 1, H - HEAD_H - FOOT_H - 1), RULE);
        let (ax, ay) = ((pane - ART) / 2, HEAD_H + 14);
        match self.cover.as_ref().and_then(|c| cover(c, ART)) {
            Some(px) => s.image_round(&px, ART, ART, ax, ay, 12),
            None => {
                s.round((ax, ay, ART, ART), 12, PANEL);
                s.glyph(Glyph::Album, ax + ART / 2, ay + ART / 2, 84.0, tinted(MUSIC_ACCENT, 0xc0));
            }
        }
        let title = if self.loaded { self.title.as_str() } else { "Nothing playing" };
        let artist = if self.loaded { self.artist.as_str() } else { "Press A on a track to play it" };
        s.text((12, ay + ART + 6, pane - 24, 26), title, INK, 18.0, Align::Centre);
        s.text((12, ay + ART + 30, pane - 24, 20), artist, DIM, 13.0, Align::Centre);
        let (bx, by, bw) = (34, ay + ART + 60, pane - 68);
        let done = match self.total {
            Some(t) if t > 0 => self.secs as f32 / t as f32,
            _ => 0.0,
        };
        s.bar((bx, by, bw, 4), done, MUSIC_ACCENT);
        if self.loaded {
            s.disc(bx + (bw as f32 * done.min(1.0)) as u32, by + 2, 6, opaque(MUSIC_ACCENT));
        }
        let total = self.total.map(clock).unwrap_or_default();
        s.text((bx - 4, by + 8, 80, 18), &clock(self.secs), DIM, 12.0, Align::Left);
        s.text((bx + bw - 76, by + 8, 80, 18), &total, DIM, 12.0, Align::Right);
        let ty = by + 46;
        let cx = pane / 2;
        s.glyph(Glyph::Previous, cx - 60, ty, 26.0, INK);
        s.disc(cx, ty, 20, opaque(MUSIC_ACCENT));
        let go = if self.loaded && !self.paused { Glyph::Pause } else { Glyph::Play };
        let nudge = if matches!(go, Glyph::Play) { 2 } else { 0 };
        s.glyph(go, cx + nudge, ty, 26.0, INK);
        s.glyph(Glyph::Next, cx + 60, ty, 26.0, INK);

        let lx = pane + 12;
        s.text((lx + 4, HEAD_H + 6, 200, 26), &self.heading.to_uppercase(), DIM, 12.0, Align::Left);
        let top = HEAD_H + 34;
        if self.tracks.is_empty() {
            s.wrapped((lx + 4, top + 100, W - lx - 24, 60), &self.status, DIM, 15.0, 2);
        }
        let first = self.first_shown();
        for (slot, (at, track)) in self.tracks.iter().enumerate().skip(first).take(TRACK_ROWS).enumerate() {
            let y = top + slot as u32 * TRACK_ROW_H;
            if at == self.cursor {
                s.round((lx, y + 1, W - lx - 22, TRACK_ROW_H - 2), 8, tinted(MUSIC_ACCENT, 0x30));
            }
            let now = self.now == Some(at);
            match now {
                true => s.glyph(if self.paused { Glyph::Pause } else { Glyph::Music }, lx + 20, y + TRACK_ROW_H / 2, 16.0, opaque(MUSIC_ACCENT)),
                false => s.text((lx + 2, y, 36, TRACK_ROW_H), &(at + 1).to_string(), FAINT, 13.0, Align::Centre),
            }
            let ink = if now { opaque(MUSIC_ACCENT) } else { INK };
            s.text((lx + 40, y, W - lx - 70, TRACK_ROW_H), track, ink, 15.0, Align::Left);
        }
        s.scrollbar(
            (W - 12, top + 2, 3, TRACK_ROWS as u32 * TRACK_ROW_H - 4),
            first,
            TRACK_ROWS,
            self.tracks.len(),
            MUSIC_ACCENT,
        );
        let status = if self.tracks.is_empty() { "" } else { &self.status };
        footer(s, status, &self.legends);
    }

    fn meta(&self) -> String {
        match self.tracks.len() {
            1 => "1 track".into(),
            n => format!("{n} tracks"),
        }
    }
}

/// The video's picture with its title, where it is and its buttons over it.
pub fn video_osd(frame: &[u8], osd: &VideoOsd) -> UndoFace {
    let mut s = Sheet {
        px: frame.to_vec(),
        w: W,
        h: H,
    };
    s.shade((0, 0, W, 70), 0xb0, 0, 12);
    s.text((PAD + 4, 6, W - 2 * PAD, 40), &osd.title, INK, 18.0, Align::Left);
    s.shade((0, H - 96, W, 96), 0, 0xd8, 12);
    let (bx, by, bw) = (78, H - 66, W - 156);
    let done = match osd.total {
        Some(t) if t > 0 => osd.secs as f32 / t as f32,
        _ => 0.0,
    };
    s.bar((bx, by, bw, 4), done, MUSIC_ACCENT);
    s.disc(bx + (bw as f32 * done.min(1.0)) as u32, by + 2, 6, opaque(MUSIC_ACCENT));
    s.text((PAD, by - 12, 58, 28), &clock(osd.secs), INK, 13.0, Align::Right);
    let total = osd.total.map(clock).unwrap_or_else(|| "--:--".into());
    s.text((W - PAD - 58, by - 12, 58, 28), &total, INK, 13.0, Align::Left);
    legends_into(&mut s.px, W, (0, H - FOOT_H - 4, W, FOOT_H), &osd.legends, INK);
    if osd.paused {
        s.disc(W / 2, H / 2, 36, [0, 0, 0, 0xa0]);
        s.glyph(Glyph::Pause, W / 2, H / 2, 40.0, INK);
    }
    s.face()
}

fn clock(secs: u32) -> String {
    match secs >= 3600 {
        true => format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60),
        false => format!("{}:{:02}", secs / 60, secs % 60),
    }
}

/// Pictures cut down to a tile, kept so that moving through a folder does not decode them again.
fn thumb(path: &Path, w: u32, h: u32) -> Option<Arc<Vec<u8>>> {
    type Thumbs = Mutex<HashMap<(PathBuf, u32, u32), Option<Arc<Vec<u8>>>>>;
    static THUMBS: OnceLock<Thumbs> = OnceLock::new();
    let thumbs = THUMBS.get_or_init(Mutex::default);
    let key = (path.to_path_buf(), w, h);
    if let Some(t) = thumbs.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return t.clone();
    }
    let made = crate::art::decode_near(path, w, h)
        .and_then(|(src, sw, sh)| crate::art::cover_rgba(&src, sw, sh, w, h))
        .map(Arc::new);
    let mut thumbs = thumbs.lock().unwrap_or_else(|e| e.into_inner());
    // About 150 tiles; enough for a few folders of them.
    if thumbs.len() >= 160 {
        thumbs.clear();
    }
    thumbs.insert(key, made.clone());
    made
}

/// A track's cover, square, kept for the track playing.
fn cover(c: &Cover, side: u32) -> Option<Arc<Vec<u8>>> {
    static LAST: OnceLock<Mutex<Option<(Cover, Option<Arc<Vec<u8>>>)>>> = OnceLock::new();
    let last = LAST.get_or_init(Mutex::default);
    let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((was, px)) = last.as_ref() {
        if was == c {
            return px.clone();
        }
    }
    let decoded = match c {
        Cover::Bytes(bytes) => crate::art::decode_bytes(bytes),
        Cover::File(path) => crate::art::decode_near(path, side, side),
    };
    let px = decoded
        .and_then(|(src, sw, sh)| crate::art::cover_rgba(&src, sw, sh, side, side))
        .map(Arc::new);
    *last = Some((c.clone(), px.clone()));
    px
}

#[derive(Copy, Clone)]
enum Align {
    Left,
    Centre,
    Right,
}

/// A picture being drawn, with what the apps draw on it.
struct Sheet {
    px: Vec<u8>,
    w: u32,
    h: u32,
}

impl Sheet {
    fn new(w: u32, h: u32) -> Sheet {
        Sheet {
            px: vec![0u8; (w * h * 4) as usize],
            w,
            h,
        }
    }

    fn face(self) -> UndoFace {
        UndoFace {
            rgba: self.px,
            w: self.w,
            h: self.h,
        }
    }

    /// Lays `c` over the pixel at `x`, `y`, `cover` of it.
    fn blend(&mut self, x: u32, y: u32, c: [u8; 4], cover: f32) {
        if x >= self.w || y >= self.h {
            return;
        }
        let a = c[3] as f32 / 255.0 * cover.clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let at = ((y * self.w + x) * 4) as usize;
        let d = &mut self.px[at..at + 4];
        let da = d[3] as f32 / 255.0;
        let out = a + da * (1.0 - a);
        for i in 0..3 {
            let mixed = (c[i] as f32 * a + d[i] as f32 * da * (1.0 - a)) / out;
            d[i] = mixed.round().clamp(0.0, 255.0) as u8;
        }
        d[3] = (out * 255.0).round() as u8;
    }

    fn fill(&mut self, (x, y, w, h): (u32, u32, u32, u32), c: [u8; 4]) {
        for py in y..(y + h).min(self.h) {
            for px in x..(x + w).min(self.w) {
                self.blend(px, py, c, 1.0);
            }
        }
    }

    /// How much of the pixel at `px`, `py` in a `w` by `h` box with corners `r` round is inside.
    fn inside(px: u32, py: u32, w: u32, h: u32, r: u32) -> f32 {
        let r = r.min(w / 2).min(h / 2) as f32;
        if r <= 0.0 {
            return 1.0;
        }
        let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
        let dx = fx - fx.clamp(r, w as f32 - r);
        let dy = fy - fy.clamp(r, h as f32 - r);
        (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0)
    }

    fn round(&mut self, (x, y, w, h): (u32, u32, u32, u32), r: u32, c: [u8; 4]) {
        for py in 0..h {
            for px in 0..w {
                let cover = Self::inside(px, py, w, h, r);
                self.blend(x + px, y + py, c, cover);
            }
        }
    }

    fn disc(&mut self, cx: u32, cy: u32, r: u32, c: [u8; 4]) {
        self.round((cx.saturating_sub(r), cy.saturating_sub(r), 2 * r, 2 * r), r, c);
    }

    /// A band shading from `from` to `to` black, top to bottom, inside the window's rounded corners.
    fn shade(&mut self, rect: (u32, u32, u32, u32), from: u8, to: u8, r: u32) {
        self.shade_in(rect, (0, 0, self.w, self.h), from, to, r);
    }

    /// A band shading from `from` to `to` black, clipped to `clip`'s rounded corners.
    fn shade_in(
        &mut self,
        (x, y, w, h): (u32, u32, u32, u32),
        (cx, cy, cw, ch): (u32, u32, u32, u32),
        from: u8,
        to: u8,
        r: u32,
    ) {
        for py in 0..h {
            let a = from as f32 + (to as f32 - from as f32) * py as f32 / h.max(1) as f32;
            for px in 0..w {
                let cover = Self::inside(x + px - cx, y + py - cy, cw, ch, r);
                self.blend(x + px, y + py, [0, 0, 0, a as u8], cover);
            }
        }
    }

    fn bar(&mut self, (x, y, w, h): (u32, u32, u32, u32), done: f32, accent: [u8; 3]) {
        self.round((x, y, w, h), h / 2, [0xff, 0xff, 0xff, 0x30]);
        let filled = (w as f32 * done.clamp(0.0, 1.0)).round() as u32;
        if filled > 0 {
            self.round((x, y, filled.max(h), h), h / 2, opaque(accent));
        }
    }

    /// Where a list stands in all of it, for one longer than is shown.
    fn scrollbar(&mut self, (x, y, w, h): (u32, u32, u32, u32), first: usize, shown: usize, all: usize, accent: [u8; 3]) {
        if all <= shown {
            return;
        }
        self.round((x, y, w, h), w / 2, [0xff, 0xff, 0xff, 0x18]);
        let thumb = (h as usize * shown / all).max(20) as u32;
        let at = ((h - thumb) as usize * first / (all - shown)) as u32;
        self.round((x, y + at, w, thumb), w / 2, tinted(accent, 0xd0));
    }

    fn image(&mut self, src: &[u8], w: u32, h: u32, x: u32, y: u32) {
        over(&mut self.px, self.w, src, w, h, x, y);
    }

    /// A picture with its corners rounded off.
    fn image_round(&mut self, src: &[u8], w: u32, h: u32, x: u32, y: u32, r: u32) {
        for py in 0..h {
            for px in 0..w {
                let s = ((py * w + px) * 4) as usize;
                let cover = Self::inside(px, py, w, h, r);
                let c = [src[s], src[s + 1], src[s + 2], src[s + 3]];
                self.blend(x + px, y + py, c, cover);
            }
        }
    }

    fn glyph(&mut self, glyph: Glyph, cx: u32, cy: u32, px: f32, ink: [u8; 4]) {
        let Some(font) = crate::icon::symbols_font() else {
            return;
        };
        let (m, cov) = font.rasterize(glyph.char(), px);
        let x0 = cx as i32 - m.width as i32 / 2;
        let y0 = cy as i32 - m.height as i32 / 2;
        for gy in 0..m.height {
            for gx in 0..m.width {
                let a = cov[gy * m.width + gx];
                let (x, y) = (x0 + gx as i32, y0 + gy as i32);
                if a > 0 && x >= 0 && y >= 0 {
                    self.blend(x as u32, y as u32, ink, a as f32 / 255.0);
                }
            }
        }
    }

    fn text_width(&self, s: &str, px: f32) -> f32 {
        let Some(font) = text::label_font() else {
            return 0.0;
        };
        let layout = text::fit_as_written(font, s, f32::MAX, px, px);
        text::line_width(font, s, layout.px, layout.tracking)
    }

    /// `s` as written in the box, on one line, cut short with dots where it is too long.
    fn text(&mut self, (x, y, w, h): (u32, u32, u32, u32), s: &str, ink: [u8; 4], px: f32, align: Align) {
        let Some(font) = text::label_font() else {
            return;
        };
        if s.is_empty() || w == 0 {
            return;
        }
        let room = w as f32 - 2.0;
        let mut line = s.to_string();
        if self.text_width(&line, px) > room {
            let mut chars: Vec<char> = s.chars().collect();
            while !chars.is_empty() {
                chars.pop();
                let cut: String = chars.iter().collect::<String>().trim_end().to_string() + "...";
                if self.text_width(&cut, px) <= room {
                    line = cut;
                    break;
                }
            }
        }
        let layout = text::fit_as_written(font, &line, f32::MAX, px, px);
        let set = (text::line_width(font, &line, layout.px, layout.tracking).ceil() as u32 + 2).min(w);
        let bx = match align {
            Align::Left => x,
            Align::Centre => x + (w - set) / 2,
            Align::Right => x + w - set,
        };
        let cov = text::coverage(set, h, &layout);
        for (i, a) in cov.into_iter().enumerate() {
            if a > 0 {
                let (px_, py) = (bx + i as u32 % set, y + i as u32 / set);
                self.blend(px_, py, ink, a as f32 / 255.0);
            }
        }
    }

    /// `s` wrapped to the box's width, up to `lines` of it, from the top left.
    fn wrapped(&mut self, (x, y, w, h): (u32, u32, u32, u32), s: &str, ink: [u8; 4], px: f32, lines: usize) {
        let Some(font) = text::label_font() else {
            return;
        };
        let line_h = (px * 1.45) as u32;
        let all = text::wrap_as_written(font, s, w as f32 - 4.0, px);
        for (i, line) in all.iter().take(lines).enumerate() {
            let ly = y + i as u32 * line_h;
            if ly + line_h > y + h + line_h / 2 {
                break;
            }
            self.text((x, ly, w, line_h), line, ink, px, Align::Left);
        }
    }

    /// A cover for a book that has none: cloth of a colour its title picks, a spine down its
    /// left, and the title set on it.
    fn made_cover(&mut self, (x, y, w, h): (u32, u32, u32, u32), title: &str) {
        let hash = title.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
        let hue = (hash % 360) as f32;
        let cloth = hsl(hue, 0.42, 0.34);
        self.round((x, y, w, h), 5, opaque(cloth));
        self.fill((x + 7, y, 2, h), [0, 0, 0, 0x50]);
        self.fill((x + 9, y, 1, h), [0xff, 0xff, 0xff, 0x24]);
        self.fill((x + 16, y + 14, w - 26, 1), [0xff, 0xff, 0xff, 0x60]);
        self.fill((x + 16, y + h - 20, w - 26, 1), [0xff, 0xff, 0xff, 0x60]);
        let Some(font) = text::label_font() else {
            return;
        };
        let layout = text::fit_box(font, title, (w - 26) as f32, (h - 44) as f32, 4, 14.0, 8.0);
        let (tw, th) = (w - 22, h - 40);
        for (i, a) in text::coverage(tw, th, &layout).into_iter().enumerate() {
            if a > 0 {
                let (px, py) = (x + 14 + i as u32 % tw, y + 20 + i as u32 / tw);
                self.blend(px, py, [0xf6, 0xf0, 0xe2, 0xff], a as f32 / 255.0);
            }
        }
    }
}

fn hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_in_the_symbols_font() {
        let font = crate::icon::symbols_font().unwrap();
        for g in [
            Glyph::Folder, Glyph::FolderOpen, Glyph::File, Glyph::Picture, Glyph::Video,
            Glyph::Audio, Glyph::Text, Glyph::Pdf, Glyph::Book, Glyph::Script, Glyph::Archive,
            Glyph::Game, Glyph::Play, Glyph::Pause, Glyph::Next, Glyph::Previous, Glyph::Card,
            Glyph::Copy, Glyph::Cut, Glyph::Paste, Glyph::Delete, Glyph::Rename,
            Glyph::NewFolder, Glyph::Album, Glyph::Check, Glyph::Chevron, Glyph::PlayCircle,
            Glyph::Film, Glyph::Pictures, Glyph::Music, Glyph::Shelf, Glyph::UpFolder,
            Glyph::Alert, Glyph::Close,
        ] {
            assert_ne!(font.lookup_glyph_index(g.char()), 0, "{g:?} is not in the font");
        }
    }

    #[test]
    fn a_grid_keeps_the_cursors_row_in_view() {
        assert_eq!(first_shown(10, 9, 12, 4), 0);
        assert_eq!(first_shown(40, 0, 12, 4), 0);
        assert_eq!(first_shown(40, 21, 12, 4), 16);
        assert_eq!(first_shown(40, 39, 12, 4), 28);
        assert_eq!(first_shown(30, 29, 7, 1), 23);
    }
}
