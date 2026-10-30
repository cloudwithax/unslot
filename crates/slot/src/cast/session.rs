//! A cast in progress: the mirroring app launched on the receiver over the control channel, then
//! the screen and the sound streamed to it over UDP until either end stops.
//!
//! The control thread owns the TLS connection. Beside it, one thread encodes and sends the
//! picture, one the sound, and one reads the receiver's feedback, sending again what it lost and
//! telling it every half second how the streams' clocks stand.

use std::io::Read;
use std::net::{Ipv4Addr, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use slot_gfx::cast_tap::{set_frame_tap, FrameTap};

use super::channel::{self, Message, NS_CONNECTION, NS_HEARTBEAT, NS_RECEIVER, NS_WEBRTC};
use super::codec::{self, Codecs, Opus, H264, OPUS_FRAMES};
use super::mdns::{Addr, Receiver};
use super::rtp::{parse_rtcp, Feedback, Stream};
use super::tls::Tls;

const FPS: u32 = 60;
const VIDEO_KBPS: u32 = 5000;
/// Threads x264 cuts each frame between, for it to be sent sooner.
const VIDEO_THREADS: u32 = 2;
const AUDIO_BPS: u32 = 128_000;
/// How long the receiver holds each frame before showing it, to have time to ask for what it lost.
const DELAY_MS: u16 = 100;
const AUDIO_PT: u8 = 96;
const VIDEO_PT: u8 = 101;

/// The receiver's own end of the control channel, and the platform's.
const RECEIVER: &str = "receiver-0";
const PLATFORM: &str = "sender-0";

/// The control channel, answering the receiver's pings as it reads.
struct Control {
    tls: Tls,
    me: String,
    requests: u32,
}

impl Control {
    fn send(&mut self, from: &str, to: &str, namespace: &str, json: &str) -> Result<(), String> {
        self.tls.write_all(&channel::encode(from, to, namespace, json))
    }

    fn request(&mut self) -> u32 {
        self.requests += 1;
        self.requests
    }

    /// The next message within `wait`, pings answered on the way.
    fn recv(&mut self, wait: Duration) -> Result<Option<Message>, String> {
        let end = Instant::now() + wait;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if !self.tls.readable(left) {
                return Ok(None);
            }
            let mut len = [0u8; 4];
            self.tls.read_exact(&mut len)?;
            let len = u32::from_be_bytes(len) as usize;
            if len > 1 << 20 {
                return Err("the receiver sent nonsense".into());
            }
            let mut body = vec![0; len];
            self.tls.read_exact(&mut body)?;
            let Some(m) = channel::decode(&body) else {
                continue;
            };
            if m.namespace == NS_HEARTBEAT && kind(&m.json) == "PING" {
                self.send(&m.to, &m.from, NS_HEARTBEAT, r#"{"type":"PONG"}"#)?;
                continue;
            }
            return Ok(Some(m));
        }
    }

    /// The first message `wanted` picks out within `wait`.
    fn wait_for<T>(
        &mut self,
        wait: Duration,
        mut wanted: impl FnMut(&Message) -> Option<T>,
    ) -> Result<T, String> {
        let end = Instant::now() + wait;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            if let Some(found) = self.recv(left)?.as_ref().and_then(&mut wanted) {
                return Ok(found);
            }
        }
        Err("the receiver did not answer".into())
    }
}

fn kind(json: &str) -> String {
    crate::json::string(json, "type").unwrap_or_default().to_uppercase()
}

pub(super) fn random<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    if let Err(e) = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut out)) {
        eprintln!("slot: cast: /dev/urandom: {e}");
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What the media threads share.
struct Shared {
    udp: UdpSocket,
    video: Mutex<Stream>,
    audio: Mutex<Stream>,
    /// The receiver wants a key frame.
    want_key: AtomicBool,
    stop: AtomicBool,
    /// RTP time 0 of both streams.
    t0: Instant,
}

impl Shared {
    fn rtp(&self, at: Instant, rate: u32) -> u32 {
        (at.saturating_duration_since(self.t0).as_secs_f64() * f64::from(rate)) as u64 as u32
    }

    fn send(&self, packet: &[u8]) {
        // A packet the network would not take is as good as lost, and asked for again.
        let _ = self.udp.send(packet);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Session {
    stop: Arc<AtomicBool>,
    ended: Arc<Mutex<Option<String>>>,
    control: Option<JoinHandle<()>>,
}

impl Session {
    /// Casts to `to` with the encoders in `lib`, returning once the receiver has agreed to it.
    pub fn start(lib: &Path, to: &Receiver) -> Result<Session, String> {
        let tap = FrameTap::new(FPS);
        set_frame_tap(Some(tap.clone()));
        let started = Session::begin(lib, to, tap);
        if started.is_err() {
            set_frame_tap(None);
            crate::audio::set_cast_tap(None);
        }
        started
    }

    fn begin(lib: &Path, to: &Receiver, tap: Arc<FrameTap>) -> Result<Session, String> {
        let Addr::Cast(addr) = to.addr else {
            return Err("not a Chromecast".into());
        };
        let codecs = Codecs::load(lib)?;
        let tls = Tls::connect(&codecs, addr, Duration::from_secs(5))?;
        let me = format!("sender-{}", 10_000 + u32::from_le_bytes(random()) % 90_000);
        let mut control = Control {
            tls,
            me,
            requests: 0,
        };
        let (session, transport) = launch(&mut control)?;
        let streaming = agree(&mut control, &tap, to, &transport).and_then(|agreed| {
            let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| e.to_string())?;
            udp.connect((addr.ip(), agreed.0)).map_err(|e| e.to_string())?;
            udp.set_read_timeout(Some(Duration::from_millis(100)))
                .map_err(|e| e.to_string())?;
            Ok((agreed, udp))
        });
        let ((port, size, audio_on, offer), udp) = match streaming {
            Ok(s) => s,
            Err(e) => {
                stop_app(&mut control, &session, &transport);
                return Err(e);
            }
        };
        let shared = Arc::new(Shared {
            udp,
            video: Mutex::new(offer.video),
            audio: Mutex::new(offer.audio),
            want_key: AtomicBool::new(true),
            stop: AtomicBool::new(false),
            t0: Instant::now(),
        });
        eprintln!(
            "slot: cast: to {} at {}, {}x{}, port {port}{}",
            to.name,
            addr,
            size.0,
            size.1,
            if audio_on { "" } else { ", no sound" }
        );
        let mut threads = Vec::new();
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
            std::thread::Builder::new()
                .name(name.into())
                .spawn(f)
                .map_err(|e| e.to_string())
        };
        let ended = Arc::new(Mutex::new(None));
        let (s, c, e) = (shared.clone(), codecs.clone(), ended.clone());
        threads.push(spawn("slot-cast-video", Box::new(move || video(&s, &tap, &c, &e)))?);
        if audio_on {
            let s = shared.clone();
            threads.push(spawn("slot-cast-audio", Box::new(move || audio(&s, &codecs)))?);
        }
        let s = shared.clone();
        threads.push(spawn("slot-cast-rtcp", Box::new(move || rtcp(&s)))?);
        let stop = Arc::new(AtomicBool::new(false));
        let (st, e) = (stop.clone(), ended.clone());
        let run = move || {
            let why = keep_up(&mut control, &st, &session, &transport);
            shared.stop.store(true, Ordering::Relaxed);
            set_frame_tap(None);
            crate::audio::set_cast_tap(None);
            for t in threads {
                let _ = t.join();
            }
            match &why {
                Some(w) => eprintln!("slot: cast: ended: {w}"),
                None => eprintln!("slot: cast: stopped"),
            }
            lock(&e).get_or_insert(why.unwrap_or_default());
        };
        let control = spawn("slot-cast", Box::new(run))?;
        Ok(Session {
            stop,
            ended,
            control: Some(control),
        })
    }

    /// Why the cast ended, once it has; empty when it was stopped from here.
    pub fn ended(&self) -> Option<String> {
        lock(&self.ended).clone()
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(c) = self.control.take() {
            let _ = c.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Offer {
    audio: Stream,
    video: Stream,
}

type Agreed = (u16, (u32, u32), bool, Offer);

/// Launches the mirroring app: its session and transport ids.
fn launch(c: &mut Control) -> Result<(String, String), String> {
    c.send(PLATFORM, RECEIVER, NS_CONNECTION, r#"{"type":"CONNECT"}"#)?;
    let id = c.request();
    let app = channel::MIRRORING_APP;
    let launch = format!(r#"{{"type":"LAUNCH","requestId":{id},"appId":"{app}"}}"#);
    c.send(PLATFORM, RECEIVER, NS_RECEIVER, &launch)?;
    c.wait_for(Duration::from_secs(20), |m| {
        let status = m.namespace == NS_RECEIVER && kind(&m.json) == "RECEIVER_STATUS";
        status.then(|| channel::mirroring_app(&m.json)).flatten()
    })
}

/// Stops the mirroring app, so the TV goes back to what it showed before.
fn stop_app(c: &mut Control, session: &str, transport: &str) {
    let id = c.request();
    let stop = format!(r#"{{"type":"STOP","requestId":{id},"sessionId":"{session}"}}"#);
    let me = c.me.clone();
    let _ = c.send(&me, transport, NS_CONNECTION, r#"{"type":"CLOSE"}"#);
    let _ = c.send(PLATFORM, RECEIVER, NS_RECEIVER, &stop);
}

/// Agrees the streams with the launched app: the port to send them to, the picture's size,
/// whether it takes the sound, and the streams.
fn agree(
    c: &mut Control,
    tap: &FrameTap,
    to: &Receiver,
    transport: &str,
) -> Result<Agreed, String> {
    let me = c.me.clone();
    c.send(&me, transport, NS_CONNECTION, r#"{"type":"CONNECT"}"#)?;

    // The picture's size, from a frame of it.
    let size = match tap.take(Duration::from_secs(2)) {
        Some(f) => {
            let size = codec::widescreen(f.w & !1, f.h & !1);
            tap.give_back(f);
            size
        }
        None => (854, 480),
    };
    let (akey, amask, vkey, vmask) = (random(), random(), random(), random());
    let assrc = 1 + u32::from_le_bytes(random()) % 50_000;
    let vssrc = 50_001 + u32::from_le_bytes(random()) % 50_000;
    let id = c.request();
    let offer = format!(
        concat!(
            r#"{{"type":"OFFER","seqNum":{id},"offer":{{"castMode":"mirroring","#,
            r#""supportedStreams":["#,
            r#"{{"index":0,"type":"audio_source","codecName":"opus","rtpProfile":"cast","#,
            r#""rtpPayloadType":{apt},"ssrc":{assrc},"targetDelay":{delay},"aesKey":"{akey}","#,
            r#""aesIvMask":"{amask}","channels":2,"bitRate":{abps},"timeBase":"1/48000","#,
            r#""receiverRtcpEventLog":false}},"#,
            r#"{{"index":1,"type":"video_source","codecName":"h264","rtpProfile":"cast","#,
            r#""rtpPayloadType":{vpt},"ssrc":{vssrc},"targetDelay":{delay},"aesKey":"{vkey}","#,
            r#""aesIvMask":"{vmask}","maxFrameRate":"{fps}/1","maxBitRate":{vbps},"#,
            r#""timeBase":"1/90000","resolutions":[{{"width":{w},"height":{h}}}],"#,
            r#""receiverRtcpEventLog":false}}]}}}}"#
        ),
        id = id,
        apt = AUDIO_PT,
        assrc = assrc,
        delay = DELAY_MS,
        akey = hex(&akey),
        amask = hex(&amask),
        abps = AUDIO_BPS,
        vpt = VIDEO_PT,
        vssrc = vssrc,
        vkey = hex(&vkey),
        vmask = hex(&vmask),
        fps = FPS,
        vbps = VIDEO_KBPS * 1000 * 4 / 3,
        w = size.0,
        h = size.1,
    );
    c.send(&me, transport, NS_WEBRTC, &offer)?;
    let answer = c.wait_for(Duration::from_secs(10), |m| {
        (m.namespace == NS_WEBRTC && kind(&m.json) == "ANSWER").then(|| m.json.clone())
    })?;
    let port = crate::json::number(&answer, "udpPort").filter(|_| {
        crate::json::string(&answer, "result").is_none_or(|r| r == "ok")
    });
    let Some(port) = port else {
        eprintln!("slot: cast: {} said no: {answer}", to.name);
        return Err("the receiver would not take the stream".into());
    };
    let streams = channel::numbers(&answer, "sendIndexes");
    if !streams.contains(&1) {
        return Err("the receiver would not take the picture".into());
    }
    let offer = Offer {
        audio: Stream::new(assrc, AUDIO_PT, akey, amask, 48_000, DELAY_MS),
        video: Stream::new(vssrc, VIDEO_PT, vkey, vmask, 90_000, DELAY_MS),
    };
    Ok((port as u16, size, streams.contains(&0), offer))
}

/// Keeps the control channel up until told to stop or the receiver ends the cast, then stops
/// the app; why it ended, when not from here.
fn keep_up(c: &mut Control, stop: &AtomicBool, session: &str, transport: &str) -> Option<String> {
    let mut pinged = Instant::now();
    let why = loop {
        if stop.load(Ordering::Relaxed) {
            break None;
        }
        if pinged.elapsed() >= Duration::from_secs(5) {
            pinged = Instant::now();
            if let Err(e) = c.send(PLATFORM, RECEIVER, NS_HEARTBEAT, r#"{"type":"PING"}"#) {
                return Some(e);
            }
        }
        let m = match c.recv(Duration::from_millis(200)) {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(e) => return Some(e),
        };
        let closed = m.namespace == NS_CONNECTION && kind(&m.json) == "CLOSE";
        let gone = m.namespace == NS_RECEIVER
            && kind(&m.json) == "RECEIVER_STATUS"
            && channel::mirroring_app(&m.json).is_none_or(|(s, _)| s != session);
        if closed || gone {
            return Some("the TV stopped the cast".into());
        }
    };
    stop_app(c, session, transport);
    why
}

/// How the picture has kept up since last told.
#[derive(Default)]
struct VideoStats {
    frames: u32,
    bytes: usize,
    read: Spread,
    wait: Spread,
    convert: Spread,
    encode: Spread,
    /// From the compositor finishing a frame to its last packet sent.
    age: Spread,
}

#[derive(Default)]
struct Spread {
    total: Duration,
    max: Duration,
}

impl Spread {
    fn add(&mut self, d: Duration) {
        self.total += d;
        self.max = self.max.max(d);
    }

    fn show(&self, n: u32) -> String {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        format!("{:.1}/{:.1} ms", ms(self.total) / f64::from(n.max(1)), ms(self.max))
    }
}

/// Encodes and sends each frame the compositor leaves.
fn video(s: &Shared, tap: &FrameTap, codecs: &Arc<Codecs>, ended: &Mutex<Option<String>>) {
    let mut enc: Option<H264> = None;
    let mut stats = VideoStats::default();
    let (mut since, mut dropped) = (Instant::now(), 0);
    while !s.stop.load(Ordering::Relaxed) {
        if since.elapsed() >= Duration::from_secs(10) {
            let (n, all) = (stats.frames, tap.dropped());
            let secs = since.elapsed().as_secs_f64();
            eprintln!(
                "slot: cast: {:.1} fps, {} dropped, {:.0} kbps; read {}, wait {}, convert {}, \
                 encode {}, finished to sent {} (average/most)",
                f64::from(n) / secs,
                all - dropped,
                stats.bytes as f64 * 8.0 / secs / 1000.0,
                stats.read.show(n),
                stats.wait.show(n),
                stats.convert.show(n),
                stats.encode.show(n),
                stats.age.show(n),
            );
            (stats, since, dropped) = (VideoStats::default(), Instant::now(), all);
        }
        let Some(mut f) = tap.take(Duration::from_millis(100)) else {
            continue;
        };
        let (w, h) = (f.w & !1, f.h & !1);
        if enc.as_ref().is_none_or(|e| (e.w, e.h) != (w, h)) {
            // The old one goes first, not to have two at once.
            drop(enc.take());
            match H264::open(codecs, w, h, FPS, VIDEO_KBPS, VIDEO_THREADS) {
                Ok(e) => enc = Some(e),
                Err(e) => {
                    lock(ended).get_or_insert(e);
                    s.stop.store(true, Ordering::Relaxed);
                    return;
                }
            }
            s.want_key.store(true, Ordering::Relaxed);
        }
        let e = enc.as_mut().expect("opened above");
        let key = s.want_key.swap(false, Ordering::Relaxed);
        let start = Instant::now();
        let stride = f.w;
        let pixels = f.pixels();
        let ready = Instant::now();
        e.fill(pixels, stride);
        let filled = Instant::now();
        if let Some((data, is_key)) = e.encode(key) {
            stats.encode.add(filled.elapsed());
            stats.bytes += data.len();
            let mut v = lock(&s.video);
            for p in v.frame(data, s.rtp(f.at, 90_000), is_key, f.at) {
                s.send(p);
            }
            drop(v);
            stats.frames += 1;
            stats.read.add(f.read);
            stats.wait.add(ready - start);
            stats.convert.add(filled - ready);
            stats.age.add(f.at.elapsed());
        }
        tap.give_back(f);
    }
}

/// Encodes and sends what is played, 20 ms at a time.
fn audio(s: &Shared, codecs: &Arc<Codecs>) {
    let mut enc = match Opus::open(codecs, AUDIO_BPS) {
        Ok(e) => e,
        Err(e) => return eprintln!("slot: cast: {e}"),
    };
    let (tx, rx) = sync_channel(64);
    crate::audio::set_cast_tap(Some(tx));
    let chunk = OPUS_FRAMES * 2;
    let mut pcm: Vec<i16> = Vec::new();
    let mut rtp: Option<u32> = None;
    let (mut sent, mut peak, mut shown) = (0u32, 0u16, Instant::now());
    while !s.stop.load(Ordering::Relaxed) {
        if shown.elapsed() >= Duration::from_secs(10) {
            eprintln!("slot: cast: {sent} sounds, loudest {peak} of 32768");
            (sent, peak, shown) = (0, 0, Instant::now());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(played) => {
                peak = played.iter().fold(peak, |p, s| p.max(s.unsigned_abs()));
                pcm.extend_from_slice(&played);
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
        while pcm.len() >= chunk {
            let now = Instant::now();
            // When the chunk began, as near as can be told: what is still to come is after it.
            let behind = (pcm.len() / 2) as u32;
            let wall = s.rtp(now, 48_000).wrapping_sub(behind);
            // The sound's own count, kept unless it drifts a tenth of a second from the clock.
            let at = rtp
                .filter(|r| (r.wrapping_sub(wall) as i32).unsigned_abs() < 4_800)
                .unwrap_or(wall);
            rtp = Some(at.wrapping_add(OPUS_FRAMES as u32));
            if let Some(packet) = enc.encode(&pcm[..chunk]) {
                let mut a = lock(&s.audio);
                for p in a.frame(packet, at, true, now) {
                    s.send(p);
                }
                sent += 1;
            }
            pcm.drain(..chunk);
        }
    }
}

/// Reads the receiver's feedback, and sends the sender reports.
fn rtcp(s: &Shared) {
    let mut reported = Instant::now();
    let mut buf = [0u8; 1500];
    let (vssrc, assrc) = (lock(&s.video).ssrc, lock(&s.audio).ssrc);
    let (mut resent, mut keys, mut shown, mut heard) = (0, 0, None, None);
    while !s.stop.load(Ordering::Relaxed) {
        if reported.elapsed() >= Duration::from_millis(500) {
            reported = Instant::now();
            for stream in [&s.video, &s.audio] {
                if let Some(r) = lock(stream).sender_report(Instant::now(), SystemTime::now()) {
                    s.send(&r);
                }
            }
        }
        let n = match s.udp.recv(&mut buf) {
            Ok(n) => n,
            // Timed out, or an ICMP refusal from a receiver not listening yet.
            Err(_) => continue,
        };
        for f in parse_rtcp(&buf[..n]) {
            match f {
                Feedback::KeyFrame { ssrc } if ssrc == vssrc => {
                    s.want_key.store(true, Ordering::Relaxed);
                    keys += 1;
                }
                Feedback::Cast {
                    ssrc,
                    checkpoint,
                    lost,
                } if ssrc == vssrc || ssrc == assrc => {
                    let stream = lock(if ssrc == vssrc { &s.video } else { &s.audio });
                    if ssrc == vssrc {
                        shown = stream.expand(checkpoint).or(shown);
                    } else {
                        heard = stream.expand(checkpoint).or(heard);
                    }
                    for l in lost {
                        for p in stream.lost(l.frame, l.packet, l.more) {
                            s.send(p);
                            resent += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    eprintln!(
        "slot: cast: {} frames sent, the TV had all up to {}, {resent} packets sent again, \
         {keys} key frames asked for; {} sounds sent, the TV had all up to {}",
        lock(&s.video).sent(),
        shown.map_or("none".to_string(), |f| f.to_string()),
        lock(&s.audio).sent(),
        heard.map_or("none".to_string(), |f| f.to_string()),
    );
}
