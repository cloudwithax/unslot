use std::path::{Path, PathBuf};

use slot_ui::FileIcon;

/// What a file on the card is, by its extension, for what opens it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Folder,
    Picture,
    Video,
    Audio,
    Text,
    Epub,
    Pdf,
    /// A port's script.
    Script,
    Other,
}

impl Kind {
    pub fn of(path: &Path) -> Kind {
        if path.is_dir() {
            return Kind::Folder;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" => Kind::Picture,
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" => Kind::Video,
            "mp3" | "flac" | "ogg" | "oga" | "wav" => Kind::Audio,
            "txt" | "md" | "nfo" => Kind::Text,
            "epub" => Kind::Epub,
            "pdf" => Kind::Pdf,
            "sh" => Kind::Script,
            _ => Kind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Folder => "Folder",
            Kind::Picture => "Picture",
            Kind::Video => "Video",
            Kind::Audio => "Music",
            Kind::Text => "Text",
            Kind::Epub => "Book",
            Kind::Pdf => "PDF",
            Kind::Script => "Port",
            Kind::Other => "",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    /// A file's size, or how many things a folder holds.
    pub size: u64,
    /// When it was last changed, in seconds since 1970.
    pub modified: Option<i64>,
}

impl Entry {
    /// Its icon: a game where a system takes its extension, an archive for a zip that is not.
    pub fn icon(&self) -> FileIcon {
        match self.kind {
            Kind::Folder => FileIcon::Folder,
            Kind::Picture => FileIcon::Picture,
            Kind::Video => FileIcon::Video,
            Kind::Audio => FileIcon::Audio,
            Kind::Text => FileIcon::Text,
            Kind::Epub => FileIcon::Book,
            Kind::Pdf => FileIcon::Pdf,
            Kind::Script => FileIcon::Script,
            Kind::Other => {
                let game = slot_store::Platform::ALL
                    .into_iter()
                    .filter(|p| !matches!(p, slot_store::Platform::Ports | slot_store::Platform::Apps))
                    .any(|p| p.accepts(&self.path));
                let ext = self.path.extension().and_then(|e| e.to_str()).unwrap_or("");
                match (game, ext.to_ascii_lowercase().as_str()) {
                    (_, "zip" | "7z" | "rar" | "tar" | "gz") => FileIcon::Archive,
                    (true, _) => FileIcon::Game,
                    _ => FileIcon::Other,
                }
            }
        }
    }

    /// Its size as the file manager says it: "1.4 MB", or "12 items" for a folder.
    pub fn size_text(&self) -> String {
        match self.kind {
            Kind::Folder => match self.size {
                0 => "Empty".into(),
                1 => "1 item".into(),
                n => format!("{n} items"),
            },
            _ => bytes_text(self.size),
        }
    }

    /// The day it was last changed: "24 Nov 2026".
    pub fn date_text(&self) -> String {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let Some(secs) = self.modified else {
            return String::new();
        };
        let (y, m, d) = slot_store::civil_from_days(secs.div_euclid(86_400));
        format!("{d} {} {y}", MONTHS[(m as usize).clamp(1, 12) - 1])
    }
}

/// `n` bytes the way people say it: "980 B", "12 KB", "1.4 MB", "3.2 GB".
pub fn bytes_text(n: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    match v < 10.0 {
        true => format!("{v:.1} {}", UNITS[unit]),
        false => format!("{v:.0} {}", UNITS[unit]),
    }
}

/// A folder's contents: folders first, then files, each in name order, hidden ones left out.
/// With `only`, files of other kinds are left out too.
pub fn list(dir: &Path, only: Option<&[Kind]>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !slot_store::is_hidden(p))
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            let kind = Kind::of(&path);
            Some(Entry {
                name,
                path,
                kind,
                size: 0,
                modified: None,
            })
        })
        .filter(|e| e.kind == Kind::Folder || only.is_none_or(|k| k.contains(&e.kind)))
        .collect();
    for e in &mut entries {
        let meta = std::fs::metadata(&e.path).ok();
        e.modified = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        e.size = match e.kind {
            Kind::Folder => std::fs::read_dir(&e.path)
                .map(|d| d.flatten().filter(|c| !slot_store::is_hidden(&c.path())).count() as u64)
                .unwrap_or(0),
            _ => meta.map_or(0, |m| m.len()),
        };
    }
    entries.sort_by(|a, b| {
        (a.kind != Kind::Folder)
            .cmp(&(b.kind != Kind::Folder))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

/// A walk through the card's folders from `top`, never above it.
#[derive(Clone, Debug)]
pub struct Browser {
    pub title: String,
    pub top: PathBuf,
    pub dir: PathBuf,
    pub only: Option<&'static [Kind]>,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    /// Where the cursor was in each folder above, to come back to.
    above: Vec<usize>,
}

impl Browser {
    pub fn new(title: &str, top: PathBuf, only: Option<&'static [Kind]>) -> Browser {
        let _ = std::fs::create_dir_all(&top);
        let entries = list(&top, only);
        Browser {
            title: title.into(),
            dir: top.clone(),
            top,
            only,
            entries,
            cursor: 0,
            above: Vec::new(),
        }
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    pub fn enter(&mut self, dir: PathBuf) {
        self.above.push(self.cursor);
        self.dir = dir;
        self.entries = list(&self.dir, self.only);
        self.cursor = 0;
    }

    /// Up a folder; false at the top.
    pub fn up(&mut self) -> bool {
        if self.dir == self.top {
            return false;
        }
        let Some(parent) = self.dir.parent() else {
            return false;
        };
        let left = self.dir.clone();
        self.dir = parent.to_path_buf();
        self.entries = list(&self.dir, self.only);
        self.cursor = self
            .above
            .pop()
            .or_else(|| self.entries.iter().position(|e| e.path == left))
            .unwrap_or(0)
            .min(self.entries.len().saturating_sub(1));
        true
    }

    /// Lists the folder again, after something in it changed, keeping the cursor near where it
    /// was, on `select` if that is in it.
    pub fn reload(&mut self, select: Option<&Path>) {
        self.entries = list(&self.dir, self.only);
        if let Some(at) = select.and_then(|p| self.entries.iter().position(|e| e.path == p)) {
            self.cursor = at;
        }
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
    }

    /// The folders from the top down to this one, the top named `top`.
    pub fn crumbs(&self, top: &str) -> Vec<String> {
        let mut crumbs = vec![top.to_string()];
        if let Ok(rest) = self.dir.strip_prefix(&self.top) {
            crumbs.extend(rest.iter().map(|p| p.to_string_lossy().into_owned()));
        }
        crumbs
    }

    /// The folder as the page's status says it, from the top.
    pub fn place(&self) -> String {
        match self.dir.strip_prefix(&self.top) {
            Ok(rest) if rest.as_os_str().is_empty() => String::new(),
            Ok(rest) => rest.display().to_string(),
            Err(_) => self.dir.display().to_string(),
        }
    }

    /// The files of `kind` in this folder and where `path` is among them.
    pub fn siblings(&self, kind: Kind, path: &Path) -> (Vec<PathBuf>, usize) {
        let files: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.path.clone())
            .collect();
        let at = files.iter().position(|p| p == path).unwrap_or(0);
        (files, at)
    }
}

/// The first picture in `dir`, for a folder's tile in the gallery.
pub fn first_picture(dir: &Path) -> Option<PathBuf> {
    list(dir, Some(&[Kind::Picture]))
        .into_iter()
        .find(|e| e.kind == Kind::Picture)
        .map(|e| e.path)
}
