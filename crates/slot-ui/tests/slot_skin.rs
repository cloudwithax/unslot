use slot_store::SpColour;
use slot_ui::{slot_skin, sp_slot, strip_is_light, SLOT_H, SLOT_W};

const STRIP: usize = (SLOT_W * SLOT_H * 4) as usize;

#[test]
fn every_sp_colour_is_a_full_slot_whose_ink_suits_it() {
    for colour in SpColour::ALL {
        let skin = sp_slot(colour);
        for strip in [&skin.back, &skin.front, &skin.port] {
            assert_eq!(strip.len(), STRIP, "{colour:?}");
        }
        assert_eq!(
            strip_is_light(&skin.front),
            colour.light(),
            "{colour:?}'s footer ink does not suit its shell"
        );
    }
}

fn png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = (0..w * h).flat_map(|_| rgba).collect();
    enc.write_header().unwrap().write_image_data(&data).unwrap();
    out
}

#[test]
fn a_slot_of_your_own_is_fitted_and_borrows_a_port_that_suits_it() {
    let pale = png(360, 29, [230, 230, 230, 255]);
    let skin = slot_skin(&pale, &pale, None).expect("it decodes");
    assert_eq!((skin.back.len(), skin.front.len()), (STRIP, STRIP));
    assert_eq!(skin.port, slot_ui::link_port_light());
    let dark = png(SLOT_W, SLOT_H, [20, 20, 20, 255]);
    let skin = slot_skin(&dark, &dark, None).unwrap();
    assert_eq!(skin.port, slot_ui::link_port());
}

#[test]
fn a_strip_that_is_not_a_png_is_refused() {
    assert!(slot_skin(b"nope", b"nope", None).is_none());
}
