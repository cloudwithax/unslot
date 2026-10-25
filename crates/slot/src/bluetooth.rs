//! Bluetooth: pairing controllers and headphones, and keeping them connected.
//!
//! On the device everything goes through `card/System/slotbt.sh`, which drives BaseOS's
//! bluetoothctl and bluealsa. Off the device a pretend radio stands in, as with the Wi-Fi.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a search for new devices runs.
pub const SEARCH_S: u32 = 10;

/// How often the paired devices are looked over while the radio is on.
const POLL: Duration = Duration::from_secs(5);
/// How often while the devices page is open, so it shows what changed straight away.
const WATCH_POLL: Duration = Duration::from_secs(1);

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Controller,
    Audio,
    Other,
}

impl Kind {
    /// What bluez's icon name says the device is.
    pub fn of_icon(icon: &str) -> Kind {
        match icon {
            "input-gaming" | "input-joystick" => Kind::Controller,
            i if i.starts_with("audio") => Kind::Audio,
            _ => Kind::Other,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Device {
    pub mac: String,
    pub name: String,
    pub kind: Kind,
    pub paired: bool,
    pub connected: bool,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum BtBusy {
    #[default]
    Idle,
    Starting,
    Searching,
    Pairing(String),
    Connecting(String),
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BtView {
    pub on: bool,
    pub busy: BtBusy,
    pub devices: Vec<Device>,
    pub error: Option<String>,
}

impl BtView {
    /// The headphones sound should go to, if any are connected.
    pub fn audio(&self) -> Option<&Device> {
        self.devices
            .iter()
            .find(|d| d.connected && d.kind == Kind::Audio)
    }
}

/// How pairing or connecting a device went.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Link {
    Up,
    Failed,
    /// A Bluetooth LE controller that linked up but never bonded.
    NeedsLe,
}

pub trait BtBackend: Send {
    fn up(&mut self) -> Result<(), String>;
    fn down(&mut self);
    fn search(&mut self, secs: u32);
    fn devices(&mut self) -> Vec<Device>;
    /// Pairs, trusts and connects.
    fn pair(&mut self, mac: &str) -> Link;
    fn connect(&mut self, mac: &str) -> Link;
    fn disconnect(&mut self, mac: &str);
    fn forget(&mut self, mac: &str);
}

enum Job {
    On,
    Off,
    Search,
    Pair(String),
    Connect(String),
    Disconnect(String),
    Forget(String),
}

pub struct Bluetooth {
    jobs: Sender<Job>,
    view: Arc<Mutex<BtView>>,
    watching: Arc<AtomicBool>,
}

impl Bluetooth {
    pub fn spawn(backend: Box<dyn BtBackend>) -> Bluetooth {
        let (jobs, rx) = channel();
        let view = Arc::new(Mutex::new(BtView::default()));
        let watching = Arc::new(AtomicBool::new(false));
        let (shared, watched) = (view.clone(), watching.clone());
        let spawned = std::thread::Builder::new()
            .name("slot-bluetooth".into())
            .spawn(move || work(backend, rx, shared, watched));
        if let Err(e) = spawned {
            eprintln!("slot: bluetooth: {e}");
        }
        Bluetooth {
            jobs,
            view,
            watching,
        }
    }

    pub fn for_card(root: &Path) -> Bluetooth {
        Bluetooth::spawn(backend(root))
    }

    pub fn view(&self) -> BtView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Whether the devices are on screen, and so looked over every second.
    pub fn watch(&self, on: bool) {
        self.watching.store(on, Ordering::Relaxed);
    }

    pub fn turn_on(&self) {
        let _ = self.jobs.send(Job::On);
    }

    pub fn turn_off(&self) {
        let _ = self.jobs.send(Job::Off);
    }

    pub fn search(&self) {
        let _ = self.jobs.send(Job::Search);
    }

    pub fn pair(&self, mac: &str) {
        let _ = self.jobs.send(Job::Pair(mac.to_string()));
    }

    pub fn connect(&self, mac: &str) {
        let _ = self.jobs.send(Job::Connect(mac.to_string()));
    }

    pub fn disconnect(&self, mac: &str) {
        let _ = self.jobs.send(Job::Disconnect(mac.to_string()));
    }

    pub fn forget(&self, mac: &str) {
        let _ = self.jobs.send(Job::Forget(mac.to_string()));
    }
}

/// Whether the device has a Bluetooth radio at all; BaseOS says, and the RG28XX has none.
pub fn present() -> bool {
    present_by(crate::baseos::var("BASEOS_BLUETOOTH").as_deref())
}

/// What BaseOS's `BASEOS_BLUETOOTH` says. Not saying means a model that has one.
pub fn present_by(said: Option<&str>) -> bool {
    !matches!(said.map(str::trim), Some("0" | "no" | "false"))
}

fn work(
    mut radio: Box<dyn BtBackend>,
    jobs: Receiver<Job>,
    view: Arc<Mutex<BtView>>,
    watching: Arc<AtomicBool>,
) {
    let set = |f: &dyn Fn(&mut BtView)| f(&mut view.lock().unwrap_or_else(|e| e.into_inner()));
    let name_of = |mac: &str| {
        view.lock()
            .unwrap_or_else(|e| e.into_inner())
            .devices
            .iter()
            .find(|d| d.mac == mac)
            .map_or_else(|| mac.to_string(), |d| d.name.clone())
    };
    loop {
        let on = view.lock().unwrap_or_else(|e| e.into_inner()).on;
        let poll = match watching.load(Ordering::Relaxed) {
            true => WATCH_POLL,
            false => POLL,
        };
        let job = match jobs.recv_timeout(poll) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) if on => {
                let devices = radio.devices();
                set(&|v| v.devices = sorted(devices.clone()));
                continue;
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        set(&|v| v.error = None);
        match job {
            Job::On => {
                set(&|v| v.busy = BtBusy::Starting);
                if let Err(e) = radio.up() {
                    eprintln!("slot: bluetooth: on: {e}");
                    set(&|v| {
                        v.busy = BtBusy::Idle;
                        v.error = Some(e.clone());
                    });
                    continue;
                }
                set(&|v| v.on = true);
                // Headphones do not come back by themselves, so ask the paired ones.
                for d in radio.devices() {
                    if d.paired && !d.connected && d.kind == Kind::Audio {
                        set(&|v| v.busy = BtBusy::Connecting(d.name.clone()));
                        radio.connect(&d.mac);
                    }
                }
                let devices = radio.devices();
                set(&|v| {
                    v.busy = BtBusy::Idle;
                    v.devices = sorted(devices.clone());
                });
            }
            Job::Off => {
                radio.down();
                set(&|v| *v = BtView::default());
            }
            Job::Search if on => {
                set(&|v| v.busy = BtBusy::Searching);
                radio.search(SEARCH_S);
                let devices = radio.devices();
                set(&|v| {
                    v.busy = BtBusy::Idle;
                    v.devices = sorted(devices.clone());
                });
            }
            Job::Pair(mac) | Job::Connect(mac) if on => {
                let name = name_of(&mac);
                let pairing = !view
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .devices
                    .iter()
                    .any(|d| d.mac == mac && d.paired);
                set(&|v| {
                    v.busy = match pairing {
                        true => BtBusy::Pairing(name.clone()),
                        false => BtBusy::Connecting(name.clone()),
                    }
                });
                let link = match pairing {
                    true => radio.pair(&mac),
                    false => radio.connect(&mac),
                };
                let devices = radio.devices();
                set(&|v| {
                    v.busy = BtBusy::Idle;
                    v.devices = sorted(devices.clone());
                    v.error = match link {
                        Link::Up => None,
                        Link::Failed => Some(format!("Could not connect to {name}")),
                        Link::NeedsLe => Some(format!(
                            "{name} would not pair. Turn Bluetooth off and on, then try again"
                        )),
                    };
                });
            }
            Job::Disconnect(mac) if on => {
                radio.disconnect(&mac);
                let devices = radio.devices();
                set(&|v| v.devices = sorted(devices.clone()));
            }
            Job::Forget(mac) if on => {
                radio.forget(&mac);
                let devices = radio.devices();
                set(&|v| v.devices = sorted(devices.clone()));
            }
            Job::Search | Job::Pair(_) | Job::Connect(_) | Job::Disconnect(_) | Job::Forget(_) => {}
        }
    }
}

/// Paired first, then the rest, each by name. A device connecting or dropping keeps its
/// place, so the one under the cursor stays the one A acts on.
fn sorted(mut devices: Vec<Device>) -> Vec<Device> {
    devices.sort_by(|a, b| {
        (!a.paired, a.name.to_lowercase(), &a.mac).cmp(&(!b.paired, b.name.to_lowercase(), &b.mac))
    });
    devices
}

/// The devices in `slotbt.sh devices`'s output: `DEV mac|paired|connected|icon|name` a line.
/// A device bluez knows only by its address is left out until it has a name.
pub fn parse_devices(text: &str) -> Vec<Device> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.strip_prefix("DEV ")?.splitn(5, '|');
            let (mac, paired, connected, icon) = (f.next()?, f.next()?, f.next()?, f.next()?);
            let name = f.next().unwrap_or_default().trim();
            let unnamed = name.is_empty() || name.replace('-', ":").eq_ignore_ascii_case(mac);
            (!unnamed).then(|| Device {
                mac: mac.to_string(),
                name: name.to_string(),
                kind: Kind::of_icon(icon.trim()),
                paired: paired.trim() == "yes",
                connected: connected.trim() == "yes",
            })
        })
        .collect()
}

/// The ALSA device that plays through `mac`'s headphones.
pub fn alsa_device(mac: &str) -> String {
    format!("bluealsa:DEV={mac},PROFILE=a2dp")
}

fn backend(root: &Path) -> Box<dyn BtBackend> {
    #[cfg(feature = "device")]
    {
        Box::new(DeviceBt {
            root: root.to_path_buf(),
        })
    }
    #[cfg(not(feature = "device"))]
    {
        let _ = root;
        Box::new(SimBt::default())
    }
}

/// The radio on the device, through `slotbt.sh`.
pub struct DeviceBt {
    root: PathBuf,
}

impl DeviceBt {
    fn script(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new("/bin/sh")
            .arg(self.root.join("System/slotbt.sh"))
            .args(args)
            .output()
            .unwrap_or_else(|e| {
                eprintln!("slot: bluetooth: {e}");
                std::process::Output {
                    status: std::process::ExitStatus::default(),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }
            })
    }

    fn ran(&self, args: &[&str]) -> bool {
        self.script(args).status.success()
    }

    fn linked(&self, args: &[&str]) -> Link {
        match self.script(args).status.code() {
            Some(0) => Link::Up,
            Some(3) => Link::NeedsLe,
            _ => Link::Failed,
        }
    }
}

impl BtBackend for DeviceBt {
    fn up(&mut self) -> Result<(), String> {
        match self.ran(&["up"]) {
            true => Ok(()),
            false => Err("Bluetooth would not start".into()),
        }
    }

    fn down(&mut self) {
        self.ran(&["down"]);
    }

    fn search(&mut self, secs: u32) {
        self.ran(&["scan", &secs.to_string()]);
    }

    fn devices(&mut self) -> Vec<Device> {
        parse_devices(&String::from_utf8_lossy(&self.script(&["devices"]).stdout))
    }

    fn pair(&mut self, mac: &str) -> Link {
        self.linked(&["pair", mac])
    }

    fn connect(&mut self, mac: &str) -> Link {
        self.linked(&["connect", mac])
    }

    fn disconnect(&mut self, mac: &str) {
        self.ran(&["disconnect", mac]);
    }

    fn forget(&mut self, mac: &str) {
        self.ran(&["forget", mac]);
    }
}

/// A pretend radio with a controller and some headphones nearby. The speaker named "Stubborn"
/// never pairs, and the "Xbox Wireless Controller" links up over Bluetooth LE but never bonds.
#[derive(Default)]
pub struct SimBt {
    up: bool,
    searched: bool,
    paired: Vec<String>,
    connected: Vec<String>,
}

impl SimBt {
    fn nearby() -> [(&'static str, &'static str, Kind); 4] {
        [
            ("AA:00:00:00:00:01", "8BitDo Pro 2", Kind::Controller),
            ("AA:00:00:00:00:02", "WH-1000XM4", Kind::Audio),
            ("AA:00:00:00:00:03", "Stubborn", Kind::Audio),
            ("AA:00:00:00:00:04", "Xbox Wireless Controller", Kind::Controller),
        ]
    }
}

impl BtBackend for SimBt {
    fn up(&mut self) -> Result<(), String> {
        self.up = true;
        Ok(())
    }

    fn down(&mut self) {
        self.up = false;
        self.connected.clear();
    }

    fn search(&mut self, _secs: u32) {
        self.searched = true;
    }

    fn devices(&mut self) -> Vec<Device> {
        SimBt::nearby()
            .into_iter()
            .filter(|(mac, ..)| self.searched || self.paired.iter().any(|p| p == mac))
            .map(|(mac, name, kind)| Device {
                mac: mac.into(),
                name: name.into(),
                kind,
                paired: self.paired.iter().any(|p| p == mac),
                connected: self.connected.iter().any(|c| c == mac),
            })
            .collect()
    }

    fn pair(&mut self, mac: &str) -> Link {
        match SimBt::nearby().iter().find(|(m, ..)| *m == mac) {
            Some((_, "Stubborn", _)) => return Link::Failed,
            Some((_, "Xbox Wireless Controller", _)) => return Link::NeedsLe,
            _ => {}
        }
        if !self.paired.iter().any(|p| p == mac) {
            self.paired.push(mac.into());
        }
        self.connect(mac)
    }

    fn connect(&mut self, mac: &str) -> Link {
        if !self.paired.iter().any(|p| p == mac) {
            return Link::Failed;
        }
        if !self.connected.iter().any(|c| c == mac) {
            self.connected.push(mac.into());
        }
        Link::Up
    }

    fn disconnect(&mut self, mac: &str) {
        self.connected.retain(|c| c != mac);
    }

    fn forget(&mut self, mac: &str) {
        self.disconnect(mac);
        self.paired.retain(|p| p != mac);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_are_read_from_the_scripts_lines_and_unnamed_ones_left_out() {
        let text = "DEV AA:BB:CC:DD:EE:01|yes|yes|input-gaming|8BitDo Pro 2\n\
                    DEV AA:BB:CC:DD:EE:02|no|no|audio-headset|WH-1000XM4\n\
                    DEV AA:BB:CC:DD:EE:03|no|no||AA-BB-CC-DD-EE-03\n\
                    DEV AA:BB:CC:DD:EE:04|no|no||\n\
                    noise\n";
        let found = parse_devices(text);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].kind, Kind::Controller);
        assert!(found[0].paired && found[0].connected);
        assert_eq!(found[1].kind, Kind::Audio);
        assert!(!found[1].paired);
    }

    #[test]
    fn paired_devices_come_first_and_connecting_one_does_not_move_it() {
        let d = |name: &str, paired, connected| Device {
            mac: name.into(),
            name: name.into(),
            kind: Kind::Other,
            paired,
            connected,
        };
        let order = |devices| -> Vec<String> {
            sorted(devices).into_iter().map(|d: Device| d.name).collect()
        };
        assert_eq!(
            order(vec![
                d("zed", false, false),
                d("bee", true, false),
                d("ant", false, false),
                d("cat", true, true),
            ]),
            ["bee", "cat", "ant", "zed"]
        );
        assert_eq!(
            order(vec![d("bee", true, true), d("cat", true, false)]),
            order(vec![d("bee", true, false), d("cat", true, true)]),
        );
    }

    #[test]
    fn headphones_play_through_bluealsas_a2dp_device() {
        assert_eq!(
            alsa_device("AA:BB:CC:DD:EE:02"),
            "bluealsa:DEV=AA:BB:CC:DD:EE:02,PROFILE=a2dp"
        );
    }

    fn settled(bt: &Bluetooth, done: impl Fn(&BtView) -> bool) -> BtView {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let view = bt.view();
            if done(&view) || std::time::Instant::now() > deadline {
                return view;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_worker_searches_pairs_and_routes_sound_to_headphones() {
        let bt = Bluetooth::spawn(Box::new(SimBt::default()));
        bt.turn_on();
        settled(&bt, |v| v.on && v.busy == BtBusy::Idle);
        bt.search();
        let view = settled(&bt, |v| v.devices.len() == 4);
        assert!(view.audio().is_none());

        bt.pair("AA:00:00:00:00:02");
        let view = settled(&bt, |v| v.audio().is_some());
        assert_eq!(view.audio().map(|d| d.name.as_str()), Some("WH-1000XM4"));
        assert_eq!(view.devices[0].name, "WH-1000XM4", "the paired one is not first");

        bt.pair("AA:00:00:00:00:03");
        let view = settled(&bt, |v| v.error.is_some());
        assert_eq!(view.error.as_deref(), Some("Could not connect to Stubborn"));

        bt.pair("AA:00:00:00:00:04");
        let view = settled(&bt, |v| v.error.as_deref().is_some_and(|e| e.contains("Xbox")));
        assert_eq!(
            view.error.as_deref(),
            Some(
                "Xbox Wireless Controller would not pair. Turn Bluetooth off and on, then try again"
            )
        );
        assert!(!view.devices.iter().any(|d| d.name.starts_with("Xbox") && d.connected));

        bt.forget("AA:00:00:00:00:02");
        let view = settled(&bt, |v| v.audio().is_none());
        assert!(!view.devices.iter().any(|d| d.paired));
    }

    #[test]
    fn baseos_saying_there_is_no_radio_hides_bluetooth() {
        assert!(present_by(None), "a model that says nothing has no radio");
        assert!(present_by(Some("1")));
        assert!(!present_by(Some("0")));
        assert!(!present_by(Some(" no ")));
    }
}
