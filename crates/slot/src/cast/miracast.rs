//! A Miracast in progress: the TV's Wi-Fi Direct group joined, its RTSP answered as a Wi-Fi
//! Display source, then the screen and the sound sent it in one transport stream over RTP.
//!
//! The control thread owns the radio and the RTSP connection. Beside it, one thread encodes and
//! sends the picture and one the sound, through the one muxer.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use slot_gfx::cast_tap::{set_frame_tap, FrameTap};

use super::codec::{Aac, Codecs, AAC_FRAMES, H264};
use super::mdns::{Addr, Receiver};
use super::p2p::{Group, Radio};
use super::session::random;
use super::ts::{self, Es, Mux, Sound};
use super::wfd::{self, Agreed, Heard, Mode, Rtsp};

const VIDEO_KBPS: u32 = 6000;
/// Threads x264 cuts each frame between, for it to be done well inside 60 a second's 16 ms.
const VIDEO_THREADS: u32 = 3;
const AAC_BPS: u32 = 192_000;
/// How far ahead of the clock each frame and sound is to be shown, for the TV to buffer.
const DELAY: Duration = Duration::from_millis(100);
/// A key frame this often, nothing being sent again that is lost.
const KEY_EVERY: Duration = Duration::from_secs(2);
/// The access unit delimiter put before each frame, which some TVs need to find them.
const AUD: [u8; 6] = [0, 0, 0, 1, 0x09, 0xf0];
/// How long the TV has to connect once in its group, and to agree the stream once connected.
const CONNECT_WAIT: Duration = Duration::from_secs(20);
const AGREE_WAIT: Duration = Duration::from_secs(15);

/// What the media threads share.
struct Shared {
    udp: UdpSocket,
    mux: Mutex<Mux>,
    want_key: AtomicBool,
    stop: AtomicBool,
    /// The stream's time 0.
    t0: Instant,
}

impl Shared {
    fn send(&self, es: Es, unit: &[u8], at: Instant) {
        let pts = at.saturating_duration_since(self.t0) + DELAY;
        let now = self.t0.elapsed();
        for p in lock(&self.mux).packets(es, unit, pts, now) {
            // A packet the network would not take is lost; the next key frame mends it.
            let _ = self.udp.send(&p);
        }
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
    /// Casts to the Miracast TV `to` with the encoders in `lib`, from `root`'s card, returning
    /// once it plays.
    pub fn start(root: &Path, lib: &Path, to: &Receiver) -> Result<Session, String> {
        let Addr::Miracast(mac) = &to.addr else {
            return Err("not a Miracast TV".into());
        };
        let codecs = Codecs::load(lib)?;
        // Listening before joining, for a TV that connects the moment it gives an address.
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, wfd::PORT))
            .and_then(|l| l.set_nonblocking(true).map(|()| l))
            .map_err(|e| format!("port {}: {e}", wfd::PORT))?;
        let radio = Radio::up(root)?;
        let group = radio.join(mac)?;
        let begun = begin(&listener, &group).and_then(|(rtsp, agreed, udp)| {
            let tap = FrameTap::new(agreed.mode.fps);
            set_frame_tap(Some(tap.clone()));
            spawn(to, radio, group, rtsp, agreed, udp, codecs, tap)
        });
        if begun.is_err() {
            set_frame_tap(None);
            crate::audio::set_cast_tap(None);
        }
        begun
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

/// Waits for the TV in `group` to connect, and agrees the stream with it.
fn begin(
    listener: &TcpListener,
    group: &Group,
) -> Result<(Rtsp<TcpStream>, Agreed, UdpSocket), String> {
    let end = Instant::now() + CONNECT_WAIT;
    let tcp = loop {
        match listener.accept() {
            // Only from the TV's group, not the home network.
            Ok((s, _)) if s.local_addr().is_ok_and(|a| a.ip() == group.ip) => break s,
            Ok(_) => {}
            Err(_) if Instant::now() >= end => return Err("the TV did not connect".into()),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let tv = tcp.peer_addr().map_err(|e| e.to_string())?.ip();
    tcp.set_nonblocking(false)
        .and_then(|()| tcp.set_read_timeout(Some(Duration::from_millis(200))))
        .map_err(|e| e.to_string())?;
    let udp = UdpSocket::bind((group.ip, 0)).map_err(|e| e.to_string())?;
    let port = udp.local_addr().map_err(|e| e.to_string())?.port();
    let mut rtsp = Rtsp::new(tcp, u32::from_le_bytes(random()) & 0x7fff_ffff);
    let agreed = rtsp.negotiate(group.ip, port, AGREE_WAIT)?;
    udp.connect(SocketAddr::new(tv, agreed.rtp_port))
        .map_err(|e| e.to_string())?;
    Ok((rtsp, agreed, udp))
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    to: &Receiver,
    radio: Radio,
    group: Group,
    mut rtsp: Rtsp<TcpStream>,
    agreed: Agreed,
    udp: UdpSocket,
    codecs: Arc<Codecs>,
    tap: Arc<FrameTap>,
) -> Result<Session, String> {
    let mode = agreed.mode;
    eprintln!(
        "slot: cast: to {} over Miracast, {}x{}, port {}{}",
        to.name,
        mode.w,
        mode.h,
        agreed.rtp_port,
        match agreed.audio {
            Some(Sound::Aac) => ", sound as AAC",
            Some(Sound::Lpcm) => ", sound as LPCM",
            None => ", no sound",
        }
    );
    let shared = Arc::new(Shared {
        udp,
        mux: Mutex::new(Mux::new(u32::from_le_bytes(random()), agreed.audio)),
        want_key: AtomicBool::new(true),
        stop: AtomicBool::new(false),
        t0: Instant::now(),
    });
    let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
        std::thread::Builder::new()
            .name(name.into())
            .spawn(f)
            .map_err(|e| e.to_string())
    };
    let ended = Arc::new(Mutex::new(None));
    let mut threads = Vec::new();
    if let Some(sound) = agreed.audio {
        let (s, c) = (shared.clone(), codecs.clone());
        threads.push(spawn(
            "slot-cast-audio",
            Box::new(move || audio(&s, sound, &c)),
        )?);
    }
    let (s, e) = (shared.clone(), ended.clone());
    threads.push(spawn(
        "slot-cast-video",
        Box::new(move || video(&s, &tap, &codecs, mode, &e)),
    )?);
    let stop = Arc::new(AtomicBool::new(false));
    let (st, e) = (stop.clone(), ended.clone());
    let run = move || {
        let why = keep_up(&mut rtsp, &radio, &st, &shared);
        shared.stop.store(true, Ordering::Relaxed);
        set_frame_tap(None);
        crate::audio::set_cast_tap(None);
        for t in threads {
            let _ = t.join();
        }
        radio.leave(&group);
        drop(radio);
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

/// Answers the TV until told to stop or it ends the cast, then ends the session; why it ended,
/// when not from here.
fn keep_up(
    rtsp: &mut Rtsp<TcpStream>,
    radio: &Radio,
    stop: &AtomicBool,
    s: &Shared,
) -> Option<String> {
    while !stop.load(Ordering::Relaxed) {
        if s.stop.load(Ordering::Relaxed) {
            return Some("the picture could not be sent".into());
        }
        if radio.gone() {
            return Some("the TV left".into());
        }
        match rtsp.tend() {
            Ok(Heard::Nothing) => {}
            Ok(Heard::KeyFrame) => s.want_key.store(true, Ordering::Relaxed),
            Ok(Heard::Teardown) => return Some("the TV stopped the cast".into()),
            Err(e) => return Some(e),
        }
    }
    rtsp.teardown();
    None
}

/// Encodes and sends each frame the compositor leaves, fitted to `mode`.
fn video(
    s: &Shared,
    tap: &FrameTap,
    codecs: &Arc<Codecs>,
    mode: Mode,
    ended: &Mutex<Option<String>>,
) {
    let mut enc: Option<H264> = None;
    let mut keyed = Instant::now();
    let (mut frames, mut bytes, mut since) = (0u32, 0usize, Instant::now());
    let (mut convert, mut encode, mut dropped) = (Duration::ZERO, Duration::ZERO, 0);
    let mut unit = Vec::new();
    while !s.stop.load(Ordering::Relaxed) {
        if since.elapsed() >= Duration::from_secs(10) {
            let secs = since.elapsed().as_secs_f64();
            let ms = |d: Duration| d.as_secs_f64() * 1000.0 / f64::from(frames.max(1));
            eprintln!(
                "slot: cast: {:.1} fps, {} dropped, {:.0} kbps; convert {:.1} ms, encode {:.1} ms",
                f64::from(frames) / secs,
                tap.dropped() - dropped,
                bytes as f64 * 8.0 / secs / 1000.0,
                ms(convert),
                ms(encode),
            );
            (frames, bytes, since, dropped) = (0, 0, Instant::now(), tap.dropped());
            (convert, encode) = (Duration::ZERO, Duration::ZERO);
        }
        let Some(mut f) = tap.take(Duration::from_millis(100)) else {
            continue;
        };
        let (w, h) = (f.w & !1, f.h & !1);
        if enc.as_ref().is_none_or(|e| (e.w, e.h) != (w, h)) {
            drop(enc.take());
            let size = (mode.w, mode.h);
            match H264::open_at(codecs, w, h, size, mode.fps, VIDEO_KBPS, VIDEO_THREADS) {
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
        let key = s.want_key.swap(false, Ordering::Relaxed) || keyed.elapsed() >= KEY_EVERY;
        let stride = f.w;
        let start = Instant::now();
        e.fill(f.pixels(), stride);
        let filled = Instant::now();
        convert += filled - start;
        if let Some((data, is_key)) = e.encode(key) {
            encode += filled.elapsed();
            if is_key {
                keyed = Instant::now();
            }
            unit.clear();
            unit.extend_from_slice(&AUD);
            unit.extend_from_slice(data);
            s.send(Es::Video, &unit, f.at);
            frames += 1;
            bytes += unit.len();
        }
        tap.give_back(f);
    }
}

/// Sends what is played: as AAC, 21 ms to a frame, or as it is, 10 ms at a time.
fn audio(s: &Shared, sound: Sound, codecs: &Arc<Codecs>) {
    let mut aac = match sound {
        Sound::Aac => match Aac::open(codecs, AAC_BPS) {
            Ok(a) => Some(a),
            Err(e) => return eprintln!("slot: cast: {e}"),
        },
        Sound::Lpcm => None,
    };
    let (tx, rx) = sync_channel(64);
    crate::audio::set_cast_tap(Some(tx));
    let unit = if aac.is_some() {
        AAC_FRAMES
    } else {
        ts::LPCM_FRAMES
    };
    let chunk = unit * 2;
    let mut pcm: Vec<i16> = Vec::new();
    let mut at: Option<Instant> = None;
    // When each chunk the encoder holds began, its frames coming out some calls behind.
    let mut held = VecDeque::new();
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
            // When the chunk began, as near as can be told: what is still to come is after it.
            let wall = Instant::now() - frames((pcm.len() / 2) as u64);
            // The sound's own count, kept unless it drifts a tenth of a second from the clock.
            let start = at
                .filter(|a| {
                    a.duration_since(wall).max(wall.duration_since(*a)) < Duration::from_millis(100)
                })
                .unwrap_or(wall);
            at = Some(start + frames(unit as u64));
            match &mut aac {
                Some(a) => {
                    held.push_back(start);
                    if let Some(frame) = a.encode(&pcm[..chunk]) {
                        let start = held.pop_front().unwrap_or(start);
                        s.send(Es::Audio, frame, start);
                        sent += 1;
                    }
                }
                None => {
                    s.send(Es::Audio, &ts::lpcm(&pcm[..chunk]), start);
                    sent += 1;
                }
            }
            pcm.drain(..chunk);
        }
    }
}

/// How long `n` frames of 48 kHz sound last.
fn frames(n: u64) -> Duration {
    Duration::from_micros(n * 1_000_000 / 48_000)
}
