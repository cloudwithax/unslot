//! Wi-Fi: finding networks, joining them and remembering them.
//!
//! On the device `card/System/slotwifi.sh` starts and stops wpa_supplicant, and everything else
//! goes straight to the supplicant's control socket, so a password never passes through a shell.
//! Saved networks live in `Config/wifi.conf` on the card, in wpa_supplicant's own format, where
//! they can be read or written by hand. Off the device a pretend radio stands in, so the pages
//! that use this can be driven and tested anywhere.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const WIFI_CONF: &str = "Config/wifi.conf";

/// How often a connected radio is asked how it is doing.
const POLL: Duration = Duration::from_secs(5);

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Security {
    Open,
    /// WPA or WPA2 personal, or WPA3's SAE: a passphrase.
    Personal,
    /// WPA enterprise or WEP, which slot cannot join.
    Unsupported,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Network {
    pub ssid: String,
    /// The strongest signal heard for it, in dBm.
    pub signal: i32,
    pub security: Security,
    pub saved: bool,
}

impl Network {
    /// Signal as bars: three at -60 dBm or better, two at -70, one below, the way NextUI counts.
    pub fn bars(&self) -> u8 {
        match self.signal {
            s if s >= -60 => 3,
            s if s >= -70 => 2,
            _ => 1,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Busy {
    #[default]
    Idle,
    Starting,
    Scanning,
    Joining(String),
}

/// Everything the pages show of the radio, as last seen.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct WifiView {
    pub on: bool,
    pub busy: Busy,
    pub networks: Vec<Network>,
    pub connected: Option<String>,
    pub ip: Option<String>,
    /// What went wrong last, until the next thing is asked for.
    pub error: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum JoinError {
    WrongPassword,
    Failed(String),
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JoinError::WrongPassword => write!(f, "Wrong password"),
            JoinError::Failed(why) => write!(f, "{why}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Status {
    pub state: String,
    pub ssid: Option<String>,
    pub ip: Option<String>,
}

impl Status {
    pub fn connected(&self) -> bool {
        self.state == "COMPLETED"
    }
}

/// The radio, as the worker drives it.
pub trait WifiBackend: Send {
    fn up(&mut self) -> Result<(), String>;
    fn down(&mut self);
    fn scan(&mut self) -> Result<Vec<Network>, String>;
    /// Joins `ssid`, with `psk` for a secured one or None for an open or already saved one, and
    /// saves it once it is in.
    fn join(&mut self, ssid: &str, psk: Option<&str>) -> Result<(), JoinError>;
    fn forget(&mut self, ssid: &str) -> Result<(), String>;
    fn status(&mut self) -> Status;
}

enum Job {
    On,
    Off,
    Scan,
    Join(String, Option<String>),
    Forget(String),
}

/// The handle the app keeps: jobs go to a thread that owns the radio, and what it last saw comes
/// back through `view`.
pub struct Wifi {
    jobs: Sender<Job>,
    view: Arc<Mutex<WifiView>>,
}

impl Wifi {
    pub fn spawn(backend: Box<dyn WifiBackend>) -> Wifi {
        let (jobs, rx) = channel();
        let view = Arc::new(Mutex::new(WifiView::default()));
        let shared = view.clone();
        let spawned = std::thread::Builder::new()
            .name("slot-wifi".into())
            .spawn(move || work(backend, rx, shared));
        if let Err(e) = spawned {
            eprintln!("slot: wifi: {e}");
        }
        Wifi { jobs, view }
    }

    /// The radio for this build: the real one on the device, a pretend one anywhere else.
    pub fn for_card(root: &Path) -> Wifi {
        Wifi::spawn(backend(root))
    }

    pub fn view(&self) -> WifiView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn turn_on(&self) {
        let _ = self.jobs.send(Job::On);
    }

    pub fn turn_off(&self) {
        let _ = self.jobs.send(Job::Off);
    }

    pub fn scan(&self) {
        let _ = self.jobs.send(Job::Scan);
    }

    pub fn join(&self, ssid: &str, psk: Option<&str>) {
        let _ = self
            .jobs
            .send(Job::Join(ssid.to_string(), psk.map(str::to_string)));
    }

    pub fn forget(&self, ssid: &str) {
        let _ = self.jobs.send(Job::Forget(ssid.to_string()));
    }
}

fn work(mut radio: Box<dyn WifiBackend>, jobs: Receiver<Job>, view: Arc<Mutex<WifiView>>) {
    let set = |f: &dyn Fn(&mut WifiView)| f(&mut view.lock().unwrap_or_else(|e| e.into_inner()));
    loop {
        let on = view.lock().unwrap_or_else(|e| e.into_inner()).on;
        let job = match jobs.recv_timeout(POLL) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) if on => {
                let status = radio.status();
                set(&|v| take_status(v, &status));
                continue;
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        set(&|v| v.error = None);
        match job {
            Job::On => {
                set(&|v| v.busy = Busy::Starting);
                match radio.up() {
                    Ok(()) => {
                        set(&|v| {
                            v.on = true;
                            v.busy = Busy::Scanning;
                        });
                        let found = radio.scan();
                        let status = radio.status();
                        set(&|v| {
                            v.busy = Busy::Idle;
                            take_scan(v, &found);
                            take_status(v, &status);
                        });
                    }
                    Err(e) => {
                        eprintln!("slot: wifi: on: {e}");
                        set(&|v| {
                            v.busy = Busy::Idle;
                            v.error = Some(e.clone());
                        });
                    }
                }
            }
            Job::Off => {
                radio.down();
                set(&|v| *v = WifiView::default());
            }
            Job::Scan if on => {
                set(&|v| v.busy = Busy::Scanning);
                let found = radio.scan();
                set(&|v| {
                    v.busy = Busy::Idle;
                    take_scan(v, &found);
                });
            }
            Job::Join(ssid, psk) if on => {
                set(&|v| v.busy = Busy::Joining(ssid.clone()));
                let joined = radio.join(&ssid, psk.as_deref());
                if let Err(e) = &joined {
                    eprintln!("slot: wifi: join {ssid:?}: {e}");
                }
                let status = radio.status();
                let found = radio.scan();
                set(&|v| {
                    v.busy = Busy::Idle;
                    take_scan(v, &found);
                    take_status(v, &status);
                    v.error = joined.as_ref().err().map(JoinError::to_string);
                });
            }
            Job::Forget(ssid) if on => {
                if let Err(e) = radio.forget(&ssid) {
                    set(&|v| v.error = Some(e.clone()));
                }
                let found = radio.scan();
                let status = radio.status();
                set(&|v| {
                    take_scan(v, &found);
                    take_status(v, &status);
                });
            }
            Job::Scan | Job::Join(..) | Job::Forget(_) => {}
        }
    }
}

fn take_scan(view: &mut WifiView, found: &Result<Vec<Network>, String>) {
    match found {
        Ok(networks) => {
            view.networks = networks.clone();
            view.networks
                .sort_by(|a, b| b.signal.cmp(&a.signal).then_with(|| a.ssid.cmp(&b.ssid)));
        }
        Err(e) => view.error = Some(e.clone()),
    }
}

fn take_status(view: &mut WifiView, status: &Status) {
    if status.connected() {
        view.connected = status.ssid.clone();
        view.ip = status.ip.clone();
    } else {
        view.connected = None;
        view.ip = None;
    }
}

/// The networks in a `SCAN_RESULTS` reply, strongest first: one entry a name, the hidden ones
/// left out, and those in `saved` marked.
pub fn parse_scan_results(text: &str, saved: &[String]) -> Vec<Network> {
    let mut networks: Vec<Network> = Vec::new();
    for line in text.lines().skip(1) {
        let mut fields = line.splitn(5, '\t');
        // bssid and frequency, neither of which is any use here
        let _ = (fields.next(), fields.next());
        let level = match fields.next() {
            Some(l) => l,
            None => continue,
        };
        let flags = match fields.next() {
            Some(f) => f,
            None => continue,
        };
        let ssid = unescape(fields.next().unwrap_or_default());
        if ssid.is_empty() || ssid.chars().all(|c| c == '\0') {
            continue;
        }
        let signal = level.trim().parse().unwrap_or(-100);
        match networks.iter_mut().find(|n| n.ssid == ssid) {
            Some(seen) => seen.signal = seen.signal.max(signal),
            None => networks.push(Network {
                saved: saved.contains(&ssid),
                ssid,
                signal,
                security: security(flags),
            }),
        }
    }
    networks.sort_by(|a, b| b.signal.cmp(&a.signal).then_with(|| a.ssid.cmp(&b.ssid)));
    networks
}

/// What a network asks for to join, from its scan flags.
pub fn security(flags: &str) -> Security {
    if flags.contains("EAP") || flags.contains("WEP") {
        return Security::Unsupported;
    }
    if flags.contains("PSK") || flags.contains("SAE") {
        return Security::Personal;
    }
    Security::Open
}

/// The ids and names in a `LIST_NETWORKS` reply.
pub fn parse_list_networks(text: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let mut fields = line.split('\t');
        let id = match fields.next().and_then(|f| f.trim().parse().ok()) {
            Some(id) => id,
            None => continue,
        };
        if let Some(name) = fields.next() {
            out.push((id, unescape(name)));
        }
    }
    out
}

pub fn parse_status(text: &str) -> Status {
    let mut status = Status::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "wpa_state" => status.state = value.to_string(),
            "ssid" => status.ssid = Some(unescape(value)),
            "ip_address" => status.ip = Some(value.to_string()),
            _ => {}
        }
    }
    status
}

/// Undoes wpa_supplicant's escaping of a name: `\xHH`, and a backslash before `\`, `"`, `e`,
/// `n`, `r` and `t`.
pub fn unescape(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let raw = text.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] != b'\\' || i + 1 >= raw.len() {
            bytes.push(raw[i]);
            i += 1;
            continue;
        }
        match raw[i + 1] {
            b'x' if i + 4 <= raw.len() => {
                let hex = std::str::from_utf8(&raw[i + 2..i + 4]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => bytes.push(b),
                    None => bytes.extend_from_slice(&raw[i..i + 4]),
                }
                i += 4;
                continue;
            }
            b'n' => bytes.push(b'\n'),
            b'r' => bytes.push(b'\r'),
            b't' => bytes.push(b'\t'),
            b'e' => bytes.push(0x1b),
            other => bytes.push(other),
        }
        i += 2;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A name as wpa_supplicant takes it with no quoting to get wrong: its bytes in hex.
pub fn ssid_hex(ssid: &str) -> String {
    ssid.bytes().map(|b| format!("{b:02x}")).collect()
}

/// A WPA passphrase as `SET_NETWORK psk` takes it, or why it cannot be one. The supplicant reads
/// a quoted string up to its last quote with no escapes, so any printable character is safe
/// inside the quotes. 64 hex digits are a key already worked out and go in bare.
pub fn psk_value(psk: &str) -> Result<String, String> {
    if psk.len() == 64 && psk.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(psk.to_string());
    }
    if !(8..=63).contains(&psk.len()) {
        return Err("A password is 8 to 63 characters".into());
    }
    if psk.bytes().any(|b| !(0x20..0x7f).contains(&b)) {
        return Err("A password can only use printable characters".into());
    }
    Ok(format!("\"{psk}\""))
}

fn backend(root: &Path) -> Box<dyn WifiBackend> {
    #[cfg(all(feature = "device", unix))]
    {
        Box::new(device::DeviceWifi::new(root))
    }
    #[cfg(not(all(feature = "device", unix)))]
    {
        let _ = root;
        Box::new(SimWifi::default())
    }
}

pub fn conf_path(root: &Path) -> PathBuf {
    root.join(WIFI_CONF)
}

/// A pretend radio with a handful of networks in range, for the desktop and the tests. An open
/// network or the password "password" gets in; anything else is the wrong password.
#[derive(Default)]
pub struct SimWifi {
    up: bool,
    saved: Vec<String>,
    connected: Option<String>,
}

impl SimWifi {
    pub const PASSWORD: &'static str = "password";

    fn in_range() -> Vec<(&'static str, i32, Security)> {
        vec![
            ("Home-5G", -48, Security::Personal),
            ("Cafe Guest", -63, Security::Open),
            ("Neighbour", -77, Security::Personal),
            ("Office-EAP", -71, Security::Unsupported),
        ]
    }
}

impl WifiBackend for SimWifi {
    fn up(&mut self) -> Result<(), String> {
        self.up = true;
        Ok(())
    }

    fn down(&mut self) {
        self.up = false;
        self.connected = None;
    }

    fn scan(&mut self) -> Result<Vec<Network>, String> {
        let mut out = Vec::new();
        for (ssid, signal, security) in SimWifi::in_range() {
            let saved = self.saved.iter().any(|s| s == ssid);
            out.push(Network {
                ssid: ssid.to_string(),
                signal,
                security,
                saved,
            });
        }
        Ok(out)
    }

    fn join(&mut self, ssid: &str, psk: Option<&str>) -> Result<(), JoinError> {
        let security = match SimWifi::in_range().into_iter().find(|(s, ..)| *s == ssid) {
            Some((_, _, security)) => security,
            None => return Err(JoinError::Failed("Not in range".into())),
        };
        let saved = self.saved.iter().any(|s| s == ssid);
        let ok = match security {
            Security::Open => true,
            Security::Personal => saved || psk == Some(SimWifi::PASSWORD),
            Security::Unsupported => false,
        };
        if !ok {
            return Err(JoinError::WrongPassword);
        }
        if !saved {
            self.saved.push(ssid.to_string());
        }
        self.connected = Some(ssid.to_string());
        Ok(())
    }

    fn forget(&mut self, ssid: &str) -> Result<(), String> {
        self.saved.retain(|s| s != ssid);
        if self.connected.as_deref() == Some(ssid) {
            self.connected = None;
        }
        Ok(())
    }

    fn status(&mut self) -> Status {
        match &self.connected {
            Some(ssid) => Status {
                state: "COMPLETED".into(),
                ssid: Some(ssid.clone()),
                ip: Some("192.168.1.42".into()),
            },
            None => Status {
                state: "DISCONNECTED".into(),
                ..Status::default()
            },
        }
    }
}

#[cfg(unix)]
pub mod ctrl {
    //! wpa_supplicant's control socket: a datagram socket in its `ctrl_interface` directory that
    //! answers each request with one reply.

    use std::os::unix::net::UnixDatagram;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    pub struct Ctrl {
        socket: UnixDatagram,
        local: PathBuf,
    }

    impl Ctrl {
        pub fn open(target: &Path) -> std::io::Result<Ctrl> {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let local = std::env::temp_dir().join(format!(
                "slot-wpa-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_file(&local);
            let socket = UnixDatagram::bind(&local)?;
            socket.connect(target)?;
            socket.set_read_timeout(Some(Duration::from_secs(4)))?;
            Ok(Ctrl { socket, local })
        }

        pub fn request(&self, cmd: &str) -> std::io::Result<String> {
            self.socket.send(cmd.as_bytes())?;
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                let n = self.socket.recv(&mut buf)?;
                let reply = String::from_utf8_lossy(&buf[..n]).into_owned();
                // An attached socket also gets events, each starting "<level>"; they are not
                // the reply.
                if !reply.starts_with('<') {
                    return Ok(reply);
                }
            }
        }

        /// The next event on an attached socket, or None if none came in `wait`.
        pub fn event(&self, wait: Duration) -> Option<String> {
            let _ = self.socket.set_read_timeout(Some(wait));
            let mut buf = vec![0u8; 4096];
            let got = self.socket.recv(&mut buf).ok();
            let _ = self.socket.set_read_timeout(Some(Duration::from_secs(4)));
            got.map(|n| String::from_utf8_lossy(&buf[..n]).into_owned())
        }
    }

    impl Drop for Ctrl {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.local);
        }
    }
}

#[cfg(all(feature = "device", unix))]
mod device {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    use super::ctrl::Ctrl;
    use super::*;

    const CTRL_DIR: &str = "/var/run/wpa_supplicant";
    const JOIN_WAIT: Duration = Duration::from_secs(25);
    const SCAN_WAIT: Duration = Duration::from_millis(2500);

    pub struct DeviceWifi {
        root: PathBuf,
    }

    impl DeviceWifi {
        pub fn new(root: &Path) -> DeviceWifi {
            DeviceWifi {
                root: root.to_path_buf(),
            }
        }

        fn script(&self, args: &[&str]) -> bool {
            Command::new("/bin/sh")
                .arg(self.root.join("System/slotwifi.sh"))
                .args(args)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }

        fn ctrl(&self) -> Result<Ctrl, String> {
            Ctrl::open(&Path::new(CTRL_DIR).join("wlan0"))
                .map_err(|e| format!("The Wi-Fi did not answer: {e}"))
        }

        fn ask(ctrl: &Ctrl, cmd: &str) -> Result<String, String> {
            ctrl.request(cmd).map_err(|e| format!("{cmd}: {e}"))
        }

        fn ok(ctrl: &Ctrl, cmd: &str) -> Result<(), String> {
            match Self::ask(ctrl, cmd)?.trim() {
                "OK" => Ok(()),
                other => Err(format!("{}: {other}", cmd.split(' ').next().unwrap_or(cmd))),
            }
        }

        fn saved(ctrl: &Ctrl) -> Vec<(u32, String)> {
            Self::ask(ctrl, "LIST_NETWORKS")
                .map(|t| parse_list_networks(&t))
                .unwrap_or_default()
        }

        /// Waits for the supplicant to say the join went through or why it did not.
        fn wait_joined(&self, ctrl: &Ctrl, events: Option<&Ctrl>) -> Result<(), JoinError> {
            let began = Instant::now();
            while began.elapsed() < JOIN_WAIT {
                if let Some(event) = events.and_then(|e| e.event(Duration::from_millis(500))) {
                    if event.contains("CTRL-EVENT-SSID-TEMP-DISABLED")
                        && event.contains("WRONG_KEY")
                    {
                        return Err(JoinError::WrongPassword);
                    }
                    if event.contains("CTRL-EVENT-CONNECTED") {
                        return Ok(());
                    }
                } else if events.is_none() {
                    std::thread::sleep(Duration::from_millis(500));
                }
                let status = Self::ask(ctrl, "STATUS").map(|s| parse_status(&s).connected());
                if status.unwrap_or(false) {
                    return Ok(());
                }
            }
            Err(JoinError::Failed("Could not connect".into()))
        }
    }

    impl WifiBackend for DeviceWifi {
        fn up(&mut self) -> Result<(), String> {
            let conf = conf_path(&self.root);
            if self.script(&["up", &conf.to_string_lossy()]) {
                Ok(())
            } else {
                Err("The Wi-Fi would not start".into())
            }
        }

        fn down(&mut self) {
            self.script(&["down"]);
        }

        fn scan(&mut self) -> Result<Vec<Network>, String> {
            let ctrl = self.ctrl()?;
            // FAIL-BUSY means a scan is under way already, often for a join; asking again
            // would only hold that join up, so the results it brings are good enough.
            let _ = Self::ask(&ctrl, "SCAN");
            std::thread::sleep(SCAN_WAIT);
            let saved: Vec<String> = Self::saved(&ctrl).into_iter().map(|(_, s)| s).collect();
            Ok(parse_scan_results(
                &Self::ask(&ctrl, "SCAN_RESULTS")?,
                &saved,
            ))
        }

        fn join(&mut self, ssid: &str, psk: Option<&str>) -> Result<(), JoinError> {
            let fail = JoinError::Failed;
            let ctrl = self.ctrl().map_err(fail)?;
            let events = Ctrl::open(&Path::new(CTRL_DIR).join("wlan0"))
                .ok()
                .filter(|e| e.request("ATTACH").is_ok_and(|r| r.trim() == "OK"));
            let existing = Self::saved(&ctrl)
                .into_iter()
                .find(|(_, s)| s == ssid)
                .map(|(id, _)| id);
            let (id, fresh) = match (existing, psk) {
                (Some(id), None) => (id, false),
                (existing, _) => {
                    if let Some(old) = existing {
                        let _ = Self::ok(&ctrl, &format!("REMOVE_NETWORK {old}"));
                    }
                    let id: u32 = Self::ask(&ctrl, "ADD_NETWORK")
                        .map_err(fail)?
                        .trim()
                        .parse()
                        .map_err(|_| fail("The Wi-Fi would not take a network".into()))?;
                    let set = |field: &str, value: &str| {
                        Self::ok(&ctrl, &format!("SET_NETWORK {id} {field} {value}"))
                    };
                    set("ssid", &ssid_hex(ssid)).map_err(fail)?;
                    match psk {
                        Some(psk) => set("psk", &psk_value(psk).map_err(fail)?).map_err(fail)?,
                        None => set("key_mgmt", "NONE").map_err(fail)?,
                    }
                    // Finds the network even when it does not announce its name.
                    set("scan_ssid", "1").map_err(fail)?;
                    (id, true)
                }
            };
            Self::ok(&ctrl, &format!("SELECT_NETWORK {id}")).map_err(fail)?;
            let joined = self.wait_joined(&ctrl, events.as_ref());
            if joined.is_err() && fresh {
                let _ = Self::ok(&ctrl, &format!("REMOVE_NETWORK {id}"));
            }
            // Selecting one network disables the rest; turn them back on before saving, or
            // they are saved disabled and never joined again.
            let _ = Self::ok(&ctrl, "ENABLE_NETWORK all");
            let _ = Self::ok(&ctrl, "SAVE_CONFIG");
            joined
        }

        fn forget(&mut self, ssid: &str) -> Result<(), String> {
            let ctrl = self.ctrl()?;
            for (id, _) in Self::saved(&ctrl).into_iter().filter(|(_, s)| s == ssid) {
                Self::ok(&ctrl, &format!("REMOVE_NETWORK {id}"))?;
            }
            Self::ok(&ctrl, "SAVE_CONFIG")
        }

        fn status(&mut self) -> Status {
            self.ctrl()
                .and_then(|c| Self::ask(&c, "STATUS"))
                .map(|s| parse_status(&s))
                .unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESULTS: &str = "bssid / frequency / signal level / flags / ssid\n\
        aa:bb:cc:00:00:01\t5180\t-52\t[WPA2-PSK-CCMP][ESS]\tHome-5G\n\
        aa:bb:cc:00:00:02\t2412\t-61\t[WPA2-PSK-CCMP][ESS]\tHome-5G\n\
        aa:bb:cc:00:00:03\t2437\t-70\t[ESS]\tCafe Guest\n\
        aa:bb:cc:00:00:04\t2462\t-80\t[WPA2-EAP-CCMP][ESS]\tOffice\n\
        aa:bb:cc:00:00:05\t2462\t-45\t[WPA2-PSK-CCMP][ESS]\t\n\
        aa:bb:cc:00:00:06\t5745\t-66\t[SAE][ESS]\tCaf\\xc3\\xa9 \\\"Nine\\\"\n";

    #[test]
    fn a_scan_is_one_entry_a_name_strongest_first_with_the_hidden_left_out() {
        let found = parse_scan_results(RESULTS, &["Cafe Guest".to_string()]);
        let names: Vec<&str> = found.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["Home-5G", "Café \"Nine\"", "Cafe Guest", "Office"]);
        assert_eq!(
            found[0].signal, -52,
            "the strongest access point did not win"
        );
        assert!(found[2].saved && !found[0].saved);
        assert_eq!(
            found[1].security,
            Security::Personal,
            "WPA3 is a passphrase too"
        );
        assert_eq!(found[2].security, Security::Open);
        assert_eq!(found[3].security, Security::Unsupported);
    }

    #[test]
    fn signal_reads_as_three_bars_down_to_one() {
        let at = |signal| Network {
            ssid: String::new(),
            signal,
            security: Security::Open,
            saved: false,
        };
        assert_eq!(at(-45).bars(), 3);
        assert_eq!(at(-65).bars(), 2);
        assert_eq!(at(-85).bars(), 1);
    }

    #[test]
    fn status_reads_the_state_name_and_address() {
        let s = parse_status(
            "bssid=aa:bb\nfreq=5180\nssid=Home-5G\nwpa_state=COMPLETED\nip_address=192.168.1.9\n",
        );
        assert!(s.connected());
        assert_eq!(s.ssid.as_deref(), Some("Home-5G"));
        assert_eq!(s.ip.as_deref(), Some("192.168.1.9"));
        assert!(!parse_status("wpa_state=SCANNING\n").connected());
    }

    #[test]
    fn saved_networks_list_by_id() {
        let list =
            "network id / ssid / bssid / flags\n0\tHome-5G\tany\t[CURRENT]\n3\tCafe Guest\tany\t\n";
        assert_eq!(
            parse_list_networks(list),
            [(0, "Home-5G".to_string()), (3, "Cafe Guest".to_string())]
        );
    }

    #[test]
    fn a_name_goes_in_as_hex_so_nothing_in_it_needs_quoting() {
        assert_eq!(ssid_hex("a\"b"), "612262");
    }

    #[test]
    fn a_password_is_quoted_whatever_printable_characters_it_has() {
        assert_eq!(psk_value("pa\"ss word\\").unwrap(), "\"pa\"ss word\\\"");
        assert!(
            psk_value("short").is_err(),
            "a 5 letter WPA password was taken"
        );
        assert!(
            psk_value(&"x".repeat(64)).is_err(),
            "64 characters that are not hex"
        );
        assert!(psk_value("line\nbreak").is_err());
        let hex = "0123456789abcdef".repeat(4);
        assert_eq!(psk_value(&hex).unwrap(), hex, "a worked-out key was quoted");
    }

    fn settled(wifi: &Wifi, done: impl Fn(&WifiView) -> bool) -> WifiView {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let view = wifi.view();
            if done(&view) || std::time::Instant::now() > deadline {
                return view;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_worker_turns_on_scans_joins_and_forgets() {
        let wifi = Wifi::spawn(Box::new(SimWifi::default()));
        wifi.turn_on();
        let view = settled(&wifi, |v| v.on && v.busy == Busy::Idle);
        assert_eq!(view.networks.len(), 4, "turning on did not scan");

        wifi.join("Home-5G", Some("nope"));
        let view = settled(&wifi, |v| v.error.is_some());
        assert_eq!(view.error.as_deref(), Some("Wrong password"));
        assert_eq!(view.connected, None);

        wifi.join("Home-5G", Some(SimWifi::PASSWORD));
        let view = settled(&wifi, |v| v.connected.is_some());
        assert_eq!(view.connected.as_deref(), Some("Home-5G"));
        assert!(view.networks.iter().any(|n| n.ssid == "Home-5G" && n.saved));

        wifi.forget("Home-5G");
        let view = settled(&wifi, |v| v.connected.is_none());
        assert!(!view.networks.iter().any(|n| n.saved));

        wifi.turn_off();
        let view = settled(&wifi, |v| !v.on);
        assert_eq!(
            view,
            WifiView::default(),
            "turning off left something shown"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_control_socket_gets_its_reply_past_the_events_in_between() {
        use std::os::unix::net::UnixDatagram;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wlan0");
        let server = UnixDatagram::bind(&path).unwrap();
        let answer = std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            let (n, from) = server.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[..n], b"STATUS");
            let to = from.as_pathname().unwrap();
            server.send_to(b"<3>CTRL-EVENT-SCAN-STARTED ", to).unwrap();
            server
                .send_to(
                    b"wpa_state=COMPLETED
",
                    to,
                )
                .unwrap();
        });
        let ctrl = ctrl::Ctrl::open(&path).unwrap();
        let reply = ctrl.request("STATUS").unwrap();
        answer.join().unwrap();
        assert_eq!(
            reply,
            "wpa_state=COMPLETED
",
            "an event was taken for the reply"
        );
    }

    #[test]
    fn nothing_is_scanned_or_joined_while_the_radio_is_off() {
        let wifi = Wifi::spawn(Box::new(SimWifi::default()));
        wifi.scan();
        wifi.join("Cafe Guest", None);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(wifi.view(), WifiView::default());
    }
}
