use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use slot_input::{Btn, InputSource, Millis, RawEvent};
use slot_retro::Sticks;

use super::evdev::{
    decode, device_name, pick_devices, to_raw, Ev, Hat, EVENT_BYTES, EV_ABS, EV_KEY, EV_SYN,
};
use super::stick::{layout, Range, StickNode};
use super::trace;

const DEV: &str = "/dev/input";
const SYS: &str = "/sys/class/input";
const PSY: &str = "/sys/class/power_supply";

const LID_POLL_MS: Millis = 150;

pub struct DeviceInput {
    pending: Arc<Mutex<Vec<RawEvent>>>,
    sticks: Arc<Mutex<Sticks>>,
    hall: Option<PathBuf>,
    lid_shut: Option<bool>,
    next_lid_poll: Millis,
    menu_dpad_display: bool,
}

impl DeviceInput {
    pub fn open(root: &Path) -> Self {
        DeviceInput::open_in(Path::new(DEV), Path::new(SYS), root, trace::enabled())
    }

    pub fn open_in(dev: &Path, sys: &Path, root: &Path, trace: bool) -> Self {
        let trace = trace.then(|| Trace::start(root, dev, sys)).flatten();
        let target = crate::baseos::var("BASEOS_TARGET");
        DeviceInput::open_traced(
            dev,
            sys,
            menu_dpad_display(target.as_deref()),
            right_stick(target.as_deref()),
            trace,
        )
    }

    fn open_traced(
        dev: &Path,
        sys: &Path,
        menu_dpad_display: bool,
        right_stick: bool,
        trace: Option<Arc<Trace>>,
    ) -> Self {
        let pending: Arc<Mutex<Vec<RawEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sticks: Arc<Mutex<Sticks>> = Arc::new(Mutex::new([0; 4]));
        for node in pick_devices(dev, sys) {
            eprintln!(
                "slot: input {} ({})",
                node.display(),
                device_name(sys, &node)
            );
            quicken_poll(sys, &node);
            let axes = stick_axes(sys, &node);
            if let Some(axes) = axes {
                eprintln!("slot: input {}: sticks on {axes:?}", node.display());
            }
            let queue = pending.clone();
            let out = sticks.clone();
            let name = node.display().to_string();
            let trace = trace.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("slot-input-{name}"))
                .spawn(move || {
                    let sticks = axes.map(|axes| (axes, right_stick, out));
                    read_node(&node, &queue, sticks, trace.as_deref())
                });
            if let Err(e) = spawned {
                eprintln!("slot: input {name}: {e}");
            }
        }
        DeviceInput {
            pending,
            sticks,
            hall: find_hall(Path::new(PSY)),
            lid_shut: None,
            next_lid_poll: 0,
            menu_dpad_display,
        }
    }
}

const POLL_MS: u32 = 10;

fn quicken_poll(sys: &Path, node: &Path) {
    let Some(name) = node.file_name() else {
        return;
    };
    let attr = sys.join(name).join("device/poll");
    let Some(now) = std::fs::read_to_string(&attr)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    else {
        return;
    };
    if now > POLL_MS {
        match std::fs::write(&attr, POLL_MS.to_string()) {
            Ok(()) => eprintln!(
                "slot: input {}: poll {now} ms -> {POLL_MS} ms",
                node.display()
            ),
            Err(e) => eprintln!("slot: input {}: poll: {e}", node.display()),
        }
    }
}

/// The axes a node carries its sticks on, if it has any.
fn stick_axes(sys: &Path, node: &Path) -> Option<[u16; 4]> {
    let name = node.file_name()?;
    let abs = std::fs::read_to_string(sys.join(name).join("device/capabilities/abs")).ok()?;
    layout(&abs)
}

type StickOut = ([u16; 4], bool, Arc<Mutex<Sticks>>);

fn read_node(
    node: &Path,
    queue: &Mutex<Vec<RawEvent>>,
    sticks: Option<StickOut>,
    trace: Option<&Trace>,
) {
    let label = node
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| node.display().to_string());
    let mut hat = Hat::default();
    let mut file = match File::open(node) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("slot: input {}: {e}", node.display());
            return;
        }
    };
    let mut sticks = sticks.map(|(axes, right, out)| {
        let ranges = axes.map(|code| {
            let range = abs_range(&file, code);
            eprintln!("slot: input {}: axis {code:#x} {range:?}", node.display());
            range
        });
        (StickNode::new(axes, ranges, right), out)
    });
    let mut buf = [0u8; EVENT_BYTES * 16];
    loop {
        let read = match file.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                eprintln!("slot: input {}: {e}", node.display());
                return;
            }
        };
        let mut events = Vec::new();
        for ev in buf[..read].chunks_exact(EVENT_BYTES).filter_map(decode) {
            if let Some(trace) = trace {
                trace.event(&label, ev);
            }
            if (ev.kind == EV_KEY && ev.value == 1) || (ev.kind == EV_ABS && ev.value != 0) {
                crate::latency::read();
            }
            if let Some((node, out)) = sticks.as_mut() {
                if node.feed(ev) {
                    *out.lock().unwrap_or_else(|e| e.into_inner()) = node.values();
                    continue;
                }
            }
            match ev.kind {
                EV_ABS => events.extend(hat.feed(ev)),
                _ => events.extend(to_raw(ev)),
            }
        }
        if events.is_empty() {
            continue;
        }
        queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(events);
    }
}

struct Trace {
    out: Mutex<File>,
    began: Instant,
}

impl Trace {
    fn start(root: &Path, dev: &Path, sys: &Path) -> Option<Arc<Trace>> {
        let dir = crate::root::logs_dir(root);
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(trace::TRACE_FILE);
        let mut file = match File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("slot: {}: {e}", path.display());
                return None;
            }
        };
        for line in trace::survey(dev, sys) {
            let _ = writeln!(file, "{line}");
        }
        let _ = writeln!(file, "--");
        let _ = file.flush();
        Some(Arc::new(Trace {
            out: Mutex::new(file),
            began: Instant::now(),
        }))
    }

    fn event(&self, node: &str, ev: Ev) {
        if ev.kind == EV_SYN {
            return;
        }
        let line = trace::event_line(node, self.began.elapsed().as_millis(), ev);
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

impl DeviceInput {
    fn read_lid(&self) -> Option<bool> {
        let raw = std::fs::read_to_string(self.hall.as_ref()?).ok()?;
        match raw.trim() {
            "0" => Some(true),
            "1" => Some(false),
            _ => None,
        }
    }
}

impl InputSource for DeviceInput {
    fn poll(&mut self, now: Millis) -> Vec<RawEvent> {
        let mut out = std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()));
        if self.hall.is_some() && now >= self.next_lid_poll {
            self.next_lid_poll = now + LID_POLL_MS;
            if let Some(shut) = self.read_lid() {
                if self.lid_shut != Some(shut) {
                    self.lid_shut = Some(shut);
                    out.push(if shut {
                        RawEvent::Down(Btn::Lid)
                    } else {
                        RawEvent::Up(Btn::Lid)
                    });
                }
            }
        }
        out
    }

    fn menu_dpad_display(&self) -> bool {
        self.menu_dpad_display
    }

    fn sticks(&self) -> [i16; 4] {
        *self.sticks.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// An axis's range from the kernel, and where it reads now. ROCKNIX's driver reports ±1800, so
/// that is the guess for a kernel that will not say.
fn abs_range(file: &File, code: u16) -> Range {
    match absinfo(file, code) {
        Some((value, min, max)) if max > min => Range::new(min, max, value),
        _ => Range::new(-1800, 1800, 0),
    }
}

/// `EVIOCGABS`: the axis's value, minimum and maximum.
#[cfg(unix)]
fn absinfo(file: &File, code: u16) -> Option<(i32, i32, i32)> {
    use std::os::unix::io::AsRawFd;
    // _IOR('E', 0x40 + code, struct input_absinfo), six i32s.
    let request = 0x8018_4540u64 + u64::from(code);
    let mut info = [0i32; 6];
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), request as _, info.as_mut_ptr()) };
    (rc == 0).then_some((info[0], info[1], info[2]))
}

#[cfg(not(unix))]
fn absinfo(_file: &File, _code: u16) -> Option<(i32, i32, i32)> {
    None
}

/// Whether the model has a second stick. The RG40XX V has only the left one; its right channels
/// read nothing worth passing on.
fn right_stick(target: Option<&str>) -> bool {
    target.map(str::trim) != Some("rg40xxv")
}

fn find_hall(psy: &Path) -> Option<PathBuf> {
    let mut supplies: Vec<PathBuf> = std::fs::read_dir(psy)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    supplies.sort();
    supplies
        .into_iter()
        .map(|d| d.join("hallkey"))
        .find(|p| p.is_file())
}

fn menu_dpad_display(target: Option<&str>) -> bool {
    target.map(str::trim) == Some("rg34xx")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_one_stick_rg40xx_v_leaves_its_right_stick_at_rest() {
        assert!(!right_stick(Some("rg40xxv")));
        for target in [Some("rg40xxh"), Some("rg34xxsp"), Some("rgcubexx"), None] {
            assert!(right_stick(target), "{target:?}");
        }
    }

    #[test]
    fn only_the_rg34xx_baseos_target_enables_the_menu_dpad_display_gesture() {
        for (target, want) in [
            (Some("rg34xx"), true),
            (Some("rg34xxsp"), false),
            (Some("rg35xxsp"), false),
            (Some("rg35xxh"), false),
            (Some(""), false),
            (None, false),
        ] {
            assert_eq!(menu_dpad_display(target), want, "{target:?}");
        }
    }
}
