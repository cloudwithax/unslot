use std::path::{Path, PathBuf};

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
            Some(Entry { name, path, kind })
        })
        .filter(|e| e.kind == Kind::Folder || only.is_none_or(|k| k.contains(&e.kind)))
        .collect();
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
