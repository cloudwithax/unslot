//! Network services: BaseOS's SSH server, sharing the card over SMB, Tailscale and a WireGuard
//! tunnel.
//!
//! The app says what it wants and a thread makes it so, a whole set at a time, so the newest
//! wish wins over any it has not got to yet. On the device `card/System/slotservices.sh` starts
//! and stops the programs; off it a pretend set stands in, so the pages can be driven anywhere.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::json;
use crate::wireguard::{self, Tunnel};

/// How often the services are looked in on: whether BaseOS's late start brought SSH back, and
/// how far a Tailscale sign-in has got.
const POLL: Duration = Duration::from_secs(5);

/// What the app wants running, from what it keeps in slot.state.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Wanted {
    pub ssh: bool,
    pub samba: bool,
    pub tailscale: bool,
    pub vpn: Option<String>,
    pub password: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Tailnet {
    #[default]
    Off,
    Starting,
    /// Waiting for someone to sign in at the URL.
    SignIn(String),
    /// Signed in, waiting for the tailnet's admin to let the device on.
    Approval,
    /// On the tailnet, at its IPv4 address if it has one yet.
    Running(Option<String>),
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ServicesView {
    pub tailnet: Tailnet,
    /// The tunnel that is up.
    pub vpn: Option<String>,
    /// What is under way, said for the page.
    pub busy: Option<String>,
    pub error: Option<String>,
}

/// What `tailscale status --json` and `up --json` say, the parts slot uses.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TsStatus {
    pub state: String,
    pub auth_url: Option<String>,
    pub ips: Vec<String>,
}

impl TsStatus {
    pub fn tailnet(&self) -> Tailnet {
        match self.state.as_str() {
            "Running" => Tailnet::Running(self.ips.iter().find(|ip| !ip.contains(':')).cloned()),
            "NeedsMachineAuth" => Tailnet::Approval,
            "NeedsLogin" => match &self.auth_url {
                Some(url) => Tailnet::SignIn(url.clone()),
                None => Tailnet::Starting,
            },
            _ => Tailnet::Starting,
        }
    }

    /// Whether the daemon is waiting to be told to go up, which hands back a sign-in URL.
    fn wants_up(&self) -> bool {
        match self.state.as_str() {
            "NeedsLogin" => self.auth_url.is_none(),
            "Stopped" | "NoState" => true,
            _ => false,
        }
    }
}

/// The services, as the worker drives them.
pub trait ServicesBackend: Send {
    fn ssh(&mut self, on: bool) -> Result<(), String>;
    /// Whether an SSH server has started since slot last stopped it, as BaseOS's may at boot.
    fn ssh_started(&mut self) -> bool;
    fn password(&mut self, password: &str) -> Result<(), String>;
    /// Shares the card to root with `password`, or stops sharing it.
    fn samba(&mut self, on: Option<&str>) -> Result<(), String>;
    fn tailscale(&mut self, on: bool) -> Result<(), String>;
    /// A tailscale command against slot's daemon, and what it printed.
    fn tailscale_cli(&mut self, args: &[&str]) -> Result<String, String>;
    fn vpn_up(&mut self, tunnel: &Tunnel) -> Result<(), String>;
    fn vpn_down(&mut self);
    /// Puts the tunnel's DNS back if a DHCP renewal wrote over it.
    fn vpn_keep(&mut self, tunnel: &Tunnel);
}

/// How the worker changes what the app sees.
type SetView<'a> = &'a dyn Fn(&mut dyn FnMut(&mut ServicesView));

/// The handle the app keeps.
pub struct Services {
    wants: Sender<Wanted>,
    view: Arc<Mutex<ServicesView>>,
}

impl Services {
    pub fn spawn(backend: Box<dyn ServicesBackend>, root: &Path) -> Services {
        Services::spawn_polling(backend, root, POLL)
    }

    fn spawn_polling(backend: Box<dyn ServicesBackend>, root: &Path, poll: Duration) -> Services {
        let (wants, rx) = channel();
        let view = Arc::new(Mutex::new(ServicesView::default()));
        let shared = view.clone();
        let root = root.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("slot-services".into())
            .spawn(move || work(backend, root, rx, shared, poll));
        if let Err(e) = spawned {
            eprintln!("slot: services: {e}");
        }
        Services { wants, view }
    }

    /// The services for this build: the real ones on the device, pretend ones anywhere else.
    pub fn for_card(root: &Path) -> Services {
        Services::spawn(backend(root), root)
    }

    pub fn want(&self, wanted: Wanted) {
        let _ = self.wants.send(wanted);
    }

    pub fn view(&self) -> ServicesView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn work(
    mut services: Box<dyn ServicesBackend>,
    root: PathBuf,
    wants: Receiver<Wanted>,
    view: Arc<Mutex<ServicesView>>,
    poll: Duration,
) {
    let set = |f: &mut dyn FnMut(&mut ServicesView)| {
        f(&mut view.lock().unwrap_or_else(|e| e.into_inner()))
    };
    // None until the first wish, which is applied whole: what an earlier slot left running is
    // not known.
    let mut have: Option<Wanted> = None;
    let mut tunnel: Option<Tunnel> = None;
    loop {
        let want = match wants.recv_timeout(poll) {
            Ok(mut want) => {
                while let Ok(newer) = wants.try_recv() {
                    want = newer;
                }
                want
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(have) = &have {
                    look_in(&mut *services, have, tunnel.as_ref(), &set);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        set(&mut |v| v.error = None);
        let mut errors = Vec::new();
        let was = have.clone();
        let changed = |f: fn(&Wanted) -> bool| was.as_ref().is_none_or(|w| f(w) != f(&want));

        if was.as_ref().map(|w| &w.password) != Some(&want.password) {
            if let Some(password) = &want.password {
                if let Err(e) = services.password(password) {
                    errors.push(format!("Password: {e}"));
                }
            }
        }
        if changed(|w| w.ssh) {
            if let Err(e) = services.ssh(want.ssh) {
                errors.push(format!("SSH: {e}"));
            }
        }
        // A new password reaches Samba by starting it again.
        let new_password = was.as_ref().is_some_and(|w| w.password != want.password);
        if changed(|w| w.samba) || (want.samba && new_password) {
            let share = match (want.samba, &want.password) {
                (true, Some(password)) => Some(password.as_str()),
                (true, None) => {
                    errors.push("File sharing needs a password".into());
                    None
                }
                (false, _) => None,
            };
            set(&mut |v| v.busy = share.map(|_| "Starting file sharing...".into()));
            if let Err(e) = services.samba(share) {
                errors.push(format!("File sharing: {e}"));
            }
        }
        if changed(|w| w.tailscale) {
            match want.tailscale {
                true => {
                    set(&mut |v| {
                        v.tailnet = Tailnet::Starting;
                        v.busy = Some("Starting Tailscale...".into());
                    });
                    match services.tailscale(true) {
                        Ok(()) => tailscale_up(&mut *services, &set),
                        Err(e) => {
                            errors.push(format!("Tailscale: {e}"));
                            set(&mut |v| v.tailnet = Tailnet::Off);
                        }
                    }
                }
                false => {
                    let _ = services.tailscale(false);
                    set(&mut |v| v.tailnet = Tailnet::Off);
                }
            }
        }
        if was.as_ref().map(|w| &w.vpn) != Some(&want.vpn) {
            if tunnel.take().is_some() || was.is_none() {
                services.vpn_down();
            }
            set(&mut |v| v.vpn = None);
            if let Some(name) = &want.vpn {
                set(&mut |v| v.busy = Some(format!("Connecting {name}...")));
                match wireguard::load(&root, name).and_then(|t| {
                    services
                        .vpn_up(&t)
                        .map(|()| t)
                        .inspect_err(|_| services.vpn_down())
                }) {
                    Ok(t) => {
                        tunnel = Some(t);
                        set(&mut |v| v.vpn = Some(name.clone()));
                    }
                    Err(e) => errors.push(format!("VPN: {e}")),
                }
            }
        }
        for e in &errors {
            eprintln!("slot: services: {e}");
        }
        set(&mut |v| {
            v.busy = None;
            v.error = errors.first().cloned();
        });
        have = Some(want);
    }
}

/// Tells tailscaled to go up, which comes back with a sign-in URL if it needs one.
fn tailscale_up(services: &mut dyn ServicesBackend, set: SetView) {
    let up = services.tailscale_cli(&[
        "up",
        "--netfilter-mode=off",
        "--accept-dns=false",
        "--json",
        "--timeout=8s",
    ]);
    let status = up
        .ok()
        .map(|text| parse_ts(&text))
        .filter(|s| !s.state.is_empty() || s.auth_url.is_some());
    let status = match status {
        Some(s) if s.auth_url.is_some() => s,
        _ => match services.tailscale_cli(&["status", "--json"]) {
            Ok(text) => parse_ts(&text),
            Err(_) => return,
        },
    };
    set(&mut |v| v.tailnet = status.tailnet());
}

fn look_in(
    services: &mut dyn ServicesBackend,
    have: &Wanted,
    tunnel: Option<&Tunnel>,
    set: SetView,
) {
    if !have.ssh && services.ssh_started() {
        let _ = services.ssh(false);
    }
    if have.tailscale {
        if let Ok(text) = services.tailscale_cli(&["status", "--json"]) {
            let status = parse_ts(&text);
            if status.wants_up() {
                tailscale_up(services, set);
            } else {
                set(&mut |v| v.tailnet = status.tailnet());
            }
        }
    }
    if let Some(tunnel) = tunnel {
        services.vpn_keep(tunnel);
    }
}

/// The fields slot reads from tailscale's JSON. Only the first of each counts, which in
/// `status --json` is the device's own, ahead of its peers'.
pub fn parse_ts(json: &str) -> TsStatus {
    TsStatus {
        state: json::string(json, "BackendState").unwrap_or_default(),
        auth_url: json::string(json, "AuthURL").filter(|u| !u.is_empty()),
        ips: json::strings(json, "TailscaleIPs"),
    }
}

/// The device's name on the network, as avahi announces it.
pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unslot".into())
}

fn backend(root: &Path) -> Box<dyn ServicesBackend> {
    #[cfg(all(feature = "device", unix))]
    {
        Box::new(device::DeviceServices::new(root))
    }
    #[cfg(not(all(feature = "device", unix)))]
    {
        let _ = root;
        Box::new(SimServices::default())
    }
}

/// Pretend services for the desktop and the tests. Tailscale asks for a sign-in, and is signed
/// in once it has been looked at `SIGN_IN_AFTER` times.
#[derive(Default)]
pub struct SimServices {
    pub ssh: bool,
    pub samba: bool,
    pub tailscale: bool,
    looks: u32,
    pub vpn: bool,
}

impl SimServices {
    pub const SIGN_IN_AFTER: u32 = 2;
    pub const URL: &'static str = "https://login.tailscale.com/a/0123456789ab";
}

impl ServicesBackend for SimServices {
    fn ssh(&mut self, on: bool) -> Result<(), String> {
        self.ssh = on;
        Ok(())
    }

    fn ssh_started(&mut self) -> bool {
        false
    }

    fn password(&mut self, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn samba(&mut self, on: Option<&str>) -> Result<(), String> {
        self.samba = on.is_some();
        Ok(())
    }

    fn tailscale(&mut self, on: bool) -> Result<(), String> {
        self.tailscale = on;
        Ok(())
    }

    fn tailscale_cli(&mut self, args: &[&str]) -> Result<String, String> {
        if !self.tailscale {
            return Err("tailscaled is not running".into());
        }
        if args.first() == Some(&"status") {
            self.looks += 1;
        }
        Ok(match self.looks > SimServices::SIGN_IN_AFTER {
            true => r#"{"BackendState": "Running", "TailscaleIPs": ["100.101.102.103", "fd7a:115c:a1e0::1"]}"#.into(),
            false => format!(r#"{{"BackendState": "NeedsLogin", "AuthURL": "{}"}}"#, SimServices::URL),
        })
    }

    fn vpn_up(&mut self, _: &Tunnel) -> Result<(), String> {
        self.vpn = true;
        Ok(())
    }

    fn vpn_down(&mut self) {
        self.vpn = false;
    }

    fn vpn_keep(&mut self, _: &Tunnel) {}
}

#[cfg(all(feature = "device", unix))]
mod device {
    use std::io::Write;
    use std::net::{IpAddr, SocketAddr};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use super::*;

    const DROPBEAR_PID: &str = "/run/dropbear.pid";
    const WG_SOCK: &str = "/var/run/wireguard/wg0.sock";
    const RESOLV: &str = "/etc/resolv.conf";
    /// What resolv.conf held before the tunnel's DNS went in, to put back when it goes.
    const RESOLV_KEPT: &str = "/run/slotvpn-resolv.conf";

    pub struct DeviceServices {
        root: PathBuf,
        /// The endpoints routed around the tunnel, to take out again when it goes.
        endpoint_routes: Vec<IpAddr>,
    }

    impl DeviceServices {
        pub fn new(root: &Path) -> DeviceServices {
            DeviceServices {
                root: root.to_path_buf(),
                endpoint_routes: Vec::new(),
            }
        }

        fn script(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
            let (ok, out, err) = self.run(args, stdin)?;
            match ok {
                true => Ok(out),
                false => Err(err),
            }
        }

        /// Runs the script: whether it succeeded, what it printed, and the last line of what it
        /// complained of.
        fn run(
            &self,
            args: &[&str],
            stdin: Option<&str>,
        ) -> Result<(bool, String, String), String> {
            let mut child = Command::new("/bin/sh")
                .arg(self.root.join("System/slotservices.sh"))
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?;
            if let Some(mut pipe) = child.stdin.take() {
                if let Some(text) = stdin {
                    let _ = writeln!(pipe, "{text}");
                }
            }
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            let err = String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .map(str::to_string)
                .unwrap_or_else(|| format!("{} failed", args.join(" ")));
            Ok((
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).into_owned(),
                err,
            ))
        }

        fn ip(args: &[String]) -> Result<String, String> {
            let out = Command::new("ip")
                .args(args)
                .output()
                .map_err(|e| e.to_string())?;
            match out.status.success() {
                true => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
                false => Err(format!(
                    "ip {}: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&out.stderr).trim()
                )),
            }
        }

        fn set_dns(tunnel: &Tunnel) {
            let Some(ours) = wireguard::resolv_conf(tunnel) else {
                return;
            };
            let now = std::fs::read_to_string(RESOLV).unwrap_or_default();
            if now == ours {
                return;
            }
            let _ = std::fs::write(RESOLV_KEPT, now);
            let _ = std::fs::write(RESOLV, ours);
        }
    }

    impl ServicesBackend for DeviceServices {
        fn ssh(&mut self, on: bool) -> Result<(), String> {
            self.script(&["ssh", if on { "on" } else { "off" }], None)
                .map(drop)
        }

        fn ssh_started(&mut self) -> bool {
            Path::new(DROPBEAR_PID).exists()
        }

        fn password(&mut self, password: &str) -> Result<(), String> {
            self.script(&["password"], Some(password)).map(drop)
        }

        fn samba(&mut self, on: Option<&str>) -> Result<(), String> {
            match on {
                Some(password) => {
                    let root = self.root.to_string_lossy().into_owned();
                    self.script(&["samba", "on", &root], Some(password))
                }
                None => self.script(&["samba", "off"], None),
            }
            .map(drop)
        }

        fn tailscale(&mut self, on: bool) -> Result<(), String> {
            self.script(&["tailscale", if on { "on" } else { "off" }], None)
                .map(drop)
        }

        fn tailscale_cli(&mut self, args: &[&str]) -> Result<String, String> {
            let args: Vec<&str> = ["tailscale", "cli"].iter().chain(args).copied().collect();
            let (ok, out, err) = self.run(&args, None)?;
            // `up` stops waiting for a sign-in with a failure, having printed the URL to use.
            match ok || (args.get(2) == Some(&"up") && !out.trim().is_empty()) {
                true => Ok(out),
                false => Err(err),
            }
        }

        fn vpn_up(&mut self, tunnel: &Tunnel) -> Result<(), String> {
            let endpoints: Vec<Option<SocketAddr>> = tunnel
                .peers
                .iter()
                .map(|p| p.endpoint.as_deref().map(wireguard::resolve).transpose())
                .collect::<Result<_, _>>()?;
            self.script(&["vpn", "up"], None)?;
            wireguard::uapi(Path::new(WG_SOCK), &wireguard::uapi_set(tunnel, &endpoints))?;
            let mut ways = Vec::new();
            for addr in endpoints.iter().flatten() {
                let ip = addr.ip().to_string();
                let got = Self::ip(&["route".into(), "get".into(), ip])?;
                if let Some((via, dev)) = wireguard::parse_route_get(&got) {
                    ways.push((addr.ip(), via, dev));
                }
            }
            for command in wireguard::ip_plan(tunnel, &ways) {
                Self::ip(&command)?;
            }
            self.endpoint_routes = ways.into_iter().map(|(ip, ..)| ip).collect();
            Self::set_dns(tunnel);
            Ok(())
        }

        fn vpn_down(&mut self) {
            let _ = self.script(&["vpn", "down"], None);
            for ip in self.endpoint_routes.drain(..) {
                let _ = Self::ip(&["route".into(), "del".into(), ip.to_string()]);
            }
            if let Ok(kept) = std::fs::read_to_string(RESOLV_KEPT) {
                let _ = std::fs::write(RESOLV, kept);
                let _ = std::fs::remove_file(RESOLV_KEPT);
            }
        }

        fn vpn_keep(&mut self, tunnel: &Tunnel) {
            Self::set_dns(tunnel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn tailscale_up_json_reads_into_a_sign_in() {
        let s = parse_ts(
            "{\n  \"AuthURL\": \"https://login.tailscale.com/a/1a2b3c\",\n  \"BackendState\": \"NeedsLogin\"\n}",
        );
        assert_eq!(
            s.tailnet(),
            Tailnet::SignIn("https://login.tailscale.com/a/1a2b3c".into())
        );
        assert!(!s.wants_up());
    }

    #[test]
    fn tailscale_status_gives_the_devices_own_ipv4() {
        let s = parse_ts(
            r#"{"Version":"1.104.1","BackendState":"Running","AuthURL":"","TailscaleIPs":["100.64.0.7","fd7a:115c:a1e0::7"],"Self":{"TailscaleIPs":["100.64.0.7"]},"Peer":{"k":{"TailscaleIPs":["100.64.0.9"]}}}"#,
        );
        assert_eq!(s.auth_url, None, "an empty AuthURL is no URL");
        assert_eq!(s.ips, ["100.64.0.7", "fd7a:115c:a1e0::7"]);
        assert_eq!(s.tailnet(), Tailnet::Running(Some("100.64.0.7".into())));
    }

    #[test]
    fn a_stopped_or_url_less_daemon_is_told_to_go_up() {
        assert!(parse_ts(r#"{"BackendState":"Stopped"}"#).wants_up());
        assert!(parse_ts(r#"{"BackendState":"NeedsLogin","AuthURL":""}"#).wants_up());
        assert!(!parse_ts(r#"{"BackendState":"Running"}"#).wants_up());
    }

    const FAST: Duration = Duration::from_millis(20);

    fn settle(services: &Services, until: impl Fn(&ServicesView) -> bool) -> ServicesView {
        let start = Instant::now();
        loop {
            let view = services.view();
            if until(&view) || start.elapsed() > Duration::from_secs(20) {
                return view;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn tailscale_asks_for_a_sign_in_then_runs() {
        let d = tempfile::tempdir().unwrap();
        let s = Services::spawn_polling(Box::new(SimServices::default()), d.path(), FAST);
        s.want(Wanted {
            tailscale: true,
            ..Wanted::default()
        });
        let v = settle(&s, |v| matches!(v.tailnet, Tailnet::SignIn(_)));
        assert_eq!(v.tailnet, Tailnet::SignIn(SimServices::URL.into()));
        let v = settle(&s, |v| matches!(v.tailnet, Tailnet::Running(_)));
        assert_eq!(v.tailnet, Tailnet::Running(Some("100.101.102.103".into())));
        s.want(Wanted::default());
        assert_eq!(
            settle(&s, |v| v.tailnet == Tailnet::Off).tailnet,
            Tailnet::Off
        );
    }

    #[test]
    fn file_sharing_without_a_password_says_so() {
        let d = tempfile::tempdir().unwrap();
        let s = Services::spawn_polling(Box::new(SimServices::default()), d.path(), FAST);
        s.want(Wanted {
            samba: true,
            ..Wanted::default()
        });
        let v = settle(&s, |v| v.error.is_some());
        assert_eq!(v.error.as_deref(), Some("File sharing needs a password"));
    }

    #[test]
    fn a_tunnel_on_the_card_comes_up_and_a_broken_one_says_why() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join(wireguard::WG_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("home.conf"),
            "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.0.0.2/32\n[Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nAllowedIPs = 10.0.0.0/24\n",
        )
        .unwrap();
        std::fs::write(dir.join("broken.conf"), "[Interface]\n").unwrap();
        let s = Services::spawn_polling(Box::new(SimServices::default()), d.path(), FAST);
        s.want(Wanted {
            vpn: Some("home".into()),
            ..Wanted::default()
        });
        assert_eq!(settle(&s, |v| v.vpn.is_some()).vpn.as_deref(), Some("home"));
        s.want(Wanted {
            vpn: Some("broken".into()),
            ..Wanted::default()
        });
        let v = settle(&s, |v| v.error.is_some());
        assert_eq!(v.vpn, None);
        assert_eq!(v.error.as_deref(), Some("VPN: broken.conf: no PrivateKey"));
    }
}
