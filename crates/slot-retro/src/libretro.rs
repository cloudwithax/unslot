use std::cell::Cell;
use std::ffi::{c_char, c_int, c_uint, c_void, CString};
use std::marker::PhantomData;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use libloading::Library;

use crate::core::{AvInfo, ButtonMask, CoreError, RetroCore, Sticks, GBA_H, GBA_W};
use crate::ffi::*;
use crate::link::Link;
use crate::rumble::Rumble;

const VIDEO_BYTES: usize = (GBA_W * GBA_H * 4) as usize;

static LIVE: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, PartialEq, Eq)]
enum PixelFormat {
    Rgb1555,
    Xrgb8888,
    Rgb565,
}

/// Where the host keeps the core's picture.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Canvas {
    /// Every frame is centred in a buffer of this size, the way the Game Boy family is drawn.
    Fixed(u32, u32),
    /// The buffer takes whatever size each frame comes in at.
    Native,
}

struct Host {
    video: Vec<u8>,
    size: (u32, u32),
    canvas: Canvas,
    aspect: f32,
    timing: AvInfo,
    format: PixelFormat,
    audio: Vec<i16>,
    inputs: [u16; 2],
    sticks: [Sticks; 2],
    disk: Option<DiskControl>,
    system_dir: CString,
    save_dir: CString,
    rumble: Rumble,
    asked_for_rumble: bool,
    netpacket: Option<NetpacketCallback>,
    net: Link,
    net_peer: Option<u16>,
    audio_status: Option<AudioBufferStatusFn>,
    options: std::collections::HashMap<String, std::ffi::CString>,
    options_dirty: bool,
    declared: std::collections::HashMap<String, Vec<String>>,
}

thread_local! {
    static ACTIVE: Cell<*mut Host> = const { Cell::new(ptr::null_mut()) };
}

struct Active<'a>(PhantomData<&'a mut Host>);

impl Active<'_> {
    fn bind(host: &mut Host) -> Active<'_> {
        ACTIVE.with(|a| a.set(host as *mut Host));
        Active(PhantomData)
    }
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        ACTIVE.with(|a| a.set(ptr::null_mut()));
    }
}

unsafe fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    let p = ACTIVE.with(|a| a.get());
    if p.is_null() {
        return None;
    }
    Some(f(&mut *p))
}

unsafe extern "C" fn log_noop(_level: c_uint, _fmt: *const c_char) {}

unsafe extern "C" fn set_rumble_state(port: c_uint, effect: c_uint, strength: u16) -> bool {
    with_host(|h| h.rumble.set(port, effect, strength)).unwrap_or(false)
}

unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    match cmd {
        GET_CAN_DUPE => {
            if data.is_null() {
                return false;
            }
            *(data as *mut bool) = true;
            true
        }
        SET_PIXEL_FORMAT => {
            if data.is_null() {
                return false;
            }
            let want = match *(data as *const c_uint) {
                PIXEL_FORMAT_0RGB1555 => PixelFormat::Rgb1555,
                PIXEL_FORMAT_XRGB8888 => PixelFormat::Xrgb8888,
                PIXEL_FORMAT_RGB565 => PixelFormat::Rgb565,
                _ => return false,
            };
            with_host(|h| h.format = want).is_some()
        }
        GET_SYSTEM_DIRECTORY | GET_SAVE_DIRECTORY => {
            if data.is_null() {
                return false;
            }
            with_host(|h| {
                let dir = if cmd == GET_SYSTEM_DIRECTORY {
                    &h.system_dir
                } else {
                    &h.save_dir
                };
                *(data as *mut *const c_char) = dir.as_ptr();
                true
            })
            .unwrap_or(false)
        }
        GET_VARIABLE => {
            if data.is_null() {
                return false;
            }
            let var = &mut *(data as *mut Variable);
            if var.key.is_null() {
                return false;
            }
            let Ok(key) = std::ffi::CStr::from_ptr(var.key).to_str() else {
                return false;
            };
            with_host(|h| match h.options.get(key) {
                Some(value) => {
                    var.value = value.as_ptr();
                    true
                }
                None => {
                    var.value = ptr::null();
                    false
                }
            })
            .unwrap_or(false)
        }
        GET_VARIABLE_UPDATE => {
            if data.is_null() {
                return false;
            }
            with_host(|h| {
                *(data as *mut bool) = h.options_dirty;
                h.options_dirty = false;
                true
            })
            .unwrap_or(false)
        }
        SET_VARIABLES => {
            if data.is_null() {
                return with_host(|h| h.declared.clear()).is_some();
            }
            let mut list = std::collections::HashMap::new();
            let mut p = data as *const Variable;
            for _ in 0..4096 {
                let var = &*p;
                if var.key.is_null() {
                    break;
                }
                let Ok(key) = std::ffi::CStr::from_ptr(var.key).to_str() else {
                    break;
                };
                let values = if var.value.is_null() {
                    Vec::new()
                } else {
                    std::ffi::CStr::from_ptr(var.value)
                        .to_str()
                        .ok()
                        .and_then(|v| v.split_once(';'))
                        .map(|(_, values)| {
                            values
                                .trim()
                                .split('|')
                                .map(|s| s.trim().to_string())
                                .collect()
                        })
                        .unwrap_or_default()
                };
                list.insert(key.to_string(), values);
                p = p.add(1);
            }
            with_host(|h| h.declared = list).is_some()
        }
        GET_RUMBLE_INTERFACE => {
            if data.is_null() {
                return false;
            }
            with_host(|h| {
                h.asked_for_rumble = true;
                (*(data as *mut RumbleInterface)).set_rumble_state = set_rumble_state;
            })
            .is_some()
        }
        GET_LOG_INTERFACE => {
            if data.is_null() {
                return false;
            }
            (*(data as *mut LogCallback)).log = log_noop as *const c_void;
            true
        }
        SET_AUDIO_BUFFER_STATUS_CALLBACK => {
            if data.is_null() {
                return with_host(|h| h.audio_status = None).is_some();
            }
            let cb = std::ptr::read(data as *const AudioBufferStatusCallback);
            with_host(|h| h.audio_status = cb.callback).is_some()
        }
        SET_NETPACKET_INTERFACE => {
            if data.is_null() {
                return with_host(|h| h.netpacket = None).is_some();
            }
            with_host(|h| {
                h.netpacket = Some(std::ptr::read(data as *const NetpacketCallback));
            })
            .is_some()
        }
        SET_GEOMETRY => {
            if data.is_null() {
                return false;
            }
            let geometry = *(data as *const GameGeometry);
            with_host(|h| h.aspect = aspect_of(&geometry)).is_some()
        }
        SET_SYSTEM_AV_INFO => {
            if data.is_null() {
                return false;
            }
            let av = *(data as *const SystemAvInfo);
            with_host(|h| {
                h.aspect = aspect_of(&av.geometry);
                h.timing = AvInfo {
                    fps: av.timing.fps,
                    sample_rate: av.timing.sample_rate,
                };
            })
            .is_some()
        }
        GET_INPUT_BITMASKS => true,
        GET_DISK_CONTROL_INTERFACE_VERSION => {
            if data.is_null() {
                return false;
            }
            *(data as *mut c_uint) = 1;
            true
        }
        SET_DISK_CONTROL_INTERFACE => {
            if data.is_null() {
                return false;
            }
            let b = *(data as *const DiskControlBasic);
            let disk = DiskControl {
                set_eject_state: b.set_eject_state,
                get_eject_state: b.get_eject_state,
                get_image_index: b.get_image_index,
                set_image_index: b.set_image_index,
                get_num_images: b.get_num_images,
                replace_image_index: b.replace_image_index,
                add_image_index: b.add_image_index,
                ..DiskControl::default()
            };
            with_host(|h| h.disk = Some(disk)).is_some()
        }
        SET_DISK_CONTROL_EXT_INTERFACE => {
            if data.is_null() {
                return false;
            }
            let disk = *(data as *const DiskControl);
            with_host(|h| h.disk = Some(disk)).is_some()
        }
        _ => false,
    }
}

/// Opens the tray, changes the disc and closes it again, the order cores expect. None when the
/// core left out a call this needs.
unsafe fn swap_disk(d: &DiskControl, index: u32) -> Option<bool> {
    let (eject, set) = (d.set_eject_state?, d.set_image_index?);
    Some(eject(true) && set(index) && eject(false))
}

fn aspect_of(g: &GameGeometry) -> f32 {
    if g.aspect_ratio > 0.0 {
        g.aspect_ratio
    } else if g.base_height > 0 {
        g.base_width as f32 / g.base_height as f32
    } else {
        GBA_W as f32 / GBA_H as f32
    }
}

unsafe extern "C" fn netpacket_send(
    _flags: c_int,
    buf: *const c_void,
    len: usize,
    _client_id: u16,
) {
    if buf.is_null() || len == 0 {
        return;
    }
    let bytes = std::slice::from_raw_parts(buf as *const u8, len).to_vec();
    with_host(|h| h.net.push_outbound(bytes));
}

unsafe extern "C" fn netpacket_poll_receive() {
    let Some((receive, net, client_id)) = with_host(|h| {
        let receive = h.netpacket.as_ref().and_then(|cb| cb.receive)?;
        let client_id = h.net_peer?;
        Some((receive, h.net.clone(), client_id))
    })
    .flatten() else {
        return;
    };
    if !net.is_active() {
        return;
    }
    while let Some(packet) = net.take_inbound() {
        receive(packet.as_ptr() as *const c_void, packet.len(), client_id);
    }
}

unsafe fn begin_link(client_id: u16) {
    let Some(start) = with_host(|h| h.netpacket.as_ref().and_then(|cb| cb.start)).flatten() else {
        return;
    };
    start(client_id, netpacket_send, netpacket_poll_receive);

    let peer = 1u16.wrapping_sub(client_id);
    let connected = with_host(|h| {
        h.net.set_active(true);
        h.net_peer = Some(peer);
        h.netpacket.as_ref().and_then(|cb| cb.connected)
    })
    .flatten();
    if let Some(connected) = connected {
        connected(peer);
    }
}

unsafe fn halt_link() {
    let Some((stop, peer, disconnected)) = with_host(|h| {
        let stop = h.netpacket.as_ref().and_then(|cb| cb.stop);
        let peer = h.net_peer.take();
        let disconnected = h.netpacket.as_ref().and_then(|cb| cb.disconnected);
        (stop, peer, disconnected)
    }) else {
        return;
    };
    if let (Some(peer), Some(disconnected)) = (peer, disconnected) {
        disconnected(peer);
    }
    if let Some(stop) = stop {
        stop();
    }
}

unsafe fn drain_link() {
    let Some((receive, poll, net, client_id)) = with_host(|h| {
        let cb = h.netpacket.as_ref()?;
        let client_id = h.net_peer?;
        Some((cb.receive, cb.poll, h.net.clone(), client_id))
    })
    .flatten() else {
        return;
    };
    if !net.is_active() {
        return;
    }
    if let Some(receive) = receive {
        while let Some(packet) = net.take_inbound() {
            receive(packet.as_ptr() as *const c_void, packet.len(), client_id);
        }
    }
    if let Some(poll) = poll {
        poll();
    }
}

unsafe fn report_audio_status(skip: bool) {
    let Some(status) = with_host(|h| h.audio_status).flatten() else {
        return;
    };
    status(true, if skip { 0 } else { 100 }, skip);
}

unsafe extern "C" fn video_refresh(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    if data.is_null() || width == 0 || height == 0 {
        return;
    }
    with_host(|h| {
        let (cw, cols, rows, ox, oy) = match h.canvas {
            Canvas::Fixed(cw, ch) => {
                let cols = width.min(cw) as usize;
                let rows = height.min(ch) as usize;
                if cols != cw as usize || rows != ch as usize {
                    h.video.fill(0);
                }
                let (ox, oy) = ((cw as usize - cols) / 2, (ch as usize - rows) / 2);
                (cw as usize, cols, rows, ox, oy)
            }
            Canvas::Native => {
                if h.size != (width, height) {
                    h.size = (width, height);
                    h.video.resize((width * height * 4) as usize, 0);
                }
                (width as usize, width as usize, height as usize, 0, 0)
            }
        };
        for y in 0..rows {
            let src = (data as *const u8).add(y * pitch);
            let at = ((y + oy) * cw + ox) * 4;
            convert_row(h.format, src, &mut h.video[at..at + cols * 4]);
        }
    });
}

unsafe fn convert_row(format: PixelFormat, src: *const u8, dst: &mut [u8]) {
    let expand5 = |v: u16| ((v << 3) | (v >> 2)) as u8;
    match format {
        PixelFormat::Xrgb8888 => {
            ptr::copy_nonoverlapping(src, dst.as_mut_ptr(), dst.len());
        }
        PixelFormat::Rgb565 => {
            for (x, px) in dst.chunks_exact_mut(4).enumerate() {
                let p = ptr::read_unaligned((src as *const u16).add(x));
                let g = (p >> 5) & 0x3f;
                px[0] = expand5(p & 0x1f);
                px[1] = ((g << 2) | (g >> 4)) as u8;
                px[2] = expand5((p >> 11) & 0x1f);
                px[3] = 0;
            }
        }
        PixelFormat::Rgb1555 => {
            for (x, px) in dst.chunks_exact_mut(4).enumerate() {
                let p = ptr::read_unaligned((src as *const u16).add(x));
                px[0] = expand5(p & 0x1f);
                px[1] = expand5((p >> 5) & 0x1f);
                px[2] = expand5((p >> 10) & 0x1f);
                px[3] = 0;
            }
        }
    }
}

unsafe extern "C" fn audio_sample(left: i16, right: i16) {
    with_host(|h| h.audio.extend_from_slice(&[left, right]));
}

unsafe extern "C" fn audio_batch(data: *const i16, frames: usize) -> usize {
    if !data.is_null() {
        with_host(|h| {
            h.audio
                .extend_from_slice(std::slice::from_raw_parts(data, frames * 2))
        });
    }
    frames
}

unsafe extern "C" fn input_poll() {}

unsafe extern "C" fn input_state(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16 {
    // The low byte is the base device; the rest names a subclass the core may have made of it.
    let device = device & 0xff;
    with_host(|h| {
        let Some(&input) = h.inputs.get(port as usize) else {
            return 0;
        };
        match (device, index) {
            (DEVICE_JOYPAD, _) => match id {
                JOYPAD_MASK => input as i16,
                _ if id < 16 => ((input >> id) & 1) as i16,
                _ => 0,
            },
            (DEVICE_ANALOG, DEVICE_INDEX_ANALOG_BUTTON) if id < 16 => {
                if (input >> id) & 1 == 1 {
                    i16::MAX
                } else {
                    0
                }
            }
            (DEVICE_ANALOG, 0 | 1) if id < 2 => h.sticks[port as usize][(index * 2 + id) as usize],
            _ => 0,
        }
    })
    .unwrap_or(0)
}

pub struct LibretroCore {
    api: Api,
    host: Box<Host>,
    rom: Vec<u8>,
    rom_path: Option<CString>,
    need_fullpath: bool,
    extensions: Vec<String>,
    loaded: bool,
    state_size: Option<usize>,
    _lib: Library,
}

unsafe fn c_str<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    std::ffi::CStr::from_ptr(p).to_str().ok()
}

fn cdir(path: &Path) -> Result<CString, CoreError> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| CoreError::Load(format!("{} contains a nul", path.display())))
}

impl LibretroCore {
    pub fn open(dylib: &Path) -> Result<Self, CoreError> {
        let dir = dylib.parent().unwrap_or(Path::new(".")).to_path_buf();
        Self::open_with(dylib, &dir, &dir)
    }

    pub fn open_with(dylib: &Path, system_dir: &Path, save_dir: &Path) -> Result<Self, CoreError> {
        if LIVE.swap(true, Ordering::SeqCst) {
            return Err(CoreError::Unsupported("a core is already open".into()));
        }
        Self::open_inner(dylib, system_dir, save_dir)
            .inspect_err(|_| LIVE.store(false, Ordering::SeqCst))
    }

    pub fn reported_system_dir(&self) -> String {
        self.host.system_dir.to_string_lossy().into_owned()
    }

    pub fn reported_save_dir(&self) -> String {
        self.host.save_dir.to_string_lossy().into_owned()
    }

    pub fn asked_for_rumble(&self) -> bool {
        self.host.asked_for_rumble
    }

    /// Fixed keeps the Game Boy family's centred 240x160 picture; Native hands frames on at
    /// whatever size the core draws them.
    pub fn set_canvas(&mut self, canvas: Canvas) {
        self.host.canvas = canvas;
        if let Canvas::Fixed(w, h) = canvas {
            self.host.size = (w, h);
            self.host.video = vec![0; (w * h * 4) as usize];
        }
    }

    /// The file extensions the core says it opens, lower case and without the dot.
    pub fn valid_extensions(&self) -> &[String] {
        &self.extensions
    }

    /// Whether the core opens the game from its path itself, so the host must not read it.
    pub fn needs_fullpath(&self) -> bool {
        self.need_fullpath
    }

    pub fn set_option(&mut self, key: &str, value: &str) {
        if !self.host.declared.is_empty() {
            match self.host.declared.get(key) {
                None => eprintln!("slot-retro: core declares no option {key:?}, setting it anyway"),
                Some(values) if !values.is_empty() && !values.iter().any(|v| v == value) => {
                    eprintln!("slot-retro: core declares {key:?} as {values:?}, not {value:?}")
                }
                Some(_) => {}
            }
        }
        let Ok(value) = CString::new(value) else {
            return;
        };
        self.host.options.insert(key.to_string(), value);
        self.host.options_dirty = true;
    }

    pub fn options(&self) -> Vec<(String, String)> {
        self.host
            .options
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.to_str().ok()?.to_string())))
            .collect()
    }

    pub fn declared_options(&self) -> &std::collections::HashMap<String, Vec<String>> {
        &self.host.declared
    }

    pub fn option(&self, key: &str) -> Option<String> {
        self.host
            .options
            .get(key)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    fn open_inner(dylib: &Path, system_dir: &Path, save_dir: &Path) -> Result<Self, CoreError> {
        let lib = unsafe { Library::new(dylib) }.map_err(|e| CoreError::Load(e.to_string()))?;
        let api = unsafe { Api::load(&lib) }?;
        let version = unsafe { (api.api_version)() };
        if version != API_VERSION {
            return Err(CoreError::Unsupported(format!("libretro api {version}")));
        }
        let mut info = SystemInfo::default();
        unsafe { (api.get_system_info)(&mut info) };
        let extensions = unsafe { c_str(info.valid_extensions) }
            .map(|list| {
                list.split('|')
                    .filter(|e| !e.is_empty())
                    .map(|e| e.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let need_fullpath = info.need_fullpath;
        let mut host = Box::new(Host {
            video: vec![0; VIDEO_BYTES],
            size: (GBA_W, GBA_H),
            canvas: Canvas::Fixed(GBA_W, GBA_H),
            aspect: GBA_W as f32 / GBA_H as f32,
            timing: AvInfo {
                fps: 0.0,
                sample_rate: 0.0,
            },
            format: PixelFormat::Rgb1555,
            audio: Vec::new(),
            inputs: [0; 2],
            sticks: [[0; 4]; 2],
            disk: None,
            system_dir: cdir(system_dir)?,
            save_dir: cdir(save_dir)?,
            rumble: Rumble::default(),
            asked_for_rumble: false,
            netpacket: None,
            net: Link::default(),
            net_peer: None,
            audio_status: None,
            options: std::collections::HashMap::new(),
            options_dirty: false,
            declared: std::collections::HashMap::new(),
        });
        unsafe {
            let _a = Active::bind(&mut host);
            (api.set_environment)(environment);
            (api.set_video_refresh)(video_refresh);
            (api.set_audio_sample)(audio_sample);
            (api.set_audio_sample_batch)(audio_batch);
            (api.set_input_poll)(input_poll);
            (api.set_input_state)(input_state);
            (api.init)();
        }
        Ok(LibretroCore {
            api,
            host,
            rom: Vec::new(),
            rom_path: None,
            need_fullpath,
            extensions,
            loaded: false,
            state_size: None,
            _lib: lib,
        })
    }

    fn unload(&mut self) {
        if !self.loaded {
            return;
        }
        let _a = Active::bind(&mut self.host);
        unsafe { (self.api.unload_game)() };
        self.loaded = false;
    }

    fn serialize_at(&mut self, size: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; size];
        let ok = {
            let _a = Active::bind(&mut self.host);
            unsafe { (self.api.serialize)(buf.as_mut_ptr() as *mut c_void, size) }
        };
        ok.then_some(buf)
    }

    fn with_disk<R>(&mut self, f: impl FnOnce(&DiskControl) -> Option<R>) -> Option<R> {
        let disk = self.host.disk?;
        let _a = Active::bind(&mut self.host);
        f(&disk)
    }

    fn memory(&self, id: c_uint) -> Option<&[u8]> {
        let data = unsafe { (self.api.get_memory_data)(id) };
        let len = unsafe { (self.api.get_memory_size)(id) };
        if data.is_null() || len == 0 {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts(data as *const u8, len) })
    }

    fn memory_mut(&mut self, id: c_uint) -> Option<&mut [u8]> {
        let data = unsafe { (self.api.get_memory_data)(id) };
        let len = unsafe { (self.api.get_memory_size)(id) };
        if data.is_null() || len == 0 {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts_mut(data as *mut u8, len) })
    }
}

impl Drop for LibretroCore {
    fn drop(&mut self) {
        self.unload();
        unsafe {
            let _a = Active::bind(&mut self.host);
            (self.api.deinit)();
        }
        LIVE.store(false, Ordering::SeqCst);
    }
}

impl RetroCore for LibretroCore {
    fn set_option(&mut self, key: &str, value: &str) {
        LibretroCore::set_option(self, key, value);
    }

    fn load(&mut self, rom: &Path) -> Result<(), CoreError> {
        self.unload();
        self.state_size = None;
        self.rom = match self.need_fullpath {
            true => Vec::new(),
            false => std::fs::read(rom)?,
        };
        self.rom_path = Some(
            CString::new(rom.as_os_str().as_encoded_bytes())
                .map_err(|_| CoreError::Load("rom path contains a nul".into()))?,
        );
        let info = GameInfo {
            path: self.rom_path.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
            data: match self.need_fullpath {
                true => ptr::null(),
                false => self.rom.as_ptr() as *const c_void,
            },
            size: self.rom.len(),
            meta: ptr::null(),
        };
        let ok = {
            let _a = Active::bind(&mut self.host);
            unsafe { (self.api.load_game)(&info) }
        };
        if !ok {
            self.rom = Vec::new();
            self.rom_path = None;
            return Err(CoreError::Load(format!("core refused {}", rom.display())));
        }
        self.loaded = true;
        let mut av = SystemAvInfo::default();
        unsafe { (self.api.get_system_av_info)(&mut av) };
        self.host.timing = AvInfo {
            fps: av.timing.fps,
            sample_rate: av.timing.sample_rate,
        };
        self.host.aspect = aspect_of(&av.geometry);
        unsafe { (self.api.set_controller_port_device)(0, DEVICE_JOYPAD) };
        Ok(())
    }

    fn run_frame(&mut self, input: ButtonMask) {
        self.run_frame_linked(input, ButtonMask::default());
    }

    fn run_frame_linked(&mut self, p1: ButtonMask, p2: ButtonMask) {
        if !self.loaded {
            return;
        }
        self.host.inputs = [p1.0, p2.0];
        let _a = Active::bind(&mut self.host);
        unsafe { (self.api.run)() };
    }

    fn set_frame_skip(&mut self, skip: bool) {
        let _a = Active::bind(&mut self.host);
        unsafe { report_audio_status(skip) };
    }

    fn video_xrgb8888(&self) -> &[u8] {
        &self.host.video
    }

    fn video_size(&self) -> (u32, u32) {
        self.host.size
    }

    fn display_aspect(&self) -> Option<f32> {
        (self.host.canvas == Canvas::Native).then_some(self.host.aspect)
    }

    fn set_sticks(&mut self, port: usize, sticks: Sticks) {
        if let Some(slot) = self.host.sticks.get_mut(port) {
            *slot = sticks;
        }
    }

    fn disk_count(&self) -> u32 {
        let Some(count) = self.host.disk.and_then(|d| d.get_num_images) else {
            return 0;
        };
        unsafe { count() }
    }

    fn disk_index(&self) -> u32 {
        let Some(index) = self.host.disk.and_then(|d| d.get_image_index) else {
            return 0;
        };
        unsafe { index() }
    }

    fn set_disk(&mut self, index: u32) -> Result<(), CoreError> {
        let swapped = self.with_disk(|d| unsafe { swap_disk(d, index) });
        match swapped {
            Some(true) => Ok(()),
            Some(false) => Err(CoreError::Unsupported(format!(
                "the core refused disk {index}"
            ))),
            None => Err(CoreError::Unsupported(
                "the core has no disk control".into(),
            )),
        }
    }

    fn take_audio(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.host.audio)
    }

    fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
        if let Some(buf) = self.state_size.and_then(|size| self.serialize_at(size)) {
            return Ok(buf);
        }
        let size = unsafe { (self.api.serialize_size)() };
        self.state_size = (size > 0).then_some(size);
        if size == 0 {
            return Err(CoreError::State("core reports no state".into()));
        }
        self.serialize_at(size)
            .ok_or_else(|| CoreError::State("serialize refused".into()))
    }

    fn unserialize(&mut self, data: &[u8]) -> Result<(), CoreError> {
        let ok = {
            let _a = Active::bind(&mut self.host);
            unsafe { (self.api.unserialize)(data.as_ptr() as *const c_void, data.len()) }
        };
        if ok {
            Ok(())
        } else {
            Err(CoreError::State("unserialize refused".into()))
        }
    }

    fn save_ram(&self) -> Option<Vec<u8>> {
        let mut sav = self.memory(MEMORY_SAVE_RAM)?.to_vec();
        if let Some(rtc) = self.memory(MEMORY_RTC) {
            sav.extend_from_slice(rtc);
        }
        Some(sav)
    }

    fn load_save_ram(&mut self, data: &[u8]) -> Result<(), CoreError> {
        let Some(ram) = self.memory_mut(MEMORY_SAVE_RAM) else {
            return Err(CoreError::Unsupported("core exposes no save ram".into()));
        };
        let n = ram.len().min(data.len());
        ram[..n].copy_from_slice(&data[..n]);
        let rest = &data[n..];
        if let Some(rtc) = self.memory_mut(MEMORY_RTC) {
            let m = rtc.len().min(rest.len());
            rtc[..m].copy_from_slice(&rest[..m]);
        }
        Ok(())
    }

    fn av_info(&self) -> AvInfo {
        self.host.timing
    }

    fn rumble(&self) -> Rumble {
        self.host.rumble.clone()
    }

    fn net(&self) -> Link {
        self.host.net.clone()
    }

    fn start_link(&mut self, client_id: u16) {
        let _a = Active::bind(&mut self.host);
        unsafe { begin_link(client_id) };
    }

    fn pump_link(&mut self) {
        let _a = Active::bind(&mut self.host);
        unsafe { drain_link() };
    }

    fn stop_link(&mut self) {
        let _a = Active::bind(&mut self.host);
        unsafe { halt_link() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ffi::CStr;

    fn host_with(options: HashMap<String, CString>, options_dirty: bool) -> Box<Host> {
        Box::new(Host {
            video: vec![0; VIDEO_BYTES],
            size: (GBA_W, GBA_H),
            canvas: Canvas::Fixed(GBA_W, GBA_H),
            aspect: 1.5,
            timing: AvInfo {
                fps: 0.0,
                sample_rate: 0.0,
            },
            format: PixelFormat::Xrgb8888,
            audio: Vec::new(),
            inputs: [0; 2],
            sticks: [[0; 4]; 2],
            disk: None,
            system_dir: CString::new(".").unwrap(),
            save_dir: CString::new(".").unwrap(),
            rumble: Rumble::default(),
            asked_for_rumble: false,
            netpacket: None,
            net: Link::default(),
            net_peer: None,
            audio_status: None,
            options,
            options_dirty,
            declared: HashMap::new(),
        })
    }

    fn declaration(key: &str, value: &str) -> (Variable, CString, CString) {
        let key = CString::new(key).unwrap();
        let value = CString::new(value).unwrap();
        let var = Variable {
            key: key.as_ptr(),
            value: value.as_ptr(),
        };
        (var, key, value)
    }

    fn end_of_list() -> Variable {
        Variable {
            key: ptr::null(),
            value: ptr::null(),
        }
    }

    #[test]
    fn set_variables_records_what_the_core_declared() {
        let mut host = host_with(HashMap::new(), false);
        let (a, _ak, _av) = declaration("mgba_sgb_borders", "Use Super Game Boy Borders; ON|OFF");
        let (b, _bk, _bv) =
            declaration("mgba_gb_colors_preset", "Game Boy Palette Preset; 0|1|2|3");
        let list = [a, b, end_of_list()];
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe { environment(SET_VARIABLES, list.as_ptr() as *mut c_void) }
        };

        assert!(ok);
        assert_eq!(
            host.declared.get("mgba_sgb_borders").map(Vec::as_slice),
            Some(["ON".to_string(), "OFF".to_string()].as_slice())
        );
        assert_eq!(
            host.declared
                .get("mgba_gb_colors_preset")
                .map(Vec::as_slice),
            Some(["0", "1", "2", "3"].map(str::to_string).as_slice())
        );
        assert_eq!(
            host.declared.len(),
            2,
            "the walk ran past the terminator and read whatever was after it"
        );
        assert!(
            !host.declared.contains_key("mgba_sgb_border"),
            "a key the core never declared came back declared"
        );
    }

    #[test]
    fn set_variables_with_no_list_declares_nothing() {
        let mut host = host_with(HashMap::new(), false);
        host.declared
            .insert("stale".to_string(), vec!["x".to_string()]);

        let ok = {
            let _active = Active::bind(&mut host);
            unsafe { environment(SET_VARIABLES, ptr::null_mut()) }
        };

        assert!(ok);
        assert!(host.declared.is_empty(), "a stale declaration survived");
    }

    #[test]
    fn get_variable_writes_the_options_pointer_for_the_core_to_read() {
        let mut options = HashMap::new();
        options.insert("gpsp_serial".to_string(), CString::new("rfu").unwrap());
        let mut host = host_with(options, false);
        let _active = Active::bind(&mut host);

        let key = CString::new("gpsp_serial").unwrap();
        let mut var = Variable {
            key: key.as_ptr(),
            value: ptr::null(),
        };
        let ok = unsafe { environment(GET_VARIABLE, &mut var as *mut Variable as *mut c_void) };

        assert!(ok);
        let value = unsafe { CStr::from_ptr(var.value) };
        assert_eq!(value.to_str().unwrap(), "rfu");
    }

    #[test]
    fn get_variable_reports_false_for_an_unknown_key() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);

        let key = CString::new("nope").unwrap();
        let mut var = Variable {
            key: key.as_ptr(),
            value: ptr::null(),
        };
        let ok = unsafe { environment(GET_VARIABLE, &mut var as *mut Variable as *mut c_void) };

        assert!(!ok);
        assert!(var.value.is_null());
    }

    #[test]
    fn get_variable_update_reports_and_clears_the_dirty_flag() {
        let mut host = host_with(HashMap::new(), true);
        let _active = Active::bind(&mut host);

        let mut dirty = false;
        let ok =
            unsafe { environment(GET_VARIABLE_UPDATE, &mut dirty as *mut bool as *mut c_void) };
        assert!(ok);
        assert!(dirty, "first call must report the pending change");

        let mut dirty_again = true;
        let ok = unsafe {
            environment(
                GET_VARIABLE_UPDATE,
                &mut dirty_again as *mut bool as *mut c_void,
            )
        };
        assert!(ok);
        assert!(!dirty_again, "flag must be cleared after being read once");
    }

    #[test]
    fn get_variable_refuses_a_null_key() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);

        let mut var = Variable {
            key: ptr::null(),
            value: ptr::null(),
        };
        let ok = unsafe { environment(GET_VARIABLE, &mut var as *mut Variable as *mut c_void) };
        assert!(!ok);
    }

    thread_local! {
        static TEST_AUDIO_STATUS: RefCell<Vec<(bool, c_uint, bool)>> =
            const { RefCell::new(Vec::new()) };
    }

    unsafe extern "C" fn test_audio_status(active: bool, occupancy: c_uint, underrun: bool) {
        TEST_AUDIO_STATUS.with(|r| r.borrow_mut().push((active, occupancy, underrun)));
    }

    #[test]
    fn set_audio_buffer_status_callback_stores_what_the_core_hands_over() {
        let mut host = host_with(HashMap::new(), false);
        let mut cb = AudioBufferStatusCallback {
            callback: Some(test_audio_status),
        };
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe {
                environment(
                    SET_AUDIO_BUFFER_STATUS_CALLBACK,
                    &mut cb as *mut AudioBufferStatusCallback as *mut c_void,
                )
            }
        };

        assert!(ok, "refusing this turns the cores' frameskip off entirely");
        assert!(host.audio_status.is_some(), "the callback was never stored");
    }

    #[test]
    fn set_audio_buffer_status_callback_null_withdraws_it() {
        let mut host = host_with(HashMap::new(), false);
        host.audio_status = Some(test_audio_status);
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe { environment(SET_AUDIO_BUFFER_STATUS_CALLBACK, ptr::null_mut()) }
        };

        assert!(ok, "withdrawing is a legal call and must be answered true");
        assert!(host.audio_status.is_none());
    }

    #[test]
    fn set_frame_skip_tells_the_core_to_skip_by_reporting_an_underrun() {
        TEST_AUDIO_STATUS.with(|r| r.borrow_mut().clear());
        let mut host = host_with(HashMap::new(), false);
        host.audio_status = Some(test_audio_status);
        {
            let _active = Active::bind(&mut host);
            unsafe {
                report_audio_status(true);
                report_audio_status(false);
            }
        }

        TEST_AUDIO_STATUS.with(|r| {
            assert_eq!(
                r.borrow().as_slice(),
                &[(true, 0, true), (true, 100, false)],
                "the skip and the draw did not read as an underrun and a healthy buffer"
            );
        });
    }

    #[test]
    fn set_frame_skip_is_a_noop_when_the_core_registered_no_callback() {
        TEST_AUDIO_STATUS.with(|r| r.borrow_mut().clear());
        let mut host = host_with(HashMap::new(), false);
        {
            let _active = Active::bind(&mut host);
            unsafe { report_audio_status(true) };
        }

        TEST_AUDIO_STATUS.with(|r| assert!(r.borrow().is_empty()));
    }

    thread_local! {
        static TEST_RECEIVED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
        static TEST_RECEIVED_CLIENT_IDS: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
        static TEST_POLLS: Cell<u32> = const { Cell::new(0) };
        static TEST_START_CLIENT: Cell<Option<u16>> = const { Cell::new(None) };
        static TEST_STOP_CALLS: Cell<u32> = const { Cell::new(0) };
        static TEST_CONNECTED_CLIENT: Cell<Option<u16>> = const { Cell::new(None) };
        static TEST_DISCONNECTED_CLIENT: Cell<Option<u16>> = const { Cell::new(None) };
    }

    fn reset_test_netpacket_recorders() {
        TEST_RECEIVED.with(|r| r.borrow_mut().clear());
        TEST_RECEIVED_CLIENT_IDS.with(|c| c.borrow_mut().clear());
        TEST_POLLS.with(|p| p.set(0));
        TEST_START_CLIENT.with(|c| c.set(None));
        TEST_STOP_CALLS.with(|c| c.set(0));
        TEST_CONNECTED_CLIENT.with(|c| c.set(None));
        TEST_DISCONNECTED_CLIENT.with(|c| c.set(None));
    }

    unsafe extern "C" fn test_receive(buf: *const c_void, len: usize, client_id: u16) {
        let bytes = std::slice::from_raw_parts(buf as *const u8, len).to_vec();
        TEST_RECEIVED.with(|r| r.borrow_mut().push(bytes));
        TEST_RECEIVED_CLIENT_IDS.with(|c| c.borrow_mut().push(client_id));
    }

    unsafe extern "C" fn test_receive_reentrant(buf: *const c_void, len: usize, client_id: u16) {
        test_receive(buf, len, client_id);
        let reentrant = b"reentrant";
        netpacket_send(0, reentrant.as_ptr() as *const c_void, reentrant.len(), 0);
    }

    unsafe extern "C" fn test_start_reentrant(
        client_id: u16,
        _send: NetpacketSend,
        poll_receive: NetpacketPollReceive,
    ) {
        TEST_START_CLIENT.with(|c| c.set(Some(client_id)));
        poll_receive();
    }

    unsafe extern "C" fn test_poll() {
        TEST_POLLS.with(|p| p.set(p.get() + 1));
    }

    unsafe extern "C" fn test_start(
        client_id: u16,
        _send: NetpacketSend,
        _poll_receive: NetpacketPollReceive,
    ) {
        TEST_START_CLIENT.with(|c| c.set(Some(client_id)));
    }

    unsafe extern "C" fn test_stop() {
        TEST_STOP_CALLS.with(|c| c.set(c.get() + 1));
    }

    unsafe extern "C" fn test_connected(client_id: u16) -> bool {
        TEST_CONNECTED_CLIENT.with(|c| c.set(Some(client_id)));
        true
    }

    unsafe extern "C" fn test_disconnected(client_id: u16) {
        TEST_DISCONNECTED_CLIENT.with(|c| c.set(Some(client_id)));
    }

    fn test_netpacket_callback() -> NetpacketCallback {
        NetpacketCallback {
            start: Some(test_start),
            receive: Some(test_receive),
            stop: None,
            poll: Some(test_poll),
            connected: None,
            disconnected: None,
            protocol_version: ptr::null(),
        }
    }

    #[test]
    fn set_netpacket_interface_stores_the_callback_the_core_hands_over() {
        let mut host = host_with(HashMap::new(), false);
        let mut cb = test_netpacket_callback();
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe {
                environment(
                    SET_NETPACKET_INTERFACE,
                    &mut cb as *mut NetpacketCallback as *mut c_void,
                )
            }
        };

        assert!(ok);
        assert!(host.netpacket.is_some(), "the callback was never stored");
    }

    #[test]
    fn set_netpacket_interface_null_data_withdraws_the_interface() {
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe { environment(SET_NETPACKET_INTERFACE, ptr::null_mut()) }
        };

        assert!(ok, "withdrawing is a legal call and must be answered true");
        assert!(
            host.netpacket.is_none(),
            "a NULL data pointer must clear a previously registered callback"
        );
    }

    #[test]
    fn netpacket_send_pushes_the_cores_packet_onto_outbound() {
        let mut host = host_with(HashMap::new(), false);
        let packet = b"link cable byte";
        {
            let _active = Active::bind(&mut host);
            unsafe {
                netpacket_send(
                    NETPACKET_RELIABLE,
                    packet.as_ptr() as *const c_void,
                    packet.len(),
                    0,
                )
            };
        }

        assert_eq!(host.net.take_outbound().as_deref(), Some(&packet[..]));
        assert_eq!(host.net.take_outbound(), None, "only one packet was sent");
    }

    #[test]
    fn netpacket_send_ignores_a_null_or_empty_packet() {
        let mut host = host_with(HashMap::new(), false);
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_send(0, ptr::null(), 4, 0) };
            unsafe { netpacket_send(0, [1u8].as_ptr() as *const c_void, 0, 0) };
        }

        assert_eq!(
            host.net.take_outbound(),
            None,
            "a null buffer or zero length must not enqueue a phantom packet"
        );
    }

    #[test]
    fn netpacket_poll_receive_drains_inbound_into_the_cores_receive_in_order() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"first".to_vec());
        host.net.push_inbound(b"second".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED.with(|r| {
            assert_eq!(
                r.borrow().as_slice(),
                &[b"first".to_vec(), b"second".to_vec()],
                "order was not kept"
            );
        });
        assert_eq!(host.net.take_inbound(), None, "queue must be drained");
    }

    #[test]
    fn netpacket_poll_receive_does_nothing_without_a_receive_callback() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.net.set_active(true);
        host.net.push_inbound(b"stranded".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED.with(|r| assert!(r.borrow().is_empty()));
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"stranded"[..]),
            "with nowhere to hand the packet it must be left queued, not dropped"
        );
    }

    #[test]
    fn netpacket_poll_receive_does_nothing_once_the_session_is_no_longer_active() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.push_inbound(b"stale".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED.with(|r| assert!(r.borrow().is_empty(), "an ended session reached the core"));
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"stale"[..]),
            "the packet must be left queued, not delivered to a session that already ended"
        );
    }

    #[test]
    fn netpacket_poll_receive_does_nothing_without_a_recorded_peer() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net.push_inbound(b"orphaned".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED.with(|r| {
            assert!(
                r.borrow().is_empty(),
                "a packet must not be delivered with no peer to tag it"
            )
        });
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"orphaned"[..]),
            "the packet must be left queued, not delivered mislabelled as our own id"
        );
    }

    #[test]
    fn netpacket_poll_receive_tags_packets_with_the_peers_client_id() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }
        reset_test_netpacket_recorders();
        host.net.push_inbound(b"from the peer".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED_CLIENT_IDS.with(|c| {
            assert_eq!(
                c.borrow().as_slice(),
                &[1],
                "packets must be tagged with the peer's id, not the host's own"
            );
        });
    }

    #[test]
    fn drain_link_hands_the_core_everything_waiting_then_polls() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"queued".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| assert_eq!(r.borrow().as_slice(), &[b"queued".to_vec()]));
        TEST_POLLS.with(|p| assert_eq!(p.get(), 1, "poll must run once a frame"));
    }

    #[test]
    fn drain_link_polls_even_with_nothing_inbound() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net_peer = Some(1);
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_POLLS.with(|p| assert_eq!(p.get(), 1));
    }

    #[test]
    fn drain_link_skips_poll_when_the_core_offered_none() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            poll: None,
            ..test_netpacket_callback()
        });
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"still delivered".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| {
            assert_eq!(r.borrow().as_slice(), &[b"still delivered".to_vec()]);
        });
        TEST_POLLS.with(|p| assert_eq!(p.get(), 0, "poll is optional and was not offered"));
    }

    #[test]
    fn drain_link_is_a_noop_when_the_core_never_registered_netpacket() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.net.set_active(true);
        host.net.push_inbound(b"nobody asked".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| assert!(r.borrow().is_empty()));
        TEST_POLLS.with(|p| assert_eq!(p.get(), 0));
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"nobody asked"[..]),
            "with no registered core the packet must be left queued, not lost"
        );
    }

    #[test]
    fn drain_link_does_nothing_once_the_session_is_no_longer_active() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.push_inbound(b"stale".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| assert!(r.borrow().is_empty(), "an ended session reached the core"));
        TEST_POLLS.with(|p| assert_eq!(p.get(), 0, "an ended session must not be polled either"));
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"stale"[..]),
            "the packet must be left queued, not delivered to a session that already ended"
        );
    }

    #[test]
    fn drain_link_does_nothing_without_a_recorded_peer() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net.push_inbound(b"orphaned".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| {
            assert!(
                r.borrow().is_empty(),
                "a packet must not be delivered with no peer to tag it"
            )
        });
        TEST_POLLS.with(|p| {
            assert_eq!(
                p.get(),
                0,
                "must not even poll with no peer to tag a receive"
            )
        });
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"orphaned"[..]),
            "the packet must be left queued, not delivered mislabelled as our own id"
        );
    }

    #[test]
    fn drain_link_tags_packets_with_the_peers_client_id() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }
        reset_test_netpacket_recorders();
        host.net.push_inbound(b"from the peer".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED_CLIENT_IDS.with(|c| {
            assert_eq!(
                c.borrow().as_slice(),
                &[1],
                "packets must be tagged with the peer's id, not our own"
            );
        });
    }

    #[test]
    fn drain_link_survives_the_reentrancy_gpsp_documents() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            receive: Some(test_receive_reentrant),
            ..test_netpacket_callback()
        });
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"queued".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| assert_eq!(r.borrow().as_slice(), &[b"queued".to_vec()]));
        assert_eq!(
            host.net.take_outbound().as_deref(),
            Some(&b"reentrant"[..]),
            "the reentrant netpacket_send call must still reach outbound"
        );
    }

    #[test]
    fn netpacket_poll_receive_survives_the_reentrancy_gpsp_documents() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            receive: Some(test_receive_reentrant),
            ..test_netpacket_callback()
        });
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"queued".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        assert_eq!(host.net.take_outbound().as_deref(), Some(&b"reentrant"[..]));
    }

    #[test]
    fn begin_link_survives_a_core_that_reenters_from_start() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            start: Some(test_start_reentrant),
            ..test_netpacket_callback()
        });
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }

        TEST_START_CLIENT.with(|c| assert_eq!(c.get(), Some(0)));
        assert!(host.net.is_active());
    }

    #[test]
    fn begin_link_hands_the_core_its_client_id_and_marks_the_session_active() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        assert!(
            !host.net.is_active(),
            "a fresh link is not backing a session"
        );
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(1) };
        }

        TEST_START_CLIENT.with(|c| assert_eq!(c.get(), Some(1), "client id was not forwarded"));
        assert!(
            host.net.is_active(),
            "starting a session must mark the link active"
        );
    }

    #[test]
    fn begin_link_is_a_noop_when_the_core_never_registered_netpacket() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }

        TEST_START_CLIENT.with(|c| assert_eq!(c.get(), None, "nothing to start, nothing called"));
        assert!(!host.net.is_active());
    }

    #[test]
    fn begin_link_calls_connected_with_the_peers_client_id() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            connected: Some(test_connected),
            ..test_netpacket_callback()
        });
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }

        TEST_CONNECTED_CLIENT.with(|c| {
            assert_eq!(
                c.get(),
                Some(1),
                "connected was not called with the peer's id"
            )
        });
    }

    #[test]
    fn begin_link_is_fine_with_no_connected_callback() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());

        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }

        assert!(
            host.net.is_active(),
            "a missing connected callback must not stop the session from starting"
        );
    }

    #[test]
    fn halt_link_calls_the_cores_stop_when_it_registered_one() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            stop: Some(test_stop),
            ..test_netpacket_callback()
        });
        {
            let _active = Active::bind(&mut host);
            unsafe { halt_link() };
        }

        TEST_STOP_CALLS.with(|c| assert_eq!(c.get(), 1, "stop was never called"));
    }

    #[test]
    fn halt_link_is_a_noop_when_the_core_never_offered_a_stop() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());

        {
            let _active = Active::bind(&mut host);
            unsafe { halt_link() };
        }

        TEST_STOP_CALLS.with(|c| assert_eq!(c.get(), 0));
    }

    #[test]
    fn halt_link_calls_disconnected_with_the_peers_client_id() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            disconnected: Some(test_disconnected),
            ..test_netpacket_callback()
        });
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(1) };
        }
        {
            let _active = Active::bind(&mut host);
            unsafe { halt_link() };
        }

        TEST_DISCONNECTED_CLIENT.with(|c| {
            assert_eq!(
                c.get(),
                Some(0),
                "disconnected was not called with the peer's id"
            )
        });
    }

    #[test]
    fn halt_link_does_not_call_disconnected_when_no_session_ever_started() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            disconnected: Some(test_disconnected),
            ..test_netpacket_callback()
        });

        {
            let _active = Active::bind(&mut host);
            unsafe { halt_link() };
        }

        TEST_DISCONNECTED_CLIENT.with(|c| assert_eq!(c.get(), None, "nothing was ever connected"));
    }

    #[test]
    fn input_state_answers_each_port_from_its_own_mask() {
        let mut host = host_with(HashMap::new(), false);
        host.inputs = [ButtonMask::A, ButtonMask::B | ButtonMask::START];
        let _active = Active::bind(&mut host);

        let mask = |port| unsafe { input_state(port, DEVICE_JOYPAD, 0, JOYPAD_MASK) } as u16;
        assert_eq!(mask(0), ButtonMask::A);
        assert_eq!(mask(1), ButtonMask::B | ButtonMask::START);
    }

    #[test]
    fn input_state_answers_single_buttons_on_port_1() {
        let mut host = host_with(HashMap::new(), false);
        host.inputs = [0, ButtonMask::B];
        let _active = Active::bind(&mut host);

        let button = |id| unsafe { input_state(1, DEVICE_JOYPAD, 0, id) };
        assert_eq!(button(0), 1, "B is bit 0 of the libretro joypad");
        assert_eq!(button(8), 0, "A is not held on port 1");
    }

    #[test]
    fn input_state_answers_nothing_past_port_1() {
        let mut host = host_with(HashMap::new(), false);
        host.inputs = [u16::MAX, u16::MAX];
        let _active = Active::bind(&mut host);

        assert_eq!(unsafe { input_state(2, DEVICE_JOYPAD, 0, JOYPAD_MASK) }, 0);
    }

    #[test]
    fn a_small_picture_is_centred_in_the_buffer() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let frame = vec![0xffu8; 160 * 144 * 4];
        unsafe { video_refresh(frame.as_ptr() as *const c_void, 160, 144, 160 * 4) };

        let lit =
            |x: usize, y: usize| unsafe { with_host(|h| h.video[(y * 240 + x) * 4]) }.unwrap() != 0;
        assert!(lit(40, 8), "the top left of the picture is not at (40, 8)");
        assert!(lit(199, 151), "the bottom right is not at (199, 151)");
        assert!(!lit(39, 8), "the picture starts one column too early");
        assert!(!lit(40, 7), "the picture starts one row too early");
    }

    #[test]
    fn the_margin_is_cleared_when_the_picture_shrinks() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        unsafe {
            let full = vec![0xffu8; 240 * 160 * 4];
            video_refresh(full.as_ptr() as *const c_void, 240, 160, 240 * 4);
            let small = vec![0x11u8; 160 * 144 * 4];
            video_refresh(small.as_ptr() as *const c_void, 160, 144, 160 * 4);
        }
        let corner = unsafe { with_host(|h| h.video[0]) }.unwrap();
        assert_eq!(corner, 0, "the previous picture is still in the margin");
    }

    fn gradient(w: usize, h: usize) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                px[o] = x as u8;
                px[o + 1] = y as u8;
                px[o + 2] = (x ^ y) as u8;
            }
        }
        px
    }

    #[test]
    fn a_full_size_picture_is_unmoved() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let frame = gradient(GBA_W as usize, GBA_H as usize);
        unsafe {
            video_refresh(
                frame.as_ptr() as *const c_void,
                GBA_W,
                GBA_H,
                GBA_W as usize * 4,
            )
        };

        assert_eq!(unsafe { with_host(|h| h.video.clone()) }.unwrap(), frame);
    }

    #[test]
    fn get_can_dupe_tells_a_core_the_frontend_keeps_the_last_frame() {
        assert_eq!(GET_CAN_DUPE, 3);

        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);

        let mut can_dupe = false;
        let ok = unsafe { environment(GET_CAN_DUPE, &mut can_dupe as *mut bool as *mut c_void) };

        assert!(ok, "a core that asks has to be answered, not ignored");
        assert!(
            can_dupe,
            "the frontend does keep the last frame, so it must say so"
        );
    }

    #[test]
    fn get_can_dupe_refuses_a_null_pointer() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);

        assert!(!unsafe { environment(GET_CAN_DUPE, ptr::null_mut()) });
    }

    #[test]
    fn a_duplicate_frame_leaves_the_previous_picture_exactly_as_it_was() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let frame = gradient(GBA_W as usize, GBA_H as usize);
        unsafe {
            video_refresh(
                frame.as_ptr() as *const c_void,
                GBA_W,
                GBA_H,
                GBA_W as usize * 4,
            );
            video_refresh(ptr::null(), GBA_W, GBA_H, GBA_W as usize * 4);
        }

        assert_eq!(
            unsafe { with_host(|h| h.video.clone()) }.unwrap(),
            frame,
            "a duplicate frame changed the picture instead of keeping it"
        );
    }

    #[test]
    fn a_duplicate_frame_keeps_a_centred_game_boy_picture_where_it_is() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let frame = gradient(160, 144);
        unsafe { video_refresh(frame.as_ptr() as *const c_void, 160, 144, 160 * 4) };
        let drawn = unsafe { with_host(|h| h.video.clone()) }.unwrap();

        unsafe {
            video_refresh(ptr::null(), 160, 144, 160 * 4);
            video_refresh(ptr::null(), 160, 144, 160 * 4);
        }

        let kept = unsafe { with_host(|h| h.video.clone()) }.unwrap();
        assert_eq!(kept, drawn, "the picture did not survive two duplicates");
        let lit = (58 * GBA_W as usize + 140) * 4;
        assert_eq!(
            &kept[lit..lit + 3],
            &[100u8, 50, 100 ^ 50],
            "the picture was blanked or moved where a duplicate should have kept it"
        );
    }

    #[test]
    fn a_native_canvas_takes_each_frame_at_its_own_size() {
        let mut host = host_with(HashMap::new(), false);
        host.canvas = Canvas::Native;
        let _active = Active::bind(&mut host);
        let frame = gradient(256, 224);
        unsafe { video_refresh(frame.as_ptr() as *const c_void, 256, 224, 256 * 4) };

        let (size, video) = unsafe { with_host(|h| (h.size, h.video.clone())) }.unwrap();
        assert_eq!(size, (256, 224));
        assert_eq!(
            video, frame,
            "the picture was moved or cropped on a native canvas"
        );
    }

    #[test]
    fn a_native_canvas_follows_a_resolution_change() {
        let mut host = host_with(HashMap::new(), false);
        host.canvas = Canvas::Native;
        let _active = Active::bind(&mut host);
        let low = gradient(320, 240);
        let high = gradient(640, 480);
        unsafe {
            video_refresh(low.as_ptr() as *const c_void, 320, 240, 320 * 4);
            video_refresh(high.as_ptr() as *const c_void, 640, 480, 640 * 4);
        }

        let (size, video) = unsafe { with_host(|h| (h.size, h.video.clone())) }.unwrap();
        assert_eq!(size, (640, 480));
        assert_eq!(video, high);
    }

    #[test]
    fn a_native_canvas_reads_rows_by_the_pitch_not_the_width() {
        let mut host = host_with(HashMap::new(), false);
        host.canvas = Canvas::Native;
        let _active = Active::bind(&mut host);
        // Two rows of two pixels, each row padded out to four pixels of pitch.
        let mut frame = [0u8; 4 * 4 * 2];
        frame[..8].copy_from_slice(&[1, 2, 3, 0, 4, 5, 6, 0]);
        frame[16..24].copy_from_slice(&[7, 8, 9, 0, 10, 11, 12, 0]);
        unsafe { video_refresh(frame.as_ptr() as *const c_void, 2, 2, 16) };

        let video = unsafe { with_host(|h| h.video.clone()) }.unwrap();
        assert_eq!(video, [1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0, 10, 11, 12, 0]);
    }

    #[test]
    fn rgb1555_comes_out_as_xrgb8888() {
        let mut host = host_with(HashMap::new(), false);
        host.format = PixelFormat::Rgb1555;
        host.canvas = Canvas::Native;
        let _active = Active::bind(&mut host);
        // Pure red, pure green, pure blue, white.
        let px: [u16; 4] = [0x7c00, 0x03e0, 0x001f, 0x7fff];
        unsafe { video_refresh(px.as_ptr() as *const c_void, 4, 1, 8) };

        let video = unsafe { with_host(|h| h.video.clone()) }.unwrap();
        assert_eq!(
            video,
            [0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
            "0RGB1555 did not come out as XRGB8888 in BGRA byte order"
        );
    }

    #[test]
    fn set_pixel_format_accepts_0rgb1555() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let mut want = PIXEL_FORMAT_0RGB1555;
        let ok = unsafe { environment(SET_PIXEL_FORMAT, &mut want as *mut c_uint as *mut c_void) };
        assert!(ok);
        assert!(unsafe { with_host(|h| h.format == PixelFormat::Rgb1555) }.unwrap());
    }

    #[test]
    fn set_geometry_updates_the_aspect() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let mut g = GameGeometry {
            base_width: 256,
            base_height: 224,
            max_width: 512,
            max_height: 448,
            aspect_ratio: 4.0 / 3.0,
        };
        let ok = unsafe { environment(SET_GEOMETRY, &mut g as *mut GameGeometry as *mut c_void) };
        assert!(ok);
        assert_eq!(unsafe { with_host(|h| h.aspect) }.unwrap(), 4.0 / 3.0);
    }

    #[test]
    fn an_unset_aspect_falls_back_to_the_base_size() {
        let g = GameGeometry {
            base_width: 320,
            base_height: 240,
            ..GameGeometry::default()
        };
        assert_eq!(aspect_of(&g), 320.0 / 240.0);
    }

    #[test]
    fn set_system_av_info_updates_timing_and_aspect() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let mut av = SystemAvInfo {
            geometry: GameGeometry {
                base_width: 320,
                base_height: 240,
                aspect_ratio: 0.0,
                ..GameGeometry::default()
            },
            timing: SystemTiming {
                fps: 50.0,
                sample_rate: 44_100.0,
            },
        };
        let ok = unsafe {
            environment(
                SET_SYSTEM_AV_INFO,
                &mut av as *mut SystemAvInfo as *mut c_void,
            )
        };
        assert!(ok);
        let (timing, aspect) = unsafe { with_host(|h| (h.timing, h.aspect)) }.unwrap();
        assert_eq!(
            timing,
            AvInfo {
                fps: 50.0,
                sample_rate: 44_100.0
            }
        );
        assert_eq!(aspect, 4.0 / 3.0);
    }

    #[test]
    fn input_state_reads_each_stick_axis_for_its_port() {
        let mut host = host_with(HashMap::new(), false);
        host.sticks = [[100, -200, 300, -400], [1, 2, 3, 4]];
        let _active = Active::bind(&mut host);

        let axis = |port, index, id| unsafe { input_state(port, DEVICE_ANALOG, index, id) };
        assert_eq!(axis(0, 0, 0), 100, "left x");
        assert_eq!(axis(0, 0, 1), -200, "left y");
        assert_eq!(axis(0, 1, 0), 300, "right x");
        assert_eq!(axis(0, 1, 1), -400, "right y");
        assert_eq!(axis(1, 1, 1), 4, "port 1 read port 0's sticks");
        assert_eq!(axis(0, 0, 2), 0, "an axis past y must read as rest");
    }

    #[test]
    fn input_state_reads_a_held_button_as_full_analog_pressure() {
        let mut host = host_with(HashMap::new(), false);
        host.inputs = [ButtonMask::R2, 0];
        let _active = Active::bind(&mut host);

        let press = |id| unsafe { input_state(0, DEVICE_ANALOG, DEVICE_INDEX_ANALOG_BUTTON, id) };
        assert_eq!(press(13), i16::MAX, "R2 is held");
        assert_eq!(press(12), 0, "L2 is not");
    }

    #[test]
    fn input_state_ignores_a_joypad_subclass() {
        let mut host = host_with(HashMap::new(), false);
        host.inputs = [ButtonMask::A, 0];
        let _active = Active::bind(&mut host);
        let subclassed = DEVICE_JOYPAD | (1 << 8);
        assert_eq!(unsafe { input_state(0, subclassed, 0, 8) }, 1);
    }

    #[test]
    fn get_input_bitmasks_says_yes() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        assert!(unsafe { environment(GET_INPUT_BITMASKS, ptr::null_mut()) });
    }

    thread_local! {
        static TEST_DISK_CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    unsafe extern "C" fn test_eject(open: bool) -> bool {
        TEST_DISK_CALLS.with(|c| c.borrow_mut().push(format!("eject {open}")));
        true
    }

    unsafe extern "C" fn test_set_index(index: c_uint) -> bool {
        TEST_DISK_CALLS.with(|c| c.borrow_mut().push(format!("index {index}")));
        index < 3
    }

    unsafe extern "C" fn test_count() -> c_uint {
        3
    }

    #[test]
    fn the_basic_disk_interface_is_kept_without_reading_past_it() {
        let mut host = host_with(HashMap::new(), false);
        let mut basic = DiskControlBasic {
            set_eject_state: Some(test_eject),
            get_eject_state: None,
            get_image_index: None,
            set_image_index: Some(test_set_index),
            get_num_images: Some(test_count),
            replace_image_index: None,
            add_image_index: None,
        };
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe {
                environment(
                    SET_DISK_CONTROL_INTERFACE,
                    &mut basic as *mut DiskControlBasic as *mut c_void,
                )
            }
        };
        assert!(ok);
        let disk = host.disk.expect("the interface was not stored");
        assert!(disk.get_num_images.is_some());
        assert!(
            disk.get_image_label.is_none(),
            "the ext-only fields must stay empty for the basic interface"
        );
    }

    #[test]
    fn get_disk_control_interface_version_offers_the_ext_interface() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let mut version: c_uint = 0;
        let ok = unsafe {
            environment(
                GET_DISK_CONTROL_INTERFACE_VERSION,
                &mut version as *mut c_uint as *mut c_void,
            )
        };
        assert!(ok);
        assert_eq!(version, 1);
    }

    #[test]
    fn swapping_a_disk_opens_the_tray_changes_it_and_closes_it() {
        TEST_DISK_CALLS.with(|c| c.borrow_mut().clear());
        let disk = DiskControl {
            set_eject_state: Some(test_eject),
            set_image_index: Some(test_set_index),
            ..DiskControl::default()
        };
        assert_eq!(unsafe { swap_disk(&disk, 1) }, Some(true));
        TEST_DISK_CALLS.with(|c| {
            assert_eq!(
                c.borrow().as_slice(),
                ["eject true", "index 1", "eject false"]
            );
        });
    }

    #[test]
    fn a_refused_disk_leaves_the_tray_open() {
        TEST_DISK_CALLS.with(|c| c.borrow_mut().clear());
        let disk = DiskControl {
            set_eject_state: Some(test_eject),
            set_image_index: Some(test_set_index),
            ..DiskControl::default()
        };
        assert_eq!(unsafe { swap_disk(&disk, 7) }, Some(false));
        TEST_DISK_CALLS.with(|c| {
            assert_eq!(c.borrow().as_slice(), ["eject true", "index 7"]);
        });
    }

    #[test]
    fn swapping_needs_both_the_tray_and_the_index() {
        let disk = DiskControl {
            set_image_index: Some(test_set_index),
            ..DiskControl::default()
        };
        assert_eq!(unsafe { swap_disk(&disk, 0) }, None);
    }
}
