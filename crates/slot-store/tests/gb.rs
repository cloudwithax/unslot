use slot_store::gb::Class;
use std::path::PathBuf;

fn card_rom(rel: &str) -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../sdcard/Games")
        .join(rel);
    if p.is_file() { Some(p) } else { None }
}

#[test]
fn a_dmg_cart_reads_its_title_and_flag() {
    let Some(rom) = card_rom("GB/Tetris Rosy Retrospection.gb") else {
        eprintln!("no Game Boy ROM on the card: skipping");
        return;
    };
    assert_eq!(slot_store::gb::title(&rom).as_deref(), Some("TETRIS"));
    assert_eq!(slot_store::gb::cgb_flag(&rom), Some(0x00));
    assert_eq!(slot_store::gb::class(&rom), Class::Original);
}

#[test]
fn a_colour_only_cart_has_no_title_and_that_is_fine() {
    let Some(rom) = card_rom("GBC/Tetris Chromatic.gbc") else {
        eprintln!("no Game Boy Color ROM on the card: skipping");
        return;
    };
    assert_eq!(
        slot_store::gb::title(&rom),
        None,
        "an empty title must read as none"
    );
    assert_eq!(slot_store::gb::cgb_flag(&rom), Some(0xc0));
    assert_eq!(slot_store::gb::class(&rom), Class::ColourOnly);
}

#[test]
fn a_colour_enhanced_cart_is_its_own_class_and_not_a_colour_only_one() {
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("Enhanced.gb");
    let mut bytes = vec![0u8; 0x150];
    bytes[0x134..0x134 + 6].copy_from_slice(b"ZELDA\0");
    bytes[0x143] = 0x80;
    std::fs::write(&rom, bytes).unwrap();

    assert_eq!(slot_store::gb::title(&rom).as_deref(), Some("ZELDA"));
    assert_eq!(slot_store::gb::cgb_flag(&rom), Some(0x80));
    assert_eq!(slot_store::gb::class(&rom), Class::DualMode);
    assert_ne!(slot_store::gb::class(&rom), Class::ColourOnly);
}

#[test]
fn the_title_read_does_not_swallow_the_manufacturer_code_or_the_flag() {
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("Long.gbc");
    let mut bytes = vec![0u8; 0x150];
    bytes[0x134..0x134 + 11].copy_from_slice(b"ABCDEFGHIJK");
    bytes[0x13f..0x143].copy_from_slice(b"WXYZ");
    bytes[0x143] = 0xc0;
    std::fs::write(&rom, bytes).unwrap();

    assert_eq!(slot_store::gb::title(&rom).as_deref(), Some("ABCDEFGHIJK"));
    assert_eq!(slot_store::gb::cgb_flag(&rom), Some(0xc0));
}

#[test]
fn a_truncated_rom_is_not_a_panic() {
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("Short.gb");
    std::fs::write(&rom, [0u8; 0x50]).unwrap();
    assert_eq!(slot_store::gb::title(&rom), None);
    assert_eq!(slot_store::gb::cgb_flag(&rom), None);
}

#[test]
fn a_flag_the_manual_never_named_falls_back_to_the_original_pak() {
    let d = tempfile::tempdir().unwrap();
    for (name, flag) in [("Junk.gb", 0x42u8), ("Half.gb", 0x40)] {
        let rom = d.path().join(name);
        let mut bytes = vec![0u8; 0x150];
        bytes[0x143] = flag;
        std::fs::write(&rom, bytes).unwrap();
        assert_eq!(slot_store::gb::class(&rom), Class::Original, "{name}");
    }
    let short = d.path().join("Short.gb");
    std::fs::write(&short, [0u8; 0x50]).unwrap();
    assert_eq!(slot_store::gb::class(&short), Class::Original);
}

fn header_rom(title: &[u8], code: &[u8], cgb: u8, dest: u8) -> Vec<u8> {
    let mut bytes = vec![0u8; 0x150];
    bytes[0x134..0x134 + title.len()].copy_from_slice(title);
    bytes[0x13f..0x13f + code.len()].copy_from_slice(code);
    bytes[0x143] = cgb;
    bytes[0x14a] = dest;
    bytes
}

#[test]
fn the_header_reads_the_manufacturer_code_and_the_destination() {
    let h = slot_store::gb::Header::parse(&header_rom(b"POKEMON_GLD", b"AAUE", 0x80, 0x01))
        .expect("a full header");
    assert_eq!(h.title, "POKEMON_GLD");
    assert_eq!(h.code, "AAUE");
    assert_eq!(h.class(), Class::DualMode);
    assert!(!h.japan);

    let jp = slot_store::gb::Header::parse(&header_rom(b"POKEMON RED", b"", 0x00, 0x00)).unwrap();
    assert!(jp.japan);
    assert_eq!(jp.code, "");
}

#[test]
fn the_tail_of_a_long_title_is_not_a_code() {
    let h = slot_store::gb::Header::parse(&header_rom(b"POKEMON YELLOW", b"", 0x80, 0x01)).unwrap();
    assert_eq!(h.title, "POKEMON YEL");
    assert_eq!(h.code, "");
}

#[test]
fn the_header_is_read_from_the_file_and_a_short_one_is_none() {
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("Crystal.gbc");
    std::fs::write(&rom, header_rom(b"PM_CRYSTAL", b"BYTE", 0xc0, 0x01)).unwrap();
    let h = slot_store::gb::header(&rom).expect("header");
    assert_eq!((h.title.as_str(), h.code.as_str()), ("PM_CRYSTAL", "BYTE"));
    assert_eq!(h.class(), Class::ColourOnly);

    let short = d.path().join("Short.gb");
    std::fs::write(&short, [0u8; 0x144]).unwrap();
    assert!(slot_store::gb::header(&short).is_none());
}
