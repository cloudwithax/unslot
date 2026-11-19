#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::sync::{Mutex, MutexGuard, PoisonError};

use slot_gfx::{Canvas, Compositor, HeadlessSurface, Layout};
use slot_store::Platform;
use slot_ui::{
    beam_rgba, draw_trophy_carousel, draw_trophy_ground, draw_trophy_strip, spotlight_rgba,
    trophy_assets, trophy_badge_over, trophy_strip_row, Draw, Shelf, TrophyBadge, TrophyTextures,
    TrophyTier,
};

static GL: Mutex<()> = Mutex::new(());

fn compositor() -> Option<(MutexGuard<'static, ()>, HeadlessSurface, Compositor)> {
    let guard = GL.lock().unwrap_or_else(PoisonError::into_inner);
    let surface = HeadlessSurface::new().ok()?;
    let compositor = Compositor::new(&surface).ok()?;
    Some((guard, surface, compositor))
}

fn write_png(px: &[u8], w: u32, h: u32, name: &str) {
    let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") else {
        return;
    };
    let path = format!("{dir}/{name}.png");
    let file = std::fs::File::create(&path).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .expect("png header")
        .write_image_data(px)
        .expect("png data");
}

fn bright_pixels_in(
    px: &[u8],
    (w, h): (u32, u32),
    [x0, y0, x1, y1]: [u32; 4],
    threshold: u8,
) -> usize {
    let mut count = 0;
    for y in y0.min(h)..y1.min(h) {
        for x in x0.min(w)..x1.min(w) {
            let o = ((y * w + x) * 4) as usize;
            if px[o].max(px[o + 1]).max(px[o + 2]) >= threshold {
                count += 1;
            }
        }
    }
    count
}

fn cart(stem: &str) -> slot_store::Cart {
    slot_store::Cart {
        platform: Platform::Gba,
        stem: stem.into(),
        rom: std::path::PathBuf::from(format!("/roms/{stem}.gba")),
        label: None,
        title: stem.into(),
        code: String::new(),
        shell: None,
    }
}

fn textures(c: &mut Compositor) -> TrophyTextures {
    let assets = trophy_assets().expect("embedded trophy assets decode");
    let tiers = [0, 1, 2, 3, 4].map(|i| {
        let (rgba, w, h) = &assets.tiers[i];
        c.create_texture(*w, *h, rgba)
    });
    let (glow, gw, gh) = spotlight_rgba();
    let (shaft, sw, sh) = beam_rgba();
    TrophyTextures {
        tiers,
        spot: c.create_texture(gw, gh, &glow),
        beam: c.create_texture(sw, sh, &shaft),
    }
}

#[test]
fn the_trophy_carousel_stands_the_cards_carts_in_the_room() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let tex = textures(&mut c);
    // A flat, bright wall, so that what the carousel does to it is what is measured.
    let wall = c.create_texture(1, 1, &[200, 200, 200, 255]);

    // Three carts of one colour each, so the middle one is easy to find.
    let mut shelf = Shelf::new(vec![cart("Aria"), cart("Fusion"), cart("Minish")]);
    let face = |c: &mut Compositor, rgb: [u8; 4]| {
        let mut rgba = Vec::new();
        for _ in 0..(24 * 32) {
            rgba.extend_from_slice(&rgb);
        }
        c.create_texture(24, 32, &rgba)
    };
    shelf.set_face(0, face(&mut c, [0x80, 0x20, 0x40, 0xff]));
    shelf.set_face(1, face(&mut c, [0x20, 0x90, 0x40, 0xff]));
    shelf.set_face(2, face(&mut c, [0x30, 0x50, 0xc0, 0xff]));
    shelf.select(1);

    let panel = (720u32, 480u32);
    let canvas = Canvas::of(Layout::Native, panel);
    let draw = |c: &mut Compositor, shelf: &Shelf, sink: f32| {
        let mut draws: Vec<Draw> = Vec::new();
        let focus = shelf
            .row(canvas, None, 0.0, 0.0)
            .into_iter()
            .find(|p| p.index == shelf.index)
            .map(|p| (p.x, p.y, p.w, p.h));
        draw_trophy_ground(canvas, Some(wall), &tex, focus, 1.0, &mut draws);
        shelf.draw_row_at(canvas, None, 0.0, 0.0, 1.0, sink, &mut draws);
        // The slot's front, which the carts go down behind.
        slot_ui::draw_empty_slot(canvas, None, &mut draws);
        let badges: Vec<TrophyBadge> = shelf
            .row(canvas, None, 0.0, 0.0)
            .into_iter()
            .map(|p| {
                trophy_badge_over(
                    (p.x, p.y, p.w, p.h),
                    1.0,
                    TrophyTier::Gold,
                    p.index == shelf.index,
                    p.alpha,
                )
            })
            .collect();
        draw_trophy_carousel(&tex, 1.0, &badges, &mut draws);
        c.fit(panel);
        c.begin_frame(Layout::Native);
        c.draw_list(&draws);
        c.read_window(panel)
    };

    let px = draw(&mut c, &shelf, 0.0);
    write_png(&px, panel.0, panel.1, "trophy-carousel-720x480");
    let lit = bright_pixels_in(&px, panel, [0, 0, panel.0, panel.1], 45);
    assert!(
        lit > (panel.0 * panel.1 / 4) as usize,
        "the carousel is too dark: {lit} lit pixels"
    );
    // The middle cart, in its own colour, in the middle of the screen.
    let mid = bright_pixels_in(&px, panel, [260, 150, 460, 380], 100);
    assert!(mid > 6000, "the middle cart is not there: {mid}");
    let green = px
        .chunks(4)
        .filter(|p| {
            p[1] > 120 && (p[1] as i32) > p[0] as i32 + 40 && (p[1] as i32) > p[2] as i32 + 40
        })
        .count();
    assert!(
        green > 4000,
        "the middle cart is not the green one: {green}"
    );
    // The wall is dimmed, and a stage light from above leaves it lighter down the middle, behind
    // the cart in hand, than at the sides.
    let at = |x: u32, y: u32| {
        let i = ((y * panel.0 + x) * 4) as usize;
        px[i] as i32 + px[i + 1] as i32 + px[i + 2] as i32
    };
    assert!(at(8, 8) < 3 * 120, "the wall was not dimmed: {}", at(8, 8));
    assert!(
        at(360, 40) > at(8, 40) + 40,
        "no light from above on the cart in hand: {} against {}",
        at(360, 40),
        at(8, 40)
    );

    // On its way down, the same cart is behind the slot's front and cannot be seen.
    let sunk = draw(&mut c, &shelf, 340.0);
    write_png(&sunk, panel.0, panel.1, "trophy-carousel-sunk-720x480");
    let gone = sunk
        .chunks(4)
        .filter(|p| {
            p[1] > 120 && (p[1] as i32) > p[0] as i32 + 40 && (p[1] as i32) > p[2] as i32 + 40
        })
        .count();
    assert!(
        gone < 200,
        "the cart did not go down behind the front: {gone}"
    );
}

#[test]
fn the_trophy_in_hand_is_the_biggest_and_the_ones_by_it_are_evenly_either_side() {
    let canvas = Canvas::of(Layout::Native, (720, 480));
    let row = trophy_strip_row(canvas, 8, 3.0, 1.0);
    let mid = row.iter().find(|t| t.index == 3).expect("in hand");
    assert!(row.iter().all(|t| t.index == 3 || t.h < mid.h));
    let (left, right) = (
        row.iter().find(|t| t.index == 2).unwrap(),
        row.iter().find(|t| t.index == 4).unwrap(),
    );
    let centre = |t: &slot_ui::StripTrophy| t.x + t.w / 2.0;
    assert!((canvas.w / 2.0 - centre(left) - (centre(right) - canvas.w / 2.0)).abs() < 0.5);
    assert!((centre(mid) - canvas.w / 2.0).abs() < 0.5);
    // They all stand on one line.
    assert!(row
        .iter()
        .all(|t| (t.y + t.h - (mid.y + mid.h)).abs() < 0.5));
    // Between two, the one it is moving to is nearly in hand.
    let between = trophy_strip_row(canvas, 8, 3.9, 1.0);
    let next = between.iter().find(|t| t.index == 4).unwrap();
    assert!(next.h > mid.h * 0.95, "{} of {}", next.h, mid.h);
    // Before it has come in, there is nothing to see; the first of them rises into place.
    assert!(trophy_strip_row(canvas, 8, 3.0, 0.0).is_empty());
    let half = trophy_strip_row(canvas, 8, 3.0, 0.5);
    let rising = half.iter().find(|t| t.index == 3).unwrap();
    assert!(rising.y > mid.y && rising.alpha < 1.0);
    // And a carousel with no trophies has nothing to stand.
    assert!(trophy_strip_row(canvas, 0, 0.0, 1.0).is_empty());
}

#[test]
fn a_stage_light_falls_on_the_trophy_in_hand_from_above() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let tex = textures(&mut c);
    let wall = c.create_texture(1, 1, &[200, 200, 200, 255]);
    let panel = (720u32, 480u32);
    let canvas = Canvas::of(Layout::Native, panel);
    let tiers = [
        TrophyTier::Locked,
        TrophyTier::Bronze,
        TrophyTier::Gold,
        TrophyTier::Platinum,
        TrophyTier::Silver,
    ];
    let row = trophy_strip_row(canvas, tiers.len(), 2.0, 1.0);
    let focus = row
        .iter()
        .find(|t| t.index == 2)
        .map(|t| (t.x, t.y, t.w, t.h));
    let mut draws: Vec<Draw> = Vec::new();
    draw_trophy_ground(canvas, Some(wall), &tex, focus, 1.0, &mut draws);
    draw_trophy_strip(&tex, &tiers, &row, &mut draws);
    c.fit(panel);
    c.begin_frame(Layout::Native);
    c.draw_list(&draws);
    let px = c.read_window(panel);
    write_png(&px, panel.0, panel.1, "trophy-strip-720x480");
    let at = |x: u32, y: u32| {
        let i = ((y * panel.0 + x) * 4) as usize;
        px[i] as i32 + px[i + 1] as i32 + px[i + 2] as i32
    };
    // The light is a shaft from the top of the screen, not a halo: lit above the trophy, and
    // dark away from it.
    assert!(
        at(360, 40) > at(40, 40) + 40,
        "no shaft down the middle: {} against {}",
        at(360, 40),
        at(40, 40)
    );
    // It widens as it comes down, and pools on the floor under the trophy.
    let floor = (focus.unwrap().1 + focus.unwrap().3) as u32;
    assert!(
        at(360 + 130, floor - 4) > at(40, floor - 4) + 30,
        "no pool on the floor"
    );
    // The trophy in hand is gold, and in front of the light.
    let gold = px
        .chunks(4)
        .filter(|p| p[0] > 150 && p[1] > 110 && p[2] < 90 && p[0] as i32 > p[2] as i32 + 70)
        .count();
    assert!(gold > 800, "the gold trophy is not there: {gold}");
}
