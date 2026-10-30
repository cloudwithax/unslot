//! Wi-Fi Direct, for Miracast: a supplicant of its own on wlan1, beside the Wi-Fi on wlan0, that
//! `card/System/slotmiracast.sh` starts. It finds the TVs that take a Wi-Fi Display, and joins
//! the group of the one picked, the TV leading it and giving the handheld its address.

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::mdns::{Addr, Receiver};
use crate::wifi::ctrl::Ctrl;

const CTRL: &str = "/run/wpa_p2p/wlan1";
/// The handheld as a Wi-Fi Display source, free for a session, its RTSP on 7236, 50 Mbps at
/// most.
const WFD_SOURCE: &str = "000600101c440032";
/// How long the TV has to be found again before joining it, and to let the handheld in once
/// asked, which some ask their owner about first.
const FIND_AGAIN: Duration = Duration::from_secs(15);
const JOIN_WAIT: Duration = Duration::from_secs(60);

/// The TV's group, joined.
pub struct Group {
    pub iface: String,
    /// The handheld's address in it.
    pub ip: Ipv4Addr,
}

/// The Wi-Fi Direct supplicant, taken down again when dropped.
pub struct Radio {
    root: PathBuf,
    ctrl: Ctrl,
    events: Ctrl,
}

fn script(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("/bin/sh")
        .arg(root.join("System/slotmiracast.sh"))
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl Radio {
    pub fn up(root: &Path) -> Result<Radio, String> {
        if script(root, &["up"]).is_none() {
            return Err("Wi-Fi Direct would not start".into());
        }
        let open = || Ctrl::open(Path::new(CTRL)).map_err(|e| format!("Wi-Fi Direct: {e}"));
        let radio = Radio {
            root: root.to_path_buf(),
            ctrl: open()?,
            events: open()?,
        };
        radio.events.request("ATTACH").map_err(|e| e.to_string())?;
        radio.ok("SET wifi_display 1")?;
        radio.ok(&format!("WFD_SUBELEM_SET 0 {WFD_SOURCE}"))?;
        Ok(radio)
    }

    fn ask(&self, cmd: &str) -> Result<String, String> {
        self.ctrl.request(cmd).map_err(|e| format!("{cmd}: {e}"))
    }

    fn ok(&self, cmd: &str) -> Result<(), String> {
        match self.ask(cmd)?.trim() {
            "OK" => Ok(()),
            other => Err(format!("{}: {other}", cmd.split(' ').next().unwrap_or(cmd))),
        }
    }

    /// The next event within `wait`, its level taken off.
    fn event(&self, wait: Duration) -> Option<String> {
        let e = self.events.event(wait)?;
        Some(e.split_once('>').map_or(e.as_str(), |(_, e)| e).to_string())
    }

    /// The TVs that answer within `wait`, by name.
    pub fn sinks(&self, wait: Duration) -> Result<Vec<Receiver>, String> {
        self.ok("P2P_FIND")?;
        std::thread::sleep(wait);
        let _ = self.ok("P2P_STOP_FIND");
        // The peers one after another, each answer starting with its address, FAIL after the
        // last; P2P_PEERS is wpa_cli's own, not the supplicant's.
        let mut found = Vec::new();
        let mut peer = self.ask("P2P_PEER FIRST")?;
        while let Some(mac) = peer
            .lines()
            .next()
            .map(str::trim)
            .filter(|m| m.contains(':'))
        {
            let mac = mac.to_string();
            found.extend(parse_peer(&mac, &peer));
            peer = self.ask(&format!("P2P_PEER NEXT-{mac}"))?;
        }
        found.sort_by_key(|r| r.name.to_lowercase());
        Ok(found)
    }

    /// Joins the group of the TV at `mac`, a client in it, and asks the TV for an address.
    pub fn join(&self, mac: &str) -> Result<Group, String> {
        // This supplicant has to have seen the TV before it can ask it anything.
        self.ok("P2P_FIND")?;
        let end = Instant::now() + FIND_AGAIN;
        let seen = loop {
            if self
                .ask(&format!("P2P_PEER {mac}"))
                .is_ok_and(|p| !p.starts_with("FAIL"))
            {
                break true;
            }
            if Instant::now() >= end {
                break false;
            }
            self.event(Duration::from_millis(250));
        };
        let _ = self.ok("P2P_STOP_FIND");
        if !seen {
            return Err("the TV is gone".into());
        }
        self.ok(&format!("P2P_CONNECT {mac} pbc go_intent=0"))?;
        let end = Instant::now() + JOIN_WAIT;
        let iface = loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                let _ = self.ok("P2P_CANCEL");
                return Err("the TV did not let the handheld in".into());
            }
            let Some(e) = self.event(left.min(Duration::from_secs(1))) else {
                continue;
            };
            if let Some((iface, leads)) = group_started(&e) {
                if leads {
                    let _ = self.ok(&format!("P2P_GROUP_REMOVE {iface}"));
                    return Err("the TV wanted the handheld to lead".into());
                }
                break iface;
            }
            if join_failed(&e) {
                eprintln!("slot: cast: wi-fi direct: {e}");
                return Err("the TV did not let the handheld in".into());
            }
        };
        let Some(ip) = script(&self.root, &["lease", &iface]).and_then(|ip| ip.parse().ok()) else {
            let _ = self.ok(&format!("P2P_GROUP_REMOVE {iface}"));
            return Err("the TV gave no address".into());
        };
        Ok(Group { iface, ip })
    }

    /// Whether the group has gone since last asked: the TV left it or was turned off.
    pub fn gone(&self) -> bool {
        std::iter::from_fn(|| self.event(Duration::from_millis(1)))
            .any(|e| e.starts_with("P2P-GROUP-REMOVED"))
    }

    pub fn leave(&self, group: &Group) {
        let _ = self.ok(&format!("P2P_GROUP_REMOVE {}", group.iface));
    }
}

impl Drop for Radio {
    fn drop(&mut self) {
        script(&self.root, &["down"]);
    }
}

/// A peer from `P2P_PEER`'s answer, if it is a TV free to take a Wi-Fi Display.
fn parse_peer(mac: &str, text: &str) -> Option<Receiver> {
    let field = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(k)?.strip_prefix('='))
            .map(str::trim)
    };
    if !takes_display(field("wfd_subelems")?) {
        return None;
    }
    let name = field("device_name")
        .filter(|n| !n.is_empty())
        .unwrap_or(mac);
    Some(Receiver {
        name: name.to_string(),
        model: "Miracast".into(),
        addr: Addr::Miracast(mac.to_string()),
    })
}

/// Whether a peer's Wi-Fi Display subelements, in hex, say it shows a picture and is free to.
fn takes_display(hex: &str) -> bool {
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect();
    let mut rest = &bytes[..];
    while rest.len() >= 3 {
        let (id, len) = (rest[0], usize::from(u16::from_be_bytes([rest[1], rest[2]])));
        let Some(body) = rest.get(3..3 + len) else {
            return false;
        };
        if id == 0 && len >= 2 {
            let info = u16::from_be_bytes([body[0], body[1]]);
            // A primary sink or one that is both, with a session available.
            return matches!(info & 3, 1 | 3) && (info >> 4) & 3 == 1;
        }
        rest = &rest[3 + len..];
    }
    false
}

/// From `P2P-GROUP-STARTED`: the group's interface, and whether the handheld leads it.
fn group_started(event: &str) -> Option<(String, bool)> {
    let mut f = event.strip_prefix("P2P-GROUP-STARTED ")?.split(' ');
    let iface = f.next()?.to_string();
    Some((iface, f.next()? == "GO"))
}

fn join_failed(event: &str) -> bool {
    [
        "P2P-GO-NEG-FAILURE",
        "P2P-GROUP-FORMATION-FAILURE",
        "WPS-FAIL",
        "WPS-TIMEOUT",
    ]
    .iter()
    .any(|f| event.starts_with(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tvs_free_to_show_the_screen_are_found_and_the_rest_left_out() {
        let lg = "d6:9d:c0:a2:b0:a8\npri_dev_type=7-0050F204-1\n\
                  device_name=[LG] webOS TV UN6955ZUF\nconfig_methods=0x1188\n\
                  wfd_subelems=00000601131c440032\n";
        let r = parse_peer("d6:9d:c0:a2:b0:a8", lg).unwrap();
        assert_eq!(r.name, "[LG] webOS TV UN6955ZUF");
        assert_eq!(r.addr, Addr::Miracast("d6:9d:c0:a2:b0:a8".into()));
        let printer = "device_name=DIRECT-35-HP ENVY\nconfig_methods=0x0\n";
        assert_eq!(parse_peer("a6:ce:da:45:aa:d6", printer), None);
        // A source, and a sink busy with another.
        assert!(!takes_display("00000600101c440032"));
        assert!(!takes_display("00000600011c440032"));
        assert!(takes_display("00000601111c440036"));
        assert!(!takes_display("000006"));
    }

    #[test]
    fn the_group_says_who_leads_it() {
        let client = "P2P-GROUP-STARTED wlan1 client ssid=\"DIRECT-xy\" freq=5745 \
                      psk=00 go_dev_addr=d6:9d:c0:a2:b0:a8";
        assert_eq!(group_started(client), Some(("wlan1".into(), false)));
        let go = "P2P-GROUP-STARTED p2p-wlan1-0 GO ssid=\"DIRECT-ab\" freq=2412";
        assert_eq!(group_started(go), Some(("p2p-wlan1-0".into(), true)));
        assert_eq!(group_started("P2P-DEVICE-FOUND d6:9d:c0:a2:b0:a8"), None);
        assert!(join_failed("P2P-GO-NEG-FAILURE status=1"));
        assert!(!join_failed("P2P-GO-NEG-SUCCESS role=client"));
    }
}
