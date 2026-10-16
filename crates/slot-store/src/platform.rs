use std::path::Path;

use crate::core::Core;
use crate::slot_state::Shader;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Platform {
    #[default]
    Gba,
    Gb,
    Gbc,
}

/// How the scanner finds a game's title, and for the GBA its four-letter code, in the ROM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Header {
    Gba,
    Gb,
}

/// Everything slot needs to know about a system. Adding one is a variant on `Platform` and a
/// row in `SPECS`; the compiler then points at the cart art it still needs.
#[derive(Debug)]
pub struct Spec {
    pub name: &'static str,
    /// The folder under Games, Labels, Saves, States and Bezels, and the key in config files.
    pub dir: &'static str,
    pub extensions: &'static [&'static str],
    /// The game's picture inside the 240x160 canvas the Game Boy family is drawn into.
    pub picture: (u32, u32),
    pub header: Header,
    /// The cores that run it, the default first.
    pub cores: &'static [Core],
    pub shaders: &'static [Shader],
    pub shader: Shader,
    /// Whether the system has a Game Boy palette to pick for monochrome games.
    pub palettes: bool,
    /// Whether it has a default bezel and a Bezels folder of its own.
    pub bezels: bool,
    /// Whether its games read L and R; when they do not, slot keeps the shoulders.
    pub shoulders: bool,
    /// Whether two of it can play over the wireless link.
    pub link: bool,
}

const GB_SHADERS: &[Shader] = &[Shader::Off, Shader::Lcd3x, Shader::Grid, Shader::Simpletex];

const SPECS: [Spec; Platform::COUNT] = [
    Spec {
        name: "Game Boy Advance",
        dir: "GBA",
        extensions: &["gba"],
        picture: (240, 160),
        header: Header::Gba,
        cores: &[Core::Mgba, Core::Gpsp],
        shaders: &[Shader::Off, Shader::Lcd3x, Shader::Grid, Shader::Dot],
        shader: Shader::Grid,
        palettes: false,
        bezels: false,
        shoulders: true,
        link: true,
    },
    Spec {
        name: "Game Boy",
        dir: "GB",
        extensions: &["gb", "gbc"],
        picture: (160, 144),
        header: Header::Gb,
        cores: &[Core::Mgba],
        shaders: GB_SHADERS,
        shader: Shader::Simpletex,
        palettes: true,
        bezels: true,
        shoulders: false,
        link: false,
    },
    Spec {
        name: "Game Boy Color",
        dir: "GBC",
        extensions: &["gb", "gbc"],
        picture: (160, 144),
        header: Header::Gb,
        cores: &[Core::Mgba],
        shaders: GB_SHADERS,
        shader: Shader::Simpletex,
        palettes: false,
        bezels: true,
        shoulders: false,
        link: false,
    },
];

impl Platform {
    pub const COUNT: usize = 3;
    pub const ALL: [Platform; Platform::COUNT] = [Platform::Gba, Platform::Gb, Platform::Gbc];

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
            assert!(!s.cores.is_empty(), "{p:?} has no core");
            assert!(
                s.shaders.contains(&s.shader),
                "{p:?} defaults to a shader it does not offer"
            );
            assert!(!s.extensions.is_empty(), "{p:?} opens no files");
        }
    }

    #[test]
    fn dir_names_are_unique_and_round_trip() {
        for p in Platform::ALL {
            assert_eq!(Platform::from_key(&p.key()), Some(p));
        }
        assert_eq!(Platform::from_key("nope"), None);
    }
}
