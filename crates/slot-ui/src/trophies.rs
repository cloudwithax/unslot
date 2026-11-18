//! The Trophy Room's pictures: the trophies each game's cart wears, a stage's spotlight over the
//! one in hand, and the carousel of a game's trophies.

use std::sync::LazyLock;

use slot_gfx::{Canvas, Draw, TexId};

use crate::art::decode_png;

const PLATINUM_PNG: &[u8] = include_bytes!("../../../assets/trophy_platinum.png");
const GOLD_PNG: &[u8] = include_bytes!("../../../assets/trophy_gold.png");
const SILVER_PNG: &[u8] = include_bytes!("../../../assets/trophy_silver.png");
const BRONZE_PNG: &[u8] = include_bytes!("../../../assets/trophy_bronze.png");
const LOCKED_PNG: &[u8] = include_bytes!("../../../assets/trophy_locked.png");

const TROPHY_W: f32 = 112.0;
const TROPHY_H: f32 = 144.0;

/// Visual trophy tier.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum TrophyTier {
    Platinum,
    Gold,
    Silver,
    Bronze,
    #[default]
    Locked,
}

impl TrophyTier {
    pub const ALL: [TrophyTier; 5] = [
        TrophyTier::Platinum,
        TrophyTier::Gold,
        TrophyTier::Silver,
        TrophyTier::Bronze,
        TrophyTier::Locked,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// GPU texture IDs the Trophy Room draws with.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TrophyTextures {
    pub tiers: [TexId; 5],
    /// A soft round glow, white over transparent: the pool of light on the floor.
    pub spot: TexId,
    /// A shaft of light, narrow at the top and wide at the bottom, white over transparent.
    pub beam: TexId,
}

/// Decoded RGBA bitmaps of the trophies.
pub struct TrophyAssets {
    pub tiers: [(Vec<u8>, u32, u32); 5],
}

static ASSETS: LazyLock<Option<TrophyAssets>> = LazyLock::new(|| {
    let tiers = [
        decode_png(PLATINUM_PNG)?,
        decode_png(GOLD_PNG)?,
        decode_png(SILVER_PNG)?,
        decode_png(BRONZE_PNG)?,
        decode_png(LOCKED_PNG)?,
    ];
    Some(TrophyAssets { tiers })
});

/// The embedded trophy PNG assets, decoded on first use.
pub fn trophy_assets() -> Option<&'static TrophyAssets> {
    ASSETS.as_ref()
}

/// The glow the pool of light is cut from: white, clear at the edge, and fading the way light
/// falls off, not in a hard disc.
pub fn spotlight_rgba() -> (Vec<u8>, u32, u32) {
    const N: u32 = 128;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (
                (x as f32 + 0.5) / N as f32 * 2.0 - 1.0,
                (y as f32 + 0.5) / N as f32 * 2.0 - 1.0,
            );
            let r = (dx * dx + dy * dy).sqrt().min(1.0);
            let k = (1.0 - r).powf(1.6);
            rgba.extend_from_slice(&[255, 250, 240, (k * 255.0).round() as u8]);
        }
    }
    (rgba, N, N)
}

/// The shaft of a stage light: a point at the top that opens to the width of the pool it lands
/// in, softest at its edges, and thinner in the air the farther it has come.
pub fn beam_rgba() -> (Vec<u8>, u32, u32) {
    const W: u32 = 96;
    const H: u32 = 256;
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        let v = (y as f32 + 0.5) / H as f32;
        let half = 0.12 + 0.88 * v;
        let along = 1.0 - 0.55 * v;
        for x in 0..W {
            let u = ((x as f32 + 0.5) / W as f32 * 2.0 - 1.0).abs();
            let across = (1.0 - u / half).clamp(0.0, 1.0);
            let k = across.powf(1.3) * along;
            rgba.extend_from_slice(&[255, 250, 240, (k * 255.0).round() as u8]);
        }
    }
    (rgba, W, H)
}

const GROUND_DIM: f32 = 0.62;
const BEAM_ALPHA: f32 = 0.34;
const POOL_ALPHA: f32 = 0.5;

/// Draws the shelf's own background, dimmed, `brass` of the way in, with a stage light from above
/// on `focus`, the box of what is in hand: its shaft, and the pool it makes on the floor the
/// carts stand on.
pub fn draw_trophy_ground(
    canvas: Canvas,
    ground: Option<TexId>,
    tex: &TrophyTextures,
    focus: Option<(f32, f32, f32, f32)>,
    brass: f32,
    out: &mut Vec<Draw>,
) {
    let brass = brass.clamp(0.0, 1.0);
    if brass <= 0.0 {
        return;
    }
    if let Some(ground) = ground {
        out.push(crate::backdrop::ground_at(canvas, ground, brass));
    }
    out.push(Draw::Rect {
        x: 0.0,
        y: 0.0,
        w: canvas.w,
        h: canvas.h,
        colour: [0.0, 0.0, 0.0, GROUND_DIM * brass],
    });
    if let Some((x, y, w, h)) = focus {
        let (cx, floor) = (x + w * 0.5, y + h);
        let spread = (w * 1.9).max(260.0);
        out.push(Draw::Tex {
            x: cx - spread * 0.5,
            y: 0.0,
            w: spread,
            h: floor,
            tex: tex.beam,
            alpha: BEAM_ALPHA * brass,
        });
        let pool = (spread * 1.15, 54.0);
        out.push(Draw::Tex {
            x: cx - pool.0 * 0.5,
            y: floor - pool.1 * 0.55,
            w: pool.0,
            h: pool.1,
            tex: tex.spot,
            alpha: POOL_ALPHA * brass,
        });
    }
}

const BADGE_W: f32 = 60.0;
const BADGE_H: f32 = BADGE_W * TROPHY_H / TROPHY_W;
const BADGE_LIFT: f32 = 6.0;

/// A trophy standing over one cart of the carousel.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct TrophyBadge {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub tier: TrophyTier,
    pub selected: bool,
    pub alpha: f32,
}

/// The badge standing over a cart whose box is `(x, y, w, h)`: above the cart, or on its top
/// where the screen's edge leaves no room.
pub fn trophy_badge_over(
    (x, y, w, _h): (f32, f32, f32, f32),
    scale: f32,
    tier: TrophyTier,
    selected: bool,
    alpha: f32,
) -> TrophyBadge {
    let k = scale * if selected { 1.0 } else { 0.86 };
    let (bw, bh) = ((BADGE_W * k).round(), (BADGE_H * k).round());
    let above = (y - bh - BADGE_LIFT * k).round();
    let by = above.max(BADGE_LIFT).min(y);
    let bx = (x + w * 0.5 - bw * 0.5).round();
    TrophyBadge {
        x: bx,
        y: by,
        w: bw,
        h: bh,
        tier,
        selected,
        alpha: alpha.clamp(0.0, 1.0),
    }
}

/// Draws a trophy over each cart of the carousel, `brass` of the way in.
pub fn draw_trophy_carousel(
    tex: &TrophyTextures,
    brass: f32,
    badges: &[TrophyBadge],
    out: &mut Vec<Draw>,
) {
    let brass = brass.clamp(0.0, 1.0);
    if brass <= 0.0 {
        return;
    }
    for badge in badges {
        if badge.alpha <= 0.0 {
            continue;
        }
        let (bw, bh) = (badge.w * brass, badge.h * brass);
        if bw <= 0.0 || bh <= 0.0 {
            continue;
        }
        out.push(Draw::Tex {
            x: badge.x + (badge.w - bw) * 0.5,
            y: badge.y + (badge.h - bh),
            w: bw,
            h: bh,
            tex: tex.tiers[badge.tier.index()],
            alpha: badge.alpha * brass,
        });
    }
}

/// How tall the trophy in hand stands, against the 144 pixels it was drawn at.
const STRIP_H: f32 = 224.0;
/// What the trophies beside it are scaled to, and how far apart their centres are.
const SIDE_SCALE: f32 = 0.56;
const SPACING: f32 = 196.0;
/// How far below its place a trophy starts, and how far it rises, as the carousel is entered.
const ENTER_DROP: f32 = 70.0;

/// One trophy of the trophy carousel, placed.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct StripTrophy {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub alpha: f32,
}

/// Where the carousel of `count` trophies stands each of them with the carousel `scroll` trophies
/// along and `enter` of the way in. They stand on the line the shelf's carts do, above its labels.
pub fn trophy_strip_row(canvas: Canvas, count: usize, scroll: f32, enter: f32) -> Vec<StripTrophy> {
    let enter = enter.clamp(0.0, 1.0);
    let floor = crate::slot_chrome::slot_name_y() - 2.0 * crate::plate::HINT_H as f32
        + canvas.extra() / 2.0
        - 14.0;
    let first = (scroll - 3.0).floor().max(0.0) as usize;
    let last = ((scroll + 3.0).ceil().max(0.0) as usize).min(count.saturating_sub(1));
    let mut out = Vec::new();
    if count == 0 {
        return out;
    }
    for index in first..=last {
        let d = index as f32 - scroll;
        let near = d.abs().min(1.0);
        let scale = 1.0 - (1.0 - SIDE_SCALE) * near - 0.07 * (d.abs() - 1.0).clamp(0.0, 2.0);
        let h = STRIP_H * scale;
        let w = h * TROPHY_W / TROPHY_H;
        let x = canvas.w * 0.5 + d * SPACING - w * 0.5;
        let alpha = (1.0 - 0.3 * near - 0.3 * (d.abs() - 1.0).clamp(0.0, 1.5)) * enter;
        if x + w <= 0.0 || x >= canvas.w || alpha <= 0.0 {
            continue;
        }
        out.push(StripTrophy {
            index,
            x,
            y: floor - h + (1.0 - enter) * ENTER_DROP,
            w,
            h,
            alpha,
        });
    }
    out
}

/// Draws the placed trophies of the carousel, each in its tier's picture.
pub fn draw_trophy_strip(
    tex: &TrophyTextures,
    tiers: &[TrophyTier],
    row: &[StripTrophy],
    out: &mut Vec<Draw>,
) {
    for t in row {
        let Some(tier) = tiers.get(t.index) else {
            continue;
        };
        out.push(Draw::Tex {
            x: t.x,
            y: t.y,
            w: t.w,
            h: t.h,
            tex: tex.tiers[tier.index()],
            alpha: t.alpha,
        });
    }
}
