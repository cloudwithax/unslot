use slot::drc::Level;

#[test]
fn the_rate_never_moves_the_pitch_enough_to_hear() {
    let mut level = Level::new();
    for queued in [0, 1, 100, 100_000] {
        for _ in 0..100 {
            let r = level.push(1.0 / 60.0, queued, 2048);
            assert!(
                (Level::MIN..=Level::MAX).contains(&r),
                "rate {r} out of bounds for queued={queued}"
            );
        }
    }
}

#[test]
fn a_sink_with_no_buffer_yet_yields_a_neutral_ratio() {
    let mut level = Level::new();
    assert_eq!(level.push(1.0 / 60.0, 0, 0), 1.0);
}

#[test]
fn an_empty_queue_is_played_slower_than_a_full_one() {
    let mut level = Level::new();
    let empty = level.push(1.0 / 60.0, 0, 2048);
    let full = level.push(1.0 / 60.0, 4096, 2048);
    assert!(empty > 1.0, "an empty queue was not slowed: {empty}");
    assert!(full < 1.0, "a full queue was not quickened: {full}");
}
