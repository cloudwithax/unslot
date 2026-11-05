use slot::bezel::{choices, find, info_window, load, overlays, pick, BEZELS};
use slot_gfx::overlay_rects;
use slot_store::Platform;

fn card(files: &[&str]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(BEZELS)).unwrap();
    for f in files {
        std::fs::write(d.path().join(BEZELS).join(f), b"").unwrap();
    }
    d
}

fn picked(d: &tempfile::TempDir, panel: (u32, u32)) -> Option<String> {
    pick(d.path(), panel).map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
}

#[test]
fn the_panel_takes_the_bezel_named_for_its_size() {
    let d = card(&["640x480.png", "720x720.png", "1024x768.png"]);
    assert_eq!(picked(&d, (640, 480)).as_deref(), Some("640x480.png"));
    assert_eq!(picked(&d, (720, 720)).as_deref(), Some("720x720.png"));
}

#[test]
fn a_panel_with_no_bezel_of_its_size_takes_the_nearest_of_its_shape() {
    let d = card(&["640x480.png", "1024x768.png", "720x720.png"]);
    assert_eq!(picked(&d, (1280, 960)).as_deref(), Some("1024x768.png"));
    assert_eq!(picked(&d, (720, 480)), None);
}

#[test]
fn files_that_are_not_sized_pngs_are_ignored() {
    let d = card(&["notes.txt", "sp.png", "640x480.jpg", "._640x480.png"]);
    assert_eq!(picked(&d, (640, 480)), None);
    assert_eq!(picked(&tempfile::tempdir().unwrap(), (640, 480)), None);
}

fn write_png(path: &std::path::Path, w: u32, h: u32, clear: (u32, u32, u32, u32)) {
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (cx, cy, cw, ch) = clear;
            let inside = x >= cx && x < cx + cw && y >= cy && y < cy + ch;
            rgba[((y * w + x) * 4) as usize..][..4].copy_from_slice(if inside {
                &[0, 0, 0, 0]
            } else {
                &[200, 10, 10, 255]
            });
        }
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut enc = png::Encoder::new(std::fs::File::create(path).unwrap(), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
}

#[test]
fn an_info_gives_the_games_window_from_its_borders() {
    let info = "{\n \"width\":720,\n \"height\":480,\n \"top\":16,\n \"left\":104,\n \"bottom\":16,\n \"right\":104,\n \"opacity\":1\n}";
    assert_eq!(
        info_window(info, (720, 480)),
        Some([104.0, 16.0, 512.0, 448.0])
    );
    // An info written for a picture of another size is scaled to the one it sits beside.
    assert_eq!(
        info_window(info, (1440, 960)),
        Some([208.0, 32.0, 1024.0, 896.0])
    );
    assert_eq!(info_window("{\"top\":0}", (720, 480)), None);
    assert_eq!(
        info_window(
            "{\"top\":0,\"left\":400,\"right\":400,\"bottom\":0}",
            (720, 480)
        ),
        None
    );
}

#[test]
fn a_bezel_without_an_info_shows_the_game_through_its_clear_middle() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("Bezels/NES/Plain.png");
    write_png(&path, 72, 48, (12, 2, 48, 44));
    let overlay = load(&path).unwrap();
    assert_eq!((overlay.w, overlay.h), (72, 48));
    assert_eq!(overlay.window, [12.0, 2.0, 48.0, 44.0]);

    std::fs::write(
        path.with_extension("info"),
        "{\"width\":72,\"height\":48,\"top\":4,\"left\":10,\"bottom\":4,\"right\":10}",
    )
    .unwrap();
    assert_eq!(load(&path).unwrap().window, [10.0, 4.0, 52.0, 40.0]);

    let solid = d.path().join("Bezels/NES/Solid.png");
    write_png(&solid, 8, 8, (0, 0, 0, 0));
    assert!(
        load(&solid).is_none(),
        "a bezel with no window would hide the game"
    );
}

#[test]
fn a_retroarch_overlay_cfg_is_a_bezel_with_its_picture_and_viewport() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("Bezels/NES");
    write_png(&dir.join("art/frame.png"), 72, 48, (12, 2, 48, 44));
    let cfg = dir.join("Frame.cfg");
    std::fs::write(&cfg, "overlays = 1\noverlay0_overlay = \"art/frame.png\"\n").unwrap();
    let overlay = load(&cfg).unwrap();
    assert_eq!((overlay.w, overlay.h), (72, 48));
    assert_eq!(
        overlay.window,
        [12.0, 2.0, 48.0, 44.0],
        "no viewport: the clear middle"
    );

    std::fs::write(
        &cfg,
        "overlay0_overlay = \"art/frame.png\"\ncustom_viewport_x = \"8\"\n\
         custom_viewport_y = \"0\"\ncustom_viewport_width = \"56\"\ncustom_viewport_height = \"48\"\n",
    )
    .unwrap();
    assert_eq!(load(&cfg).unwrap().window, [8.0, 0.0, 56.0, 48.0]);
    assert!(choices(d.path(), Platform::Nes, (72, 48))
        .iter()
        .any(|c| c == "Frame.cfg"));
}

#[test]
fn a_system_picks_from_its_own_bezels_and_the_shipped_ones_for_the_panel() {
    let d = tempfile::tempdir().unwrap();
    for f in [
        "System/overlays/720x480/NES/NES.png",
        "System/overlays/720x720/NES/NES.png",
        "System/overlays/720x720/NES/Famicom.png",
        "Bezels/NES/mine.png",
        "Bezels/NES/NES.png",
        "Bezels/NES/notes.txt",
    ] {
        std::fs::create_dir_all(d.path().join(f).parent().unwrap()).unwrap();
        std::fs::write(d.path().join(f), b"").unwrap();
    }
    assert_eq!(
        choices(d.path(), Platform::Nes, (720, 720)),
        ["Famicom.png", "mine.png", "NES.png"]
    );
    assert_eq!(
        choices(d.path(), Platform::Nes, (720, 480)),
        ["mine.png", "NES.png"]
    );
    assert!(choices(d.path(), Platform::Snes, (720, 480)).is_empty());
    // The card's own wins over the shipped one of its name.
    assert_eq!(
        find(d.path(), Platform::Nes, "NES.png", (720, 720)).unwrap(),
        d.path().join("Bezels/NES/NES.png")
    );
    assert_eq!(
        find(d.path(), Platform::Nes, "Famicom.png", (720, 720)).unwrap(),
        d.path().join("System/overlays/720x720/NES/Famicom.png")
    );
    // A panel of no shipped size takes the nearest of its shape, else the 720 by 480 ones.
    assert_eq!(
        overlays(d.path(), (480, 480)).unwrap(),
        d.path().join("System/overlays/720x720")
    );
    assert_eq!(
        overlays(d.path(), (1280, 720)).unwrap(),
        d.path().join("System/overlays/720x480")
    );
}

#[test]
fn an_overlay_fits_the_panel_and_its_window_scales_with_it() {
    // The 720 by 720 panel shows a 720 by 720 overlay one to one; GL counts up from the bottom.
    let (whole, game) = overlay_rects((720, 720), (720, 720), [60.0, 80.0, 600.0, 540.0]);
    assert_eq!(whole, (0, 0, 720, 720));
    assert_eq!(game, (60, 100, 600, 540));
    // A 720 by 480 overlay on a 1440 by 1080 window is doubled and centred top to bottom.
    let (whole, game) = overlay_rects((1440, 1080), (720, 480), [104.0, 16.0, 512.0, 448.0]);
    assert_eq!(whole, (0, 60, 1440, 960));
    assert_eq!(game, (208, 92, 1024, 896));
}
