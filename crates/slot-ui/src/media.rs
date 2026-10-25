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
    Pcb,
    Mvs,
    FamicomDisk,
    Nintendo64,
    VirtualBoy,
    PokemonMini,
    GameAndWatch,
    Sega32x,
    Sg1000,
    Atari2600,
    Atari5200,
    Atari7800,
    ColecoVision,
    Intellivision,
    Vectrex,
    Odyssey2,
    ChannelF,
    Supervision,
    MegaDuck,
    Msx,
    Floppy35,
    Floppy525,
    Tape,
    AmstradDisk,
    Umd,
    Fantasy,
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
    Art {
        png: include_bytes!("../../../assets/pcb_neutral.png"),
        w: 360,
        h: 269,
        label: (24, 22, 168, 106),
        colour: [0x2e, 0x6b, 0x3a],
    },
    Art {
        png: include_bytes!("../../../assets/mvs_neutral.png"),
        w: 360,
        h: 252,
        label: (27, 58, 306, 166),
        colour: [0x22, 0x22, 0x24],
    },
    Art {
        png: include_bytes!("../../../assets/fds_neutral.png"),
        w: 272,
        h: 322,
        label: (25, 25, 222, 136),
        colour: [0xe8, 0xc4, 0x2c],
    },
    Art {
        png: include_bytes!("../../../assets/n64_neutral.png"),
        w: 360,
        h: 236,
        label: (50, 74, 261, 137),
        colour: [0x5a, 0x5a, 0x60],
    },
    Art {
        png: include_bytes!("../../../assets/vb_neutral.png"),
        w: 300,
        h: 333,
        label: (33, 100, 233, 200),
        colour: [0x2a, 0x2a, 0x2c],
    },
    Art {
        png: include_bytes!("../../../assets/pokemini_neutral.png"),
        w: 300,
        h: 278,
        label: (33, 89, 233, 156),
        colour: [0x50, 0x50, 0x58],
    },
    Art {
        png: include_bytes!("../../../assets/gw_neutral.png"),
        w: 360,
        h: 219,
        label: (98, 33, 164, 131),
        colour: [0xc4, 0xac, 0x80],
    },
    Art {
        png: include_bytes!("../../../assets/32x_neutral.png"),
        w: 250,
        h: 345,
        label: (30, 135, 190, 180),
        colour: [0x24, 0x24, 0x26],
    },
    Art {
        png: include_bytes!("../../../assets/sg1000_neutral.png"),
        w: 280,
        h: 343,
        label: (33, 99, 214, 198),
        colour: [0x28, 0x28, 0x2a],
    },
    Art {
        png: include_bytes!("../../../assets/a2600_neutral.png"),
        w: 300,
        h: 360,
        label: (30, 30, 240, 188),
        colour: [0x22, 0x22, 0x22],
    },
    Art {
        png: include_bytes!("../../../assets/a5200_neutral.png"),
        w: 300,
        h: 331,
        label: (29, 23, 243, 160),
        colour: [0x6a, 0x6a, 0x70],
    },
    Art {
        png: include_bytes!("../../../assets/a7800_neutral.png"),
        w: 288,
        h: 360,
        label: (29, 22, 230, 202),
        colour: [0x3a, 0x3a, 0x40],
    },
    Art {
        png: include_bytes!("../../../assets/coleco_neutral.png"),
        w: 295,
        h: 360,
        label: (29, 29, 237, 209),
        colour: [0x2a, 0x2a, 0x2c],
    },
    Art {
        png: include_bytes!("../../../assets/intv_neutral.png"),
        w: 177,
        h: 360,
        label: (17, 23, 143, 148),
        colour: [0x2a, 0x2a, 0x2a],
    },
    Art {
        png: include_bytes!("../../../assets/vectrex_neutral.png"),
        w: 300,
        h: 352,
        label: (30, 38, 240, 180),
        colour: [0x1e, 0x1e, 0x2a],
    },
    Art {
        png: include_bytes!("../../../assets/odyssey_neutral.png"),
        w: 340,
        h: 286,
        label: (36, 100, 268, 157),
        colour: [0x26, 0x26, 0x28],
    },
    Art {
        png: include_bytes!("../../../assets/channelf_neutral.png"),
        w: 300,
        h: 336,
        label: (30, 30, 240, 162),
        colour: [0xe0, 0xb8, 0x30],
    },
    Art {
        png: include_bytes!("../../../assets/supervision_neutral.png"),
        w: 320,
        h: 299,
        label: (32, 32, 256, 160),
        colour: [0x34, 0x34, 0x38],
    },
    Art {
        png: include_bytes!("../../../assets/megaduck_neutral.png"),
        w: 300,
        h: 331,
        label: (31, 72, 238, 186),
        colour: [0x88, 0x88, 0x8c],
    },
    Art {
        png: include_bytes!("../../../assets/msx_neutral.png"),
        w: 360,
        h: 192,
        label: (90, 24, 246, 138),
        colour: [0x2c, 0x2c, 0x2e],
    },
    Art {
        png: include_bytes!("../../../assets/floppy35_neutral.png"),
        w: 300,
        h: 313,
        label: (33, 153, 233, 147),
        colour: [0x30, 0x30, 0x34],
    },
    Art {
        png: include_bytes!("../../../assets/floppy525_neutral.png"),
        w: 330,
        h: 330,
        label: (25, 15, 280, 55),
        colour: [0x26, 0x26, 0x28],
    },
    Art {
        png: include_bytes!("../../../assets/tape_neutral.png"),
        w: 360,
        h: 230,
        label: (29, 22, 302, 79),
        colour: [0x3a, 0x3a, 0x3e],
    },
    Art {
        png: include_bytes!("../../../assets/cpc3_neutral.png"),
        w: 288,
        h: 360,
        label: (29, 29, 230, 180),
        colour: [0xd0, 0xd0, 0xcc],
    },
    Art {
        png: include_bytes!("../../../assets/umd_neutral.png"),
        w: 320,
        h: 325,
        label: (18, 15, 284, 284),
        colour: [0xe6, 0xe6, 0xe2],
    },
    Art {
        png: include_bytes!("../../../assets/fantasy_neutral.png"),
        w: 320,
        h: 347,
        label: (41, 46, 238, 192),
        colour: [0xe0, 0x50, 0x60],
    },
];

/// The disc's print runs from the clear hub to just inside the rim, as fractions of its radius:
/// 22.5 mm and 58.5 mm of a 60 mm disc.
const PRINT_INNER: f32 = 22.5 / 60.0;
const PRINT_OUTER: f32 = 58.5 / 60.0;

/// The UMD's print runs from its white hub to just inside its window: 12.6 mm of 28.4 mm.
const UMD_PRINT_INNER: f32 = 12.6 / 28.4;

/// How tall a medium may sit in the slot: the Game Boy pak's seated height.
const SEATED_MAX_H: u32 = 259;

impl Media {
    pub const COUNT: usize = 36;
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
        Media::Pcb,
        Media::Mvs,
        Media::FamicomDisk,
        Media::Nintendo64,
        Media::VirtualBoy,
        Media::PokemonMini,
        Media::GameAndWatch,
        Media::Sega32x,
        Media::Sg1000,
        Media::Atari2600,
        Media::Atari5200,
        Media::Atari7800,
        Media::ColecoVision,
        Media::Intellivision,
        Media::Vectrex,
        Media::Odyssey2,
        Media::ChannelF,
        Media::Supervision,
        Media::MegaDuck,
        Media::Msx,
        Media::Floppy35,
        Media::Floppy525,
        Media::Tape,
        Media::AmstradDisk,
        Media::Umd,
        Media::Fantasy,
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
            Platform::NeoGeoCd
            | Platform::SegaCd
            | Platform::Dreamcast
            | Platform::PcEngineCd
            | Platform::PcFx => Media::Disc,
            Platform::SuperGrafx => Media::HuCard,
            Platform::Arcade | Platform::Cps1 | Platform::Cps2 | Platform::Cps3 => Media::Pcb,
            Platform::NeoGeo => Media::Mvs,
            Platform::Fds => Media::FamicomDisk,
            Platform::Nintendo64 => Media::Nintendo64,
            Platform::VirtualBoy => Media::VirtualBoy,
            Platform::PokemonMini => Media::PokemonMini,
            Platform::GameAndWatch => Media::GameAndWatch,
            Platform::Sega32x => Media::Sega32x,
            Platform::Sg1000 => Media::Sg1000,
            Platform::Atari2600 => Media::Atari2600,
            Platform::Atari5200 => Media::Atari5200,
            Platform::Atari7800 => Media::Atari7800,
            Platform::ColecoVision => Media::ColecoVision,
            Platform::Intellivision => Media::Intellivision,
            Platform::Vectrex => Media::Vectrex,
            Platform::Odyssey2 => Media::Odyssey2,
            Platform::ChannelF => Media::ChannelF,
            Platform::Supervision => Media::Supervision,
            Platform::MegaDuck => Media::MegaDuck,
            Platform::Msx => Media::Msx,
            Platform::Amiga | Platform::AtariSt | Platform::Dos | Platform::ScummVm => {
                Media::Floppy35
            }
            Platform::Commodore64 | Platform::Atari800 | Platform::Pc98 | Platform::X68000 => {
                Media::Floppy525
            }
            Platform::ZxSpectrum => Media::Tape,
            Platform::AmstradCpc => Media::AmstradDisk,
            Platform::Psp => Media::Umd,
            Platform::Pico8 | Platform::Tic80 => Media::Fantasy,
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
    /// the disc's ring between its hub and rim, the UMD's between its hub and window. Coverage a
    /// pixel, row by row over the label box.
    pub fn label_mask(self) -> Option<&'static [u8]> {
        static RINGS: [OnceLock<Vec<u8>>; 2] = [const { OnceLock::new() }; 2];
        let (slot, inner, outer) = match self {
            Media::Disc => (0, PRINT_INNER, PRINT_OUTER),
            Media::Umd => (1, UMD_PRINT_INNER, 1.0),
            _ => return None,
        };
        Some(RINGS[slot].get_or_init(|| {
            let (_, _, w, h) = self.label();
            ring(w, h, inner, outer)
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
            if m.label_mask().is_none() {
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
    fn the_umd_prints_round_its_hub_but_not_on_it_or_past_its_window() {
        let ring = Media::Umd.label_mask().expect("the UMD has a ring");
        let (_, _, w, h) = Media::Umd.label();
        let at = |x: u32, y: u32| ring[(y * w + x) as usize];
        assert_eq!(at(w / 2, h / 2), 0, "the hub was printed on");
        assert_eq!(at(w / 2, h / 10), 255, "the print is missing above the hub");
        assert_eq!(at(0, 0), 0, "the corner past the window was printed on");
    }

    #[test]
    fn nothing_sits_taller_in_the_slot_than_a_game_boy_pak() {
        for m in Media::ALL {
            let (w, h) = m.seated(240);
            assert!(h <= SEATED_MAX_H && w <= 240, "{m:?} sits at {w}x{h}");
        }
    }
}
