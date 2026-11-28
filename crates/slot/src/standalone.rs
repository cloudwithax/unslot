//! The standalone emulators, DraStic, PPSSPP-SDL and the owner's PICO-8, driven as slot drives a
//! core of its own: slot
//! starts the emulator's launch script with UNSLOT_BRIDGE naming memory and a socket, and the shim
//! preloaded into it (tools/bridge/bridge.h) hands over each frame's picture and sound there, and
//! takes the pad, saves and loads from it. Nothing of the emulator's reaches the screen, so slot's
//! in-game menu, shaders, bezels and casting are all over it as over any game.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime};

use slot_retro::{AvInfo, ButtonMask, CoreError, RetroCore, Sticks};

const MAX_PIXELS: usize = 2048 * 1088;
const MAX_SAMPLES: usize = 8192;

/// The memory the shim and slot share, as tools/bridge/bridge.h lays it out.
#[repr(C)]
struct Bridge {
    buttons: u32,
    sticks: [i16; 4],
    width: u32,
    height: u32,
    rate: u32,
    samples: u32,
    audio: [i16; 2 * MAX_SAMPLES],
    frame: [u32; MAX_PIXELS],
}

/// How long the emulator may take to show its first frame, and any after.
const STARTING: Duration = Duration::from_secs(60);
const RUNNING: Duration = Duration::from_secs(6);
/// How long it has to save the game and close once asked, before it is killed.
const CLOSING: Duration = Duration::from_secs(5);
/// How long a state may take to be written once asked for, and how often its key is pressed
/// again meanwhile while slot does not yet know the state's name, as a press before the game has
/// started saves nothing.
const SAVING: Duration = Duration::from_secs(20);
const PRESS_AGAIN: Duration = Duration::from_millis(1500);

/// RAM-backed scratch directories for PPSSPP and DraStic savestate exchange so save/load hotkeys
/// and `--state` do not spend ~1.5–2.3 s writing multi-megabyte state files synchronously to FAT32
/// SD cards. Persistent states still live under `<root>/States/` via slot's own state writer.
const PPSSPP_TMP_STATES: &str = "/tmp/unslot-ppsspp-states";
const DRASTIC_TMP_STATES: &str = "/tmp/unslot-drastic-states";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Emulator {
    Drastic,
    Ppsspp,
    Pico8,
}

impl Emulator {
    fn script(self) -> &'static str {
        match self {
            Emulator::Drastic => crate::apps::ports::DRASTIC,
            Emulator::Ppsspp => crate::apps::ports::PPSSPP,
            Emulator::Pico8 => crate::apps::ports::PICO8,
        }
    }

    fn fps(self) -> f64 {
        match self {
            Emulator::Drastic => 59.8261,
            Emulator::Ppsspp => 59.94,
            Emulator::Pico8 => 60.0,
        }
    }

    /// Where the emulator keeps its scratch states for save/load exchange, and how the name of a
    /// game's first ends; PICO-8 keeps none.
    fn states(self, root: &Path) -> Option<(PathBuf, &'static str)> {
        match self {
            Emulator::Drastic => {
                let tmp = PathBuf::from(DRASTIC_TMP_STATES);
                if std::fs::create_dir_all(&tmp).is_ok() {
                    Some((tmp, "_0.dss"))
                } else {
                    Some((root.join("Saves/NDS/savestates"), "_0.dss"))
                }
            }
            Emulator::Ppsspp => {
                let tmp = PathBuf::from(PPSSPP_TMP_STATES);
                if std::fs::create_dir_all(&tmp).is_ok() {
                    Some((tmp, "_0.ppst"))
                } else {
                    Some((root.join("States/PSP"), "_0.ppst"))
                }
            }
            Emulator::Pico8 => None,
        }
    }

    /// Where the emulator's save and load keys leave its state for `rom`, if slot can tell:
    /// DraStic names it after the file, PPSSPP after the game's disc, which slot learns from the
    /// first save and keeps beside the persistent states on the card.
    fn state_file(self, root: &Path, rom: &Path) -> Option<PathBuf> {
        let stem = rom.file_stem().unwrap_or_default().to_string_lossy();
        let (dir, end) = self.states(root)?;
        match self {
            Emulator::Drastic => Some(dir.join(format!("{stem}{end}"))),
            _ => {
                let name = std::fs::read_to_string(self.learned(root, rom)?).ok()?;
                Some(dir.join(name.trim()))
            }
        }
    }

    /// Where the name of `rom`'s state is kept once learned (persisted on the card).
    fn learned(self, root: &Path, rom: &Path) -> Option<PathBuf> {
        let stem = rom.file_stem().unwrap_or_default().to_string_lossy();
        let dir = match self {
            Emulator::Ppsspp => root.join("States/PSP"),
            _ => self.states(root)?.0,
        };
        Some(dir.join(format!(".{stem}.unslot")))
    }
}

pub struct StandaloneCore {
    emulator: Emulator,
    root: PathBuf,
    rom: PathBuf,
    /// Where the game's state is saved and loaded, once known.
    state: Option<PathBuf>,
    /// Pre-staged state file passed to PPSSPP via `--state=` on launch.
    preload_state: Option<PathBuf>,
    child: Option<Child>,
    sock: Option<UnixStream>,
    mem: *mut Bridge,
    /// The memory's descriptor, until the emulator is started with it.
    mem_fd: Option<OwnedFd>,
    gone: bool,
}

// SAFETY: the memory is only touched from the thread that owns the core.
unsafe impl Send for StandaloneCore {}

impl StandaloneCore {
    pub fn new(emulator: Emulator, root: &Path) -> std::io::Result<Self> {
        let size = std::mem::size_of::<Bridge>();
        // SAFETY: a new memfd, sized and mapped as the shim maps it. Through the syscall, as glibc
        // only wraps it from 2.27 and TrimUI's firmware has 2.23.
        let (mem, fd) = unsafe {
            let fd =
                libc::syscall(libc::SYS_memfd_create, c"unslot-bridge".as_ptr(), 0) as libc::c_int;
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let fd = OwnedFd::from_raw_fd(fd);
            if libc::ftruncate(fd.as_raw_fd(), size as libc::off_t) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let p = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            );
            if p == libc::MAP_FAILED {
                return Err(std::io::Error::last_os_error());
            }
            (p as *mut Bridge, fd)
        };
        Ok(StandaloneCore {
            emulator,
            root: root.to_path_buf(),
            rom: PathBuf::new(),
            state: None,
            preload_state: None,
            child: None,
            sock: None,
            mem,
            mem_fd: Some(fd),
            gone: false,
        })
    }

    fn bridge(&self) -> &Bridge {
        // SAFETY: mapped for the life of the core; the shim only writes it while slot waits.
        unsafe { &*self.mem }
    }

    fn bridge_mut(&mut self) -> &mut Bridge {
        // SAFETY: as above.
        unsafe { &mut *self.mem }
    }

    /// Asks the shim `op` and waits for its answer, which is `want`, as long as `patience`.
    fn ask(&mut self, op: u8, want: u8, patience: Duration) -> Result<(), CoreError> {
        let Some(sock) = self.sock.as_mut() else {
            return Err(CoreError::State("the emulator is gone".into()));
        };
        let _ = sock.set_read_timeout(Some(patience));
        let mut answer = [0u8];
        let asked = sock
            .write_all(&[op])
            .and_then(|()| sock.read_exact(&mut answer));
        match asked {
            Ok(()) if answer[0] == want => Ok(()),
            Ok(()) => Err(CoreError::State(format!(
                "the emulator answered {:?} to {:?}",
                answer[0] as char, op as char
            ))),
            Err(e) => {
                eprintln!("slot: standalone: {:?}: {e}", op as char);
                self.gone = true;
                self.sock = None;
                Err(e.into())
            }
        }
    }

    /// The games' first states in the emulator's folder, with when each was written and its size.
    fn written(&self) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
        let Some((dir, end)) = self.emulator.states(&self.root) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(end))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((e.path(), meta.modified().ok(), meta.len()))
            })
            .collect()
    }
}

impl RetroCore for StandaloneCore {
    fn preload_resume(&mut self, data: &[u8]) -> bool {
        if self.emulator != Emulator::Ppsspp {
            return false;
        }
        let Some((dir, _)) = self.emulator.states(&self.root) else {
            return false;
        };
        let path = dir.join("resume.ppst");
        if std::fs::write(&path, data).is_err() {
            return false;
        }
        self.preload_state = Some(path);
        true
    }

    fn load(&mut self, rom: &Path) -> Result<(), CoreError> {
        self.rom = rom.to_path_buf();
        self.state = self.emulator.state_file(&self.root, rom);
        let script = self.root.join(self.emulator.script());
        let (ours, theirs) = UnixStream::pair()?;
        let Some(mem) = self.mem_fd.take() else {
            return Err(CoreError::Load("the emulator is already started".into()));
        };
        let mut cmd = Command::new("/bin/sh");
        cmd.arg(&script)
            .arg(rom)
            .env("SLOT_ROOT", &self.root)
            .env("UNSLOT_BRIDGE", "3 4");
        match self.emulator {
            Emulator::Ppsspp => {
                if let Some((dir, _)) = self.emulator.states(&self.root) {
                    cmd.env("SLOT_PPSSPP_STATES", dir);
                }
                if let Some(state_path) = self.preload_state.take() {
                    cmd.env("SLOT_RESUME_STATE", state_path);
                }
            }
            Emulator::Drastic => {
                if let Some((dir, _)) = self.emulator.states(&self.root) {
                    cmd.env("SLOT_DRASTIC_STATES", dir);
                }
            }
            Emulator::Pico8 => {}
        }
        let (mem_raw, sock_raw) = (mem.as_raw_fd(), theirs.as_raw_fd());
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                // Its own group, so the emulator goes with the script that started it.
                libc::setpgid(0, 0);
                if libc::dup2(mem_raw, 3) < 0 || libc::dup2(sock_raw, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn()?;
        drop((mem, theirs));
        eprintln!(
            "slot: standalone: {} {} in {}",
            script.display(),
            rom.display(),
            child.id()
        );
        self.child = Some(child);
        let began = Instant::now();
        let mut sock = ours;
        let _ = sock.set_read_timeout(Some(STARTING));
        let mut hello = [0u8];
        let said = sock.read_exact(&mut hello);
        self.sock = Some(sock);
        match said {
            Ok(()) if hello[0] == b'h' => {
                eprintln!("slot: standalone: up in {} ms", began.elapsed().as_millis());
                Ok(())
            }
            Ok(()) => Err(CoreError::Load(format!("the emulator said {:?}", hello[0]))),
            Err(e) => {
                self.gone = true;
                Err(CoreError::Load(format!(
                    "the emulator never showed a frame: {e}"
                )))
            }
        }
    }

    fn run_frame(&mut self, input: ButtonMask) {
        self.bridge_mut().buttons = u32::from(input.0);
        if self.ask(b'r', b'f', RUNNING).is_err() {
            self.bridge_mut().samples = 0;
        }
    }

    fn set_sticks(&mut self, port: usize, sticks: Sticks) {
        if port == 0 {
            self.bridge_mut().sticks = sticks;
        }
    }

    fn video_xrgb8888(&self) -> &[u8] {
        let b = self.bridge();
        let len = (b.width as usize * b.height as usize).min(MAX_PIXELS);
        // SAFETY: the frame's pixels are u32s; their bytes are what is read.
        unsafe { std::slice::from_raw_parts(b.frame.as_ptr().cast::<u8>(), len * 4) }
    }

    fn video_size(&self) -> (u32, u32) {
        let b = self.bridge();
        match b.width as usize * b.height as usize {
            0 => (1, 1),
            n if n > MAX_PIXELS => (1, 1),
            _ => (b.width, b.height),
        }
    }

    fn display_aspect(&self) -> Option<f32> {
        let (w, h) = self.video_size();
        Some(w as f32 / h as f32)
    }

    fn take_audio(&mut self) -> Vec<i16> {
        let b = self.bridge_mut();
        let n = (b.samples as usize).min(MAX_SAMPLES) * 2;
        b.samples = 0;
        b.audio[..n].to_vec()
    }

    fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
        if self.emulator.states(&self.root).is_none() {
            return Err(CoreError::State("PICO-8 keeps no states".into()));
        }
        // A known state is gone first, so the one read is the one just saved; an unknown one is
        // whichever state the save makes or changes. Either is read once it has stopped growing.
        if let Some(file) = &self.state {
            let _ = std::fs::remove_file(file);
        }
        let before = self.written();
        let began = Instant::now();
        let mut pressed = began;
        self.ask(b's', b'k', RUNNING)?;
        let mut seen = None;
        let file = loop {
            let now = self.written().into_iter().find(|w| match &self.state {
                Some(file) => &w.0 == file,
                None => !before.contains(w),
            });
            if let (Some(a), Some(b)) = (&now, &seen) {
                if a == b {
                    break a.0.clone();
                }
            }
            seen = now;
            if began.elapsed() >= SAVING {
                return Err(CoreError::State("the emulator saved no state".into()));
            }
            if self.state.is_none() && seen.is_none() && pressed.elapsed() >= PRESS_AGAIN {
                pressed = Instant::now();
                self.ask(b's', b'k', RUNNING)?;
            } else {
                self.ask(b'r', b'f', RUNNING)?;
            }
        };
        if self.state.is_none() {
            if let (Some(learned), Some(name)) = (
                self.emulator.learned(&self.root, &self.rom),
                file.file_name(),
            ) {
                if let Some(dir) = learned.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(learned, name.to_string_lossy().as_bytes());
            }
            self.state = Some(file.clone());
        }
        let bytes = std::fs::read(&file)
            .map_err(|e| CoreError::State(format!("{}: {e}", file.display())))?;
        if matches!(self.emulator, Emulator::Ppsspp | Emulator::Drastic) {
            let _ = std::fs::remove_file(&file);
            let _ = std::fs::remove_file(file.with_extension("jpg"));
        }
        Ok(bytes)
    }

    fn unserialize(&mut self, data: &[u8]) -> Result<(), CoreError> {
        // Where PPSSPP loads the game's state from is first learned by saving it.
        if self.state.is_none() {
            self.serialize()?;
        }
        let Some(file) = self.state.clone() else {
            return Err(CoreError::State("the emulator's state is nowhere".into()));
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if std::fs::read(&file).ok().as_deref() != Some(data) {
            std::fs::write(&file, data)?;
        }
        self.ask(b'l', b'k', RUNNING)
    }

    // The emulator keeps the game's own save, where it always has.
    fn save_ram(&self) -> Option<Vec<u8>> {
        None
    }

    fn load_save_ram(&mut self, _data: &[u8]) -> Result<(), CoreError> {
        Ok(())
    }

    fn av_info(&self) -> AvInfo {
        AvInfo {
            fps: self.emulator.fps(),
            sample_rate: match self.bridge().rate {
                0 => 44100.0,
                hz => f64::from(hz),
            },
        }
    }

    fn gone(&self) -> bool {
        self.gone
    }
}

impl Drop for StandaloneCore {
    fn drop(&mut self) {
        if self.sock.is_some() {
            let _ = self.ask(b'q', b'k', CLOSING);
        }
        if let Some(mut child) = self.child.take() {
            let until = Instant::now() + CLOSING;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) if Instant::now() < until => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Ok(None) => {
                        eprintln!("slot: standalone: the emulator would not close, killing it");
                        // SAFETY: the group the script was started in, which is its own.
                        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
        if self.emulator == Emulator::Ppsspp {
            let _ = std::fs::remove_file(Path::new(PPSSPP_TMP_STATES).join("resume.ppst"));
        }
        // SAFETY: mapped in new, with this size, and not touched after.
        unsafe { libc::munmap(self.mem.cast(), std::mem::size_of::<Bridge>()) };
    }
}
