//! Apps: the Apps shelf's carts, the built-in ones, and the Ports shelf's. Each opens on its own
//! from the shelf; a page opens over the last, and B goes up a folder or back to the one before,
//! then to the shelf.

pub mod epub;
pub mod files;
pub mod manage;
pub mod music;
pub mod ports;
pub mod reader;
pub mod video;

use std::path::{Path, PathBuf};

use slot_input::{Action, Btn};
use slot_store::{Cart, Platform};
use slot_ui::{
    AppPage, ButtonLegend, Dialog, FileRow, FilesPage, GalleryPage, Glyph, Legend, LibraryPage,
    Menu, MusicPage, ReaderPage, TextPage, Tile, VideoOsd, ViewerPage, BOOKS_ACCENT,
    MANUALS_ACCENT,
};

use crate::app::PageModel;
use files::{Browser, Entry, Kind};
use music::{Music, MusicView};
use reader::{Pages, Reader};
use video::Video;

const BOOKS: &[Kind] = &[Kind::Text, Kind::Epub, Kind::Pdf];
const MANUALS: &[Kind] = &[Kind::Pdf, Kind::Text, Kind::Epub, Kind::Picture];
const GALLERY: &[Kind] = &[Kind::Picture, Kind::Video];
/// How far L1, R1, Left and Right skip through a video, in seconds.
const VIDEO_SKIP: f64 = 10.0;
const FILES_PAGE: usize = 7;
const GALLERY_COLS: usize = 4;
const GALLERY_PAGE: usize = 12;
const LIBRARY_COLS: usize = 6;
const LIBRARY_PAGE: usize = 12;

/// How a browser screen lays out its folder.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Style {
    Files,
    Gallery,
    Library { accent: [u8; 3] },
}

/// The built-in apps, as the shelf lists them: a name, the folder each opens on, its filter and
/// its style.
const BUILT_IN: [(&str, &str, Option<&[Kind]>, Style); 5] = [
    ("Files", "", None, Style::Files),
    (
        "Books",
        "Books",
        Some(BOOKS),
        Style::Library {
            accent: BOOKS_ACCENT,
        },
    ),
    (
        "Manuals",
        "Manuals",
        Some(MANUALS),
        Style::Library {
            accent: MANUALS_ACCENT,
        },
    ),
    ("Gallery", "Pictures", Some(GALLERY), Style::Gallery),
    ("Music", music::DIR, None, Style::Files),
];

pub enum Screen {
    Browser {
        browser: Browser,
        style: Style,
    },
    Picture {
        files: Vec<PathBuf>,
        at: usize,
        chrome: bool,
    },
    Reader {
        reader: Reader,
        accent: [u8; 3],
    },
    Music {
        cursor: usize,
    },
    Video(Video),
}

/// What the App is to do once an app has had a button.
#[derive(PartialEq, Eq, Debug)]
pub enum Done {
    Stay,
    /// Back out of the app, to the shelf.
    Leave,
    /// Quit slot for this port to run.
    Port(PathBuf),
    /// Open the keyboard to name a file or folder, then pass the result to [`Apps::typed`].
    Type {
        title: String,
        text: String,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Clipboard {
    path: PathBuf,
    cut: bool,
}

enum Overlay {
    Menu { cursor: usize },
    Delete(PathBuf),
    Copying(manage::Copy),
}

enum PendingType {
    Rename(PathBuf),
    NewFolder,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum FileAction {
    Open,
    Copy,
    Cut,
    Paste,
    Rename,
    Delete,
    NewFolder,
}

impl FileAction {
    const ALL: [FileAction; 7] = [
        FileAction::Open,
        FileAction::Copy,
        FileAction::Cut,
        FileAction::Paste,
        FileAction::Rename,
        FileAction::Delete,
        FileAction::NewFolder,
    ];

    fn glyph(self) -> Glyph {
        match self {
            FileAction::Open => Glyph::FolderOpen,
            FileAction::Copy => Glyph::Copy,
            FileAction::Cut => Glyph::Cut,
            FileAction::Paste => Glyph::Paste,
            FileAction::Rename => Glyph::Rename,
            FileAction::Delete => Glyph::Delete,
            FileAction::NewFolder => Glyph::NewFolder,
        }
    }

    fn label(self) -> &'static str {
        match self {
            FileAction::Open => "Open",
            FileAction::Copy => "Copy",
            FileAction::Cut => "Cut",
            FileAction::Paste => "Paste here",
            FileAction::Rename => "Rename",
            FileAction::Delete => "Delete",
            FileAction::NewFolder => "New folder",
        }
    }
}

pub struct Apps {
    root: PathBuf,
    stack: Vec<Screen>,
    clipboard: Option<Clipboard>,
    overlay: Option<Overlay>,
    typing: Option<PendingType>,
    /// What went wrong last, said in the footer.
    note: Option<String>,
}

/// Whether `stem` names one of the built-in apps rather than a port.
pub fn built_in(stem: &str) -> bool {
    BUILT_IN.iter().any(|b| b.0 == stem)
}

/// The app that walks the whole card, and runs any script on it as a port.
pub const FILES: &str = BUILT_IN[0].0;

/// The Apps shelf, the built-in apps, then the Ports shelf, the ports. A built-in app has no
/// file; each can have a label in `Labels/APPS` or `Labels/PORTS`, named as the shelf names it.
pub fn carts(root: &Path) -> Vec<Cart> {
    let cart = |platform: Platform, stem: String, rom: PathBuf| {
        let labels = root.join("Labels").join(platform.dir_name());
        Cart {
            platform,
            label: Some(labels.join(format!("{stem}.png"))).filter(|l| l.is_file()),
            stem,
            rom,
            title: String::new(),
            code: String::new(),
            shell: None,
        }
    };
    BUILT_IN
        .iter()
        .map(|b| cart(Platform::Apps, b.0.to_string(), PathBuf::new()))
        .chain(
            ports::scripts(root)
                .into_iter()
                .map(|p| cart(Platform::Ports, music::name(&p), p)),
        )
        .collect()
}

impl Apps {
    /// Opens the app `cart` is, straight from the shelf. A port has slot quit for it.
    pub fn open(root: &Path, cart: &Cart, music: &Option<Music>) -> (Apps, Done) {
        let mut apps = Apps {
            root: root.to_path_buf(),
            stack: Vec::new(),
            clipboard: None,
            overlay: None,
            typing: None,
            note: None,
        };
        if !cart.rom.as_os_str().is_empty() {
            let done = apps.port(&cart.rom);
            return (apps, done);
        }
        let Some(&(name, dir, only, style)) = BUILT_IN.iter().find(|b| b.0 == cart.stem) else {
            return (apps, Done::Leave);
        };
        let screen = match name {
            "Music" => {
                let _ = std::fs::create_dir_all(root.join(dir));
                let view = music.as_ref().map(Music::view).unwrap_or_default();
                Screen::Music {
                    cursor: view.at.unwrap_or(0),
                }
            }
            _ => Screen::Browser {
                browser: Browser::new(name, root.join(dir), only),
                style,
            },
        };
        apps.stack.push(screen);
        (apps, Done::Stay)
    }

    pub fn screen(&self) -> Option<&Screen> {
        self.stack.last()
    }

    /// Whether a video is playing, which the music gives way to.
    pub fn watching(&self) -> bool {
        self.stack.iter().any(|s| matches!(s, Screen::Video(_)))
    }

    pub fn set_volume(&self, volume: u8) {
        for screen in &self.stack {
            if let Screen::Video(v) = screen {
                v.set_volume(volume);
            }
        }
    }

    /// Finishes an active file copy, or backs out of a video once it has played to its end.
    pub fn tend(&mut self) {
        if let Some(Overlay::Copying(copy)) = &self.overlay {
            if let Some(result) = copy.ended() {
                let to = copy.to.clone();
                self.overlay = None;
                match result {
                    Ok(()) => {
                        self.note = None;
                        self.reload_browser(Some(&to));
                    }
                    Err(e) => {
                        self.note = Some(if e == "Stopped" {
                            "Copy cancelled".into()
                        } else {
                            e
                        });
                        self.reload_browser(None);
                    }
                }
            }
        }
        let Some(Screen::Video(v)) = self.stack.last_mut() else {
            return;
        };
        if !v.ended() || v.error.is_some() {
            return;
        }
        match v.frame().serial {
            0 => v.error = Some("Could not play that video".into()),
            _ => {
                self.stack.pop();
            }
        }
    }

    /// Carries out the rename or new folder the keyboard was opened for.
    pub fn typed(&mut self, text: String) {
        let Some(pending) = self.typing.take() else {
            return;
        };
        match pending {
            PendingType::Rename(path) => match manage::rename(&path, &text) {
                Ok(to) => {
                    if let Some(clip) = &mut self.clipboard {
                        if clip.path == path {
                            clip.path = to.clone();
                        }
                    }
                    self.note = None;
                    self.reload_browser(Some(&to));
                }
                Err(e) => self.note = Some(e),
            },
            PendingType::NewFolder => {
                let Some(Screen::Browser { browser, .. }) = self.stack.last() else {
                    return;
                };
                match manage::new_folder(&browser.dir, &text) {
                    Ok(to) => {
                        self.note = None;
                        self.reload_browser(Some(&to));
                    }
                    Err(e) => self.note = Some(e),
                }
            }
        }
    }

    pub fn cancel_typing(&mut self) {
        self.typing = None;
    }

    fn reload_browser(&mut self, select: Option<&Path>) {
        if let Some(Screen::Browser { browser, .. }) = self.stack.last_mut() {
            browser.reload(select);
        }
    }

    /// The music page's tracks: the queue playing, else all of `Music`.
    fn music_rows(&self, view: &MusicView) -> Vec<PathBuf> {
        match view.queue.is_empty() {
            true => music::tracks(&self.root.join(music::DIR)),
            false => view.queue.clone(),
        }
    }

    fn file_action_enabled(&self, browser: &Browser, action: FileAction) -> bool {
        let selected = browser.selected();
        let protected = selected.is_some_and(|e| manage::kept(&self.root, &e.path));
        file_action_can(
            selected.is_some(),
            protected,
            self.clipboard.is_some(),
            action,
        )
    }

    fn open_entry(
        &mut self,
        entry: Entry,
        style: Style,
        music: &mut Option<Music>,
        volume: u8,
    ) -> Done {
        let root = self.root.clone();
        let accent = match style {
            Style::Library { accent } => accent,
            _ => BOOKS_ACCENT,
        };
        let open = match entry.kind {
            Kind::Folder => {
                if let Some(Screen::Browser { browser, .. }) = self.stack.last_mut() {
                    browser.enter(entry.path);
                    self.note = None;
                }
                return Done::Stay;
            }
            Kind::Picture => {
                let Some(Screen::Browser { browser, .. }) = self.stack.last() else {
                    return Done::Stay;
                };
                let (files, at) = browser.siblings(Kind::Picture, &entry.path);
                Screen::Picture {
                    files,
                    at,
                    chrome: true,
                }
            }
            Kind::Text | Kind::Epub | Kind::Pdf => match Reader::open(&root, &entry.path) {
                Ok(reader) => Screen::Reader { reader, accent },
                Err(e) => {
                    self.note = Some(e);
                    return Done::Stay;
                }
            },
            Kind::Audio => {
                let Some(Screen::Browser { browser, .. }) = self.stack.last() else {
                    return Done::Stay;
                };
                let (files, at) = browser.siblings(Kind::Audio, &entry.path);
                music.get_or_insert_with(Music::spawn).play(files, at);
                Screen::Music { cursor: at }
            }
            Kind::Video => Screen::Video(Video::open(&root, &entry.path, 0.0, volume)),
            Kind::Script => return self.port(&entry.path),
            Kind::Other => {
                self.note = Some(format!("Nothing here opens {}", entry.name));
                return Done::Stay;
            }
        };
        self.note = None;
        self.stack.push(open);
        Done::Stay
    }

    fn overlay_input(&mut self, btn: Btn, music: &mut Option<Music>, volume: u8) -> Done {
        match self.overlay.take() {
            Some(Overlay::Copying(copy)) => {
                if btn == Btn::B {
                    copy.cancel();
                }
                self.overlay = Some(Overlay::Copying(copy));
                Done::Stay
            }
            Some(Overlay::Delete(path)) => match btn {
                Btn::A => {
                    match manage::delete(&path) {
                        Ok(()) => {
                            if self.clipboard.as_ref().is_some_and(|c| c.path == path) {
                                self.clipboard = None;
                            }
                            self.note = None;
                            self.reload_browser(None);
                        }
                        Err(e) => self.note = Some(e),
                    }
                    Done::Stay
                }
                Btn::B => Done::Stay,
                _ => {
                    self.overlay = Some(Overlay::Delete(path));
                    Done::Stay
                }
            },
            Some(Overlay::Menu { mut cursor }) => {
                let n = FileAction::ALL.len();
                match btn {
                    Btn::B | Btn::X => Done::Stay,
                    Btn::Up => {
                        cursor = (cursor + n - 1) % n;
                        self.overlay = Some(Overlay::Menu { cursor });
                        Done::Stay
                    }
                    Btn::Down => {
                        cursor = (cursor + 1) % n;
                        self.overlay = Some(Overlay::Menu { cursor });
                        Done::Stay
                    }
                    Btn::A => {
                        let action = FileAction::ALL[cursor];
                        let Some(Screen::Browser { browser, style }) = self.stack.last() else {
                            return Done::Stay;
                        };
                        let (style, dir, selected) =
                            (*style, browser.dir.clone(), browser.selected().cloned());
                        if !self.file_action_enabled(browser, action) {
                            self.overlay = Some(Overlay::Menu { cursor });
                            return Done::Stay;
                        }
                        match action {
                            FileAction::Open => match selected {
                                Some(entry) => self.open_entry(entry, style, music, volume),
                                None => Done::Stay,
                            },
                            FileAction::Copy => {
                                if let Some(entry) = selected {
                                    self.clipboard = Some(Clipboard {
                                        path: entry.path,
                                        cut: false,
                                    });
                                    self.note = None;
                                }
                                Done::Stay
                            }
                            FileAction::Cut => {
                                if let Some(entry) = selected {
                                    self.clipboard = Some(Clipboard {
                                        path: entry.path,
                                        cut: true,
                                    });
                                    self.note = None;
                                }
                                Done::Stay
                            }
                            FileAction::Paste => {
                                let Some(clip) = self.clipboard.clone() else {
                                    return Done::Stay;
                                };
                                if clip.cut {
                                    match manage::move_into(&clip.path, &dir) {
                                        Ok(to) => {
                                            self.clipboard = None;
                                            self.note = None;
                                            self.reload_browser(Some(&to));
                                        }
                                        Err(e) => self.note = Some(e),
                                    }
                                } else {
                                    match manage::Copy::start(&clip.path, &dir) {
                                        Ok(copy) => {
                                            self.note = None;
                                            self.overlay = Some(Overlay::Copying(copy));
                                        }
                                        Err(e) => self.note = Some(e),
                                    }
                                }
                                Done::Stay
                            }
                            FileAction::Rename => {
                                if let Some(entry) = selected {
                                    self.typing = Some(PendingType::Rename(entry.path));
                                    return Done::Type {
                                        title: "Rename".into(),
                                        text: entry.name,
                                    };
                                }
                                Done::Stay
                            }
                            FileAction::Delete => {
                                if let Some(entry) = selected {
                                    self.overlay = Some(Overlay::Delete(entry.path));
                                }
                                Done::Stay
                            }
                            FileAction::NewFolder => {
                                self.typing = Some(PendingType::NewFolder);
                                Done::Type {
                                    title: "New folder".into(),
                                    text: String::new(),
                                }
                            }
                        }
                    }
                    _ => {
                        self.overlay = Some(Overlay::Menu { cursor });
                        Done::Stay
                    }
                }
            }
            None => Done::Stay,
        }
    }

    pub fn input(&mut self, action: Action, music: &mut Option<Music>, volume: u8) -> Done {
        let Action::GbaDown(btn) = action else {
            return match action {
                Action::QuickMenu => Done::Leave,
                _ => Done::Stay,
            };
        };
        if self.overlay.is_some() {
            return self.overlay_input(btn, music, volume);
        }
        if btn == Btn::B {
            self.note = None;
            if let Some(Screen::Browser { browser, .. }) = self.stack.last_mut() {
                if browser.up() {
                    return Done::Stay;
                }
            }
            self.stack.pop();
            return match self.stack.is_empty() {
                true => Done::Leave,
                false => Done::Stay,
            };
        }
        let root = self.root.clone();
        let view = music.as_ref().map(Music::view).unwrap_or_default();
        let music_rows = match self.stack.last() {
            Some(Screen::Music { .. }) => self.music_rows(&view),
            _ => Vec::new(),
        };
        match self.stack.last_mut() {
            Some(Screen::Browser { browser, style }) => {
                let style = *style;
                let n = browser.entries.len();
                match btn {
                    Btn::X if style == Style::Files => {
                        let has_sel = browser.selected().is_some();
                        let protected = browser
                            .selected()
                            .is_some_and(|e| manage::kept(&root, &e.path));
                        let has_clip = self.clipboard.is_some();
                        let first = FileAction::ALL
                            .iter()
                            .position(|&a| file_action_can(has_sel, protected, has_clip, a))
                            .unwrap_or(FileAction::ALL.len() - 1);
                        self.overlay = Some(Overlay::Menu { cursor: first });
                    }
                    Btn::Y => {
                        if browser.up() {
                            self.note = None;
                        }
                    }
                    Btn::A => {
                        if let Some(entry) = browser.selected().cloned() {
                            return self.open_entry(entry, style, music, volume);
                        }
                    }
                    _ if n > 0 => match style {
                        Style::Files => match btn {
                            Btn::Up => browser.cursor = (browser.cursor + n - 1) % n,
                            Btn::Down => browser.cursor = (browser.cursor + 1) % n,
                            Btn::Left | Btn::L1 => {
                                browser.cursor = browser.cursor.saturating_sub(FILES_PAGE)
                            }
                            Btn::Right | Btn::R1 => {
                                browser.cursor = (browser.cursor + FILES_PAGE).min(n - 1)
                            }
                            _ => {}
                        },
                        Style::Gallery | Style::Library { .. } => {
                            let (cols, page) = match style {
                                Style::Gallery => (GALLERY_COLS, GALLERY_PAGE),
                                _ => (LIBRARY_COLS, LIBRARY_PAGE),
                            };
                            grid_move(&mut browser.cursor, n, cols, page, btn);
                        }
                    },
                    _ => {}
                }
            }
            Some(Screen::Picture { files, at, chrome }) => {
                let n = files.len().max(1);
                match btn {
                    Btn::A => *chrome = !*chrome,
                    Btn::Left | Btn::L1 | Btn::Up => *at = (*at + n - 1) % n,
                    Btn::Right | Btn::R1 | Btn::Down => *at = (*at + 1) % n,
                    _ => {}
                }
            }
            Some(Screen::Reader { reader, .. }) => {
                let by = match btn {
                    Btn::Left | Btn::Up => -1,
                    Btn::Right | Btn::Down | Btn::A => 1,
                    Btn::L1 | Btn::L2 => -10,
                    Btn::R1 | Btn::R2 => 10,
                    _ => 0,
                };
                if by != 0 {
                    reader.turn(&root, by);
                }
            }
            Some(Screen::Music { cursor }) => {
                let n = music_rows.len().max(1);
                match btn {
                    Btn::Up => *cursor = (*cursor + n - 1) % n,
                    Btn::Down => *cursor = (*cursor + 1) % n,
                    Btn::A if !music_rows.is_empty() => {
                        music
                            .get_or_insert_with(Music::spawn)
                            .play(music_rows.clone(), *cursor);
                    }
                    Btn::X => {
                        if let Some(m) = music {
                            m.toggle();
                        }
                    }
                    Btn::L1 | Btn::R1 => {
                        if let Some(m) = music {
                            m.skip(if btn == Btn::L1 { -1 } else { 1 });
                        }
                    }
                    Btn::Y => {
                        if let Some(m) = music {
                            m.stop();
                        }
                    }
                    _ => {}
                }
            }
            Some(Screen::Video(v)) => {
                v.poke_osd();
                match btn {
                    Btn::A | Btn::X => v.toggle(),
                    Btn::L1 | Btn::R1 | Btn::Left | Btn::Right => {
                        let back = matches!(btn, Btn::L1 | Btn::Left);
                        let to = (v.at() + if back { -VIDEO_SKIP } else { VIDEO_SKIP }).max(0.0);
                        let path = v.path.clone();
                        let total = v.total();
                        // The old one's ffmpegs end before the new one's start.
                        self.stack.pop();
                        let next = Video::open(&root, &path, to, volume);
                        next.set_total(total);
                        self.stack.push(Screen::Video(next));
                    }
                    _ => {}
                }
            }
            None => return Done::Leave,
        }
        Done::Stay
    }

    fn port(&mut self, script: &Path) -> Done {
        match ports::queue(&self.root, script) {
            Ok(()) => Done::Port(script.to_path_buf()),
            Err(e) => {
                eprintln!("slot: ports: {e}");
                self.note = Some("Could not start that port".into());
                Done::Stay
            }
        }
    }

    pub fn page(&self, music: Option<&MusicView>) -> Option<PageModel> {
        let note = self.note.clone().unwrap_or_default();
        let back = Legend::new(ButtonLegend::B, "Back");
        Some(match self.stack.last()? {
            Screen::Browser { browser, style } => match *style {
                Style::Files => PageModel::App(AppPage::Files(self.files_page(browser, note))),
                Style::Gallery => {
                    PageModel::App(AppPage::Gallery(self.gallery_page(browser, note)))
                }
                Style::Library { accent } => {
                    PageModel::App(AppPage::Library(self.library_page(browser, accent, note)))
                }
            },
            Screen::Picture { files, at, chrome } => {
                let path = files.get(*at).cloned().unwrap_or_default();
                PageModel::App(AppPage::Viewer(ViewerPage {
                    name: music::name(&path),
                    path,
                    place: format!("{} / {}", at + 1, files.len()),
                    chrome: *chrome,
                    legends: vec![
                        back,
                        Legend::new(ButtonLegend::Left, "Previous"),
                        Legend::new(ButtonLegend::Right, "Next"),
                        Legend::new(ButtonLegend::A, "Hide"),
                    ],
                }))
            }
            Screen::Reader { reader, accent } => {
                let (lines, picture, error) = match (&reader.pages, &reader.drawn) {
                    (Pages::Pdf { .. }, Ok(path)) => (Vec::new(), Some(path.clone()), None),
                    (Pages::Pdf { .. }, Err(e)) => (Vec::new(), None, Some(e.clone())),
                    (Pages::Text(_), _) => (reader.lines(), None, None),
                };
                PageModel::App(AppPage::Reader(ReaderPage {
                    title: reader.title.clone(),
                    accent: *accent,
                    page: reader.page,
                    pages: reader.count(),
                    lines,
                    picture,
                    error,
                    legends: vec![
                        back,
                        Legend::new(ButtonLegend::Right, "Turn"),
                        Legend::new(ButtonLegend::R1, "10 Pages"),
                    ],
                }))
            }
            Screen::Music { cursor } => {
                let view = music.cloned().unwrap_or_default();
                let tracks = self.music_rows(&view);
                let playing = view.playing().map(Path::to_path_buf);
                let now = playing
                    .as_ref()
                    .and_then(|p| tracks.iter().position(|t| t == p));
                let loaded = playing.is_some();
                let heading = match view.queue.is_empty() {
                    true => "All music",
                    false => "Up next",
                };
                let status = match (&view.error, &playing, tracks.is_empty()) {
                    (Some(e), _, _) => e.clone(),
                    (None, _, true) => format!("Put music in {}", music::DIR),
                    (None, Some(t), _) => format!("Playing {}, on after B", music::name(t)),
                    (None, None, false) => String::new(),
                };
                let title = if view.title.is_empty() {
                    playing.as_deref().map(music::name).unwrap_or_default()
                } else {
                    view.title.clone()
                };
                PageModel::App(AppPage::Music(MusicPage {
                    title,
                    artist: view.artist,
                    cover: view.cover,
                    secs: view.secs,
                    total: view.total,
                    loaded,
                    paused: view.paused,
                    heading: heading.into(),
                    tracks: tracks.iter().map(|t| music::name(t)).collect(),
                    now,
                    cursor: (*cursor).min(tracks.len().saturating_sub(1)),
                    status,
                    legends: vec![
                        back,
                        Legend::new(ButtonLegend::A, "Play"),
                        Legend::new(
                            ButtonLegend::X,
                            if view.paused { "Resume" } else { "Pause" },
                        ),
                        Legend::new(ButtonLegend::Y, "Stop"),
                    ],
                }))
            }
            Screen::Video(v) => match &v.error {
                Some(e) => PageModel::Text(TextPage {
                    title: v.title.clone(),
                    status: String::new(),
                    lines: vec![e.clone()],
                    legends: vec![back],
                }),
                None => {
                    let osd = v.osd_shown().then(|| VideoOsd {
                        title: v.title.clone(),
                        secs: v.at().max(0.0) as u32,
                        total: v.total(),
                        paused: v.paused(),
                        legends: vec![
                            back,
                            Legend::new(ButtonLegend::A, if v.paused() { "Play" } else { "Pause" }),
                            Legend::new(ButtonLegend::Right, "10s"),
                        ],
                    });
                    PageModel::Video(VideoModel {
                        path: v.path.clone(),
                        frame: v.frame(),
                        osd,
                    })
                }
            },
        })
    }

    fn files_page(&self, browser: &Browser, note: String) -> FilesPage {
        let cut_path = self
            .clipboard
            .as_ref()
            .filter(|c| c.cut)
            .map(|c| c.path.as_path());
        let rows: Vec<FileRow> = browser
            .entries
            .iter()
            .map(|e| FileRow {
                name: e.name.clone(),
                icon: e.icon(),
                size: e.size_text(),
                date: e.date_text(),
                faded: cut_path == Some(e.path.as_path()),
            })
            .collect();
        let meta = count_label(rows.len());
        let status = if !note.is_empty() {
            note
        } else if let Some(clip) = &self.clipboard {
            let what = clip.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            format!("{}: {what}", if clip.cut { "Cut" } else { "Copied" })
        } else {
            crate::port_runtime::free_bytes(&browser.dir)
                .map(|n| format!("{} free", files::bytes_text(n)))
                .unwrap_or_default()
        };
        let back_label = if browser.dir == browser.top {
            "Back"
        } else {
            "Up"
        };
        let menu = match &self.overlay {
            Some(Overlay::Menu { cursor }) => {
                let title = browser
                    .selected()
                    .map(|e| e.name.clone())
                    .unwrap_or_else(|| "Folder".into());
                let rows = FileAction::ALL
                    .into_iter()
                    .map(|a| {
                        (
                            a.glyph(),
                            a.label().to_string(),
                            self.file_action_enabled(browser, a),
                        )
                    })
                    .collect();
                Some(Menu {
                    title,
                    rows,
                    cursor: *cursor,
                })
            }
            _ => None,
        };
        let dialog = match &self.overlay {
            Some(Overlay::Delete(path)) => {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("this item");
                Some(Dialog {
                    title: "Delete".into(),
                    body: format!("Delete {name}? This cannot be undone."),
                    progress: None,
                    legends: vec![
                        Legend::new(ButtonLegend::B, "Cancel"),
                        Legend::new(ButtonLegend::A, "Delete"),
                    ],
                })
            }
            Some(Overlay::Copying(copy)) => Some(Dialog {
                title: "Copying".into(),
                body: format!(
                    "{}  ({} / {})",
                    copy.name,
                    files::bytes_text(copy.done_bytes()),
                    files::bytes_text(copy.total)
                ),
                progress: Some(copy.progress()),
                legends: vec![Legend::new(ButtonLegend::B, "Cancel")],
            }),
            _ => None,
        };
        FilesPage {
            title: browser.title.clone(),
            crumbs: browser.crumbs("Card"),
            cursor: browser.cursor.min(rows.len().saturating_sub(1)),
            rows,
            status,
            meta,
            legends: vec![
                Legend::new(ButtonLegend::B, back_label),
                Legend::new(ButtonLegend::A, "Open"),
                Legend::new(ButtonLegend::X, "Actions"),
            ],
            menu,
            dialog,
        }
    }

    fn gallery_page(&self, browser: &Browser, note: String) -> GalleryPage {
        let tiles: Vec<Tile> = browser
            .entries
            .iter()
            .map(|e| {
                let picture = match e.kind {
                    Kind::Picture => Some(e.path.clone()),
                    Kind::Folder => files::first_picture(&e.path),
                    _ => None,
                };
                Tile {
                    name: e.name.clone(),
                    icon: e.icon(),
                    picture,
                    detail: String::new(),
                    read: None,
                }
            })
            .collect();
        let meta = count_label(tiles.len());
        let status = if !note.is_empty() {
            note
        } else {
            browser.place()
        };
        let back_label = if browser.dir == browser.top {
            "Back"
        } else {
            "Up"
        };
        GalleryPage {
            title: browser.title.clone(),
            meta,
            cursor: browser.cursor.min(tiles.len().saturating_sub(1)),
            tiles,
            status,
            legends: vec![
                Legend::new(ButtonLegend::B, back_label),
                Legend::new(ButtonLegend::A, "Open"),
            ],
        }
    }

    fn library_page(&self, browser: &Browser, accent: [u8; 3], note: String) -> LibraryPage {
        let progress = reader::progress(&self.root);
        let tiles: Vec<Tile> = browser
            .entries
            .iter()
            .map(|e| {
                let read = progress.get(&e.path).copied();
                let (name, detail, picture) = match e.kind {
                    Kind::Folder => (e.name.clone(), e.size_text(), None),
                    Kind::Picture => (music::name(&e.path), "IMG".into(), Some(e.path.clone())),
                    _ => {
                        let fmt = match e.kind {
                            Kind::Pdf => "PDF",
                            Kind::Epub => "EPUB",
                            Kind::Text => "TXT",
                            _ => e.kind.label(),
                        };
                        let detail = match read {
                            Some(p) => format!("{fmt} · {p}%"),
                            None => fmt.into(),
                        };
                        (music::name(&e.path), detail, None)
                    }
                };
                Tile {
                    name,
                    icon: e.icon(),
                    picture,
                    detail,
                    read,
                }
            })
            .collect();
        let meta = count_label(tiles.len());
        let status = if !note.is_empty() {
            note
        } else {
            browser.place()
        };
        let back_label = if browser.dir == browser.top {
            "Back"
        } else {
            "Up"
        };
        LibraryPage {
            title: browser.title.clone(),
            accent,
            meta,
            cursor: browser.cursor.min(tiles.len().saturating_sub(1)),
            tiles,
            status,
            legends: vec![
                Legend::new(ButtonLegend::B, back_label),
                Legend::new(ButtonLegend::A, "Read"),
            ],
        }
    }
}

fn file_action_can(has_sel: bool, protected: bool, has_clip: bool, action: FileAction) -> bool {
    match action {
        FileAction::Open => has_sel,
        FileAction::Copy | FileAction::Cut | FileAction::Rename | FileAction::Delete => {
            has_sel && !protected
        }
        FileAction::Paste => has_clip,
        FileAction::NewFolder => true,
    }
}

fn count_label(n: usize) -> String {
    match n {
        0 => "Empty".into(),
        1 => "1 item".into(),
        n => format!("{n} items"),
    }
}

fn grid_move(cursor: &mut usize, n: usize, cols: usize, page: usize, btn: Btn) {
    match btn {
        Btn::Left => *cursor = (*cursor + n - 1) % n,
        Btn::Right => *cursor = (*cursor + 1) % n,
        Btn::Up => {
            if *cursor >= cols {
                *cursor -= cols;
            }
        }
        Btn::Down => {
            if *cursor + cols < n {
                *cursor += cols;
            } else if *cursor / cols + 1 < n.div_ceil(cols) {
                *cursor = n - 1;
            }
        }
        Btn::L1 => *cursor = cursor.saturating_sub(page),
        Btn::R1 => *cursor = (*cursor + page).min(n - 1),
        _ => {}
    }
}

/// A video's picture and optional on-screen controls, for the frontend to show as the page.
#[derive(Clone, Debug)]
pub struct VideoModel {
    pub path: PathBuf,
    pub frame: video::Frame,
    pub osd: Option<VideoOsd>,
}

impl PartialEq for VideoModel {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.frame.serial == other.frame.serial && self.osd == other.osd
    }
}
