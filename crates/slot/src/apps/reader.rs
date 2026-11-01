//! Books and manuals: text and EPUBs laid out in pages, PDFs drawn a page at a time by MuPDF's
//! mutool, which a build ships in System/bin. Where each book was left is kept in Config.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::files::Kind;

/// Where each book was left: a line each, the page, a tab, then the book's path on the card.
const PLACES: &str = "Config/bookmarks.txt";
/// Where a PDF's page is drawn out to be shown.
const PDF_PAGE: &str = "Config/.reader-page.png";

pub enum Pages {
    Text(Vec<String>),
    Pdf { count: usize },
}

pub struct Reader {
    pub title: String,
    pub path: PathBuf,
    pub pages: Pages,
    pub page: usize,
    /// For a PDF: the page as drawn, or why it was not.
    pub drawn: Result<PathBuf, String>,
}

/// MuPDF's tool: the one System ships, else one on the PATH.
pub fn mutool(root: &Path) -> PathBuf {
    let ours = root.join("System/bin/mutool");
    match ours.is_file() {
        true => ours,
        false => PathBuf::from("mutool"),
    }
}

impl Reader {
    pub fn open(root: &Path, path: &Path) -> Result<Reader, String> {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Book")
            .to_string();
        let (title, pages) = match Kind::of(path) {
            Kind::Text => {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                (
                    stem,
                    Pages::Text(slot_ui::wrap_text(&String::from_utf8_lossy(&bytes))),
                )
            }
            Kind::Epub => {
                let (title, text) = super::epub::read(path).ok_or("This book will not open")?;
                (
                    title.unwrap_or(stem),
                    Pages::Text(slot_ui::wrap_text(&text)),
                )
            }
            Kind::Pdf => (
                stem,
                Pages::Pdf {
                    count: pdf_pages(root, path)?,
                },
            ),
            _ => return Err("The Reader does not open that".into()),
        };
        let mut reader = Reader {
            title,
            path: path.to_path_buf(),
            pages,
            page: 0,
            drawn: Err(String::new()),
        };
        reader.page = place(root, path).min(reader.count().saturating_sub(1));
        reader.draw(root);
        Ok(reader)
    }

    pub fn count(&self) -> usize {
        match &self.pages {
            Pages::Text(lines) => lines.len().div_ceil(slot_ui::TEXT_LINES).max(1),
            Pages::Pdf { count } => *count,
        }
    }

    /// The lines on the page showing, for text.
    pub fn lines(&self) -> Vec<String> {
        match &self.pages {
            Pages::Text(lines) => lines
                .iter()
                .skip(self.page * slot_ui::TEXT_LINES)
                .take(slot_ui::TEXT_LINES)
                .cloned()
                .collect(),
            Pages::Pdf { .. } => Vec::new(),
        }
    }

    /// Turns `by` pages, as far as the book goes, and keeps the place. False if it did not move.
    pub fn turn(&mut self, root: &Path, by: isize) -> bool {
        let to = (self.page as isize + by).clamp(0, self.count() as isize - 1) as usize;
        if to == self.page {
            return false;
        }
        self.page = to;
        self.draw(root);
        keep_place(root, &self.path, self.page);
        true
    }

    fn draw(&mut self, root: &Path) {
        if !matches!(self.pages, Pages::Pdf { .. }) {
            return;
        }
        let out = root.join(PDF_PAGE);
        let drawn = Command::new(mutool(root))
            .args(["draw", "-q", "-F", "png"])
            .arg("-w")
            .arg(slot_ui::PICTURE_W.to_string())
            .arg("-h")
            .arg(slot_ui::PICTURE_H.to_string())
            .arg("-o")
            .arg(&out)
            .arg(&self.path)
            .arg((self.page + 1).to_string())
            .output();
        self.drawn = match drawn {
            Ok(o) if o.status.success() => Ok(out),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr)
                .lines()
                .next()
                .unwrap_or("")
                .into()),
            Err(_) => Err("PDFs need mutool in System/bin".into()),
        };
    }
}

fn pdf_pages(root: &Path, path: &Path) -> Result<usize, String> {
    let out = Command::new(mutool(root))
        .arg("info")
        .arg(path)
        .output()
        .map_err(|_| "PDFs need mutool in System/bin".to_string())?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.trim().strip_prefix("Pages:")?.trim().parse().ok())
        .filter(|&n: &usize| n > 0)
        .ok_or("This PDF will not open".into())
}

fn places(root: &Path) -> Vec<(usize, String)> {
    std::fs::read_to_string(root.join(PLACES))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (page, path) = l.split_once('\t')?;
            Some((page.parse().ok()?, path.to_string()))
        })
        .collect()
}

fn key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// The page the book was left on, or the first.
pub fn place(root: &Path, path: &Path) -> usize {
    let key = key(root, path);
    places(root)
        .into_iter()
        .find(|(_, p)| *p == key)
        .map_or(0, |(page, _)| page)
}

fn keep_place(root: &Path, path: &Path, page: usize) {
    let key = key(root, path);
    let mut all = places(root);
    all.retain(|(_, p)| *p != key);
    all.insert(0, (page, key));
    all.truncate(200);
    let text: String = all.iter().map(|(n, p)| format!("{n}\t{p}\n")).collect();
    let _ = std::fs::create_dir_all(root.join("Config"));
    let _ = slot_store::atomic_write(&root.join(PLACES), text.as_bytes());
}
