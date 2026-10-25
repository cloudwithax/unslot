//! Cast's control channel: JSON messages between named ends, each wrapped in a small protobuf
//! `CastMessage` and sent with its length before it.

pub const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
pub const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
pub const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
pub const NS_WEBRTC: &str = "urn:x-cast:com.google.cast.webrtc";

/// The receiver app that shows a mirrored screen.
pub const MIRRORING_APP: &str = "0F5096E8";

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Message {
    pub from: String,
    pub to: String,
    pub namespace: String,
    pub json: String,
}

fn varint(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        out.push(b | if n == 0 { 0 } else { 0x80 });
        if n == 0 {
            return;
        }
    }
}

fn field(out: &mut Vec<u8>, number: u64, text: &str) {
    varint(out, number << 3 | 2);
    varint(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// `json` from `from` to `to` in `namespace`, ready to write.
pub fn encode(from: &str, to: &str, namespace: &str, json: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[1 << 3, 0]); // protocol version 2.1.0
    field(&mut body, 2, from);
    field(&mut body, 3, to);
    field(&mut body, 4, namespace);
    body.extend_from_slice(&[5 << 3, 0]); // a string payload
    field(&mut body, 6, json);
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend(body);
    out
}

fn read_varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let c = *b.get(*at)?;
        *at += 1;
        v |= u64::from(c & 0x7f) << shift;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// A message's body, what follows its length.
pub fn decode(b: &[u8]) -> Option<Message> {
    let mut m = Message {
        from: String::new(),
        to: String::new(),
        namespace: String::new(),
        json: String::new(),
    };
    let mut at = 0;
    while at < b.len() {
        let key = read_varint(b, &mut at)?;
        match key & 7 {
            0 => {
                read_varint(b, &mut at)?;
            }
            2 => {
                let len = read_varint(b, &mut at)? as usize;
                let text = String::from_utf8_lossy(b.get(at..at.checked_add(len)?)?).into_owned();
                at += len;
                match key >> 3 {
                    2 => m.from = text,
                    3 => m.to = text,
                    4 => m.namespace = text,
                    6 => m.json = text,
                    _ => {}
                }
            }
            _ => return None,
        }
    }
    Some(m)
}

/// The JSON object around byte `at` of `json`.
fn object_around(json: &str, at: usize) -> Option<&str> {
    let mut opens = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in json.char_indices() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => opens.push(i),
            '}' => {
                let open = opens.pop()?;
                if open < at && at < i {
                    return Some(&json[open..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The mirroring app's session and transport ids, from a receiver's status once it is running.
pub fn mirroring_app(status: &str) -> Option<(String, String)> {
    let at = status.find(&format!("\"{MIRRORING_APP}\""))?;
    let app = object_around(status, at)?;
    let session = crate::json::string(app, "sessionId")?;
    let transport = crate::json::string(app, "transportId")?;
    Some((session, transport))
}

/// The numbers in the list named `key`.
pub fn numbers(json: &str, key: &str) -> Vec<u64> {
    let Some(at) = json.find(&format!("\"{key}\"")) else {
        return Vec::new();
    };
    let rest = &json[at + key.len() + 2..];
    let list = rest
        .split_once('[')
        .and_then(|(_, after)| after.split_once(']'))
        .map_or("", |(list, _)| list);
    list.split(',').filter_map(|n| n.trim().parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_comes_back_as_it_went() {
        let bytes = encode("sender-0", "receiver-0", NS_RECEIVER, r#"{"type":"LAUNCH"}"#);
        let len = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert_eq!(len, bytes.len() - 4);
        let m = decode(&bytes[4..]).unwrap();
        assert_eq!(m.from, "sender-0");
        assert_eq!(m.to, "receiver-0");
        assert_eq!(m.namespace, NS_RECEIVER);
        assert_eq!(m.json, r#"{"type":"LAUNCH"}"#);
    }

    #[test]
    fn the_mirroring_apps_ids_are_its_own_not_anothers() {
        let status = r#"{"type":"RECEIVER_STATUS","status":{"applications":[
            {"appId":"E8C28D3C","sessionId":"backdrop","transportId":"t-0"},
            {"appId":"0F5096E8","displayName":"Chrome Mirroring","namespaces":[{"name":"x"}],
             "sessionId":"abc-123","statusText":"{curly}","transportId":"abc-123"}]}}"#;
        assert_eq!(mirroring_app(status), Some(("abc-123".into(), "abc-123".into())));
        assert_eq!(mirroring_app(r#"{"status":{"applications":[]}}"#), None);
    }

    #[test]
    fn a_list_of_numbers_is_read() {
        let answer = r#"{"answer":{"udpPort":39662,"sendIndexes":[0, 1],"ssrcs":[5,6]}}"#;
        assert_eq!(numbers(answer, "sendIndexes"), [0, 1]);
        assert_eq!(numbers(answer, "ssrcs"), [5, 6]);
        assert!(numbers(answer, "nothing").is_empty());
    }
}
