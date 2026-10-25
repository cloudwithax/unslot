//! The cores that do not come with unslot, downloaded to the card's Cores folder from unslot's
//! `cores` release. That release holds libretro's own builds for the H700, copied from its
//! buildbot once they were tried on the device, and a list of them, `cores.txt`, signed with
//! unslot's release key: a core is only kept if the list was signed by unslot and the core is
//! the one the list names.
//!
//! The list is a line saying what it is, then a line a core, `<core> <sha256> <size>`:
//!
//!   format=unslot-cores/1
//!   nestopia 0f3c...  4825352

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use slot_store::{Core, CORES_DIR};

use crate::update::{UpdateBackend, REPO};

/// The release the cores are in, which is not one of unslot's own.
pub const TAG: &str = "cores";
pub const LIST: &str = "cores.txt";
const FORMAT: &str = "format=unslot-cores/1";

/// A core the list has, by the checksum and size of its library.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Listed {
    pub core: Core,
    pub sha256: String,
    pub size: u64,
}

/// Where the release keeps `name`.
pub fn url(name: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{TAG}/{name}")
}

/// The cores a list names, once it checks out against `key` by its signature `sig`. Cores this
/// unslot does not know, or that come with it, are passed over.
pub fn parse_list(text: &str, sig: &str, key: &[u8; 32]) -> Result<Vec<Listed>, String> {
    crate::pak::verify("the list of cores", text.as_bytes(), sig, key)?;
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(FORMAT) {
        return Err("the list of cores is not one this unslot can read".into());
    }
    Ok(lines
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let core = Core::parse(words.next()?).filter(|c| c.download_only())?;
            let sha256 = words.next()?.to_ascii_lowercase();
            let size = words.next()?.parse().ok()?;
            Some(Listed { core, sha256, size })
        })
        .collect())
}

/// Where a downloaded core goes.
pub fn path(root: &Path, core: Core) -> PathBuf {
    root.join(CORES_DIR).join(core.file_name())
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CoresView {
    pub checking: bool,
    /// What the release has, once its list has been read.
    pub listed: Option<Vec<Listed>>,
    /// The cores on the card, by the checksum of each.
    pub installed: Vec<(Core, String)>,
    /// The core coming down, and how far along it is.
    pub downloading: Option<(Core, u8)>,
    /// Cores asked for that are waiting their turn.
    pub queued: Vec<Core>,
    pub error: Option<String>,
}

impl CoresView {
    pub fn listed(&self, core: Core) -> Option<&Listed> {
        self.listed.as_ref()?.iter().find(|l| l.core == core)
    }

    pub fn installed(&self, core: Core) -> bool {
        self.installed.iter().any(|(c, _)| *c == core)
    }

    /// Whether the release has a later build of a core on the card than the card's.
    pub fn outdated(&self, core: Core) -> bool {
        let listed = self.listed(core);
        self.installed
            .iter()
            .any(|(c, sum)| *c == core && listed.is_some_and(|l| l.sha256 != *sum))
    }

    /// Whether the core is on its way: coming down, or waiting to.
    pub fn pending(&self, core: Core) -> bool {
        self.downloading.is_some_and(|(c, _)| c == core) || self.queued.contains(&core)
    }
}

enum Job {
    Check,
    Download(Core),
    Remove(Core),
}

/// The handle the app keeps.
pub struct CoreDownloads {
    jobs: Sender<Job>,
    view: Arc<Mutex<CoresView>>,
}

impl CoreDownloads {
    pub fn spawn(backend: Box<dyn UpdateBackend>, root: &Path) -> CoreDownloads {
        let (jobs, rx) = channel();
        let view = Arc::new(Mutex::new(CoresView {
            installed: on_card(root),
            ..CoresView::default()
        }));
        let shared = view.clone();
        let root = root.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("slot-cores".into())
            .spawn(move || work(backend, root, rx, shared));
        if let Err(e) = spawned {
            eprintln!("slot: cores: {e}");
        }
        CoreDownloads { jobs, view }
    }

    /// The downloader for this build: unslot's release through curl on the device, a pretend
    /// one anywhere else.
    pub fn for_card(root: &Path) -> CoreDownloads {
        #[cfg(all(feature = "device", unix))]
        let backend = crate::update::backend(root).0;
        #[cfg(not(all(feature = "device", unix)))]
        let backend: Box<dyn UpdateBackend> = Box::new(SimCores);
        CoreDownloads::spawn(backend, root)
    }

    pub fn check(&self) {
        let _ = self.jobs.send(Job::Check);
    }

    pub fn download(&self, core: Core) {
        let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
        if view.pending(core) {
            return;
        }
        view.queued.push(core);
        let _ = self.jobs.send(Job::Download(core));
    }

    pub fn remove(&self, core: Core) {
        let _ = self.jobs.send(Job::Remove(core));
    }

    pub fn view(&self) -> CoresView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// The downloaded cores on the card, by the checksum of each.
fn on_card(root: &Path) -> Vec<(Core, String)> {
    Core::ALL
        .into_iter()
        .filter(|c| c.download_only())
        .filter_map(|c| Some((c, sha256(&std::fs::read(path(root, c)).ok()?))))
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn read_list(backend: &mut dyn UpdateBackend) -> Result<Vec<Listed>, String> {
    let text = backend.get(&url(LIST))?;
    let sig = backend.get(&url(&format!("{LIST}.sig")))?;
    parse_list(&text, &sig, &backend.release_key())
}

fn work(
    mut backend: Box<dyn UpdateBackend>,
    root: PathBuf,
    jobs: Receiver<Job>,
    view: Arc<Mutex<CoresView>>,
) {
    let set =
        |f: &mut dyn FnMut(&mut CoresView)| f(&mut view.lock().unwrap_or_else(|e| e.into_inner()));
    let mut listed: Option<Vec<Listed>> = None;
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Check => {
                set(&mut |v| {
                    v.checking = true;
                    v.error = None;
                });
                let read = read_list(&mut *backend);
                if let Err(e) = &read {
                    eprintln!("slot: cores: {e}");
                }
                listed = read.as_ref().ok().cloned().or(listed);
                set(&mut |v| {
                    v.checking = false;
                    v.listed = listed.clone();
                    v.error = read.as_ref().err().cloned();
                });
            }
            Job::Download(core) => {
                set(&mut |v| {
                    v.queued.retain(|c| *c != core);
                    v.downloading = Some((core, 0));
                    v.error = None;
                });
                let list = match &listed {
                    Some(l) => Ok(l.clone()),
                    None => read_list(&mut *backend),
                };
                if let Ok(l) = &list {
                    listed = Some(l.clone());
                }
                let fetched = list.and_then(|list| {
                    let l = list
                        .into_iter()
                        .find(|l| l.core == core)
                        .ok_or(format!("{} is not in unslot's cores", core.text()))?;
                    let size = l.size.max(1);
                    std::fs::create_dir_all(root.join(CORES_DIR)).map_err(|e| e.to_string())?;
                    backend.fetch(
                        &url(&core.file_name()),
                        &l.sha256,
                        &path(&root, core),
                        &mut |bytes| {
                            let pct = (bytes.saturating_mul(100) / size).min(99) as u8;
                            set(&mut |v| v.downloading = Some((core, pct)));
                        },
                    )?;
                    Ok(l.sha256)
                });
                if let Err(e) = &fetched {
                    eprintln!("slot: cores: {}: {e}", core.as_str());
                }
                set(&mut |v| {
                    v.downloading = None;
                    v.listed = listed.clone();
                    match &fetched {
                        Ok(sum) => {
                            v.installed.retain(|(c, _)| *c != core);
                            v.installed.push((core, sum.clone()));
                        }
                        Err(e) => v.error = Some(format!("{}: {e}", core.text())),
                    }
                });
            }
            Job::Remove(core) => {
                let removed = std::fs::remove_file(path(&root, core));
                if let Err(e) = &removed {
                    eprintln!("slot: cores: {}: {e}", core.as_str());
                }
                set(&mut |v| {
                    v.installed.retain(|(c, _)| *c != core || removed.is_err());
                });
            }
        }
    }
}

/// A pretend release for the desktop and the tests: every downloaded core, each the bytes
/// `SimCores::CORE`, in a list signed with the pretend releases' key.
pub struct SimCores;

impl SimCores {
    pub const CORE: &'static [u8] = b"pretend";

    pub fn list() -> String {
        let sum = sha256(SimCores::CORE);
        let mut text = format!("{FORMAT}\n");
        for core in Core::ALL.into_iter().filter(|c| c.download_only()) {
            text.push_str(&format!(
                "{} {sum} {}\n",
                core.as_str(),
                SimCores::CORE.len()
            ));
        }
        text
    }
}

impl UpdateBackend for SimCores {
    fn get(&mut self, url: &str) -> Result<String, String> {
        let list = SimCores::list();
        Ok(match url.ends_with(".sig") {
            true => crate::pak::build::sign(list.as_bytes(), crate::pak::build::TEST_SEED),
            false => list,
        })
    }

    fn fetch(
        &mut self,
        _: &str,
        _: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), String> {
        progress(3);
        std::fs::write(dest, SimCores::CORE).map_err(|e| e.to_string())
    }

    fn release_key(&self) -> [u8; 32] {
        crate::pak::build::test_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pak::build::{sign, test_key, TEST_SEED};
    use std::time::{Duration, Instant};

    fn signed(text: &str) -> (String, String) {
        (text.to_string(), sign(text.as_bytes(), TEST_SEED))
    }

    #[test]
    fn the_mirror_copies_every_core_unslot_downloads() {
        let list = include_str!("../../../tools/cores/cores.list");
        let named: Vec<&str> = list
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .collect();
        let downloaded: Vec<&str> = Core::ALL
            .into_iter()
            .filter(|c| c.download_only())
            .map(|c| c.as_str())
            .collect();
        assert_eq!(named, downloaded);
        for core in Core::ALL.into_iter().filter(|c| c.download_only()) {
            assert!(
                slot_store::Platform::ALL.into_iter().any(|p| core.runs(p)),
                "no system runs {core:?}"
            );
        }
    }

    #[test]
    fn a_signed_list_names_its_cores() {
        let (text, sig) = signed(&format!(
            "{FORMAT}\nnestopia {} 4096\nmgba {} 10\nnot_a_core {} 1\n",
            "AB".repeat(32),
            "c".repeat(64),
            "d".repeat(64)
        ));
        let listed = parse_list(&text, &sig, &test_key()).unwrap();
        assert_eq!(
            listed,
            [Listed {
                core: Core::Nestopia,
                sha256: "ab".repeat(32),
                size: 4096
            }],
            "a core unslot comes with, or does not know, was listed"
        );
    }

    #[test]
    fn a_list_unslot_did_not_sign_is_refused() {
        let (text, sig) = signed(&format!("{FORMAT}\nnestopia {} 4096\n", "a".repeat(64)));
        let changed = text.replace("4096", "4097");
        let e = parse_list(&changed, &sig, &test_key()).unwrap_err();
        assert!(e.contains("not signed by unslot"), "{e}");
        let e = parse_list(&text, &sig, &crate::pak::RELEASE_KEY).unwrap_err();
        assert!(e.contains("not signed by unslot"), "{e}");
    }

    #[test]
    fn a_list_in_another_format_is_refused() {
        let (text, sig) = signed("format=unslot-cores/2\nnestopia x 1\n");
        assert!(parse_list(&text, &sig, &test_key()).is_err());
    }

    fn wait(cores: &CoreDownloads, done: impl Fn(&CoresView) -> bool) -> CoresView {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let v = cores.view();
            if done(&v) || Instant::now() > until {
                return v;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_core_downloads_to_the_cores_folder_and_can_be_removed() {
        let root = tempfile::tempdir().unwrap();
        let cores = CoreDownloads::spawn(Box::new(SimCores), root.path());
        assert!(!cores.view().installed(Core::Nestopia));
        assert!(!Core::Nestopia.installed(root.path()));

        cores.download(Core::Nestopia);
        let v = wait(&cores, |v| v.installed(Core::Nestopia));
        assert!(v.installed(Core::Nestopia), "{v:?}");
        assert!(!v.outdated(Core::Nestopia));
        assert!(!v.pending(Core::Nestopia));
        assert!(Core::Nestopia.installed(root.path()));

        cores.remove(Core::Nestopia);
        let v = wait(&cores, |v| !v.installed(Core::Nestopia));
        assert!(!v.installed(Core::Nestopia), "{v:?}");
        assert!(!Core::Nestopia.installed(root.path()));
    }

    #[test]
    fn a_core_on_the_card_that_the_list_has_moved_past_is_outdated() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(CORES_DIR)).unwrap();
        std::fs::write(path(root.path(), Core::Gambatte), b"older").unwrap();
        let cores = CoreDownloads::spawn(Box::new(SimCores), root.path());
        assert!(cores.view().installed(Core::Gambatte));
        cores.check();
        let v = wait(&cores, |v| v.listed.is_some());
        assert!(v.outdated(Core::Gambatte), "{v:?}");
        assert!(
            !v.outdated(Core::Nestopia),
            "a core not on the card was outdated"
        );
    }

    /// A release whose list was not signed by unslot.
    struct Forged;

    impl UpdateBackend for Forged {
        fn get(&mut self, url: &str) -> Result<String, String> {
            let list = SimCores::list();
            Ok(match url.ends_with(".sig") {
                true => sign(list.as_bytes(), [9; 32]),
                false => list,
            })
        }

        fn fetch(
            &mut self,
            _: &str,
            _: &str,
            dest: &Path,
            _: &mut dyn FnMut(u64),
        ) -> Result<(), String> {
            std::fs::write(dest, SimCores::CORE).map_err(|e| e.to_string())
        }

        fn release_key(&self) -> [u8; 32] {
            test_key()
        }
    }

    #[test]
    fn nothing_is_downloaded_from_a_list_unslot_did_not_sign() {
        let root = tempfile::tempdir().unwrap();
        let cores = CoreDownloads::spawn(Box::new(Forged), root.path());
        cores.download(Core::Nestopia);
        let v = wait(&cores, |v| v.error.is_some());
        assert!(v.error.is_some(), "{v:?}");
        assert!(!v.installed(Core::Nestopia));
        assert!(!path(root.path(), Core::Nestopia).exists());
    }
}
