use std::collections::HashMap;
use std::path::Path;

use crate::Platform;

pub const SELECTED_CORE_FILE: &str = "Config/selected_core.txt";

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
}

impl Core {
    pub const ALL: [Core; 10] = [
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
        .unwrap_or_else(|| Core::default_for(platform))
}

pub fn write_selected_core(root: &Path, stem: &str, core: Core) -> std::io::Result<()> {
    crate::ini::write(root, SELECTED_CORE_FILE, stem, core.as_str())
}
