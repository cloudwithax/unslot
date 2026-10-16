//! WireGuard through wireguard-go, which runs in userspace on a kernel too old for the module.
//!
//! A tunnel is a wg-quick `.conf` in `Config/wireguard/` on the card. Slot reads it, hands the
//! keys and peers to wireguard-go over its UAPI socket, and sets the address, routes and DNS the
//! way wg-quick would. Everything here but the socket is plain data, so it is tested off device.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::Path;

pub const WG_DIR: &str = "Config/wireguard";
pub const IFACE: &str = "wg0";

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Peer {
    pub public_key: [u8; 32],
    pub preshared_key: Option<[u8; 32]>,
    /// `host:port` as written; resolved when the tunnel comes up.
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    pub keepalive: Option<u16>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Tunnel {
    pub private_key: [u8; 32],
    pub addresses: Vec<String>,
    pub dns: Vec<String>,
    pub mtu: u32,
    pub listen_port: Option<u16>,
    pub peers: Vec<Peer>,
}

/// The tunnels on the card: the names of the `.conf` files in `Config/wireguard`, sorted.
pub fn tunnels(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join(WG_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("conf"))
        })
        .filter(|p| !slot_store::is_hidden(p))
        .filter_map(|p| Some(p.file_stem()?.to_str()?.to_string()))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

pub fn load(root: &Path, name: &str) -> Result<Tunnel, String> {
    let path = root.join(WG_DIR).join(format!("{name}.conf"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{name}.conf: {e}"))?;
    parse(&text).map_err(|e| format!("{name}.conf: {e}"))
}

/// Reads a wg-quick config. What only wg-quick's scripts use (PostUp and the like) is left out.
pub fn parse(text: &str) -> Result<Tunnel, String> {
    enum Section {
        None,
        Interface,
        Peer,
    }
    let mut section = Section::None;
    let mut private_key = None;
    let (mut addresses, mut dns) = (Vec::new(), Vec::new());
    let (mut mtu, mut listen_port) = (None, None);
    let mut peers: Vec<Peer> = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("[Interface]") {
            section = Section::Interface;
            continue;
        }
        if line.eq_ignore_ascii_case("[Peer]") {
            section = Section::Peer;
            peers.push(Peer {
                public_key: [0; 32],
                preshared_key: None,
                endpoint: None,
                allowed_ips: Vec::new(),
                keepalive: None,
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("cannot read {line:?}"));
        };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        let list = || -> Vec<String> {
            value
                .split(',')
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect()
        };
        match (&section, key.as_str()) {
            (Section::Interface, "privatekey") => private_key = Some(key32(value)?),
            (Section::Interface, "address") => addresses.extend(list()),
            (Section::Interface, "dns") => dns.extend(list()),
            (Section::Interface, "mtu") => {
                mtu = Some(value.parse().map_err(|_| format!("MTU {value:?}"))?)
            }
            (Section::Interface, "listenport") => {
                listen_port = Some(value.parse().map_err(|_| format!("ListenPort {value:?}"))?)
            }
            (Section::Peer, _) => {
                let peer = peers.last_mut().expect("a [Peer] line pushed one");
                match key.as_str() {
                    "publickey" => peer.public_key = key32(value)?,
                    "presharedkey" => peer.preshared_key = Some(key32(value)?),
                    "endpoint" => peer.endpoint = Some(value.to_string()),
                    "allowedips" => peer.allowed_ips.extend(list()),
                    "persistentkeepalive" => {
                        peer.keepalive =
                            Some(value.parse().map_err(|_| format!("keepalive {value:?}"))?)
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let private_key = private_key.ok_or("no PrivateKey")?;
    if addresses.is_empty() {
        return Err("no Address".into());
    }
    if peers.is_empty() {
        return Err("no [Peer]".into());
    }
    if peers.iter().any(|p| p.public_key == [0; 32]) {
        return Err("a [Peer] with no PublicKey".into());
    }
    Ok(Tunnel {
        private_key,
        addresses,
        dns,
        mtu: mtu.unwrap_or(1420),
        listen_port,
        peers,
    })
}

/// A 32-byte key from the base64 the config holds it in.
fn key32(text: &str) -> Result<[u8; 32], String> {
    let bytes = base64(text).ok_or_else(|| format!("{text:?} is not base64"))?;
    bytes
        .try_into()
        .map_err(|_| format!("{text:?} is not a 32-byte key"))
}

/// Standard base64, padded or not.
pub fn base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let digits: Vec<u8> = text.trim().trim_end_matches('=').bytes().collect();
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let mut acc = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            acc |= value(*c)? << (18 - 6 * i);
        }
        let take = match chunk.len() {
            4 => 3,
            3 => 2,
            2 => 1,
            _ => return None,
        };
        out.extend_from_slice(&acc.to_be_bytes()[1..1 + take]);
    }
    Some(out)
}

fn hex(key: &[u8; 32]) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

/// The endpoint's address, looked up now, since wireguard-go takes only an IP.
pub fn resolve(endpoint: &str) -> Result<SocketAddr, String> {
    endpoint
        .to_socket_addrs()
        .map_err(|e| format!("{endpoint}: {e}"))?
        .next()
        .ok_or_else(|| format!("{endpoint} has no address"))
}

/// The UAPI `set` that configures the interface, peers' endpoints already resolved.
pub fn uapi_set(tunnel: &Tunnel, endpoints: &[Option<SocketAddr>]) -> String {
    let mut out = String::from("set=1\n");
    out.push_str(&format!("private_key={}\n", hex(&tunnel.private_key)));
    if let Some(port) = tunnel.listen_port {
        out.push_str(&format!("listen_port={port}\n"));
    }
    out.push_str("replace_peers=true\n");
    for (peer, endpoint) in tunnel.peers.iter().zip(endpoints) {
        out.push_str(&format!("public_key={}\n", hex(&peer.public_key)));
        if let Some(psk) = &peer.preshared_key {
            out.push_str(&format!("preshared_key={}\n", hex(psk)));
        }
        if let Some(addr) = endpoint {
            out.push_str(&format!("endpoint={addr}\n"));
        }
        if let Some(seconds) = peer.keepalive {
            out.push_str(&format!("persistent_keepalive_interval={seconds}\n"));
        }
        out.push_str("replace_allowed_ips=true\n");
        for ip in &peer.allowed_ips {
            out.push_str(&format!("allowed_ip={ip}\n"));
        }
    }
    out.push('\n');
    out
}

/// Whether a route covers everything, so the tunnel takes the default route.
fn is_default(cidr: &str) -> bool {
    matches!(cidr, "0.0.0.0/0" | "::/0")
}

/// The `ip` commands that put the tunnel's addresses and routes in place, given how the
/// endpoints are reached now (`via` gateway and `dev` for each, from `ip route get`).
///
/// A full tunnel does not replace the default route, which would also take the endpoint's own
/// traffic into the tunnel. It keeps a host route to each endpoint through the old gateway and
/// covers everything else with two halves that outrank the default, as wg-quick's fallback does.
pub fn ip_plan(
    tunnel: &Tunnel,
    endpoints: &[(IpAddr, Option<String>, String)],
) -> Vec<Vec<String>> {
    let s = |v: &str| v.to_string();
    let mut plan = Vec::new();
    for addr in &tunnel.addresses {
        let family = if addr.contains(':') { "-6" } else { "-4" };
        plan.push(vec![
            s(family),
            s("address"),
            s("add"),
            addr.clone(),
            s("dev"),
            s(IFACE),
        ]);
    }
    plan.push(vec![
        s("link"),
        s("set"),
        s("mtu"),
        tunnel.mtu.to_string(),
        s("up"),
        s("dev"),
        s(IFACE),
    ]);
    let all: Vec<&String> = tunnel.peers.iter().flat_map(|p| &p.allowed_ips).collect();
    let full = all.iter().any(|c| is_default(c));
    if full {
        for (ip, via, dev) in endpoints {
            let mut route = vec![s("route"), s("add"), ip.to_string()];
            if let Some(via) = via {
                route.extend([s("via"), via.clone()]);
            }
            route.extend([s("dev"), dev.clone()]);
            plan.push(route);
        }
    }
    for cidr in all {
        let halves: Vec<&str> = match cidr.as_str() {
            "0.0.0.0/0" => vec!["0.0.0.0/1", "128.0.0.0/1"],
            "::/0" => vec!["::/1", "8000::/1"],
            other => vec![other],
        };
        for half in halves {
            let family = if half.contains(':') { "-6" } else { "-4" };
            plan.push(vec![
                s(family),
                s("route"),
                s("add"),
                s(half),
                s("dev"),
                s(IFACE),
            ]);
        }
    }
    plan
}

/// Where `ip route get` says traffic to an address goes: the gateway, if any, and the device.
pub fn parse_route_get(text: &str) -> Option<(Option<String>, String)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let after = |w: &str| {
        words
            .iter()
            .position(|x| *x == w)
            .and_then(|i| words.get(i + 1))
            .map(|v| v.to_string())
    };
    Some((after("via"), after("dev")?))
}

/// The resolv.conf a tunnel with DNS servers wants.
pub fn resolv_conf(tunnel: &Tunnel) -> Option<String> {
    let servers: Vec<&String> = tunnel
        .dns
        .iter()
        .filter(|d| d.parse::<IpAddr>().is_ok())
        .collect();
    if servers.is_empty() {
        return None;
    }
    Some(
        servers
            .into_iter()
            .map(|s| format!("nameserver {s}\n"))
            .collect(),
    )
}

#[cfg(unix)]
pub fn uapi(socket: &Path, request: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket)
        .map_err(|e| format!("{}: {e}", socket.display()))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(4)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reply = Vec::new();
    let mut buf = [0u8; 1024];
    while !reply.ends_with(b"\n\n") {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => reply.extend_from_slice(&buf[..n]),
            Err(e) => return Err(e.to_string()),
        }
    }
    let reply = String::from_utf8_lossy(&reply).into_owned();
    match reply.lines().find_map(|l| l.strip_prefix("errno=")) {
        Some("0") | None => Ok(reply),
        Some(errno) => Err(format!("wireguard refused the config: errno {errno}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Keys from the WireGuard documentation's example, not anyone's real ones.
    const CONF: &str = "\
[Interface]
PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
Address = 10.192.122.3/32, fd00::3/128
DNS = 10.192.122.1, example.lan
# a comment
PostUp = echo hi

[Peer]
PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
PresharedKey = /UwcSPg38hW/D9Y3tcS1FOV0K1wuURMbS0sesJEP5ak=
Endpoint = 192.95.5.67:1234
AllowedIPs = 0.0.0.0/0, ::/0
PersistentKeepalive = 25
";

    #[test]
    fn base64_decodes_with_or_without_padding() {
        assert_eq!(base64("aGk=").unwrap(), b"hi");
        assert_eq!(base64("aGk").unwrap(), b"hi");
        assert_eq!(base64("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert!(base64("!!!!").is_none());
    }

    #[test]
    fn a_wg_quick_config_reads_into_a_tunnel() {
        let t = parse(CONF).unwrap();
        assert_eq!(t.addresses, ["10.192.122.3/32", "fd00::3/128"]);
        assert_eq!(t.dns, ["10.192.122.1", "example.lan"]);
        assert_eq!(t.mtu, 1420, "the default MTU was not filled in");
        assert_eq!(t.peers.len(), 1);
        assert_eq!(t.peers[0].keepalive, Some(25));
        assert_eq!(t.peers[0].allowed_ips, ["0.0.0.0/0", "::/0"]);
        assert_eq!(
            hex(&t.private_key),
            "c809f3e5317e9575c9b5ed78b638b7ce530dabe85ddab614220241801ddf0669"
        );
    }

    #[test]
    fn a_config_missing_what_a_tunnel_needs_says_what() {
        assert_eq!(
            parse("[Interface]\nAddress = 10.0.0.2/32\n").unwrap_err(),
            "no PrivateKey"
        );
        let no_peer = "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.0.0.2/32\n";
        assert_eq!(parse(no_peer).unwrap_err(), "no [Peer]");
        assert!(parse("[Interface]\nPrivateKey = short\n").is_err());
    }

    #[test]
    fn the_uapi_set_carries_hex_keys_and_the_resolved_endpoint() {
        let t = parse(CONF).unwrap();
        let addr: SocketAddr = "192.95.5.67:1234".parse().unwrap();
        let set = uapi_set(&t, &[Some(addr)]);
        assert!(set.starts_with("set=1\nprivate_key=c809f3e5"), "{set}");
        assert!(set.contains("\npublic_key=c5320103"), "{set}");
        assert!(set.contains("\nendpoint=192.95.5.67:1234\n"));
        assert!(set.contains("\npersistent_keepalive_interval=25\n"));
        assert!(set.contains("\nallowed_ip=0.0.0.0/0\nallowed_ip=::/0\n"));
        assert!(
            set.ends_with("\n\n"),
            "the request was not ended with a blank line"
        );
    }

    #[test]
    fn a_full_tunnel_keeps_its_endpoint_off_the_tunnel_and_splits_the_default() {
        let t = parse(CONF).unwrap();
        let plan = ip_plan(
            &t,
            &[(
                "192.95.5.67".parse().unwrap(),
                Some("192.168.1.1".into()),
                "wlan0".into(),
            )],
        );
        let lines: Vec<String> = plan.iter().map(|c| c.join(" ")).collect();
        assert_eq!(
            lines,
            [
                "-4 address add 10.192.122.3/32 dev wg0",
                "-6 address add fd00::3/128 dev wg0",
                "link set mtu 1420 up dev wg0",
                "route add 192.95.5.67 via 192.168.1.1 dev wlan0",
                "-4 route add 0.0.0.0/1 dev wg0",
                "-4 route add 128.0.0.0/1 dev wg0",
                "-6 route add ::/1 dev wg0",
                "-6 route add 8000::/1 dev wg0",
            ]
        );
    }

    #[test]
    fn a_split_tunnel_routes_only_its_networks() {
        let t = parse(&CONF.replace("0.0.0.0/0, ::/0", "10.192.122.0/24")).unwrap();
        let lines: Vec<String> = ip_plan(&t, &[]).iter().map(|c| c.join(" ")).collect();
        assert!(lines.contains(&"-4 route add 10.192.122.0/24 dev wg0".to_string()));
        assert!(!lines.iter().any(|l| l.contains("via")), "{lines:?}");
    }

    #[test]
    fn route_get_names_the_gateway_and_the_device() {
        assert_eq!(
            parse_route_get("192.95.5.67 via 192.168.1.1 dev wlan0 src 192.168.1.42 uid 0"),
            Some((Some("192.168.1.1".into()), "wlan0".into()))
        );
        assert_eq!(
            parse_route_get("192.168.1.9 dev wlan0 src 192.168.1.42"),
            Some((None, "wlan0".into()))
        );
    }

    #[test]
    fn only_addresses_go_in_resolv_conf() {
        let t = parse(CONF).unwrap();
        assert_eq!(resolv_conf(&t).unwrap(), "nameserver 10.192.122.1\n");
    }

    #[test]
    fn the_cards_tunnels_are_its_conf_files() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join(WG_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["home.conf", "Office.conf", "notes.txt", ".hidden.conf"] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        assert_eq!(tunnels(d.path()), ["home", "Office"]);
    }
}
