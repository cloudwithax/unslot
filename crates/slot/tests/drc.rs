use slot::drc::Level;

#[test]
fn the_rate_stays_within_its_limits() {
    let mut level = Level::new();
    let target = 2048;
    for queued in [0, 1, 100, 2048, 4096, 100_000] {
        for _ in 0..100 {
            let r = level.push(1.0 / 60.0, queued, target);
            // A queue close to full may be drained below the 1% a pitch may move by: see
            // Level::FULL_MIN.
            let floor = if queued > target * 3 / 2 {
                Level::FULL_MIN
            } else {
                Level::MIN
            };
            assert!(
                (floor..=Level::MAX).contains(&r),
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
