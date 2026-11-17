//! Which CPUs a Dreamcast game's threads run on, on the A133P handhelds, whose four Cortex-A53s
//! the scheduler otherwise shares out as it likes.
//!
//! A trace of Crazy Taxi 2 on the Brick Pro had Nodosumi's emu thread, which works about 13 ms of
//! each 16.7 ms frame, on all four CPUs and switched out about 170 times a second. Its frames on
//! CPU 0, which takes the interrupts, were the slowest: 1.1 ms more in the SH4 than the geometry
//! they drew accounts for, and 1.4 ms off the CPU, against 0.2 ms on CPU 2. Half the frames
//! before a dip ran there, against a fifth of the rest. So while Nodosumi runs in its core host:
//!
//! - the last CPU is its emu thread's,
//! - the one before that is the core host's main thread's, which draws the frames,
//! - the others take everything else: slot's own threads and the core's other ones.
//!
//! Threads are told apart by name. One whose CPUs or nice can't be set is left as it was.
//! `SLOT_CPUS=off` leaves them all to the scheduler.

use slot_store::Core;

/// The CPUs each kind of thread gets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub emu: usize,
    pub video: usize,
    pub rest: Vec<usize>,
}

/// What a thread is to the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Emu,
    Video,
    Rest,
}

impl Plan {
    /// The plan for the CPUs online, in order; none with fewer than four, where there are too
    /// few to keep two to themselves.
    pub fn for_cpus(online: &[usize]) -> Option<Plan> {
        let [rest @ .., video, emu] = online else {
            return None;
        };
        if rest.len() < 2 {
            return None;
        }
        Some(Plan {
            emu: *emu,
            video: *video,
            rest: rest.to_vec(),
        })
    }

    pub fn cpus(&self, place: Place) -> Vec<usize> {
        match place {
            Place::Emu => vec![self.emu],
            Place::Video => vec![self.video],
            Place::Rest => self.rest.clone(),
        }
    }

    pub fn all(&self) -> Vec<usize> {
        let mut all = self.rest.clone();
        all.extend([self.video, self.emu]);
        all
    }
}

/// The CPUs a list like /sys/devices/system/cpu/online's names: "0-3", "0,2-3".
pub fn parse_cpu_list(text: &str) -> Vec<usize> {
    let mut cpus = Vec::new();
    for part in text.trim().split(',').filter(|p| !p.is_empty()) {
        let (lo, hi) = part.split_once('-').unwrap_or((part, part));
        if let (Ok(lo), Ok(hi)) = (lo.trim().parse::<usize>(), hi.trim().parse::<usize>()) {
            cpus.extend(lo..=hi);
        }
    }
    cpus
}

/// Where a thread of the core host goes, and the nice it takes (None leaves its own). The main
/// thread runs the core's frames and draws them. The emu thread gives way to nothing else on its
/// CPU, and the CHD reader nearly so, as the emu thread waits on it when it falls behind. Trace
/// writers and the thread placing these give way to everything.
pub fn host_thread(main: bool, comm: &str) -> (Place, Option<i32>) {
    match comm {
        _ if main => (Place::Video, Some(-5)),
        "emu" => (Place::Emu, Some(-10)),
        "chd" => (Place::Rest, Some(-5)),
        "nodo-ftrace" | "slot-ftrace" | "slot-cpus" => (Place::Rest, Some(10)),
        _ => (Place::Rest, None),
    }
}

/// The nice a thread of slot's own takes while the game runs; every one keeps to the rest of the
/// CPUs. The sound output asks little of the CPU and must never wait for it.
pub fn slot_thread_nice(comm: &str) -> Option<i32> {
    match comm {
        "slot-audio" => Some(-10),
        "slot-ftrace" => Some(10),
        _ => None,
    }
}

/// The plan for `core` on this handheld, if it has one.
pub fn plan(core: Core) -> Option<Plan> {
    if crate::pak::CHIP != "a133p" || core != Core::Nodosumi {
        return None;
    }
    if std::env::var("SLOT_CPUS").is_ok_and(|v| v == "off") {
        return None;
    }
    let online = std::fs::read_to_string("/sys/devices/system/cpu/online").ok()?;
    Plan::for_cpus(&parse_cpu_list(&online))
}

#[cfg(target_os = "linux")]
pub use os::{confine_slot, place_host, Confined};

#[cfg(target_os = "linux")]
mod os {
    use std::collections::HashMap;
    use std::io;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::{host_thread, slot_thread_nice, Place, Plan};

    /// The priority the emu thread runs at.
    const RT_PRIO: i32 = 10;

    /// How many games have slot's threads confined, and the nices they had before the first, so
    /// the last game to end puts them back.
    static CONFINED: Mutex<(usize, Vec<(i32, i32)>)> = Mutex::new((0, Vec::new()));

    fn set_affinity(tid: i32, cpus: &[usize]) -> io::Result<()> {
        // SAFETY: set is a plain bitmask, zeroed and filled here, and its size is passed along.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            for &cpu in cpus {
                libc::CPU_SET(cpu, &mut set);
            }
            if libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn nice(tid: i32) -> io::Result<i32> {
        // getpriority returns -1 for a nice of -1 too, so errno tells.
        // SAFETY: errno is this thread's, and getpriority only reads.
        unsafe {
            *libc::__errno_location() = 0;
            let n = libc::getpriority(libc::PRIO_PROCESS, tid as libc::id_t);
            match io::Error::last_os_error() {
                e if e.raw_os_error().unwrap_or(0) != 0 => Err(e),
                _ => Ok(n),
            }
        }
    }

    /// Gives thread `tid` the real-time round robin scheduler at `prio`, so the kernel's workers
    /// and the firmware's daemons on its CPU wait for it rather than it for them.
    fn set_rt(tid: i32, prio: i32) -> io::Result<()> {
        // SAFETY: a plain sched_param with only the field its policy reads set.
        unsafe {
            let param = libc::sched_param {
                sched_priority: prio,
            };
            match libc::sched_setscheduler(tid, libc::SCHED_RR, &param) {
                0 => Ok(()),
                _ => Err(io::Error::last_os_error()),
            }
        }
    }

    fn set_nice(tid: i32, nice: i32) -> io::Result<()> {
        // SAFETY: a plain call; Linux keeps a nice for each thread, by its id.
        match unsafe { libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, nice) } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// This process's threads, by id and name.
    fn threads() -> Vec<(i32, String)> {
        let Ok(dir) = std::fs::read_dir("/proc/self/task") else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|e| {
                let tid = e.file_name().to_str()?.parse().ok()?;
                let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
                Some((tid, comm.trim_end().to_string()))
            })
            .collect()
    }

    /// Puts thread `tid` on `cpus` and, with `nice`, gives it that, noting its old one in
    /// `niced`. What fails is said and left.
    fn place(
        tid: i32,
        comm: &str,
        cpus: &[usize],
        nice_to: Option<i32>,
        niced: &mut Vec<(i32, i32)>,
    ) {
        if let Err(e) = set_affinity(tid, cpus) {
            eprintln!("slot: cpus: {comm} ({tid}) stays where it was: {e}");
        }
        let Some(to) = nice_to else {
            return;
        };
        let was = nice(tid).unwrap_or(0);
        match set_nice(tid, to) {
            Ok(()) => niced.push((tid, was)),
            Err(e) => eprintln!("slot: cpus: {comm} ({tid}) keeps its nice: {e}"),
        }
    }

    fn list(cpus: &[usize]) -> String {
        match (cpus.first(), cpus.last()) {
            (Some(a), Some(b)) if cpus.len() > 1 && b - a + 1 == cpus.len() => format!("{a}-{b}"),
            _ => cpus
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(","),
        }
    }

    /// Slot's threads, on the rest of the CPUs until it is dropped.
    pub struct Confined {
        all: Vec<usize>,
    }

    /// Keeps every thread slot has, and so every one they start, to the plan's rest of the CPUs,
    /// the core host it starts next included, until the game ends and this is dropped.
    pub fn confine_slot(plan: &Plan) -> Confined {
        let mut confined = CONFINED.lock().unwrap_or_else(|e| e.into_inner());
        let (games, niced) = &mut *confined;
        if *games == 0 {
            eprintln!(
                "slot: cpus: Nodosumi's emu thread on CPU {}, its frames on {}, the rest on {}",
                plan.emu,
                plan.video,
                list(&plan.rest)
            );
        }
        for (tid, comm) in threads() {
            // A game already running has given them their nices.
            let nice_to = slot_thread_nice(&comm).filter(|_| *games == 0);
            place(tid, &comm, &plan.rest, nice_to, niced);
        }
        *games += 1;
        Confined { all: plan.all() }
    }

    impl Drop for Confined {
        fn drop(&mut self) {
            let mut confined = CONFINED.lock().unwrap_or_else(|e| e.into_inner());
            let (games, niced) = &mut *confined;
            *games -= 1;
            if *games > 0 {
                return;
            }
            for (tid, _) in threads() {
                let _ = set_affinity(tid, &self.all);
            }
            for (tid, was) in niced.drain(..) {
                let _ = set_nice(tid, was);
            }
        }
    }

    /// Places the core host's threads by the plan as the core starts them: often while it
    /// loads, then once in a while for any it starts later. A thread is placed again if it is
    /// named after it was first seen.
    pub fn place_host(plan: Plan) {
        let spawned = std::thread::Builder::new()
            .name("slot-cpus".into())
            .spawn(move || {
                let pid = std::process::id() as i32;
                let start = Instant::now();
                let mut placed: HashMap<i32, String> = HashMap::new();
                loop {
                    let now = threads();
                    for (tid, comm) in &now {
                        if placed.get(tid) == Some(comm) {
                            continue;
                        }
                        let (at, nice_to) = host_thread(*tid == pid, comm);
                        place(*tid, comm, &plan.cpus(at), nice_to, &mut Vec::new());
                        if at == Place::Emu {
                            // Prefer the emulator to normal-policy work on its CPU. Higher-priority
                            // real-time threads and the kernel's RT runtime limit still apply.
                            if let Err(e) = set_rt(*tid, RT_PRIO) {
                                eprintln!("slot: cpus: {comm} ({tid}) keeps its scheduling: {e}");
                            }
                        }
                        placed.insert(*tid, comm.clone());
                    }
                    placed.retain(|tid, _| now.iter().any(|(t, _)| t == tid));
                    let wait = if start.elapsed() < Duration::from_secs(30) {
                        500
                    } else {
                        5000
                    };
                    std::thread::sleep(Duration::from_millis(wait));
                }
            });
        if let Err(e) = spawned {
            eprintln!("slot: cpus: the core's threads are left to the scheduler: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists_read_as_sysfs_writes_them() {
        assert_eq!(parse_cpu_list("0-3\n"), vec![0, 1, 2, 3]);
        assert_eq!(parse_cpu_list("0,2-3"), vec![0, 2, 3]);
        assert_eq!(parse_cpu_list("0"), vec![0]);
        assert_eq!(parse_cpu_list(""), Vec::<usize>::new());
    }

    #[test]
    fn four_cpus_give_the_emu_the_last_and_the_frames_the_one_before() {
        let plan = Plan::for_cpus(&[0, 1, 2, 3]).unwrap();
        assert_eq!(plan.emu, 3);
        assert_eq!(plan.video, 2);
        assert_eq!(plan.rest, vec![0, 1]);
        assert_eq!(plan.cpus(Place::Rest), vec![0, 1]);
        assert_eq!(plan.all(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn fewer_than_four_cpus_have_no_plan() {
        assert_eq!(Plan::for_cpus(&[0, 1, 2]), None);
        assert_eq!(Plan::for_cpus(&[0]), None);
        assert_eq!(Plan::for_cpus(&[]), None);
    }

    #[test]
    fn a_cpu_taken_offline_moves_the_plan_to_those_left() {
        let plan = Plan::for_cpus(&[0, 1, 3, 4]).unwrap();
        assert_eq!((plan.emu, plan.video, plan.rest), (4, 3, vec![0, 1]));
    }

    #[test]
    fn core_host_threads_are_told_apart_by_name() {
        assert_eq!(host_thread(true, "ld-2.33.so"), (Place::Video, Some(-5)));
        assert_eq!(host_thread(false, "emu"), (Place::Emu, Some(-10)));
        assert_eq!(host_thread(false, "chd"), (Place::Rest, Some(-5)));
        assert_eq!(host_thread(false, "conv"), (Place::Rest, None));
        assert_eq!(host_thread(false, "nodo-ftrace"), (Place::Rest, Some(10)));
        // The emu thread before Nodosumi has named it is placed as the rest, then again.
        assert_eq!(host_thread(false, "ld-2.33.so"), (Place::Rest, None));
    }

    #[test]
    fn slot_gives_its_sound_output_the_cpu_first() {
        assert_eq!(slot_thread_nice("slot-audio"), Some(-10));
        assert_eq!(slot_thread_nice("slot-ftrace"), Some(10));
        assert_eq!(slot_thread_nice("slot-emu"), None);
    }

    #[test]
    fn only_nodosumi_on_the_a133p_has_a_plan() {
        assert_eq!(plan(Core::Flycast), None);
        // Host tests build for the H700 unless UNSLOT_CHIP says otherwise.
        if crate::pak::CHIP != "a133p" {
            assert_eq!(plan(Core::Nodosumi), None);
        }
    }
}
