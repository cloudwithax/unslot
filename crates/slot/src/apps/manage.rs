//! The file manager's work on the card: copying, on a thread for its size, with how far it has
//! got; moving, renaming and making folders, which are quick; and deleting.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// What the file manager must not touch: slot itself.
const KEPT: &[&str] = &["System"];

/// Whether `path` is one of the card's own folders, which stay where they are.
pub fn kept(root: &Path, path: &Path) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .ok()
            .and_then(|rest| rest.to_str())
            .is_some_and(|rest| KEPT.contains(&rest))
}

/// A name for `name` in `dir` that nothing there has yet: "Notes (2).txt" beside "Notes.txt".
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Whether `name` will do as a file's name: not empty, no slashes, not a dot or two.
pub fn good_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

/// Moves `from` into `dir`, under a name of its own there.
pub fn move_into(from: &Path, dir: &Path) -> Result<PathBuf, String> {
    if dir.starts_with(from) {
        return Err("A folder will not go inside itself".into());
    }
    if from.parent() == Some(dir) {
        return Ok(from.to_path_buf());
    }
    let name = from.file_name().and_then(|n| n.to_str()).ok_or("That name will not do")?;
    let to = free_name(dir, name);
    std::fs::rename(from, &to).map_err(|e| e.to_string())?;
    Ok(to)
}

pub fn rename(from: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if !good_name(name) {
        return Err("That name will not do".into());
    }
    let to = from.with_file_name(name);
    if to == from {
        return Ok(to);
    }
    if to.exists() {
        return Err(format!("There is already a {name} here"));
    }
    std::fs::rename(from, &to).map_err(|e| e.to_string())?;
    Ok(to)
}

pub fn new_folder(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if !good_name(name) {
        return Err("That name will not do".into());
    }
    let to = dir.join(name);
    if to.exists() {
        return Err(format!("There is already a {name} here"));
    }
    std::fs::create_dir(&to).map_err(|e| e.to_string())?;
    Ok(to)
}

pub fn delete(path: &Path) -> Result<(), String> {
    let gone = match path.is_dir() {
        true => std::fs::remove_dir_all(path),
        false => std::fs::remove_file(path),
    };
    gone.map_err(|e| e.to_string())
}

/// A copy under way.
pub struct Copy {
    pub name: String,
    /// Where it is going.
    pub to: PathBuf,
    pub total: u64,
    done: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    ended: Arc<Mutex<Option<Result<(), String>>>>,
}

impl Copy {
    /// Starts copying `from` into `dir`, under a name of its own there.
    pub fn start(from: &Path, dir: &Path) -> Result<Copy, String> {
        if dir.starts_with(from) {
            return Err("A folder will not go inside itself".into());
        }
        let name = from.file_name().and_then(|n| n.to_str()).ok_or("That name will not do")?;
        let to = free_name(dir, name);
        let total = size_of(from);
        if crate::port_runtime::free_bytes(dir).is_some_and(|free| free < total) {
            return Err("Not enough room on the card".into());
        }
        let copy = Copy {
            name: name.to_string(),
            to: to.clone(),
            total,
            done: Arc::default(),
            stop: Arc::default(),
            ended: Arc::default(),
        };
        let (done, stop, ended) = (copy.done.clone(), copy.stop.clone(), copy.ended.clone());
        let from = from.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("slot-copy".into())
            .spawn(move || {
                let result = copy_all(&from, &to, &done, &stop);
                if result.is_err() {
                    let _ = delete(&to);
                }
                *ended.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            });
        spawned.map_err(|e| e.to_string())?;
        Ok(copy)
    }

    /// How much of it is done, from 0 to 1.
    pub fn progress(&self) -> f32 {
        match self.total {
            0 => 1.0,
            t => (self.done.load(Ordering::Relaxed) as f64 / t as f64).min(1.0) as f32,
        }
    }

    pub fn done_bytes(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    /// Stops it; what was copied is taken away again.
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// How it ended, once it has.
    pub fn ended(&self) -> Option<Result<(), String>> {
        self.ended.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn size_of(path: &Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| size_of(&e.path()))
            .sum(),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

fn copy_all(from: &Path, to: &Path, done: &AtomicU64, stop: &AtomicBool) -> Result<(), String> {
    use std::io::{Read, Write};
    if stop.load(Ordering::Relaxed) {
        return Err("Stopped".into());
    }
    if from.is_dir() {
        std::fs::create_dir(to).map_err(|e| e.to_string())?;
        for e in std::fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
            copy_all(&e.path(), &to.join(e.file_name()), done, stop)?;
        }
        return Ok(());
    }
    let mut src = std::fs::File::open(from).map_err(|e| e.to_string())?;
    let mut dst = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err("Stopped".into());
        }
        let n = src.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done.fetch_add(n as u64, Ordering::Relaxed);
    }
    dst.sync_all().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_taken_gets_a_number() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Notes.txt"), "").unwrap();
        std::fs::write(d.path().join("Notes (2).txt"), "").unwrap();
        std::fs::create_dir(d.path().join("Saves")).unwrap();
        assert_eq!(free_name(d.path(), "Notes.txt"), d.path().join("Notes (3).txt"));
        assert_eq!(free_name(d.path(), "Saves"), d.path().join("Saves (2)"));
        assert_eq!(free_name(d.path(), "New.txt"), d.path().join("New.txt"));
    }

    #[test]
    fn a_folder_copies_whole_and_will_not_go_inside_itself() {
        let d = tempfile::tempdir().unwrap();
        let from = d.path().join("A");
        std::fs::create_dir_all(from.join("B")).unwrap();
        std::fs::write(from.join("B/c.txt"), "hello").unwrap();
        let copy = Copy::start(&from, d.path()).unwrap();
        while copy.ended().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(copy.ended(), Some(Ok(())));
        assert_eq!(copy.to, d.path().join("A (2)"));
        assert_eq!(std::fs::read_to_string(d.path().join("A (2)/B/c.txt")).unwrap(), "hello");
        assert!(Copy::start(&from, &from.join("B")).is_err());
        assert!(move_into(&from, &from.join("B")).is_err());
    }

    #[test]
    fn slots_own_folder_is_kept() {
        let root = Path::new("/card");
        assert!(kept(root, Path::new("/card/System")));
        assert!(kept(root, root));
        assert!(!kept(root, Path::new("/card/System/bin")));
        assert!(!kept(root, Path::new("/card/Games")));
    }
}
