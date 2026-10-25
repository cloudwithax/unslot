use std::time::Duration;

use slot_ftrace::{self as ft, ev, RECORD};

#[test]
fn records_reach_the_file_with_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.bin");
    std::env::set_var(ft::VAR, &path);
    assert!(ft::enabled());
    {
        let _s = ft::span(ev::EMU_ITER, 7);
        ft::counter(ev::EMU_RAN, -3);
    }
    ft::instant(ev::TICK, 1);
    std::thread::sleep(Duration::from_millis(600));

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len(), 4 * RECORD);
    let rec = |i: usize| {
        let r = &bytes[i * RECORD..(i + 1) * RECORD];
        (
            u64::from_le_bytes(r[0..8].try_into().unwrap()),
            u16::from_le_bytes(r[12..14].try_into().unwrap()),
            r[14],
            r[15],
            i64::from_le_bytes(r[16..24].try_into().unwrap()),
        )
    };
    let got: Vec<_> = (0..4).map(rec).collect();
    assert_eq!(
        got.iter().map(|r| (r.1, r.2, r.3, r.4)).collect::<Vec<_>>(),
        [
            (ev::EMU_ITER, ft::BEGIN, 0, 7),
            (ev::EMU_RAN, ft::COUNTER, 0, -3),
            (ev::EMU_ITER, ft::END, 0, 7),
            (ev::TICK, ft::INSTANT, 0, 1),
        ]
    );
    assert!(got.windows(2).all(|w| w[0].0 <= w[1].0));
    let names = std::fs::read_to_string(dir.path().join("trace.bin.names")).unwrap();
    assert!(names.contains(&format!("{} emu.iter\n", ev::EMU_ITER)));
}
