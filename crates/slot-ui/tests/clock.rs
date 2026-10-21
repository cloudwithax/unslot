use slot_ui::{clock_label, Polaroids};

#[test]
fn recent_states_read_relative_and_old_ones_read_absolute() {
    let now = "2026-08-09_14-36-05";
    assert_eq!(
        Polaroids::relative_time("2026-08-09_14-32-05", now),
        "4 min ago"
    );
    assert_eq!(
        Polaroids::relative_time("2026-08-09_03-00-00", now),
        "11 hr ago"
    );
    assert_eq!(
        Polaroids::relative_time("2026-08-08_20-00-00", now),
        "2026-08-08 20:00"
    );
    assert_eq!(
        Polaroids::relative_time("2025-01-02_09-05-00", now),
        "2025-01-02 09:05"
    );
}

#[test]
fn the_clock_is_24_hour_and_shows_no_seconds() {
    assert_eq!(
        clock_label("2026-08-09_21-07-00"),
        "21:07",
        "not 24 hour, or showing seconds"
    );
}

#[test]
fn a_12_hour_clock_reads_am_and_pm_with_twelve_for_noon_and_midnight() {
    use slot_ui::{date_time, time_text};
    let at = |h: i64, m: i64| h * 3600 + m * 60;
    assert_eq!(time_text(at(19, 5), false), "19:05");
    assert_eq!(time_text(at(19, 5), true), "7:05 PM");
    assert_eq!(time_text(at(0, 30), true), "12:30 AM", "midnight is twelve");
    assert_eq!(
        time_text(at(12, 0), true),
        "12:00 PM",
        "noon is twelve, after midday"
    );
    assert_eq!(time_text(at(9, 41), true), "9:41 AM");
    assert_eq!(date_time(at(13, 0), true), "JAN 1 1:00 PM");
    assert_eq!(date_time(at(13, 0), false), "JAN 1 13:00");
}
