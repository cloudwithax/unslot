//! Wi-Fi Display's RTSP, the handheld as the source: the TV connects, the two agree on the
//! picture, the sound and the port they go to (M1 to M7), and then the source keeps the session
//! alive and answers the TV until either ends it.

use std::io::{ErrorKind, Read, Write};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use super::ts::Sound;

/// Where the TV connects, as the handheld's Wi-Fi Display information says.
pub const PORT: u16 = 7236;
/// How long the TV may go unheard from before the session is dead, as told it at SETUP.
const TIMEOUT_S: u64 = 30;

/// A picture Wi-Fi Display names: one of CEA's, H.264 Constrained Baseline at `level`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mode {
    pub w: u32,
    pub h: u32,
    pub fps: u32,
    /// Its bit in the CEA mask.
    bit: u32,
    level: u8,
}

/// The pictures sent: 640 by 480 at 60 a second, which every TV takes. The screens are no
/// bigger than it, near enough, and 720p costs the handheld 40 ms a frame, 25 a second at best.
const MODES: [Mode; 1] = [Mode {
    w: 640,
    h: 480,
    fps: 60,
    bit: 0,
    level: 0x01,
}];

/// What the TV took: the picture, whether it takes the sound, and its RTP port.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Agreed {
    pub mode: Mode,
    pub audio: Option<Sound>,
    pub rtp_port: u16,
}

/// What the TV asked for while streaming.
#[derive(PartialEq, Eq, Debug)]
pub enum Heard {
    Nothing,
    KeyFrame,
    Teardown,
}

/// One RTSP message, a request or a response.
#[derive(Debug)]
pub struct Msg {
    pub line: String,
    headers: Vec<(String, String)>,
    pub body: String,
}

impl Msg {
    pub fn header(&self, name: &str) -> Option<&str> {
        let h = self
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name));
        h.map(|(_, v)| v.as_str())
    }

    fn cseq(&self) -> u32 {
        self.header("CSeq")
            .and_then(|c| c.trim().parse().ok())
            .unwrap_or(0)
    }

    fn is_response(&self) -> bool {
        self.line.starts_with("RTSP/")
    }

    fn method(&self) -> &str {
        self.line.split(' ').next().unwrap_or("")
    }

    fn ok(&self) -> bool {
        self.line.split(' ').nth(1) == Some("200")
    }

    /// A `name: value` line of a text/parameters body.
    fn param(&self, name: &str) -> Option<&str> {
        self.body.lines().find_map(|l| {
            let (n, v) = l.split_once(':')?;
            n.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }
}

/// The RTSP connection: `S` reads with a timeout, as a TcpStream with one set does.
pub struct Rtsp<S> {
    s: S,
    buf: Vec<u8>,
    cseq: u32,
    session: u32,
    heard: Instant,
    pinged: Instant,
}

impl<S: Read + Write> Rtsp<S> {
    pub fn new(s: S, session: u32) -> Rtsp<S> {
        Rtsp {
            s,
            buf: Vec::new(),
            cseq: 0,
            session,
            heard: Instant::now(),
            pinged: Instant::now(),
        }
    }

    /// The next whole message, or None when none came before the stream's timeout.
    fn read(&mut self) -> Result<Option<Msg>, String> {
        loop {
            if let Some(m) = self.take() {
                self.heard = Instant::now();
                return Ok(Some(m));
            }
            let mut chunk = [0u8; 4096];
            match self.s.read(&mut chunk) {
                Ok(0) => return Err("the TV hung up".into()),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Ok(None)
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    /// A whole message off the front of what has been read, if one is there.
    fn take(&mut self) -> Option<Msg> {
        let end = self.buf.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
        let head = String::from_utf8_lossy(&self.buf[..end - 4]).into_owned();
        let mut lines = head.split("\r\n");
        let line = lines.next().unwrap_or("").to_string();
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
            .collect();
        let len = headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("Content-Length"))
            .and_then(|(_, v)| v.parse::<usize>().ok())
            .unwrap_or(0);
        if self.buf.len() < end + len {
            return None;
        }
        let body = String::from_utf8_lossy(&self.buf[end..end + len]).into_owned();
        self.buf.drain(..end + len);
        Some(Msg {
            line,
            headers,
            body,
        })
    }

    fn write(&mut self, text: &str) -> Result<(), String> {
        self.s.write_all(text.as_bytes()).map_err(|e| e.to_string())
    }

    /// Sends a request, its body text/parameters; its CSeq.
    fn request(&mut self, method: &str, extra: &str, body: &str) -> Result<u32, String> {
        self.cseq += 1;
        let url = if method == "OPTIONS" {
            "*"
        } else {
            "rtsp://localhost/wfd1.0"
        };
        let mut m = format!("{method} {url} RTSP/1.0\r\nCSeq: {}\r\n{extra}", self.cseq);
        if !body.is_empty() {
            m += &format!(
                "Content-Type: text/parameters\r\nContent-Length: {}\r\n",
                body.len()
            );
        }
        m += "\r\n";
        m += body;
        self.write(&m)?;
        Ok(self.cseq)
    }

    fn reply(&mut self, to: &Msg, extra: &str) -> Result<(), String> {
        let m = format!("RTSP/1.0 200 OK\r\nCSeq: {}\r\n{extra}\r\n", to.cseq());
        self.write(&m)
    }

    fn session_header(&self) -> String {
        format!("Session: {};timeout={TIMEOUT_S}\r\n", self.session)
    }

    /// Talks the TV through to playing, `me` being the handheld's address and `server_port`
    /// where the stream leaves from, giving up after `wait`.
    pub fn negotiate(
        &mut self,
        me: Ipv4Addr,
        server_port: u16,
        wait: Duration,
    ) -> Result<Agreed, String> {
        let end = Instant::now() + wait;
        let m1 = self.request("OPTIONS", "Require: org.wfa.wfd1.0\r\n", "")?;
        let (mut m3, mut m4, mut m5) = (None, None, None);
        let mut agreed: Option<Agreed> = None;
        while Instant::now() < end {
            let Some(m) = self.read()? else {
                continue;
            };
            if m.is_response() {
                let cseq = Some(m.cseq());
                if !m.ok() {
                    eprintln!("slot: cast: the TV answered {} with {}", m.cseq(), m.line);
                    return Err("the TV would not take the stream".into());
                }
                if cseq == m3 {
                    let (a, p) = from_m3(&m)?;
                    let body = m4_body(&a, &p, me);
                    agreed = Some(a);
                    m4 = Some(self.request("SET_PARAMETER", "", &body)?);
                } else if cseq == m4 {
                    let body = "wfd_trigger_method: SETUP\r\n";
                    m5 = Some(self.request("SET_PARAMETER", "", body)?);
                } else if cseq != Some(m1) && cseq != m5 {
                    eprintln!("slot: cast: the TV answered {}, never asked", m.cseq());
                }
                continue;
            }
            match m.method() {
                "OPTIONS" => {
                    let public = "Public: org.wfa.wfd1.0, SETUP, TEARDOWN, PLAY, PAUSE, \
                                  GET_PARAMETER, SET_PARAMETER\r\n";
                    self.reply(&m, public)?;
                    if m3.is_none() {
                        let body = "wfd_video_formats\r\nwfd_audio_codecs\r\n\
                                    wfd_client_rtp_ports\r\n";
                        m3 = Some(self.request("GET_PARAMETER", "", body)?);
                    }
                }
                "SETUP" => {
                    let Some(a) = agreed.as_mut() else {
                        return Err("the TV asked to start before it said what it takes".into());
                    };
                    let port = m
                        .header("Transport")
                        .and_then(client_port)
                        .unwrap_or(a.rtp_port);
                    a.rtp_port = port;
                    let extra = format!(
                        "{}Transport: RTP/AVP/UDP;unicast;client_port={port};\
                         server_port={server_port}\r\n",
                        self.session_header()
                    );
                    self.reply(&m, &extra)?;
                }
                "PLAY" => {
                    let extra = self.session_header();
                    self.reply(&m, &extra)?;
                    return agreed.ok_or_else(|| "the TV started out of turn".into());
                }
                "TEARDOWN" => {
                    self.reply(&m, "")?;
                    return Err("the TV said no".into());
                }
                _ => self.reply(&m, "")?,
            }
        }
        Err("the TV did not answer".into())
    }

    /// Answers whatever the TV said within the stream's timeout, and keeps it alive: what it
    /// asked for, or why the session is over.
    pub fn tend(&mut self) -> Result<Heard, String> {
        if self.pinged.elapsed() >= Duration::from_secs(TIMEOUT_S - 5) {
            self.pinged = Instant::now();
            let extra = format!("Session: {}\r\n", self.session);
            self.request("GET_PARAMETER", &extra, "")?;
        }
        if self.heard.elapsed() >= Duration::from_secs(TIMEOUT_S + 5) {
            return Err("the TV stopped answering".into());
        }
        let Some(m) = self.read()? else {
            return Ok(Heard::Nothing);
        };
        if m.is_response() {
            return Ok(Heard::Nothing);
        }
        let extra = self.session_header();
        self.reply(&m, &extra)?;
        Ok(match m.method() {
            "TEARDOWN" => Heard::Teardown,
            "SET_PARAMETER" if m.body.contains("wfd_idr_request") => Heard::KeyFrame,
            _ => Heard::Nothing,
        })
    }

    /// Asks the TV to end the session, and answers its TEARDOWN if it sends one in time.
    pub fn teardown(&mut self) {
        let body = "wfd_trigger_method: TEARDOWN\r\n";
        if self.request("SET_PARAMETER", "", body).is_err() {
            return;
        }
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            match self.tend() {
                Ok(Heard::Teardown) | Err(_) => return,
                Ok(_) => {}
            }
        }
    }
}

/// What the TV said it takes, in its answer to M3: what is agreed, and its RTP ports as it put
/// them.
fn from_m3(m: &Msg) -> Result<(Agreed, String), String> {
    let video = m.param("wfd_video_formats").unwrap_or("none");
    let Some(mode) = pick_mode(video) else {
        eprintln!("slot: cast: the TV takes no picture sent: {video}");
        return Err("the TV takes no picture the handheld sends".into());
    };
    let codecs = m.param("wfd_audio_codecs").unwrap_or("none");
    eprintln!("slot: cast: the TV takes {video}; {codecs}");
    let audio = pick_sound(codecs);
    let ports = m.param("wfd_client_rtp_ports").unwrap_or("").to_string();
    let rtp_port = ports
        .split_whitespace()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .filter(|&p| p != 0)
        .ok_or("the TV gave no port for the stream")?;
    Ok((
        Agreed {
            mode,
            audio,
            rtp_port,
        },
        ports,
    ))
}

/// How the sound goes to a TV with `wfd_audio_codecs`, at 48 kHz stereo as it is played: AAC if
/// it takes it, else LPCM, else none.
fn pick_sound(codecs: &str) -> Option<Sound> {
    let takes = |codec: &str, bit: u32| {
        codecs.split(',').any(|c| {
            let mut f = c.split_whitespace();
            f.next() == Some(codec)
                && f.next()
                    .and_then(|m| u32::from_str_radix(m, 16).ok())
                    .is_some_and(|m| m & bit != 0)
        })
    };
    if takes("AAC", 1) {
        Some(Sound::Aac)
    } else if takes("LPCM", 2) {
        Some(Sound::Lpcm)
    } else {
        None
    }
}

/// The first of `MODES` a TV's `wfd_video_formats` has, in any profile: Constrained Baseline,
/// which is sent, is Constrained High too.
pub fn pick_mode(formats: &str) -> Option<Mode> {
    let mut cea = 0u32;
    for (i, codec) in formats.split(',').enumerate() {
        let t: Vec<&str> = codec.split_whitespace().collect();
        // The native and preferred fields come before the first codec only.
        let t = if i == 0 { t.get(2..)? } else { &t[..] };
        let hex = |n: usize| t.get(n).and_then(|s| u32::from_str_radix(s, 16).ok());
        cea |= hex(2)?;
    }
    MODES.into_iter().find(|m| cea & (1 << m.bit) != 0)
}

/// M4: the picture and sound chosen, where the session is, and the TV's ports back.
fn m4_body(a: &Agreed, ports: &str, me: Ipv4Addr) -> String {
    let mut body = format!(
        "wfd_video_formats: 00 00 01 {:02x} {:08x} 00000000 00000000 00 0000 0000 00 none none\r\n",
        a.mode.level,
        1u32 << a.mode.bit
    );
    match a.audio {
        Some(Sound::Aac) => body += "wfd_audio_codecs: AAC 00000001 00\r\n",
        Some(Sound::Lpcm) => body += "wfd_audio_codecs: LPCM 00000002 00\r\n",
        None => {}
    }
    body += &format!("wfd_presentation_URL: rtsp://{me}/wfd1.0/streamid=0 none\r\n");
    body += &format!("wfd_client_rtp_ports: {ports}\r\n");
    body
}

/// `client_port=` of a Transport header.
fn client_port(transport: &str) -> Option<u16> {
    let p = transport
        .split(';')
        .find_map(|f| f.trim().strip_prefix("client_port="))?;
    p.split('-').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn the_picture_is_640_by_480_at_60() {
        let lg = "00 00 02 10 0001ffff 1fffffff 00000fff 00 0000 0000 11 none none";
        assert_eq!(pick_mode(lg).map(|m| (m.w, m.fps)), Some((640, 60)));
        // Constrained High in the first codec, Baseline in the second.
        let two = "40 00 02 04 00000020 0 0 00 0000 0000 00 none none, \
                   01 01 00000001 0 0 00 0000 0000 00 none none";
        assert_eq!(pick_mode(two).map(|m| (m.w, m.fps)), Some((640, 60)));
        let no_vga = "00 00 01 01 00000060 00000000 00000000 00 0000 0000 00 none none";
        assert_eq!(pick_mode(no_vga), None);
        assert_eq!(pick_mode("none"), None);
    }

    #[test]
    fn the_sound_goes_as_aac_at_48_khz_else_as_lpcm() {
        assert_eq!(
            pick_sound("LPCM 00000003 00, AAC 0000000F 00"),
            Some(Sound::Aac)
        );
        assert_eq!(
            pick_sound("AAC 00000002 00, LPCM 00000002 00"),
            Some(Sound::Lpcm)
        );
        assert_eq!(pick_sound("LPCM 00000001 00"), None);
        assert_eq!(pick_sound("none"), None);
    }

    #[test]
    fn a_setup_says_its_port() {
        assert_eq!(
            client_port("RTP/AVP/UDP;unicast;client_port=19000"),
            Some(19000)
        );
        assert_eq!(
            client_port("RTP/AVP/UDP;unicast;client_port=1028-1029"),
            Some(1028)
        );
        assert_eq!(client_port("RTP/AVP/UDP;unicast"), None);
    }

    /// A TV that goes through M1 to M7 as a Roku does, then asks for a key frame and ends it.
    fn sink(s: TcpStream) -> Vec<String> {
        let mut r = std::io::BufReader::new(s.try_clone().unwrap());
        let mut w = s;
        let mut seen = Vec::new();
        let mut read = |r: &mut std::io::BufReader<TcpStream>| {
            let (mut head, mut len) = (Vec::new(), 0);
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                let l = l.trim_end().to_string();
                if l.is_empty() {
                    break;
                }
                if let Some(v) = l.strip_prefix("Content-Length: ") {
                    len = v.parse().unwrap();
                }
                head.push(l);
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let all = format!("{}\n{}", head.join("\n"), String::from_utf8(body).unwrap());
            seen.push(all.clone());
            all
        };
        let cseq = |m: &str| {
            m.lines()
                .find_map(|l| l.strip_prefix("CSeq: "))
                .unwrap()
                .to_string()
        };
        let m1 = read(&mut r);
        write!(w, "RTSP/1.0 200 OK\r\nCSeq: {}\r\nPublic: org.wfa.wfd1.0, GET_PARAMETER, SET_PARAMETER\r\n\r\n", cseq(&m1)).unwrap();
        write!(
            w,
            "OPTIONS * RTSP/1.0\r\nCSeq: 1\r\nRequire: org.wfa.wfd1.0\r\n\r\n"
        )
        .unwrap();
        let _m2 = read(&mut r);
        let m3 = read(&mut r);
        let body = "wfd_video_formats: 00 00 02 10 0001ffff 1fffffff 00000fff 00 0000 0000 11 none none\r\n\
                    wfd_audio_codecs: LPCM 00000003 00, AAC 00000007 00\r\n\
                    wfd_client_rtp_ports: RTP/AVP/UDP;unicast 1028 0 mode=play\r\n";
        write!(
            w,
            "RTSP/1.0 200 OK\r\nCSeq: {}\r\nContent-Length: {}\r\n\r\n{body}",
            cseq(&m3),
            body.len()
        )
        .unwrap();
        let m4 = read(&mut r);
        write!(w, "RTSP/1.0 200 OK\r\nCSeq: {}\r\n\r\n", cseq(&m4)).unwrap();
        let m5 = read(&mut r);
        write!(w, "RTSP/1.0 200 OK\r\nCSeq: {}\r\n\r\n", cseq(&m5)).unwrap();
        write!(w, "SETUP rtsp://192.168.49.2/wfd1.0/streamid=0 RTSP/1.0\r\nCSeq: 2\r\nTransport: RTP/AVP/UDP;unicast;client_port=1028\r\n\r\n").unwrap();
        read(&mut r);
        write!(
            w,
            "PLAY rtsp://192.168.49.2/wfd1.0/streamid=0 RTSP/1.0\r\nCSeq: 3\r\nSession: 1\r\n\r\n"
        )
        .unwrap();
        read(&mut r);
        let idr = "wfd_idr_request\r\n";
        write!(w, "SET_PARAMETER rtsp://localhost/wfd1.0 RTSP/1.0\r\nCSeq: 4\r\nContent-Length: {}\r\n\r\n{idr}", idr.len()).unwrap();
        read(&mut r);
        write!(
            w,
            "TEARDOWN rtsp://192.168.49.2/wfd1.0/streamid=0 RTSP/1.0\r\nCSeq: 5\r\n\r\n"
        )
        .unwrap();
        read(&mut r);
        seen
    }

    #[test]
    fn a_tv_is_talked_through_to_playing_then_asks_for_a_key_frame_and_ends_it() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let tv = std::thread::spawn(move || sink(TcpStream::connect(addr).unwrap()));
        let (s, _) = l.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut rtsp = Rtsp::new(s, 1234);
        let me = Ipv4Addr::new(192, 168, 49, 2);
        let agreed = rtsp.negotiate(me, 5000, Duration::from_secs(5)).unwrap();
        assert_eq!(
            (agreed.mode.w, agreed.mode.h, agreed.mode.fps),
            (640, 480, 60)
        );
        assert_eq!(agreed.audio, Some(Sound::Aac));
        assert_eq!(agreed.rtp_port, 1028);
        let mut heard = Vec::new();
        while heard.last() != Some(&Heard::Teardown) {
            match rtsp.tend().unwrap() {
                Heard::Nothing => {}
                h => heard.push(h),
            }
        }
        assert_eq!(heard, [Heard::KeyFrame, Heard::Teardown]);
        let seen = tv.join().unwrap();
        let m4 = &seen[3];
        assert!(
            m4.contains("wfd_video_formats: 00 00 01 01 00000001 "),
            "{m4}"
        );
        assert!(m4.contains("wfd_audio_codecs: AAC 00000001 00"));
        assert!(m4.contains("wfd_presentation_URL: rtsp://192.168.49.2/wfd1.0/streamid=0 none"));
        assert!(m4.contains("wfd_client_rtp_ports: RTP/AVP/UDP;unicast 1028 0 mode=play"));
        assert!(seen[4].contains("wfd_trigger_method: SETUP"));
        let setup = &seen[5];
        assert!(setup.contains("Session: 1234;timeout=30"), "{setup}");
        assert!(
            setup.contains("client_port=1028;server_port=5000"),
            "{setup}"
        );
    }
}
