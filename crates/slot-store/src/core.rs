use std::collections::HashMap;
use std::path::Path;

use crate::Platform;

pub const SELECTED_CORE_FILE: &str = "Config/selected_core.txt";
/// The core each system's games run when they have no line of their own, keyed by the system's
/// folder lower-cased.
pub const SYSTEM_CORE_FILE: &str = "Config/system_core.txt";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Core {
    #[default]
    Mgba,
    Gpsp,
    Fceumm,
    Snes9x,
    GenesisPlusGx,
    MednafenPceFast,
    Race,
    MednafenWswan,
    Handy,
    PcsxRearmed,
    Fbneo,
    Mame2003Plus,
    Neocd,
    ParallelN64,
    MednafenVb,
    Pokemini,
    Gw,
    Picodrive,
    Gearsystem,
    Flycast,
    MednafenSupergrafx,
    MednafenPcfx,
    Stella2014,
    A5200,
    Prosystem,
    Gearcoleco,
    Freeintv,
    Vecx,
    O2em,
    Freechaf,
    Potator,
    Sameduck,
    Fmsx,
    Bluemsx,
    ViceX64,
    Puae2021,
    Fuse,
    Cap32,
    Hatarib,
    Atari800,
    DosboxPure,
    Np2kai,
    Px68k,
    Scummvm,
    Retro8,
    Tic80,
    Ppsspp,
}

impl Core {
    pub const ALL: [Core; 47] = [
        Core::Mgba,
        Core::Gpsp,
        Core::Fceumm,
        Core::Snes9x,
        Core::GenesisPlusGx,
        Core::MednafenPceFast,
        Core::Race,
        Core::MednafenWswan,
        Core::Handy,
        Core::PcsxRearmed,
        Core::Fbneo,
        Core::Mame2003Plus,
        Core::Neocd,
        Core::ParallelN64,
        Core::MednafenVb,
        Core::Pokemini,
        Core::Gw,
        Core::Picodrive,
        Core::Gearsystem,
        Core::Flycast,
        Core::MednafenSupergrafx,
        Core::MednafenPcfx,
        Core::Stella2014,
        Core::A5200,
        Core::Prosystem,
        Core::Gearcoleco,
        Core::Freeintv,
        Core::Vecx,
        Core::O2em,
        Core::Freechaf,
        Core::Potator,
        Core::Sameduck,
        Core::Fmsx,
        Core::Bluemsx,
        Core::ViceX64,
        Core::Puae2021,
        Core::Fuse,
        Core::Cap32,
        Core::Hatarib,
        Core::Atari800,
        Core::DosboxPure,
        Core::Np2kai,
        Core::Px68k,
        Core::Scummvm,
        Core::Retro8,
        Core::Tic80,
        Core::Ppsspp,
    ];

    /// The core's file name without `_libretro` and the extension, as the buildbot names it.
    pub fn as_str(&self) -> &'static str {
        match self {
            Core::Mgba => "mgba",
            Core::Gpsp => "gpsp",
            Core::Fceumm => "fceumm",
            Core::Snes9x => "snes9x",
            Core::GenesisPlusGx => "genesis_plus_gx",
            Core::MednafenPceFast => "mednafen_pce_fast",
            Core::Race => "race",
            Core::MednafenWswan => "mednafen_wswan",
            Core::Handy => "handy",
            Core::PcsxRearmed => "pcsx_rearmed",
            Core::Fbneo => "fbneo",
            Core::Mame2003Plus => "mame2003_plus",
            Core::Neocd => "neocd",
            Core::ParallelN64 => "parallel_n64",
            Core::MednafenVb => "mednafen_vb",
            Core::Pokemini => "pokemini",
            Core::Gw => "gw",
            Core::Picodrive => "picodrive",
            Core::Gearsystem => "gearsystem",
            Core::Flycast => "flycast",
            Core::MednafenSupergrafx => "mednafen_supergrafx",
            Core::MednafenPcfx => "mednafen_pcfx",
            Core::Stella2014 => "stella2014",
            Core::A5200 => "a5200",
            Core::Prosystem => "prosystem",
            Core::Gearcoleco => "gearcoleco",
            Core::Freeintv => "freeintv",
            Core::Vecx => "vecx",
            Core::O2em => "o2em",
            Core::Freechaf => "freechaf",
            Core::Potator => "potator",
            Core::Sameduck => "sameduck",
            Core::Fmsx => "fmsx",
            Core::Bluemsx => "bluemsx",
            Core::ViceX64 => "vice_x64",
            Core::Puae2021 => "puae2021",
            Core::Fuse => "fuse",
            Core::Cap32 => "cap32",
            Core::Hatarib => "hatarib",
            Core::Atari800 => "atari800",
            Core::DosboxPure => "dosbox_pure",
            Core::Np2kai => "np2kai",
            Core::Px68k => "px68k",
            Core::Scummvm => "scummvm",
            Core::Retro8 => "retro8",
            Core::Tic80 => "tic80",
            Core::Ppsspp => "ppsspp",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn text(self) -> &'static str {
        match self {
            Core::Mgba => "mGBA",
            Core::Gpsp => "gpSP",
            Core::Fceumm => "FCEUmm",
            Core::Snes9x => "Snes9x",
            Core::GenesisPlusGx => "Genesis Plus GX",
            Core::MednafenPceFast => "Beetle PCE Fast",
            Core::Race => "RACE",
            Core::MednafenWswan => "Beetle Cygne",
            Core::Handy => "Handy",
            Core::PcsxRearmed => "PCSX ReARMed",
            Core::Fbneo => "FinalBurn Neo",
            Core::Mame2003Plus => "MAME 2003-Plus",
            Core::Neocd => "NeoCD",
            Core::ParallelN64 => "ParaLLEl N64",
            Core::MednafenVb => "Beetle VB",
            Core::Pokemini => "PokeMini",
            Core::Gw => "Game & Watch",
            Core::Picodrive => "PicoDrive",
            Core::Gearsystem => "Gearsystem",
            Core::Flycast => "Flycast",
            Core::MednafenSupergrafx => "Beetle SuperGrafx",
            Core::MednafenPcfx => "Beetle PC-FX",
            Core::Stella2014 => "Stella 2014",
            Core::A5200 => "a5200",
            Core::Prosystem => "ProSystem",
            Core::Gearcoleco => "Gearcoleco",
            Core::Freeintv => "FreeIntv",
            Core::Vecx => "vecx",
            Core::O2em => "O2EM",
            Core::Freechaf => "FreeChaF",
            Core::Potator => "Potator",
            Core::Sameduck => "SameDuck",
            Core::Fmsx => "fMSX",
            Core::Bluemsx => "blueMSX",
            Core::ViceX64 => "VICE x64",
            Core::Puae2021 => "PUAE 2021",
            Core::Fuse => "Fuse",
            Core::Cap32 => "Caprice32",
            Core::Hatarib => "Hatari B",
            Core::Atari800 => "Atari800",
            Core::DosboxPure => "DOSBox Pure",
            Core::Np2kai => "Neko Project II Kai",
            Core::Px68k => "PX68k",
            Core::Scummvm => "ScummVM",
            Core::Retro8 => "Retro8",
            Core::Tic80 => "TIC-80",
            Core::Ppsspp => "PPSSPP",
        }
    }

    pub fn parse(s: &str) -> Option<Core> {
        let s = s.trim();
        Core::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(s))
    }

    pub fn runs(self, platform: Platform) -> bool {
        platform.spec().cores.contains(&self)
    }

    pub fn default_for(platform: Platform) -> Core {
        platform.spec().cores[0]
    }
}

pub fn read_selected_cores(root: &Path) -> HashMap<String, Core> {
    crate::ini::read(root, SELECTED_CORE_FILE)
        .into_iter()
        .filter_map(|(stem, name)| Core::parse(&name).map(|core| (stem, core)))
        .collect()
}

pub fn core_for(root: &Path, stem: &str) -> Core {
    crate::ini::value(root, SELECTED_CORE_FILE, stem)
        .as_deref()
        .and_then(Core::parse)
        .unwrap_or_default()
}

pub fn core_for_platform(root: &Path, stem: &str, platform: Platform) -> Core {
    crate::ini::value(root, SELECTED_CORE_FILE, stem)
        .as_deref()
        .and_then(Core::parse)
        .filter(|core| core.runs(platform))
        .unwrap_or_else(|| core_for_system(root, platform))
}

/// The core a system's games run when they have none of their own: the one picked for the
/// system, or its default.
pub fn core_for_system(root: &Path, platform: Platform) -> Core {
    crate::ini::value(root, SYSTEM_CORE_FILE, &platform.key())
        .as_deref()
        .and_then(Core::parse)
        .filter(|core| core.runs(platform))
        .unwrap_or_else(|| Core::default_for(platform))
}

pub fn write_selected_core(root: &Path, stem: &str, core: Core) -> std::io::Result<()> {
    crate::ini::write(root, SELECTED_CORE_FILE, stem, core.as_str())
}

/// Gives a game a core of its own, or takes its line away when the core is its system's, so it
/// follows the system from then on.
pub fn write_game_core(
    root: &Path,
    stem: &str,
    platform: Platform,
    core: Core,
) -> std::io::Result<()> {
    if core == core_for_system(root, platform) {
        crate::ini::remove(root, SELECTED_CORE_FILE, stem)
    } else {
        write_selected_core(root, stem, core)
    }
}

pub fn write_system_core(root: &Path, platform: Platform, core: Core) -> std::io::Result<()> {
    crate::ini::write(root, SYSTEM_CORE_FILE, &platform.key(), core.as_str())
}
