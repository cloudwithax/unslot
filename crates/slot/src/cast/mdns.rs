//! DNS-SD discovery of Google Cast screens, listening for both unicast and multicast replies.
//! A receiver's PTR, TXT, SRV and address records can arrive in separate packets.

use std::collections::{BTreeMap, HashMap};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const SERVICE: &str = "_googlecast._tcp.local";
const SERVICE_SUFFIX: &str = "._googlecast._tcp.local";
const MDNS: (Ipv4Addr, u16) = (Ipv4Addr::new(224, 0, 0, 251), 5353);

const PTR: u16 = 12;
const TXT: u16 = 16;
const SRV: u16 = 33;
const A: u16 = 1;

/// Bind alongside Avahi, which also listens on 5353 on BaseOS. A temporary port alone misses
/// receivers that answer the service browse by multicast.
#[cfg(unix)]
fn multicast_socket() -> std::io::Result<UdpSocket> {
    use std::os::fd::{AsRawFd, FromRawFd};
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let socket = UdpSocket::from_raw_fd(fd);
        if libc::fcntl(socket.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let yes: libc::c_int = 1;
        let set = |option| {
            if libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                option,
                &yes as *const _ as *const libc::c_void,
                std::mem::size_of_val(&yes) as _,
            ) < 0
            {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        };
        set(libc::SO_REUSEADDR)?;
        #[cfg(target_vendor = "apple")]
        set(libc::SO_REUSEPORT)?;
        let mut addr: libc::sockaddr_in = std::mem::zeroed();
        addr.sin_family = libc::AF_INET as _;
        addr.sin_port = MDNS.1.to_be();
        if libc::bind(
            socket.as_raw_fd(),
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of_val(&addr) as _,
        ) < 0
        {
            return Err(std::io::Error::last_os_error());
        }
        socket.join_multicast_v4(&MDNS.0, &Ipv4Addr::UNSPECIFIED)?;
        Ok(socket)
    }
}

#[cfg(not(unix))]
fn multicast_socket() -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, MDNS.1))?;
    socket.join_multicast_v4(&MDNS.0, &Ipv4Addr::UNSPECIFIED)?;
    Ok(socket)
}

/// A TV to cast to: a Chromecast, a TV with one built in, or a Miracast TV.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Receiver {
    /// What its owner called it.
    pub name: String,
    pub model: String,
    pub addr: Addr,
}

/// How a receiver is reached.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Addr {
    /// A Chromecast's control channel.
    Cast(SocketAddr),
    /// A Miracast TV's Wi-Fi Direct device address.
    Miracast(String),
}

/// The receivers that answer within `wait`, those that can show a picture, by name.
pub fn search(wait: Duration) -> std::io::Result<Vec<Receiver>> {
    let sock = multicast_socket().or_else(|e| {
        eprintln!("slot: cast: multicast discovery unavailable, using unicast: {e}");
        UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
    })?;
    sock.set_multicast_ttl_v4(255)?;
    let end = Instant::now() + wait;
    let mut asked = HashMap::<(String, u16), Instant>::new();
    let mut browse = Browse::default();
    let mut buf = [0u8; 9000];
    loop {
        let now = Instant::now();
        if now >= end {
            break;
        }
        // Ask for missing records as soon as their instance or host becomes known. Retrying
        // individual questions also covers Wi-Fi packet loss without ten manual searches.
        for query in browse.questions() {
            if asked
                .get(&query)
                .is_none_or(|at| now.duration_since(*at) >= Duration::from_secs(1))
            {
                sock.send_to(&question(&query.0, query.1), MDNS)?;
                asked.insert(query, now);
            }
        }
        let until = end
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(250));
        sock.set_read_timeout(Some(until.max(Duration::from_millis(1))))?;
        match sock.recv_from(&mut buf) {
            Ok((n, _)) => browse.ingest(&buf[..n]),
            Err(e) if timed_out(&e) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(browse.receivers())
}

/// Whether a read gave up for its timeout.
pub(crate) fn timed_out(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// QU requests a unicast answer; the socket also hears multicast replies on 5353.
fn question(service: &str, kind: u16) -> Vec<u8> {
    let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in service.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&kind.to_be_bytes());
    q.extend_from_slice(&0x8001u16.to_be_bytes());
    q
}

/// The name at `at` in `msg`, following compression pointers, and where the record goes on.
fn name(msg: &[u8], mut at: usize) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut end = None;
    for _ in 0..64 {
        let len = *msg.get(at)? as usize;
        if len & 0xc0 == 0xc0 {
            end.get_or_insert(at + 2);
            at = (len & 0x3f) << 8 | *msg.get(at + 1)? as usize;
            continue;
        }
        if len == 0 {
            return Some((out, end.unwrap_or(at + 1)));
        }
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&String::from_utf8_lossy(msg.get(at + 1..at + 1 + len)?));
        at += 1 + len;
    }
    None
}

fn u16_at(msg: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*msg.get(at)?, *msg.get(at + 1)?]))
}

#[derive(Default)]
struct Service {
    instance: String,
    advertised: bool,
    info: Option<(String, String, bool)>,
    target: Option<(String, u16)>,
}

#[derive(Default)]
struct Browse {
    services: BTreeMap<String, Service>,
    hosts: BTreeMap<String, Ipv4Addr>,
}

impl Browse {
    fn service(&mut self, instance: &str) -> Option<&mut Service> {
        let key = instance.to_ascii_lowercase();
        if !key.ends_with(SERVICE_SUFFIX) {
            return None;
        }
        Some(self.services.entry(key).or_insert_with(|| Service {
            instance: instance.into(),
            ..Default::default()
        }))
    }

    fn ingest(&mut self, msg: &[u8]) {
        if u16_at(msg, 2).is_none_or(|flags| flags & 0x8000 == 0) {
            return;
        }
        let Some(records) = records(msg) else { return };
        for r in records {
            if r.class & 0x7fff != 1 {
                continue;
            }
            match r.kind {
                PTR if r.name.eq_ignore_ascii_case(SERVICE) => {
                    if let Some((instance, _)) = name(msg, r.data) {
                        if r.ttl == 0 {
                            self.services.remove(&instance.to_ascii_lowercase());
                        } else if let Some(s) = self.service(&instance) {
                            s.advertised = true;
                        }
                    }
                }
                TXT => {
                    let Some(s) = self.service(&r.name) else {
                        continue;
                    };
                    if r.ttl == 0 {
                        s.info = None;
                        continue;
                    }
                    let txt = text(&msg[r.data..r.data + r.len]);
                    let get = |key: &str| {
                        txt.iter()
                            .find_map(|(k, v)| k.eq_ignore_ascii_case(key).then(|| v.clone()))
                    };
                    // Bit 0 is a screen, bit 5 a speaker group.
                    let ca: u32 = get("ca").and_then(|v| v.parse().ok()).unwrap_or(0);
                    s.info = Some((
                        get("fn").unwrap_or_else(|| s.instance.clone()),
                        get("md").unwrap_or_default(),
                        ca & 1 != 0 && ca & 32 == 0,
                    ));
                }
                SRV if r.len >= 7 => {
                    let Some(s) = self.service(&r.name) else {
                        continue;
                    };
                    s.target = if r.ttl == 0 {
                        None
                    } else {
                        u16_at(msg, r.data + 4)
                            .filter(|&p| p != 0)
                            .zip(name(msg, r.data + 6).map(|(n, _)| n.to_ascii_lowercase()))
                            .map(|(port, host)| (host, port))
                    };
                }
                A if r.len == 4 => {
                    let host = r.name.to_ascii_lowercase();
                    if r.ttl == 0 {
                        self.hosts.remove(&host);
                    } else {
                        let d = &msg[r.data..r.data + 4];
                        self.hosts
                            .insert(host, Ipv4Addr::new(d[0], d[1], d[2], d[3]));
                    }
                }
                _ => {}
            }
        }
    }

    fn questions(&self) -> Vec<(String, u16)> {
        let mut out = vec![(SERVICE.into(), PTR)];
        for s in self.services.values().filter(|s| s.advertised) {
            if s.info.is_none() {
                out.push((s.instance.clone(), TXT));
            } else if s.info.as_ref().is_some_and(|(_, _, screen)| !screen) {
                continue;
            }
            match &s.target {
                None => out.push((s.instance.clone(), SRV)),
                Some((host, _)) if !self.hosts.contains_key(host) => {
                    out.push((host.clone(), A));
                }
                _ => {}
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn receivers(&self) -> Vec<Receiver> {
        let mut out = Vec::new();
        for s in self.services.values().filter(|s| s.advertised) {
            let Some((name, model, true)) = &s.info else {
                continue;
            };
            let Some((host, port)) = &s.target else {
                continue;
            };
            let Some(ip) = self.hosts.get(host) else {
                continue;
            };
            let addr = Addr::Cast(SocketAddr::from((*ip, *port)));
            if !out.iter().any(|r: &Receiver| r.addr == addr) {
                out.push(Receiver {
                    name: name.clone(),
                    model: model.clone(),
                    addr,
                });
            }
        }
        out.sort_by_key(|r| r.name.to_lowercase());
        out
    }
}

struct Record {
    name: String,
    kind: u16,
    class: u16,
    ttl: u32,
    data: usize,
    len: usize,
}

/// Every record of every section of `msg`.
fn records(msg: &[u8]) -> Option<Vec<Record>> {
    let count = |at| u16_at(msg, at).map(usize::from);
    let (questions, rest) = (count(4)?, count(6)? + count(8)? + count(10)?);
    let mut at = 12;
    for _ in 0..questions {
        at = name(msg, at)?.1 + 4;
    }
    let mut out = Vec::new();
    for _ in 0..rest {
        let (n, after) = name(msg, at)?;
        let len = usize::from(u16_at(msg, after + 8)?);
        let data = after + 10;
        msg.get(data..data + len)?;
        out.push(Record {
            name: n,
            kind: u16_at(msg, after)?,
            class: u16_at(msg, after + 2)?,
            ttl: u32::from_be_bytes(msg.get(after + 4..after + 8)?.try_into().ok()?),
            data,
            len,
        });
        at = data + len;
    }
    Some(out)
}

/// A TXT record's `key=value` strings.
fn text(mut data: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    while let Some((&len, rest)) = data.split_first() {
        let Some(s) = rest.get(..len as usize) else {
            break;
        };
        let s = String::from_utf8_lossy(s);
        if let Some((k, v)) = s.split_once('=') {
            out.push((k.to_string(), v.to_string()));
        }
        data = &rest[len as usize..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for l in name.split('.') {
            out.push(l.len() as u8);
            out.extend_from_slice(l.as_bytes());
        }
        out.push(0);
        out
    }

    fn record(out: &mut Vec<u8>, name: &[u8], kind: u16, data: &[u8]) {
        out.extend_from_slice(name);
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&[0x80, 1, 0, 0, 0x11, 0x94]);
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(data);
    }

    fn answer(ca: &str) -> Vec<u8> {
        let mut m = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 3];
        // The PTR's name at 12, so the instance can point back at it.
        let instance = labels("Chromecast-abc");
        let mut ptr = instance[..instance.len() - 1].to_vec();
        ptr.extend_from_slice(&[0xc0, 12]);
        record(&mut m, &labels(SERVICE), PTR, &ptr);
        let at = (12 + labels(SERVICE).len() + 10) as u8;
        let mut txt = Vec::new();
        for kv in [
            "id=1234",
            "md=Chromecast",
            &format!("ca={ca}"),
            "fn=Bedroom TV",
        ] {
            txt.push(kv.len() as u8);
            txt.extend_from_slice(kv.as_bytes());
        }
        record(&mut m, &[0xc0, at], TXT, &txt);
        let mut srv = vec![0, 0, 0, 0];
        srv.extend_from_slice(&8010u16.to_be_bytes());
        srv.extend_from_slice(&labels("abc.local"));
        record(&mut m, &[0xc0, at], SRV, &srv);
        record(&mut m, &labels("abc.local"), A, &[192, 168, 1, 82]);
        m
    }

    #[test]
    fn a_chromecast_is_reached_at_its_advertised_host() {
        let mut browse = Browse::default();
        browse.ingest(&answer("268805"));
        assert_eq!(
            browse.receivers(),
            [Receiver {
                name: "Bedroom TV".into(),
                model: "Chromecast".into(),
                addr: Addr::Cast("192.168.1.82:8010".parse().unwrap()),
            }]
        );
    }

    #[test]
    fn speakers_and_groups_are_passed_over() {
        let mut browse = Browse::default();
        browse.ingest(&answer("2052"));
        assert!(browse.receivers().is_empty(), "audio only");
        browse.ingest(&answer("2085"));
        assert!(browse.receivers().is_empty(), "a group");
        browse.ingest(&[0; 5]);
    }

    #[test]
    fn the_question_asks_for_the_service() {
        let q = question(SERVICE, PTR);
        assert_eq!(name(&q, 12).unwrap(), (SERVICE.to_string(), q.len() - 4));
    }

    fn part(kind: u16, goodbye: bool) -> Vec<u8> {
        let instance = "Chromecast-abc._googlecast._tcp.local";
        let (owner, data) = match kind {
            PTR => (SERVICE, labels(instance)),
            SRV => {
                let mut data = vec![0, 0, 0, 0, 0x1f, 0x49]; // port 8009
                data.extend(labels("abc.local"));
                (instance, data)
            }
            TXT => {
                let mut data = Vec::new();
                for text in ["fn=Bedroom TV", "md=Chromecast", "ca=1"] {
                    data.push(text.len() as u8);
                    data.extend(text.as_bytes());
                }
                (instance, data)
            }
            A => ("abc.local", vec![192, 168, 1, 82]),
            _ => unreachable!(),
        };
        let mut msg = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        let owner = labels(owner);
        record(&mut msg, &owner, kind, &data);
        if goodbye {
            let ttl = 12 + owner.len() + 4;
            msg[ttl..ttl + 4].fill(0);
        }
        msg
    }

    #[test]
    fn separate_packets_resolve_in_any_order_and_missing_records_are_queried() {
        for order in [[PTR, TXT, SRV, A], [A, SRV, TXT, PTR], [TXT, PTR, A, SRV]] {
            let mut browse = Browse::default();
            for (i, kind) in order.into_iter().enumerate() {
                browse.ingest(&part(kind, false));
                if i < 3 {
                    assert!(browse.receivers().is_empty());
                }
            }
            assert_eq!(
                browse.receivers()[0].addr,
                Addr::Cast("192.168.1.82:8009".parse().unwrap())
            );
            assert_eq!(browse.receivers()[0].name, "Bedroom TV");
        }
        let mut browse = Browse::default();
        browse.ingest(&part(PTR, false));
        let wanted = browse.questions();
        assert!(wanted.contains(&("Chromecast-abc._googlecast._tcp.local".into(), TXT)));
        assert!(wanted.contains(&("Chromecast-abc._googlecast._tcp.local".into(), SRV)));
        browse.ingest(&part(SRV, false));
        assert!(browse.questions().contains(&("abc.local".into(), A)));
    }

    #[test]
    fn withdrawn_services_and_addresses_are_not_connectable() {
        let mut browse = Browse::default();
        browse.ingest(&answer("1"));
        assert!(!browse.receivers().is_empty());
        browse.ingest(&part(A, true));
        assert!(browse.receivers().is_empty());
        browse.ingest(&part(A, false));
        assert!(!browse.receivers().is_empty());
        browse.ingest(&part(PTR, true));
        assert!(browse.receivers().is_empty());
    }
}
