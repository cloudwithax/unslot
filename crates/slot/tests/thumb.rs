use slot::thumb;
use slot_retro::{GBA_H, GBA_W};

fn decode(png_bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let mut reader = png::Decoder::new(png_bytes).read_info().expect("read info");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("decode");
    assert_eq!(info.color_type, png::ColorType::Rgb);
    buf.truncate(info.buffer_size());
    (buf, info.width, info.height)
}

#[test]
fn a_thumbnail_keeps_the_frames_colours() {
    let mut frame = vec![0u8; (GBA_W * GBA_H * 4) as usize];
    for px in frame.chunks_exact_mut(4) {
        px.copy_from_slice(&[0x20, 0x40, 0xd0, 0xff]);
    }
    let encoded = thumb::png(&frame, (GBA_W, GBA_H)).expect("encode");
    let (rgb, w, h) = decode(&encoded);
    assert_eq!((w, h), (GBA_W, GBA_H));
    assert_eq!(&rgb[..3], &[0xd0, 0x40, 0x20]);
}

#[test]
fn a_short_frame_is_not_a_thumbnail() {
    assert!(thumb::png(&[], (GBA_W, GBA_H)).is_none());
    assert!(thumb::png(&vec![0u8; (GBA_W * GBA_H * 4) as usize - 4], (GBA_W, GBA_H)).is_none());
}

#[test]
fn a_thumbnail_takes_the_size_of_the_frame_it_was_given() {
    let frame = vec![0u8; 256 * 224 * 4];
    let (_, w, h) = decode(&thumb::png(&frame, (256, 224)).expect("encode"));
    assert_eq!((w, h), (256, 224));
}
