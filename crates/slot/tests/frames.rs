use slot::frames::{Frames, Picture, Shape, SharedPicture};

fn publish(frames: &Frames, value: u8, size: usize) {
    let mut buf = frames.take_write();
    buf.clear();
    buf.resize(size, value);
    frames.publish(Picture::bytes(buf), Shape::default());
}

#[test]
fn the_reader_gets_the_newest_frame_and_never_the_same_one_twice() {
    let f = Frames::new(4);
    for i in 0..50u8 {
        publish(&f, i, 4);
    }
    let latest = f.latest().expect("a published frame should be readable");
    assert_eq!(
        latest.pixels().expect("the frame's pixels"),
        &[49u8; 4],
        "the reader was handed a stale frame"
    );
    drop(latest);
    assert!(
        f.latest().is_none(),
        "a consumed frame was handed out twice"
    );
}

#[test]
fn buffers_are_recycled_rather_than_reallocated_per_frame() {
    let f = Frames::new(4);
    publish(&f, 1, 4);
    let held = f.latest().expect("a published frame should be readable");
    for i in 0..500u32 {
        publish(&f, i as u8, 4);
    }
    drop(held);
    publish(&f, 0, 4);
    assert!(
        f.allocated() <= 3,
        "{} buffers in flight, the pool is leaking",
        f.allocated()
    );
}

/// A frame the core left in a buffer of its own, which this frontend can draw where it is.
fn shared(index: usize) -> SharedPicture {
    let file = std::fs::File::open("/dev/null").expect("/dev/null");
    SharedPicture {
        generation: 7,
        index,
        width: 640,
        height: 480,
        fds: Some(vec![std::os::fd::OwnedFd::from(file)]),
    }
}

#[test]
fn a_frame_left_in_a_core_buffer_reaches_the_reader_with_its_buffer() {
    let f = Frames::new(4);
    f.publish(Picture::shared(shared(2)), Shape::default());
    let shown = f.latest().expect("a shared frame should be readable");
    assert!(
        shown.pixels().is_none(),
        "a frame that was not copied out carried pixels"
    );
    let buffer = shown.buffer().expect("the buffer the frame is in");
    assert_eq!(
        (buffer.generation, buffer.index, buffer.width, buffer.height),
        (7, 2, 640, 480)
    );
    assert_eq!(
        buffer.fds.as_ref().map(Vec::len),
        Some(1),
        "the buffers of the set did not come with it"
    );
}

#[test]
fn a_frame_that_came_both_ways_offers_pixels_and_its_buffer() {
    let f = Frames::new(4);
    let mut buf = f.take_write();
    buf.clear();
    buf.extend_from_slice(&[1, 2, 3, 4]);
    f.publish(Picture::both(buf, shared(0)), Shape::default());
    let both = f.latest().expect("a frame should be readable");
    assert_eq!(both.pixels(), Some(&[1u8, 2, 3, 4][..]));
    assert!(both.buffer().is_some());
    // The pixels go back to the pool, and the buffer's details are not needed for that.
    f.publish(Picture::shared(shared(1)), Shape::default());
    let buffer_only = f.latest().expect("a frame should be readable");
    assert!(buffer_only.pixels().is_none());
    assert!(buffer_only.buffer().is_some());
    // Held by the reader, the pixels are still out of the pool; let go, they come back to it.
    drop(both);
    drop(buffer_only);
    publish(&f, 9, 4);
    assert_eq!(
        f.allocated(),
        1,
        "the pixels were not handed back to the pool"
    );
}
