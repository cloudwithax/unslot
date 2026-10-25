use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slot_gfx::{Canvas, Layout, OUT_H, OUT_W};
use slot_input::{Action, Btn, MENU_DOUBLE_TAP_MS, MUTE_CHORD_MS};
use slot_power::{Battery, Charge, LedState, LidPolicy, Power};
use slot_retro::{CoreOption, LinkChannel};
use slot_store::{
    format_stamp, parse_stamp, read_slot_state, scan, write_slot_state, BezelPick, Cart, Core,
    GbPalette, Platform, Recent, Shader, SlotState, StateEntry, StateRing, WallpaperPick,
    WallpaperTint, BLUE_LIGHT_MAX, BRIGHTNESS_MAX, FF_SPEEDS, RING_MAX, RUN_AHEAD_MAX, VOLUME_MAX,
};
use slot_ui::{
    board_from, board_zoom, centred_hints, draw_empty_slot, draw_footer, draw_ground,
    draw_slot_name, draw_sticker, ease, footer_ink, grown, lid_at, lid_from, lift_of, on_board,
    repeat_gap, shelf_cart_at, ButtonLegend, ClockPicker, Draw, FfState, Hud, HudKind, Icon,
    KeyInput, KeyOutcome, Keyboard, Legend, LinkBadge, ListIcon, ListPage, ListRow, Media, Millis,
    Mould, Placed, Polaroids, PowerChoice, QrPage, QuickMenu, QuickMenuFaces, QuickRow, QuickValue,
    Refusal, Shelf, SlotChrome, TexId, Toast, CART_W, CHIP_H, CHIP_U, CHIP_V, CHIP_W, HINT_EDGE,
    HOP_LIFT, MOUTH_H, REPEAT_DELAY_MS, SHADOW_H, SHADOW_W, SOCKET_H, SOCKET_U, SOCKET_V, SOCKET_W,
    TURN_PAD,
};

use crate::audio::Sfx;
use crate::bluetooth::{Bluetooth, BtBusy, BtView};
use crate::core_downloads::{CoreDownloads, CoresView};
use crate::core_picker::{Chip, CorePicker, Outcome, Press};
use crate::link_kind::{link_carried, link_kind, serial_option, LinkKind};
use crate::link_radio::{radio_jobs, LinkRole, RadioJob, RadioJobs};
use crate::link_screen::LinkSprites;
use crate::link_start::{link_port, LinkFail, LinkProgress, LinkStarter, LinkStep};
use crate::persist::{self, Snapshot};
use crate::scrape::{ScrapeView, Scraper, SsState};
use crate::services::{Services, ServicesView, SyncProgress, Tailnet, Wanted};
use crate::update::{Part, Stage, Track, Updates, UpdatesView};
use crate::video_mode::{self, VideoMode};
use crate::wifi::{Busy, Network, Security, Wifi, WifiView};

pub const INSERT_S: f32 = 0.73;
/// How long an achievement's line stays up, and how long it takes to fade in and out.
const CHEEVO_MS: Millis = 4000;
const CHEEVO_FADE_MS: Millis = 250;
const INSERT_HOLD_S: f32 = 0.28;

pub const SEATED_AT: f32 = INSERT_S - INSERT_HOLD_S;

pub const EJECT_S: f32 = SEATED_AT;

/// A disc spins up once it seats and the screen goes black before its core starts, so the
/// heavier systems' cores do not load over the animation.
const SPIN_S: f32 = 0.9;
const SPIN_UP_S: f32 = 0.5;
const SPIN_RAD_S: f32 = 4.0 * std::f32::consts::PI;
const DARK_S: f32 = 0.35;

const EJECT_HOLD_S: f32 = 0.35;

pub const IN_GAME_MENU: Layout = Layout::Native;

const POWER_ON_S: f32 = 0.22;

const POWER_OFF_S: f32 = 0.16;

const FOOTER_SLIDE_S: f32 = POWER_ON_S;

const VOLUME_STEP: u8 = 5;

const AUTOSAVE_MS: Millis = 300_000;
const CARD_CHECK_MS: Millis = 2_000;

const BATTERY_CRITICAL: u8 = 5;

const BATTERY_POLL_MS: Millis = 10_000;
const MENU_REVEAL_MS: Millis = 180;

const CHARGE_POLL_MS: Millis = 1_000;
const HEADPHONES_POLL_MS: Millis = 500;

const BATTERY_LOW: u8 = 20;

pub const UNDO_GRACE_MS: Millis = 30_000;

pub const LINK_LOST_MS: Millis = 2000;

const PLAY_HOLD_MS: Millis = 500;

const SHUTDOWN_SHOW_MS: Millis = 250;

const CORE_PICKER_RECEDE: f32 = 0.26;

const SLOT_NAME_IN_MS: Millis = 200;
const SLOT_NAME_HOLD_MS: Millis = 1200;
const SLOT_NAME_OUT_MS: Millis = 800;
const SLOT_NAME_ALPHA: f32 = 1.0;
const CORE_PICKER_DIM: f32 = 0.25 / (1.0 - CORE_PICKER_RECEDE);
const CORE_LEGEND_Y: f32 = OUT_H as f32 - MOUTH_H * 2.0;
const CORE_LEGEND_GAP: f32 = 24.0;
const LINK_TEXT_Y: f32 = 44.0;
const LINK_LEGEND_Y: f32 = OUT_H as f32 - MOUTH_H;
pub const LINK_SCRIM: [f32; 4] = [0.0, 0.0, 0.0, 0.5];
const LINK_LEGEND_GAP: f32 = 40.0;
const LINK_LEGEND_EDGE: f32 = 0.0;
const LID_SHADOW_W: f32 = 168.0;
const LID_SHADOW_H: f32 = 18.0;
const LID_SHADOW_DROP: f32 = 27.1;
const LID_SHADOW_ALPHA: f32 = 0.8;
const FACES_WAIT_MS: Millis = 1500;

const ALERT_HOLD: f32 = 0.45;
const ALERT_GONE: f32 = 0.9;

const CLOCK_FLOOR: i64 = 1_577_836_800;

pub enum PendingUndo {
    Save {
        stamp: String,
        evicted: Option<(String, Vec<u8>, Vec<u8>)>,
    },
    Load {
        prior: Vec<u8>,
    },
}

struct LinkSession {
    client_id: u16,
    lost_at: Option<Millis>,
}

struct LinkStarting {
    starter: LinkStarter,
    client_id: u16,
}

struct Reload {
    stem: String,
    role: LinkRow,
    cancelled: bool,
    from: LinkKind,
    from_serial: &'static str,
    fallback: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GameMenu {
    Pick(LinkRow),
    Working {
        role: LinkRow,
        step: LinkStep,
        since: Millis,
    },
    Linked {
        role: LinkRow,
        worked: Millis,
        since: Millis,
        opened: bool,
    },
    Failed {
        role: LinkRow,
        fail: LinkFail,
        worked: Millis,
        since: Millis,
    },
    Unplug {
        role: LinkRow,
        since: Millis,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkRow {
    Host,
    Join,
}

impl LinkRow {
    pub const ALL: [LinkRow; 2] = [LinkRow::Host, LinkRow::Join];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn text(self) -> &'static str {
        match self {
            LinkRow::Host => "Host",
            LinkRow::Join => "Join",
        }
    }

    pub fn role(self) -> LinkRole {
        match self {
            LinkRow::Host => LinkRole::Host,
            LinkRow::Join => LinkRole::Join,
        }
    }

    pub fn client_id(self) -> u16 {
        match self {
            LinkRow::Host => 0,
            LinkRow::Join => 1,
        }
    }

    pub fn other(self) -> LinkRow {
        match self {
            LinkRow::Host => LinkRow::Join,
            LinkRow::Join => LinkRow::Host,
        }
    }

    pub fn from_client_id(id: u16) -> LinkRow {
        if id == 0 {
            LinkRow::Host
        } else {
            LinkRow::Join
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkLegend {
    Cancel,
    Mode,
    Swap,
    Link,
    Ok,
    Back,
    EndLink,
}

impl LinkLegend {
    pub const ALL: [LinkLegend; 7] = [
        LinkLegend::Cancel,
        LinkLegend::Mode,
        LinkLegend::Swap,
        LinkLegend::Link,
        LinkLegend::Ok,
        LinkLegend::Back,
        LinkLegend::EndLink,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

pub const LINKED_HOLD_MS: Millis = 1000;

pub const UNPLUG_HOLD_MS: Millis = 420;

#[derive(Debug)]
pub enum Phase {
    SetClock {
        picker: ClockPicker,
        seed: i64,
        from_menu: bool,
    },
    Shelf,
    QuickMenu {
        row: QuickRow,
    },
    Inserting {
        cart: String,
        t: f32,
        core_ready: bool,
        resumed: bool,
        clean: bool,
    },
    Playing {
        cart: String,
    },
    Ejecting {
        cart: String,
        t: f32,
    },
    Polaroids {
        cart: String,
    },
    About,
    Doze {
        cart: Option<String>,
    },
    /// The Wi-Fi networks in range. `note` says why the last pick went nowhere.
    Networks {
        cursor: usize,
        note: Option<String>,
    },
    /// The Bluetooth devices paired or nearby.
    Devices {
        cursor: usize,
    },
    /// SSH, file sharing, Syncthing, Tailscale and the VPN. `note` says why the last change went
    /// nowhere.
    Services {
        cursor: usize,
        note: Option<String>,
    },
    /// The code to scan to sign the device in to Tailscale.
    TailscaleLogin,
    /// Syncthing's device ID, as a code for its phone app to scan.
    SyncthingId,
    /// unslot's and BaseOS's releases, and whether this device is behind them.
    Updates {
        cursor: usize,
    },
    /// The cores that do not come with unslot, to download, update or remove.
    CoreDownloads {
        cursor: usize,
    },
    /// Finding labels online, for all the card's systems or one. `note` says why A did nothing.
    GameArt {
        cursor: usize,
        note: Option<String>,
    },
    /// The cores that run the game on the shelf, for a system other than the GBA, whose cores
    /// are picked on its board instead.
    Cores {
        cursor: usize,
    },
    /// The games a search found, from X on the shelf.
    Search {
        cursor: usize,
    },
    /// Typing for the network or services page, which it goes back to at `cursor`.
    Keyboard {
        keys: Keyboard,
        typing: Typing,
        cursor: usize,
    },
}

/// What the keyboard is being typed for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Typing {
    Password(String),
    /// The name of a network that does not announce itself.
    HiddenName,
    HiddenPassword(String),
    /// Root's password, for SSH, file sharing and Syncthing, the row `then` turned on after.
    RootPassword {
        then: Option<ServiceRow>,
    },
    /// The player's ScreenScraper username, then their password.
    SsUser,
    SsPassword(String),
    /// The player's RetroAchievements username, then their password.
    RaUser,
    RaPassword(String),
    /// A game's name to look for, from the shelf or, `again`, from the last search's results.
    Search {
        again: bool,
    },
}

/// A row on the services page.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ServiceRow {
    Ssh,
    Sharing,
    Syncthing,
    Tailscale,
    Vpn,
    Password,
}

impl ServiceRow {
    const ALL: [ServiceRow; 6] = [
        ServiceRow::Ssh,
        ServiceRow::Sharing,
        ServiceRow::Syncthing,
        ServiceRow::Tailscale,
        ServiceRow::Vpn,
        ServiceRow::Password,
    ];
}

/// A page slot draws whole, for the frontend to turn into a picture when it changes.
#[derive(Clone, PartialEq, Debug)]
pub enum PageModel {
    List(ListPage),
    Keys(Keyboard),
    Qr(QrPage),
}

/// A row on the network page.
enum NetRow {
    Network(Network),
    Other,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Rack {
    Recents,
    Favorites,
    Of(Platform),
}

impl Rack {
    fn platform(self) -> Option<Platform> {
        match self {
            Rack::Of(p) => Some(p),
            Rack::Recents | Rack::Favorites => None,
        }
    }

    fn key(self) -> String {
        match self {
            Rack::Recents => "recents".into(),
            Rack::Favorites => "favorites".into(),
            Rack::Of(p) => p.dir_name().to_ascii_lowercase(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Rack::Recents => "Recents",
            Rack::Favorites => "Favorites",
            Rack::Of(p) => p.name(),
        }
    }
}

const RECENTS_AT: usize = 0;
const FAVORITES_AT: usize = 1;

pub struct App {
    phase: Phase,
    shelves: Vec<(Rack, Shelf)>,
    shelf_at: usize,
    orphaned: Vec<TexId>,
    play_held: Option<Millis>,
    held_dir: Option<(Btn, Millis, usize)>,
    refusal: Option<Refusal>,
    refused_from: Option<f32>,
    alert_face: Option<TexId>,
    shutdown_faces: Vec<(TexId, u32, u32)>,
    core_picker: Option<CorePicker>,
    core_board_face: Option<TexId>,
    core_interior_face: Option<TexId>,
    core_lid_face: Option<TexId>,
    core_faces_stem: Option<String>,
    core_socket_faces: Vec<TexId>,
    core_chip_faces: Vec<TexId>,
    core_blank_chip_face: Option<TexId>,
    core_chip_shadow_face: Option<TexId>,
    core_legend_faces: Vec<(TexId, u32)>,
    game_menu: Option<GameMenu>,
    link_sprites: Option<LinkSprites>,
    link_shot: Option<TexId>,
    link_hardware: LinkKind,
    last_role: LinkRow,
    link_choices: HashMap<String, LinkKind>,
    link_loaded: Option<&'static str>,
    link_reload: Option<(String, &'static str)>,
    reload: Option<Reload>,
    link_menu_faces: Vec<(TexId, u32, u32)>,
    link_linked_face: Option<(TexId, u32, u32)>,
    link_step_faces: Vec<(TexId, u32, u32)>,
    link_fail_faces: Vec<(TexId, u32, u32)>,
    link_legend_faces: Vec<(TexId, u32)>,
    starting: Option<LinkStarting>,
    link_transport: Option<(u16, Box<dyn LinkChannel>)>,
    restarting: bool,
    reloading: bool,
    /// Whether the picture is on a TV through HDMI rather than on the panel.
    external_display: bool,
    act_at: Millis,
    root: Option<PathBuf>,
    state: SlotState,
    vol_before: Vec<(u8, bool, Millis)>,
    snapshot: Option<Box<dyn Snapshot>>,
    core: Core,
    colour_pending: Option<bool>,
    palette_live: bool,
    palette_pending: Option<GbPalette>,
    /// The running game's discs and which is in the drive.
    disks: (u32, u32),
    disc_pending: Option<u32>,
    link_player: Option<u8>,
    platform: Platform,
    named_core: bool,
    video_mode: VideoMode,
    link: Option<LinkSession>,
    sfx: Option<Sfx>,
    buzz: Option<Buzz>,
    polaroids: Option<Polaroids>,
    pending: Option<(PendingUndo, Millis)>,
    legend_faces: Vec<TexId>,
    undo_face: Option<TexId>,
    clock_faces: Option<(TexId, TexId)>,
    clock_back: Option<(TexId, u32)>,
    quick_menu_faces: Option<QuickMenuFaces>,
    quick_clock_faces: Option<[(TexId, u32, u32); 2]>,
    quick_wallpaper_faces: Option<[(TexId, u32, u32); 2]>,
    quick_palette_faces: Option<[(TexId, u32, u32); 2]>,
    quick_bezel_faces: [Option<[(TexId, u32, u32); 2]>; Platform::COUNT],
    game_bezel: Option<TexId>,
    in_game_menu: Option<QuickRow>,
    /// The running core's own settings and the one picked, while Select+X has them over the game.
    core_options: Option<(Vec<CoreOption>, usize)>,
    core_options_wanted: bool,
    core_option_pending: Option<(String, String)>,
    /// 0 while the pause footer is off the bottom, 1 when it sits in the carousel's place.
    footer: f32,
    footer_row: Option<QuickRow>,
    states_from_menu: bool,
    menu_tap: Option<(Millis, Option<QuickRow>)>,
    sticker_face: Option<TexId>,
    ground: Option<TexId>,
    blur: Option<TexId>,
    wallpaper: Option<TexId>,
    random_wallpaper: Option<Option<String>>,
    battery_percent: slot_ui::Printed,
    bolt: Option<TexId>,
    hud_bolt: Option<TexId>,
    /// The footer's Wi-Fi and Bluetooth icons.
    net_faces: Option<[TexId; 2]>,
    slot_faces: Option<(TexId, TexId)>,
    shelf_platform: slot_ui::Printed,
    shelf_title: slot_ui::Printed,
    name_pending: bool,
    slot_letter: Option<char>,
    shelf_named: Option<Millis>,
    shelf_clock: slot_ui::Printed,
    hud: Hud,
    screen: f32,
    /// The game opens out of black, after a disc's launch went dark.
    launched_dark: bool,
    lift: f32,
    eject_overlay: f32,
    panel: (u32, u32),
    game_ready: bool,
    clock: f64,
    power: Option<Power>,
    dozed_at: Millis,
    woke_on_press: bool,
    autosave_at: Millis,
    pending_save: Option<std::thread::JoinHandle<()>>,
    card_writes: Arc<crate::card_log::CardWrites>,
    card_read_only: bool,
    card_check_at: Millis,
    battery_at: Millis,
    charge_at: Millis,
    headphones: bool,
    headphones_at: Millis,
    battery: Option<Battery>,
    last_led: Option<LedState>,
    powering_off: bool,
    radio: Box<dyn RadioJobs>,
    state_writer: crate::state_writer::StateWriter,
    wifi: Option<Wifi>,
    wifi_view: WifiView,
    bluetooth: Option<Bluetooth>,
    bt_view: BtView,
    services: Option<Services>,
    services_view: ServicesView,
    updates: Option<Updates>,
    updates_view: UpdatesView,
    core_downloads: Option<CoreDownloads>,
    cores_view: CoresView,
    scraper: Option<Scraper>,
    scrape_view: ScrapeView,
    /// The last search: what was typed, and the games it found by system and file name.
    search: Option<Search>,
    /// Each found game's picture for the results page, made as its row comes into view.
    search_icons: HashMap<(Platform, String), ListIcon>,
    /// How many of the scraper's new labels the shelves already show.
    labelled: usize,
    /// Carts given a label since their face was built, by shelf and index, for the face workers
    /// to build again. The old face stays up until the new one is in.
    relabelled: Vec<(usize, usize)>,
    /// Whether Game Art can ask ScreenScraper, read at boot and after a sign in.
    ss_state: SsState,
    page_face: Option<(TexId, u32, u32)>,
    /// How many files Syncthing had in step when the card's games were last looked through.
    synced_seen: u64,
    /// A look through the card's games under way, whether another is wanted when it is done,
    /// and what one found, waiting for the shelf to be showing.
    rescan: Option<std::sync::mpsc::Receiver<Vec<Cart>>>,
    rescan_again: bool,
    rescanned: Option<Vec<Cart>>,
    /// The line saying how far Syncthing has got.
    sync_face: Option<(TexId, u32, u32)>,
    /// Lines from the game's achievements waiting to be shown, the first since `cheevo_since`.
    cheevo_lines: VecDeque<String>,
    cheevo_since: Millis,
    cheevo_face: Option<(TexId, u32, u32)>,
    /// A RetroAchievements sign in waiting on the network.
    ra_sign_in: Option<std::sync::mpsc::Receiver<Result<crate::cheevos::Account, String>>>,
}

/// What a search was for and what it found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Search {
    pub query: String,
    pub found: Vec<(Platform, String)>,
}

/// A nudge of the rumble motor for something done in the menus: on at `at`, for `ms`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Buzz {
    pub at: Millis,
    pub ms: Millis,
}

/// How long the motor runs for a setting changed, and for something refused. A shorter pulse
/// does not get the motor turning.
const BUZZ_TOGGLE_MS: Millis = 60;
const BUZZ_REFUSE_MS: Millis = 150;
/// For the cart going home and coming out.
const BUZZ_SEAT_MS: Millis = 90;
const BUZZ_EJECT_MS: Millis = 70;

/// Where the player is, as the menu sounds hear it: how deep in the menus, which shelf, and
/// what the cursor is on.
#[derive(PartialEq, Eq, Debug)]
struct Feel {
    depth: usize,
    shelf: usize,
    spot: String,
}

const FACE_AHEAD: i32 = 8;
const FACE_KEEP: i32 = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaceWant {
    pub shelf: usize,
    pub index: usize,
    pub on_screen: bool,
}

fn shelves_of(carts: Vec<Cart>) -> Vec<(Rack, Shelf)> {
    let mut rows: Vec<(Rack, Vec<Cart>)> = [Rack::Recents, Rack::Favorites]
        .into_iter()
        .chain(Platform::ALL.iter().map(|p| Rack::Of(*p)))
        .map(|r| (r, Vec::new()))
        .collect();
    for cart in carts {
        if let Some((_, row)) = rows.iter_mut().find(|(r, _)| *r == Rack::Of(cart.platform)) {
            row.push(cart);
        }
    }
    rows.into_iter()
        .map(|(rack, carts)| (rack, Shelf::new(carts)))
        .collect()
}

/// "Syncing 42% · 1.2 GB left · 35 min".
pub fn sync_line(p: SyncProgress) -> String {
    let done = p.total.saturating_sub(p.need);
    let percent = (done * 100).checked_div(p.total).unwrap_or(0).min(99);
    let left = match p.need {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n => format!("{} MB", n.div_ceil(1 << 20)),
    };
    let mut line = format!("Syncing {percent}% \u{b7} {left} left");
    match p.secs_left {
        Some(s) if s < 60 => line.push_str(" \u{b7} under a minute"),
        Some(s) if s < 90 * 60 => line.push_str(&format!(" \u{b7} {} min", s.div_ceil(60))),
        Some(s) => line.push_str(&format!(" \u{b7} {} h {} min", s / 3600, s % 3600 / 60)),
        None => {}
    }
    line
}

fn shift_down(draws: &mut [Draw], dy: f32) {
    if dy == 0.0 {
        return;
    }
    for draw in draws {
        match draw {
            Draw::Rect { y, .. }
            | Draw::Tex { y, .. }
            | Draw::Part { y, .. }
            | Draw::Turned { y, .. } => *y += dy,
            Draw::Game | Draw::Shot { .. } => {}
        }
    }
}

impl App {
    pub fn new(carts: Vec<Cart>) -> Self {
        let shelves = shelves_of(carts);
        let shelf_at = shelves
            .iter()
            .position(|(_, s)| !s.carts.is_empty())
            .unwrap_or(0);
        let state_writer = crate::state_writer::StateWriter::spawn();
        let card_writes = state_writer.card_writes();
        App {
            radio: radio_jobs(),
            state_writer,
            phase: Phase::Shelf,
            shelves,
            shelf_at,
            orphaned: Vec::new(),
            play_held: None,
            held_dir: None,
            refusal: None,
            refused_from: None,
            alert_face: None,
            shutdown_faces: Vec::new(),
            core_picker: None,
            core_board_face: None,
            core_interior_face: None,
            core_lid_face: None,
            core_faces_stem: None,
            core_socket_faces: Vec::new(),
            core_chip_faces: Vec::new(),
            core_blank_chip_face: None,
            core_chip_shadow_face: None,
            core_legend_faces: Vec::new(),
            game_menu: None,
            link_sprites: None,
            link_shot: None,
            link_hardware: LinkKind::Cable,
            last_role: LinkRow::Host,
            link_choices: HashMap::new(),
            link_loaded: None,
            link_reload: None,
            reload: None,
            link_menu_faces: Vec::new(),
            link_linked_face: None,
            link_step_faces: Vec::new(),
            link_fail_faces: Vec::new(),
            link_legend_faces: Vec::new(),
            starting: None,
            link_transport: None,
            restarting: false,
            reloading: false,
            external_display: false,
            act_at: 0,
            root: None,
            state: SlotState::default(),
            vol_before: Vec::new(),
            snapshot: None,
            core: Core::default(),
            colour_pending: None,
            palette_live: false,
            palette_pending: None,
            disks: (0, 0),
            disc_pending: None,
            link_player: None,
            platform: Platform::default(),
            named_core: false,
            video_mode: VideoMode::default(),
            link: None,
            sfx: None,
            buzz: None,
            polaroids: None,
            pending: None,
            legend_faces: Vec::new(),
            undo_face: None,
            clock_faces: None,
            clock_back: None,
            quick_menu_faces: None,
            quick_clock_faces: None,
            quick_wallpaper_faces: None,
            quick_palette_faces: None,
            quick_bezel_faces: [None; Platform::COUNT],
            game_bezel: None,
            in_game_menu: None,
            core_options: None,
            core_options_wanted: false,
            core_option_pending: None,
            footer: 0.0,
            footer_row: None,
            states_from_menu: false,
            menu_tap: None,
            sticker_face: None,
            ground: None,
            blur: None,
            wallpaper: None,
            random_wallpaper: None,
            battery_percent: slot_ui::Printed::default(),
            bolt: None,
            hud_bolt: None,
            net_faces: None,
            slot_faces: None,
            shelf_platform: slot_ui::Printed::default(),
            shelf_title: slot_ui::Printed::default(),
            name_pending: false,
            slot_letter: None,
            shelf_named: None,
            shelf_clock: slot_ui::Printed::default(),
            hud: Hud::new(),
            screen: 0.0,
            launched_dark: false,
            lift: 1.0,
            eject_overlay: 0.0,
            panel: (0, 0),
            game_ready: false,
            clock: 0.0,
            power: None,
            dozed_at: 0,
            woke_on_press: false,
            autosave_at: AUTOSAVE_MS,
            pending_save: None,
            card_writes,
            card_read_only: false,
            card_check_at: 0,
            battery_at: BATTERY_POLL_MS,
            charge_at: CHARGE_POLL_MS,
            headphones: false,
            headphones_at: 0,
            battery: None,
            last_led: None,
            powering_off: false,
            wifi: None,
            wifi_view: WifiView::default(),
            bluetooth: None,
            bt_view: BtView::default(),
            services: None,
            services_view: ServicesView::default(),
            updates: None,
            updates_view: UpdatesView::default(),
            core_downloads: None,
            cores_view: CoresView::default(),
            scraper: None,
            scrape_view: ScrapeView::default(),
            search: None,
            search_icons: HashMap::new(),
            labelled: 0,
            relabelled: Vec::new(),
            ss_state: SsState::Off,
            page_face: None,
            synced_seen: 0,
            rescan: None,
            rescan_again: false,
            rescanned: None,
            sync_face: None,
            cheevo_lines: VecDeque::new(),
            cheevo_since: 0,
            cheevo_face: None,
            ra_sign_in: None,
        }
    }

    pub fn boot(root: &Path) -> Self {
        crate::root::ensure(root);
        crate::boot_time::mark("root");
        let carts = scan(root).unwrap_or_default();
        crate::boot_time::mark("scan");
        let mut app = App::new(carts);
        app.root = Some(root.to_path_buf());
        app.state = read_slot_state(root);
        let wifi = Wifi::for_card(root);
        if app.state.wifi {
            wifi.turn_on();
        }
        app.wifi = Some(wifi);
        if crate::bluetooth::present() {
            let bluetooth = Bluetooth::for_card(root);
            if app.state.bluetooth {
                bluetooth.turn_on();
            }
            app.bluetooth = Some(bluetooth);
        }
        let services = Services::for_card(root);
        services.want(app.wanted());
        app.services = Some(services);
        app.updates = Some(Updates::for_card(root));
        app.core_downloads = Some(CoreDownloads::for_card(root));
        app.scraper = Some(Scraper::for_card(root));
        app.ss_state = crate::scrape::ss_state(root);
        app.restock_recents();
        app.restock_favorites();
        if let Some((at, i)) = app.remembered_seat() {
            app.shelf_at = at;
            app.shelves[at].1.select(i);
        } else if !app.shelves[RECENTS_AT].1.carts.is_empty() {
            app.shelf_at = RECENTS_AT;
        }
        if app.state.clock_set {
            app.start();
        } else {
            app.phase = clock_screen(system_secs(), 0, false);
        }
        app
    }

    fn start(&mut self) {
        let seated = if self.single_cart() {
            Some((self.shelf_at, 0))
        } else {
            let stem = self.state.cart.clone();
            let platform = self.state.cart_platform;
            stem.and_then(|stem| {
                let shelf = &self.shelf().carts;
                let here = shelf
                    .iter()
                    .position(|c| c.stem == stem && platform.is_none_or(|p| c.platform == p));
                here.map(|i| (self.shelf_at, i))
                    .or_else(|| self.seat_of(&stem, platform))
            })
        };
        self.phase = Phase::Shelf;
        match seated {
            Some((at, i)) => {
                self.shelf_at = at;
                self.shelf_mut().select(i);
                self.insert(false);
                if let Phase::Inserting { resumed, t, .. } = &mut self.phase {
                    *resumed = true;
                    *t = INSERT_S;
                }
            }
            None => {
                self.state.cart = None;
                self.state.cart_platform = None;
                self.name_pending = true;
            }
        }
    }

    fn shelf(&self) -> &Shelf {
        &self.shelves[self.shelf_at].1
    }

    fn shelf_mut(&mut self) -> &mut Shelf {
        &mut self.shelves[self.shelf_at].1
    }

    fn switch_shelf(&mut self, by: i32) {
        let Some(to) = self.next_shelf(by) else {
            return;
        };
        self.shelf_mut().release_hold();
        self.play_held = None;
        self.shelf_at = to;
        self.shelf_platform = slot_ui::Printed::default();
        self.slot_letter = None;
        self.shelf_named = None;
        self.name_pending = true;
    }

    fn track_held_dir(&mut self, action: Action) {
        match action {
            Action::GbaDown(b @ (Btn::Up | Btn::Down | Btn::Left | Btn::Right)) => {
                if self.held_dir.map(|(held, ..)| held) != Some(b) {
                    self.held_dir = Some((b, self.now() + REPEAT_DELAY_MS, 0));
                }
            }
            Action::GbaUp(b) if self.held_dir.is_some_and(|(held, ..)| held == b) => {
                self.held_dir = None;
            }
            _ => {}
        }
    }

    fn repeat_held_dir(&mut self) {
        let Some((btn, due, fired)) = self.held_dir else {
            return;
        };
        if self.now() < due {
            return;
        }
        if !self.repeats(btn) {
            self.held_dir = None;
            return;
        }
        self.held_dir = Some((btn, self.now() + repeat_gap(fired), fired + 1));
        self.apply(Action::GbaDown(btn));
    }

    fn repeats(&self, btn: Btn) -> bool {
        let vertical = matches!(btn, Btn::Up | Btn::Down);
        if self.game_menu.is_some() {
            return false;
        }
        match self.phase {
            Phase::SetClock { .. } | Phase::Keyboard { .. } => true,
            Phase::QuickMenu { .. }
            | Phase::Networks { .. }
            | Phase::Devices { .. }
            | Phase::Services { .. }
            | Phase::Updates { .. }
            | Phase::CoreDownloads { .. }
            | Phase::GameArt { .. }
            | Phase::Cores { .. }
            | Phase::Search { .. } => vertical,
            Phase::Shelf => vertical && self.core_picker.is_none(),
            Phase::Polaroids { .. } => !vertical,
            Phase::Playing { .. } => vertical && self.in_game_menu.is_some(),
            _ => false,
        }
    }

    fn jump_letter(&mut self, dir: i32) {
        let before = self.shelf().index;
        match dir > 0 {
            true => self.shelf_mut().jump_next_letter(),
            false => self.shelf_mut().jump_prev_letter(),
        }
        if self.shelf().index == before {
            return;
        }
        let Some(letter) = self.selected_stem().map(slot_store::initial) else {
            return;
        };
        if self.slot_letter != Some(letter) {
            self.shelf_platform = slot_ui::Printed::default();
        }
        self.slot_letter = Some(letter);
        self.shelf_named = None;
        self.name_pending = true;
    }

    fn slot_name_alpha(&self) -> f32 {
        let Some(at) = self.shelf_named else {
            return 0.0;
        };
        let t = self.now().saturating_sub(at);
        let level = if t < SLOT_NAME_IN_MS {
            t as f32 / SLOT_NAME_IN_MS as f32
        } else if t < SLOT_NAME_IN_MS + SLOT_NAME_HOLD_MS {
            1.0
        } else {
            let out = t - SLOT_NAME_IN_MS - SLOT_NAME_HOLD_MS;
            1.0 - (out as f32 / SLOT_NAME_OUT_MS as f32).min(1.0)
        };
        SLOT_NAME_ALPHA * ease(level)
    }

    fn next_shelf(&self, by: i32) -> Option<usize> {
        let n = self.shelves.len() as i32;
        (1..n)
            .map(|step| (self.shelf_at as i32 + by * step).rem_euclid(n) as usize)
            .find(|at| !self.shelves[*at].1.carts.is_empty())
    }

    fn remembered_seat(&self) -> Option<(usize, usize)> {
        let key = self.state.shelf.as_deref()?;
        let cart = self.state.shelf_cart.as_ref()?;
        let at = self.shelves.iter().position(|(r, _)| r.key() == key)?;
        let i = self.shelves[at]
            .1
            .carts
            .iter()
            .position(|c| c.platform == cart.platform && c.stem == cart.stem)?;
        Some((at, i))
    }

    fn remember_shelf(&mut self) {
        let Some(cart) = self.shelf().carts.get(self.shelf().index) else {
            return;
        };
        self.state.shelf_cart = Some(Recent {
            platform: cart.platform,
            stem: cart.stem.clone(),
        });
        self.state.shelf = Some(self.shelves[self.shelf_at].0.key());
        self.persist();
    }

    fn seat_of(&self, stem: &str, platform: Option<Platform>) -> Option<(usize, usize)> {
        self.shelves
            .iter()
            .enumerate()
            .filter(|(_, (r, _))| {
                platform.is_none_or(|want| r.platform().is_none_or(|p| p == want))
            })
            .find_map(|(at, (_, shelf))| {
                shelf
                    .carts
                    .iter()
                    .position(|c| c.stem == stem && platform.is_none_or(|p| c.platform == p))
                    .map(|i| (at, i))
            })
    }

    pub fn confirm_clock(&mut self) {
        let Phase::SetClock {
            picker,
            seed,
            from_menu,
        } = &self.phase
        else {
            return;
        };
        let (moved, offset, from_menu) = (picker.secs() - *seed, picker.offset_min(), *from_menu);
        let utc = self.utc_secs() + moved;
        if let Some(power) = &mut self.power {
            power.set_clock(utc);
        }
        self.state.utc_offset_min = offset as i16;
        self.state.clock_set = true;
        self.persist();
        if from_menu {
            self.phase = Phase::QuickMenu {
                row: QuickRow::DateTime,
            };
        } else {
            self.start();
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn wall_secs(&self) -> i64 {
        self.utc_secs() + i64::from(self.state.utc_offset_min) * 60
    }

    fn utc_secs(&self) -> i64 {
        self.power.as_ref().map_or_else(system_secs, |p| p.now())
    }

    pub fn picker(&self) -> Option<&ClockPicker> {
        match &self.phase {
            Phase::SetClock { picker, .. } => Some(picker),
            _ => None,
        }
    }

    pub fn quick_menu(&self) -> Option<QuickRow> {
        match self.phase {
            Phase::QuickMenu { row } => Some(row),
            _ => None,
        }
    }

    pub fn in_game_menu(&self) -> Option<QuickRow> {
        self.in_game_menu
    }

    pub fn in_game_hidden(&self) -> Vec<QuickRow> {
        let no_bezel = !self.playing_platform().spec().bezels && !self.framed_panel();
        let mut hidden = Vec::new();
        if !self.palette_live {
            hidden.push(QuickRow::Palette);
        }
        if no_bezel {
            hidden.push(QuickRow::Bezel);
        }
        if self.single_cart() {
            hidden.push(QuickRow::ExitGame);
        }
        if self.disks.0 < 2 {
            hidden.push(QuickRow::Disc);
        }
        hidden
    }

    /// What the running core says of its discs: how many, and which is in the drive.
    pub fn set_disks(&mut self, count: u32, index: u32) {
        if self.disc_pending.is_none() {
            self.disks = (count, index);
        }
    }

    pub fn take_disc(&mut self) -> Option<u32> {
        self.disc_pending.take()
    }

    pub fn gb_palette_label(&self) -> &'static str {
        self.state.gb_palette.label()
    }

    pub fn set_quick_palette_faces(&mut self, dim: (TexId, u32, u32), lit: (TexId, u32, u32)) {
        self.quick_palette_faces = Some([dim, lit]);
    }

    fn playing_row(&self, row: QuickRow) -> QuickRow {
        let platform = self.playing_platform();
        match row {
            QuickRow::Shader => QuickRow::ShaderOf(platform),
            QuickRow::ColourCorrection => QuickRow::ColourCorrectionOf(platform),
            QuickRow::Bezel => QuickRow::BezelOf(platform),
            row => row,
        }
    }

    pub fn quick_value(&self, row: QuickRow) -> Option<QuickValue> {
        match row {
            QuickRow::Shader | QuickRow::ColourCorrection => {
                self.quick_value(self.playing_row(row))
            }
            QuickRow::FastForward => QuickValue::speed(self.state.ff_speed),
            QuickRow::FastForwardSound => Some(QuickValue::flag(self.state.ff_sound)),
            QuickRow::ColourCorrectionOf(p) => {
                Some(QuickValue::flag(self.state.screen(p).colour_correction))
            }
            QuickRow::GbPalettes => Some(QuickValue::flag(self.state.gb_palettes)),
            QuickRow::Disc => QuickValue::disc(self.disks.1),
            QuickRow::ShaderOf(p) => Some(shader_value(self.state.screen(p).shader)),
            QuickRow::Rumble => Some(QuickValue::flag(self.state.rumble)),
            QuickRow::EjectSave => Some(QuickValue::flag(self.state.eject_save)),
            QuickRow::Turbo => Some(QuickValue::flag(self.state.turbo)),
            QuickRow::Rewind => Some(QuickValue::flag(self.state.rewind)),
            QuickRow::RunAhead => QuickValue::frames(self.state.runahead),
            QuickRow::MenuSounds => Some(QuickValue::flag(self.state.menu_sounds)),
            QuickRow::Haptics => Some(QuickValue::flag(self.state.haptics)),
            QuickRow::Clock => Some(match self.state.clock_12h {
                true => QuickValue::Hours12,
                false => QuickValue::Hours24,
            }),
            QuickRow::Recents => Some(if self.state.show_recents {
                QuickValue::Show
            } else {
                QuickValue::Hide
            }),
            QuickRow::ShowTitles => Some(if self.state.show_titles {
                QuickValue::Show
            } else {
                QuickValue::Hide
            }),
            QuickRow::WallpaperTint => Some(match self.state.wallpaper_tint {
                WallpaperTint::Off => QuickValue::Off,
                WallpaperTint::Low => QuickValue::Low,
                WallpaperTint::Medium => QuickValue::Medium,
                WallpaperTint::High => QuickValue::High,
            }),
            QuickRow::Theme => Some(if self.light() {
                QuickValue::Light
            } else {
                QuickValue::Dark
            }),
            QuickRow::DateTime
            | QuickRow::About
            | QuickRow::Screen
            | QuickRow::Game
            | QuickRow::General
            | QuickRow::Wallpaper
            | QuickRow::Resume
            | QuickRow::States
            | QuickRow::Settings
            | QuickRow::ExitGame
            | QuickRow::PlayScreen
            | QuickRow::PlayGameplay
            | QuickRow::Palette
            | QuickRow::ScreenOf(_)
            | QuickRow::BezelOf(_)
            | QuickRow::Network
            | QuickRow::WifiNetworks
            | QuickRow::Bezel => None,
            QuickRow::Wifi => Some(QuickValue::flag(self.state.wifi)),
            QuickRow::Bluetooth => Some(QuickValue::flag(self.state.bluetooth)),
            QuickRow::BtDevices
            | QuickRow::Services
            | QuickRow::Updates
            | QuickRow::Cores
            | QuickRow::GameArt
            | QuickRow::Achievements
            | QuickRow::Power
            | QuickRow::Restart
            | QuickRow::ShutDown
            | QuickRow::Reload => None,
        }
    }

    pub fn set_quick_menu_faces(&mut self, faces: QuickMenuFaces) {
        self.quick_menu_faces = Some(faces);
    }

    pub fn set_quick_clock_faces(&mut self, dim: (TexId, u32, u32), lit: (TexId, u32, u32)) {
        self.quick_clock_faces = Some([dim, lit]);
    }

    pub fn set_quick_wallpaper_faces(&mut self, dim: (TexId, u32, u32), lit: (TexId, u32, u32)) {
        self.quick_wallpaper_faces = Some([dim, lit]);
    }

    pub fn set_quick_bezel_faces(
        &mut self,
        platform: Platform,
        dim: (TexId, u32, u32),
        lit: (TexId, u32, u32),
    ) {
        self.quick_bezel_faces[platform.index()] = Some([dim, lit]);
    }

    pub fn bezel_files(&self, platform: Platform) -> Vec<String> {
        if !platform.spec().bezels {
            return Vec::new();
        }
        self.root()
            .map(|root| crate::bezel::list(root, platform))
            .unwrap_or_default()
    }

    fn bezel_choices(&self, platform: Platform) -> Vec<BezelPick> {
        let mut choices = vec![BezelPick::None];
        if platform.spec().bezels {
            choices.push(BezelPick::Default);
        }
        if self.framed_panel() {
            choices.push(BezelPick::Ags102);
        }
        choices.extend(self.bezel_files(platform).into_iter().map(BezelPick::File));
        choices
    }

    fn bezel_pick(&mut self, platform: Platform) -> &mut BezelPick {
        &mut self.state.screen_mut(platform).bezel
    }

    pub fn resolved_bezel(&self, platform: Platform) -> BezelPick {
        let pick = &self.state.screen(platform).bezel;
        match self.bezel_choices(platform).contains(pick) {
            true => pick.clone(),
            false => BezelPick::None,
        }
    }

    fn cycle_bezel(&mut self, platform: Platform, right: bool) {
        let choices = self.bezel_choices(platform);
        let key = self.resolved_bezel(platform).key();
        let at = choices.iter().position(|c| c.key() == key).unwrap_or(0);
        let n = choices.len();
        *self.bezel_pick(platform) = choices[if right {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }]
        .clone();
    }

    pub fn sp_frame(&self) -> bool {
        self.resolved_bezel(self.playing_platform()) == BezelPick::Ags102
    }

    pub fn set_game_bezel(&mut self, tex: Option<TexId>) {
        self.game_bezel = tex;
    }

    pub fn shown_game_bezel(&self) -> Option<TexId> {
        if self.video_mode == VideoMode::Stretch {
            return None;
        }
        self.game_bezel
    }

    pub fn wallpaper_files(&self) -> Vec<String> {
        self.root().map(crate::wallpaper::list).unwrap_or_default()
    }

    pub fn wallpaper(&self) -> WallpaperPick {
        self.state.wallpaper.clone()
    }

    pub fn resolved_wallpaper(&self) -> WallpaperPick {
        match &self.state.wallpaper {
            WallpaperPick::File(name) if !self.wallpaper_files().iter().any(|n| n == name) => {
                WallpaperPick::Default
            }
            other => other.clone(),
        }
    }

    pub fn shown_wallpaper(&mut self) -> Option<String> {
        match self.resolved_wallpaper() {
            WallpaperPick::File(name) => Some(name),
            WallpaperPick::Random => self.random_wallpaper(),
            _ => None,
        }
    }

    fn random_wallpaper(&mut self) -> Option<String> {
        if self.random_wallpaper.is_none() {
            let seed = self.wall_secs().unsigned_abs();
            let pick = self
                .root()
                .and_then(|root| crate::wallpaper::pick(root, seed))
                .and_then(|path| Some(path.file_name()?.to_str()?.to_owned()));
            self.random_wallpaper = Some(pick);
        }
        self.random_wallpaper.clone().flatten()
    }

    fn cycle_wallpaper(&mut self, right: bool) {
        let mut choices = vec![
            WallpaperPick::None,
            WallpaperPick::Random,
            WallpaperPick::Default,
            WallpaperPick::Blur,
        ];
        choices.extend(self.wallpaper_files().into_iter().map(WallpaperPick::File));
        let key = self.resolved_wallpaper().key();
        let at = choices.iter().position(|c| c.key() == key).unwrap_or(2);
        let n = choices.len();
        let next = if right {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        };
        self.state.wallpaper = choices[next].clone();
    }

    fn menu_ground(&self) -> Option<TexId> {
        match &self.state.wallpaper {
            WallpaperPick::None => None,
            WallpaperPick::Default | WallpaperPick::Blur => self.blur.or(self.ground),
            WallpaperPick::Random if !self.random_drawn() => self.blur.or(self.ground),
            WallpaperPick::File(_) | WallpaperPick::Random => self.wallpaper.or(self.ground),
        }
    }

    fn draw_background(&self, out: &mut Vec<Draw>) {
        let face = match &self.state.wallpaper {
            WallpaperPick::None => None,
            WallpaperPick::Default => self.ground,
            WallpaperPick::Random if !self.random_drawn() => self.ground,
            WallpaperPick::Blur => self.blur.or(self.ground),
            WallpaperPick::File(_) | WallpaperPick::Random => self.wallpaper.or(self.ground),
        };
        draw_ground(self.canvas(), face, out);
        let alpha = tint_alpha(self.state.wallpaper_tint);
        if alpha > 0.0 && self.own_wallpaper_shown() {
            out.push(Draw::Rect {
                x: 0.0,
                y: 0.0,
                w: self.canvas().w,
                h: OUT_H as f32,
                colour: [0.0, 0.0, 0.0, alpha],
            });
        }
    }

    fn own_wallpaper_shown(&self) -> bool {
        match self.state.wallpaper {
            WallpaperPick::File(_) => self.wallpaper.is_some(),
            WallpaperPick::Random => self.random_drawn() && self.wallpaper.is_some(),
            _ => false,
        }
    }

    pub fn hidden_rows(&self) -> Vec<QuickRow> {
        let own = matches!(
            self.state.wallpaper,
            WallpaperPick::File(_) | WallpaperPick::Random
        );
        // A disc is only swapped from inside a game.
        let mut hidden = vec![QuickRow::Disc];
        if !self.state.wifi {
            hidden.push(QuickRow::WifiNetworks);
        }
        match self.bluetooth.is_some() {
            true if !self.state.bluetooth => hidden.push(QuickRow::BtDevices),
            true => {}
            false => hidden.extend([QuickRow::Bluetooth, QuickRow::BtDevices]),
        }
        if !own {
            hidden.push(QuickRow::WallpaperTint);
        }
        // The Game Boy family's pages are always there; another system's only once the card
        // holds a game for it.
        hidden.extend(
            Platform::ALL
                .into_iter()
                .filter(|p| p.spec().native && !self.has_games(*p))
                .map(QuickRow::ScreenOf),
        );
        if !self.framed_panel() {
            // Without the SP's frame a platform with no bezels of its own has nothing to pick.
            hidden.extend(
                Platform::ALL
                    .into_iter()
                    .filter(|p| !p.spec().bezels)
                    .map(QuickRow::BezelOf),
            );
        }
        hidden
    }

    fn has_games(&self, platform: Platform) -> bool {
        self.shelves
            .iter()
            .any(|(rack, shelf)| *rack == Rack::Of(platform) && !shelf.carts.is_empty())
    }

    fn random_drawn(&self) -> bool {
        matches!(self.random_wallpaper, Some(Some(_)))
    }

    pub fn set_sticker_face(&mut self, face: TexId) {
        self.sticker_face = Some(face);
    }

    pub fn set_clock_faces(&mut self, line: TexId, hint: TexId) {
        self.clock_faces = Some((line, hint));
    }

    pub fn set_clock_back_face(&mut self, back: (TexId, u32)) {
        self.clock_back = Some(back);
    }

    pub fn set_star_face(&mut self, face: TexId, w: u32, h: u32) {
        for (_, shelf) in &mut self.shelves {
            shelf.set_star(face, w, h);
        }
    }

    fn mark_stars(&mut self) {
        let state = &self.state;
        for (_, shelf) in &mut self.shelves {
            shelf.mark_starred(|c| state.is_favorite(c.platform, &c.stem));
        }
    }

    pub fn set_cart_shadow(&mut self, mould: Mould, face: TexId) {
        for (_, shelf) in &mut self.shelves {
            shelf.set_shadow(mould, face);
        }
    }

    pub fn set_cart_backing(&mut self, mould: Mould, face: TexId) {
        for (_, shelf) in &mut self.shelves {
            shelf.set_backing(mould, face);
        }
    }

    pub fn set_ground(&mut self, face: TexId) {
        self.ground = Some(face);
    }

    pub fn set_blur(&mut self, face: TexId) {
        self.blur = Some(face);
    }

    pub fn set_wallpaper(&mut self, face: TexId) {
        self.wallpaper = Some(face);
    }

    /// Whether the clock reads 7:05 PM rather than 19:05.
    pub fn clock_12h(&self) -> bool {
        self.state.clock_12h
    }

    pub fn set_net_faces(&mut self, faces: [TexId; 2]) {
        self.net_faces = Some(faces);
    }

    /// The footer's icons for the networks that are up: Wi-Fi when it has joined one, Bluetooth
    /// when a device is connected.
    pub fn net_icons(&self) -> Vec<TexId> {
        let Some([wifi, bt]) = self.net_faces else {
            return Vec::new();
        };
        let mut icons = Vec::new();
        if self.wifi_view.connected.is_some() {
            icons.push(wifi);
        }
        if self.bt_view.devices.iter().any(|d| d.connected) {
            icons.push(bt);
        }
        icons
    }

    pub fn set_bolt_face(&mut self, bolt: TexId) {
        self.bolt = Some(bolt);
    }

    pub fn set_hud_bolt_face(&mut self, bolt: TexId) {
        self.hud_bolt = Some(bolt);
    }

    pub fn set_slot_faces(&mut self, back: TexId, front: TexId) {
        self.slot_faces = Some((back, front));
    }

    pub fn set_shelf_platform_face(&mut self, face: TexId, w: u32) {
        self.shelf_platform = slot_ui::Printed::new(face, w);
    }

    pub fn clear_shelf_platform(&mut self) {
        self.shelf_platform = slot_ui::Printed::default();
    }

    pub fn slot_text(&self) -> Option<String> {
        match self.slot_letter {
            Some(letter) => Some(letter.to_string()),
            None => self.shelf_platform_name().map(str::to_string),
        }
    }

    pub fn title_text(&self) -> Option<String> {
        if !self.state.show_titles {
            return None;
        }
        let shelf = self.shelf();
        shelf.carts.get(shelf.index).map(slot_ui::label_text)
    }

    pub fn set_shelf_title_face(&mut self, face: TexId, w: u32) {
        self.shelf_title = slot_ui::Printed::new(face, w);
    }

    pub fn clear_shelf_title(&mut self) {
        self.shelf_title = slot_ui::Printed::default();
    }

    fn title_alpha(&self) -> f32 {
        if !self.state.show_titles || (self.name_pending && self.slot_text().is_some()) {
            return 0.0;
        }
        let named = self.shelf_named.map(|at| self.now().saturating_sub(at));
        if named.is_some_and(|t| t < SLOT_NAME_IN_MS + SLOT_NAME_HOLD_MS) {
            return 0.0;
        }
        1.0 - self.slot_name_alpha() / SLOT_NAME_ALPHA
    }

    pub fn shelf_platform_name(&self) -> Option<&'static str> {
        self.next_shelf(1)?;
        Some(self.shelves[self.shelf_at].0.name())
    }

    pub fn set_battery_percent_face(&mut self, face: TexId, w: u32) {
        self.battery_percent = slot_ui::Printed::new(face, w);
    }

    pub fn set_shelf_clock_face(&mut self, face: TexId, w: u32) {
        self.shelf_clock = slot_ui::Printed::new(face, w);
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn carts(&self) -> impl Iterator<Item = &Cart> {
        self.shelves
            .iter()
            .filter(|(r, _)| r.platform().is_some())
            .flat_map(|(_, shelf)| shelf.carts.iter())
    }

    pub fn seated_cart(&self) -> Option<&Cart> {
        let stem = match &self.phase {
            Phase::Inserting { cart, .. }
            | Phase::Playing { cart }
            | Phase::Ejecting { cart, .. }
            | Phase::Polaroids { cart } => cart,
            Phase::Doze { cart: Some(cart) } => cart,
            _ => return None,
        };
        self.shelf().carts.iter().find(|c| c.stem == *stem)
    }

    pub fn single_cart(&self) -> bool {
        self.carts().count() == 1
    }

    pub fn set_faces(&mut self, faces: Vec<TexId>) {
        let mut faces = faces.into_iter();
        for (_, shelf) in &mut self.shelves {
            let n = shelf.carts.len();
            shelf.set_faces(faces.by_ref().take(n).collect());
        }
    }

    pub fn face_wants(&self) -> Vec<FaceWant> {
        let here = self.shelf();
        let screen = here.on_screen();
        let mut wants: Vec<(usize, usize)> = screen.iter().map(|i| (self.shelf_at, *i)).collect();
        for at in self.neighbour_shelves() {
            wants.extend(self.shelves[at].1.on_screen().into_iter().map(|i| (at, i)));
        }
        wants.extend(
            here.around(FACE_AHEAD)
                .into_iter()
                .filter(|i| !screen.contains(i))
                .map(|i| (self.shelf_at, i)),
        );
        wants
            .into_iter()
            .filter(|(at, i)| self.shelves[*at].1.face(*i).is_none())
            .map(|(shelf, index)| FaceWant {
                shelf,
                index,
                on_screen: shelf == self.shelf_at && screen.contains(&index),
            })
            .collect()
    }

    pub fn face_cart(&self, shelf: usize, index: usize) -> Option<&Cart> {
        self.shelves.get(shelf)?.1.carts.get(index)
    }

    pub fn set_cart_face(&mut self, shelf: usize, index: usize, tex: TexId) -> Option<TexId> {
        self.shelves.get_mut(shelf)?.1.set_face(index, tex)
    }

    pub fn set_cart_interior(&mut self, shelf: usize, index: usize, tex: TexId) -> Option<TexId> {
        self.shelves.get_mut(shelf)?.1.set_interior(index, tex)
    }

    /// The carts whose faces want building again, now they have a label.
    pub fn take_relabelled(&mut self) -> Vec<(usize, usize)> {
        std::mem::take(&mut self.relabelled)
    }

    /// Puts each label the scraper has made since last time on its cart, on every shelf the
    /// game is on, so it shows without slot starting again.
    fn show_scraped(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let made = self
            .scrape_view
            .made
            .get(self.labelled..)
            .unwrap_or_default();
        for (platform, stem) in made {
            let label = root
                .join("Labels")
                .join(platform.dir_name())
                .join(format!("{stem}.png"));
            for (at, (_, shelf)) in self.shelves.iter_mut().enumerate() {
                for i in 0..shelf.carts.len() {
                    let cart = &mut shelf.carts[i];
                    if cart.platform != *platform || cart.stem != *stem {
                        continue;
                    }
                    cart.label = Some(label.clone());
                    // A cart with no face yet gets one with the label when it comes into view.
                    if shelf.face(i).is_some() {
                        self.relabelled.push((at, i));
                    }
                }
            }
        }
        self.labelled = self.scrape_view.made.len();
    }

    pub fn shed_faces(&mut self) -> Vec<TexId> {
        let neighbours = self.neighbour_shelves();
        let mut shed = std::mem::take(&mut self.orphaned);
        for (at, (_, shelf)) in self.shelves.iter_mut().enumerate() {
            let keep = if at == self.shelf_at {
                shelf.around(FACE_KEEP)
            } else if neighbours.contains(&at) {
                shelf.on_screen()
            } else {
                Vec::new()
            };
            for i in 0..shelf.carts.len() {
                if !keep.contains(&i) {
                    shed.extend(shelf.take_face(i));
                    shed.extend(shelf.take_interior(i));
                }
            }
        }
        shed
    }

    fn neighbour_shelves(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for by in [1, -1] {
            if let Some(at) = self.next_shelf(by) {
                if !out.contains(&at) {
                    out.push(at);
                }
            }
        }
        out
    }

    pub fn set_snapshot(&mut self, snapshot: Box<dyn Snapshot>) {
        self.snapshot = Some(snapshot);
    }

    pub fn set_core(&mut self, core: Core) {
        self.core = core;
    }

    pub fn set_platform(&mut self, platform: Platform) {
        self.platform = platform;
    }

    pub fn set_named_core(&mut self, named: bool) {
        self.named_core = named;
    }

    pub fn set_video_mode(&mut self, mode: VideoMode) {
        self.video_mode = mode;
    }

    pub fn video_mode(&self) -> VideoMode {
        self.video_mode
    }

    pub fn source_rect(&self) -> [f32; 4] {
        video_mode::source_rect(self.platform, self.video_mode)
    }

    fn slot_owns_the_shoulders(&self) -> bool {
        matches!(self.phase, Phase::Playing { .. }) && !self.platform.spec().shoulders
    }

    pub fn taken_buttons(&self) -> &'static [Btn] {
        if self.slot_owns_the_shoulders() {
            &[Btn::L1, Btn::R1]
        } else {
            &[]
        }
    }

    pub fn takes_from_the_game(&self, action: Action) -> bool {
        match action {
            Action::GbaDown(btn) | Action::GbaUp(btn) => self.taken_buttons().contains(&btn),
            _ => false,
        }
    }

    fn set_picture(&mut self, mode: VideoMode) {
        if self.video_mode == mode {
            return;
        }
        self.video_mode = mode;
        let (Some(root), Phase::Playing { cart }) = (self.root.clone(), &self.phase) else {
            return;
        };
        let written = video_mode::write_video_mode(&root, cart, mode);
        if let Err(e) = &written {
            eprintln!("slot: video: could not write video_mode.txt: {e}");
        }
        self.card_writes.wrote("video_mode.txt", &written);
    }

    pub fn set_link_loaded(&mut self, serial: &'static str) {
        self.link_loaded = Some(serial);
    }

    pub fn link_mode(&self, stem: &str) -> (LinkKind, &'static str) {
        let Some((cart, auto)) = self.auto_link(stem) else {
            return (LinkKind::Cable, "auto");
        };
        let chosen = self.link_choices.get(stem).copied().unwrap_or(auto);
        (chosen, serial_option(chosen, auto, &cart.code, &cart.title))
    }

    pub fn link_active(&self) -> bool {
        self.link.is_some()
    }

    pub fn begin_link(&mut self, client_id: u16) {
        self.link = Some(LinkSession {
            client_id,
            lost_at: None,
        });
        self.sync_link_badge();
    }

    pub fn link_client_id(&self) -> Option<u16> {
        self.link.as_ref().map(|s| s.client_id)
    }

    pub fn may_rewind(&self) -> bool {
        self.records_rewind() && !self.link_active()
    }

    pub fn turbo(&self) -> bool {
        self.state.turbo && self.platform.spec().turbo
    }

    pub fn records_rewind(&self) -> bool {
        self.state.rewind && self.platform.spec().rewind
    }

    /// Whether L2 and R2 belong to the game, not to rewind and fast forward.
    pub fn game_triggers(&self) -> bool {
        matches!(self.phase, Phase::Playing { .. })
            && self.in_game_menu.is_none()
            && self.platform.spec().triggers
    }

    pub fn picture_rect(&self) -> [f32; 4] {
        let (w, h) = self.platform.picture();
        let (sw, sh) = (slot_gfx::SRC_W as f32, slot_gfx::SRC_H as f32);
        let (x, y) = ((sw - w as f32) / 2.0, (sh - h as f32) / 2.0);
        [x / sw, y / sh, (x + w as f32) / sw, (y + h as f32) / sh]
    }

    pub fn may_load_state(&self) -> bool {
        !self.link_active()
    }

    pub fn may_fast_forward(&self) -> bool {
        !self.link_active()
    }

    pub fn end_link(&mut self) {
        self.link = None;
        self.link_player = None;
        self.sync_link_badge();
        self.radio.ask(RadioJob::Down);
        self.cool_radio();
    }

    pub fn peer_lost(&mut self) {
        if matches!(self.game_menu, Some(GameMenu::Linked { .. })) {
            self.game_menu = None;
        }
        let now = self.now();
        if let Some(session) = &mut self.link {
            session.lost_at.get_or_insert(now);
        }
        self.sync_link_badge();
    }

    pub fn bios_mismatch(&mut self) {
        if !self.link_active() {
            return;
        }
        let role = self
            .link_client_id()
            .map_or(self.last_role, LinkRow::from_client_id);
        self.end_link();
        self.hud.toast(Toast::BiosMismatch, self.now());
        self.unplug(role);
    }

    pub fn peer_ended(&mut self) {
        if !self.link_active() {
            return;
        }
        let role = self
            .link_client_id()
            .map_or(self.last_role, LinkRow::from_client_id);
        self.end_link();
        self.hud.toast(Toast::PeerEnded, self.now());
        self.unplug(role);
    }

    pub fn link_badge(&self) -> LinkBadge {
        self.hud.link()
    }

    pub fn set_link_badge_faces(&mut self, faces: Vec<TexId>) {
        self.hud.set_link_faces(faces);
    }

    fn sync_link_badge(&mut self) {
        let badge = match &self.link {
            None => LinkBadge::Off,
            Some(s) => match (s.client_id == 0, s.lost_at.is_some()) {
                (true, false) => LinkBadge::Hosting,
                (false, false) => LinkBadge::Joined,
                (true, true) => LinkBadge::HostingLost,
                (false, true) => LinkBadge::JoinedLost,
            },
        };
        self.hud.set_link(badge);
    }

    pub fn take_sfx(&mut self) -> Option<Sfx> {
        self.sfx.take()
    }

    pub fn set_power(&mut self, mut power: Power) {
        power.set_backlight(self.state.brightness);
        self.headphones = power.headphones();
        self.hud.set_headphones(self.headphones);
        let secs = power.now();
        if matches!(self.phase, Phase::SetClock { .. }) || secs < CLOCK_FLOOR {
            self.phase = clock_screen(secs, 0, false);
        }
        self.copy_card_log(&power);
        self.power = Some(power);
        self.battery_at = self.now();
        self.charge_at = self.now();
        self.last_led = None;
    }

    fn copy_card_log(&self, power: &Power) {
        let (Some(log), Some(root)) = (power.card_log(), &self.root) else {
            return;
        };
        if let Err(e) = crate::card_log::copy_to_card(&log, root) {
            eprintln!("slot: card log: {e}");
        }
    }

    fn log_card(&self, alarm: bool) {
        let Some(power) = &self.power else {
            return;
        };
        let Some(log) = power.card_log() else {
            return;
        };
        let kernel = if alarm {
            power.kernel_lines()
        } else {
            Vec::new()
        };
        let write_error = self.card_writes.last_error();
        let entry = crate::card_log::entry(&crate::card_log::CardEvent {
            secs: power.now(),
            alarm,
            read_only: self.card_read_only,
            write_error: write_error.as_deref(),
            cart: self.seated(),
            kernel: &kernel,
        });
        if let Err(e) = crate::card_log::append(&log, &entry) {
            eprintln!("slot: card log: {e}");
        }
    }

    pub fn set_rumble(&mut self, strength: u16) {
        if let Some(power) = &mut self.power {
            power.set_rumble(strength);
        }
    }

    pub fn rumble_enabled(&self) -> bool {
        self.state.rumble
    }

    pub fn ff_speed(&self) -> u8 {
        self.state.ff_speed
    }

    pub fn ff_sound(&self) -> bool {
        self.state.ff_sound
    }

    pub fn runahead(&self) -> u8 {
        self.state.runahead
    }

    pub fn runahead_in_play(&self) -> u8 {
        runahead_for(self.core, self.platform, self.state.runahead)
    }

    pub fn screen_shader(&self) -> Shader {
        self.state.screen(self.playing_platform()).shader
    }

    pub fn colour_correction(&self) -> bool {
        self.colour_correction_for(self.playing_platform())
    }

    pub fn playing_platform(&self) -> Platform {
        self.seated_cart().map_or(self.platform, |c| c.platform)
    }

    pub fn colour_correction_for(&self, platform: Platform) -> bool {
        self.state.screen(platform).colour_correction
    }

    pub fn light(&self) -> bool {
        self.state.appearance.light()
    }

    pub fn status_ink(&self) -> [u8; 3] {
        match self.phase {
            Phase::Playing { .. } | Phase::Polaroids { .. } => footer_ink(false),
            _ => footer_ink(self.light()),
        }
    }

    pub fn powering_off(&self) -> bool {
        self.powering_off
    }

    pub fn ready_to_power_off(&self) -> bool {
        self.powering_off && self.now() >= self.act_at
    }

    pub fn ready_to_restart(&self) -> bool {
        self.restarting && self.now() >= self.act_at
    }

    pub fn ready_to_reload(&self) -> bool {
        self.reloading && self.now() >= self.act_at
    }

    pub fn shutting_down(&self) -> bool {
        self.powering_off || self.restarting || self.reloading
    }

    pub fn restarting(&self) -> bool {
        self.restarting
    }

    pub fn restart(&mut self) {
        self.settle_saves();
        if let Some(power) = &mut self.power {
            power.restart();
        }
    }

    /// Settles the saves before slot exits, for whatever started it to start it again.
    pub fn reload(&mut self) {
        self.settle_saves();
    }

    /// Saves everything, as for power off, before slot exits to move the picture to or from a
    /// TV: the screen's size is fixed once it opens, so slot starts again on the new one.
    pub fn switch_display(&mut self) {
        if self.link_active() {
            self.end_link();
        }
        self.close_game_menu();
        self.remember_shelf();
        self.flush_resume();
        self.settle_saves();
    }

    /// Says the picture is on a TV, where closing the lid should not put the game to sleep.
    pub fn set_external_display(&mut self, on: bool) {
        self.external_display = on;
    }

    pub fn core_picker(&self) -> Option<Core> {
        self.core_picker.map(|p| p.seat())
    }

    pub fn core_picker_chip(&self) -> Option<Chip> {
        self.core_picker.map(|p| p.chip(self.now()))
    }

    pub fn game_menu_open(&self) -> bool {
        self.game_menu.is_some()
    }

    pub fn game_menu(&self) -> Option<GameMenu> {
        self.game_menu
    }

    pub fn take_link_transport(&mut self) -> Option<(u16, Box<dyn LinkChannel>)> {
        self.link_transport.take()
    }

    pub fn take_link_reload(&mut self) -> Option<(String, &'static str)> {
        self.link_reload.take()
    }

    pub fn link_player(&self) -> Option<u8> {
        self.link_player
    }

    pub fn core_options_open(&self) -> bool {
        self.core_options.is_some()
    }

    /// Whether the player asked for the core's options, for the session to fetch them.
    pub fn take_core_options_wanted(&mut self) -> bool {
        std::mem::take(&mut self.core_options_wanted)
    }

    /// Opens the core options page on what the core declared, or refuses when it declared none.
    pub fn show_core_options(&mut self, options: Vec<CoreOption>) {
        if options.is_empty() || !matches!(self.phase, Phase::Playing { .. }) {
            return self.refuse();
        }
        self.core_options = Some((options, 0));
    }

    /// The option the player last changed, for the session to hand the core and keep.
    pub fn take_core_option(&mut self) -> Option<(String, String)> {
        self.core_option_pending.take()
    }

    fn core_options_input(&mut self, action: Action) {
        let Some((options, cursor)) = &mut self.core_options else {
            return;
        };
        let step = match action {
            Action::GbaDown(Btn::Up) => {
                *cursor = (*cursor + options.len() - 1) % options.len();
                return;
            }
            Action::GbaDown(Btn::Down) => {
                *cursor = (*cursor + 1) % options.len();
                return;
            }
            Action::GbaDown(Btn::Left) | Action::ShelfLeft => -1,
            Action::GbaDown(Btn::Right) | Action::ShelfRight | Action::GbaDown(Btn::A) => 1,
            Action::GbaDown(Btn::B) | Action::CoreOptions | Action::QuickMenu => {
                self.core_options = None;
                return;
            }
            _ => return,
        };
        let o = &mut options[*cursor];
        let at = o.values.iter().position(|v| *v == o.value).unwrap_or(0) as isize;
        let n = o.values.len() as isize;
        o.value = o.values[(at + step).rem_euclid(n) as usize].clone();
        self.core_option_pending = Some((o.key.clone(), o.value.clone()));
    }

    fn core_options_page(&self, options: &[CoreOption], cursor: usize) -> ListPage {
        ListPage {
            title: "Core Options".into(),
            status: self.core().text().into(),
            cursor,
            rows: options
                .iter()
                .map(|o| ListRow {
                    label: o.label.clone(),
                    detail: o.value.clone(),
                    bars: None,
                    icon: None,
                })
                .collect(),
            legends: vec![back_legend(), Legend::new(ButtonLegend::Right, "Change")],
        }
    }

    pub fn take_colour_correction(&mut self) -> Option<bool> {
        self.colour_pending.take()
    }

    pub fn gb_palette(&self) -> Option<GbPalette> {
        self.state.gb_palettes.then_some(self.state.gb_palette)
    }

    pub fn set_palette_live(&mut self, live: bool) {
        self.palette_live = live;
    }

    pub fn palette_live(&self) -> bool {
        self.palette_live
    }

    pub fn take_gb_palette(&mut self) -> Option<GbPalette> {
        self.palette_pending.take()
    }

    pub fn core(&self) -> Core {
        self.core
    }

    pub fn link_reload_done(&mut self) {
        let Some(reload) = self.reload.take() else {
            return;
        };
        if reload.fallback {
            self.link_choices.insert(reload.stem, reload.from);
            self.close_game_menu();
            return self.refuse();
        }
        if reload.cancelled {
            return self.close_game_menu();
        }
        let since = match self.game_menu {
            Some(GameMenu::Working { since, .. }) => since,
            _ => self.now(),
        };
        self.start_link_from(
            LinkStarter::spawn(reload.role.role(), link_port()),
            reload.role.client_id(),
            since,
        );
    }

    pub fn link_reload_failed(&mut self) {
        let Some(mut reload) = self.reload.take() else {
            return;
        };
        if !reload.fallback {
            self.link_reload = Some((reload.stem.clone(), reload.from_serial));
            reload.fallback = true;
            self.reload = Some(reload);
            return;
        }
        self.link_choices.insert(reload.stem, reload.from);
        self.refuse_seated();
    }

    pub fn selected_stem(&self) -> Option<&str> {
        self.shelf()
            .carts
            .get(self.shelf().index)
            .map(|c| c.stem.as_str())
    }

    pub fn battery(&self) -> Option<Battery> {
        self.battery
    }

    pub fn poweroff(&mut self) {
        self.settle_saves();
        if let Some(power) = &mut self.power {
            power.poweroff();
        }
    }

    pub fn apply(&mut self, action: Action) {
        let before = (self.feel(), self.state.clone());
        self.act(action);
        self.answer(action, before);
    }

    /// Where the player is in the menus, or None while a cart goes in or out or the screen is
    /// off, which have their own sounds or none. A game with no menu over it is the top.
    fn feel(&self) -> Option<Feel> {
        let hops = |row: QuickRow| {
            std::iter::successors(row.parent(), |r| r.parent())
                .take(8)
                .count()
        };
        let (depth, spot) = match &self.phase {
            Phase::Shelf => match &self.core_picker {
                Some(picker) => (1, format!("{:?}", picker.seat())),
                None => (0, self.shelf().index.to_string()),
            },
            Phase::QuickMenu { row } => (1 + hops(*row), format!("{row:?}")),
            Phase::About => (2, String::new()),
            Phase::SetClock { picker, .. } => (3, format!("{picker:?}")),
            Phase::Networks { cursor, .. }
            | Phase::Devices { cursor }
            | Phase::Services { cursor, .. }
            | Phase::Updates { cursor }
            | Phase::CoreDownloads { cursor }
            | Phase::GameArt { cursor, .. } => (3, cursor.to_string()),
            Phase::Cores { cursor } | Phase::Search { cursor } => (1, cursor.to_string()),
            Phase::Keyboard { keys, .. } => (4, format!("{keys:?}")),
            Phase::TailscaleLogin | Phase::SyncthingId => (4, String::new()),
            Phase::Polaroids { .. } => (
                1,
                format!("{:?}", self.polaroids.as_ref().map(Polaroids::dots)),
            ),
            Phase::Playing { .. } => match (&self.game_menu, self.in_game_menu) {
                (Some(menu), _) => (1, format!("{menu:?}")),
                (None, Some(row)) => (1 + hops(row), format!("{row:?}")),
                (None, None) => (0, String::new()),
            },
            Phase::Inserting { .. } | Phase::Ejecting { .. } | Phase::Doze { .. } => return None,
        };
        Some(Feel {
            depth,
            shelf: self.shelf_at,
            spot,
        })
    }

    /// The click for what `action` did: in a page, back out of one, along a list, to another
    /// shelf, or a setting changed. A on a page of actions clicks whatever it starts.
    fn answer(&mut self, action: Action, (before, state): (Option<Feel>, SlotState)) {
        // Only the buttons that work the menus: not volume, the screen's light or the hotkeys.
        let menu_button = matches!(
            action,
            Action::GbaDown(_)
                | Action::ShelfLeft
                | Action::ShelfRight
                | Action::QuickMenu
                | Action::GameMenu
                | Action::CoreOptions
                | Action::Polaroids
        );
        let (true, Some(before), Some(after)) = (menu_button, before, self.feel()) else {
            return;
        };
        // The game's own buttons.
        if before.depth == 0 && after.depth == 0 && matches!(self.phase, Phase::Playing { .. }) {
            return;
        }
        let on_page = matches!(
            self.phase,
            Phase::Networks { .. }
                | Phase::Devices { .. }
                | Phase::Services { .. }
                | Phase::Updates { .. }
                | Phase::CoreDownloads { .. }
                | Phase::GameArt { .. }
                | Phase::Cores { .. }
                | Phase::Search { .. }
        );
        let sfx = match () {
            _ if after.depth > before.depth => Sfx::Open,
            _ if after.depth < before.depth => Sfx::Back,
            _ if after.shelf != before.shelf => Sfx::Shelf,
            _ if after.spot != before.spot => Sfx::Move,
            _ if self.state != state => Sfx::Toggle,
            _ if on_page && action == Action::GbaDown(Btn::A) => Sfx::Open,
            _ => return,
        };
        self.cue(sfx);
        if sfx == Sfx::Toggle {
            self.buzz(BUZZ_TOGGLE_MS, 0);
        }
    }

    /// Plays a menu sound, unless they are off or the cart is making its own.
    fn cue(&mut self, sfx: Sfx) {
        if !self.state.menu_sounds || self.sfx.is_some_and(Sfx::is_cart) {
            return;
        }
        self.sfx = Some(sfx);
    }

    /// Runs the motor for `ms`, starting `after` from now, unless the menus are not to.
    fn buzz(&mut self, ms: Millis, after: Millis) {
        if !self.state.haptics {
            return;
        }
        self.buzz = Some(Buzz {
            at: self.now() + after,
            ms,
        });
    }

    pub fn take_buzz(&mut self) -> Option<Buzz> {
        self.buzz.take()
    }

    fn act(&mut self, action: Action) {
        self.track_held_dir(action);
        match action {
            Action::LidClose | Action::LidOpen if self.external_display => return,
            Action::LidClose => return self.doze(),
            Action::LidOpen => return self.wake(),
            Action::PowerPress => {
                if self.link_active() {
                    self.end_link();
                    return;
                }
                self.woke_on_press = matches!(self.phase, Phase::Doze { .. });
                if self.woke_on_press {
                    self.wake();
                }
                return self.flush_resume();
            }
            Action::PowerTap => return self.power_press(),
            Action::PowerHold => return self.hold_power(),
            Action::PowerOff => return,
            _ => {}
        }
        if let Phase::SetClock {
            picker, from_menu, ..
        } = &mut self.phase
        {
            match action {
                Action::GbaDown(Btn::Left) | Action::ShelfLeft => picker.left(),
                Action::GbaDown(Btn::Right) | Action::ShelfRight => picker.right(),
                Action::GbaDown(Btn::Up) => picker.up(),
                Action::GbaDown(Btn::Down) => picker.down(),
                Action::GbaDown(Btn::A) | Action::Insert => self.confirm_clock(),
                Action::GbaDown(Btn::B) if *from_menu => {
                    self.phase = Phase::QuickMenu {
                        row: QuickRow::DateTime,
                    }
                }
                _ => {}
            }
            return;
        }
        if self.adjust(action) {
            return;
        }
        match action {
            Action::GbaUp(Btn::Left) => self.shelf_mut().release_left(),
            Action::GbaUp(Btn::Right) => self.shelf_mut().release_right(),
            _ => {}
        }
        if self.game_menu.is_some() {
            return self.game_menu_input(action);
        }
        if self.core_options.is_some() && matches!(self.phase, Phase::Playing { .. }) {
            if action != Action::Eject {
                return self.core_options_input(action);
            }
            self.core_options = None;
        }
        if let (Some(row), Phase::Playing { .. }) = (self.in_game_menu, &self.phase) {
            if action != Action::Eject {
                return self.in_game_menu_input(row, action);
            }
        }
        let now = self.now();
        match self.phase {
            Phase::Shelf => match action {
                Action::GbaDown(Btn::Start) if self.core_picker.is_none() => {
                    self.open_core_picker()
                }
                _ if self.core_picker.is_some() => self.core_picker_input(action),
                Action::GbaDown(Btn::Up) => self.jump_letter(-1),
                Action::GbaDown(Btn::Down) => self.jump_letter(1),
                Action::ShelfLeft | Action::GbaDown(Btn::Left) => self.shelf_mut().hold_left(now),
                Action::ShelfRight | Action::GbaDown(Btn::Right) => {
                    self.shelf_mut().hold_right(now)
                }
                Action::QuickMenu => self.open_quick_menu(),
                Action::GbaDown(Btn::A) => self.play_held = Some(now),
                Action::GbaUp(Btn::A) => {
                    if self.play_held.take().is_some() {
                        self.insert(!self.state.eject_save);
                    }
                }
                Action::Insert => self.insert(!self.state.eject_save),
                Action::GbaDown(Btn::L1) => self.switch_shelf(-1),
                Action::GbaDown(Btn::R1) => self.switch_shelf(1),
                Action::GbaDown(Btn::Y) => self.toggle_favorite(),
                Action::GbaDown(Btn::X) => self.open_search(false),
                _ => {}
            },
            Phase::Inserting { .. } if action == Action::Eject => self.eject(),
            Phase::Playing { .. } => match action {
                Action::GbaDown(Btn::L1) if self.slot_owns_the_shoulders() => {
                    self.set_picture(VideoMode::Stretch)
                }
                Action::GbaDown(Btn::R1) if self.slot_owns_the_shoulders() => {
                    self.set_picture(VideoMode::Actual)
                }
                Action::Eject => self.eject(),
                Action::GameMenu => self.game_menu_shortcut(),
                Action::CoreOptions => self.core_options_wanted = true,
                Action::QuickMenu => self.tap_in_game_menu(Some(QuickRow::PLAY[0])),
                Action::Polaroids => self.double_tap_states(),
                Action::SaveState => self.save_state(),
                Action::LoadState => self.load_newest(),
                Action::RewindStart if !self.may_rewind() => self.refuse(),
                Action::FfStart if !self.may_fast_forward() => self.refuse(),
                _ => {}
            },
            Phase::Polaroids { .. } => match action {
                Action::ShelfLeft | Action::GbaDown(Btn::Left) => self.flick(Polaroids::left),
                Action::ShelfRight | Action::GbaDown(Btn::Right) => self.flick(Polaroids::right),
                Action::GbaDown(Btn::A) => self.load_selected(),
                Action::GbaDown(Btn::B) | Action::Polaroids => self.close_polaroids(),
                Action::GbaDown(Btn::X) => self.undo(self.now()),
                Action::GbaDown(Btn::Y) => self.delete_selected(),
                _ => {}
            },
            Phase::About if action == Action::GbaDown(Btn::B) || action == Action::QuickMenu => {
                self.phase = Phase::QuickMenu {
                    row: QuickRow::About,
                }
            }
            Phase::About if action == Action::GbaDown(Btn::X) => self.begin_restart(),
            Phase::QuickMenu { row } => self.quick_menu_input(row, action),
            Phase::Networks { cursor, .. } => self.networks_input(cursor, action),
            Phase::Devices { cursor } => self.devices_input(cursor, action),
            Phase::Services { cursor, .. } => self.services_input(cursor, action),
            Phase::TailscaleLogin => self.tailscale_login_input(action),
            Phase::SyncthingId => self.syncthing_id_input(action),
            Phase::Updates { cursor } => self.updates_input(cursor, action),
            Phase::CoreDownloads { cursor } => self.core_downloads_input(cursor, action),
            Phase::GameArt { cursor, .. } => self.game_art_input(cursor, action),
            Phase::Cores { cursor } => self.cores_input(cursor, action),
            Phase::Search { cursor } => self.search_input(cursor, action),
            Phase::Keyboard { .. } => self.keyboard_input(action),
            _ => {}
        }
    }

    fn wanted(&self) -> Wanted {
        Wanted {
            ssh: self.state.ssh,
            samba: self.state.samba,
            syncthing: self.state.syncthing,
            tailscale: self.state.tailscale,
            vpn: self.state.vpn.clone(),
            password: self.state.password.clone(),
        }
    }

    /// Saves the services' settings and has them put into effect.
    fn services_changed(&mut self) {
        self.persist();
        if let Some(services) = &self.services {
            services.want(self.wanted());
        }
    }

    pub fn services_view(&self) -> &ServicesView {
        &self.services_view
    }

    fn tunnels(&self) -> Vec<String> {
        self.root
            .as_deref()
            .map(crate::wireguard::tunnels)
            .unwrap_or_default()
    }

    fn services_input(&mut self, cursor: usize, action: Action) {
        let n = ServiceRow::ALL.len();
        let cursor = cursor.min(n - 1);
        let (cursor, note) = match action {
            Action::GbaDown(Btn::Up) => ((cursor + n - 1) % n, None),
            Action::GbaDown(Btn::Down) => ((cursor + 1) % n, None),
            Action::GbaDown(Btn::A) => match self.change_service(ServiceRow::ALL[cursor], cursor) {
                Some(note) => (cursor, note),
                None => return,
            },
            Action::GbaDown(Btn::X)
                if ServiceRow::ALL[cursor] == ServiceRow::Syncthing
                    && self.services_view.syncthing_id.is_some() =>
            {
                self.phase = Phase::SyncthingId;
                return;
            }
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                self.phase = Phase::QuickMenu {
                    row: QuickRow::Services,
                };
                return;
            }
            _ => return,
        };
        self.phase = Phase::Services { cursor, note };
    }

    /// Changes what `row` is for. None when it has gone to another page, else the note to
    /// show, if any.
    fn change_service(&mut self, row: ServiceRow, cursor: usize) -> Option<Option<String>> {
        let password = |then: Option<ServiceRow>| Phase::Keyboard {
            keys: Keyboard::new("New password for root", "", true, 4, 63),
            typing: Typing::RootPassword { then },
            cursor,
        };
        match row {
            ServiceRow::Ssh => self.state.ssh = !self.state.ssh,
            ServiceRow::Sharing if !self.state.samba && self.state.password.is_none() => {
                self.phase = password(Some(row));
                return None;
            }
            ServiceRow::Sharing => self.state.samba = !self.state.samba,
            ServiceRow::Syncthing if !self.state.syncthing && self.state.password.is_none() => {
                self.phase = password(Some(row));
                return None;
            }
            ServiceRow::Syncthing => self.state.syncthing = !self.state.syncthing,
            ServiceRow::Tailscale => match (self.state.tailscale, &self.services_view.tailnet) {
                (true, Tailnet::SignIn(_)) => {
                    self.phase = Phase::TailscaleLogin;
                    return None;
                }
                (on, _) => self.state.tailscale = !on,
            },
            ServiceRow::Vpn => {
                let tunnels = self.tunnels();
                if tunnels.is_empty() {
                    return Some(Some(format!(
                        "Put WireGuard configs in {} on the card",
                        crate::wireguard::WG_DIR
                    )));
                }
                // Off, then each tunnel in turn, then off again.
                let at = self
                    .state
                    .vpn
                    .as_ref()
                    .and_then(|v| tunnels.iter().position(|t| t == v));
                self.state.vpn = match at {
                    None => tunnels.first().cloned(),
                    Some(i) => tunnels.get(i + 1).cloned(),
                };
            }
            ServiceRow::Password => {
                self.phase = password(None);
                return None;
            }
        }
        self.services_changed();
        Some(None)
    }

    fn services_page(&self, cursor: usize, note: Option<&str>) -> ListPage {
        let v = &self.services_view;
        let on = |b: bool| if b { "On" } else { "Off" }.to_string();
        let rows: Vec<ListRow> = ServiceRow::ALL
            .iter()
            .map(|row| {
                let (label, detail) = match row {
                    ServiceRow::Ssh => ("SSH", on(self.state.ssh)),
                    ServiceRow::Sharing => ("File sharing", on(self.state.samba)),
                    ServiceRow::Syncthing => ("Syncthing", on(self.state.syncthing)),
                    ServiceRow::Tailscale => (
                        "Tailscale",
                        match (&v.tailnet, self.state.tailscale) {
                            (_, false) | (Tailnet::Off, _) => "Off".into(),
                            (Tailnet::Starting, _) => "Starting".into(),
                            (Tailnet::SignIn(_), _) => "Sign in".into(),
                            (Tailnet::Approval, _) => "Needs approval".into(),
                            (Tailnet::Running(ip), _) => ip.clone().unwrap_or_else(|| "On".into()),
                        },
                    ),
                    ServiceRow::Vpn => (
                        "VPN",
                        match (&self.state.vpn, &v.vpn) {
                            (None, _) => "Off".into(),
                            (Some(name), Some(up)) if name == up => name.clone(),
                            (Some(_), _) if v.busy.is_some() => "Connecting".into(),
                            (Some(_), _) => "Not connected".into(),
                        },
                    ),
                    ServiceRow::Password => (
                        "Password",
                        match self.state.password {
                            Some(_) => "Set".into(),
                            None => "BaseOS's".into(),
                        },
                    ),
                };
                ListRow {
                    label: label.into(),
                    detail,
                    bars: None,
                    icon: None,
                }
            })
            .collect();
        let row = ServiceRow::ALL[cursor.min(ServiceRow::ALL.len() - 1)];
        let address = match &self.wifi_view.ip {
            Some(ip) if row == ServiceRow::Syncthing && v.syncthing_id.is_some() => {
                format!("http://{ip}:8384   user root")
            }
            Some(ip) => format!("root@{}.local   {ip}", crate::services::hostname()),
            None => "Connect to Wi-Fi to reach these".into(),
        };
        let status = match (note, &v.busy, &v.error) {
            (Some(note), ..) if !note.is_empty() => note.to_string(),
            (_, Some(busy), _) => busy.clone(),
            (_, None, Some(error)) => error.clone(),
            _ => address,
        };
        let turn = |on: bool| if on { "Turn Off" } else { "Turn On" };
        let action = match row {
            ServiceRow::Ssh => turn(self.state.ssh),
            ServiceRow::Sharing => turn(self.state.samba),
            ServiceRow::Syncthing => turn(self.state.syncthing),
            ServiceRow::Tailscale => match &v.tailnet {
                Tailnet::SignIn(_) if self.state.tailscale => "Sign In",
                _ => turn(self.state.tailscale),
            },
            ServiceRow::Vpn => match self.state.vpn {
                Some(_) => "Change Tunnel",
                None => "Turn On",
            },
            ServiceRow::Password => match self.state.password {
                Some(_) => "Change Password",
                None => "Set Password",
            },
        };
        let mut legends = with_action(Some(action));
        if row == ServiceRow::Syncthing && v.syncthing_id.is_some() {
            legends.insert(1, Legend::new(ButtonLegend::X, "Show ID"));
        }
        ListPage {
            title: "Services".into(),
            status,
            cursor: cursor.min(rows.len() - 1),
            rows,
            legends,
        }
    }

    /// The systems with games on the card, in shelf order.
    fn art_platforms(&self) -> Vec<Platform> {
        self.shelves
            .iter()
            .filter_map(|(rack, shelf)| match rack {
                Rack::Of(p) if !shelf.carts.is_empty() => Some(*p),
                _ => None,
            })
            .collect()
    }

    /// A system's games, or every system's for None, and which of them have a label: one found
    /// at boot or one made since.
    fn art_carts(&self, platform: Option<Platform>) -> Vec<(Cart, bool)> {
        let made = &self.scrape_view.made;
        self.shelves
            .iter()
            .filter(|(rack, _)| match (rack, platform) {
                (Rack::Of(p), Some(want)) => *p == want,
                (Rack::Of(_), None) => true,
                _ => false,
            })
            .flat_map(|(_, shelf)| shelf.carts.iter())
            .map(|c| {
                let has = c.label.is_some()
                    || made
                        .iter()
                        .any(|(p, stem)| *p == c.platform && *stem == c.stem);
                (c.clone(), has)
            })
            .collect()
    }

    /// Saves a ScreenScraper login typed in on the device; a note if it could not be.
    fn save_ss_login(&mut self, user: &str, password: &str) -> Option<String> {
        let root = self.root.clone()?;
        let saved = crate::scrape::save_ss_login(&root, user, password);
        self.ss_state = crate::scrape::ss_state(&root);
        saved
            .err()
            .map(|e| format!("Could not save the login: {e}"))
    }

    fn game_art_input(&mut self, cursor: usize, action: Action) {
        let platforms = self.art_platforms();
        // All systems, each system, then the ScreenScraper sign in.
        let n = platforms.len() + 2;
        let cursor = cursor.min(n - 1);
        let (cursor, note) = match action {
            Action::GbaDown(Btn::Up) => ((cursor + n - 1) % n, None),
            Action::GbaDown(Btn::Down) => ((cursor + 1) % n, None),
            Action::GbaDown(Btn::A) if cursor == n - 1 => {
                let user = match &self.ss_state {
                    SsState::NoDevId(u) | SsState::On(Some(u)) => u.clone(),
                    _ => String::new(),
                };
                self.phase = Phase::Keyboard {
                    keys: Keyboard::new(
                        "ScreenScraper username, or none to sign out",
                        &user,
                        false,
                        0,
                        64,
                    ),
                    typing: Typing::SsUser,
                    cursor,
                };
                return;
            }
            Action::GbaDown(Btn::A) if !self.scrape_view.running => {
                let which = cursor.checked_sub(1).map(|i| platforms[i]);
                let missing: Vec<Cart> = self
                    .art_carts(which)
                    .into_iter()
                    .filter(|(_, has)| !has)
                    .map(|(c, _)| c)
                    .collect();
                if missing.is_empty() {
                    (cursor, Some("Every game here has a label".to_string()))
                } else {
                    if let Some(scraper) = &self.scraper {
                        scraper.fetch(missing);
                    }
                    (cursor, None)
                }
            }
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                self.phase = Phase::QuickMenu {
                    row: QuickRow::GameArt,
                };
                return;
            }
            _ => return,
        };
        self.phase = Phase::GameArt { cursor, note };
    }

    pub fn scrape_view(&self) -> &ScrapeView {
        &self.scrape_view
    }

    fn game_art_page(&self, cursor: usize, note: Option<&str>) -> ListPage {
        let v = &self.scrape_view;
        let count = |platform: Option<Platform>| {
            let carts = self.art_carts(platform);
            let has = carts.iter().filter(|(_, has)| *has).count();
            format!("{has} of {}", carts.len())
        };
        let platforms = self.art_platforms();
        let missing =
            |platform: Option<Platform>| self.art_carts(platform).iter().any(|(_, has)| !has);
        let rows: Vec<ListRow> = std::iter::once(ListRow {
            label: "All systems".into(),
            detail: count(None),
            bars: None,
            icon: None,
        })
        .chain(platforms.iter().map(|&p| ListRow {
            label: p.name().into(),
            detail: count(Some(p)),
            bars: None,
            icon: None,
        }))
        .chain(std::iter::once(ListRow {
            label: "ScreenScraper".into(),
            detail: match &self.ss_state {
                SsState::Off | SsState::On(None) => "Sign in".into(),
                SsState::NoDevId(user) | SsState::On(Some(user)) => user.clone(),
            },
            bars: None,
            icon: None,
        }))
        .collect();
        let on_ss = cursor >= rows.len() - 1;
        let made = v.made.len();
        let status = match () {
            _ if v.running => format!(
                "Finding art for {} ({} of {})",
                v.current.as_deref().unwrap_or("..."),
                (v.done + 1).min(v.total),
                v.total
            ),
            _ if note.is_some() => note.unwrap_or_default().to_string(),
            _ if v.error.is_some() => v.error.clone().unwrap_or_default(),
            _ if v.skipped > 0 => format!(
                "Skipped {} game{} ScreenScraper turned away",
                v.skipped,
                if v.skipped == 1 { "" } else { "s" }
            ),
            _ if made > 0 => format!(
                "Added {made} label{} to the shelves",
                if made == 1 { "" } else { "s" }
            ),
            _ if v.missed > 0 => format!(
                "No art found for {} game{}",
                v.missed,
                if v.missed == 1 { "" } else { "s" }
            ),
            _ => match &self.ss_state {
                SsState::On(_) => "Art from ScreenScraper and libretro".into(),
                SsState::NoDevId(_) => {
                    "Signed in, but ScreenScraper needs unslot's developer ID".into()
                }
                SsState::Off => "Art from libretro-thumbnails".into(),
            },
        };
        let action = match () {
            _ if on_ss => Some(match self.ss_state {
                SsState::Off | SsState::On(None) => "Sign In",
                SsState::NoDevId(_) | SsState::On(Some(_)) => "Change Account",
            }),
            _ if v.running => None,
            _ => missing(
                cursor
                    .checked_sub(1)
                    .and_then(|i| platforms.get(i).copied()),
            )
            .then_some("Find Missing Art"),
        };
        ListPage {
            title: "Game Art".into(),
            status,
            cursor: cursor.min(rows.len() - 1),
            rows,
            legends: with_action(action),
        }
    }

    fn with_updates(&self, f: impl FnOnce(&Updates)) {
        if let Some(updates) = &self.updates {
            f(updates);
        }
    }

    pub fn updates_view(&self) -> &UpdatesView {
        &self.updates_view
    }

    fn updates_input(&mut self, cursor: usize, action: Action) {
        let parts = self.updates_view.parts();
        let n = parts.len();
        let cursor = cursor.min(n - 1);
        let cursor = match action {
            Action::GbaDown(Btn::Up) => (cursor + n - 1) % n,
            Action::GbaDown(Btn::Down) => (cursor + 1) % n,
            Action::GbaDown(Btn::Y) => {
                self.with_updates(Updates::check);
                cursor
            }
            Action::GbaDown(Btn::A) => {
                let part = parts[cursor];
                match self.updates_view.track(part).map(|t| &t.stage) {
                    Some(Stage::Available(_)) => self.with_updates(|u| u.download(part)),
                    // BaseOS puts what was downloaded in as the device starts again.
                    Some(Stage::Ready(_)) => self.begin_restart(),
                    Some(Stage::Checking | Stage::Downloading(..)) => {}
                    _ => self.with_updates(Updates::check),
                }
                cursor
            }
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                self.phase = Phase::QuickMenu {
                    row: QuickRow::Updates,
                };
                return;
            }
            _ => return,
        };
        self.phase = Phase::Updates { cursor };
    }

    fn updates_page(&self, cursor: usize) -> ListPage {
        let v = &self.updates_view;
        let detail = |t: &Track| match &t.stage {
            Stage::Unchecked => t.current.clone().unwrap_or_else(|| "Unknown".into()),
            Stage::Checking => "Checking".into(),
            Stage::UpToDate => "Up to date".into(),
            Stage::Available(version) => format!("{version} available"),
            Stage::Downloading(_, pct) => format!("{pct}%"),
            Stage::Ready(_) => "Ready to install".into(),
        };
        let parts = v.parts();
        let rows: Vec<ListRow> = parts
            .iter()
            .filter_map(|p| v.track(*p).map(|t| (p, t)))
            .map(|(p, t)| ListRow {
                label: p.name().into(),
                detail: detail(t),
                bars: None,
                icon: None,
            })
            .collect();
        let stages: Vec<(Part, &Stage)> = parts
            .iter()
            .filter_map(|p| v.track(*p).map(|t| (*p, &t.stage)))
            .collect();
        let downloading = stages.iter().find_map(|(p, s)| match s {
            Stage::Downloading(version, _) => {
                Some(format!("Downloading {} {version}...", p.name()))
            }
            _ => None,
        });
        let status = match () {
            _ if downloading.is_some() => downloading.unwrap_or_default(),
            _ if v.error.is_some() => v.error.clone().unwrap_or_default(),
            _ if stages.iter().any(|(_, s)| matches!(s, Stage::Ready(_))) => {
                "Installs when the device restarts. Keep the power connected".into()
            }
            _ if stages.iter().any(|(_, s)| **s == Stage::Checking) => {
                "Checking for updates...".into()
            }
            _ => parts
                .iter()
                .filter_map(|p| {
                    let t = v.track(*p)?;
                    Some(format!(
                        "{} {}",
                        p.name(),
                        t.current.as_deref().unwrap_or("(own build)")
                    ))
                })
                .collect::<Vec<_>>()
                .join("   "),
        };
        let picked = parts
            .get(cursor.min(parts.len().saturating_sub(1)))
            .and_then(|p| v.track(*p));
        let mut legends = vec![back_legend()];
        match picked.map(|t| &t.stage) {
            Some(Stage::Checking | Stage::Downloading(..)) => {}
            Some(Stage::Available(version)) => {
                legends.push(Legend::new(ButtonLegend::Y, "Check Again"));
                legends.push(Legend::new(ButtonLegend::A, &format!("Download {version}")));
            }
            Some(Stage::Ready(_)) => {
                legends.push(Legend::new(ButtonLegend::Y, "Check Again"));
                legends.push(Legend::new(ButtonLegend::A, "Restart to Install"));
            }
            _ => legends.push(Legend::new(ButtonLegend::A, "Check for Updates")),
        }
        ListPage {
            title: "Updates".into(),
            status,
            cursor: cursor.min(rows.len().saturating_sub(1)),
            rows,
            legends,
        }
    }

    fn with_cores(&self, f: impl FnOnce(&CoreDownloads)) {
        if let Some(cores) = &self.core_downloads {
            f(cores);
        }
    }

    fn core_downloads_input(&mut self, cursor: usize, action: Action) {
        let cores = downloadable_cores();
        let n = cores.len();
        let cursor = cursor.min(n - 1);
        let core = cores[cursor];
        let v = &self.cores_view;
        let (pending, installed) = (v.pending(core), v.installed(core));
        let wanted = !pending && (!installed || v.outdated(core));
        let cursor = match action {
            Action::GbaDown(Btn::Up) => (cursor + n - 1) % n,
            Action::GbaDown(Btn::Down) => (cursor + 1) % n,
            Action::GbaDown(Btn::A) if wanted => {
                self.with_cores(|c| c.download(core));
                cursor
            }
            Action::GbaDown(Btn::X) if installed && !pending => {
                self.with_cores(|c| c.remove(core));
                cursor
            }
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                self.phase = Phase::QuickMenu {
                    row: QuickRow::Cores,
                };
                return;
            }
            _ => return,
        };
        self.phase = Phase::CoreDownloads { cursor };
    }

    fn core_downloads_page(&self, cursor: usize) -> ListPage {
        let v = &self.cores_view;
        let cores = downloadable_cores();
        let cursor = cursor.min(cores.len() - 1);
        let rows = cores
            .iter()
            .map(|&c| ListRow {
                label: c.text().into(),
                detail: core_state(v, c).unwrap_or_else(|| {
                    Platform::ALL
                        .into_iter()
                        .filter(|p| p.spec().cores.contains(&c))
                        .map(Platform::dir_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                }),
                bars: None,
                icon: None,
            })
            .collect();
        let status = if let Some((c, _)) = v.downloading {
            format!("Downloading {}...", c.text())
        } else if let Some(e) = &v.error {
            e.clone()
        } else if v.checking {
            "Checking for cores...".into()
        } else {
            let have = cores.iter().filter(|c| v.installed(**c)).count();
            format!("{have} of {} downloaded", cores.len())
        };
        let core = cores[cursor];
        let mut legends = vec![back_legend()];
        if v.installed(core) && !v.pending(core) {
            legends.push(Legend::new(ButtonLegend::X, "Remove"));
        }
        if !v.pending(core) && !v.installed(core) {
            legends.push(Legend::new(ButtonLegend::A, "Download"));
        } else if !v.pending(core) && v.outdated(core) {
            legends.push(Legend::new(ButtonLegend::A, "Update"));
        }
        ListPage {
            title: "Cores".into(),
            status,
            cursor,
            rows,
            legends,
        }
    }

    fn tailscale_login_input(&mut self, action: Action) {
        let back = Phase::Services {
            cursor: ServiceRow::Tailscale as usize,
            note: None,
        };
        match action {
            Action::GbaDown(Btn::X) => {
                self.state.tailscale = false;
                self.services_changed();
                self.phase = back;
            }
            Action::GbaDown(Btn::B) | Action::QuickMenu => self.phase = back,
            _ => {}
        }
    }

    fn syncthing_id_input(&mut self, action: Action) {
        if let Action::GbaDown(Btn::B) | Action::QuickMenu = action {
            self.phase = Phase::Services {
                cursor: ServiceRow::Syncthing as usize,
                note: None,
            };
        }
    }

    fn with_bluetooth(&self, f: impl FnOnce(&Bluetooth)) {
        if let Some(bluetooth) = &self.bluetooth {
            f(bluetooth);
        }
    }

    pub fn bt_view(&self) -> &BtView {
        &self.bt_view
    }

    fn devices_input(&mut self, cursor: usize, action: Action) {
        let devices = self.bt_view.devices.clone();
        let n = devices.len().max(1);
        let cursor = cursor.min(n - 1);
        let picked = devices.get(cursor);
        let cursor = match action {
            Action::GbaDown(Btn::Up) => (cursor + n - 1) % n,
            Action::GbaDown(Btn::Down) => (cursor + 1) % n,
            Action::GbaDown(Btn::Y) => {
                self.with_bluetooth(Bluetooth::search);
                cursor
            }
            Action::GbaDown(Btn::X) => {
                if let Some(d) = picked.filter(|d| d.paired) {
                    self.with_bluetooth(|b| b.forget(&d.mac));
                }
                cursor
            }
            Action::GbaDown(Btn::A) => {
                match picked {
                    Some(d) if d.connected => self.with_bluetooth(|b| b.disconnect(&d.mac)),
                    Some(d) if d.paired => self.with_bluetooth(|b| b.connect(&d.mac)),
                    Some(d) => self.with_bluetooth(|b| b.pair(&d.mac)),
                    None => self.with_bluetooth(Bluetooth::search),
                }
                cursor
            }
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                self.phase = Phase::QuickMenu {
                    row: QuickRow::BtDevices,
                };
                return;
            }
            _ => return,
        };
        self.phase = Phase::Devices { cursor };
    }

    fn devices_page(&self, cursor: usize) -> ListPage {
        let v = &self.bt_view;
        let rows: Vec<ListRow> = v
            .devices
            .iter()
            .map(|d| ListRow {
                label: d.name.clone(),
                detail: match (d.connected, d.paired) {
                    (true, _) => "Connected".into(),
                    (false, true) => "Paired".into(),
                    (false, false) => match d.kind {
                        crate::bluetooth::Kind::Controller => "Controller".into(),
                        crate::bluetooth::Kind::Audio => "Headphones".into(),
                        crate::bluetooth::Kind::Other => String::new(),
                    },
                },
                bars: None,
                icon: None,
            })
            .collect();
        let status = match (&v.busy, &v.error) {
            (BtBusy::Starting, _) => "Turning on Bluetooth...".to_string(),
            (BtBusy::Searching, _) => "Searching for devices...".to_string(),
            (BtBusy::Pairing(name), _) => format!("Pairing {name}..."),
            (BtBusy::Connecting(name), _) => format!("Connecting {name}..."),
            (BtBusy::Idle, Some(error)) => error.clone(),
            (BtBusy::Idle, None) if rows.is_empty() => {
                "Put a device in pairing mode, then press Y to search".to_string()
            }
            (BtBusy::Idle, None) => match v.devices.iter().filter(|d| d.connected).count() {
                0 => "Nothing connected".to_string(),
                1 => "1 device connected".to_string(),
                n => format!("{n} devices connected"),
            },
        };
        let picked = v.devices.get(cursor.min(v.devices.len().saturating_sub(1)));
        let mut legends = vec![back_legend(), Legend::new(ButtonLegend::Y, "Search")];
        if picked.is_some_and(|d| d.paired) {
            legends.push(Legend::new(ButtonLegend::X, "Forget"));
        }
        legends.push(Legend::new(
            ButtonLegend::A,
            match picked {
                Some(d) if d.connected => "Disconnect",
                Some(d) if d.paired => "Connect",
                Some(_) => "Pair",
                None => "Search",
            },
        ));
        ListPage {
            title: "Bluetooth".into(),
            status,
            cursor: cursor.min(rows.len().saturating_sub(1)),
            rows,
            legends,
        }
    }

    fn with_wifi(&self, f: impl FnOnce(&Wifi)) {
        if let Some(wifi) = &self.wifi {
            f(wifi);
        }
    }

    /// Steps the radio aside for a link session, which needs wlan0 to itself.
    fn warm_radio(&mut self) {
        if self.state.wifi {
            self.with_wifi(Wifi::turn_off);
        }
        self.radio.ask(RadioJob::Warm);
    }

    /// Lets the link radio go and brings the Wi-Fi back if it was on.
    fn cool_radio(&mut self) {
        self.radio.ask(RadioJob::Cool);
        if self.state.wifi {
            self.with_wifi(Wifi::turn_on);
        }
    }

    pub fn wifi_view(&self) -> &WifiView {
        &self.wifi_view
    }

    fn network_rows(&self) -> Vec<NetRow> {
        self.wifi_view
            .networks
            .iter()
            .cloned()
            .map(NetRow::Network)
            .chain([NetRow::Other])
            .collect()
    }

    fn networks_input(&mut self, cursor: usize, action: Action) {
        let rows = self.network_rows();
        let n = rows.len();
        let cursor = cursor.min(n - 1);
        let (cursor, note) = match action {
            Action::GbaDown(Btn::Up) => ((cursor + n - 1) % n, None),
            Action::GbaDown(Btn::Down) => ((cursor + 1) % n, None),
            Action::GbaDown(Btn::Y) => {
                self.with_wifi(Wifi::scan);
                (cursor, None)
            }
            Action::GbaDown(Btn::X) => {
                if let Some(NetRow::Network(net)) = rows.get(cursor) {
                    if net.saved {
                        self.with_wifi(|w| w.forget(&net.ssid));
                    }
                }
                (cursor, None)
            }
            Action::GbaDown(Btn::A) => match rows.get(cursor) {
                Some(NetRow::Other) => {
                    self.phase = Phase::Keyboard {
                        keys: Keyboard::new("Network name", "", false, 1, 32),
                        typing: Typing::HiddenName,
                        cursor,
                    };
                    return;
                }
                Some(NetRow::Network(net)) => match self.pick_network(net, cursor) {
                    Some(note) => (cursor, Some(note)),
                    None => return,
                },
                None => (cursor, None),
            },
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                self.phase = Phase::QuickMenu {
                    row: QuickRow::WifiNetworks,
                };
                return;
            }
            _ => return,
        };
        self.phase = Phase::Networks { cursor, note };
    }

    /// Joins `net`, or opens the keyboard for its password. Some note when it cannot be joined.
    fn pick_network(&mut self, net: &Network, cursor: usize) -> Option<String> {
        if self.wifi_view.connected.as_deref() == Some(net.ssid.as_str()) {
            return Some(format!("Already connected to {}", net.ssid));
        }
        match (net.security, net.saved) {
            (Security::Unsupported, _) => {
                Some("Enterprise and WEP networks are not supported".into())
            }
            (Security::Open, _) | (_, true) => {
                self.with_wifi(|w| w.join(&net.ssid, None));
                Some(String::new())
            }
            (Security::Personal, false) => {
                self.phase = Phase::Keyboard {
                    keys: Keyboard::new(&format!("Password for {}", net.ssid), "", true, 8, 63),
                    typing: Typing::Password(net.ssid.clone()),
                    cursor,
                };
                None
            }
        }
    }

    fn keyboard_input(&mut self, action: Action) {
        let input = match action {
            Action::GbaDown(Btn::Up) => KeyInput::Up,
            Action::GbaDown(Btn::Down) => KeyInput::Down,
            Action::GbaDown(Btn::Left) => KeyInput::Left,
            Action::GbaDown(Btn::Right) => KeyInput::Right,
            Action::GbaDown(Btn::A) => KeyInput::Pick,
            Action::GbaDown(Btn::B) => KeyInput::Delete,
            Action::GbaDown(Btn::Y) => KeyInput::Space,
            Action::GbaDown(Btn::X) => KeyInput::Shift,
            Action::GbaDown(Btn::L1 | Btn::R1) => KeyInput::Page,
            Action::GbaDown(Btn::Start) => KeyInput::Done,
            Action::GbaDown(Btn::Select) => KeyInput::Reveal,
            Action::QuickMenu => KeyInput::Cancel,
            _ => return,
        };
        let Phase::Keyboard {
            keys,
            typing,
            cursor,
        } = &mut self.phase
        else {
            return;
        };
        let (typing, cursor) = (typing.clone(), *cursor);
        match keys.press(input) {
            KeyOutcome::Typing => {}
            KeyOutcome::Cancelled => {
                self.phase = match typing {
                    Typing::Search { again: true } => Phase::Search { cursor },
                    Typing::Search { again: false } => Phase::Shelf,
                    Typing::RootPassword { .. } => Phase::Services { cursor, note: None },
                    Typing::SsUser | Typing::SsPassword(_) => Phase::GameArt { cursor, note: None },
                    Typing::RaUser | Typing::RaPassword(_) => Phase::QuickMenu {
                        row: QuickRow::Achievements,
                    },
                    _ => Phase::Networks { cursor, note: None },
                };
            }
            KeyOutcome::Done(text) => match typing {
                Typing::Search { .. } => self.search_for(&text),
                Typing::SsUser if text.is_empty() => {
                    let note = self.save_ss_login("", "");
                    self.phase = Phase::GameArt { cursor, note };
                }
                Typing::SsUser => {
                    self.phase = Phase::Keyboard {
                        keys: Keyboard::new(
                            &format!("ScreenScraper password for {text}"),
                            "",
                            true,
                            1,
                            64,
                        ),
                        typing: Typing::SsPassword(text),
                        cursor,
                    };
                }
                Typing::SsPassword(user) => {
                    let note = self.save_ss_login(&user, &text);
                    self.phase = Phase::GameArt { cursor, note };
                }
                Typing::RaUser if text.is_empty() => {
                    let line = match self.root.as_deref().map(crate::cheevos::sign_out) {
                        Some(Ok(())) => "Signed out of RetroAchievements".to_string(),
                        Some(Err(e)) => format!("Could not sign out: {e}"),
                        None => "No card to sign out on".to_string(),
                    };
                    self.cheevo_line(line);
                    self.phase = Phase::QuickMenu {
                        row: QuickRow::Achievements,
                    };
                }
                Typing::RaUser => {
                    self.phase = Phase::Keyboard {
                        keys: Keyboard::new(
                            &format!("RetroAchievements password for {text}"),
                            "",
                            true,
                            1,
                            64,
                        ),
                        typing: Typing::RaPassword(text),
                        cursor,
                    };
                }
                Typing::RaPassword(user) => {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let spawned = std::thread::Builder::new()
                        .name("slot-ra-sign-in".into())
                        .spawn(move || {
                            let _ = tx.send(crate::cheevos::sign_in(&user, &text));
                        });
                    if spawned.is_ok() {
                        self.ra_sign_in = Some(rx);
                        self.cheevo_line("Signing in to RetroAchievements...".into());
                    }
                    self.phase = Phase::QuickMenu {
                        row: QuickRow::Achievements,
                    };
                }
                Typing::RootPassword { then } => {
                    self.state.password = Some(text);
                    match then {
                        Some(ServiceRow::Sharing) => self.state.samba = true,
                        Some(ServiceRow::Syncthing) => self.state.syncthing = true,
                        _ => {}
                    }
                    self.services_changed();
                    self.phase = Phase::Services { cursor, note: None };
                }
                Typing::Password(ssid) => {
                    self.with_wifi(|w| w.join(&ssid, Some(&text)));
                    self.phase = Phase::Networks { cursor, note: None };
                }
                Typing::HiddenName => {
                    self.phase = Phase::Keyboard {
                        keys: Keyboard::new(
                            &format!("Password for {text}, or none if it is open"),
                            "",
                            true,
                            0,
                            63,
                        ),
                        typing: Typing::HiddenPassword(text),
                        cursor,
                    };
                }
                Typing::HiddenPassword(ssid) => {
                    let psk = Some(text.as_str()).filter(|p| !p.is_empty());
                    self.with_wifi(|w| w.join(&ssid, psk));
                    self.phase = Phase::Networks { cursor, note: None };
                }
            },
        }
    }

    /// The page the frontend should draw for the phase, if it is one slot lays out whole.
    pub fn page_model(&self) -> Option<PageModel> {
        if let (Some((options, cursor)), Phase::Playing { .. }) = (&self.core_options, &self.phase)
        {
            return Some(PageModel::List(self.core_options_page(options, *cursor)));
        }
        match &self.phase {
            Phase::Networks { cursor, note } => Some(PageModel::List(
                self.networks_page(*cursor, note.as_deref()),
            )),
            Phase::Keyboard { keys, .. } => Some(PageModel::Keys(keys.clone())),
            Phase::Devices { cursor } => Some(PageModel::List(self.devices_page(*cursor))),
            Phase::Services { cursor, note } => Some(PageModel::List(
                self.services_page(*cursor, note.as_deref()),
            )),
            Phase::Updates { cursor } => Some(PageModel::List(self.updates_page(*cursor))),
            Phase::CoreDownloads { cursor } => {
                Some(PageModel::List(self.core_downloads_page(*cursor)))
            }
            Phase::Cores { cursor } => self.cores_page(*cursor).map(PageModel::List),
            Phase::Search { cursor } => Some(PageModel::List(self.search_page(*cursor))),
            Phase::GameArt { cursor, note } => Some(PageModel::List(
                self.game_art_page(*cursor, note.as_deref()),
            )),
            Phase::TailscaleLogin => match &self.services_view.tailnet {
                Tailnet::SignIn(url) => Some(PageModel::Qr(QrPage {
                    title: "Tailscale".into(),
                    status: "Scan to sign in, or open the link on another device".into(),
                    url: url.clone(),
                    legends: vec![back_legend(), Legend::new(ButtonLegend::X, "Turn Off")],
                })),
                _ => None,
            },
            Phase::SyncthingId => self.services_view.syncthing_id.as_ref().map(|id| {
                PageModel::Qr(QrPage {
                    title: "Syncthing".into(),
                    status: "Scan in the Syncthing app to add this device".into(),
                    url: id.clone(),
                    legends: vec![back_legend()],
                })
            }),
            _ => None,
        }
    }

    pub fn set_page_face(&mut self, tex: TexId, w: u32, h: u32) {
        self.page_face = Some((tex, w, h));
    }

    fn networks_page(&self, cursor: usize, note: Option<&str>) -> ListPage {
        let v = &self.wifi_view;
        let nets = self.network_rows();
        let mut legends = vec![back_legend(), Legend::new(ButtonLegend::Y, "Search")];
        match nets.get(cursor.min(nets.len().saturating_sub(1))) {
            Some(NetRow::Other) => legends.push(Legend::new(ButtonLegend::A, "Join Other")),
            Some(NetRow::Network(net)) => {
                if net.saved {
                    legends.push(Legend::new(ButtonLegend::X, "Forget"));
                }
                let here = v.connected.as_deref() == Some(net.ssid.as_str());
                if !here && net.security != Security::Unsupported {
                    legends.push(Legend::new(ButtonLegend::A, "Join"));
                }
            }
            None => {}
        }
        let rows: Vec<ListRow> = nets
            .into_iter()
            .map(|row| match row {
                NetRow::Other => ListRow {
                    label: "Other network...".into(),
                    ..ListRow::default()
                },
                NetRow::Network(net) => ListRow {
                    detail: match () {
                        _ if v.connected.as_deref() == Some(net.ssid.as_str()) => {
                            "Connected".into()
                        }
                        _ if net.saved => "Saved".into(),
                        _ if net.security == Security::Unsupported => "Not supported".into(),
                        _ if net.security == Security::Open => "Open".into(),
                        _ => String::new(),
                    },
                    bars: Some(net.bars()),
                    label: net.ssid,
                    icon: None,
                },
            })
            .collect();
        let status = match (&v.busy, note.filter(|n| !n.is_empty()), &v.error) {
            (Busy::Starting, ..) => "Turning on Wi-Fi...".to_string(),
            (Busy::Joining(ssid), ..) => format!("Joining {ssid}..."),
            (_, Some(note), _) => note.to_string(),
            (_, None, Some(error)) => error.clone(),
            (Busy::Scanning, ..) => "Searching for networks...".to_string(),
            (Busy::Idle, None, None) => match (&v.connected, &v.ip) {
                (Some(ssid), Some(ip)) => format!("Connected to {ssid}, {ip}"),
                (Some(ssid), None) => format!("Connected to {ssid}"),
                (None, _) => "Not connected".to_string(),
            },
        };
        ListPage {
            title: "Wi-Fi".into(),
            status,
            cursor: cursor.min(rows.len().saturating_sub(1)),
            rows,
            legends,
        }
    }

    fn open_quick_menu(&mut self) {
        self.phase = Phase::QuickMenu {
            row: QuickRow::MAIN[0],
        };
    }

    fn quick_menu_input(&mut self, row: QuickRow, action: Action) {
        let row = match action {
            Action::GbaDown(Btn::Up) => row.up(&self.hidden_rows()),
            Action::GbaDown(Btn::Down) => row.down(&self.hidden_rows()),
            Action::GbaDown(Btn::Left) => return self.change_setting(row, false),
            Action::GbaDown(Btn::Right) => return self.change_setting(row, true),
            Action::GbaDown(Btn::A) => return self.open_quick_row(row),
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                self.phase = match row.parent() {
                    Some(row) => Phase::QuickMenu { row },
                    None => Phase::Shelf,
                };
                return;
            }
            _ => return,
        };
        self.phase = Phase::QuickMenu { row };
    }

    fn in_game_menu_input(&mut self, row: QuickRow, action: Action) {
        let hidden = self.in_game_hidden();
        self.in_game_menu = match action {
            Action::GbaDown(Btn::Up) => Some(row.up(&hidden)),
            Action::GbaDown(Btn::Down) => Some(row.down(&hidden)),
            Action::GbaDown(Btn::Left) => return self.change_setting(row, false),
            Action::GbaDown(Btn::Right) => return self.change_setting(row, true),
            Action::GbaDown(Btn::A) => return self.open_in_game_row(row),
            Action::GbaDown(Btn::B) => row.parent_in_game(),
            Action::QuickMenu => return self.tap_in_game_menu(None),
            Action::Polaroids => return self.double_tap_states(),
            _ => return,
        };
    }

    fn open_in_game_row(&mut self, row: QuickRow) {
        match row {
            QuickRow::Resume => self.in_game_menu = None,
            QuickRow::States => self.open_states(),
            QuickRow::ExitGame => self.eject(),
            _ => {
                if let Some(child) = row.child() {
                    // The page's first row may be hidden, as the disc is for a one-disc game.
                    let shown = child.shown(&self.in_game_hidden());
                    self.in_game_menu = Some(shown.first().copied().unwrap_or(child));
                }
            }
        }
    }

    fn tap_in_game_menu(&mut self, to: Option<QuickRow>) {
        self.menu_tap = Some((self.now(), self.in_game_menu));
        self.in_game_menu = to;
    }

    fn in_game_menu_shown(&self) -> Option<QuickRow> {
        match self.menu_tap {
            Some((at, None)) if self.now().saturating_sub(at) < MENU_REVEAL_MS => None,
            _ => self.in_game_menu,
        }
    }

    fn double_tap_states(&mut self) {
        if let Some((at, before)) = self.menu_tap.take() {
            if self.now().saturating_sub(at) <= MENU_DOUBLE_TAP_MS {
                self.in_game_menu = before;
            }
        }
        match self.in_game_menu {
            Some(_) => self.open_states(),
            None => self.open_polaroids(),
        }
    }

    fn open_states(&mut self) {
        self.in_game_menu = None;
        self.open_polaroids();
        match self.phase {
            Phase::Polaroids { .. } => self.states_from_menu = true,
            _ => self.in_game_menu = Some(QuickRow::States),
        }
    }

    fn open_quick_row(&mut self, row: QuickRow) {
        match row {
            QuickRow::DateTime => {
                self.phase = clock_screen(self.utc_secs(), self.state.utc_offset_min, true);
            }
            QuickRow::About => self.phase = Phase::About,
            QuickRow::Services => {
                self.phase = Phase::Services {
                    cursor: 0,
                    note: None,
                }
            }
            QuickRow::GameArt => {
                self.phase = Phase::GameArt {
                    cursor: 0,
                    note: None,
                }
            }
            QuickRow::Achievements => {
                let user = self.cheevos_account().map(|a| a.user).unwrap_or_default();
                self.phase = Phase::Keyboard {
                    keys: Keyboard::new(
                        "RetroAchievements username, or none to sign out",
                        &user,
                        false,
                        0,
                        64,
                    ),
                    typing: Typing::RaUser,
                    cursor: 0,
                };
            }
            QuickRow::Updates => {
                self.phase = Phase::Updates { cursor: 0 };
                if self.updates_view.unslot.stage == Stage::Unchecked {
                    self.with_updates(Updates::check);
                }
            }
            QuickRow::Cores => {
                self.phase = Phase::CoreDownloads { cursor: 0 };
                if self.cores_view.listed.is_none() {
                    self.with_cores(CoreDownloads::check);
                }
            }
            QuickRow::BtDevices => {
                self.phase = Phase::Devices { cursor: 0 };
                // Open on what is paired; a search only when there is nothing yet.
                if !self.bt_view.devices.iter().any(|d| d.paired) {
                    self.with_bluetooth(Bluetooth::search);
                }
            }
            QuickRow::WifiNetworks => {
                self.phase = Phase::Networks {
                    cursor: 0,
                    note: None,
                };
                self.with_wifi(Wifi::scan);
            }
            QuickRow::Restart => {
                self.flush_resume();
                self.begin_restart();
            }
            QuickRow::ShutDown => self.hold_power(),
            QuickRow::Reload => {
                self.flush_resume();
                self.begin_reload();
            }
            QuickRow::Screen
            | QuickRow::Game
            | QuickRow::General
            | QuickRow::Network
            | QuickRow::Power
            | QuickRow::ScreenOf(_) => {
                if let Some(row) = row.child() {
                    let row = row.shown(&self.hidden_rows())[0];
                    self.phase = Phase::QuickMenu { row };
                }
            }
            QuickRow::EjectSave
            | QuickRow::Turbo
            | QuickRow::Rewind
            | QuickRow::FastForward
            | QuickRow::FastForwardSound
            | QuickRow::Disc
            | QuickRow::ColourCorrectionOf(_)
            | QuickRow::ShaderOf(_)
            | QuickRow::GbPalettes
            | QuickRow::RunAhead
            | QuickRow::Rumble
            | QuickRow::Theme
            | QuickRow::Clock
            | QuickRow::MenuSounds
            | QuickRow::Haptics
            | QuickRow::Recents
            | QuickRow::ShowTitles
            | QuickRow::Wallpaper
            | QuickRow::WallpaperTint
            | QuickRow::Resume
            | QuickRow::States
            | QuickRow::Settings
            | QuickRow::ExitGame
            | QuickRow::PlayScreen
            | QuickRow::PlayGameplay
            | QuickRow::Shader
            | QuickRow::ColourCorrection
            | QuickRow::Palette
            | QuickRow::BezelOf(_)
            | QuickRow::Wifi
            | QuickRow::Bluetooth
            | QuickRow::Bezel => {}
        }
    }

    fn change_setting(&mut self, row: QuickRow, right: bool) {
        let row = self.playing_row(row);
        if row == QuickRow::Bluetooth {
            self.state.bluetooth = !self.state.bluetooth;
            match self.state.bluetooth {
                true => self.with_bluetooth(Bluetooth::turn_on),
                false => self.with_bluetooth(Bluetooth::turn_off),
            }
            self.persist();
            return;
        }
        if row == QuickRow::Wifi {
            self.state.wifi = !self.state.wifi;
            match self.state.wifi {
                true => self.with_wifi(Wifi::turn_on),
                false => self.with_wifi(Wifi::turn_off),
            }
            self.persist();
            return;
        }
        if row == QuickRow::Palette {
            let to = match right {
                true => self.state.gb_palette.next(),
                false => self.state.gb_palette.prev(),
            };
            self.state.gb_palette = to;
            self.palette_pending = Some(to);
            self.persist();
            return;
        }
        if row == QuickRow::Disc {
            let (count, at) = self.disks;
            if count > 1 {
                let to = (at + if right { 1 } else { count - 1 }) % count;
                self.disks.1 = to;
                self.disc_pending = Some(to);
            }
            return;
        }
        if let QuickRow::BezelOf(platform) = row {
            self.cycle_bezel(platform, right);
            self.persist();
            return;
        }
        if row == QuickRow::Wallpaper {
            self.cycle_wallpaper(right);
            self.persist();
            return;
        }
        if row == QuickRow::Recents {
            self.state.show_recents = !self.state.show_recents;
            self.restock_recents();
            self.persist();
            return;
        }
        if row == QuickRow::ShowTitles {
            self.state.show_titles = !self.state.show_titles;
            self.persist();
            return;
        }
        let s = &mut self.state;
        match row {
            QuickRow::FastForward => {
                let to = ff_next(s.ff_speed, right);
                if to == s.ff_speed {
                    return;
                }
                s.ff_speed = to;
            }
            QuickRow::FastForwardSound => s.ff_sound = !s.ff_sound,
            QuickRow::ColourCorrectionOf(p) => {
                let screen = s.screen_mut(p);
                screen.colour_correction = !screen.colour_correction;
                self.colour_pending = Some(self.colour_correction());
            }
            QuickRow::ShaderOf(p) => {
                let screen = s.screen_mut(p);
                let to = screen.shader.step(p.spec().shaders, right);
                if to == screen.shader {
                    return;
                }
                screen.shader = to;
            }
            QuickRow::Rumble => s.rumble = !s.rumble,
            QuickRow::EjectSave => s.eject_save = !s.eject_save,
            QuickRow::Turbo => s.turbo = !s.turbo,
            QuickRow::Rewind => s.rewind = !s.rewind,
            QuickRow::GbPalettes => s.gb_palettes = !s.gb_palettes,
            QuickRow::RunAhead => {
                let n = RUN_AHEAD_MAX + 1;
                s.runahead = (s.runahead + if right { 1 } else { n - 1 }) % n;
            }
            QuickRow::Theme => s.appearance = s.appearance.toggle(),
            QuickRow::Clock => s.clock_12h = !s.clock_12h,
            QuickRow::MenuSounds => s.menu_sounds = !s.menu_sounds,
            QuickRow::Haptics => s.haptics = !s.haptics,
            QuickRow::WallpaperTint => s.wallpaper_tint = s.wallpaper_tint.step(right),
            QuickRow::Wallpaper
            | QuickRow::Recents
            | QuickRow::ShowTitles
            | QuickRow::DateTime
            | QuickRow::About
            | QuickRow::Screen
            | QuickRow::Game
            | QuickRow::General
            | QuickRow::Resume
            | QuickRow::States
            | QuickRow::Settings
            | QuickRow::ExitGame
            | QuickRow::PlayScreen
            | QuickRow::PlayGameplay
            | QuickRow::Shader
            | QuickRow::ColourCorrection
            | QuickRow::Palette
            | QuickRow::Disc
            | QuickRow::ScreenOf(_)
            | QuickRow::BezelOf(_)
            | QuickRow::Network
            | QuickRow::Wifi
            | QuickRow::WifiNetworks
            | QuickRow::Bluetooth
            | QuickRow::BtDevices
            | QuickRow::Services
            | QuickRow::Updates
            | QuickRow::Cores
            | QuickRow::GameArt
            | QuickRow::Achievements
            | QuickRow::Power
            | QuickRow::Restart
            | QuickRow::ShutDown
            | QuickRow::Reload
            | QuickRow::Bezel => return,
        }
        self.persist();
    }

    pub fn apply_at(&mut self, action: Action, now: Millis) {
        self.clock = now as f64;
        self.apply(action);
    }

    fn step_shader(&mut self, right: bool) {
        if !matches!(self.phase, Phase::Inserting { .. } | Phase::Playing { .. }) {
            return;
        }
        let platform = self.playing_platform();
        let shader = &mut self.state.screen_mut(platform).shader;
        *shader = shader.step(platform.spec().shaders, right);
        let said = match *shader {
            Shader::Off => Toast::ShaderOff,
            Shader::Lcd3x => Toast::ShaderLcd3x,
            Shader::Grid => Toast::ShaderGrid,
            Shader::Dot => Toast::ShaderDot,
            Shader::Simpletex => Toast::ShaderSimpletex,
        };
        self.persist();
        self.hud.toast(said, self.now());
    }

    fn adjust(&mut self, action: Action) -> bool {
        if action == Action::MuteToggle {
            self.mute_toggle();
            return true;
        }
        if self.in_game_menu.is_some()
            && matches!(
                action,
                Action::ShaderNext
                    | Action::ShaderPrev
                    | Action::PaletteNext
                    | Action::PalettePrev
                    | Action::ColourCorrectionToggle
            )
        {
            return true;
        }
        if let Action::ShaderNext | Action::ShaderPrev = action {
            self.step_shader(action == Action::ShaderNext);
            return true;
        }
        if let Action::PaletteNext | Action::PalettePrev = action {
            if self.palette_live && matches!(self.phase, Phase::Playing { .. }) {
                let to = match action {
                    Action::PalettePrev => self.state.gb_palette.prev(),
                    _ => self.state.gb_palette.next(),
                };
                self.state.gb_palette = to;
                self.palette_pending = Some(to);
                self.persist();
                self.hud.toast(Toast::Palette(to), self.now());
            }
            return true;
        }
        if action == Action::ColourCorrectionToggle {
            self.change_setting(QuickRow::ColourCorrectionOf(self.playing_platform()), true);
            let said = match self.colour_correction() {
                true => Toast::ColourOn,
                false => Toast::ColourOff,
            };
            self.hud.toast(said, self.now());
            return true;
        }
        let s = &self.state;
        let (kind, value) = match action {
            Action::BrightnessUp => (HudKind::Brightness, up(s.brightness, 1, BRIGHTNESS_MAX)),
            Action::BrightnessDown => (HudKind::Brightness, s.brightness.saturating_sub(1)),
            Action::BlueLightUp => (HudKind::BlueLight, up(s.blue_light, 1, BLUE_LIGHT_MAX)),
            Action::BlueLightDown => (HudKind::BlueLight, s.blue_light.saturating_sub(1)),
            Action::VolumeUp => (HudKind::Volume, up(self.level(), VOLUME_STEP, VOLUME_MAX)),
            Action::VolumeDown => (HudKind::Volume, self.level().saturating_sub(VOLUME_STEP)),
            _ => return false,
        };
        if kind == HudKind::Volume {
            self.remember_volume();
        }
        let level = match kind {
            HudKind::Brightness => &mut self.state.brightness,
            HudKind::BlueLight => &mut self.state.blue_light,
            HudKind::Volume if self.headphones => &mut self.state.volume_hp,
            HudKind::Volume => &mut self.state.volume,
            HudKind::Rewind => return false,
        };
        let moved = *level != value;
        *level = value;
        let unmuted = kind == HudKind::Volume && std::mem::take(self.muted_mut());
        let (shown, now) = (self.hud_value(kind, value), self.now());
        self.hud.show(kind, shown, self.muted(), now);
        if let (HudKind::Brightness, Some(power)) = (kind, &mut self.power) {
            power.set_backlight(value);
        }
        if moved || unmuted {
            self.persist_soon();
        }
        true
    }

    fn remember_volume(&mut self) {
        if self.vol_before.len() == 2 {
            self.vol_before.remove(0);
        }
        self.vol_before
            .push((self.level(), self.muted(), self.now()));
    }

    fn mute_toggle(&mut self) {
        let now = self.now();
        if let Some((volume, muted, _)) = self
            .vol_before
            .iter()
            .find(|(_, _, at)| now.saturating_sub(*at) <= MUTE_CHORD_MS)
            .copied()
        {
            *self.level_mut() = volume;
            *self.muted_mut() = muted;
        }
        self.vol_before.clear();
        let muted = !self.muted();
        *self.muted_mut() = muted;
        self.hud
            .show(HudKind::Volume, self.output_volume(), muted, now);
        self.persist_soon();
    }

    fn hud_value(&self, kind: HudKind, value: u8) -> u8 {
        match kind {
            HudKind::Volume => self.output_volume(),
            _ => value,
        }
    }

    pub fn show_rewind(&mut self, fill: u8) {
        let now = self.now();
        self.hud.show(HudKind::Rewind, fill, false, now);
    }

    pub fn hide_rewind(&mut self) {
        self.hud.release_rewind();
    }

    pub fn set_ff(&mut self, ff: FfState) {
        self.hud.set_ff(ff);
    }

    pub fn ff_badge(&self) -> Option<Icon> {
        self.hud.badge()
    }

    pub fn blue_light(&self) -> u8 {
        self.state.blue_light
    }

    pub fn brightness(&self) -> u8 {
        self.state.brightness
    }

    pub fn volume(&self) -> u8 {
        self.level()
    }

    fn level(&self) -> u8 {
        if self.headphones {
            self.state.volume_hp
        } else {
            self.state.volume
        }
    }

    fn level_mut(&mut self) -> &mut u8 {
        if self.headphones {
            &mut self.state.volume_hp
        } else {
            &mut self.state.volume
        }
    }

    pub fn muted(&self) -> bool {
        if self.headphones {
            self.state.muted_hp
        } else {
            self.state.muted
        }
    }

    fn muted_mut(&mut self) -> &mut bool {
        if self.headphones {
            &mut self.state.muted_hp
        } else {
            &mut self.state.muted
        }
    }

    pub fn output_volume(&self) -> u8 {
        if self.muted() {
            0
        } else {
            self.level()
        }
    }

    pub fn hud_icon(&self) -> Icon {
        self.hud.glyph()
    }

    pub fn now(&self) -> Millis {
        self.clock as Millis
    }

    fn flick(&mut self, step: fn(&mut Polaroids)) {
        if let Some(p) = &mut self.polaroids {
            step(p);
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.clock += dt as f64 * 1000.0;
        if self.name_pending && self.shelf_platform.face.is_some() {
            self.name_pending = false;
            self.shelf_named = Some(self.now());
        }
        self.timers();
        self.poll_link();
        self.poll_ra_sign_in();
        self.expire_cheevo();
        let now = self.now();
        let ready = self.core_faces_ready();
        if let Some(picker) = &mut self.core_picker {
            if picker.waiting() && (ready || picker.waited(now) >= FACES_WAIT_MS) {
                picker.start(now);
            }
        }
        if self.core_picker.is_some_and(|p| p.finished(now)) {
            self.core_picker = None;
        }
        if !self.on_shelf() {
            self.shelf_mut().release_hold();
        }
        let mut touched = false;
        let mut stepped = false;
        let at = self.shelf_at;
        let spawn_at = self.spawn_at();
        let next = match &mut self.phase {
            Phase::Shelf => {
                let shelf = &mut self.shelves[at].1;
                let was = shelf.index;
                shelf.tick(now);
                shelf.update(dt);
                stepped = shelf.index != was;
                None
            }
            Phase::Inserting {
                cart,
                t,
                core_ready,
                resumed,
                ..
            } => {
                let was = *t;
                *t += dt;
                let at = SEATED_AT - Sfx::Insert.lead();
                touched = !*resumed && was < at && *t >= at;
                (*t >= INSERT_S.max(spawn_at) && *core_ready).then(|| Phase::Playing {
                    cart: std::mem::take(cart),
                })
            }
            Phase::Ejecting { t, .. } => {
                if self.screen <= 0.0 {
                    let was = *t;
                    *t += dt;
                    touched = was < 0.0 && *t >= 0.0;
                }
                (*t >= EJECT_S).then_some(Phase::Shelf)
            }
            _ => None,
        };
        if stepped {
            self.cue(Sfx::Move);
        }
        if touched {
            let sfx = match self.phase {
                Phase::Ejecting { .. } => Sfx::Eject,
                _ => Sfx::Insert,
            };
            self.sfx = Some(sfx);
            // The motor knocks as the cart hits home, which the insert's clip leads up to.
            match sfx {
                Sfx::Insert => self.buzz(BUZZ_SEAT_MS, (Sfx::Insert.lead() * 1000.0) as Millis),
                _ => self.buzz(BUZZ_EJECT_MS, 0),
            }
        }
        if let Some(phase) = next {
            self.phase = phase;
            self.launched_dark = spawn_at > 0.0;
            self.refused_from = None;
            let seated = match &self.phase {
                Phase::Playing { cart } => Some(cart.clone()),
                _ => None,
            };
            if seated.is_none() {
                self.link_loaded = None;
            }
            self.record_cart(seated);
        }
        self.step_screen(dt);
        self.step_footer(dt);
    }

    fn step_screen(&mut self, dt: f32) {
        let lit = matches!(self.phase, Phase::Playing { .. } | Phase::Polaroids { .. });
        let step = if lit {
            dt / POWER_ON_S
        } else {
            -dt / POWER_OFF_S
        };
        self.screen = (self.screen + step).clamp(0.0, 1.0);
        let docked = matches!(
            self.phase,
            Phase::Shelf | Phase::Inserting { .. } | Phase::Ejecting { .. }
        );
        let sliding = matches!(
            self.phase,
            Phase::Inserting { .. }
                | Phase::Playing { .. }
                | Phase::Polaroids { .. }
                | Phase::Ejecting { .. }
        );
        let reach = if sliding { dt / POWER_ON_S } else { 1.0 };
        self.lift = match docked {
            true => (self.lift - reach).max(0.0),
            false => (self.lift + reach).min(1.0),
        };
    }

    fn step_footer(&mut self, dt: f32) {
        let shown = match &self.phase {
            Phase::Playing { .. } => self.in_game_menu_shown(),
            // The carousel already holds this strip, so the menu does not slide it.
            Phase::QuickMenu { row } => {
                self.footer = 1.0;
                self.footer_row = Some(*row);
                return;
            }
            Phase::Ejecting { .. } => {
                self.footer = match self.screen > 0.0 {
                    true => (self.footer + dt / POWER_OFF_S).min(1.0),
                    false => 1.0,
                };
                self.footer_row = None;
                return;
            }
            _ => {
                self.footer = 0.0;
                self.footer_row = None;
                return;
            }
        };
        if let Some(row) = shown {
            self.footer_row = Some(row);
        }
        let target = if shown.is_some() { 1.0 } else { 0.0 };
        let step = dt / FOOTER_SLIDE_S;
        self.footer = if target > self.footer {
            (self.footer + step).min(target)
        } else {
            (self.footer - step).max(target)
        };
        if self.footer == 0.0 {
            self.footer_row = None;
        }
    }

    fn menu_footer_offset(&self) -> f32 {
        (1.0 - ease(self.footer)) * MOUTH_H
    }

    /// The carousel strip shares this canvas, so the picture scales in the space above it.
    /// A 4:3 game frame is a different canvas; its close keeps the full-frame scale.
    fn footer_on_carousel_canvas(&self) -> bool {
        !self.framed_panel() || self.layout() == Layout::Native
    }

    pub fn screen_footer(&self) -> f32 {
        if matches!(self.phase, Phase::Ejecting { .. })
            && self.screen > 0.0
            && self.footer_on_carousel_canvas()
        {
            MOUTH_H * ease(self.footer)
        } else {
            0.0
        }
    }

    pub fn set_panel(&mut self, panel: (u32, u32)) {
        self.panel = panel;
    }

    pub fn layout(&self) -> Layout {
        if matches!(self.phase, Phase::Playing { .. }) && self.in_game_menu_shown().is_some() {
            return IN_GAME_MENU;
        }
        match &self.phase {
            Phase::Playing { .. } | Phase::Polaroids { .. } => Layout::Framed,
            Phase::Ejecting { .. } if self.overlay_slide() > 0.0 => Layout::Framed,
            Phase::Doze { cart: Some(_) } => Layout::Framed,
            _ => Layout::Native,
        }
    }

    pub fn canvas(&self) -> Canvas {
        Canvas::of(self.layout(), self.panel)
    }

    pub fn overlay_slide(&self) -> f32 {
        match &self.phase {
            Phase::Ejecting { t, .. } => ((-t - (EJECT_HOLD_S - POWER_ON_S)) / POWER_ON_S)
                .clamp(0.0, 1.0)
                .min(self.eject_overlay),
            Phase::Playing { .. } | Phase::Polaroids { .. } | Phase::Doze { cart: Some(_) } => {
                self.lift
            }
            _ => 0.0,
        }
    }

    fn framed_panel(&self) -> bool {
        slot_gfx::framed(self.panel)
    }

    fn draw_powering(&self, out: &mut Vec<Draw>) {
        out.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: OUT_W as f32,
            h: OUT_H as f32,
            colour: [0.0, 0.0, 0.0, 1.0],
        });
        if self.game_ready && self.screen > 0.0 {
            out.push(Draw::Game);
        }
    }

    pub fn screen_power(&self) -> f32 {
        self.screen
    }

    pub fn set_game_ready(&mut self, ready: bool) {
        self.game_ready = ready;
    }

    pub fn game_ready(&self) -> bool {
        self.game_ready
    }

    pub fn game_visible(&self) -> bool {
        self.game_ready && self.screen > 0.0
    }

    pub fn tick_ms(&mut self, now: Millis) {
        self.clock = self.clock.max(now as f64);
        self.timers();
    }

    fn timers(&mut self) {
        if let Some(wifi) = &self.wifi {
            self.wifi_view = wifi.view();
        }
        if let Some(bluetooth) = &self.bluetooth {
            bluetooth.watch(matches!(self.phase, Phase::Devices { .. }));
            let view = bluetooth.view();
            // Sound goes to connected headphones, and back to the speaker when they go.
            if view.audio().map(|d| &d.mac) != self.bt_view.audio().map(|d| &d.mac) {
                crate::audio::set_route(
                    view.audio().map(|d| crate::bluetooth::alsa_device(&d.mac)),
                );
            }
            self.bt_view = view;
        }
        if let Some(updates) = &self.updates {
            self.updates_view = updates.view();
        }
        if let Some(cores) = &self.core_downloads {
            self.cores_view = cores.view();
        }
        if let Some(scraper) = &self.scraper {
            self.scrape_view = scraper.view();
            self.show_scraped();
        }
        if let Some(services) = &self.services {
            self.services_view = services.view();
            // The code goes once it has been used, or when there is no longer one to show.
            if matches!(self.phase, Phase::TailscaleLogin)
                && !matches!(self.services_view.tailnet, Tailnet::SignIn(_))
            {
                self.phase = Phase::Services {
                    cursor: ServiceRow::Tailscale as usize,
                    note: None,
                };
            }
            if matches!(self.phase, Phase::SyncthingId) && self.services_view.syncthing_id.is_none()
            {
                self.phase = Phase::Services {
                    cursor: ServiceRow::Syncthing as usize,
                    note: None,
                };
            }
            // Games Syncthing brings go on the shelves as they land.
            if self.services_view.synced != self.synced_seen {
                self.synced_seen = self.services_view.synced;
                self.rescan();
            }
        }
        self.take_rescan();
        self.play_hold();
        self.repeat_held_dir();
        let offer = self.undo_label();
        if let Some(p) = &mut self.polaroids {
            p.set_undo(offer);
        }
        if self.doze_expired() {
            self.on_doze_timeout();
        }
        if self.now() >= self.autosave_at {
            self.autosave();
        }
        if self.now() >= self.card_check_at {
            self.card_check_at = self.now() + CARD_CHECK_MS;
            self.card_read_only = self.power.as_ref().is_some_and(|p| p.card_read_only());
        }
        let failing = self.card_writes.failing() || self.card_read_only;
        if failing != self.hud.alarmed() {
            eprintln!(
                "slot: card: alarm {} (read-only {})",
                if failing { "on" } else { "off" },
                self.card_read_only
            );
            self.log_card(failing);
        }
        self.hud.set_alarm(failing, self.now());
        if self.now() >= self.battery_at {
            self.battery_at = self.now() + BATTERY_POLL_MS;
            self.battery = self.power.as_ref().and_then(|p| p.battery());
            if let Some(b) = self.battery {
                self.on_battery(b);
            }
        }
        if self.now() >= self.headphones_at {
            self.headphones_at = self.now() + HEADPHONES_POLL_MS;
            let on = self.power.as_ref().is_some_and(|p| p.headphones());
            if on != self.headphones {
                self.headphones = on;
                self.hud.set_headphones(on);
                let now = self.now();
                self.hud
                    .show(HudKind::Volume, self.output_volume(), self.muted(), now);
            }
        }
        if self.now() >= self.charge_at {
            self.charge_at = self.now() + CHARGE_POLL_MS;
            if let (Some(power), Some(b)) = (self.power.as_ref(), self.battery.as_mut()) {
                b.charge = power.charge();
            }
            let state = self.led_state();
            self.set_led(state);
        }
        if let Some(GameMenu::Linked {
            since,
            opened: false,
            ..
        }) = self.game_menu
        {
            if self.now().saturating_sub(since) >= LINKED_HOLD_MS {
                self.game_menu = None;
            }
        }
        if let Some(GameMenu::Unplug { since, .. }) = self.game_menu {
            if self.now().saturating_sub(since) >= UNPLUG_HOLD_MS {
                self.game_menu = None;
            }
        }
        if let Some(at) = self.link.as_ref().and_then(|s| s.lost_at) {
            if self.now().saturating_sub(at) >= LINK_LOST_MS {
                self.end_link();
            }
        }
    }

    pub fn led_state(&self) -> LedState {
        let Some(b) = self.battery else {
            return LedState::Running;
        };
        match b.charge {
            Charge::Charging => LedState::Charging,
            Charge::Full => LedState::Charged,
            _ if b.percent <= BATTERY_LOW => LedState::Low,
            _ => LedState::Running,
        }
    }

    fn set_led(&mut self, state: LedState) {
        if self.shutting_down() && state != LedState::Off {
            return;
        }
        if self.last_led == Some(state) {
            return;
        }
        self.last_led = Some(state);
        if let Some(power) = self.power.as_mut() {
            power.set_led(state);
        }
    }

    fn restock_recents(&mut self) {
        let carts: Vec<Cart> = match self.state.show_recents {
            true => self
                .state
                .recents
                .iter()
                .filter_map(|r| {
                    self.carts()
                        .find(|c| c.platform == r.platform && c.stem == r.stem)
                })
                .cloned()
                .collect(),
            false => Vec::new(),
        };
        self.restock(RECENTS_AT, carts);
        self.mark_stars();
    }

    fn restock_favorites(&mut self) {
        let mut carts: Vec<Cart> = self
            .carts()
            .filter(|c| self.state.is_favorite(c.platform, &c.stem))
            .cloned()
            .collect();
        carts.sort_by(|a, b| {
            (slot_store::sort_key(&a.stem), a.platform as u8)
                .cmp(&(slot_store::sort_key(&b.stem), b.platform as u8))
        });
        let index = self.shelves[FAVORITES_AT].1.index;
        self.restock(FAVORITES_AT, carts);
        let shelf = &mut self.shelves[FAVORITES_AT].1;
        if self.shelf_at == FAVORITES_AT && !shelf.carts.is_empty() {
            shelf.select(index.min(shelf.carts.len() - 1));
        }
        self.mark_stars();
    }

    /// Whether the shelf's carts changed.
    fn restock(&mut self, at: usize, carts: Vec<Cart>) -> bool {
        let stocked = &self.shelves[at].1.carts;
        if stocked.len() == carts.len()
            && stocked
                .iter()
                .zip(&carts)
                .all(|(a, b)| a.platform == b.platform && a.stem == b.stem)
        {
            return false;
        }
        let old = self.shelves[at].1.restock(carts);
        self.orphaned.extend(old);
        let empty = self.shelves[at].1.carts.is_empty();
        if self.shelf_at == at && empty {
            self.shelf_at = self.next_shelf(1).unwrap_or(0);
        }
        true
    }

    /// Looks through the card's games again, on a thread of its own.
    fn rescan(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.rescan.is_some() {
            self.rescan_again = true;
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("slot-rescan".into())
            .spawn(move || {
                if let Ok(carts) = scan(&root) {
                    let _ = tx.send(carts);
                }
            });
        match spawned {
            Ok(_) => self.rescan = Some(rx),
            Err(e) => eprintln!("slot: rescan: {e}"),
        }
    }

    /// Puts what a rescan found on the shelves once the shelf is what is showing.
    fn take_rescan(&mut self) {
        if let Some(rx) = &self.rescan {
            match rx.try_recv() {
                Ok(carts) => {
                    self.rescanned = Some(carts);
                    self.rescan = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.rescan = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            if self.rescan.is_none() && std::mem::take(&mut self.rescan_again) {
                self.rescan();
            }
        }
        if self.on_shelf() && self.core_picker.is_none() {
            if let Some(carts) = self.rescanned.take() {
                self.restock_games(carts);
            }
        }
    }

    /// Gives each system's shelf the games now on the card, each staying on the cart it was on.
    fn restock_games(&mut self, carts: Vec<Cart>) {
        let shelf_at = self.shelf_at;
        for (at, (rack, fresh)) in shelves_of(carts).into_iter().enumerate() {
            if rack.platform().is_none() {
                continue;
            }
            let shelf = &self.shelves[at].1;
            let (was, picked) = (
                shelf.index,
                shelf.carts.get(shelf.index).map(|c| c.rom.clone()),
            );
            if !self.restock(at, fresh.carts) {
                continue;
            }
            let shelf = &mut self.shelves[at].1;
            let i = picked
                .and_then(|rom| shelf.carts.iter().position(|c| c.rom == rom))
                .unwrap_or_else(|| was.min(shelf.carts.len().saturating_sub(1)));
            shelf.select(i);
        }
        if self.shelf().carts.is_empty() {
            if let Some(at) = self.next_shelf(1) {
                self.shelf_at = at;
            }
        }
        self.restock_recents();
        self.restock_favorites();
        if self.shelf_at != shelf_at {
            self.shelf_platform = slot_ui::Printed::default();
            self.slot_letter = None;
            self.shelf_named = None;
            self.name_pending = true;
        }
    }

    /// The line at the top of the shelf while Syncthing is fetching.
    pub fn sync_text(&self) -> Option<String> {
        self.services_view.syncing.map(sync_line)
    }

    pub fn set_sync_face(&mut self, face: Option<(TexId, u32, u32)>) {
        self.sync_face = face;
    }

    /// The RetroAchievements account on the card, if one has signed in.
    pub fn cheevos_account(&self) -> Option<crate::cheevos::Account> {
        crate::cheevos::account(self.root.as_deref()?)
    }

    pub fn on_cheevo(&mut self, notice: &slot_cheevos::Notice) {
        eprintln!("slot: cheevos: {notice:?}");
        if let Some(line) = crate::cheevos::line(notice) {
            self.cheevo_line(line);
        }
    }

    fn cheevo_line(&mut self, line: String) {
        if self.cheevo_lines.is_empty() {
            self.cheevo_since = self.now();
        }
        self.cheevo_lines.push_back(line);
    }

    /// The achievement line on screen now, if there is one.
    pub fn cheevo_text(&self) -> Option<String> {
        self.cheevo_lines.front().cloned()
    }

    pub fn set_cheevo_face(&mut self, face: Option<(TexId, u32, u32)>) {
        self.cheevo_face = face;
    }

    fn poll_ra_sign_in(&mut self) {
        let Some(rx) = &self.ra_sign_in else {
            return;
        };
        let line = match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                "RetroAchievements did not sign you in".to_string()
            }
            Ok(Err(e)) => e,
            Ok(Ok(account)) => match self.root.as_deref() {
                Some(root) => match crate::cheevos::save_account(root, &account) {
                    Ok(()) => format!("Signed in to RetroAchievements as {}", account.user),
                    Err(e) => format!("Could not save the sign in: {e}"),
                },
                None => "No card to keep the sign in on".to_string(),
            },
        };
        self.ra_sign_in = None;
        // In place of the line saying it was signing in.
        self.cheevo_lines.clear();
        self.cheevo_line(line);
    }

    fn expire_cheevo(&mut self) {
        if !self.cheevo_lines.is_empty() && self.now() >= self.cheevo_since + CHEEVO_MS {
            self.cheevo_lines.pop_front();
            self.cheevo_since = self.now();
        }
    }

    fn toggle_favorite(&mut self) {
        let Some(cart) = self.shelf().carts.get(self.shelf().index) else {
            return;
        };
        let (platform, stem) = (cart.platform, cart.stem.clone());
        self.state.toggle_favorite(platform, &stem);
        let shelf_at = self.shelf_at;
        self.restock_favorites();
        if self.shelf_at != shelf_at {
            self.shelf_platform = slot_ui::Printed::default();
            self.slot_letter = None;
            self.shelf_named = None;
            self.name_pending = true;
        }
        self.persist();
    }

    fn record_cart(&mut self, cart: Option<String>) {
        let platform = self.seated_cart().map(|c| c.platform);
        if let (Some(stem), Some(platform)) = (&cart, platform) {
            let first = self.state.recents.first();
            if first.is_none_or(|r| r.platform != platform || r.stem != *stem) {
                let stem = stem.clone();
                self.state.played(platform, &stem);
                self.restock_recents();
                self.persist();
            }
        }
        if self.state.cart == cart && self.state.cart_platform == platform {
            return;
        }
        self.state.cart = cart;
        self.state.cart_platform = platform;
        self.persist();
    }

    fn persist(&self) {
        let Some(root) = &self.root else {
            return;
        };
        self.state_writer.flush();
        let written = write_slot_state(root, &self.state);
        if let Err(e) = &written {
            eprintln!("slot: slot.state: {e}");
        }
        self.card_writes.wrote("slot.state", &written);
    }

    fn persist_soon(&self) {
        if let Some(root) = &self.root {
            self.state_writer.save(root, &self.state);
        }
    }

    pub fn flush_state(&self) {
        self.state_writer.flush();
    }

    pub fn on_core_ready(&mut self) {
        let Phase::Inserting {
            cart, core_ready, ..
        } = &mut self.phase
        else {
            return;
        };
        *core_ready = true;
        let cart = cart.clone();
        self.retire_refused_resume(&cart);
    }

    fn retire_refused_resume(&mut self, stem: &str) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        if snapshot.resume_trusted() {
            return;
        }
        if !self.named_core {
            return;
        }
        let Some(root) = &self.root else {
            return;
        };
        if let Some(h) = self.pending_save.take() {
            let _ = h.join();
        }
        let ring = StateRing::new(root, self.platform, self.core, stem);
        match ring.retire_resume(&format_stamp(self.wall_secs())) {
            Ok(Some(to)) => eprintln!(
                "slot: resume: {} refused this state, moved it to {}",
                self.core.as_str(),
                to.display()
            ),
            Ok(None) => {}
            Err(e) => eprintln!("slot: resume: could not move the refused state aside: {e}"),
        }
    }

    pub fn on_core_failed(&mut self) {
        let caught = self.seat();
        let Phase::Inserting { cart, .. } = &mut self.phase else {
            return;
        };
        let cart = std::mem::take(cart);
        self.refuse_out(cart, caught);
    }

    /// The core died under the game. Out the cart comes as an eject takes it, with the alert of
    /// one that wouldn't open and nothing saved: a dead core has no state to give, so the card's
    /// is left as it was. The alert's clock starts when the cart starts up, as a seated one's
    /// would, not with the hold, where the slot front hides it.
    pub fn on_core_died(&mut self) {
        let Phase::Playing { cart } = &mut self.phase else {
            return;
        };
        let cart = std::mem::take(cart);
        self.eject_overlay = match self.layout() {
            Layout::Framed => self.overlay_slide(),
            Layout::Native => 0.0,
        };
        self.in_game_menu = None;
        self.end_link();
        self.close_game_menu();
        self.pending = None;
        self.refusal = None;
        self.phase = Phase::Ejecting {
            cart,
            t: -EJECT_HOLD_S,
        };
        self.refused_from = Some(0.0);
    }

    fn refuse_out(&mut self, cart: String, caught: f32) {
        let t = (1.0 - caught) * EJECT_S;
        self.phase = Phase::Ejecting { cart, t };
        self.refused_from = Some(t);
    }

    pub fn refuse(&mut self) {
        self.refusal = Some(Refusal::started(self.now()));
        self.cue(Sfx::Error);
        self.buzz(BUZZ_REFUSE_MS, 0);
    }

    pub fn refusal_active(&self, now: Millis) -> bool {
        self.refusal.is_some_and(|r| r.active(now))
    }

    /// When into the insert its core may start: at once, but for a disc, once it has spun and
    /// the screen has gone black.
    fn spawn_at(&self) -> f32 {
        let Phase::Inserting {
            cart,
            resumed: false,
            ..
        } = &self.phase
        else {
            return 0.0;
        };
        match self.shelf().find(cart) {
            Some((c, _)) if Media::of(c.platform) == Some(Media::Disc) => {
                INSERT_S + SPIN_S + DARK_S
            }
            _ => 0.0,
        }
    }

    pub fn may_spawn_core(&self) -> bool {
        match &self.phase {
            Phase::Inserting { t, .. } => *t >= self.spawn_at(),
            _ => true,
        }
    }

    /// How far round the seated disc has turned, spinning up from when it seats.
    fn disc_turn(&self) -> f32 {
        let Phase::Inserting { t, .. } = &self.phase else {
            return 0.0;
        };
        if self.spawn_at() == 0.0 {
            return 0.0;
        }
        let s = (t - SEATED_AT).max(0.0);
        match s < SPIN_UP_S {
            true => SPIN_RAD_S * s * s / (2.0 * SPIN_UP_S),
            false => SPIN_RAD_S * (s - SPIN_UP_S / 2.0),
        }
    }

    /// How black the screen has gone over a disc's spin, ahead of its core starting.
    fn launch_dark(&self) -> f32 {
        let (Phase::Inserting { t, .. }, at) = (&self.phase, self.spawn_at()) else {
            return 0.0;
        };
        if at == 0.0 {
            return 0.0;
        }
        ((t - (at - DARK_S)) / DARK_S).clamp(0.0, 1.0)
    }

    fn black(&self, alpha: f32, out: &mut Vec<Draw>) {
        out.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: self.canvas().w,
            h: OUT_H as f32,
            colour: [0.0, 0.0, 0.0, alpha],
        });
    }

    pub fn seat(&self) -> f32 {
        match &self.phase {
            Phase::Shelf => 0.0,
            Phase::Inserting { t, resumed, .. } => {
                if *resumed {
                    1.0
                } else {
                    (t / SEATED_AT).clamp(0.0, 1.0)
                }
            }
            Phase::Ejecting { t, .. } => 1.0 - (t / EJECT_S).clamp(0.0, 1.0),
            _ => 1.0,
        }
    }

    pub fn draw(&self, out: &mut Vec<Draw>) {
        if self.shutting_down() {
            out.push(Draw::Rect {
                x: 0.0,
                y: 0.0,
                w: self.canvas().w,
                h: OUT_H as f32,
                colour: [0.0, 0.0, 0.0, 1.0],
            });
            let which = if self.restarting {
                PowerChoice::Restart
            } else if self.reloading {
                PowerChoice::Reload
            } else {
                PowerChoice::PowerOff
            };
            if let Some((tex, w, h)) = self.shutdown_faces.get(which.index()).copied() {
                out.push(Draw::Tex {
                    x: self.canvas().centred(w as f32).floor(),
                    y: ((OUT_H - h) / 2) as f32,
                    w: w as f32,
                    h: h as f32,
                    tex,
                    alpha: 1.0,
                });
            }
            return;
        }
        match &self.phase {
            Phase::SetClock {
                picker, from_menu, ..
            } => {
                let (line, hint) = match self.clock_faces {
                    Some((line, hint)) => (Some(line), Some(hint)),
                    None => (None, None),
                };
                let back = self.clock_back.filter(|_| *from_menu);
                picker.draw(self.canvas(), line, hint, back, out);
                return;
            }
            Phase::QuickMenu { row } => QuickMenu {
                row: *row,
                hidden: &self.hidden_rows(),
                values: QuickRow::ALL.map(|r| self.quick_value(r)),
                clock: self.quick_clock_faces,
                wallpaper: self.quick_wallpaper_faces,
                faces: self.quick_menu_faces.as_ref(),
                ground: self.menu_ground(),
                palette: None,
                bezels: self.quick_bezel_faces,
                bezel: None,
                over_game: false,
                footer_legends: true,
                footer_top: OUT_H as f32,
                canvas: self.canvas(),
            }
            .draw(out),
            Phase::Shelf => {
                self.draw_background(out);
                match (self.core_picker_shown(), self.selected_stem()) {
                    (Some(picker), Some(stem)) => {
                        let open = ease(picker.openness(self.now()));
                        let dim = 1.0 + (CORE_PICKER_DIM - 1.0) * open;
                        self.shelf().draw_row(
                            self.canvas(),
                            Some(stem),
                            0.0,
                            CORE_PICKER_RECEDE * open,
                            dim,
                            out,
                        );
                        draw_empty_slot(self.canvas(), self.slot_faces, out);
                    }
                    _ => {
                        self.shelf()
                            .draw(self.canvas(), self.shelf_shake(), self.slot_faces, out);
                        draw_slot_name(
                            self.canvas(),
                            self.shelf_platform,
                            self.slot_name_alpha(),
                            out,
                        );
                        slot_ui::draw_slot_title(
                            self.canvas(),
                            self.shelf_title,
                            self.title_alpha(),
                            out,
                        );
                    }
                }
                draw_footer(
                    self.canvas(),
                    self.battery,
                    self.battery_percent,
                    self.bolt,
                    self.shelf_clock,
                    &self.net_icons(),
                    footer_ink(self.light()),
                    out,
                );
                if let Some((tex, w, h)) = self.sync_face {
                    let (w, h) = (w as f32, h as f32);
                    out.push(Draw::Tex {
                        x: self.canvas().centred(w).floor(),
                        y: ((slot_ui::PLATE_H - h) / 2.0).floor(),
                        w,
                        h,
                        tex,
                        alpha: 1.0,
                    });
                }
            }
            Phase::About => {
                slot_ui::draw_menu_ground(self.canvas(), self.menu_ground(), out);
                draw_sticker(self.canvas(), self.sticker_face, out);
                self.draw_menu_footer(out);
                return;
            }
            Phase::Networks { .. }
            | Phase::Devices { .. }
            | Phase::Services { .. }
            | Phase::TailscaleLogin
            | Phase::SyncthingId
            | Phase::Updates { .. }
            | Phase::CoreDownloads { .. }
            | Phase::GameArt { .. }
            | Phase::Cores { .. }
            | Phase::Search { .. }
            | Phase::Keyboard { .. } => {
                slot_ui::draw_menu_ground(self.canvas(), self.menu_ground(), out);
                if let Some((tex, w, h)) = self.page_face {
                    let canvas = self.canvas();
                    out.push(Draw::Tex {
                        x: canvas.centred(w as f32).round(),
                        y: ((OUT_H - h) / 2) as f32,
                        w: w as f32,
                        h: h as f32,
                        tex,
                        alpha: 1.0,
                    });
                }
                return;
            }
            Phase::Inserting { cart, resumed, .. } => {
                if !resumed {
                    self.draw_background(out);
                    self.shelf()
                        .draw_row(self.canvas(), Some(cart), 0.0, self.seat(), 1.0, out);
                }
                self.chrome(cart, 0.0, out);
                let dark = self.launch_dark();
                if dark > 0.0 {
                    self.black(dark, out);
                }
            }
            Phase::Playing { .. } if self.launched_dark && self.screen < 1.0 => {
                self.push_game(out);
                self.black(1.0 - self.screen, out);
            }
            Phase::Ejecting { .. }
                if self.framed_panel()
                    && (self.layout() == Layout::Framed || self.screen > 0.0) =>
            {
                self.draw_powering(out)
            }
            Phase::Playing { .. } if self.framed_panel() && self.screen < 1.0 => {
                self.draw_powering(out)
            }
            Phase::Ejecting { cart, .. } => {
                self.draw_background(out);
                self.shelf()
                    .draw_row(self.canvas(), Some(cart), 0.0, self.seat(), 1.0, out);
                self.chrome(cart, 0.0, out);
            }
            Phase::Playing { cart } if self.screen < 1.0 => {
                self.draw_background(out);
                self.chrome(cart, 0.0, out);
            }
            Phase::Playing { .. } => self.push_game(out),
            Phase::Polaroids { .. } => {
                self.push_game(out);
                if let Some(p) = &self.polaroids {
                    p.draw(
                        self.battery,
                        self.battery_percent,
                        self.hud_bolt,
                        self.shelf_clock,
                        out,
                    );
                }
            }
            Phase::Doze { .. } => {
                out.push(Draw::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: self.canvas().w,
                    h: OUT_H as f32,
                    colour: [0.0, 0.0, 0.0, 1.0],
                });
                return;
            }
        }
        if let (Some(row), Phase::Playing { .. }, true) =
            (self.in_game_menu_shown(), &self.phase, self.screen >= 1.0)
        {
            QuickMenu {
                row,
                hidden: &self.in_game_hidden(),
                values: QuickRow::ALL.map(|r| self.quick_value(r)),
                clock: None,
                wallpaper: None,
                faces: self.quick_menu_faces.as_ref(),
                ground: None,
                palette: self.quick_palette_faces,
                bezels: [None; Platform::COUNT],
                bezel: self.quick_bezel_faces[self.playing_platform().index()],
                over_game: true,
                footer_legends: false,
                footer_top: OUT_H as f32 - MOUTH_H + self.menu_footer_offset(),
                canvas: self.canvas(),
            }
            .draw(out);
            if row.page() != QuickRow::PLAY_SCREEN {
                slot_ui::draw_corners(
                    self.canvas(),
                    self.battery,
                    self.battery_percent,
                    self.hud_bolt,
                    self.shelf_clock,
                    out,
                );
            }
        }
        if let (Some(_), Phase::Playing { .. }, Some((tex, w, h))) =
            (&self.core_options, &self.phase, self.page_face)
        {
            slot_ui::draw_menu_ground(self.canvas(), self.menu_ground(), out);
            out.push(Draw::Tex {
                x: self.canvas().centred(w as f32).round(),
                y: ((OUT_H - h) / 2) as f32,
                w: w as f32,
                h: h as f32,
                tex,
                alpha: 1.0,
            });
        }
        self.draw_menu_footer(out);
        self.draw_close_footer(out);
        if let Some(picker) = self.core_picker_shown() {
            self.draw_core_picker(&picker, out);
        }
        if let Some(menu) = self.game_menu {
            self.draw_game_menu(menu, out);
        }
        self.hud.draw(self.canvas(), self.now(), out);
        if let Some((tex, w, h)) = self.cheevo_face {
            let (w, h) = (w as f32, h as f32);
            let shown = self.now().saturating_sub(self.cheevo_since);
            let left = CHEEVO_MS.saturating_sub(shown);
            let alpha = (shown.min(left) as f32 / CHEEVO_FADE_MS as f32).min(1.0);
            out.push(Draw::Tex {
                x: self.canvas().centred(w).floor(),
                y: 12.0,
                w,
                h,
                tex,
                alpha,
            });
        }
    }

    pub fn screen_shake(&self) -> f32 {
        self.shake_at(self.now())
    }

    pub fn shake_at(&self, now: Millis) -> f32 {
        self.shake_when(matches!(self.phase, Phase::Playing { .. }), now)
    }

    pub fn shelf_shake(&self) -> f32 {
        self.shake_when(self.on_shelf() && self.core_picker.is_none(), self.now())
    }

    fn shake_when(&self, mine: bool, now: Millis) -> f32 {
        if !mine {
            return 0.0;
        }
        self.refusal.map_or(0.0, |r| r.offset(now))
    }

    fn push_game(&self, out: &mut Vec<Draw>) {
        if self.game_visible() {
            out.push(Draw::Game);
        }
    }

    fn draw_menu_footer(&self, out: &mut Vec<Draw>) {
        let carousel = matches!(self.phase, Phase::QuickMenu { .. } | Phase::About);
        let dy = if carousel {
            0.0
        } else {
            self.menu_footer_offset()
        };
        let start = out.len();
        let row = match &self.phase {
            Phase::Playing { cart } if self.footer > 0.0 => {
                self.draw_seated_mouth(cart, out);
                self.in_game_menu_shown().or(self.footer_row)
            }
            // The carousel menu is not a game, so the mouth stays empty, in the
            // same place the shelf already had it.
            Phase::QuickMenu { row } => {
                draw_empty_slot(self.canvas(), self.slot_faces, out);
                Some(*row)
            }
            Phase::About => {
                draw_empty_slot(self.canvas(), self.slot_faces, out);
                self.draw_about_legend(out);
                return;
            }
            _ => return,
        };
        shift_down(&mut out[start..], dy);
        if let Some(row) = row {
            self.draw_menu_legends(row, LINK_LEGEND_Y + dy, out);
        }
    }

    fn draw_close_footer(&self, out: &mut Vec<Draw>) {
        let Phase::Ejecting { cart, .. } = &self.phase else {
            return;
        };
        if !self.footer_on_carousel_canvas() {
            return;
        }
        let start = out.len();
        // The framed power-off paints black and the picture, and never the slot. The other
        // close already draws the seated cart through the chrome.
        if self.framed_panel() && self.screen > 0.0 {
            self.draw_seated_mouth(cart, out);
        }
        draw_footer(
            self.canvas(),
            self.battery,
            self.battery_percent,
            self.bolt,
            self.shelf_clock,
            &self.net_icons(),
            footer_ink(self.light()),
            out,
        );
        shift_down(&mut out[start..], self.menu_footer_offset());
    }

    fn draw_seated_mouth(&self, stem: &str, out: &mut Vec<Draw>) {
        let Some((cart, face)) = self.shelf().find(stem) else {
            return;
        };
        let (rest, scale) = self.shelf().selected_at(self.canvas());
        SlotChrome {
            cart,
            face,
            rest,
            scale,
            seat: 1.0,
            turn: 0.0,
            alert: None,
            dim: 0.0,
            screen: 0.0,
            game: false,
            over: true,
            behind: self.shelf().behind(stem),
            slot: self.slot_faces,
            canvas: self.canvas(),
        }
        .draw(out);
    }

    fn draw_about_legend(&self, out: &mut Vec<Draw>) {
        let Some(faces) = &self.quick_menu_faces else {
            return;
        };
        let [back, .., restart] = match self.light() {
            true => faces.legend_light,
            false => faces.legend,
        };
        self.draw_side_legend(back, false, LINK_LEGEND_Y, out);
        self.draw_side_legend(restart, true, LINK_LEGEND_Y, out);
    }

    fn draw_menu_legends(&self, row: QuickRow, y: f32, out: &mut Vec<Draw>) {
        let Some(faces) = &self.quick_menu_faces else {
            return;
        };
        let [back, change, open, resume, _] = match self.light() {
            true => faces.legend_light,
            false => faces.legend,
        };
        let left = match row.page() == QuickRow::PLAY {
            true => resume,
            false => back,
        };
        let right = if row.opens() { open } else { change };
        self.draw_side_legend(left, false, y, out);
        self.draw_side_legend(right, true, y, out);
    }

    fn draw_side_legend(&self, face: (TexId, u32), right: bool, y: f32, out: &mut Vec<Draw>) {
        let seen = face.1.saturating_sub(HINT_EDGE) as f32;
        let x = if right {
            (self.canvas().w - LINK_LEGEND_EDGE - seen).round()
        } else {
            LINK_LEGEND_EDGE
        };
        out.push(Draw::Tex {
            x,
            y,
            w: face.1 as f32,
            h: MOUTH_H,
            tex: face.0,
            alpha: 1.0,
        });
    }

    fn chrome(&self, stem: &str, dim: f32, out: &mut Vec<Draw>) {
        let Some((cart, face)) = self.shelf().find(stem) else {
            return;
        };
        let alpha = self.alert_alpha();
        let (rest, scale) = self.shelf().selected_at(self.canvas());
        SlotChrome {
            cart,
            face,
            rest,
            scale,
            seat: self.seat(),
            turn: self.disc_turn(),
            alert: self.alert_face.filter(|_| alpha > 0.0).map(|t| (t, alpha)),
            dim,
            screen: self.screen,
            game: self.game_ready,
            over: !matches!(
                &self.phase,
                Phase::Inserting { resumed: true, .. }
                    | Phase::Playing { .. }
                    | Phase::Ejecting { .. }
            ),
            behind: self.shelf().behind(stem),
            slot: self.slot_faces,
            canvas: self.canvas(),
        }
        .draw(out);
    }

    pub fn alert_visible(&self) -> bool {
        self.alert_alpha() > 0.0
    }

    pub fn alert_alpha(&self) -> f32 {
        let (Some(from), Phase::Ejecting { t, .. }) = (self.refused_from, &self.phase) else {
            return 0.0;
        };
        let span = EJECT_S - from;
        if span <= 0.0 {
            return 0.0;
        }
        let u = ((t - from) / span).clamp(0.0, 1.0);
        ((ALERT_GONE - u) / (ALERT_GONE - ALERT_HOLD)).clamp(0.0, 1.0)
    }

    pub fn set_alert_face(&mut self, face: TexId) {
        self.alert_face = Some(face);
    }

    pub fn set_shutdown_faces(&mut self, faces: Vec<(TexId, u32, u32)>) {
        self.shutdown_faces = faces;
    }

    pub fn set_core_interior_face(&mut self, interior: TexId) {
        self.core_interior_face = Some(interior);
    }

    pub fn set_core_board_faces(&mut self, board: TexId, lid: TexId) {
        self.core_board_face = Some(board);
        self.core_lid_face = Some(lid);
        self.core_faces_stem = self.selected_stem().map(str::to_string);
    }

    pub fn set_core_part_faces(
        &mut self,
        sockets: Vec<TexId>,
        chips: Vec<TexId>,
        blank: TexId,
        shadow: TexId,
    ) {
        self.core_socket_faces = sockets;
        self.core_chip_faces = chips;
        self.core_blank_chip_face = Some(blank);
        self.core_chip_shadow_face = Some(shadow);
    }

    pub fn set_core_legend_faces(&mut self, faces: Vec<(TexId, u32)>) {
        self.core_legend_faces = faces;
    }

    pub fn set_link_menu_faces(&mut self, faces: Vec<(TexId, u32, u32)>) {
        self.link_menu_faces = faces;
    }

    pub fn set_link_linked_face(&mut self, face: (TexId, u32, u32)) {
        self.link_linked_face = Some(face);
    }

    pub fn set_link_step_faces(&mut self, faces: Vec<(TexId, u32, u32)>) {
        self.link_step_faces = faces;
    }

    pub fn set_link_fail_faces(&mut self, faces: Vec<(TexId, u32, u32)>) {
        self.link_fail_faces = faces;
    }

    pub fn set_link_legend_faces(&mut self, faces: Vec<(TexId, u32)>) {
        self.link_legend_faces = faces;
    }

    pub fn set_link_sprites(&mut self, sprites: LinkSprites) {
        self.link_sprites = Some(sprites);
    }

    pub fn set_link_shot(&mut self, tex: Option<TexId>) {
        self.link_shot = tex;
    }

    pub fn set_link_port(&mut self, tex: TexId) {
        if let Some(sprites) = &mut self.link_sprites {
            sprites.port.tex = tex;
        }
    }

    pub fn link_sprites_ready(&self) -> bool {
        self.link_sprites.is_some()
    }

    fn core_picker_shown(&self) -> Option<CorePicker> {
        self.core_picker.filter(|p| !p.waiting())
    }

    fn draw_core_picker(&self, picker: &CorePicker, out: &mut Vec<Draw>) {
        let now = self.now();
        let progress = picker.openness(now);
        let lift = lift_of(progress);
        let (rest, scale) = self.shelf().selected_at(self.canvas());
        let shelf = shelf_cart_at(rest, scale);
        let board = board_from(self.canvas(), shelf, progress);
        let zoom = board_zoom(board);
        let ready = self.core_faces_ready();

        if ready {
            if let Some(tex) = self.core_interior_face {
                out.push(Draw::Tex {
                    x: board.x,
                    y: board.y,
                    w: board.w,
                    h: board.h,
                    tex,
                    alpha: 1.0,
                });
            }
            if let Some(tex) = self.core_board_face {
                out.push(Draw::Tex {
                    x: board.x,
                    y: board.y,
                    w: board.w,
                    h: board.h,
                    tex,
                    alpha: 1.0,
                });
            }
            for (i, tex) in self.core_socket_faces.iter().copied().enumerate() {
                let (x, y) = on_board(board, SOCKET_U[i], SOCKET_V);
                out.push(Draw::Tex {
                    x: x.round(),
                    y: y.round(),
                    w: SOCKET_W as f32 * zoom,
                    h: SOCKET_H as f32 * zoom,
                    tex,
                    alpha: 1.0,
                });
            }

            let chip = picker.chip(now);
            let u = CHIP_U[0] + (CHIP_U[1] - CHIP_U[0]) * chip.across;
            if chip.lift > 0.0 {
                if let Some(tex) = self.core_chip_shadow_face {
                    let (cx, cy) = on_board(board, u + 19.0, CHIP_V + 29.4);
                    let (w, h) = (SHADOW_W as f32 * zoom, SHADOW_H as f32 * zoom);
                    out.push(Draw::Tex {
                        x: cx - w / 2.0,
                        y: cy - h / 2.0,
                        w,
                        h,
                        tex,
                        alpha: 0.6 * chip.lift * lift,
                    });
                }
            }
            let face = match chip.seated {
                Some(core) => self.core_chip_faces.get(core.index()).copied(),
                None => self.core_blank_chip_face,
            };
            if let Some(tex) = face {
                let (x, y) = on_board(board, u, CHIP_V - HOP_LIFT * chip.lift);
                let body = Placed {
                    x: x + chip.shake,
                    y,
                    w: CHIP_W as f32 * zoom,
                    h: CHIP_H as f32 * zoom,
                };
                let at = grown(body, TURN_PAD as f32 * zoom);
                out.push(Draw::Turned {
                    x: at.x.round(),
                    y: at.y.round(),
                    w: at.w,
                    h: at.h,
                    tex,
                    alpha: 1.0,
                    turn: chip.tip,
                });
            }
        }

        if let Some(tex) = self.core_chip_shadow_face {
            let (lid, _) = lid_from(self.canvas(), shelf, progress);
            let k = lid.w / lid_at(self.canvas(), 1.0).0.w;
            let (w, h) = (LID_SHADOW_W * k, LID_SHADOW_H * k);
            out.push(Draw::Tex {
                x: lid.x + (lid.w - w) / 2.0,
                y: lid.y + lid.h + LID_SHADOW_DROP * k - h / 2.0,
                w,
                h,
                tex,
                alpha: LID_SHADOW_ALPHA * lift,
            });
        }

        if ready {
            if let Some(tex) = self.core_lid_face {
                let (lid, turn) = lid_from(self.canvas(), shelf, progress);
                let at = grown(lid, TURN_PAD as f32 * lid.w / CART_W as f32);
                out.push(Draw::Turned {
                    x: at.x,
                    y: at.y,
                    w: at.w,
                    h: at.h,
                    tex,
                    alpha: 1.0,
                    turn,
                });
            }
        } else if let Some((_, Some(tex))) = self
            .selected_stem()
            .and_then(|stem| self.shelf().find(stem))
        {
            let (lid, turn) = lid_from(self.canvas(), shelf, progress);
            out.push(Draw::Turned {
                x: lid.x,
                y: lid.y,
                w: lid.w,
                h: lid.h,
                tex,
                alpha: 1.0,
                turn,
            });
        }

        for (tex, w, x) in centred_hints(self.canvas(), &self.core_legend_faces, CORE_LEGEND_GAP) {
            out.push(Draw::Tex {
                x,
                y: CORE_LEGEND_Y,
                w: w as f32,
                h: MOUTH_H,
                tex,
                alpha: lift,
            });
        }
    }

    fn draw_game_menu(&self, menu: GameMenu, out: &mut Vec<Draw>) {
        if let Some(tex) = self.link_shot {
            out.push(Draw::Tex {
                x: 0.0,
                y: 0.0,
                w: OUT_W as f32,
                h: OUT_H as f32,
                tex,
                alpha: 1.0,
            });
        }
        out.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: OUT_W as f32,
            h: OUT_H as f32,
            colour: LINK_SCRIM,
        });
        if let Some(sprites) = &self.link_sprites {
            crate::link_screen::draw_link_art(menu, self.link_hardware, self.now(), sprites, out);
        }
        let line = match menu {
            GameMenu::Pick(role) => self.link_menu_faces.get(role.index()).copied(),
            GameMenu::Working { step, .. } => self
                .link_step_faces
                .get(step.shown(self.radio.warmed()).index())
                .copied(),
            GameMenu::Linked { .. } => self.link_linked_face,
            GameMenu::Failed { fail, .. } => fail
                .shown()
                .and_then(|i| self.link_fail_faces.get(i).copied()),
            GameMenu::Unplug { .. } => None,
        };
        if let Some((tex, w, h)) = line {
            out.push(Draw::Tex {
                x: ((OUT_W as f32 - w as f32) / 2.0).round(),
                y: LINK_TEXT_Y,
                w: w as f32,
                h: h as f32,
                tex,
                alpha: 1.0,
            });
        }
        let switchable = self.seated().is_some_and(|stem| self.link_switchable(stem));
        // Cancel and Mode stay on the left of the footer, Swap and Link on the right, so the
        // socket in the middle of the strip stays clear.
        let (left, right): (&[LinkLegend], &[LinkLegend]) = match menu {
            GameMenu::Pick(_) if switchable => (
                &[LinkLegend::Cancel, LinkLegend::Mode],
                &[LinkLegend::Swap, LinkLegend::Link],
            ),
            GameMenu::Pick(_) => (&[LinkLegend::Cancel], &[LinkLegend::Swap, LinkLegend::Link]),
            GameMenu::Working { .. } => (&[LinkLegend::Cancel], &[]),
            GameMenu::Linked { opened: true, .. } => (&[LinkLegend::Back], &[LinkLegend::EndLink]),
            GameMenu::Linked { .. } => (&[], &[]),
            GameMenu::Failed { .. } => (&[], &[LinkLegend::Ok]),
            GameMenu::Unplug { .. } => (&[], &[]),
        };
        self.draw_link_keys(left, false, out);
        self.draw_link_keys(right, true, out);
    }

    fn draw_link_keys(&self, keys: &[LinkLegend], right: bool, out: &mut Vec<Draw>) {
        let faces: Vec<(TexId, u32)> = keys
            .iter()
            .filter_map(|k| self.link_legend_faces.get(k.index()).copied())
            .collect();
        if faces.is_empty() {
            return;
        }
        let seen = |w: u32| w.saturating_sub(HINT_EDGE) as f32;
        let total = faces.iter().map(|(_, w)| seen(*w)).sum::<f32>()
            + LINK_LEGEND_GAP * faces.len().saturating_sub(1) as f32;
        let mut x = if right {
            (OUT_W as f32 - LINK_LEGEND_EDGE - total).round()
        } else {
            LINK_LEGEND_EDGE
        };
        for (tex, w) in faces {
            out.push(Draw::Tex {
                x,
                y: LINK_LEGEND_Y,
                w: w as f32,
                h: MOUTH_H,
                tex,
                alpha: 1.0,
            });
            x += seen(w) + LINK_LEGEND_GAP;
        }
    }

    fn auto_link(&self, stem: &str) -> Option<(&Cart, LinkKind)> {
        let cart = self.shelf().carts.iter().find(|c| c.stem == stem)?;
        let auto = link_kind(&cart.code, &cart.title, slot_store::header_clean(&cart.rom));
        Some((cart, auto))
    }

    fn on_shelf(&self) -> bool {
        matches!(self.phase, Phase::Shelf)
    }

    pub fn browsed_rom(&self) -> Option<&std::path::Path> {
        if !self.on_shelf() {
            return None;
        }
        let shelf = self.shelf();
        shelf.carts.get(shelf.index).map(|c| c.rom.as_path())
    }

    fn insert(&mut self, clean: bool) {
        if !self.on_shelf() {
            return;
        }
        let at = self.shelf().index;
        let Some((cart, platform)) = self.shelf_mut().carts.get_mut(at).map(|c| {
            c.read_header();
            (c.stem.clone(), c.platform)
        }) else {
            return;
        };
        if let Some(missing) = self.missing_bios(platform) {
            eprintln!("slot: launch: {cart} needs BIOS/{missing}, which is not on the card");
            self.hud.toast(Toast::NoBios, self.now());
            self.refuse();
            return;
        }
        eprintln!(
            "slot: launch: insert {cart} at {:.1} ms",
            crate::boot_time::ms()
        );
        self.play_held = None;
        self.refusal = None;
        self.refused_from = None;
        self.phase = Phase::Inserting {
            cart,
            t: 0.0,
            core_ready: false,
            resumed: false,
            clean,
        };
    }

    /// The first file a platform's core cannot start without that is not on the card.
    fn missing_bios(&self, platform: Platform) -> Option<&'static str> {
        let root = self.root.as_ref()?;
        crate::root::missing_bios(root, platform)
    }

    pub fn starting_clean(&self) -> bool {
        matches!(self.phase, Phase::Inserting { clean: true, .. })
    }

    fn play_hold(&mut self) {
        let Some(at) = self.play_held else {
            return;
        };
        if !self.on_shelf() {
            self.play_held = None;
            return;
        }
        if self.now().saturating_sub(at) >= PLAY_HOLD_MS {
            self.insert(true);
        }
    }

    fn eject(&mut self) {
        if self.single_cart() {
            return self.refuse();
        }
        self.eject_overlay = match self.layout() {
            Layout::Framed => self.overlay_slide(),
            Layout::Native => 0.0,
        };
        self.in_game_menu = None;
        let cart = match &mut self.phase {
            Phase::Playing { cart } | Phase::Inserting { cart, .. } => std::mem::take(cart),
            _ => return,
        };
        self.end_link();
        self.close_game_menu();
        self.flush_eject(&cart);
        self.pending = None;
        self.refusal = None;
        self.refused_from = None;
        self.phase = Phase::Ejecting {
            cart,
            t: -EJECT_HOLD_S,
        };
    }

    fn flush_eject(&mut self, stem: &str) {
        self.settle_saves();
        let (Some(root), Some(snapshot)) = (&self.root, &self.snapshot) else {
            return;
        };
        let Some(state) = snapshot.state() else {
            eprintln!("slot: eject: the core gave up no state");
            return;
        };
        let (state, sav) = trusted_write(snapshot.as_ref(), state, "eject");
        let state = state.filter(|_| self.state.eject_save);
        // The card write runs beside the eject, which a disc's tens of MB held up otherwise.
        let (root, platform, core, stem) =
            (root.clone(), self.platform, self.core, stem.to_owned());
        let failing = self.card_writes.clone();
        let write = move || {
            let written = persist::eject(
                &root,
                platform,
                core,
                &stem,
                state.as_deref(),
                sav.as_deref(),
            );
            if let Err(e) = &written {
                eprintln!("slot: eject: {e}");
            }
            failing.wrote("eject", &written);
        };
        match std::thread::Builder::new()
            .name("slot-eject-save".into())
            .spawn(write)
        {
            Ok(h) => self.pending_save = Some(h),
            Err(e) => eprintln!("slot: eject: no writer thread: {e}"),
        }
        self.state.cart = None;
        self.state.cart_platform = None;
    }

    fn doze(&mut self) {
        if self.link_active() {
            self.end_link();
        }
        self.close_game_menu();
        self.core_picker = None;
        if matches!(self.phase, Phase::Doze { .. }) {
            return;
        }
        self.state_writer.flush();
        self.flush_resume();
        let cart = match &mut self.phase {
            Phase::Playing { cart } | Phase::Polaroids { cart } => Some(std::mem::take(cart)),
            _ => None,
        };
        if std::mem::take(&mut self.states_from_menu) {
            self.in_game_menu = Some(QuickRow::States);
        }
        self.polaroids = None;
        self.phase = Phase::Doze { cart };
        self.dozed_at = self.now();
        self.cool_radio();
        if let Some(power) = &mut self.power {
            power.on_close();
        }
    }

    fn wake(&mut self) {
        let Phase::Doze { cart } = &mut self.phase else {
            return;
        };
        self.phase = match cart.take() {
            Some(cart) => Phase::Playing { cart },
            None => Phase::Shelf,
        };
        if let Some(power) = &mut self.power {
            power.on_open();
        }
    }

    pub fn on_doze_timeout(&mut self) {
        if !matches!(self.phase, Phase::Doze { .. }) {
            return;
        }
        self.begin_power_off();
    }

    fn power_press(&mut self) {
        if std::mem::take(&mut self.woke_on_press) {
            return;
        }
        match self.phase {
            Phase::Doze { .. } => self.wake(),
            _ => self.doze(),
        }
    }

    fn hold_power(&mut self) {
        self.flush_resume();
        self.begin_power_off();
    }

    fn begin_restart(&mut self) {
        if self.shutting_down() {
            return;
        }
        self.remember_shelf();
        self.restarting = true;
        self.act_at = self.now() + SHUTDOWN_SHOW_MS;
        self.set_led(LedState::Off);
    }

    fn begin_reload(&mut self) {
        if self.shutting_down() {
            return;
        }
        self.remember_shelf();
        self.reloading = true;
        self.act_at = self.now() + SHUTDOWN_SHOW_MS;
        self.set_led(LedState::Off);
    }

    fn open_core_picker(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(cart) = self.shelf().carts.get(self.shelf().index) else {
            return;
        };
        let cores = cart.platform.spec().cores;
        if cores.len() < 2 {
            return;
        }
        // The GBA's board seats its own two cores; once another is downloaded it has a list.
        if cart.platform != Platform::Gba || cores[2..].iter().any(|c| c.installed(&root)) {
            let chosen = slot_store::core_for_platform(&root, &cart.stem, cart.platform);
            self.phase = Phase::Cores {
                cursor: cores.iter().position(|c| *c == chosen).unwrap_or(0),
            };
            self.shelf_mut().release_hold();
            self.play_held = None;
            return;
        }
        let seat = slot_store::core_for_platform(&root, &cart.stem, cart.platform);
        let now = self.now();
        let mut picker = CorePicker::open(seat, now);
        if self.core_faces_ready() {
            picker.start(now);
        }
        self.core_picker = Some(picker);
        self.shelf_mut().release_hold();
        self.play_held = None;
    }

    /// Whether `core` can be opened: it came with unslot or has been downloaded.
    fn core_on_card(&self, core: Core) -> bool {
        self.root
            .as_deref()
            .map_or(!core.download_only(), |root| core.installed(root))
    }

    /// The game on the shelf and the cores its system has.
    fn shelf_cores(&self) -> Option<(&Cart, &'static [slot_store::Core])> {
        let cart = self.shelf().carts.get(self.shelf().index)?;
        Some((cart, cart.platform.spec().cores))
    }

    fn cores_input(&mut self, cursor: usize, action: Action) {
        let Some((_, cores)) = self.shelf_cores() else {
            self.phase = Phase::Shelf;
            return;
        };
        let n = cores.len();
        let cursor = cursor.min(n - 1);
        self.phase = match action {
            Action::GbaDown(Btn::Up) => Phase::Cores {
                cursor: (cursor + n - 1) % n,
            },
            Action::GbaDown(Btn::Down) => Phase::Cores {
                cursor: (cursor + 1) % n,
            },
            Action::GbaDown(Btn::A) if !self.core_on_card(cores[cursor]) => {
                self.with_cores(|c| c.download(cores[cursor]));
                Phase::Cores { cursor }
            }
            Action::GbaDown(Btn::Y) if !self.core_on_card(cores[cursor]) => return,
            Action::GbaDown(Btn::A) => {
                self.write_core(cores[cursor], false);
                Phase::Shelf
            }
            Action::GbaDown(Btn::Y) => {
                self.write_core(cores[cursor], true);
                Phase::Shelf
            }
            Action::GbaDown(Btn::B | Btn::Start) | Action::QuickMenu => Phase::Shelf,
            _ => return,
        };
    }

    fn cores_page(&self, cursor: usize) -> Option<ListPage> {
        let (cart, cores) = self.shelf_cores()?;
        let (chosen, system) = self.root.as_deref().map_or((cores[0], cores[0]), |root| {
            (
                slot_store::core_for_platform(root, &cart.stem, cart.platform),
                slot_store::core_for_system(root, cart.platform),
            )
        });
        let cursor = cursor.min(cores.len() - 1);
        let rows = cores
            .iter()
            .map(|c| ListRow {
                label: c.text().into(),
                detail: match () {
                    _ if *c == chosen => "In use".into(),
                    _ if *c == system => "Default".into(),
                    _ if !self.core_on_card(*c) => {
                        core_state(&self.cores_view, *c).unwrap_or_else(|| "Download".into())
                    }
                    _ => String::new(),
                },
                bars: None,
                icon: None,
            })
            .collect();
        let core = cores[cursor];
        if !self.core_on_card(core) {
            let pending = self.cores_view.pending(core);
            return Some(ListPage {
                title: "Core".into(),
                status: slot_ui::label_text(cart),
                cursor,
                rows,
                legends: with_action((!pending).then_some("Download")),
            });
        }
        let mut legends = with_action((core != chosen).then_some("This Game"));
        if core != system {
            let all = format!("All {}", cart.platform.short_name());
            legends.push(Legend::new(ButtonLegend::Y, &all));
        }
        Some(ListPage {
            title: "Core".into(),
            status: slot_ui::label_text(cart),
            cursor,
            rows,
            legends,
        })
    }

    /// The keyboard, for a game's name: the last search's to start from.
    fn open_search(&mut self, again: bool) {
        let query = self.search.as_ref().map_or("", |s| s.query.as_str());
        self.phase = Phase::Keyboard {
            keys: Keyboard::new("Search for a game", query, false, 1, 48),
            typing: Typing::Search { again },
            cursor: 0,
        };
        self.shelf_mut().release_hold();
        self.play_held = None;
    }

    /// Looks for `query` on every system's shelf and shows what it found.
    fn search_for(&mut self, query: &str) {
        let carts: Vec<&Cart> = self.carts().collect();
        let names: Vec<String> = carts.iter().map(|c| slot_ui::label_text(c)).collect();
        let found = crate::search::matches(names.iter().map(String::as_str), query)
            .into_iter()
            .map(|i| (carts[i].platform, carts[i].stem.clone()))
            .collect();
        self.search = Some(Search {
            query: query.trim().to_string(),
            found,
        });
        self.phase = Phase::Search { cursor: 0 };
        self.make_search_icons(0);
    }

    fn found_cart(&self, (platform, stem): &(Platform, String)) -> Option<&Cart> {
        self.carts()
            .find(|c| c.platform == *platform && c.stem == *stem)
    }

    /// Makes the pictures for the rows in view around `cursor` that do not have one yet: the
    /// game's cart, card or disc with its label, as the shelf shows it, shrunk.
    fn make_search_icons(&mut self, cursor: usize) {
        let (Some(search), Some(root)) = (&self.search, &self.root) else {
            return;
        };
        let page = ListPage {
            rows: vec![ListRow::default(); search.found.len()],
            cursor,
            ..ListPage::default()
        };
        let first = page.first_shown();
        let wanted: Vec<(Platform, String)> = search
            .found
            .iter()
            .skip(first)
            .take(slot_ui::LIST_ROWS)
            .filter(|key| !self.search_icons.contains_key(*key))
            .cloned()
            .collect();
        let root = root.clone();
        for key in wanted {
            let Some(cart) = self.found_cart(&key) else {
                continue;
            };
            let face = slot_ui::cart_face_with(cart, crate::label_cache::label_art(&root, cart));
            self.search_icons.insert(key, ListIcon::of(&face));
        }
    }

    fn search_input(&mut self, cursor: usize, action: Action) {
        let n = self.search.as_ref().map_or(0, |s| s.found.len());
        let cursor = cursor.min(n.saturating_sub(1));
        let cursor = match action {
            Action::GbaDown(Btn::Up) if n > 0 => (cursor + n - 1) % n,
            Action::GbaDown(Btn::Down) if n > 0 => (cursor + 1) % n,
            Action::GbaDown(Btn::A) if n > 0 => {
                let key = self.search.as_ref().map(|s| s.found[cursor].clone());
                if let Some(key) = key {
                    self.go_to_game(&key);
                }
                return;
            }
            Action::GbaDown(Btn::Y) => return self.open_search(true),
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                self.phase = Phase::Shelf;
                return;
            }
            _ => return,
        };
        self.make_search_icons(cursor);
        self.phase = Phase::Search { cursor };
    }

    /// Turns to the game's own system's shelf with the game in the slot.
    fn go_to_game(&mut self, (platform, stem): &(Platform, String)) {
        let at = self
            .shelves
            .iter()
            .position(|(rack, _)| *rack == Rack::Of(*platform));
        let Some(at) = at else {
            return;
        };
        let Some(i) = self.shelves[at]
            .1
            .carts
            .iter()
            .position(|c| c.stem == *stem)
        else {
            return;
        };
        self.phase = Phase::Shelf;
        if at != self.shelf_at {
            self.shelf_mut().release_hold();
            self.shelf_at = at;
            self.shelf_platform = slot_ui::Printed::default();
            self.slot_letter = None;
            self.shelf_named = None;
            self.name_pending = true;
        }
        self.shelves[at].1.select(i);
        self.remember_shelf();
    }

    fn search_page(&self, cursor: usize) -> ListPage {
        let Some(search) = &self.search else {
            return ListPage::default();
        };
        let rows: Vec<ListRow> = search
            .found
            .iter()
            .filter_map(|key| {
                let cart = self.found_cart(key)?;
                Some(ListRow {
                    label: slot_ui::label_text(cart),
                    detail: cart.platform.short_name().into(),
                    bars: None,
                    icon: self.search_icons.get(key).cloned(),
                })
            })
            .collect();
        let status = match rows.len() {
            0 => format!("No games match \u{201c}{}\u{201d}", search.query),
            1 => format!("1 game matches \u{201c}{}\u{201d}", search.query),
            n if n == crate::search::MOST => {
                format!(
                    "The first {n} games matching \u{201c}{}\u{201d}",
                    search.query
                )
            }
            n => format!("{n} games match \u{201c}{}\u{201d}", search.query),
        };
        let mut legends = vec![back_legend(), Legend::new(ButtonLegend::Y, "New Search")];
        if !rows.is_empty() {
            legends.push(Legend::new(ButtonLegend::A, "Go to Game"));
        }
        ListPage {
            title: "Search".into(),
            status,
            cursor: cursor.min(rows.len().saturating_sub(1)),
            rows,
            legends,
        }
    }

    fn core_faces_ready(&self) -> bool {
        self.core_faces_stem
            .as_deref()
            .is_some_and(|stem| self.selected_stem() == Some(stem))
    }

    fn core_picker_input(&mut self, action: Action) {
        let press = match action {
            Action::GbaDown(Btn::Left) | Action::ShelfLeft => Press::Left,
            Action::GbaDown(Btn::Right) | Action::ShelfRight => Press::Right,
            Action::GbaDown(Btn::A) => Press::Keep,
            Action::GbaDown(Btn::Y) => Press::KeepAll,
            Action::GbaDown(Btn::B) => Press::Back,
            _ => return,
        };
        let now = self.now();
        let Some(picker) = &mut self.core_picker else {
            return;
        };
        match picker.press(press, now) {
            Outcome::Write(core) => self.write_core(core, false),
            Outcome::WriteAll(core) => self.write_core(core, true),
            Outcome::Nothing | Outcome::Refused => {}
        }
    }

    fn open_game_menu(&mut self) {
        if self.game_menu.is_some() {
            return;
        }
        if self.link_active() {
            return self.refuse();
        }
        if self.core == Core::Mgba {
            let wireless = self
                .seated()
                .is_some_and(|stem| self.link_mode(stem).0 == LinkKind::Wireless);
            if wireless {
                self.hud.toast(Toast::NeedsGpsp, self.now());
                return;
            }
            self.link_hardware = LinkKind::Cable;
            self.warm_radio();
            self.game_menu = Some(GameMenu::Pick(self.last_role));
            return;
        }
        if !self.platform.spec().link {
            self.hud.toast(Toast::NoLink, self.now());
            return;
        }
        let carried = self
            .seated()
            .and_then(|stem| self.auto_link(stem))
            .is_some_and(|(cart, _)| link_carried(&cart.code, &cart.title));
        if !carried {
            self.hud.toast(Toast::NoLink, self.now());
            return;
        }
        if self.core != Core::Gpsp {
            self.hud.toast(Toast::NeedsGpsp, self.now());
            return;
        }
        let hardware = self
            .seated()
            .map_or(LinkKind::Cable, |stem| self.link_mode(stem).0);
        self.link_hardware = hardware;
        self.warm_radio();
        self.game_menu = Some(GameMenu::Pick(self.last_role));
    }

    fn game_menu_shortcut(&mut self) {
        let Some(client_id) = self.link_client_id() else {
            return self.open_game_menu();
        };
        let now = self.now();
        self.game_menu = Some(GameMenu::Linked {
            role: LinkRow::from_client_id(client_id),
            worked: now,
            since: now,
            opened: true,
        });
    }

    fn end_link_from_menu(&mut self) {
        let role = self
            .link_client_id()
            .map_or(self.last_role, LinkRow::from_client_id);
        self.end_link();
        self.hud.toast(Toast::LinkEnded, self.now());
        self.unplug(role);
    }

    fn unplug(&mut self, role: LinkRow) {
        self.game_menu = Some(GameMenu::Unplug {
            role,
            since: self.now(),
        });
    }

    pub fn set_radio_jobs(&mut self, jobs: Box<dyn RadioJobs>) {
        self.radio = jobs;
    }

    fn game_menu_input(&mut self, action: Action) {
        let Some(menu) = self.game_menu else {
            return;
        };
        match menu {
            GameMenu::Pick(role) => match action {
                Action::GbaDown(Btn::Left) | Action::GbaDown(Btn::Right) => {
                    self.last_role = role.other();
                    self.game_menu = Some(GameMenu::Pick(role.other()));
                }
                Action::GbaDown(Btn::Select) => self.switch_hardware(),
                Action::GbaDown(Btn::A) => self.pick_link(role),
                Action::GbaDown(Btn::B) | Action::GameMenu => self.close_game_menu(),
                _ => {}
            },
            GameMenu::Working { .. } => {
                if action == Action::GbaDown(Btn::B) {
                    if let Some(starting) = &mut self.starting {
                        starting.starter.cancel();
                    }
                    if let Some(reload) = &mut self.reload {
                        reload.cancelled = true;
                    }
                }
            }
            GameMenu::Linked { opened: true, .. } => match action {
                Action::GbaDown(Btn::A) => self.end_link_from_menu(),
                Action::GbaDown(Btn::B) | Action::GameMenu => self.close_game_menu(),
                _ => {}
            },
            GameMenu::Linked { .. } => {}
            GameMenu::Failed { .. } => {
                if matches!(
                    action,
                    Action::GbaDown(Btn::A) | Action::GbaDown(Btn::B) | Action::GameMenu
                ) {
                    self.close_game_menu();
                }
            }
            GameMenu::Unplug { .. } => {}
        }
    }

    pub fn start_link(&mut self, starter: LinkStarter, client_id: u16) {
        self.start_link_from(starter, client_id, self.now());
    }

    fn start_link_from(&mut self, starter: LinkStarter, client_id: u16, since: Millis) {
        if let Some(mut old) = self.starting.replace(LinkStarting { starter, client_id }) {
            old.starter.cancel();
        }
        self.game_menu = Some(GameMenu::Working {
            role: LinkRow::from_client_id(client_id),
            step: LinkStep::Radio,
            since,
        });
    }

    fn switch_hardware(&mut self) {
        let Some(stem) = self.seated().map(str::to_string) else {
            return;
        };
        if !self.link_switchable(&stem) {
            return self.refuse();
        }
        let other = self.link_hardware.other();
        self.link_hardware = other;
        self.link_choices.insert(stem, other);
    }

    fn link_switchable(&self, stem: &str) -> bool {
        self.auto_link(stem).is_some_and(|(cart, auto)| {
            serial_option(self.link_hardware.other(), auto, &cart.code, &cart.title)
                != serial_option(self.link_hardware, auto, &cart.code, &cart.title)
        })
    }

    fn pick_link(&mut self, role: LinkRow) {
        let Some(stem) = self.seated().map(str::to_string) else {
            return;
        };
        let loaded = self.link_loaded.unwrap_or("auto");
        if self.core == Core::Mgba {
            let player = role.client_id() as u8;
            if self.link_player == Some(player) {
                return self.start_link(
                    LinkStarter::spawn(role.role(), link_port()),
                    role.client_id(),
                );
            }
            if self.snapshot.as_ref().is_some_and(|s| !s.resume_trusted()) {
                return self.refuse();
            }
            self.link_player = Some(player);
            self.link_reload = Some((stem.clone(), loaded));
            self.reload = Some(Reload {
                stem,
                role,
                cancelled: false,
                from: self.link_hardware,
                from_serial: loaded,
                fallback: false,
            });
            self.game_menu = Some(GameMenu::Working {
                role,
                step: LinkStep::Radio,
                since: self.now(),
            });
            return;
        }
        let (_, serial) = self.link_mode(&stem);
        if serial == loaded {
            return self.start_link(
                LinkStarter::spawn(role.role(), link_port()),
                role.client_id(),
            );
        }
        if self.snapshot.as_ref().is_some_and(|s| !s.resume_trusted()) {
            return self.refuse();
        }
        self.link_reload = Some((stem.clone(), serial));
        self.reload = Some(Reload {
            stem,
            role,
            cancelled: false,
            from: self.link_hardware.other(),
            from_serial: loaded,
            fallback: false,
        });
        self.game_menu = Some(GameMenu::Working {
            role,
            step: LinkStep::Radio,
            since: self.now(),
        });
    }

    fn refuse_seated(&mut self) {
        self.close_game_menu();
        self.pending = None;
        let caught = self.seat();
        let cart = match &mut self.phase {
            Phase::Playing { cart } => Some(std::mem::take(cart)),
            Phase::Doze { cart } => {
                *cart = None;
                None
            }
            _ => None,
        };
        if let Some(cart) = cart {
            self.refuse_out(cart, caught);
        }
    }

    fn close_game_menu(&mut self) {
        self.game_menu = None;
        if let Some(mut starting) = self.starting.take() {
            starting.starter.cancel();
        }
        if !self.link_active() {
            self.cool_radio();
        }
        let uncollected =
            self.link_reload.is_some() && self.reload.as_ref().is_some_and(|r| !r.fallback);
        if uncollected {
            self.link_reload = None;
            self.reload = None;
        } else if let Some(reload) = &mut self.reload {
            reload.cancelled = true;
        }
    }

    fn working_role(&self, client_id: u16) -> (LinkRow, Millis) {
        match self.game_menu {
            Some(GameMenu::Working { role, since, .. }) => (role, since),
            _ => (LinkRow::from_client_id(client_id), self.now()),
        }
    }

    fn poll_link(&mut self) {
        let Some(mut starting) = self.starting.take() else {
            return;
        };
        match starting.starter.poll() {
            None => self.starting = Some(starting),
            Some(LinkProgress::At(step)) => {
                if let Some(GameMenu::Working { role, since, .. }) = self.game_menu {
                    self.game_menu = Some(GameMenu::Working { role, step, since });
                }
                self.starting = Some(starting);
            }
            Some(LinkProgress::Ready(link)) => {
                let (role, worked) = self.working_role(starting.client_id);
                self.game_menu = Some(GameMenu::Linked {
                    role,
                    worked,
                    since: self.now(),
                    opened: false,
                });
                self.begin_link(starting.client_id);
                self.link_transport = Some((starting.client_id, Box::new(link)));
            }
            Some(LinkProgress::Failed(LinkFail::Cancelled)) => self.game_menu = None,
            Some(LinkProgress::Failed(fail)) => {
                let (role, worked) = self.working_role(starting.client_id);
                self.game_menu = Some(GameMenu::Failed {
                    role,
                    fail,
                    worked,
                    since: self.now(),
                });
            }
        }
    }

    /// Sets the core for the game on the shelf, or for every game on its system when `all`,
    /// which leaves this one following the system.
    fn write_core(&self, core: Core, all: bool) {
        let (Some(root), Some(cart)) = (
            self.root.clone(),
            self.shelf().carts.get(self.shelf().index),
        ) else {
            return;
        };
        if all {
            if let Err(e) = slot_store::write_system_core(&root, cart.platform, core) {
                eprintln!("slot: core: could not write system_core.txt: {e}");
                return;
            }
        }
        if let Err(e) = slot_store::write_game_core(&root, &cart.stem, cart.platform, core) {
            eprintln!("slot: core: could not write selected_core.txt: {e}");
        }
    }

    fn begin_power_off(&mut self) {
        if self.powering_off {
            return;
        }
        if self.link_active() {
            self.end_link();
        }
        self.close_game_menu();
        self.remember_shelf();
        self.powering_off = true;
        self.act_at = self.now() + SHUTDOWN_SHOW_MS;
        self.set_led(LedState::Off);
    }

    pub fn on_battery(&mut self, b: Battery) {
        if b.percent > BATTERY_CRITICAL || self.powering_off {
            return;
        }
        if matches!(b.charge, Charge::Charging | Charge::Full) {
            return;
        }
        self.flush_resume();
        self.begin_power_off();
    }

    fn doze_expired(&self) -> bool {
        if self.link_active() {
            return false;
        }
        let (Phase::Doze { .. }, Some(power)) = (&self.phase, &self.power) else {
            return false;
        };
        self.now().saturating_sub(self.dozed_at) >= power.timeout().as_millis() as Millis
    }

    pub fn settle_saves(&mut self) {
        self.state_writer.flush();
        if let Some(h) = self.pending_save.take() {
            let _ = h.join();
        }
    }

    fn autosave(&mut self) {
        self.autosave_at = self.now() + AUTOSAVE_MS;
        // Taking a Dreamcast state holds the game up for a quarter second, and writing it can stall
        // the game's reads from the card for seconds. It is still written on sleep, eject, power
        // off and a flat battery.
        if self.platform == Platform::Dreamcast {
            return;
        }
        self.settle_saves();
        let (Some(root), Some(snapshot), Some(cart)) = (&self.root, &self.snapshot, self.seated())
        else {
            return;
        };
        let Some(state) = snapshot.state() else {
            eprintln!("slot: autosave: the core gave up no state");
            return;
        };
        let (state, sav) = trusted_write(snapshot.as_ref(), state, "autosave");
        let (root, platform, core, cart) =
            (root.clone(), self.platform, self.core, cart.to_owned());
        let failing = self.card_writes.clone();
        let write = move || {
            let written = persist::flush(
                &root,
                platform,
                core,
                &cart,
                state.as_deref(),
                sav.as_deref(),
            );
            if let Err(e) = &written {
                eprintln!("slot: autosave: {e}");
            }
            failing.wrote("autosave", &written);
        };
        match std::thread::Builder::new()
            .name("slot-autosave".into())
            .spawn(write)
        {
            Ok(h) => self.pending_save = Some(h),
            Err(e) => eprintln!("slot: autosave: no writer thread: {e}"),
        }
    }

    pub fn flush_resume(&mut self) {
        self.settle_saves();
        self.autosave_at = self.now() + AUTOSAVE_MS;
        let (Some(root), Some(snapshot), Some(cart)) = (&self.root, &self.snapshot, self.seated())
        else {
            return;
        };
        let Some(state) = snapshot.state() else {
            eprintln!("slot: flush: the core gave up no state");
            return;
        };
        let (state, sav) = trusted_write(snapshot.as_ref(), state, "flush");
        let written = persist::flush(
            root,
            self.platform,
            self.core,
            cart,
            state.as_deref(),
            sav.as_deref(),
        );
        if let Err(e) = &written {
            eprintln!("slot: flush: {e}");
        }
        self.card_writes.wrote("flush", &written);
    }

    fn ring(&self) -> Option<StateRing> {
        let (Some(root), Some(cart)) = (&self.root, self.seated()) else {
            return None;
        };
        Some(StateRing::new(root, self.platform, self.core, cart))
    }

    fn seated(&self) -> Option<&str> {
        match &self.phase {
            Phase::Playing { cart } | Phase::Polaroids { cart } => Some(cart),
            _ => None,
        }
    }

    fn entries(&self) -> Vec<StateEntry> {
        self.ring().and_then(|r| r.list().ok()).unwrap_or_default()
    }

    fn open_polaroids(&mut self) {
        if self.link_active() {
            return self.refuse();
        }
        let entries = self.entries();
        if entries.is_empty() {
            return self.refuse();
        }
        let Phase::Playing { cart } = &mut self.phase else {
            return;
        };
        let cart = std::mem::take(cart);
        let mut p = Polaroids::new(entries);
        p.set_undo(self.undo_label());
        self.polaroids = Some(p);
        self.phase = Phase::Polaroids { cart };
        self.push_hint_faces();
    }

    fn close_polaroids(&mut self) {
        let Phase::Polaroids { cart } = &mut self.phase else {
            return;
        };
        let cart = std::mem::take(cart);
        self.polaroids = None;
        self.phase = Phase::Playing { cart };
        if std::mem::take(&mut self.states_from_menu) {
            self.in_game_menu = Some(QuickRow::States);
        }
    }

    fn load_selected(&mut self) {
        let state = self
            .polaroids
            .as_ref()
            .and_then(|p| p.selected())
            .map(|e| e.state.clone());
        let refused = state.map(|state| self.load_file(&state)) == Some(false);
        if !refused {
            self.states_from_menu = false;
            self.close_polaroids();
        }
    }

    fn delete_selected(&mut self) {
        let stamp = self
            .polaroids
            .as_ref()
            .and_then(|p| p.selected())
            .map(|e| e.stamp.clone());
        let (Some(stamp), Some(ring)) = (stamp, self.ring()) else {
            return;
        };
        if let Err(e) = ring.remove(&stamp) {
            eprintln!("slot: delete: {e}");
            return;
        }
        if self.undo_targets(&stamp) {
            self.pending = None;
        }
        let Some(p) = &mut self.polaroids else {
            return;
        };
        p.remove_selected();
        if p.is_empty() {
            self.close_polaroids();
        }
    }

    fn undo_targets(&self, stamp: &str) -> bool {
        match self.pending.as_ref() {
            Some((PendingUndo::Save { stamp: pending, .. }, _)) => pending == stamp,
            _ => false,
        }
    }

    fn load_newest(&mut self) {
        let Some(newest) = self.entries().first().map(|e| e.state.clone()) else {
            return self.refuse();
        };
        self.load_file(&newest);
    }

    fn load_file(&mut self, state: &Path) -> bool {
        if !self.may_load_state() {
            self.refuse();
            return false;
        }
        let Some(snapshot) = &self.snapshot else {
            return false;
        };
        let bytes = match slot_store::read_state(state) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("slot: load: {e}");
                return false;
            }
        };
        let prior = snapshot.state();
        snapshot.load(bytes);
        self.hud.toast(Toast::StateLoaded, self.now());
        if let Some(prior) = prior {
            self.pending = Some((PendingUndo::Load { prior }, self.now()));
        }
        true
    }

    fn save_state(&mut self) {
        let (Some(ring), Some(snapshot)) = (self.ring(), &self.snapshot) else {
            return;
        };
        if !snapshot.resume_trusted() {
            eprintln!("slot: save: the core refused the resume it was given, not pushing a state");
            return self.refuse();
        }
        let Some(state) = snapshot.state() else {
            eprintln!("slot: save: the core gave up no state");
            return;
        };
        let thumb = snapshot.thumb().unwrap_or_default();
        let stamp = free_stamp(&ring, self.wall_secs());
        let evicted = doomed(&ring);
        if let Err(e) = ring.push(&state, &thumb, &stamp) {
            eprintln!("slot: save: {e}");
            return;
        }
        self.hud.toast(Toast::StateSaved, self.now());
        self.pending = Some((PendingUndo::Save { stamp, evicted }, self.now()));
    }

    pub fn undo_available(&self, now: Millis) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(_, at)| now.saturating_sub(*at) <= UNDO_GRACE_MS)
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        if !self.undo_available(self.now()) {
            return None;
        }
        match self.pending.as_ref()?.0 {
            PendingUndo::Save { .. } => Some("undo save"),
            PendingUndo::Load { .. } => Some("undo load"),
        }
    }

    pub fn set_legend_faces(&mut self, faces: Vec<TexId>) {
        self.legend_faces = faces;
        self.push_hint_faces();
    }

    pub fn set_undo_face(&mut self, face: Option<TexId>) {
        self.undo_face = face;
        self.push_hint_faces();
    }

    fn push_hint_faces(&mut self) {
        let mut faces = self.legend_faces.clone();
        faces.extend(self.undo_face);
        if let Some(p) = &mut self.polaroids {
            p.set_hint_faces(faces);
        }
    }

    pub fn set_icon_faces(&mut self, faces: Vec<TexId>) {
        self.hud.set_icons(faces);
    }

    pub fn set_toast_faces(&mut self, faces: Vec<TexId>) {
        self.hud.set_toasts(faces);
    }

    pub fn toast(&self) -> Option<Toast> {
        self.hud.said(self.now())
    }

    pub fn card_alarm(&self) -> bool {
        self.hud.alarmed()
    }

    pub fn undo(&mut self, now: Millis) {
        if !self.undo_available(now) {
            self.pending = None;
            return;
        }
        if matches!(&self.pending, Some((PendingUndo::Load { .. }, _))) && !self.may_load_state() {
            return self.refuse();
        }
        let Some((what, _)) = self.pending.take() else {
            return;
        };
        match what {
            PendingUndo::Save { stamp, evicted } => self.undo_save(&stamp, evicted),
            PendingUndo::Load { prior } => {
                if let Some(snapshot) = &self.snapshot {
                    snapshot.load(prior);
                }
            }
        }
        self.close_polaroids();
    }

    fn undo_save(&self, stamp: &str, evicted: Option<(String, Vec<u8>, Vec<u8>)>) {
        let Some(ring) = self.ring() else {
            return;
        };
        if let Err(e) = ring.remove(stamp) {
            eprintln!("slot: undo: {e}");
            return;
        }
        let Some((stamp, state, thumb)) = evicted else {
            return;
        };
        if let Err(e) = ring.push(&state, &thumb, &stamp) {
            eprintln!("slot: undo: {e}");
        }
    }

    pub fn polaroid_entries(&self) -> &[StateEntry] {
        match &self.polaroids {
            Some(p) => &p.entries,
            None => &[],
        }
    }

    pub fn set_polaroid_faces(&mut self, faces: Vec<TexId>) {
        if let Some(p) = &mut self.polaroids {
            p.set_faces(faces);
        }
    }

    pub fn polaroid_stamp(&self) -> Option<&str> {
        self.polaroids
            .as_ref()
            .and_then(|p| p.selected())
            .map(|e| e.stamp.as_str())
    }

    pub fn polaroid_title(&self, now: &str) -> String {
        self.polaroids
            .as_ref()
            .map_or_else(String::new, |p| p.title(now))
    }

    pub fn set_polaroid_title_face(&mut self, face: TexId) {
        if let Some(p) = &mut self.polaroids {
            p.set_title_face(Some(face));
        }
    }
}

fn trusted_write(
    snapshot: &dyn Snapshot,
    state: Vec<u8>,
    verb: &str,
) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let state = if snapshot.resume_trusted() {
        Some(state)
    } else {
        eprintln!(
            "slot: {verb}: the core refused the resume it was given, not overwriting the saved one"
        );
        None
    };
    let sav = snapshot.save_ram();
    let sav = if snapshot.save_ram_trusted() {
        sav
    } else {
        if sav.is_some() {
            eprintln!(
                "slot: {verb}: the core refused the save ram it was given, not overwriting the saved one"
            );
        }
        None
    };
    (state, sav)
}

fn up(level: u8, step: u8, max: u8) -> u8 {
    level.saturating_add(step).min(max)
}

/// B, which leaves every page for the one it was opened from.
/// The cores to download, a system's after the one before's, each once.
fn downloadable_cores() -> Vec<Core> {
    let mut cores = Vec::new();
    for p in Platform::ALL {
        for c in p.spec().cores {
            if c.download_only() && !cores.contains(c) {
                cores.push(*c);
            }
        }
    }
    cores
}

/// How a downloaded core stands, when it is on the card or on its way.
fn core_state(v: &CoresView, core: Core) -> Option<String> {
    match v.downloading {
        Some((c, pct)) if c == core => Some(format!("{pct}%")),
        _ if v.queued.contains(&core) => Some("Waiting".into()),
        _ if v.outdated(core) => Some("Update".into()),
        _ if v.installed(core) => Some("Downloaded".into()),
        _ => None,
    }
}

fn back_legend() -> Legend {
    Legend::new(ButtonLegend::B, "Back")
}

/// Back, and A for `action` when there is one.
fn with_action(action: Option<&str>) -> Vec<Legend> {
    let mut legends = vec![back_legend()];
    legends.extend(action.map(|a| Legend::new(ButtonLegend::A, a)));
    legends
}

fn shader_value(shader: Shader) -> QuickValue {
    match shader {
        Shader::Off => QuickValue::Off,
        Shader::Lcd3x => QuickValue::Lcd3x,
        Shader::Grid => QuickValue::Grid,
        Shader::Dot => QuickValue::Dot,
        Shader::Simpletex => QuickValue::Simpletex,
    }
}

pub fn runahead_for(core: Core, platform: Platform, frames: u8) -> u8 {
    match (core, platform) {
        (Core::Mgba, Platform::Gba) => frames,
        _ => 0,
    }
}

fn tint_alpha(tint: WallpaperTint) -> f32 {
    match tint {
        WallpaperTint::Off => 0.0,
        WallpaperTint::Low => 0.25,
        WallpaperTint::Medium => 0.45,
        WallpaperTint::High => 0.65,
    }
}

fn ff_next(from: u8, right: bool) -> u8 {
    let n = FF_SPEEDS.len();
    let at = FF_SPEEDS.iter().position(|&v| v == from).unwrap_or(0);
    FF_SPEEDS[if right {
        (at + 1) % n
    } else {
        (at + n - 1) % n
    }]
}

fn clock_screen(utc: i64, offset_min: i16, from_menu: bool) -> Phase {
    Phase::SetClock {
        picker: ClockPicker::local(utc, offset_min),
        seed: utc - utc.rem_euclid(60),
        from_menu,
    }
}

fn system_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn doomed(ring: &StateRing) -> Option<(String, Vec<u8>, Vec<u8>)> {
    let entries = ring.list().ok()?;
    let oldest = entries.get(RING_MAX - 1)?;
    let (state, thumb) = ring.read(&oldest.stamp).ok()?;
    Some((oldest.stamp.clone(), state, thumb))
}

fn free_stamp(ring: &StateRing, now: i64) -> String {
    let taken: Vec<String> = ring
        .list()
        .map(|l| l.into_iter().map(|e| e.stamp).collect())
        .unwrap_or_default();
    let newest = taken.iter().filter_map(|s| parse_stamp(s)).max();
    let mut secs = newest.map_or(now, |n| now.max(n + 1));
    let mut stamp = format_stamp(secs);
    while taken.contains(&stamp) {
        secs += 1;
        stamp = format_stamp(secs);
    }
    stamp
}
