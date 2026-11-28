mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{boot, stored_zip, tmp_root_with_carts};
use slot::app::{App, PageModel, Phase};
use slot_input::{Action, Btn};
use slot_store::Platform;
use slot_ui::{AppPage, FilesPage, GalleryPage, LibraryPage, MusicPage, ReaderPage, ViewerPage};

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

fn files_page(a: &App) -> FilesPage {
    match a.page_model() {
        Some(PageModel::App(AppPage::Files(page))) => page,
        other => panic!("no files page is open: {other:?}"),
    }
}

fn gallery_page(a: &App) -> GalleryPage {
    match a.page_model() {
        Some(PageModel::App(AppPage::Gallery(page))) => page,
        other => panic!("no gallery page is open: {other:?}"),
    }
}

fn viewer_page(a: &App) -> ViewerPage {
    match a.page_model() {
        Some(PageModel::App(AppPage::Viewer(page))) => page,
        other => panic!("no viewer page is open: {other:?}"),
    }
}

fn library_page(a: &App) -> LibraryPage {
    match a.page_model() {
        Some(PageModel::App(AppPage::Library(page))) => page,
        other => panic!("no library page is open: {other:?}"),
    }
}

fn reader_page(a: &App) -> ReaderPage {
    match a.page_model() {
        Some(PageModel::App(AppPage::Reader(page))) => page,
        other => panic!("no reader page is open: {other:?}"),
    }
}

fn music_page(a: &App) -> MusicPage {
    match a.page_model() {
        Some(PageModel::App(AppPage::Music(page))) => page,
        other => panic!("no music page is open: {other:?}"),
    }
}

fn text_page(a: &App) -> slot_ui::TextPage {
    match a.page_model() {
        Some(PageModel::Text(t)) => t,
        other => panic!("no text is open: {other:?}"),
    }
}

/// Takes the app called `name` off the Apps shelf.
fn open_app(root: &Path, name: &str) -> App {
    open_from(root, "Apps", name)
}

/// Takes the cart called `name` off the shelf called `shelf`.
fn open_from(root: &Path, shelf: &str, name: &str) -> App {
    let mut a = boot(root);
    for _ in 0..Platform::COUNT {
        if a.shelf_platform_name() == Some(shelf) {
            break;
        }
        press(&mut a, Btn::R1);
    }
    assert_eq!(a.shelf_platform_name(), Some(shelf));
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

fn select_file(a: &mut App, name: &str) {
    for _ in 0..30 {
        let page = files_page(a);
        if page.rows[page.cursor].name == name {
            return;
        }
        press(a, Btn::Down);
    }
    panic!("never reached {name}");
}

fn open_gallery(a: &mut App, name: &str) {
    for _ in 0..30 {
        let page = gallery_page(a);
        if page.tiles[page.cursor].name == name {
            press(a, Btn::A);
            return;
        }
        press(a, Btn::Right);
    }
    panic!("never reached {name}");
}

fn open_book(a: &mut App, name: &str) {
    for _ in 0..30 {
        let page = library_page(a);
        if page.tiles[page.cursor].name == name {
            press(a, Btn::A);
            return;
        }
        press(a, Btn::Right);
    }
    panic!("never reached {name}");
}

fn pick_menu(a: &mut App, label: &str) {
    for _ in 0..15 {
        let menu = files_page(a).menu.expect("menu is not open");
        if menu.rows[menu.cursor].1 == label {
            press(a, Btn::A);
            return;
        }
        press(a, Btn::Down);
    }
    panic!("never reached menu row {label}");
}

#[test]
fn the_apps_shelf_holds_the_apps_and_b_goes_back_to_it() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = open_app(d.path(), "Files");
    assert_eq!(files_page(&a).title, "Files");
    press(&mut a, Btn::B);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    assert_eq!(a.shelf_platform_name(), Some("Apps"));
    let mut stems = Vec::new();
    for _ in 0..5 {
        stems.push(a.selected_stem().unwrap().to_string());
        press(&mut a, Btn::Right);
    }
    assert_eq!(stems, ["Files", "Books", "Manuals", "Gallery", "Music"]);
}

#[test]
fn the_ports_have_a_shelf_of_their_own_as_discs() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Ports")).unwrap();
    std::fs::write(d.path().join("Ports/Cave Story.sh"), "#!/bin/sh\n").unwrap();
    std::fs::write(d.path().join("Ports/Doom.sh"), "#!/bin/sh\n").unwrap();
    std::fs::write(d.path().join("Ports/readme.txt"), "not a port").unwrap();
    let a = boot(d.path());
    let ports: Vec<&str> = a
        .carts()
        .filter(|c| c.platform == Platform::Ports)
        .map(|c| c.stem.as_str())
        .collect();
    assert_eq!(ports, ["Cave Story", "Doom"]);
    assert_eq!(
        slot_ui::Media::of(Platform::Ports),
        Some(slot_ui::Media::Disc)
    );
}

#[test]
fn a_port_is_left_for_the_launcher_and_slot_quits_for_it() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Ports")).unwrap();
    let script = d.path().join("Ports/Cave Story.sh");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    let mut a = open_from(d.path(), "Ports", "Cave Story");
    let queued = std::fs::read_to_string(d.path().join(".unslot-port")).unwrap();
    assert_eq!(Path::new(&queued), script);
    until(&mut a, App::ready_to_reload);
}

#[test]
fn a_ds_game_goes_in_as_any_cart_does_for_slot_to_drive_drastic() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Games/NDS")).unwrap();
    std::fs::write(d.path().join("Games/NDS/Mario 64 DS.nds"), "nds").unwrap();
    let mut a = boot(d.path());
    for _ in 0..Platform::COUNT {
        if a.shelf_platform_name() == Some("Nintendo DS") {
            break;
        }
        press(&mut a, Btn::R1);
    }
    assert_eq!(a.shelf_platform_name(), Some("Nintendo DS"));
    press(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Inserting { .. }),
        "{:?}",
        a.phase()
    );
    assert!(
        !d.path().join(".unslot-port").exists(),
        "slot quit for DraStic"
    );
}

#[test]
fn a_psp_game_goes_in_as_any_cart_does_for_slot_to_drive_ppsspp() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Games/PSP")).unwrap();
    std::fs::write(d.path().join("Games/PSP/Lumines.iso"), "psp").unwrap();
    let mut a = boot(d.path());
    for _ in 0..Platform::COUNT {
        if a.shelf_platform_name() == Some("PlayStation Portable") {
            break;
        }
        press(&mut a, Btn::R1);
    }
    assert_eq!(a.shelf_platform_name(), Some("PlayStation Portable"));
    press(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Inserting { .. }),
        "{:?}",
        a.phase()
    );
    assert!(
        !d.path().join(".unslot-port").exists(),
        "slot quit for PPSSPP"
    );
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
    assert_eq!(gallery_page(&a).tiles[0].name, "Trip");
    press(&mut a, Btn::A);
    let names: Vec<String> = gallery_page(&a).tiles.into_iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        ["a.png", "b.png", "c.jpg"],
        "the gallery showed more than pictures"
    );
    open_gallery(&mut a, "b.png");
    assert_eq!(viewer_page(&a).path, shots.join("b.png"));
    assert_eq!(viewer_page(&a).place, "2 / 3");
    assert!(viewer_page(&a).chrome);
    press(&mut a, Btn::A);
    assert!(!viewer_page(&a).chrome, "A did not hide the viewer chrome");
    press(&mut a, Btn::A);
    assert!(
        viewer_page(&a).chrome,
        "A did not show the viewer chrome again"
    );
    press(&mut a, Btn::Right);
    assert_eq!(viewer_page(&a).path, shots.join("c.jpg"));
    press(&mut a, Btn::Right);
    assert_eq!(
        viewer_page(&a).path,
        shots.join("a.png"),
        "it did not go round"
    );
    press(&mut a, Btn::B);
    assert_eq!(
        gallery_page(&a).tiles[gallery_page(&a).cursor].name,
        "b.png"
    );
    press(&mut a, Btn::B);
    assert_eq!(
        gallery_page(&a).tiles[0].name,
        "Trip",
        "B did not go up a folder"
    );
    press(&mut a, Btn::B);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
}

#[test]
fn files_manages_copy_move_delete_rename_new_folder_and_protects_system() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("System/bin")).unwrap();
    std::fs::create_dir_all(d.path().join("Notes")).unwrap();
    std::fs::create_dir_all(d.path().join("Dest")).unwrap();
    std::fs::write(d.path().join("Notes/hello.txt"), "hi there").unwrap();

    let mut a = open_app(d.path(), "Files");
    // System is protected from copy/cut/rename/delete.
    select_file(&mut a, "System");
    press(&mut a, Btn::X);
    let menu = files_page(&a).menu.unwrap();
    for label in ["Copy", "Cut", "Rename", "Delete"] {
        let row = menu.rows.iter().find(|r| r.1 == label).unwrap();
        assert!(!row.2, "{label} should be disabled on System");
    }
    press(&mut a, Btn::B);
    assert!(files_page(&a).menu.is_none());

    // Enter Notes, copy hello.txt, go up with B, enter Dest, paste.
    select_file(&mut a, "Notes");
    press(&mut a, Btn::A);
    assert_eq!(files_page(&a).crumbs, ["Card", "Notes"]);
    select_file(&mut a, "hello.txt");
    press(&mut a, Btn::X);
    pick_menu(&mut a, "Copy");
    assert_eq!(files_page(&a).status, "Copied: hello.txt");

    press(&mut a, Btn::B);
    select_file(&mut a, "Dest");
    press(&mut a, Btn::A);
    press(&mut a, Btn::X);
    pick_menu(&mut a, "Paste here");
    until(&mut a, |a| files_page(a).dialog.is_none());
    assert_eq!(
        std::fs::read_to_string(d.path().join("Dest/hello.txt")).unwrap(),
        "hi there"
    );

    // Delete the copy in Dest via the confirmation dialog.
    select_file(&mut a, "hello.txt");
    press(&mut a, Btn::X);
    pick_menu(&mut a, "Delete");
    assert!(files_page(&a).dialog.is_some());
    press(&mut a, Btn::A);
    assert!(files_page(&a).dialog.is_none());
    assert!(!d.path().join("Dest/hello.txt").exists());

    // Cut hello.txt from Notes and paste into Dest (moves it).
    press(&mut a, Btn::B);
    select_file(&mut a, "Notes");
    press(&mut a, Btn::A);
    select_file(&mut a, "hello.txt");
    press(&mut a, Btn::X);
    pick_menu(&mut a, "Cut");
    assert!(files_page(&a).rows[0].faded);
    press(&mut a, Btn::B);
    select_file(&mut a, "Dest");
    press(&mut a, Btn::A);
    press(&mut a, Btn::X);
    pick_menu(&mut a, "Paste here");
    assert!(!d.path().join("Notes/hello.txt").exists());
    assert!(d.path().join("Dest/hello.txt").exists());

    // Rename opens the keyboard; Y cancels and Start renames.
    select_file(&mut a, "hello.txt");
    press(&mut a, Btn::X);
    pick_menu(&mut a, "Rename");
    assert!(matches!(a.phase(), Phase::Keyboard { .. }));
    press(&mut a, Btn::Y);
    assert!(matches!(a.phase(), Phase::Apps));
    assert!(d.path().join("Dest/hello.txt").exists());

    press(&mut a, Btn::X);
    pick_menu(&mut a, "Rename");
    press(&mut a, Btn::A);
    press(&mut a, Btn::Start);
    assert!(matches!(a.phase(), Phase::Apps));
    assert!(!d.path().join("Dest/hello.txt").exists());
    assert!(d.path().join("Dest/hello.txtq").exists());

    // New folder creates a folder and selects it.
    press(&mut a, Btn::X);
    pick_menu(&mut a, "New folder");
    press(&mut a, Btn::A);
    press(&mut a, Btn::Start);
    assert!(matches!(a.phase(), Phase::Apps));
    assert!(d.path().join("Dest/q").is_dir());
    assert_eq!(files_page(&a).rows[files_page(&a).cursor].name, "q");
}

#[test]
fn a_book_turns_its_pages_and_opens_again_where_it_was_left() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    std::fs::create_dir_all(d.path().join("Books")).unwrap();
    let text: String = (1..=200).map(|n| format!("Line {n}\n")).collect();
    std::fs::write(d.path().join("Books/Long.txt"), text).unwrap();
    let mut a = open_app(d.path(), "Books");
    open_book(&mut a, "Long");
    let first = reader_page(&a);
    assert_eq!(first.title, "Long");
    assert_eq!(first.lines[0], "Line 1");
    assert_eq!(first.page, 0);
    assert!(first.pages > 10, "expected >10 pages, got {}", first.pages);
    press(&mut a, Btn::Right);
    let second = reader_page(&a);
    assert_eq!(second.page, 1);
    assert_eq!(second.lines[0], format!("Line {}", first.lines.len() + 1));
    press(&mut a, Btn::R1);
    assert_eq!(reader_page(&a).page, 11);
    press(&mut a, Btn::L1);
    assert_eq!(reader_page(&a).page, 1);
    press(&mut a, Btn::B);
    let lib = library_page(&a);
    assert!(
        lib.tiles[0].read.is_some(),
        "read progress was not recorded"
    );
    open_book(&mut a, "Long");
    assert_eq!(
        reader_page(&a).lines,
        second.lines,
        "the place was not kept"
    );
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
    open_book(&mut a, "tale");
    let page = reader_page(&a);
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
    assert_eq!(music_page(&a).tracks, ["01 Tone", "02 Tone"]);
    assert!(!music_page(&a).loaded);
    press(&mut a, Btn::A);
    until(&mut a, |a| {
        music_page(a).status == "Playing 01 Tone, on after B"
    });
    assert!(music_page(&a).loaded);
    assert_eq!(music_page(&a).now, Some(0));
    assert_eq!(music_page(&a).total, Some(2));

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
        music_page(&a).status.starts_with("Playing "),
        "{}",
        music_page(&a).status
    );
    press(&mut a, Btn::Y);
    until(&mut a, |a| !music_page(a).loaded);
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
    open_gallery(&mut a, "clip.mp4");
    let serial = |a: &App| match a.page_model() {
        Some(PageModel::Video(v)) => {
            assert!(v.frame.serial == 0 || v.frame.rgba.len() == (680 * 440 * 4) as usize);
            v.frame.serial
        }
        other => panic!("no video is open: {other:?}"),
    };
    until(&mut a, |a| serial(a) > 3);
    press(&mut a, Btn::A);
    match a.page_model() {
        Some(PageModel::Video(v)) => assert!(v.osd.is_some_and(|o| o.paused)),
        other => panic!("expected paused video: {other:?}"),
    }
    std::thread::sleep(Duration::from_millis(100));
    let held = serial(&a);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(serial(&a), held, "the picture moved on while paused");
    press(&mut a, Btn::B);
    assert_eq!(gallery_page(&a).title, "Gallery");
}

#[test]
fn a_video_that_plays_to_its_end_goes_back_to_the_list_and_one_that_will_not_play_says_so() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    if !clip(d.path(), "0.5") {
        return;
    }
    std::fs::write(d.path().join("Pictures/broken.mp4"), b"not a video").unwrap();
    let mut a = open_app(d.path(), "Gallery");
    open_gallery(&mut a, "clip.mp4");
    assert!(matches!(a.page_model(), Some(PageModel::Video(_))));
    until(&mut a, |a| {
        matches!(a.page_model(), Some(PageModel::App(AppPage::Gallery(_))))
    });
    assert_eq!(gallery_page(&a).title, "Gallery");
    open_gallery(&mut a, "broken.mp4");
    until(&mut a, |a| {
        matches!(a.page_model(), Some(PageModel::Text(_)))
    });
    assert_eq!(text_page(&a).lines, ["Could not play that video"]);
    press(&mut a, Btn::B);
    assert_eq!(gallery_page(&a).title, "Gallery");
}
