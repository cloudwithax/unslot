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
}

/// How the scanner finds a game's title, and for the GBA its four-letter code, in the ROM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Header {
    Gba,
    Gb,
    /// No title worth reading in the file; the shelf shows the file's name.
    None,
}

/// Everything slot needs to know about a system. Adding one is a variant on `Platform` and a
/// row in `SPECS`; the compiler then points at the cart art it still needs.
#[derive(Debug)]
pub struct Spec {
    pub name: &'static str,
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
    },
];

impl Platform {
    pub const COUNT: usize = 13;
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
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                self.extensions()
                    .iter()
                    .any(|k| ext.eq_ignore_ascii_case(k))
            })
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
