//! Boot every system past the Game Boy family on its real core and check the host hands each
//! one what it needs: a picture at the core's own size, a frame rate, sound, and a save state
//! that goes back in. The ROMs are a few bytes of hand-written machine code that loop forever,
//! so nothing copyrighted is involved.
//!
//! Point SLOT_SYSTEM_CORES at a folder of this machine's builds of the cores, e.g. the
//! buildbot's linux/x86_64 ones; without it the test has nothing to boot and passes.

use std::path::{Path, PathBuf};

use slot_retro::{ButtonMask, Canvas, LibretroCore, RetroCore};

struct Case {
    core: &'static str,
    ext: &'static str,
    rom: fn() -> Vec<u8>,
    /// The smallest and largest picture the core may plausibly draw for this system.
    min: (u32, u32),
    max: (u32, u32),
    fps: (f64, f64),
}

/// NROM-128: a 16 KiB PRG bank holding `JMP $C000`, the reset vector pointing at it, CHR RAM.
fn nes() -> Vec<u8> {
    let mut rom = b"NES\x1a\x01\x00\x00\x00".to_vec();
    rom.resize(16, 0);
    let mut prg = vec![0xea; 0x4000];
    prg[..3].copy_from_slice(&[0x4c, 0x00, 0xc0]);
    for v in [0x3ffa, 0x3ffc, 0x3ffe] {
        prg[v..v + 2].copy_from_slice(&[0x00, 0xc0]);
    }
    rom.extend(prg);
    rom
}

/// LoROM: `BRA -2` at $8000, the emulation-mode reset vector pointing at it, and a header whose
/// checksum and its complement agree so the core reads it as LoROM.
fn snes() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[..2].copy_from_slice(&[0x80, 0xfe]);
    rom[0x7fc0..0x7fd5].copy_from_slice(b"UNSLOT SMOKE TEST    ");
    rom[0x7fd5] = 0x20;
    rom[0x7fd7] = 0x08;
    rom[0x7ffc..0x7ffe].copy_from_slice(&[0x00, 0x80]);
    let sum = rom.iter().fold(0u16, |s, b| s.wrapping_add(u16::from(*b)));
    rom[0x7fdc..0x7fde].copy_from_slice(&(!sum).to_le_bytes());
    rom[0x7fde..0x7fe0].copy_from_slice(&sum.to_le_bytes());
    rom
}

/// A 68000 vector table, the SEGA header and `BRA.S *` at $200.
fn genesis() -> Vec<u8> {
    let mut rom = vec![0u8; 0x10000];
    rom[..4].copy_from_slice(&0x00ff_fe00u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x0000_0200u32.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1a0..0x1a8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0xff, 0xff]);
    rom[0x200..0x202].copy_from_slice(&[0x60, 0xfe]);
    rom
}

/// `DI; JR $` at the Z80's reset address, and the TMR SEGA header.
fn sega_8bit(region: u8) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[..3].copy_from_slice(&[0xf3, 0x18, 0xfe]);
    rom[0x7ff0..0x7ff8].copy_from_slice(b"TMR SEGA");
    rom[0x7fff] = region << 4 | 0x0c;
    rom
}

fn sms() -> Vec<u8> {
    sega_8bit(4)
}

fn game_gear() -> Vec<u8> {
    sega_8bit(7)
}

/// An 8 KiB HuCard: `BRA *` at $E000 and the reset vector at $FFFE pointing at it.
fn pce() -> Vec<u8> {
    let mut rom = vec![0u8; 0x2000];
    rom[..2].copy_from_slice(&[0x80, 0xfe]);
    rom[0x1ffe..0x2000].copy_from_slice(&[0x00, 0xe0]);
    rom
}

/// The SNK licence line, a start address in the cart and `JR T,$` there.
fn ngp() -> Vec<u8> {
    let mut rom = vec![0u8; 0x20000];
    rom[..28].copy_from_slice(b"COPYRIGHT BY SNK CORPORATION");
    rom[0x1c..0x20].copy_from_slice(&0x0020_0040u32.to_le_bytes());
    rom[0x23] = 0x10;
    rom[0x24..0x30].copy_from_slice(b"UNSLOT SMOKE");
    rom[0x40..0x42].copy_from_slice(&[0x68, 0xfe]);
    rom
}

/// 128 KiB with the V30's reset jump in the last 16 bytes landing on `JMP $` at F000:0000.
fn wonderswan() -> Vec<u8> {
    let mut rom = vec![0u8; 0x20000];
    let base = rom.len() - 0x10000;
    rom[base..base + 2].copy_from_slice(&[0xeb, 0xfe]);
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xea, 0x00, 0x00, 0x00, 0xf0]);
    rom
}

/// A PS-X EXE that loads at $80010000 and spins there: `J $80010000; NOP`.
fn playstation() -> Vec<u8> {
    let mut exe = vec![0u8; 0x800];
    exe[..8].copy_from_slice(b"PS-X EXE");
    let word =
        |exe: &mut Vec<u8>, at: usize, v: u32| exe[at..at + 4].copy_from_slice(&v.to_le_bytes());
    word(&mut exe, 0x10, 0x8001_0000);
    word(&mut exe, 0x18, 0x8001_0000);
    word(&mut exe, 0x1c, 0x800);
    word(&mut exe, 0x30, 0x801f_ff00);
    exe[0x4c..0x4c + 22].copy_from_slice(b"Sony Computer Entertai");
    let mut code = vec![0u8; 0x800];
    code[..4].copy_from_slice(&(0x0800_0000u32 | (0x0001_0000 >> 2)).to_le_bytes());
    exe.extend(code);
    exe
}

const CASES: &[Case] = &[
    Case {
        core: "fceumm",
        ext: "nes",
        rom: nes,
        min: (240, 224),
        max: (256, 240),
        fps: (59.0, 61.0),
    },
    Case {
        core: "snes9x",
        ext: "sfc",
        rom: snes,
        min: (256, 224),
        max: (512, 478),
        fps: (59.0, 61.0),
    },
    Case {
        core: "genesis_plus_gx",
        ext: "md",
        rom: genesis,
        min: (256, 192),
        max: (320, 240),
        fps: (59.0, 61.0),
    },
    Case {
        core: "genesis_plus_gx",
        ext: "sms",
        rom: sms,
        min: (248, 192),
        max: (256, 240),
        fps: (59.0, 61.0),
    },
    Case {
        core: "genesis_plus_gx",
        ext: "gg",
        rom: game_gear,
        min: (160, 144),
        max: (256, 224),
        fps: (59.0, 61.0),
    },
    Case {
        core: "mednafen_pce_fast",
        ext: "pce",
        rom: pce,
        min: (240, 200),
        max: (512, 243),
        fps: (59.0, 61.0),
    },
    Case {
        core: "race",
        ext: "ngc",
        rom: ngp,
        min: (160, 152),
        max: (160, 152),
        fps: (59.0, 61.0),
    },
    Case {
        core: "mednafen_wswan",
        ext: "ws",
        rom: wonderswan,
        min: (144, 144),
        max: (224, 224),
        fps: (75.0, 76.0),
    },
    Case {
        core: "pcsx_rearmed",
        ext: "exe",
        rom: playstation,
        min: (256, 224),
        max: (640, 512),
        fps: (49.0, 61.0),
    },
];

fn cores() -> Option<PathBuf> {
    std::env::var_os("SLOT_SYSTEM_CORES").map(PathBuf::from)
}

fn boot(dir: &Path, case: &Case, scratch: &Path) -> Result<String, String> {
    let lib = dir.join(format!(
        "{}_libretro.{}",
        case.core,
        std::env::consts::DLL_EXTENSION
    ));
    if !lib.exists() {
        return Err(format!("no {}", lib.display()));
    }
    let rom = scratch.join(format!("smoke.{}", case.ext));
    std::fs::write(&rom, (case.rom)()).map_err(|e| e.to_string())?;
    let mut core = LibretroCore::open_with(&lib, scratch, scratch).map_err(|e| e.to_string())?;
    core.set_canvas(Canvas::Native);
    if !core.valid_extensions().iter().any(|e| e == case.ext) {
        return Err(format!(
            "does not open .{}: {:?}",
            case.ext,
            core.valid_extensions()
        ));
    }
    core.load(&rom).map_err(|e| e.to_string())?;
    let mut sound = 0;
    for _ in 0..120 {
        core.run_frame(ButtonMask::default());
        sound += core.take_audio().len();
    }
    let (w, h) = core.video_size();
    let av = core.av_info();
    let mut faults = Vec::new();
    if w < case.min.0 || h < case.min.1 || w > case.max.0 || h > case.max.1 {
        faults.push(format!("drew {w}x{h}"));
    }
    if core.video_xrgb8888().len() != (w * h * 4) as usize {
        faults.push("the picture is not its size".into());
    }
    if !(case.fps.0..=case.fps.1).contains(&av.fps) {
        faults.push(format!("runs at {:.3} fps", av.fps));
    }
    if av.sample_rate <= 0.0 || sound == 0 {
        faults.push(format!("{sound} samples at {} Hz", av.sample_rate));
    }
    if core
        .display_aspect()
        .is_none_or(|a| !(1.0..=2.0).contains(&a))
    {
        faults.push(format!("aspect {:?}", core.display_aspect()));
    }
    match core.serialize() {
        Ok(state) if !state.is_empty() => {
            if let Err(e) = core.unserialize(&state) {
                faults.push(format!("its state would not go back in: {e}"));
            }
        }
        Ok(_) => faults.push("an empty state".into()),
        Err(e) => faults.push(format!("no state: {e}")),
    }
    match faults.is_empty() {
        true => Ok(format!(
            "{w}x{h} at {:.2} fps, {} Hz, aspect {:.3}, {} disks",
            av.fps,
            av.sample_rate,
            core.display_aspect().unwrap_or_default(),
            core.disk_count()
        )),
        false => Err(faults.join("; ")),
    }
}

#[test]
fn every_system_boots_on_its_core_and_draws_at_its_own_size() {
    let Some(dir) = cores() else {
        eprintln!("SLOT_SYSTEM_CORES is not set, so there are no cores to boot");
        return;
    };
    let scratch = tempfile::tempdir().expect("scratch dir");
    let mut failed = Vec::new();
    for case in CASES {
        match boot(&dir, case, scratch.path()) {
            Ok(said) => eprintln!("{:>18} .{:<4} {said}", case.core, case.ext),
            Err(why) => {
                eprintln!("{:>18} .{:<4} FAILED: {why}", case.core, case.ext);
                failed.push(format!("{} .{}: {why}", case.core, case.ext));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
