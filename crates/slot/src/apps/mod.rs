//! Apps: what the Apps page opens, the built-in ones and the ports in `Ports`. Each opens over the
//! last, and B goes back to the one before.

pub mod epub;
pub mod files;
pub mod music;
pub mod ports;
pub mod reader;
pub mod video;

use std::path::{Path, PathBuf};

use slot_input::{Action, Btn};
use slot_ui::{ButtonLegend, Legend, ListPage, ListRow, PicturePage, TextPage};

use crate::app::PageModel;
use files::{Browser, Kind};
use music::{Music, MusicView};
use reader::{Pages, Reader};
use video::Video;

const BOOKS: &[Kind] = &[Kind::Text, Kind::Epub, Kind::Pdf];
const MANUALS: &[Kind] = &[Kind::Pdf, Kind::Text, Kind::Epub, Kind::Picture];
const GALLERY: &[Kind] = &[Kind::Picture, Kind::Video];
/// How far L1 and R1 skip through a video, in seconds.
const VIDEO_SKIP: f64 = 10.0;

/// The built-in apps, as the launcher lists them: a name and the folder each opens on.
const BUILT_IN: [(&str, &str, Option<&[Kind]>); 5] = [
    ("Files", "", None),
    ("Books", "Books", Some(BOOKS)),
    ("Manuals", "Manuals", Some(MANUALS)),
    ("Gallery", "Pictures", Some(GALLERY)),
    ("Music", music::DIR, None),
];

pub enum Screen {
    Launcher { cursor: usize },
    Browser(Browser),
    Picture { files: Vec<PathBuf>, at: usize },
    Reader(Reader),
    Music { cursor: usize },
    Video(Video),
}

/// What the App is to do once an app has had a button.
#[derive(PartialEq, Eq, Debug)]
pub enum Done {
    Stay,
    /// Back out of the launcher, to the menu.
    Leave,
    /// Quit slot for this port to run.
    Port(PathBuf),
}

pub struct Apps {
    root: PathBuf,
    stack: Vec<Screen>,
    /// What went wrong last, said under the page's title.
    note: Option<String>,
}

impl Apps {
    pub fn new(root: &Path) -> Apps {
        Apps {
            root: root.to_path_buf(),
            stack: vec![Screen::Launcher { cursor: 0 }],
            note: None,
        }
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

    /// Back to the list once a video has played to its end; one that never showed a picture
    /// says so instead.
    pub fn tend(&mut self) {
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

    /// What the launcher lists: the built-in apps, then the ports.
    fn launcher_rows(&self) -> Vec<(String, &'static str)> {
        let ports = ports::scripts(&self.root);
        BUILT_IN
            .iter()
            .map(|b| (b.0.to_string(), ""))
            .chain(ports.iter().map(|p| (music::name(p), "Port")))
            .collect()
    }

    /// The music page's tracks: the queue playing, else all of `Music`.
    fn music_rows(&self, view: &MusicView) -> Vec<PathBuf> {
        match view.queue.is_empty() {
            true => music::tracks(&self.root.join(music::DIR)),
            false => view.queue.clone(),
        }
    }

    pub fn input(&mut self, action: Action, music: &mut Option<Music>, volume: u8) -> Done {
        let Action::GbaDown(btn) = action else {
            return match action {
                Action::QuickMenu => Done::Leave,
                _ => Done::Stay,
            };
        };
        if btn == Btn::B {
            self.note = None;
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
        let launcher_n = self.launcher_rows().len();
        let mut open = None;
        match self.stack.last_mut() {
            Some(Screen::Launcher { cursor }) => match btn {
                Btn::Up => *cursor = (*cursor + launcher_n - 1) % launcher_n,
                Btn::Down => *cursor = (*cursor + 1) % launcher_n,
                Btn::A => {
                    let at = *cursor;
                    if let Some(&(name, dir, only)) = BUILT_IN.get(at) {
                        open = Some(match name {
                            "Music" => {
                                let _ = std::fs::create_dir_all(root.join(dir));
                                let cursor = view.at.unwrap_or(0);
                                Screen::Music { cursor }
                            }
                            _ => Screen::Browser(Browser::new(name, root.join(dir), only)),
                        });
                    } else if let Some(port) = ports::scripts(&root).get(at - BUILT_IN.len()) {
                        return self.port(port);
                    }
                }
                _ => {}
            },
            Some(Screen::Browser(b)) => {
                let n = b.entries.len().max(1);
                match btn {
                    Btn::Up => b.cursor = (b.cursor + n - 1) % n,
                    Btn::Down => b.cursor = (b.cursor + 1) % n,
                    Btn::Left => b.cursor = b.cursor.saturating_sub(10),
                    Btn::Right => b.cursor = (b.cursor + 10).min(n - 1),
                    Btn::Y => {
                        b.up();
                    }
                    Btn::A => {
                        let Some(entry) = b.selected().cloned() else {
                            return Done::Stay;
                        };
                        match entry.kind {
                            Kind::Folder => b.enter(entry.path),
                            Kind::Picture => {
                                let (files, at) = b.siblings(Kind::Picture, &entry.path);
                                open = Some(Screen::Picture { files, at });
                            }
                            Kind::Text | Kind::Epub | Kind::Pdf => {
                                match Reader::open(&root, &entry.path) {
                                    Ok(r) => open = Some(Screen::Reader(r)),
                                    Err(e) => self.note = Some(e),
                                }
                            }
                            Kind::Audio => {
                                let (files, at) = b.siblings(Kind::Audio, &entry.path);
                                music.get_or_insert_with(Music::spawn).play(files, at);
                                open = Some(Screen::Music { cursor: at });
                            }
                            Kind::Video => {
                                open = Some(Screen::Video(Video::open(
                                    &root,
                                    &entry.path,
                                    0.0,
                                    volume,
                                )));
                            }
                            Kind::Script => return self.port(&entry.path),
                            Kind::Other => {
                                self.note = Some(format!("Nothing here opens {}", entry.name));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Some(Screen::Picture { files, at }) => {
                let n = files.len().max(1);
                match btn {
                    Btn::Left | Btn::L1 | Btn::Up => *at = (*at + n - 1) % n,
                    Btn::Right | Btn::R1 | Btn::Down | Btn::A => *at = (*at + 1) % n,
                    _ => {}
                }
            }
            Some(Screen::Reader(r)) => {
                let by = match btn {
                    Btn::Left | Btn::Up | Btn::L1 => -1,
                    Btn::Right | Btn::Down | Btn::R1 | Btn::A => 1,
                    Btn::L2 => -10,
                    Btn::R2 => 10,
                    _ => 0,
                };
                if by != 0 {
                    r.turn(&root, by);
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
            Some(Screen::Video(v)) => match btn {
                Btn::A | Btn::X => v.toggle(),
                Btn::L1 | Btn::R1 | Btn::Left | Btn::Right => {
                    let back = matches!(btn, Btn::L1 | Btn::Left);
                    let to = (v.at() + if back { -VIDEO_SKIP } else { VIDEO_SKIP }).max(0.0);
                    let path = v.path.clone();
                    // The old one's ffmpegs end before the new one's start.
                    self.stack.pop();
                    open = Some(Screen::Video(Video::open(&root, &path, to, volume)));
                }
                _ => {}
            },
            None => return Done::Leave,
        }
        if let Some(screen) = open {
            self.note = None;
            self.stack.push(screen);
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
            Screen::Launcher { cursor } => {
                let rows: Vec<ListRow> = self
                    .launcher_rows()
                    .into_iter()
                    .map(|(label, detail)| ListRow {
                        label,
                        detail: detail.into(),
                        ..ListRow::default()
                    })
                    .collect();
                let status = match (note.is_empty(), music.and_then(|m| m.playing())) {
                    (true, Some(track)) => format!("Playing {}", music::name(track)),
                    _ => note,
                };
                PageModel::List(ListPage {
                    title: "Apps".into(),
                    status,
                    cursor: (*cursor).min(rows.len().saturating_sub(1)),
                    rows,
                    legends: vec![back, Legend::new(ButtonLegend::A, "Open")],
                })
            }
            Screen::Browser(b) => {
                let rows: Vec<ListRow> = b
                    .entries
                    .iter()
                    .map(|e| ListRow {
                        label: e.name.clone(),
                        detail: e.kind.label().into(),
                        ..ListRow::default()
                    })
                    .collect();
                let status = match (note.is_empty(), rows.is_empty()) {
                    (false, _) => note,
                    (true, true) => format!("Nothing in {}", b.dir.display()),
                    (true, false) => b.place(),
                };
                PageModel::List(ListPage {
                    title: b.title.clone(),
                    status,
                    cursor: b.cursor.min(rows.len().saturating_sub(1)),
                    rows,
                    legends: vec![
                        back,
                        Legend::new(ButtonLegend::A, "Open"),
                        Legend::new(ButtonLegend::Y, "Up a Folder"),
                    ],
                })
            }
            Screen::Picture { files, at } => {
                let path = files.get(*at).cloned().unwrap_or_default();
                PageModel::Picture(PicturePage {
                    title: music::name(&path),
                    status: format!("{} of {}", at + 1, files.len()),
                    path,
                    legends: vec![back, Legend::new(ButtonLegend::Left, "Previous")],
                })
            }
            Screen::Reader(r) => {
                let status = format!("Page {} of {}", r.page + 1, r.count());
                let legends = vec![back, Legend::new(ButtonLegend::Right, "Turn")];
                match (&r.pages, &r.drawn) {
                    (Pages::Pdf { .. }, Ok(path)) => PageModel::Picture(PicturePage {
                        title: r.title.clone(),
                        status,
                        path: path.clone(),
                        legends,
                    }),
                    (Pages::Pdf { .. }, Err(e)) => PageModel::Text(TextPage {
                        title: r.title.clone(),
                        status,
                        lines: vec![e.clone()],
                        legends,
                    }),
                    (Pages::Text(_), _) => PageModel::Text(TextPage {
                        title: r.title.clone(),
                        status,
                        lines: r.lines(),
                        legends,
                    }),
                }
            }
            Screen::Music { cursor } => {
                let view = music.cloned().unwrap_or_default();
                let tracks = self.music_rows(&view);
                let playing = view.playing().map(Path::to_path_buf);
                let rows: Vec<ListRow> = tracks
                    .iter()
                    .map(|t| ListRow {
                        label: music::name(t),
                        detail: match (Some(t) == playing.as_ref(), view.paused) {
                            (true, true) => "Paused".into(),
                            (true, false) => clock(view.secs),
                            _ => String::new(),
                        },
                        ..ListRow::default()
                    })
                    .collect();
                let status = match (&view.error, &playing, rows.is_empty()) {
                    (Some(e), _, _) => e.clone(),
                    (None, _, true) => format!("Put music in {}", music::DIR),
                    (None, Some(t), _) => format!("Playing {}, on after B", music::name(t)),
                    (None, None, false) => String::new(),
                };
                PageModel::List(ListPage {
                    title: "Music".into(),
                    status,
                    cursor: (*cursor).min(rows.len().saturating_sub(1)),
                    rows,
                    legends: vec![
                        back,
                        Legend::new(ButtonLegend::A, "Play"),
                        Legend::new(ButtonLegend::X, "Pause"),
                        Legend::new(ButtonLegend::Y, "Stop"),
                    ],
                })
            }
            Screen::Video(v) => match &v.error {
                Some(e) => PageModel::Text(TextPage {
                    title: v.title.clone(),
                    status: String::new(),
                    lines: vec![e.clone()],
                    legends: vec![back],
                }),
                None => PageModel::Video(VideoModel {
                    path: v.path.clone(),
                    frame: v.frame(),
                }),
            },
        })
    }
}

/// A video's picture, for the frontend to show as the page.
#[derive(Clone, Debug)]
pub struct VideoModel {
    pub path: PathBuf,
    pub frame: video::Frame,
}

impl PartialEq for VideoModel {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.frame.serial == other.frame.serial
    }
}

fn clock(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}
