use std::path::{Path, PathBuf};

use slot_retro::{Canvas, LibretroCore, MockCore, RetroCore};
use slot_store::{Core, GbPalette, Platform};

use crate::root;

const CORE_ENV: &str = "SLOT_CORE";

pub fn dylib_name(core: Core) -> String {
    core.file_name()
}

fn candidates(root: &Path, core: Core) -> Vec<PathBuf> {
    if let Some(named) = std::env::var_os(CORE_ENV) {
        return vec![PathBuf::from(named)];
    }
    let name = dylib_name(core);
    let mut paths = vec![
        root.join("System").join(&name),
        root.join(slot_store::CORES_DIR).join(&name),
    ];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        paths.push(dir.join(&name));
        paths.push(dir.join("vendor").join(&name));
    }
    paths.push(Path::new("vendor").join(&name));
    paths
}

pub struct Opened {
    pub core: Box<dyn RetroCore>,
    pub named: bool,
}

pub fn open_core(
    root: &Path,
    core: Core,
    platform: Platform,
    serial: &str,
    colour: bool,
    palette: Option<GbPalette>,
    link: Option<u8>,
) -> Opened {
    let paths = candidates(root, core);
    match open_named(root, core, platform, serial, colour, palette, link, &paths) {
        Some(core) => Opened { core, named: true },
        None => {
            report_missing(core, &paths);
            Opened {
                core: Box::new(MockCore::new()),
                named: false,
            }
        }
    }
}

pub fn open_core_for(
    root: &Path,
    core: Core,
    serial: &str,
    colour: bool,
    paths: &[PathBuf],
) -> Box<dyn RetroCore> {
    open_core_for_palette(root, core, Platform::Gba, serial, colour, None, paths)
}

pub fn open_core_for_palette(
    root: &Path,
    core: Core,
    platform: Platform,
    serial: &str,
    colour: bool,
    palette: Option<GbPalette>,
    paths: &[PathBuf],
) -> Box<dyn RetroCore> {
    open_named(root, core, platform, serial, colour, palette, None, paths).unwrap_or_else(|| {
        report_missing(core, paths);
        Box::new(MockCore::new())
    })
}

#[allow(clippy::too_many_arguments)]
fn open_named(
    root: &Path,
    core: Core,
    platform: Platform,
    serial: &str,
    colour: bool,
    palette: Option<GbPalette>,
    link: Option<u8>,
    paths: &[PathBuf],
) -> Option<Box<dyn RetroCore>> {
    #[cfg(feature = "device")]
    if matches!(core, Core::Flycast | Core::ParallelN64 | Core::Ppsspp)
        && palette.is_none()
        && link.is_none()
    {
        match open_remote(root, core, platform, serial, colour, paths) {
            Ok(remote) => return Some(remote),
            Err(e) => eprintln!(
                "slot: core host for {}: {e}, running it here",
                core.as_str()
            ),
        }
    }
    open_here(root, core, platform, serial, colour, palette, link, paths)
}

/// Opens a core that draws with the GPU in a process of its own, `slot --core-host`, which has
/// the GPU to itself where in slot's it would wait on slot's page flips (see slot_retro's
/// remote).
#[cfg(feature = "device")]
fn open_remote(
    root: &Path,
    core: Core,
    platform: Platform,
    serial: &str,
    colour: bool,
    paths: &[PathBuf],
) -> std::io::Result<Box<dyn RetroCore>> {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};

    let (ours, theirs) = UnixStream::pair()?;
    let exe = std::env::current_exe()?;
    // Run through the loader as slot is, it is the loader that is the exe.
    let mut cmd = match exe.file_name().and_then(|n| n.to_str()) {
        Some(n) if n.starts_with("ld-linux") => {
            let mut cmd = Command::new(&exe);
            cmd.arg(std::env::args_os().next().unwrap_or_default());
            cmd
        }
        _ => Command::new(&exe),
    };
    cmd.arg("--core-host")
        .arg(root)
        .arg(core.as_str())
        .arg(platform.key())
        .arg(serial)
        .arg(if colour { "1" } else { "0" })
        .args(paths)
        .stdin(Stdio::from(OwnedFd::from(theirs)));
    let child = cmd.spawn()?;
    let remote = slot_retro::RemoteCore::new(child, ours)?;
    eprintln!("slot: {} runs in a core host", core.as_str());
    Ok(Box::new(remote))
}

/// `slot --core-host ROOT CORE PLATFORM SERIAL COLOUR PATH...`: opens the core as slot would and
/// serves it over the socket on stdin until slot hangs up.
#[cfg(feature = "device")]
pub fn host(args: &[String]) -> Result<(), String> {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::net::UnixStream;

    let [root, core, platform, serial, colour, paths @ ..] = args else {
        return Err("--core-host needs a root, core, platform, serial, colour and paths".into());
    };
    let core = Core::parse(core).ok_or(format!("no core {core}"))?;
    let platform = Platform::from_key(platform).ok_or(format!("no platform {platform}"))?;
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    // Gone with slot, whatever it was doing.
    unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };
    // SAFETY: slot hands the socket over as stdin and nothing else here reads it.
    let stream = UnixStream::from(unsafe { OwnedFd::from_raw_fd(0) });
    let opened = open_here(
        Path::new(root),
        core,
        platform,
        serial,
        colour == "1",
        None,
        None,
        &paths,
    )
    .ok_or("no core opened")?;
    slot_retro::serve(opened, stream);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn open_here(
    root: &Path,
    core: Core,
    platform: Platform,
    serial: &str,
    colour: bool,
    palette: Option<GbPalette>,
    link: Option<u8>,
    paths: &[PathBuf],
) -> Option<Box<dyn RetroCore>> {
    crate::gpu::install();
    root::seed_core_files(root, core);
    let bios = root::bios_dir(root);
    let saves = root::saves_dir(root);
    for path in paths {
        if !path.exists() {
            continue;
        }
        match LibretroCore::open_with(path, &bios, &saves) {
            Ok(mut opened) => {
                if platform.spec().native {
                    opened.set_canvas(Canvas::Native);
                }
                apply_core_options(
                    &mut opened,
                    core,
                    platform,
                    serial,
                    root::has_real_bios(root),
                    colour,
                    palette,
                );
                for (key, value) in slot_store::ini::read(root, &options_file(core)) {
                    opened.set_option(&key, &value);
                }
                if let Some(player) = link {
                    apply_link_options(&mut opened, core, player);
                }
                eprintln!("slot: core {}", path.display());
                return Some(Box::new(opened));
            }
            Err(e) => eprintln!("slot: {}: {e}", path.display()),
        }
    }
    None
}

/// Where the options the player chose on the core options page are kept, one file a core, put
/// over slot's own each time it opens.
pub fn options_file(core: Core) -> String {
    format!(
        "{}/core_options/{}.txt",
        slot_store::CONFIG_DIR,
        core.as_str()
    )
}

pub fn palette_for(
    chosen: Option<GbPalette>,
    platform: Platform,
    rom: &Path,
    link: Option<u8>,
) -> Option<GbPalette> {
    chosen.filter(|_| link.is_none() && platform != Platform::Gba && dmg_only(rom))
}

pub fn dmg_only(rom: &Path) -> bool {
    use std::io::Read;
    let mut header = [0u8; 0x150];
    std::fs::File::open(rom)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_ok_and(|_| header[0x143] & 0x80 == 0)
}

fn report_missing(core: Core, paths: &[PathBuf]) {
    eprintln!(
        "slot: no {} core found, running the mock test pattern instead. Looked in: {}",
        core.as_str(),
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

pub fn colour_option(
    which: Core,
    platform: Platform,
    on: bool,
) -> Option<(&'static str, &'static str)> {
    match which {
        Core::Mgba => Some((
            "mgba_color_correction",
            if on && platform == Platform::Gb {
                "GBC"
            } else if on {
                "Auto"
            } else {
                "OFF"
            },
        )),
        Core::Gpsp => Some((
            "gpsp_color_correction",
            if on { "enabled" } else { "disabled" },
        )),
        _ => None,
    }
}

pub fn apply_link_options(core: &mut LibretroCore, which: Core, player: u8) {
    if which != Core::Mgba {
        return;
    }
    core.set_option("mgba_link", "on");
    core.set_option("mgba_link_player", &player.to_string());
    eprintln!("slot: core: link mode on, player {player}");
}

/// Settings for the cores past the Game Boy family that suit the H700's four Cortex-A53s and
/// Mali-G31, after the defaults Knulli and ROCKNIX give the same cores on the same chip. A core
/// that does not declare a key ignores it.
pub const H700_OPTIONS: &[(Core, &str, &str)] = &[
    (Core::ParallelN64, "parallel-n64-gfxplugin", "gln64"),
    (Core::ParallelN64, "parallel-n64-screensize", "320x240"),
    (
        Core::ParallelN64,
        "parallel-n64-cpucore",
        "dynamic_recompiler",
    ),
    (Core::ParallelN64, "parallel-n64-rspplugin", "hle"),
    (Core::ParallelN64, "parallel-n64-pak1", "memory"),
    (Core::ParallelN64, "parallel-n64-astick-deadzone", "15"),
    // Each frame a GPU core draws is read back for the CPU, which on the Mali costs 5 ms at
    // 320x240 and 29 at 640x480, too much of a frame to leave room for the Dreamcast. Its
    // threaded rendering is worth ten frames a second, and anisotropic filtering is GPU time a
    // 320x240 picture does not show. The rest is Flycast Xtreme's tuning, which muOS ships: two
    // frames skipped after each one drawn, the fast alpha sorting and the quick GD-ROM.
    (Core::Flycast, "reicast_internal_resolution", "320x240"),
    (Core::Flycast, "reicast_threaded_rendering", "enabled"),
    (Core::Flycast, "reicast_enable_dsp", "disabled"),
    (
        Core::Flycast,
        "reicast_alpha_sorting",
        "per-strip (fast, least accurate)",
    ),
    (Core::Flycast, "reicast_frame_skipping", "2"),
    (Core::Flycast, "reicast_gdrom_fast_loading", "enabled"),
    (Core::Flycast, "reicast_anisotropic_filtering", "off"),
    (Core::Flycast, "reicast_per_content_vmus", "All VMUs"),
    (Core::Flycast, "reicast_analog_stick_deadzone", "15%"),
    (Core::Ppsspp, "ppsspp_internal_resolution", "480x272"),
    (Core::Ppsspp, "ppsspp_cpu_core", "JIT"),
    (Core::Ppsspp, "ppsspp_fast_memory", "enabled"),
    (Core::Ppsspp, "ppsspp_auto_frameskip", "enabled"),
    (Core::Ppsspp, "ppsspp_skip_gpu_readbacks", "enabled"),
    (Core::Ppsspp, "ppsspp_texture_scaling_level", "disabled"),
    (Core::Ppsspp, "ppsspp_io_timing_method", "Fast"),
    (Core::Fbneo, "fbneo-frameskip-type", "disabled"),
    (Core::Fbneo, "fbneo-diagnostic-input", "Start + L + R"),
    (Core::Fbneo, "fbneo-memcard-mode", "per-game"),
    (Core::Fbneo, "fbneo-hiscores", "enabled"),
    (Core::Fbneo, "fbneo-samplerate", "44100"),
    (
        Core::Mame2003Plus,
        "mame2003-plus_skip_disclaimer",
        "enabled",
    ),
    (Core::Mame2003Plus, "mame2003-plus_skip_warnings", "enabled"),
    (Core::Mame2003Plus, "mame2003-plus_sample_rate", "44100"),
    (
        Core::Mame2003Plus,
        "mame2003-plus_input_interface",
        "retropad",
    ),
    (Core::Neocd, "neocd_per_content_saves", "On"),
    (Core::Picodrive, "picodrive_sprlim", "enabled"),
    (Core::Picodrive, "picodrive_input1", "6 button pad"),
    (Core::MednafenPcfx, "pcfx_nospritelimit", "enabled"),
    (Core::MednafenVb, "vb_color_mode", "black & red"),
    (Core::DosboxPure, "dosbox_pure_cycles", "auto"),
    (Core::DosboxPure, "dosbox_pure_cpu_core", "auto"),
    (Core::DosboxPure, "dosbox_pure_machine", "svga"),
    (Core::DosboxPure, "dosbox_pure_memory_size", "16"),
    (Core::DosboxPure, "dosbox_pure_auto_mapping", "true"),
    (Core::Puae2021, "puae_model", "auto"),
    (Core::Puae2021, "puae_cpu_compatibility", "normal"),
    (Core::Puae2021, "puae_video_resolution", "hires"),
    (Core::Puae2021, "puae_use_whdload_prefs", "config"),
    (Core::ViceX64, "vice_autoloadwarp", "enabled"),
    (Core::ViceX64, "vice_joyport", "2"),
    (Core::ViceX64, "vice_external_palette", "colodore"),
    (Core::Fuse, "fuse_machine", "Spectrum 128K"),
    (Core::Cap32, "cap32_model", "6128"),
    (Core::Cap32, "cap32_ram", "128"),
    (Core::Atari800, "atari800_system", "800XL (64K)"),
    (Core::Atari800, "atari800_sioaccel", "enabled"),
    (Core::Np2kai, "np2kai_model", "PC-9801VX"),
    (Core::Np2kai, "np2kai_clk_mult", "4"),
    (Core::Px68k, "px68k_cpuspeed", "33Mhz (OC)"),
    (Core::Px68k, "px68k_ramsize", "12MB"),
    (Core::Scummvm, "scummvm_analog_deadzone", "15"),
    (Core::Scummvm, "scummvm_speed_hack", "enabled"),
];

pub fn apply_core_options(
    core: &mut LibretroCore,
    which: Core,
    platform: Platform,
    serial: &str,
    bios: bool,
    colour: bool,
    palette: Option<GbPalette>,
) {
    for (_, key, value) in H700_OPTIONS.iter().filter(|(c, ..)| *c == which) {
        core.set_option(key, value);
    }
    // The two GBA cores read the audio buffer's state to skip frames; the rest have no such key.
    if matches!(which, Core::Mgba | Core::Gpsp) {
        core.set_option(&format!("{}_frameskip", which.as_str()), "auto");
    }
    if which == Core::Mgba {
        core.set_option("mgba_sgb_borders", "OFF");
        core.set_option("mgba_force_gbp", "ON");
        match palette {
            Some(p) => {
                core.set_option("mgba_gb_model", "Game Boy");
                core.set_option("mgba_gb_colors_preset", "0");
                core.set_option("mgba_gb_colors", p.core_name());
            }
            None => {
                core.set_option("mgba_gb_model", "Autodetect");
                core.set_option("mgba_gb_colors_preset", "1");
                core.set_option("mgba_gb_colors", "GBC Dark Green →A");
            }
        }
        if let Some((key, value)) = colour_option(which, platform, colour) {
            core.set_option(key, value);
        }
    }
    if which == Core::Gpsp {
        core.set_option("gpsp_serial", serial);
        core.set_option("gpsp_rtc_time_source", "system");
        if bios {
            core.set_option("gpsp_boot_mode", "bios");
        }
        if let Some((key, value)) = colour_option(which, platform, colour) {
            core.set_option(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn each_core_spells_colour_correction_its_own_way() {
        assert_eq!(
            colour_option(Core::Mgba, Platform::Gba, true),
            Some(("mgba_color_correction", "Auto"))
        );
        assert_eq!(
            colour_option(Core::Mgba, Platform::Gba, false),
            Some(("mgba_color_correction", "OFF"))
        );
        assert_eq!(
            colour_option(Core::Gpsp, Platform::Gba, true),
            Some(("gpsp_color_correction", "enabled"))
        );
        assert_eq!(
            colour_option(Core::Gpsp, Platform::Gba, false),
            Some(("gpsp_color_correction", "disabled"))
        );
    }

    #[test]
    fn mgba_uses_gbc_correction_for_game_boy_games() {
        assert_eq!(
            colour_option(Core::Mgba, Platform::Gb, true),
            Some(("mgba_color_correction", "GBC"))
        );
    }

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn candidates_search_the_named_cores_own_filename_only() {
        let _g = lock();
        std::env::remove_var(CORE_ENV);

        let root = Path::new("/root");
        let mgba = candidates(root, Core::Mgba);
        let gpsp = candidates(root, Core::Gpsp);
        assert_ne!(mgba, gpsp, "the two cores searched the same paths");
        assert!(!mgba.is_empty());
        assert!(!gpsp.is_empty());
        for path in &mgba {
            let name = path.file_name().unwrap().to_str().unwrap();
            assert!(name.starts_with("mgba_libretro"), "{name} is not mGBA's");
        }
        for path in &gpsp {
            let name = path.file_name().unwrap().to_str().unwrap();
            assert!(name.starts_with("gpsp_libretro"), "{name} is not gpSP's");
        }
    }

    #[test]
    fn candidates_search_the_roots_own_system_directory() {
        let _g = lock();
        std::env::remove_var(CORE_ENV);
        let root = Path::new("/some/content/root");
        assert_eq!(
            candidates(root, Core::Gpsp)[0],
            root.join("System").join(dylib_name(Core::Gpsp)),
        );
    }

    #[test]
    fn candidates_search_the_downloaded_cores_after_the_system_directory() {
        let _g = lock();
        std::env::remove_var(CORE_ENV);
        let root = Path::new("/some/content/root");
        assert_eq!(
            candidates(root, Core::Nestopia)[1],
            root.join("Cores").join(dylib_name(Core::Nestopia)),
        );
    }

    #[test]
    fn the_env_override_ignores_which_core_was_asked_for() {
        let _g = lock();
        std::env::set_var(CORE_ENV, "/dev/null/named-core");
        let root = Path::new("/root");
        let mgba = candidates(root, Core::Mgba);
        let gpsp = candidates(root, Core::Gpsp);
        std::env::remove_var(CORE_ENV);
        assert_eq!(mgba, vec![PathBuf::from("/dev/null/named-core")]);
        assert_eq!(
            mgba, gpsp,
            "the override stopped winning for one of the cores"
        );
    }
}
