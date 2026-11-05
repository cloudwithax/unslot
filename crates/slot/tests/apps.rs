mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{boot, tmp_root_with_carts};
use slot::app::{App, PageModel, Phase};
use slot_input::{Action, Btn};
use slot_store::Platform;
use slot_ui::ListPage;

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn until(a: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut now = 0;
    while !done(a) && Instant::now() < deadline {
        now += 16;
        a.tick_ms(now);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(done(a), "it never settled: {:?}", a.phase());
}

fn list(a: &App) -> ListPage {
    match a.page_model() {
        Some(PageModel::List(page)) => page,
        other => panic!("no list page is open: {other:?}"),
    }
}

/// Takes the app called `name` off the Apps shelf.
fn open_app(root: &Path, name: &str) -> App {
    let mut a = boot(root);
    for _ in 0..Platform::COUNT {
        if a.shelf_platform_name() == Some("Apps") {
            break;
        }
        press(&mut a, Btn::R1);
    }
    assert_eq!(a.shelf_platform_name(), Some("Apps"));
    for _ in 0..30 {
        if a.selected_stem() == Some(name) {
            break;
        }
        press(&mut a, Btn::Right);
    }
    assert_eq!(a.selected_stem(), Some(name));
    press(&mut a, Btn::A);
    a
}

/// Moves the list's cursor onto the row called `label` and opens it.
fn open(a: &mut App, label: &str) {
    for _ in 0..30 {
        let page = list(a);
        if page.rows[page.cursor].label == label {
            press(a, Btn::A);
            return;
        }
        press(a, Btn::Down);
    }
    panic!("never reached {label}");
}

#[test]
fn the_apps_shelf_holds_the_apps_then_the_ports_and_b_goes_back_to_it() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Ports")).unwrap();
    std::fs::write(d.path().join("Ports/Cave Story.sh"), "#!/bin/sh\n").unwrap();
    std::fs::write(d.path().join("Ports/readme.txt"), "not a port").unwrap();
    let mut a = open_app(d.path(), "Files");
    assert_eq!(list(&a).title, "Files");
    press(&mut a, Btn::B);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    assert_eq!(a.shelf_platform_name(), Some("Apps"));
    let mut stems = Vec::new();
    for _ in 0..6 {
        stems.push(a.selected_stem().unwrap().to_string());
        press(&mut a, Btn::Right);
    }
    assert_eq!(
        stems,
        [
            "Files",
            "Books",
            "Manuals",
            "Gallery",
            "Music",
            "Cave Story"
        ]
    );
}

#[test]
fn a_port_is_left_for_the_launcher_and_slot_quits_for_it() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Ports")).unwrap();
    let script = d.path().join("Ports/Cave Story.sh");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    let mut a = open_app(d.path(), "Cave Story");
    let queued = std::fs::read_to_string(d.path().join(".unslot-port")).unwrap();
    assert_eq!(Path::new(&queued), script);
    until(&mut a, App::ready_to_reload);
}

#[test]
fn files_walks_the_card_and_a_picture_steps_through_its_folder() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let shots = d.path().join("Pictures/Trip");
    std::fs::create_dir_all(&shots).unwrap();
    for name in ["a.png", "b.png", "c.jpg"] {
        std::fs::write(shots.join(name), "picture").unwrap();
    }
    std::fs::write(shots.join("notes.bin"), "?").unwrap();
    let mut a = open_app(d.path(), "Gallery");
    assert_eq!(list(&a).rows[0].label, "Trip");
    press(&mut a, Btn::A);
    let names: Vec<String> = list(&a).rows.into_iter().map(|r| r.label).collect();
    assert_eq!(
        names,
        ["a.png", "b.png", "c.jpg"],
        "the gallery showed more than pictures"
    );
    open(&mut a, "b.png");
    let picture = |a: &App| match a.page_model() {
        Some(PageModel::Picture(p)) => p,
        other => panic!("no picture is open: {other:?}"),
    };
    assert_eq!(picture(&a).path, shots.join("b.png"));
    assert_eq!(picture(&a).status, "2 of 3");
    press(&mut a, Btn::Right);
    assert_eq!(picture(&a).path, shots.join("c.jpg"));
    press(&mut a, Btn::Right);
    assert_eq!(picture(&a).path, shots.join("a.png"), "it did not go round");
    press(&mut a, Btn::B);
    assert_eq!(list(&a).rows[list(&a).cursor].label, "b.png");
    press(&mut a, Btn::Y);
    assert_eq!(list(&a).rows[0].label, "Trip", "Y did not go up a folder");
}

fn text_page(a: &App) -> slot_ui::TextPage {
    match a.page_model() {
        Some(PageModel::Text(t)) => t,
        other => panic!("no text is open: {other:?}"),
    }
}

#[test]
fn a_book_turns_its_pages_and_opens_again_where_it_was_left() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Books")).unwrap();
    let text: String = (1..=200).map(|n| format!("Line {n}\n")).collect();
    std::fs::write(d.path().join("Books/Long.txt"), text).unwrap();
    let mut a = open_app(d.path(), "Books");
    open(&mut a, "Long.txt");
    let first = text_page(&a);
    assert_eq!(first.title, "Long");
    assert_eq!(first.lines[0], "Line 1");
    assert!(first.status.starts_with("Page 1 of "), "{}", first.status);
    press(&mut a, Btn::Right);
    let second = text_page(&a);
    assert!(second.status.starts_with("Page 2 of "), "{}", second.status);
    assert_eq!(second.lines[0], format!("Line {}", first.lines.len() + 1));
    press(&mut a, Btn::B);
    open(&mut a, "Long.txt");
    assert_eq!(text_page(&a).lines, second.lines, "the place was not kept");
}

/// A zip with each file stored as it is, as an EPUB may be.
fn stored_zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, body) in files {
        let at = out.len() as u32;
        let head = |sig: u32, out: &mut Vec<u8>, central: bool| {
            out.extend(sig.to_le_bytes());
            if central {
                out.extend(20u16.to_le_bytes());
            }
            out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend(0u32.to_le_bytes());
            out.extend((body.len() as u32).to_le_bytes());
            out.extend((body.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
        };
        head(0x0403_4b50, &mut out, false);
        out.extend(name.as_bytes());
        out.extend(body.as_bytes());
        head(0x0201_4b50, &mut central, true);
        central.extend([0; 6]);
        central.extend(0u32.to_le_bytes());
        central.extend(at.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

#[test]
fn an_epub_reads_its_chapters_in_spine_order_under_its_own_title() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Books")).unwrap();
    let book = stored_zip(&[
        ("mimetype", "application/epub+zip"),
        (
            "META-INF/container.xml",
            r#"<container><rootfiles><rootfile full-path="OEBPS/book.opf"/></rootfiles></container>"#,
        ),
        (
            "OEBPS/book.opf",
            r#"<package><metadata><dc:title>The Tale</dc:title></metadata><manifest>
<item id="one" href="one.xhtml"/><item id="two" href="text/two.xhtml"/></manifest>
<spine><itemref idref="two"/><itemref idref="one"/></spine></package>"#,
        ),
        (
            "OEBPS/one.xhtml",
            "<html><head><style>p{}</style></head><body><p>Second &amp; last.</p></body></html>",
        ),
        (
            "OEBPS/text/two.xhtml",
            "<html><body><h1>Chapter</h1><p>It began.</p></body></html>",
        ),
    ]);
    std::fs::write(d.path().join("Books/tale.epub"), book).unwrap();
    let mut a = open_app(d.path(), "Books");
    open(&mut a, "tale.epub");
    let page = text_page(&a);
    assert_eq!(page.title, "The Tale");
    let lines: Vec<&str> = page
        .lines
        .iter()
        .map(String::as_str)
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(lines, ["Chapter", "It began.", "Second & last."]);
}

/// Two seconds of a tone, as a 16-bit mono WAV at 22050 Hz.
fn tone(path: &Path) {
    let rate = 22_050u32;
    let samples: Vec<i16> = (0..rate * 2)
        .map(|i| ((i as f32 * 0.1).sin() * 12_000.0) as i16)
        .collect();
    let data = samples.len() as u32 * 2;
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + data).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(rate.to_le_bytes());
    wav.extend((rate * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(data.to_le_bytes());
    for s in samples {
        wav.extend(s.to_le_bytes());
    }
    std::fs::write(path, wav).unwrap();
}

#[test]
fn music_plays_into_the_output_and_keeps_playing_once_the_page_is_closed() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Music/Album")).unwrap();
    tone(&d.path().join("Music/Album/01 Tone.wav"));
    tone(&d.path().join("Music/Album/02 Tone.wav"));
    let mut a = open_app(d.path(), "Music");
    let names: Vec<String> = list(&a).rows.into_iter().map(|r| r.label).collect();
    assert_eq!(names, ["01 Tone", "02 Tone"]);
    press(&mut a, Btn::A);
    until(&mut a, |a| list(a).status == "Playing 01 Tone, on after B");

    let ring = slot::audio::Ring::new(4_800);
    let heard = |ring: &slot::audio::Ring| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let mut out = vec![0i16; 960];
            ring.fill(&mut out);
            if out.iter().any(|&s| s != 0) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    };
    assert!(heard(&ring), "nothing reached the output");

    press(&mut a, Btn::B);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    assert_eq!(a.selected_stem(), Some("Music"));
    assert!(heard(&ring), "the music stopped with the page");

    press(&mut a, Btn::A);
    assert!(
        list(&a).status.starts_with("Playing "),
        "{}",
        list(&a).status
    );
    press(&mut a, Btn::Y);
    until(&mut a, |a| !list(a).status.starts_with("Playing"));
}

/// A clip `secs` long in the card's Pictures, made by the ffmpeg here, if there is one.
fn clip(root: &Path, secs: &str) -> bool {
    let clips = root.join("Pictures");
    std::fs::create_dir_all(&clips).unwrap();
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=30",
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440",
            "-t",
            secs,
            "-shortest",
        ])
        .arg(clips.join("clip.mp4"))
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("no ffmpeg here to make a video with; skipping");
        return false;
    }
    true
}

#[test]
fn a_video_shows_its_pictures_through_ffmpeg_and_a_pauses_it() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    if !clip(d.path(), "3") {
        return;
    }
    let mut a = open_app(d.path(), "Gallery");
    open(&mut a, "clip.mp4");
    let serial = |a: &App| match a.page_model() {
        Some(PageModel::Video(v)) => {
            assert!(v.frame.serial == 0 || v.frame.rgba.len() == (680 * 440 * 4) as usize);
            v.frame.serial
        }
        other => panic!("no video is open: {other:?}"),
    };
    until(&mut a, |a| serial(a) > 3);
    press(&mut a, Btn::A);
    std::thread::sleep(Duration::from_millis(100));
    let held = serial(&a);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(serial(&a), held, "the picture moved on while paused");
    press(&mut a, Btn::B);
    assert_eq!(list(&a).title, "Gallery");
}

#[test]
fn a_video_that_plays_to_its_end_goes_back_to_the_list_and_one_that_will_not_play_says_so() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    if !clip(d.path(), "0.5") {
        return;
    }
    std::fs::write(d.path().join("Pictures/broken.mp4"), b"not a video").unwrap();
    let mut a = open_app(d.path(), "Gallery");
    open(&mut a, "clip.mp4");
    assert!(matches!(a.page_model(), Some(PageModel::Video(_))));
    until(&mut a, |a| {
        matches!(a.page_model(), Some(PageModel::List(_)))
    });
    assert_eq!(list(&a).title, "Gallery");
    open(&mut a, "broken.mp4");
    until(&mut a, |a| {
        matches!(a.page_model(), Some(PageModel::Text(_)))
    });
    assert_eq!(text_page(&a).lines, ["Could not play that video"]);
    press(&mut a, Btn::B);
    assert_eq!(list(&a).title, "Gallery");
}
