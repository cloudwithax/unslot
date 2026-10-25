//! Finding Chromecasts: one multicast DNS question for `_googlecast._tcp.local`, asked from an
//! ordinary port so each answers straight back to it rather than to the whole network.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const SERVICE: &str = "_googlecast._tcp.local";
const MDNS: (Ipv4Addr, u16) = (Ipv4Addr::new(224, 0, 0, 251), 5353);

const PTR: u16 = 12;
const TXT: u16 = 16;
const SRV: u16 = 33;

/// A Chromecast or TV with one built in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Receiver {
    /// What its owner called it.
    pub name: String,
    pub model: String,
    pub addr: SocketAddr,
}

/// The receivers that answer within `wait`, those that can show a picture, by name.
pub fn search(wait: Duration) -> std::io::Result<Vec<Receiver>> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    let query = question();
    let end = Instant::now() + wait;
    let mut asked = None::<Instant>;
    let mut found: Vec<Receiver> = Vec::new();
    let mut buf = [0u8; 9000];
    loop {
        let now = Instant::now();
        if now >= end {
            break;
        }
        // Asked again each second, as one question on Wi-Fi is easily lost.
        if asked.is_none_or(|a| now >= a + Duration::from_secs(1)) {
            sock.send_to(&query, MDNS)?;
            asked = Some(now);
        }
        let until = (end - now).min(Duration::from_millis(250));
        sock.set_read_timeout(Some(until.max(Duration::from_millis(1))))?;
        let (n, from) = match sock.recv_from(&mut buf) {
            Ok(got) => got,
            Err(e) if timed_out(&e) => continue,
            Err(e) => return Err(e),
        };
        for r in parse(&buf[..n], from) {
            if !found.iter().any(|f| f.addr == r.addr) {
                found.push(r);
            }
        }
    }
    found.sort_by_key(|r| r.name.to_lowercase());
    Ok(found)
}

/// Whether a read gave up for its timeout.
pub(crate) fn timed_out(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
}

/// The question: who offers the Cast service.
fn question() -> Vec<u8> {
    let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in SERVICE.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&PTR.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());
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

/// The receivers an answer from `from` tells of: there is one per Chromecast, its TXT record
/// giving the name and what it can do, its SRV record the port.
pub fn parse(msg: &[u8], from: SocketAddr) -> Vec<Receiver> {
    let Some(records) = records(msg) else {
        return Vec::new();
    };
    let instances = records
        .iter()
        .filter(|r| r.kind == PTR && r.name.eq_ignore_ascii_case(SERVICE))
        .filter_map(|r| name(msg, r.data).map(|(n, _)| n));
    let mut out = Vec::new();
    for instance in instances {
        let mine = |kind: u16| {
            records
                .iter()
                .find(|r| r.kind == kind && r.name.eq_ignore_ascii_case(&instance))
        };
        let Some(txt) = mine(TXT) else { continue };
        let txt = text(&msg[txt.data..txt.data + txt.len]);
        let get = |key: &str| {
            txt.iter()
                .find_map(|(k, v)| k.eq_ignore_ascii_case(key).then(|| v.clone()))
        };
        // Bit 0 says it has a screen; bit 5 that it is a group of speakers.
        let can: u32 = get("ca").and_then(|c| c.parse().ok()).unwrap_or(0);
        if can & 1 == 0 || can & 32 != 0 {
            continue;
        }
        let port = mine(SRV)
            .and_then(|s| u16_at(msg, s.data + 4))
            .unwrap_or(8009);
        out.push(Receiver {
            name: get("fn").unwrap_or_else(|| instance.clone()),
            model: get("md").unwrap_or_default(),
            addr: SocketAddr::new(from.ip(), port),
        });
    }
    out
}

struct Record {
    name: String,
    kind: u16,
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
        let Some(s) = rest.get(..len as usize) else { break };
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
        let mut m = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 2];
        // The PTR's name at 12, so the instance can point back at it.
        let instance = labels("Chromecast-abc");
        let mut ptr = instance[..instance.len() - 1].to_vec();
        ptr.extend_from_slice(&[0xc0, 12]);
        record(&mut m, &labels(SERVICE), PTR, &ptr);
        let at = (12 + labels(SERVICE).len() + 10) as u8;
        let mut txt = Vec::new();
        for kv in ["id=1234", "md=Chromecast", &format!("ca={ca}"), "fn=Bedroom TV"] {
            txt.push(kv.len() as u8);
            txt.extend_from_slice(kv.as_bytes());
        }
        record(&mut m, &[0xc0, at], TXT, &txt);
        let mut srv = vec![0, 0, 0, 0];
        srv.extend_from_slice(&8010u16.to_be_bytes());
        srv.extend_from_slice(&labels("abc.local"));
        record(&mut m, &[0xc0, at], SRV, &srv);
        m
    }

    #[test]
    fn a_chromecast_is_known_by_its_name_and_found_where_it_answered_from() {
        let from: SocketAddr = "192.168.1.82:5353".parse().unwrap();
        assert_eq!(
            parse(&answer("268805"), from),
            [Receiver {
                name: "Bedroom TV".into(),
                model: "Chromecast".into(),
                addr: "192.168.1.82:8010".parse().unwrap(),
            }]
        );
    }

    #[test]
    fn speakers_and_groups_are_passed_over() {
        let from: SocketAddr = "192.168.1.9:5353".parse().unwrap();
        assert!(parse(&answer("2052"), from).is_empty(), "audio only");
        assert!(parse(&answer("2085"), from).is_empty(), "a group");
        assert!(parse(&[0; 5], from).is_empty());
    }

    #[test]
    fn the_question_asks_for_the_service() {
        let q = question();
        assert_eq!(name(&q, 12).unwrap(), (SERVICE.to_string(), q.len() - 4));
    }
}
