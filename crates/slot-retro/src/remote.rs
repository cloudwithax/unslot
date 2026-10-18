//! A core run in a process of its own, for the cores that draw with the GPU. The Mali's driver
//! puts all of a process's GL work, and its waits for the screen, through one thread, so a core
//! sharing slot's process waits on slot's page flips: Dreamcast held to 30 fps that runs at 90
//! alone. In a second process it has the GPU to itself between slot's frames.
//!
//! The host process runs the core and `serve`s it over a socket; slot holds a `RemoteCore` that
//! is a `RetroCore` like any other. Every call is a request and a reply. A reply carries the
//! core's state along with it (picture, sound, sizes, disks, rumble) so the calls slot makes
//! every frame cost no trip of their own.
//!
//! The picture goes through memory both processes map, handed over with the READY byte, rather
//! than through the socket: a Dreamcast frame is 1.2 MB, and on the socket it was copied four
//! times more on its way to slot.

use std::cell::{Cell, RefCell};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

use crate::core::{AvInfo, ButtonMask, CoreError, CoreOption, RetroCore, Sticks};
use crate::ffi::RUMBLE_STRONG;
use crate::rumble::Rumble;

const LOAD: u8 = 0;
const RUN: u8 = 1;
const OPTION: u8 = 2;
const DISK: u8 = 3;
const SERIALIZE: u8 = 4;
const UNSERIALIZE: u8 = 5;
const SAVE_RAM: u8 = 6;
const LOAD_SAVE_RAM: u8 = 7;
const OPTIONS: u8 = 8;

/// Sent by the host once its core is open, before anything else.
const READY: u8 = 0x5a;

#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f64(&mut self, v: f64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
        self
    }
    fn send(&self, to: &mut UnixStream) -> io::Result<()> {
        self.send_with(to, &[])
    }
    /// As `send`, with `fds` along with its first bytes.
    fn send_with(&self, to: &mut UnixStream, fds: &[OwnedFd]) -> io::Result<()> {
        let len = (self.0.len() as u32).to_le_bytes();
        let sent = match fds {
            [] => 0,
            _ => send_fds(to, &len, fds)?,
        };
        to.write_all(&len[sent..])?;
        to.write_all(&self.0)
    }
}

struct In<'a>(&'a [u8]);

impl<'a> In<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        if self.0.len() < n {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "short message"));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn bytes(&mut self) -> io::Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    fn str(&mut self) -> io::Result<&'a str> {
        std::str::from_utf8(self.bytes()?)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))
    }
}

/// Memory shared with the other process, through a memfd, to pass the picture in. It holds two,
/// put in turn, so the host can put a frame's while slot still copies the one before's.
struct Shm {
    fd: OwnedFd,
    ptr: *mut u8,
    len: usize,
    /// Whether the last picture went in the second half.
    second: bool,
}

impl Shm {
    fn create() -> Option<Shm> {
        // SAFETY: a fresh memfd with a NUL-terminated name.
        let fd = unsafe { libc::memfd_create(c"slot-video".as_ptr(), libc::MFD_CLOEXEC) };
        // SAFETY: a valid fd just made, owned from here on.
        (fd >= 0).then(|| Shm::from(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    fn from(fd: OwnedFd) -> Shm {
        Shm {
            fd,
            ptr: std::ptr::null_mut(),
            len: 0,
            second: false,
        }
    }

    /// Maps `len` bytes of it, if that isn't what is mapped already, growing it first if `grow`.
    fn map(&mut self, len: usize, grow: bool) -> bool {
        if len == self.len {
            return true;
        }
        self.unmap();
        let fd = self.fd.as_raw_fd();
        // SAFETY: our own fd, mapped fresh at whatever address the kernel picks.
        unsafe {
            if grow && libc::ftruncate(fd, len as libc::off_t) != 0 {
                return false;
            }
            let prot = if grow { libc::PROT_WRITE } else { libc::PROT_READ };
            let p = libc::mmap(std::ptr::null_mut(), len, prot, libc::MAP_SHARED, fd, 0);
            if p == libc::MAP_FAILED {
                return false;
            }
            self.ptr = p.cast();
        }
        self.len = len;
        true
    }

    fn unmap(&mut self) {
        if self.len > 0 {
            // SAFETY: the mapping made in `map`, used by nothing after.
            unsafe { libc::munmap(self.ptr.cast(), self.len) };
            self.len = 0;
        }
    }

    /// Copies `data` into the half the last picture didn't go in, growing it to two halves of a
    /// whole number of pages that each hold it if they don't. Where in it `data` went.
    fn put(&mut self, data: &[u8]) -> Option<usize> {
        let half = data.len().next_multiple_of(1 << 16);
        if 2 * half > self.len && !self.map(2 * half, true) {
            return None;
        }
        self.second = !self.second;
        let at = if self.second { half } else { 0 };
        // SAFETY: the mapping is writable and at least `at + data.len()` long.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.add(at), data.len()) };
        Some(at)
    }

    fn get(&self, at: usize, n: usize) -> &[u8] {
        match at + n <= self.len {
            // SAFETY: the mapping is at least `at + n` long and lives as long as `self`.
            true if n > 0 => unsafe { std::slice::from_raw_parts(self.ptr.add(at), n) },
            _ => &[],
        }
    }
}

// SAFETY: the mapping is the Shm's own, and goes with it to whichever thread holds it.
unsafe impl Send for Shm {}

impl Drop for Shm {
    fn drop(&mut self) {
        self.unmap();
    }
}

/// The most fds sent along with one message.
const MAX_FDS: usize = 4;

/// Writes some of `bytes`, at least one, with `fds` along with them. How many went.
fn send_fds(stream: &UnixStream, bytes: &[u8], fds: &[OwnedFd]) -> io::Result<usize> {
    let fds = &fds[..fds.len().min(MAX_FDS)];
    let size = std::mem::size_of_val(fds) as u32;
    // SAFETY: the control buffer is sized by CMSG_SPACE for at most MAX_FDS fds, which are
    // written into it as the raw fds `OwnedFd` holds.
    unsafe {
        let mut iov = libc::iovec {
            iov_base: bytes.as_ptr() as *mut libc::c_void,
            iov_len: bytes.len(),
        };
        let mut ctl = [0u64; 8];
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = ctl.as_mut_ptr().cast();
        msg.msg_controllen = libc::CMSG_SPACE(size) as _;
        let c = libc::CMSG_FIRSTHDR(&msg);
        (*c).cmsg_level = libc::SOL_SOCKET;
        (*c).cmsg_type = libc::SCM_RIGHTS;
        (*c).cmsg_len = libc::CMSG_LEN(size) as _;
        for (i, fd) in fds.iter().enumerate() {
            let at = libc::CMSG_DATA(c).cast::<i32>().add(i);
            std::ptr::write_unaligned(at, fd.as_raw_fd());
        }
        match libc::sendmsg(stream.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) {
            n if n > 0 => Ok(n as usize),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

/// Reads some of `bytes`, at least one, and any fds that come with them into `fds`. How many
/// came.
fn receive_fds(stream: &UnixStream, bytes: &mut [u8], fds: &mut Vec<OwnedFd>) -> io::Result<usize> {
    // SAFETY: as `send_fds`; every fd received is taken into an `OwnedFd`.
    unsafe {
        let mut iov = libc::iovec {
            iov_base: bytes.as_mut_ptr().cast(),
            iov_len: bytes.len(),
        };
        let mut ctl = [0u64; 8];
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = ctl.as_mut_ptr().cast();
        msg.msg_controllen = std::mem::size_of_val(&ctl) as _;
        let n = loop {
            let n = libc::recvmsg(stream.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC);
            if n >= 0 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                break n;
            }
        };
        match n {
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            n if n < 0 => return Err(io::Error::last_os_error()),
            _ => {}
        }
        let c = libc::CMSG_FIRSTHDR(&msg);
        if !c.is_null() && (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS
        {
            let count = ((*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / 4;
            for i in 0..count {
                let fd = std::ptr::read_unaligned(libc::CMSG_DATA(c).cast::<i32>().add(i));
                fds.push(OwnedFd::from_raw_fd(fd));
            }
        }
        Ok(n as usize)
    }
}

/// Writes READY with `fd` along with it.
fn send_ready(stream: &UnixStream, fd: &OwnedFd) -> io::Result<()> {
    send_fds(stream, &[READY], std::slice::from_ref(fd)).map(drop)
}

/// Reads the one byte the host first sends, and the fd that may come with it.
fn receive_ready(stream: &UnixStream) -> io::Result<(u8, Option<OwnedFd>)> {
    let (mut byte, mut fds) = ([0u8], Vec::new());
    receive_fds(stream, &mut byte, &mut fds)?;
    Ok((byte[0], fds.pop()))
}

fn receive(from: &mut UnixStream, buf: &mut Vec<u8>) -> io::Result<()> {
    receive_with(from, buf, &mut Vec::new())
}

/// As `receive`, keeping any fds sent along with the message in `fds`.
fn receive_with(from: &mut UnixStream, buf: &mut Vec<u8>, fds: &mut Vec<OwnedFd>) -> io::Result<()> {
    let mut len = [0u8; 4];
    let got = receive_fds(from, &mut len, fds)?;
    from.read_exact(&mut len[got..])?;
    buf.resize(u32::from_le_bytes(len) as usize, 0);
    from.read_exact(buf)
}

/// What a call came to: Ok with whatever it gives back, or the core's error.
fn put_result(out: &mut Out, r: Result<Vec<u8>, CoreError>) {
    match r {
        Ok(data) => out.u8(0).bytes(&data),
        Err(e) => {
            let (tag, m) = match e {
                CoreError::Io(e) => (1, e.to_string()),
                CoreError::Load(m) => (2, m),
                CoreError::Unsupported(m) => (3, m),
                CoreError::State(m) => (4, m),
            };
            out.u8(tag).bytes(m.as_bytes())
        }
    };
}

fn get_result(i: &mut In) -> io::Result<Result<Vec<u8>, CoreError>> {
    let tag = i.u8()?;
    let data = i.bytes()?;
    let m = || String::from_utf8_lossy(data).into_owned();
    Ok(match tag {
        0 => Ok(data.to_vec()),
        1 => Err(CoreError::Io(io::Error::other(m()))),
        2 => Err(CoreError::Load(m())),
        3 => Err(CoreError::Unsupported(m())),
        _ => Err(CoreError::State(m())),
    })
}

/// Runs `core` for the `RemoteCore` at the other end of `stream` until it goes away.
pub fn serve(mut core: Box<dyn RetroCore>, mut stream: UnixStream) {
    // Without memory to share, the picture goes on the socket as before.
    let mut shm = Shm::create();
    let sent = match &shm {
        Some(s) => send_ready(&stream, &s.fd),
        None => stream.write_all(&[READY]),
    };
    if sent.is_err() {
        return;
    }
    let mut buf = Vec::new();
    // Kept across calls, so a frame's picture isn't copied into a newly grown buffer each time.
    let mut out = Out::default();
    while receive(&mut stream, &mut buf).is_ok() {
        out.0.clear();
        let mut video = false;
        if let Err(e) = answer(core.as_mut(), &buf, &mut out, &mut video) {
            eprintln!("slot: core host: {e}");
            return;
        }
        let fds = put_state(core.as_mut(), &mut out, video, shm.as_mut());
        if out.send_with(&mut stream, &fds).is_err() {
            return;
        }
    }
}

fn answer(core: &mut dyn RetroCore, msg: &[u8], out: &mut Out, video: &mut bool) -> io::Result<()> {
    let mut i = In(msg);
    let r = match i.u8()? {
        LOAD => core.load(Path::new(i.str()?)).map(|()| Vec::new()),
        RUN => {
            let p1 = ButtonMask(i.u16()?);
            let p2 = match i.u8()? {
                1 => Some(ButtonMask(i.u16()?)),
                _ => None,
            };
            let skip = i.u8()? == 1;
            for _ in 0..i.u8()? {
                let port = i.u8()? as usize;
                let mut sticks: Sticks = [0; 4];
                for s in &mut sticks {
                    *s = i.u16()? as i16;
                }
                core.set_sticks(port, sticks);
            }
            core.set_frame_skip(skip);
            match p2 {
                Some(p2) => core.run_frame_linked(p1, p2),
                None => core.run_frame(p1),
            }
            *video = !skip;
            Ok(Vec::new())
        }
        OPTION => {
            core.set_option(i.str()?, i.str()?);
            Ok(Vec::new())
        }
        DISK => core.set_disk(i.u32()?).map(|()| Vec::new()),
        SERIALIZE => core.serialize(),
        UNSERIALIZE => core.unserialize(i.bytes()?).map(|()| Vec::new()),
        SAVE_RAM => Ok(core.save_ram().map_or_else(Vec::new, |mut ram| {
            ram.insert(0, 1);
            ram
        })),
        LOAD_SAVE_RAM => core.load_save_ram(i.bytes()?).map(|()| Vec::new()),
        OPTIONS => {
            let mut list = Out::default();
            let options = core.core_options();
            list.u32(options.len() as u32);
            for o in &options {
                list.bytes(o.key.as_bytes())
                    .bytes(o.label.as_bytes())
                    .bytes(o.value.as_bytes())
                    .u32(o.values.len() as u32);
                for v in &o.values {
                    list.bytes(v.as_bytes());
                }
            }
            Ok(list.0)
        }
        t => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("call {t}"),
            ))
        }
    };
    put_result(out, r);
    Ok(())
}

/// Puts the core's state after a call in `out`, giving the fds to send along with it.
fn put_state(
    core: &mut dyn RetroCore,
    out: &mut Out,
    video: bool,
    shm: Option<&mut Shm>,
) -> Vec<OwnedFd> {
    let (w, h) = core.video_size();
    let av = core.av_info();
    out.u32(w)
        .u32(h)
        .f64(core.display_aspect().map_or(-1.0, f64::from))
        .u32(core.rotation())
        .u32(core.disk_count())
        .u32(core.disk_index())
        .f64(av.fps)
        .f64(av.sample_rate)
        .u16(core.rumble().strength());
    // Left where the GPU put it, the picture isn't copied at all: slot maps the buffers it's in.
    let mut fds = Vec::new();
    if let Some(frame) = video.then(|| core.shared_frame()).flatten() {
        let new = frame.buffers.is_some();
        fds = frame.buffers.unwrap_or_default();
        out.u8(3)
            .u32(w * h * 4)
            .u32(frame.index as u32)
            .u8(u8::from(new));
    } else {
        put_picture(core, out, video, shm);
    }
    let audio = core.take_audio();
    out.u32(audio.len() as u32);
    for s in audio {
        out.u16(s as u16);
    }
    fds
}

fn put_picture(core: &mut dyn RetroCore, out: &mut Out, video: bool, shm: Option<&mut Shm>) {
    let picture = core.video_xrgb8888();
    let shared = shm.and_then(|s| video.then(|| s.put(picture).map(|at| (at, s.len)))?);
    match shared {
        _ if !video => out.u8(0),
        Some((at, len)) => out.u8(2).u32(picture.len() as u32).u32(len as u32).u32(at as u32),
        None => out.u8(1).bytes(picture),
    };
}

/// A core running in a host process, through the socket to it.
pub struct RemoteCore {
    child: Child,
    stream: RefCell<UnixStream>,
    buf: RefCell<Vec<u8>>,
    /// Set once the host is gone, so that is said once and nothing more is asked of it.
    dead: RefCell<bool>,
    sticks: Vec<(u8, Sticks)>,
    skip: bool,
    video: Vec<u8>,
    /// The memory the host puts the picture in, and where in it the last one is and how long, if
    /// it is there rather than in `video`.
    shm: Option<Shm>,
    shm_video: Option<(usize, usize)>,
    /// The buffers the host leaves pictures in where the GPU put them, mapped, and which the
    /// last one is in and how long, when it is in one.
    frames: Vec<Shm>,
    frame: Option<(usize, usize)>,
    /// Fds the host sent along with an answer not yet taken up.
    arrived: RefCell<Vec<OwnedFd>>,
    /// Set while a frame run overlapped hasn't been answered.
    running: Cell<bool>,
    /// That answer, read by a call that couldn't take up the state in it.
    held: RefCell<Option<Vec<u8>>>,
    size: (u32, u32),
    aspect: Option<f32>,
    rotation: u32,
    disks: (u32, u32),
    av: AvInfo,
    rumble: Rumble,
    audio: Vec<i16>,
}

impl RemoteCore {
    /// Waits for the host `child` at the other end of `stream` to say its core is open.
    pub fn new(mut child: Child, stream: UnixStream) -> io::Result<RemoteCore> {
        let shm = match receive_ready(&stream).and_then(|ready| match ready {
            (READY, fd) => Ok(fd.map(Shm::from)),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a core host",
            )),
        }) {
            Ok(shm) => shm,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        Ok(RemoteCore {
            child,
            stream: RefCell::new(stream),
            buf: RefCell::new(Vec::new()),
            dead: RefCell::new(false),
            sticks: Vec::new(),
            skip: false,
            video: Vec::new(),
            shm,
            shm_video: None,
            frames: Vec::new(),
            frame: None,
            arrived: RefCell::new(Vec::new()),
            running: Cell::new(false),
            held: RefCell::new(None),
            size: (0, 0),
            aspect: None,
            rotation: 0,
            disks: (0, 0),
            av: AvInfo {
                fps: 60.0,
                sample_rate: 48_000.0,
            },
            rumble: Rumble::default(),
            audio: Vec::new(),
        })
    }

    /// Does `f` with the stream to the host, unless it is gone or goes, which is said the first
    /// time. Whether it did.
    fn talk(&self, f: impl FnOnce(&mut UnixStream) -> io::Result<()>) -> bool {
        if *self.dead.borrow() {
            return false;
        }
        match f(&mut self.stream.borrow_mut()) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("slot: core host gone: {e}");
                *self.dead.borrow_mut() = true;
                false
            }
        }
    }

    /// Reads the answer to a frame run overlapped, if one is owed, into `held`.
    fn collect(&self, stream: &mut UnixStream) -> io::Result<()> {
        if self.running.replace(false) {
            let mut reply = Vec::new();
            receive_with(stream, &mut reply, &mut self.arrived.borrow_mut())?;
            *self.held.borrow_mut() = Some(reply);
        }
        Ok(())
    }

    /// Takes up the state from the answer to a frame run overlapped, waiting for it if need be.
    fn settle(&mut self) {
        self.talk(|stream| self.collect(stream));
        if let Some(reply) = self.held.get_mut().take() {
            let mut i = In(&reply);
            let _ = get_result(&mut i).and_then(|_| self.take_state(&mut i));
        }
    }

    /// Sends `msg` and reads the reply into `buf`, or says why not, the first time.
    fn call(&self, msg: &Out) -> Option<std::cell::RefMut<'_, Vec<u8>>> {
        let mut buf = self.buf.borrow_mut();
        self.talk(|stream| {
            self.collect(stream)?;
            msg.send(stream)?;
            receive_with(stream, &mut buf, &mut self.arrived.borrow_mut())
        })
        .then_some(buf)
    }

    /// A call that changes the core, taking up the state that comes back with it.
    fn call_mut(&mut self, msg: &Out) -> Result<Vec<u8>, CoreError> {
        self.settle();
        let gone = || CoreError::Io(io::Error::other("the core host is gone"));
        // Taken rather than cloned, and put back after, so its room is used again.
        let reply = std::mem::take(&mut *self.call(msg).ok_or_else(gone)?);
        let mut i = In(&reply);
        let r = get_result(&mut i).and_then(|r| self.take_state(&mut i).map(|()| r));
        *self.buf.borrow_mut() = reply;
        r.unwrap_or_else(|e| Err(CoreError::Io(e)))
    }

    /// A call that only asks, leaving the state slot keeps alone.
    fn call_ref(&self, msg: &Out) -> Result<Vec<u8>, CoreError> {
        let gone = || CoreError::Io(io::Error::other("the core host is gone"));
        let reply = self.call(msg).ok_or_else(gone)?;
        get_result(&mut In(&reply)).unwrap_or_else(|e| Err(CoreError::Io(e)))
    }

    fn take_state(&mut self, i: &mut In) -> io::Result<()> {
        self.size = (i.u32()?, i.u32()?);
        let aspect = i.f64()?;
        self.aspect = (aspect >= 0.0).then_some(aspect as f32);
        self.rotation = i.u32()?;
        self.disks = (i.u32()?, i.u32()?);
        self.av = AvInfo {
            fps: i.f64()?,
            sample_rate: i.f64()?,
        };
        self.rumble.set(0, RUMBLE_STRONG, i.u16()?);
        match i.u8()? {
            1 => {
                let video = i.bytes()?;
                self.video.clear();
                self.video.extend_from_slice(video);
                self.shm_video = None;
                self.frame = None;
            }
            2 => {
                let (n, len, at) = (i.u32()? as usize, i.u32()? as usize, i.u32()? as usize);
                let mapped = self.shm.as_mut().is_some_and(|s| s.map(len, false));
                if !mapped || at + n > len {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "no picture"));
                }
                self.shm_video = Some((at, n));
                self.frame = None;
            }
            3 => {
                let (n, index, new) = (i.u32()? as usize, i.u32()? as usize, i.u8()? == 1);
                if new {
                    let fds = std::mem::take(self.arrived.get_mut());
                    self.frames = fds.into_iter().map(Shm::from).collect();
                }
                let mapped = self.frames.get_mut(index).is_some_and(|s| s.map(n, false));
                if !mapped {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "no picture"));
                }
                self.frame = Some((index, n));
            }
            _ => {}
        }
        let n = i.u32()? as usize;
        let samples = i.take(n * 2)?;
        self.audio.extend(
            samples
                .chunks_exact(2)
                .map(|s| i16::from_le_bytes([s[0], s[1]])),
        );
        Ok(())
    }

    fn run(&mut self, p1: ButtonMask, p2: Option<ButtonMask>, overlapped: bool) {
        let mut msg = Out::default();
        msg.u8(RUN).u16(p1.0);
        match p2 {
            Some(p2) => msg.u8(1).u16(p2.0),
            None => msg.u8(0),
        };
        msg.u8(u8::from(self.skip)).u8(self.sticks.len() as u8);
        for (port, sticks) in &self.sticks {
            msg.u8(*port);
            for s in sticks {
                msg.u16(*s as u16);
            }
        }
        if !overlapped {
            let _ = self.call_mut(&msg);
            return;
        }
        // The frame before's answer is taken up, and this one is left to come while slot shows it.
        self.settle();
        if self.talk(|stream| msg.send(stream)) {
            self.running.set(true);
        }
    }
}

impl RetroCore for RemoteCore {
    fn load(&mut self, rom: &Path) -> Result<(), CoreError> {
        let rom = rom
            .to_str()
            .ok_or_else(|| CoreError::Load(format!("{}: not UTF-8", rom.display())))?;
        self.call_mut(Out::default().u8(LOAD).bytes(rom.as_bytes()))
            .map(drop)
    }

    fn run_frame(&mut self, input: ButtonMask) {
        self.run(input, None, false);
    }

    fn run_frame_overlapped(&mut self, input: ButtonMask) {
        self.run(input, None, true);
    }

    fn run_frame_linked(&mut self, p1: ButtonMask, p2: ButtonMask) {
        self.run(p1, Some(p2), false);
    }

    fn set_option(&mut self, key: &str, value: &str) {
        let _ = self.call_mut(
            Out::default()
                .u8(OPTION)
                .bytes(key.as_bytes())
                .bytes(value.as_bytes()),
        );
    }

    fn core_options(&self) -> Vec<CoreOption> {
        let Ok(reply) = self.call_ref(Out::default().u8(OPTIONS)) else {
            return Vec::new();
        };
        let read = |i: &mut In| -> io::Result<Vec<CoreOption>> {
            let mut options = Vec::new();
            for _ in 0..i.u32()? {
                let (key, label, value) = (i.str()?.into(), i.str()?.into(), i.str()?.into());
                let mut values = Vec::new();
                for _ in 0..i.u32()? {
                    values.push(i.str()?.into());
                }
                options.push(CoreOption {
                    key,
                    label,
                    values,
                    value,
                });
            }
            Ok(options)
        };
        read(&mut In(&reply)).unwrap_or_default()
    }

    fn set_frame_skip(&mut self, skip: bool) {
        self.skip = skip;
    }

    fn video_xrgb8888(&self) -> &[u8] {
        if let Some((index, n)) = self.frame {
            return self.frames[index].get(0, n);
        }
        match (&self.shm, self.shm_video) {
            (Some(shm), Some((at, n))) => shm.get(at, n),
            _ => &self.video,
        }
    }

    fn video_size(&self) -> (u32, u32) {
        self.size
    }

    fn display_aspect(&self) -> Option<f32> {
        self.aspect
    }

    fn rotation(&self) -> u32 {
        self.rotation
    }

    fn set_sticks(&mut self, port: usize, sticks: Sticks) {
        let port = port as u8;
        match self.sticks.iter_mut().find(|(p, _)| *p == port) {
            Some(held) => held.1 = sticks,
            None => self.sticks.push((port, sticks)),
        }
    }

    fn disk_count(&self) -> u32 {
        self.disks.0
    }

    fn disk_index(&self) -> u32 {
        self.disks.1
    }

    fn set_disk(&mut self, index: u32) -> Result<(), CoreError> {
        self.call_mut(Out::default().u8(DISK).u32(index)).map(drop)
    }

    fn take_audio(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.audio)
    }

    fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
        self.call_mut(Out::default().u8(SERIALIZE))
    }

    fn unserialize(&mut self, data: &[u8]) -> Result<(), CoreError> {
        self.call_mut(Out::default().u8(UNSERIALIZE).bytes(data))
            .map(drop)
    }

    fn save_ram(&self) -> Option<Vec<u8>> {
        let mut ram = self.call_ref(Out::default().u8(SAVE_RAM)).ok()?;
        match ram.first() {
            Some(1) => {
                ram.remove(0);
                Some(ram)
            }
            _ => None,
        }
    }

    fn load_save_ram(&mut self, data: &[u8]) -> Result<(), CoreError> {
        self.call_mut(Out::default().u8(LOAD_SAVE_RAM).bytes(data))
            .map(drop)
    }

    fn av_info(&self) -> AvInfo {
        self.av
    }

    fn rumble(&self) -> Rumble {
        self.rumble.clone()
    }
}

impl Drop for RemoteCore {
    /// Hangs up, which ends the host, and gives it a second to unload its core before it is
    /// killed.
    fn drop(&mut self) {
        let _ = self.stream.borrow().shutdown(std::net::Shutdown::Both);
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::SharedFrame;
    use crate::mock::MockCore;

    fn pair() -> RemoteCore {
        pair_with(Box::new(MockCore::new()))
    }

    fn pair_with(core: Box<dyn RetroCore>) -> RemoteCore {
        let (ours, theirs) = UnixStream::pair().unwrap();
        std::thread::spawn(move || serve(core, theirs));
        let child = std::process::Command::new("true").spawn().unwrap();
        RemoteCore::new(child, ours).unwrap()
    }

    /// The mock, leaving each picture in the next of three memfds as a GPU core leaves its
    /// frames in buffers, and nothing in `video_xrgb8888`.
    struct Sharing {
        core: MockCore,
        buffers: Vec<std::fs::File>,
        next: usize,
        handed: bool,
    }

    impl Sharing {
        fn new() -> Sharing {
            let buffers = (0..3)
                .map(|_| {
                    // SAFETY: a fresh memfd with a NUL-terminated name, owned from here on.
                    let fd = unsafe { libc::memfd_create(c"frame".as_ptr(), 0) };
                    assert!(fd >= 0);
                    std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) })
                })
                .collect();
            Sharing {
                core: MockCore::new(),
                buffers,
                next: 0,
                handed: false,
            }
        }
    }

    impl RetroCore for Sharing {
        fn load(&mut self, rom: &Path) -> Result<(), CoreError> {
            self.core.load(rom)
        }
        fn run_frame(&mut self, input: ButtonMask) {
            self.core.run_frame(input);
        }
        fn video_xrgb8888(&self) -> &[u8] {
            &[]
        }
        fn shared_frame(&mut self) -> Option<SharedFrame> {
            use std::os::unix::fs::FileExt;
            let index = self.next;
            self.next = (self.next + 1) % 3;
            let b = &self.buffers[index];
            b.write_all_at(self.core.video_xrgb8888(), 0).unwrap();
            let buffers = (!std::mem::replace(&mut self.handed, true)).then(|| {
                self.buffers
                    .iter()
                    .map(|b| OwnedFd::from(b.try_clone().unwrap()))
                    .collect()
            });
            Some(SharedFrame { index, buffers })
        }
        fn video_size(&self) -> (u32, u32) {
            self.core.video_size()
        }
        fn take_audio(&mut self) -> Vec<i16> {
            self.core.take_audio()
        }
        fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
            self.core.serialize()
        }
        fn unserialize(&mut self, data: &[u8]) -> Result<(), CoreError> {
            self.core.unserialize(data)
        }
        fn save_ram(&self) -> Option<Vec<u8>> {
            self.core.save_ram()
        }
        fn load_save_ram(&mut self, data: &[u8]) -> Result<(), CoreError> {
            self.core.load_save_ram(data)
        }
        fn av_info(&self) -> AvInfo {
            self.core.av_info()
        }
    }

    #[test]
    fn a_remote_core_gives_what_the_core_does() {
        let mut local = MockCore::new();
        let mut remote = pair();
        for core in [&mut local as &mut dyn RetroCore, &mut remote] {
            core.run_frame(ButtonMask(ButtonMask::A));
            core.run_frame(ButtonMask::default());
        }
        assert_eq!(remote.video_size(), local.video_size());
        assert_eq!(remote.video_xrgb8888(), local.video_xrgb8888());
        assert!(remote.shm_video.is_some(), "the picture came through shared memory");
        assert_eq!(remote.take_audio(), local.take_audio());
        assert_eq!(remote.av_info(), local.av_info());
        let state = remote.serialize().unwrap();
        assert_eq!(state, local.serialize().unwrap());
        remote.unserialize(&state).unwrap();
        assert_eq!(remote.save_ram(), local.save_ram());
    }

    #[test]
    fn an_overlapped_frame_shows_the_one_before_until_the_next_call() {
        let mut local = MockCore::new();
        let mut remote = pair();
        let mut shown = Vec::new();
        for _ in 0..3 {
            shown.push(local.video_xrgb8888().to_vec());
            local.run_frame(ButtonMask::default());
            remote.run_frame_overlapped(ButtonMask::default());
        }
        assert_eq!(remote.video_xrgb8888(), shown[2], "the frame before's picture");
        assert_ne!(shown[1], shown[2]);
        // A call that can't take up the state reads the answer owed and keeps it for the next.
        assert_eq!(remote.save_ram(), local.save_ram());
        let state = remote.serialize().unwrap();
        assert_eq!(state, local.serialize().unwrap(), "every frame ran before the call");
        assert_eq!(remote.video_xrgb8888(), local.video_xrgb8888());
        assert_eq!(remote.take_audio(), local.take_audio());
    }

    #[test]
    fn a_picture_left_in_a_shared_buffer_is_read_from_it() {
        let mut local = MockCore::new();
        let mut remote = pair_with(Box::new(Sharing::new()));
        for _ in 0..4 {
            local.run_frame(ButtonMask::default());
            remote.run_frame(ButtonMask::default());
            assert_eq!(remote.video_xrgb8888(), local.video_xrgb8888());
        }
        assert!(remote.frame.is_some(), "the picture came from the buffers");
        assert_eq!(remote.frames.len(), 3);
        // Overlapped, each frame's picture still comes from the buffer it was left in.
        let mut shown = Vec::new();
        for _ in 0..4 {
            shown.push(local.video_xrgb8888().to_vec());
            local.run_frame(ButtonMask::default());
            remote.run_frame_overlapped(ButtonMask::default());
        }
        assert_eq!(remote.video_xrgb8888(), shown[3]);
        assert_eq!(remote.serialize().unwrap(), local.serialize().unwrap());
        assert_eq!(remote.video_xrgb8888(), local.video_xrgb8888());
    }

    #[test]
    fn a_skipped_frame_keeps_the_last_picture() {
        let mut remote = pair();
        remote.run_frame(ButtonMask::default());
        let shown = remote.video_xrgb8888().to_vec();
        remote.set_frame_skip(true);
        remote.run_frame(ButtonMask::default());
        assert_eq!(remote.video_xrgb8888(), shown);
    }

    #[test]
    fn a_core_host_gone_leaves_errors_not_a_hang() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let child = std::process::Command::new("true").spawn().unwrap();
        let host = std::thread::spawn(move || {
            let mut theirs = theirs;
            theirs.write_all(&[READY]).unwrap();
        });
        let mut remote = RemoteCore::new(child, ours).unwrap();
        host.join().unwrap();
        remote.run_frame(ButtonMask::default());
        assert!(remote.serialize().is_err());
        assert!(remote.save_ram().is_none());
    }
}
