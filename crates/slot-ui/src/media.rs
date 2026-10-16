//! The carts, cards and discs of the systems past the Game Boy family. Each one's neutral shell
//! is rendered from a 3D model by `tools/media/render_media.py`, which also measures the label
//! boxes below.

use std::sync::OnceLock;

use slot_store::Platform;

use crate::bitmaps;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Media {
    Nes,
    Snes,
    Genesis,
    MasterSystem,
    GameGear,
    HuCard,
    NeoGeoPocket,
    WonderSwan,
    Lynx,
    Disc,
}

struct Art {
    png: &'static [u8],
    w: u32,
    h: u32,
    label: (u32, u32, u32, u32),
    colour: [u8; 3],
}

const ART: [Art; Media::COUNT] = [
    Art {
        png: include_bytes!("../../../assets/nes_neutral.png"),
        w: 304,
        h: 337,
        label: (25, 20, 253, 208),
        colour: [0x8a, 0x8a, 0x8e],
    },
    Art {
        png: include_bytes!("../../../assets/snes_neutral.png"),
        w: 360,
        h: 259,
        label: (84, 56, 191, 146),
        colour: [0x9a, 0x99, 0xa0],
    },
    Art {
        png: include_bytes!("../../../assets/genesis_neutral.png"),
        w: 267,
        h: 336,
        label: (27, 107, 214, 214),
        colour: [0x2a, 0x2a, 0x2c],
    },
    Art {
        png: include_bytes!("../../../assets/sms_neutral.png"),
        w: 360,
        h: 180,
        label: (118, 17, 219, 146),
        colour: [0x26, 0x26, 0x28],
    },
    Art {
        png: include_bytes!("../../../assets/gamegear_neutral.png"),
        w: 320,
        h: 254,
        label: (56, 71, 207, 160),
        colour: [0x22, 0x22, 0x24],
    },
    Art {
        png: include_bytes!("../../../assets/hucard_neutral.png"),
        w: 212,
        h: 334,
        label: (16, 16, 181, 259),
        colour: [0xe6, 0xe4, 0xde],
    },
    Art {
        png: include_bytes!("../../../assets/ngp_neutral.png"),
        w: 300,
        h: 312,
        label: (42, 108, 210, 174),
        colour: [0x30, 0x30, 0x34],
    },
    Art {
        png: include_bytes!("../../../assets/wonderswan_neutral.png"),
        w: 340,
        h: 272,
        label: (28, 34, 193, 204),
        colour: [0x2c, 0x2c, 0x30],
    },
    Art {
        png: include_bytes!("../../../assets/lynx_neutral.png"),
        w: 340,
        h: 237,
        label: (36, 82, 268, 134),
        colour: [0x3a, 0x3a, 0x3e],
    },
    Art {
        png: include_bytes!("../../../assets/disc_neutral.png"),
        w: 337,
        h: 337,
        label: (0, 0, 337, 337),
        colour: [0xd8, 0xd8, 0xdc],
    },
];

/// The disc's print runs from the clear hub to just inside the rim, as fractions of its radius:
/// 22.5 mm and 58.5 mm of a 60 mm disc.
const PRINT_INNER: f32 = 22.5 / 60.0;
const PRINT_OUTER: f32 = 58.5 / 60.0;

/// How tall a medium may sit in the slot: the Game Boy pak's seated height.
const SEATED_MAX_H: u32 = 259;

impl Media {
    pub const COUNT: usize = 10;
    pub const ALL: [Media; Media::COUNT] = [
        Media::Nes,
        Media::Snes,
        Media::Genesis,
        Media::MasterSystem,
        Media::GameGear,
        Media::HuCard,
        Media::NeoGeoPocket,
        Media::WonderSwan,
        Media::Lynx,
        Media::Disc,
    ];

    /// What a platform's games come on, or None for the Game Boy family's own moulds.
    pub fn of(platform: Platform) -> Option<Media> {
        Some(match platform {
            Platform::Gba | Platform::Gb | Platform::Gbc => return None,
            Platform::Nes => Media::Nes,
            Platform::Snes => Media::Snes,
            Platform::Genesis => Media::Genesis,
            Platform::MasterSystem => Media::MasterSystem,
            Platform::GameGear => Media::GameGear,
            Platform::PcEngine => Media::HuCard,
            Platform::NeoGeoPocket => Media::NeoGeoPocket,
            Platform::WonderSwan => Media::WonderSwan,
            Platform::Lynx => Media::Lynx,
            Platform::PlayStation => Media::Disc,
        })
    }

    pub fn index(self) -> usize {
        self as usize
    }

    fn art(self) -> &'static Art {
        &ART[self.index()]
    }

    pub fn size(self) -> (u32, u32) {
        (self.art().w, self.art().h)
    }

    /// The label box, x, y, w, h in the shell's pixels.
    pub fn label(self) -> (u32, u32, u32, u32) {
        self.art().label
    }

    /// The plastic a game gets when nothing names another colour for it.
    pub fn colour(self) -> [u8; 3] {
        self.art().colour
    }

    /// The size it is drawn at once it sits in the slot: as wide as a seated cart, unless that
    /// would stand taller than a seated Game Boy pak.
    pub fn seated(self, seated_w: u32) -> (u32, u32) {
        let (w, h) = self.size();
        let tall = (seated_w * h + w / 2) / w;
        if tall <= SEATED_MAX_H {
            return (seated_w, tall);
        }
        ((SEATED_MAX_H * w + h / 2) / h, SEATED_MAX_H)
    }

    /// The neutral shell, decoded once.
    pub fn neutral(self) -> &'static [u8] {
        static SHELLS: [OnceLock<Vec<u8>>; Media::COUNT] =
            [const { OnceLock::new() }; Media::COUNT];
        let art = self.art();
        bitmaps::load(art.png, art.w, art.h, &SHELLS[self.index()])
    }

    /// The shell's coverage, from the render's alpha.
    pub fn mask(self) -> &'static [u8] {
        static MASKS: [OnceLock<Vec<u8>>; Media::COUNT] = [const { OnceLock::new() }; Media::COUNT];
        MASKS[self.index()]
            .get_or_init(|| self.neutral().iter().skip(3).step_by(4).copied().collect())
    }

    /// Where on the label box the label may print, for a medium whose print is not the whole box:
    /// the disc's ring between its hub and rim. Coverage a pixel, row by row over the label box.
    pub fn label_mask(self) -> Option<&'static [u8]> {
        if self != Media::Disc {
            return None;
        }
        static RING: OnceLock<Vec<u8>> = OnceLock::new();
        Some(RING.get_or_init(|| {
            let (_, _, w, h) = self.label();
            ring(w, h, PRINT_INNER, PRINT_OUTER)
        }))
    }
}

/// A ring's coverage over a w by h box centred on it, the radii as fractions of the half width,
/// with its edges antialiased across a pixel.
fn ring(w: u32, h: u32, inner: f32, outer: f32) -> Vec<u8> {
    let half = w as f32 / 2.0;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = ((x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy)) / half;
            let into_outer = ((outer - r) * half + 0.5).clamp(0.0, 1.0);
            let past_inner = ((r - inner) * half + 0.5).clamp(0.0, 1.0);
            out.push((into_outer.min(past_inner) * 255.0).round() as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_past_the_game_boy_has_media() {
        for p in Platform::ALL {
            let game_boy = matches!(p, Platform::Gba | Platform::Gb | Platform::Gbc);
            assert_eq!(Media::of(p).is_none(), game_boy, "{p:?}");
        }
    }

    #[test]
    fn every_shell_decodes_at_the_size_it_claims_and_its_label_fits() {
        for m in Media::ALL {
            let (w, h) = m.size();
            assert_eq!(m.neutral().len(), (w * h * 4) as usize, "{m:?}");
            let (lx, ly, lw, lh) = m.label();
            assert!(lx + lw <= w && ly + lh <= h, "{m:?} label leaves the shell");
        }
    }

    #[test]
    fn a_shell_is_solid_at_its_label_and_clear_past_its_corners() {
        for m in Media::ALL {
            let (w, _) = m.size();
            let (lx, ly, lw, lh) = m.label();
            let at = |x: u32, y: u32| m.mask()[(y * w + x) as usize];
            let (cx, cy) = (lx + lw / 2, ly + lh / 4);
            if m != Media::Disc {
                assert_eq!(at(cx, cy), 255, "{m:?} has a hole in its label");
            }
            assert_eq!(at(0, 0), 0, "{m:?} fills its top left corner");
        }
    }

    #[test]
    fn the_disc_prints_on_its_ring_but_not_its_hub_or_past_its_rim() {
        let ring = Media::Disc.label_mask().expect("the disc has a ring");
        let (_, _, w, h) = Media::Disc.label();
        let at = |x: u32, y: u32| ring[(y * w + x) as usize];
        assert_eq!(at(w / 2, h / 2), 0, "the hub was printed on");
        assert_eq!(at(w / 2, h / 10), 255, "the print is missing above the hub");
        assert_eq!(at(0, 0), 0, "the corner past the rim was printed on");
    }

    #[test]
    fn nothing_sits_taller_in_the_slot_than_a_game_boy_pak() {
        for m in Media::ALL {
            let (w, h) = m.seated(240);
            assert!(h <= SEATED_MAX_H && w <= 240, "{m:?} sits at {w}x{h}");
        }
    }
}
