//! Updates for unslot and for the BaseOS under it, from their GitHub releases.
//!
//! Each is downloaded to the card's root, and restarting puts it in. BaseOS writes a `.bosupd` to
//! its spare rootfs and boots that on trial. unslot's comes as `unslot-<version>.pak`, which
//! System/launch_frontend.sh checks is signed by unslot (see `pak`) before it installs the
//! `MinUI.zip` inside: that replaces System/, and the launcher gives the new one three starts to
//! draw a frame before it puts the old one back. `confirm` is how a start says it drew one.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::json;

pub const REPO: &str = "cloudwithax/unslot";
pub const BASEOS_REPO: &str = "pvaibhav/BaseOS";
/// Each release's checksums, as `sha256sum` writes them.
pub const SUMS: &str = "SHA256SUMS";
const UPDATE_DIR: &str = ".unslot-update";
const VERSION_FILE: &str = "System/version";

/// Marks the System folder an update put in as having started, so it is kept.
pub fn confirm(root: &Path) {
    let dir = root.join(UPDATE_DIR);
    if std::fs::remove_file(dir.join("trial")).is_ok() {
        let _ = std::fs::remove_dir_all(dir.join("System.old"));
        eprintln!("slot: update: this System came up, so the one it replaced is gone");
    }
}

/// The release this System came from, which the release writes beside slot.
pub fn installed_version(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join(VERSION_FILE))
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Whether `latest` is later than `current`, each as `v1.2.3` or `1.2.3`. A current version
/// that is not a release's, a build off a branch say, is behind any release.
pub fn newer(latest: &str, current: &str) -> bool {
    match (numbers(latest), numbers(current)) {
        (Some(l), Some(c)) => l > c,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn numbers(version: &str) -> Option<Vec<u64>> {
    let core = version
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

/// A release from GitHub's `releases/latest`. Each asset is read from its download URL, whose
/// last part is its name, and the size listed ahead of it.
pub fn parse_release(text: &str) -> Option<Release> {
    let tag = json::string(text, "tag_name")?;
    const KEY: &str = "\"browser_download_url\"";
    let mut assets = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(KEY) {
        let (before, after) = rest.split_at(at);
        let url = json::string(after, "browser_download_url")?;
        let size = before
            .rfind("\"size\"")
            .and_then(|i| json::number(&before[i..], "size"))
            .unwrap_or(0);
        assets.push(Asset {
            name: url.rsplit('/').next()?.to_string(),
            url,
            size,
        });
        rest = &after[KEY.len()..];
    }
    Some(Release { tag, assets })
}

/// The checksum `sha256sum` gave `name`.
pub fn parse_sums(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (sum, file) = line.split_once(char::is_whitespace)?;
        if file.trim_start().trim_start_matches('*') == name {
            Some(sum.to_ascii_lowercase())
        } else {
            None
        }
    })
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Part {
    Unslot,
    BaseOs,
}

impl Part {
    pub fn name(self) -> &'static str {
        match self {
            Part::Unslot => "unslot",
            Part::BaseOs => "BaseOS",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Stage {
    #[default]
    Unchecked,
    Checking,
    UpToDate,
    /// A later release, by its version.
    Available(String),
    Downloading(String, u8),
    /// Downloaded and where BaseOS will find it at the next start.
    Ready(String),
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Track {
    pub current: Option<String>,
    pub stage: Stage,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct UpdatesView {
    pub unslot: Track,
    /// None when slot is not running on BaseOS.
    pub baseos: Option<Track>,
    pub error: Option<String>,
}

impl UpdatesView {
    pub fn track(&self, part: Part) -> Option<&Track> {
        match part {
            Part::Unslot => Some(&self.unslot),
            Part::BaseOs => self.baseos.as_ref(),
        }
    }

    fn track_mut(&mut self, part: Part) -> Option<&mut Track> {
        match part {
            Part::Unslot => Some(&mut self.unslot),
            Part::BaseOs => self.baseos.as_mut(),
        }
    }

    pub fn parts(&self) -> Vec<Part> {
        let mut parts = vec![Part::Unslot];
        if self.baseos.is_some() {
            parts.push(Part::BaseOs);
        }
        parts
    }
}

/// The BaseOS slot runs on, from /etc/baseos-release.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BaseOs {
    pub version: String,
    /// The model its releases are built per, `rg35xxsp` say.
    pub target: String,
}

/// Fetching, as the worker does it.
pub trait UpdateBackend: Send {
    fn get(&mut self, url: &str) -> Result<String, String>;
    /// Downloads `url` to `dest`, refusing it unless it has the checksum, and says how many
    /// bytes have come as they come.
    fn fetch(
        &mut self,
        url: &str,
        sha256: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), String>;

    /// The key unslot's packs are signed with.
    fn release_key(&self) -> [u8; 32] {
        crate::pak::RELEASE_KEY
    }
}

enum Job {
    Check,
    Download(Part),
}

/// The handle the app keeps.
pub struct Updates {
    jobs: Sender<Job>,
    view: Arc<Mutex<UpdatesView>>,
}

impl Updates {
    pub fn spawn(backend: Box<dyn UpdateBackend>, root: &Path, baseos: Option<BaseOs>) -> Updates {
        let (jobs, rx) = channel();
        let view = Arc::new(Mutex::new(UpdatesView {
            unslot: Track {
                current: installed_version(root),
                stage: Stage::Unchecked,
            },
            baseos: baseos.as_ref().map(|b| Track {
                current: Some(b.version.clone()),
                stage: Stage::Unchecked,
            }),
            error: None,
        }));
        let shared = view.clone();
        let root = root.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("slot-updates".into())
            .spawn(move || work(backend, root, baseos, rx, shared));
        if let Err(e) = spawned {
            eprintln!("slot: updates: {e}");
        }
        Updates { jobs, view }
    }

    /// The updater for this build: GitHub through curl on the device, pretend releases anywhere
    /// else.
    pub fn for_card(root: &Path) -> Updates {
        let (backend, baseos) = backend(root);
        Updates::spawn(backend, root, baseos)
    }

    pub fn check(&self) {
        let _ = self.jobs.send(Job::Check);
    }

    pub fn download(&self, part: Part) {
        let _ = self.jobs.send(Job::Download(part));
    }

    pub fn view(&self) -> UpdatesView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// What a check found for one part: its latest release's file, and the checksum to hold it to.
struct Found {
    version: String,
    asset: Asset,
    sha256: String,
}

fn work(
    mut backend: Box<dyn UpdateBackend>,
    root: PathBuf,
    baseos: Option<BaseOs>,
    jobs: Receiver<Job>,
    view: Arc<Mutex<UpdatesView>>,
) {
    let set = |f: &mut dyn FnMut(&mut UpdatesView)| {
        f(&mut view.lock().unwrap_or_else(|e| e.into_inner()))
    };
    let mut found: Vec<(Part, Found)> = Vec::new();
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Check => {
                let parts = view.lock().unwrap_or_else(|e| e.into_inner()).parts();
                set(&mut |v| {
                    v.error = None;
                    for part in &parts {
                        if let Some(t) = v.track_mut(*part) {
                            t.stage = Stage::Checking;
                        }
                    }
                });
                for part in parts {
                    let looked = look(&mut *backend, part, baseos.as_ref());
                    found.retain(|(p, _)| *p != part);
                    set(&mut |v| {
                        let error = match &looked {
                            Err(e) => Some(format!("{}: {e}", part.name())),
                            Ok(_) => None,
                        };
                        let Some(t) = v.track_mut(part) else {
                            return;
                        };
                        t.stage = match &looked {
                            Ok(f) if root.join(&f.asset.name).exists() => {
                                Stage::Ready(f.version.clone())
                            }
                            Ok(f) if newer(&f.version, t.current.as_deref().unwrap_or("")) => {
                                Stage::Available(f.version.clone())
                            }
                            Ok(_) => Stage::UpToDate,
                            Err(_) => Stage::Unchecked,
                        };
                        v.error = v.error.take().or(error);
                    });
                    if let Err(e) = &looked {
                        eprintln!("slot: updates: {}: {e}", part.name());
                    }
                    if let Ok(f) = looked {
                        found.push((part, f));
                    }
                }
            }
            Job::Download(part) => {
                let Some((_, f)) = found.iter().find(|(p, _)| *p == part) else {
                    continue;
                };
                let version = f.version.clone();
                let dest = root.join(&f.asset.name);
                set(&mut |v| {
                    v.error = None;
                    if let Some(t) = v.track_mut(part) {
                        t.stage = Stage::Downloading(version.clone(), 0);
                    }
                });
                let size = f.asset.size.max(1);
                let fetched = backend.fetch(&f.asset.url, &f.sha256, &dest, &mut |bytes| {
                    let pct = (bytes.saturating_mul(100) / size).min(99) as u8;
                    set(&mut |v| {
                        if let Some(t) = v.track_mut(part) {
                            t.stage = Stage::Downloading(version.clone(), pct);
                        }
                    });
                });
                // A pack is only kept if unslot signed it, as the launcher will not install it else.
                let fetched = fetched.and_then(|()| match part {
                    Part::Unslot => {
                        let bytes = std::fs::read(&dest).map_err(|e| e.to_string())?;
                        crate::pak::check(&bytes, &backend.release_key())
                            .map(|_| ())
                            .inspect_err(|_| {
                                let _ = std::fs::remove_file(&dest);
                            })
                    }
                    Part::BaseOs => Ok(()),
                });
                if let Err(e) = &fetched {
                    eprintln!("slot: updates: {} {version}: {e}", part.name());
                }
                set(&mut |v| {
                    if let Some(t) = v.track_mut(part) {
                        t.stage = match fetched {
                            Ok(()) => Stage::Ready(version.clone()),
                            Err(_) => Stage::Available(version.clone()),
                        };
                    }
                    if let Err(e) = &fetched {
                        v.error = Some(format!("{}: {e}", part.name()));
                    }
                });
            }
        }
    }
}

/// Finds `part`'s latest release, its file for this device, and that file's checksum.
fn look(
    backend: &mut dyn UpdateBackend,
    part: Part,
    baseos: Option<&BaseOs>,
) -> Result<Found, String> {
    let repo = match part {
        Part::Unslot => REPO,
        Part::BaseOs => BASEOS_REPO,
    };
    let listing = backend.get(&format!(
        "https://api.github.com/repos/{repo}/releases/latest"
    ))?;
    let release = parse_release(&listing).ok_or("GitHub's answer could not be read")?;
    let name = match (part, baseos) {
        (Part::Unslot, _) => crate::pak::file_name(&release.tag),
        (Part::BaseOs, Some(b)) => format!(
            "baseos-{}-{}.bosupd",
            b.target,
            release.tag.trim_start_matches('v')
        ),
        (Part::BaseOs, None) => return Err("not running on BaseOS".into()),
    };
    let tag = &release.tag;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| format!("{tag} has no {name}"))?
        .clone();
    let sums = release
        .assets
        .iter()
        .find(|a| a.name == SUMS)
        .ok_or_else(|| format!("{tag} has no checksums"))?;
    let sha256 = parse_sums(&backend.get(&sums.url)?, &name)
        .ok_or_else(|| format!("{name} has no checksum"))?;
    Ok(Found {
        version: release.tag.clone(),
        asset,
        sha256,
    })
}

fn backend(root: &Path) -> (Box<dyn UpdateBackend>, Option<BaseOs>) {
    #[cfg(all(feature = "device", unix))]
    {
        let baseos = crate::baseos::var("BASEOS_VERSION")
            .zip(crate::baseos::var("BASEOS_TARGET"))
            .map(|(version, target)| BaseOs { version, target });
        (Box::new(device::DeviceUpdates::new(root)), baseos)
    }
    #[cfg(not(all(feature = "device", unix)))]
    {
        let _ = root;
        (Box::new(SimUpdates), Some(SimUpdates::baseos()))
    }
}

/// Pretend releases for the desktop and the tests: unslot `UNSLOT` and BaseOS `BASEOS`, which a
/// device on `SimUpdates::baseos` has not got yet.
pub struct SimUpdates;

impl SimUpdates {
    pub const UNSLOT: &'static str = "v2.1.0";
    pub const BASEOS: &'static str = "v1.5.0";
    pub const TARGET: &'static str = "rg35xxsp";

    pub fn baseos() -> BaseOs {
        BaseOs {
            version: "1.4.0".into(),
            target: SimUpdates::TARGET.into(),
        }
    }

    fn listing(repo: &str, tag: &str, file: &str) -> String {
        let url = |name: &str| format!("https://github.com/{repo}/releases/download/{tag}/{name}");
        format!(
            r#"{{"tag_name": "{tag}", "assets": [{{"name": "{file}", "size": 4096, "browser_download_url": "{}"}}, {{"name": "{SUMS}", "size": 90, "browser_download_url": "{}"}}]}}"#,
            url(file),
            url(SUMS)
        )
    }
}

impl UpdateBackend for SimUpdates {
    fn get(&mut self, url: &str) -> Result<String, String> {
        let bosupd = format!(
            "baseos-{}-{}.bosupd",
            SimUpdates::TARGET,
            SimUpdates::BASEOS.trim_start_matches('v')
        );
        Ok(match url {
            u if u.ends_with(SUMS) => format!(
                "{}  {}\n{}  {bosupd}\n",
                "a".repeat(64),
                crate::pak::file_name(SimUpdates::UNSLOT),
                "b".repeat(64)
            ),
            u if u.contains(BASEOS_REPO) => {
                SimUpdates::listing(BASEOS_REPO, SimUpdates::BASEOS, &bosupd)
            }
            _ => SimUpdates::listing(
                REPO,
                SimUpdates::UNSLOT,
                &crate::pak::file_name(SimUpdates::UNSLOT),
            ),
        })
    }

    fn fetch(
        &mut self,
        _: &str,
        _: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), String> {
        progress(2048);
        let pretend = match dest.extension().is_some_and(|e| e == "pak") {
            true => crate::pak::build::pack(
                SimUpdates::UNSLOT,
                b"pretend",
                crate::pak::build::TEST_SEED,
                |_| {},
            ),
            false => b"pretend".to_vec(),
        };
        std::fs::write(dest, pretend).map_err(|e| e.to_string())
    }

    fn release_key(&self) -> [u8; 32] {
        crate::pak::build::test_key()
    }
}

#[cfg(all(feature = "device", unix))]
mod device {
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use super::UpdateBackend;

    pub struct DeviceUpdates {
        script: PathBuf,
    }

    impl DeviceUpdates {
        pub fn new(root: &Path) -> DeviceUpdates {
            DeviceUpdates {
                script: root.join("System/slotupdate.sh"),
            }
        }
    }

    fn complaint(stderr: &[u8]) -> String {
        String::from_utf8_lossy(stderr)
            .lines()
            .last()
            .unwrap_or("failed")
            .to_string()
    }

    impl UpdateBackend for DeviceUpdates {
        fn get(&mut self, url: &str) -> Result<String, String> {
            let out = Command::new("/bin/sh")
                .arg(&self.script)
                .args(["get", url])
                .stdin(Stdio::null())
                .output()
                .map_err(|e| e.to_string())?;
            match out.status.success() {
                true => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
                false => Err(complaint(&out.stderr)),
            }
        }

        fn fetch(
            &mut self,
            url: &str,
            sha256: &str,
            dest: &Path,
            progress: &mut dyn FnMut(u64),
        ) -> Result<(), String> {
            let mut child = Command::new("/bin/sh")
                .arg(&self.script)
                .args(["fetch", url, sha256])
                .arg(dest)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?;
            let mut part = dest.as_os_str().to_owned();
            part.push(".part");
            let status = loop {
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    break status;
                }
                progress(std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0));
                std::thread::sleep(Duration::from_millis(250));
            };
            let mut stderr = Vec::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_end(&mut stderr);
            }
            match status.success() {
                true => Ok(()),
                false => Err(complaint(&stderr)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(newer("v2.1.0", "v2.0.9"));
        assert!(newer("v2.10.0", "v2.9.0"), "10 was read as text");
        assert!(newer("1.5.0", "1.4.0"));
        assert!(!newer("v2.0.0", "v2.0.0"));
        assert!(!newer("v2.0.0", "v2.1.0"));
        assert!(
            !newer("v2.0.0", "v2.0.0-3-gabc1234"),
            "a build past a tag is not behind it"
        );
        assert!(
            newer("v2.0.0", "abc1234"),
            "a build off no tag is behind any release"
        );
        assert!(!newer("nightly", "v1.0.0"));
    }

    #[test]
    fn a_github_release_reads_into_its_tag_and_files() {
        // Trimmed from a real `releases/latest`, uploader and all.
        let json = r#"{"url":"https://api.github.com/repos/pvaibhav/BaseOS/releases/1","tag_name":"v1.4.0","name":"v1.4.0","assets":[
            {"url":"https://api.github.com/repos/pvaibhav/BaseOS/releases/assets/1","id":1,"name":"baseos-rgsp-1.4.0.bosupd","uploader":{"login":"pvaibhav","id":9},"content_type":"application/octet-stream","state":"uploaded","size":187654321,"digest":"sha256:ab","download_count":3,"browser_download_url":"https://github.com/pvaibhav/BaseOS/releases/download/v1.4.0/baseos-rgsp-1.4.0.bosupd"},
            {"url":"https://api.github.com/repos/pvaibhav/BaseOS/releases/assets/2","id":2,"name":"SHA256SUMS","size":1500,"browser_download_url":"https://github.com/pvaibhav/BaseOS/releases/download/v1.4.0/SHA256SUMS"}
        ],"body":"Notes with a \"size\": 5 in them"}"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.tag, "v1.4.0");
        assert_eq!(
            r.assets,
            [
                Asset {
                    name: "baseos-rgsp-1.4.0.bosupd".into(),
                    url: "https://github.com/pvaibhav/BaseOS/releases/download/v1.4.0/baseos-rgsp-1.4.0.bosupd".into(),
                    size: 187654321,
                },
                Asset {
                    name: "SHA256SUMS".into(),
                    url: "https://github.com/pvaibhav/BaseOS/releases/download/v1.4.0/SHA256SUMS".into(),
                    size: 1500,
                },
            ]
        );
    }

    #[test]
    fn a_checksum_is_found_by_its_file() {
        let sums = "AB12  baseos-rgsp-1.4.0.bosupd\ncd34 *MinUI.zip\n";
        assert_eq!(parse_sums(sums, "MinUI.zip").as_deref(), Some("cd34"));
        assert_eq!(
            parse_sums(sums, "baseos-rgsp-1.4.0.bosupd").as_deref(),
            Some("ab12")
        );
        assert_eq!(parse_sums(sums, "other"), None);
    }

    #[test]
    fn confirming_keeps_the_new_system_and_drops_the_old() {
        let d = tempfile::tempdir().unwrap();
        let upd = d.path().join(UPDATE_DIR);
        std::fs::create_dir_all(upd.join("System.old")).unwrap();
        std::fs::write(upd.join("trial"), "1").unwrap();
        confirm(d.path());
        assert!(!upd.join("trial").exists());
        assert!(!upd.join("System.old").exists());
    }

    fn settle(u: &Updates, until: impl Fn(&UpdatesView) -> bool) -> UpdatesView {
        let start = Instant::now();
        loop {
            let v = u.view();
            if until(&v) || start.elapsed() > Duration::from_secs(10) {
                return v;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn an_older_unslot_finds_the_release_and_downloads_its_pack_to_the_root() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("System")).unwrap();
        std::fs::write(d.path().join(VERSION_FILE), "v2.0.0\n").unwrap();
        let u = Updates::spawn(Box::new(SimUpdates), d.path(), Some(SimUpdates::baseos()));
        u.check();
        let v = settle(&u, |v| {
            v.unslot.stage == Stage::Available(SimUpdates::UNSLOT.into())
        });
        assert_eq!(v.unslot.current.as_deref(), Some("v2.0.0"));
        let v = settle(&u, |v| {
            v.baseos
                .as_ref()
                .is_some_and(|b| b.stage != Stage::Checking)
        });
        assert_eq!(
            v.baseos.unwrap().stage,
            Stage::Available(SimUpdates::BASEOS.into())
        );
        u.download(Part::Unslot);
        settle(&u, |v| matches!(v.unslot.stage, Stage::Ready(_)));
        assert!(
            d.path()
                .join(crate::pak::file_name(SimUpdates::UNSLOT))
                .exists(),
            "the pack is not on the card"
        );
        u.download(Part::BaseOs);
        settle(&u, |v| {
            v.baseos
                .as_ref()
                .is_some_and(|b| matches!(b.stage, Stage::Ready(_)))
        });
        assert!(d.path().join("baseos-rg35xxsp-1.5.0.bosupd").exists());
    }

    /// The pretend releases, held to unslot's own key, which did not sign them.
    struct Unsigned(SimUpdates);

    impl UpdateBackend for Unsigned {
        fn get(&mut self, url: &str) -> Result<String, String> {
            self.0.get(url)
        }

        fn fetch(
            &mut self,
            url: &str,
            sha256: &str,
            dest: &Path,
            progress: &mut dyn FnMut(u64),
        ) -> Result<(), String> {
            self.0.fetch(url, sha256, dest, progress)
        }
    }

    #[test]
    fn a_pack_unslot_did_not_sign_is_not_kept() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("System")).unwrap();
        std::fs::write(
            d.path().join(VERSION_FILE),
            "v2.0.0
",
        )
        .unwrap();
        let u = Updates::spawn(Box::new(Unsigned(SimUpdates)), d.path(), None);
        u.check();
        settle(&u, |v| matches!(v.unslot.stage, Stage::Available(_)));
        u.download(Part::Unslot);
        let v = settle(&u, |v| v.error.is_some());
        assert!(
            matches!(v.unslot.stage, Stage::Available(_)),
            "{:?}",
            v.unslot.stage
        );
        assert!(
            v.error
                .as_deref()
                .is_some_and(|e| e.contains("not signed by unslot")),
            "{:?}",
            v.error
        );
        assert!(!d
            .path()
            .join(crate::pak::file_name(SimUpdates::UNSLOT))
            .exists());
    }

    #[test]
    fn the_latest_unslot_is_up_to_date() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("System")).unwrap();
        std::fs::write(d.path().join(VERSION_FILE), SimUpdates::UNSLOT).unwrap();
        let u = Updates::spawn(Box::new(SimUpdates), d.path(), None);
        u.check();
        let v = settle(&u, |v| {
            v.unslot.stage != Stage::Checking && v.unslot.stage != Stage::Unchecked
        });
        assert_eq!(v.unslot.stage, Stage::UpToDate);
        assert_eq!(v.baseos, None, "a BaseOS row with no BaseOS");
    }
}
