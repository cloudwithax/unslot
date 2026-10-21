use std::path::Path;

use crate::core::Core;
use crate::slot_state::Shader;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Platform {
    #[default]
    Gba,
    Gb,
    Gbc,
    Nes,
    Snes,
    Genesis,
    MasterSystem,
    GameGear,
    PcEngine,
    NeoGeoPocket,
    WonderSwan,
    Lynx,
    PlayStation,
    Arcade,
    Cps1,
    Cps2,
    Cps3,
    NeoGeo,
    NeoGeoCd,
    Fds,
    Nintendo64,
    VirtualBoy,
    PokemonMini,
    GameAndWatch,
    Sega32x,
    SegaCd,
    Sg1000,
    Dreamcast,
    PcEngineCd,
    SuperGrafx,
    PcFx,
    Atari2600,
    Atari5200,
    Atari7800,
    Atari800,
    AtariSt,
    ColecoVision,
    Intellivision,
    Vectrex,
    Odyssey2,
    ChannelF,
    Supervision,
    MegaDuck,
    Msx,
    Commodore64,
    Amiga,
    ZxSpectrum,
    AmstradCpc,
    Dos,
    Pc98,
    X68000,
    ScummVm,
    Pico8,
    Tic80,
    Psp,
}

/// How the scanner finds a game's title, and for the GBA its four-letter code, in the ROM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Header {
    Gba,
    Gb,
    /// An arcade set: the zip's name is the set's, and the shelf shows the game the arcade
    /// databases say it is.
    Arcade,
    /// No title worth reading in the file; the shelf shows the file's name.
    None,
}

/// Everything slot needs to know about a system. Adding one is a variant on `Platform` and a
/// row in `SPECS`; the compiler then points at the cart art it still needs.
#[derive(Debug)]
pub struct Spec {
    pub name: &'static str,
    /// A shorter name where the whole one will not fit, as in a list's right-hand column; empty
    /// when the name is short enough already.
    pub short: &'static str,
    /// The folder under Games, Labels, Saves, States and Bezels, and the key in config files.
    pub dir: &'static str,
    pub extensions: &'static [&'static str],
    /// Whether the core's picture is drawn at its own size and aspect. When it is not, the game
    /// is centred in the Game Boy family's 240x160 canvas and `picture` says where it sits.
    pub native: bool,
    /// The game's picture inside the 240x160 canvas the Game Boy family is drawn into.
    pub picture: (u32, u32),
    pub header: Header,
    /// The cores that run it, the default first.
    pub cores: &'static [Core],
    pub shaders: &'static [Shader],
    pub shader: Shader,
    /// Whether its core can correct the colour of a handheld's LCD.
    pub colour_correction: bool,
    /// Whether the system has a Game Boy palette to pick for monochrome games.
    pub palettes: bool,
    /// Whether it has a default bezel and a Bezels folder of its own.
    pub bezels: bool,
    /// Whether its games read L and R; when they do not, slot keeps the shoulders.
    pub shoulders: bool,
    /// Whether its games read L2 and R2; when they do, rewind and fast forward give them up.
    pub triggers: bool,
    /// Whether X and Y can fire A and B as turbo buttons. Not on a pad that has its own X and Y.
    pub turbo: bool,
    /// Whether its save states are small enough to keep a rewind buffer of.
    pub rewind: bool,
    /// Whether two of it can play over the wireless link.
    pub link: bool,
    /// The files its core will not start without, in the BIOS folder.
    pub bios: &'static [&'static str],
}

const GB_SHADERS: &[Shader] = &[Shader::Off, Shader::Lcd3x, Shader::Grid, Shader::Simpletex];
const HANDHELD_SHADERS: &[Shader] = &[Shader::Off, Shader::Lcd3x, Shader::Grid, Shader::Dot];
const TV_SHADERS: &[Shader] = &[Shader::Off, Shader::Grid, Shader::Lcd3x];

/// What a system drawn at its own size shares unless its row says otherwise.
const CONSOLE: Spec = Spec {
    name: "",
    short: "",
    dir: "",
    extensions: &[],
    native: true,
    picture: (240, 160),
    header: Header::None,
    cores: &[],
    shaders: TV_SHADERS,
    shader: Shader::Off,
    colour_correction: false,
    palettes: false,
    bezels: false,
    shoulders: false,
    triggers: false,
    turbo: true,
    rewind: true,
    link: false,
    bios: &[],
};

const HANDHELD: Spec = Spec {
    shaders: HANDHELD_SHADERS,
    shader: Shader::Grid,
    ..CONSOLE
};

const SPECS: [Spec; Platform::COUNT] = [
    Spec {
        name: "Game Boy Advance",
        dir: "GBA",
        extensions: &["gba"],
        native: false,
        picture: (240, 160),
        header: Header::Gba,
        cores: &[Core::Mgba, Core::Gpsp],
        shaders: HANDHELD_SHADERS,
        shader: Shader::Grid,
        colour_correction: true,
        shoulders: true,
        link: true,
        ..CONSOLE
    },
    Spec {
        name: "Game Boy",
        dir: "GB",
        extensions: &["gb", "gbc"],
        native: false,
        picture: (160, 144),
        header: Header::Gb,
        cores: &[Core::Mgba],
        shaders: GB_SHADERS,
        shader: Shader::Simpletex,
        colour_correction: true,
        palettes: true,
        bezels: true,
        ..CONSOLE
    },
    Spec {
        name: "Game Boy Color",
        dir: "GBC",
        extensions: &["gb", "gbc"],
        native: false,
        picture: (160, 144),
        header: Header::Gb,
        cores: &[Core::Mgba],
        shaders: GB_SHADERS,
        shader: Shader::Simpletex,
        colour_correction: true,
        bezels: true,
        ..CONSOLE
    },
    Spec {
        name: "Nintendo Entertainment System",
        short: "NES",
        dir: "NES",
        extensions: &["nes", "unf", "unif"],
        cores: &[Core::Fceumm],
        ..CONSOLE
    },
    Spec {
        name: "Super Nintendo",
        dir: "SNES",
        extensions: &["sfc", "smc"],
        cores: &[Core::Snes9x],
        shoulders: true,
        turbo: false,
        ..CONSOLE
    },
    Spec {
        name: "Sega Genesis",
        dir: "MD",
        extensions: &["md", "gen", "bin", "smd"],
        cores: &[Core::GenesisPlusGx],
        shoulders: true,
        turbo: false,
        ..CONSOLE
    },
    Spec {
        name: "Sega Master System",
        dir: "SMS",
        extensions: &["sms"],
        cores: &[Core::GenesisPlusGx],
        ..CONSOLE
    },
    Spec {
        name: "Sega Game Gear",
        dir: "GG",
        extensions: &["gg"],
        cores: &[Core::GenesisPlusGx],
        ..HANDHELD
    },
    Spec {
        name: "PC Engine",
        dir: "PCE",
        extensions: &["pce"],
        cores: &[Core::MednafenPceFast],
        ..CONSOLE
    },
    Spec {
        name: "Neo Geo Pocket",
        dir: "NGP",
        extensions: &["ngp", "ngc"],
        cores: &[Core::Race],
        ..HANDHELD
    },
    Spec {
        name: "WonderSwan",
        dir: "WS",
        extensions: &["ws", "wsc"],
        cores: &[Core::MednafenWswan],
        shoulders: true,
        turbo: false,
        ..HANDHELD
    },
    Spec {
        name: "Atari Lynx",
        dir: "LYNX",
        extensions: &["lnx"],
        cores: &[Core::Handy],
        shoulders: true,
        bios: &["lynxboot.img"],
        ..HANDHELD
    },
    Spec {
        name: "PlayStation",
        dir: "PS",
        extensions: &["cue", "chd", "pbp", "m3u"],
        cores: &[Core::PcsxRearmed],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },    Spec {
        name: "Arcade",
        dir: "ARCADE",
        header: Header::Arcade,
        extensions: &["zip", "7z"],
        cores: &[Core::Fbneo, Core::Mame2003Plus],
        shoulders: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "CPS-1",
        dir: "CPS1",
        header: Header::Arcade,
        extensions: &["zip", "7z"],
        cores: &[Core::Fbneo, Core::Mame2003Plus],
        shoulders: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "CPS-2",
        dir: "CPS2",
        header: Header::Arcade,
        extensions: &["zip", "7z"],
        cores: &[Core::Fbneo, Core::Mame2003Plus],
        shoulders: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "CPS-3",
        dir: "CPS3",
        header: Header::Arcade,
        extensions: &["zip", "7z"],
        cores: &[Core::Fbneo],
        shoulders: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "Neo Geo",
        dir: "NEOGEO",
        header: Header::Arcade,
        extensions: &["zip", "7z"],
        cores: &[Core::Fbneo, Core::Mame2003Plus],
        turbo: false,
        rewind: false,
        bios: &["fbneo/neogeo.zip|neogeo.zip|@neogeo.zip"],
        ..CONSOLE
    },
    Spec {
        name: "Neo Geo CD",
        dir: "NEOCD",
        extensions: &["cue", "chd"],
        cores: &[Core::Neocd],
        turbo: false,
        rewind: false,
        bios: &["neocd/neocd_f.rom|neocd/neocd_sf.rom|neocd/neocd_t.rom|neocd/neocd_st.rom|neocd/neocd_z.rom|neocd/neocd_sz.rom|neocd/front-sp1.bin|neocd/top-sp1.bin|neocd/neocd.bin|neocd/uni-bioscd.rom",
            "neocd/ng-lo.rom|neocd/000-lo.lo",
        ],
        ..CONSOLE
    },
    Spec {
        name: "Famicom Disk System",
        short: "Famicom Disk",
        dir: "FDS",
        extensions: &["fds"],
        cores: &[Core::Fceumm],
        bios: &["disksys.rom"],
        ..CONSOLE
    },
    Spec {
        name: "Nintendo 64",
        dir: "N64",
        extensions: &["n64", "v64", "z64", "bin", "u1", "ndd"],
        cores: &[Core::ParallelN64],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "Virtual Boy",
        dir: "VB",
        extensions: &["vb", "vboy"],
        cores: &[Core::MednafenVb],
        shoulders: true,
        turbo: false,
        ..CONSOLE
    },
    Spec {
        name: "Pokémon mini",
        dir: "POKEMINI",
        extensions: &["min"],
        cores: &[Core::Pokemini],
        ..HANDHELD
    },
    Spec {
        name: "Game & Watch",
        dir: "GW",
        extensions: &["mgw"],
        cores: &[Core::Gw],
        ..HANDHELD
    },
    Spec {
        name: "Sega 32X",
        dir: "32X",
        extensions: &["32x"],
        cores: &[Core::Picodrive],
        shoulders: true,
        turbo: false,
        ..CONSOLE
    },
    Spec {
        name: "Sega CD",
        dir: "SEGACD",
        extensions: &["cue", "chd", "iso", "m3u"],
        cores: &[Core::GenesisPlusGx, Core::Picodrive],
        shoulders: true,
        turbo: false,
        rewind: false,
        bios: &["bios_CD_U.bin|bios_CD_E.bin|bios_CD_J.bin"],
        ..CONSOLE
    },
    Spec {
        name: "SG-1000",
        dir: "SG1000",
        extensions: &["sg"],
        cores: &[Core::GenesisPlusGx, Core::Gearsystem],
        ..CONSOLE
    },
    Spec {
        name: "Dreamcast",
        dir: "DC",
        extensions: &["chd", "cdi", "gdi", "cue", "m3u"],
        cores: &[Core::Flycast],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "PC Engine CD",
        dir: "PCECD",
        extensions: &["cue", "ccd", "chd", "toc", "m3u"],
        cores: &[Core::MednafenPceFast],
        rewind: false,
        bios: &["syscard3.pce"],
        ..CONSOLE
    },
    Spec {
        name: "SuperGrafx",
        dir: "SGFX",
        extensions: &["sgx", "pce", "cue", "ccd", "chd"],
        cores: &[Core::MednafenSupergrafx],
        ..CONSOLE
    },
    Spec {
        name: "PC-FX",
        dir: "PCFX",
        extensions: &["cue", "ccd", "toc", "chd"],
        cores: &[Core::MednafenPcfx],
        shoulders: true,
        turbo: false,
        rewind: false,
        bios: &["pcfx.rom"],
        ..CONSOLE
    },
    Spec {
        name: "Atari 2600",
        dir: "A2600",
        extensions: &["a26", "bin"],
        cores: &[Core::Stella2014],
        ..CONSOLE
    },
    Spec {
        name: "Atari 5200",
        dir: "A5200",
        extensions: &["a52", "bin"],
        cores: &[Core::A5200],
        shoulders: true,
        turbo: false,
        bios: &["5200.rom"],
        ..CONSOLE
    },
    Spec {
        name: "Atari 7800",
        dir: "A7800",
        extensions: &["a78", "bin", "cdf"],
        cores: &[Core::Prosystem],
        ..CONSOLE
    },
    Spec {
        name: "Atari 800",
        dir: "A800",
        extensions: &["atr", "xex", "xfd", "cas", "bin", "car"],
        cores: &[Core::Atari800],
        shoulders: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "Atari ST",
        dir: "ATARIST",
        extensions: &["st", "msa", "stx", "dim", "ipf"],
        cores: &[Core::Hatarib],
        shoulders: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "ColecoVision",
        dir: "COLECO",
        extensions: &["col", "cv", "bin", "rom"],
        cores: &[Core::Gearcoleco],
        shoulders: true,
        turbo: false,
        bios: &["colecovision.rom"],
        ..CONSOLE
    },
    Spec {
        name: "Intellivision",
        dir: "INTV",
        extensions: &["int", "bin", "rom"],
        cores: &[Core::Freeintv],
        shoulders: true,
        turbo: false,
        bios: &["exec.bin", "grom.bin"],
        ..CONSOLE
    },
    Spec {
        name: "Vectrex",
        dir: "VECTREX",
        extensions: &["vec", "bin"],
        cores: &[Core::Vecx],
        turbo: false,
        ..CONSOLE
    },
    Spec {
        name: "Odyssey²",
        dir: "ODYSSEY",
        extensions: &["bin"],
        cores: &[Core::O2em],
        bios: &["o2rom.bin"],
        ..CONSOLE
    },
    Spec {
        name: "Channel F",
        dir: "CHANNELF",
        extensions: &["bin", "chf"],
        cores: &[Core::Freechaf],
        shoulders: true,
        turbo: false,
        bios: &["sl31254.bin", "sl31253.bin|sl90025.bin"],
        ..CONSOLE
    },
    Spec {
        name: "Supervision",
        dir: "SUPERVISION",
        extensions: &["sv", "bin"],
        cores: &[Core::Potator],
        ..HANDHELD
    },
    Spec {
        name: "Mega Duck",
        dir: "MEGADUCK",
        extensions: &["bin", "duck"],
        cores: &[Core::Sameduck],
        ..HANDHELD
    },
    Spec {
        name: "MSX",
        dir: "MSX",
        extensions: &["rom", "mx1", "mx2", "dsk", "cas"],
        cores: &[Core::Fmsx, Core::Bluemsx],
        bios: &["MSX.ROM", "MSX2.ROM", "MSX2EXT.ROM"],
        shoulders: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "Commodore 64",
        dir: "C64",
        extensions: &["d64", "t64", "prg", "crt", "tap", "d81", "m3u"],
        cores: &[Core::ViceX64],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "Amiga",
        dir: "AMIGA",
        extensions: &["adf", "hdf", "lha", "ipf", "m3u", "zip"],
        cores: &[Core::Puae2021],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "ZX Spectrum",
        dir: "ZXS",
        extensions: &["tzx", "tap", "z80", "sna", "dsk", "scl", "trd"],
        cores: &[Core::Fuse],
        shoulders: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "Amstrad CPC",
        dir: "CPC",
        extensions: &["dsk", "cdt", "sna", "m3u"],
        cores: &[Core::Cap32],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "DOS",
        dir: "DOS",
        extensions: &["zip", "dosz", "exe", "com", "bat", "iso", "cue", "img", "conf"],
        cores: &[Core::DosboxPure],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "PC-98",
        dir: "PC98",
        extensions: &["d98", "fdi", "hdi", "hdm", "fdd", "nhd", "thd", "m3u"],
        cores: &[Core::Np2kai],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        bios: &["np2kai/font.bmp|np2kai/FONT.ROM"],
        ..CONSOLE
    },
    Spec {
        name: "X68000",
        dir: "X68000",
        extensions: &["dim", "xdf", "hdm", "hdf", "2hd", "m3u"],
        cores: &[Core::Px68k],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        bios: &["keropi/iplrom.dat", "keropi/cgrom.dat"],
        ..CONSOLE
    },
    Spec {
        name: "ScummVM",
        dir: "SCUMMVM",
        extensions: &["scummvm"],
        cores: &[Core::Scummvm],
        shoulders: true,
        triggers: true,
        turbo: false,
        rewind: false,
        ..CONSOLE
    },
    Spec {
        name: "PICO-8",
        dir: "PICO8",
        extensions: &["p8", "png"],
        cores: &[Core::Retro8],
        turbo: false,
        ..CONSOLE
    },
    Spec {
        name: "TIC-80",
        dir: "TIC80",
        extensions: &["tic"],
        cores: &[Core::Tic80],
        turbo: false,
        ..CONSOLE
    },
    Spec {
        name: "PlayStation Portable",
        short: "PSP",
        dir: "PSP",
        extensions: &["iso", "cso", "pbp", "chd", "elf", "prx"],
        cores: &[Core::Ppsspp],
        shoulders: true,
        turbo: false,
        rewind: false,
        ..HANDHELD
    },
];

impl Platform {
    pub const COUNT: usize = 55;
    pub const ALL: [Platform; Platform::COUNT] = [
        Platform::Gba,
        Platform::Gb,
        Platform::Gbc,
        Platform::Nes,
        Platform::Snes,
        Platform::Genesis,
        Platform::MasterSystem,
        Platform::GameGear,
        Platform::PcEngine,
        Platform::NeoGeoPocket,
        Platform::WonderSwan,
        Platform::Lynx,
        Platform::PlayStation,
        Platform::Arcade,
        Platform::Cps1,
        Platform::Cps2,
        Platform::Cps3,
        Platform::NeoGeo,
        Platform::NeoGeoCd,
        Platform::Fds,
        Platform::Nintendo64,
        Platform::VirtualBoy,
        Platform::PokemonMini,
        Platform::GameAndWatch,
        Platform::Sega32x,
        Platform::SegaCd,
        Platform::Sg1000,
        Platform::Dreamcast,
        Platform::PcEngineCd,
        Platform::SuperGrafx,
        Platform::PcFx,
        Platform::Atari2600,
        Platform::Atari5200,
        Platform::Atari7800,
        Platform::Atari800,
        Platform::AtariSt,
        Platform::ColecoVision,
        Platform::Intellivision,
        Platform::Vectrex,
        Platform::Odyssey2,
        Platform::ChannelF,
        Platform::Supervision,
        Platform::MegaDuck,
        Platform::Msx,
        Platform::Commodore64,
        Platform::Amiga,
        Platform::ZxSpectrum,
        Platform::AmstradCpc,
        Platform::Dos,
        Platform::Pc98,
        Platform::X68000,
        Platform::ScummVm,
        Platform::Pico8,
        Platform::Tic80,
        Platform::Psp,
    ];

    pub fn spec(self) -> &'static Spec {
        &SPECS[self.index()]
    }

    /// Where the platform sits in `ALL`, for tables kept one entry a platform.
    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        self.spec().name
    }

    /// The name, or its short form where it has one.
    pub fn short_name(self) -> &'static str {
        match self.spec().short {
            "" => self.name(),
            short => short,
        }
    }

    pub fn dir_name(self) -> &'static str {
        self.spec().dir
    }

    /// The dir name lower-cased, as config files spell it.
    pub fn key(self) -> String {
        self.dir_name().to_ascii_lowercase()
    }

    pub fn from_key(key: &str) -> Option<Platform> {
        Platform::ALL
            .into_iter()
            .find(|p| key.eq_ignore_ascii_case(p.dir_name()))
    }

    pub fn extensions(self) -> &'static [&'static str] {
        self.spec().extensions
    }

    pub fn picture(self) -> (u32, u32) {
        self.spec().picture
    }

    pub fn accepts(self, path: &Path) -> bool {
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => ext,
            None => return false,
        };
        self.extensions().iter().any(|k| ext.eq_ignore_ascii_case(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_sits_at_its_own_index() {
        for (at, p) in Platform::ALL.into_iter().enumerate() {
            assert_eq!(p.index(), at, "{p:?} is out of place in ALL");
        }
    }

    #[test]
    fn every_spec_is_complete() {
        for p in Platform::ALL {
            let s = p.spec();
            assert!(!s.name.is_empty() && !s.dir.is_empty(), "{p:?} has no name");
            assert!(!s.cores.is_empty(), "{p:?} has no core");
            assert!(
                s.shaders.contains(&s.shader),
                "{p:?} defaults to a shader it does not offer"
            );
            assert!(!s.extensions.is_empty(), "{p:?} opens no files");
            assert!(
                s.native || s.header != Header::None,
                "{p:?} is drawn in the Game Boy canvas but is not a Game Boy"
            );
        }
    }

    #[test]
    fn dir_names_are_unique_and_round_trip() {
        for p in Platform::ALL {
            assert_eq!(Platform::from_key(&p.key()), Some(p));
        }
        assert_eq!(Platform::from_key("nope"), None);
    }

    #[test]
    fn turbo_stays_off_a_pad_whose_x_and_y_are_its_own() {
        for p in [Platform::Snes, Platform::Genesis, Platform::PlayStation] {
            assert!(!p.spec().turbo, "{p:?} would lose X and Y to turbo");
        }
    }
}
