use slot_store::{initial, sort_key};

#[test]
fn titles_file_digits_first_then_a_to_z_whatever_their_case() {
    let mut names = vec![
        "Zebra",
        "apple",
        "3D Pinball",
        "Metroid",
        "1943",
        "banana",
        "Apotris",
    ];
    names.sort_by_key(|n| sort_key(n));
    assert_eq!(
        names,
        vec![
            "1943",
            "3D Pinball",
            "Apotris",
            "apple",
            "banana",
            "Metroid",
            "Zebra"
        ]
    );
}

#[test]
fn a_title_led_by_punctuation_files_last() {
    let mut names = vec!["[BIOS] Test", "Apotris", "1943"];
    names.sort_by_key(|n| sort_key(n));
    assert_eq!(names, vec!["1943", "Apotris", "[BIOS] Test"]);
}

#[test]
fn digits_and_punctuation_share_one_stop() {
    assert_eq!(initial("1943"), '#');
    assert_eq!(initial("3D Pinball"), '#');
    assert_eq!(initial("[BIOS] Test"), '#');
    assert_eq!(initial("apple"), 'A');
    assert_eq!(initial("Apotris"), 'A');
    assert_eq!(initial("  Metroid"), 'M', "leading space hid the letter");
}

use slot_store::{Cart, Platform};
use slot_ui::Shelf;

fn shelf_of(names: &[&str]) -> Shelf {
    Shelf::new(
        names
            .iter()
            .map(|n| Cart {
                platform: Platform::Gba,
                stem: (*n).to_string(),
                rom: format!("Games/GBA/{n}.gba").into(),
                label: None,
                code: String::new(),
                shell: None,
                title: n.to_uppercase(),
            })
            .collect(),
    )
}

const ROW: [&str; 7] = [
    "1943",
    "Apotris",
    "Advance Wars",
    "Metroid",
    "Mario Kart",
    "Zelda",
    "Zzz",
];

#[test]
fn down_crosses_to_the_next_letter() {
    let mut s = shelf_of(&ROW);
    assert_eq!(s.index, 0);
    s.jump_next_letter();
    assert_eq!(s.carts[s.index].stem, "Apotris");
    s.jump_next_letter();
    assert_eq!(s.carts[s.index].stem, "Metroid");
    s.jump_next_letter();
    assert_eq!(s.carts[s.index].stem, "Zelda");
}

#[test]
fn up_lands_on_the_start_of_the_letter_before_leaving_it() {
    let mut s = shelf_of(&ROW);
    s.select(4);
    s.jump_prev_letter();
    assert_eq!(s.carts[s.index].stem, "Metroid", "it left the Ms too early");
    s.jump_prev_letter();
    assert_eq!(s.carts[s.index].stem, "Apotris");
}

#[test]
fn the_letters_wrap_at_both_ends() {
    let mut s = shelf_of(&ROW);
    s.select(5);
    s.jump_next_letter();
    assert_eq!(
        s.carts[s.index].stem, "1943",
        "the end did not loop forward"
    );

    s.select(0);
    s.jump_prev_letter();
    assert_eq!(
        s.carts[s.index].stem, "Zelda",
        "the start did not loop back"
    );
}

#[test]
fn a_wrap_travels_the_way_the_press_asked() {
    let mut s = shelf_of(&ROW);
    s.select(5);
    let before = s.scroll_target();
    s.jump_next_letter();
    assert!(
        s.scroll_target() > before,
        "Down at the end turned round instead of carrying on forwards"
    );

    s.select(0);
    let before = s.scroll_target();
    s.jump_prev_letter();
    assert!(
        s.scroll_target() < before,
        "Up at the start turned round instead of carrying on backwards"
    );
}

#[test]
fn a_row_of_one_letter_stays_put() {
    let mut s = shelf_of(&["Metroid", "Mario Kart"]);
    s.jump_next_letter();
    assert_eq!(
        s.carts[s.index].stem, "Metroid",
        "it moved within one letter"
    );
    s.jump_prev_letter();
    assert_eq!(s.carts[s.index].stem, "Metroid");
}

#[test]
fn letter_jumps_through_a_huge_shelf_keep_every_frame_cheap() {
    let names: Vec<String> = (0..5000)
        .map(|i| format!("{}{i:04}", (b'A' + (i * 26 / 5000) as u8) as char))
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut s = shelf_of(&names);
    let mut out = Vec::new();
    let mut worst = std::time::Duration::ZERO;
    let mut all = Vec::new();
    for press in 0..40 {
        match press % 3 {
            2 => s.jump_prev_letter(),
            _ => s.jump_next_letter(),
        }
        for _ in 0..30 {
            let start = std::time::Instant::now();
            s.update(1.0 / 60.0);
            out.clear();
            s.draw_row(slot_ui::Canvas::FRAMED, None, 0.0, 0.0, 1.0, &mut out);
            worst = worst.max(start.elapsed());
            all.push(start.elapsed());
        }
    }
    all.sort();
    let p99 = all[all.len() * 99 / 100];
    assert!(
        p99 < std::time::Duration::from_micros(500),
        "a frame on a big shelf took {p99:?} at the 99th percentile, {worst:?} at worst"
    );
}

#[test]
fn a_letter_jump_keeps_carts_on_screen_the_frame_after() {
    let names: Vec<String> = (0..2000)
        .map(|i| format!("{}{i:04}", (b'A' + (i * 26 / 2000) as u8) as char))
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut s = shelf_of(&names);
    let mut out = Vec::new();
    for _ in 0..10 {
        s.jump_next_letter();
        s.update(1.0 / 60.0);
        out.clear();
        s.draw_row(slot_ui::Canvas::FRAMED, None, 0.0, 0.0, 1.0, &mut out);
        assert!(!out.is_empty(), "the shelf drew nothing mid-jump");
    }
}
