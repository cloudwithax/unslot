use std::path::{Path, PathBuf};

use crate::atomic::atomic_write;
use crate::gb_palette::GbPalette;
use crate::platform::Platform;

pub const BRIGHTNESS_MAX: u8 = 9;
pub const BLUE_LIGHT_MAX: u8 = 9;
pub const VOLUME_MAX: u8 = 100;

pub const UTC_OFFSET_MIN: i16 = -720;
pub const UTC_OFFSET_MAX: i16 = 840;

pub const FF_SPEEDS: [u8; 4] = [2, 3, 4, 6];

pub const FF_SPEED_DEFAULT: u8 = 6;

pub const RUN_AHEAD_MAX: u8 = 1;

pub const RECENTS_MAX: usize = 10;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Recent {
    pub platform: Platform,
    pub stem: String,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

impl Appearance {
    pub fn light(self) -> bool {
        matches!(self, Appearance::Light)
    }

    pub fn name(self) -> &'static str {
        match self {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
        }
    }

    pub fn parse(name: &str) -> Option<Appearance> {
        match name {
            "dark" => Some(Appearance::Dark),
            "light" => Some(Appearance::Light),
            _ => None,
        }
    }

    pub fn toggle(self) -> Appearance {
        match self {
            Appearance::Dark => Appearance::Light,
            Appearance::Light => Appearance::Dark,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum WallpaperTint {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl WallpaperTint {
    pub const ALL: [WallpaperTint; 4] = [
        WallpaperTint::Off,
        WallpaperTint::Low,
        WallpaperTint::Medium,
        WallpaperTint::High,
    ];

    pub fn key(self) -> &'static str {
        match self {
            WallpaperTint::Off => "off",
            WallpaperTint::Low => "low",
            WallpaperTint::Medium => "medium",
            WallpaperTint::High => "high",
        }
    }

    pub fn parse(value: &str) -> Option<WallpaperTint> {
        WallpaperTint::ALL.into_iter().find(|t| t.key() == value)
    }

    pub fn step(self, right: bool) -> WallpaperTint {
        let n = WallpaperTint::ALL.len();
        let at = WallpaperTint::ALL
            .iter()
            .position(|t| *t == self)
            .unwrap_or(0);
        WallpaperTint::ALL[if right {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }]
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum WallpaperPick {
    None,
    Random,
    #[default]
    Default,
    Blur,
    File(String),
}

impl WallpaperPick {
    pub fn key(&self) -> String {
        match self {
            WallpaperPick::None => "none".to_string(),
            WallpaperPick::Random => "random".to_string(),
            WallpaperPick::Default => "default".to_string(),
            WallpaperPick::Blur => "default_blur".to_string(),
            WallpaperPick::File(name) => name.clone(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            WallpaperPick::None => "None".to_string(),
            WallpaperPick::Random => "Random".to_string(),
            WallpaperPick::Default => "Default".to_string(),
            WallpaperPick::Blur => "DEFAULT BLUR".to_string(),
            WallpaperPick::File(name) => name
                .rsplit_once('.')
                .map(|(stem, _)| stem)
                .unwrap_or(name)
                .to_string(),
        }
    }

    pub fn parse(value: &str) -> WallpaperPick {
        match value {
            "none" => WallpaperPick::None,
            "random" => WallpaperPick::Random,
            "" | "default" => WallpaperPick::Default,
            "default_blur" => WallpaperPick::Blur,
            other => WallpaperPick::File(other.to_string()),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum BezelPick {
    None,
    Default,
    #[default]
    Ags102,
    File(String),
}

impl BezelPick {
    pub fn key(&self) -> String {
        match self {
            BezelPick::None => "none".to_string(),
            BezelPick::Default => "default".to_string(),
            BezelPick::Ags102 => "ags-102".to_string(),
            BezelPick::File(name) => name.clone(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            BezelPick::None => "None".to_string(),
            BezelPick::Default => "Default".to_string(),
            BezelPick::Ags102 => "AGS-102".to_string(),
            BezelPick::File(name) => name
                .rsplit_once('.')
                .map(|(stem, _)| stem)
                .unwrap_or(name)
                .to_string(),
        }
    }

    pub fn parse(value: &str) -> BezelPick {
        match value {
            "none" => BezelPick::None,
            "default" => BezelPick::Default,
            "" | "ags-102" => BezelPick::Ags102,
            other => BezelPick::File(other.to_string()),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlotState {
    pub cart: Option<String>,
    pub cart_platform: Option<Platform>,
    pub brightness: u8,
    pub blue_light: u8,
    pub volume: u8,
    pub volume_hp: u8,
    pub muted: bool,
    pub muted_hp: bool,
    pub clock_set: bool,
    pub utc_offset_min: i16,
    pub rumble: bool,
    pub ff_speed: u8,
    pub ff_sound: bool,
    /// One a platform, in `Platform::ALL` order.
    pub screens: [Screen; Platform::COUNT],
    pub eject_save: bool,
    pub turbo: bool,
    pub rewind: bool,
    pub runahead: u8,
    pub gb_palettes: bool,
    pub gb_palette: GbPalette,
    pub appearance: Appearance,
    pub wallpaper: WallpaperPick,
    pub wallpaper_tint: WallpaperTint,
    pub show_recents: bool,
    pub show_titles: bool,
    /// Whether the Wi-Fi is meant to be on, so it comes back on at boot.
    pub wifi: bool,
    /// Whether Bluetooth is meant to be on, likewise.
    pub bluetooth: bool,
    /// Whether BaseOS's SSH server is left running. It starts at boot whatever slot says.
    pub ssh: bool,
    /// Whether the card is shared over SMB.
    pub samba: bool,
    pub tailscale: bool,
    /// The WireGuard tunnel to bring up, by the name of its config in `Config/wireguard`.
    pub vpn: Option<String>,
    /// The root password slot sets for SSH and file sharing, or None to keep BaseOS's own. Kept
    /// as written, as BaseOS keeps its own in baseos.conf.
    pub password: Option<String>,
    pub recents: Vec<Recent>,
    pub favorites: Vec<Recent>,
    pub shelf: Option<String>,
    pub shelf_cart: Option<Recent>,
}

/// How one platform's picture is drawn.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Screen {
    pub shader: Shader,
    pub colour_correction: bool,
    pub bezel: BezelPick,
}

impl Screen {
    pub fn default_for(platform: Platform) -> Screen {
        Screen {
            shader: platform.spec().shader,
            colour_correction: false,
            bezel: BezelPick::Ags102,
        }
    }
}

impl SlotState {
    pub fn screen(&self, platform: Platform) -> &Screen {
        &self.screens[platform.index()]
    }

    pub fn screen_mut(&mut self, platform: Platform) -> &mut Screen {
        &mut self.screens[platform.index()]
    }

    pub fn played(&mut self, platform: Platform, stem: &str) {
        self.recents
            .retain(|r| !(r.platform == platform && r.stem == stem));
        self.recents.insert(
            0,
            Recent {
                platform,
                stem: stem.to_string(),
            },
        );
        self.recents.truncate(RECENTS_MAX);
    }

    pub fn is_favorite(&self, platform: Platform, stem: &str) -> bool {
        self.favorites
            .iter()
            .any(|f| f.platform == platform && f.stem == stem)
    }

    pub fn toggle_favorite(&mut self, platform: Platform, stem: &str) -> bool {
        if self.is_favorite(platform, stem) {
            self.favorites
                .retain(|f| !(f.platform == platform && f.stem == stem));
            return false;
        }
        self.favorites.push(Recent {
            platform,
            stem: stem.to_string(),
        });
        true
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Shader {
    Off,
    Lcd3x,
    Grid,
    Dot,
    Simpletex,
}

impl Shader {
    pub fn name(self) -> &'static str {
        match self {
            Shader::Off => "off",
            Shader::Lcd3x => "lcd3x",
            Shader::Grid => "grid",
            Shader::Dot => "dot",
            Shader::Simpletex => "simpletex",
        }
    }

    pub fn parse(name: &str) -> Option<Shader> {
        [
            Shader::Off,
            Shader::Lcd3x,
            Shader::Grid,
            Shader::Dot,
            Shader::Simpletex,
        ]
        .into_iter()
        .find(|s| s.name() == name)
    }

    pub fn step(self, choices: &[Shader], right: bool) -> Shader {
        let n = choices.len();
        let at = choices.iter().position(|&s| s == self).unwrap_or(0);
        choices[if right {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }]
    }
}

impl Default for SlotState {
    fn default() -> Self {
        SlotState {
            cart: None,
            cart_platform: None,
            brightness: 5,
            blue_light: 0,
            volume: 60,
            volume_hp: 60,
            muted: false,
            muted_hp: false,
            clock_set: false,
            utc_offset_min: 0,
            rumble: true,
            ff_speed: FF_SPEED_DEFAULT,
            ff_sound: false,
            screens: Platform::ALL.map(Screen::default_for),
            eject_save: true,
            turbo: true,
            rewind: true,
            runahead: 0,
            gb_palettes: false,
            gb_palette: GbPalette::DEFAULT,
            appearance: Appearance::Dark,
            wallpaper: WallpaperPick::Default,
            wallpaper_tint: WallpaperTint::Off,
            show_recents: false,
            show_titles: false,
            wifi: false,
            bluetooth: false,
            ssh: true,
            samba: false,
            tailscale: false,
            vpn: None,
            password: None,
            recents: Vec::new(),
            favorites: Vec::new(),
            shelf: None,
            shelf_cart: None,
        }
    }
}

fn state_path(root: &Path) -> PathBuf {
    root.join("Config").join("slot.state")
}

const RECENTS_FILE: &str = "recents.txt";
const FAVORITES_FILE: &str = "favorites.txt";

fn list_path(root: &Path, name: &str) -> PathBuf {
    root.join("Config").join(name)
}

pub fn read_slot_state(root: &Path) -> SlotState {
    let mut s: SlotState = std::fs::read(state_path(root))
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| parse(&s))
        .unwrap_or_default();
    s.recents = read_list(root, RECENTS_FILE);
    s.recents.truncate(RECENTS_MAX);
    s.favorites = read_list(root, FAVORITES_FILE);
    s
}

fn read_list(root: &Path, name: &str) -> Vec<Recent> {
    let text = std::fs::read_to_string(list_path(root, name)).unwrap_or_default();
    let mut list: Vec<Recent> = Vec::new();
    for r in text.lines().filter_map(recent_value) {
        if !list.contains(&r) {
            list.push(r);
        }
    }
    list
}

fn write_list(root: &Path, name: &str, list: &[Recent]) -> std::io::Result<()> {
    let text: String = list
        .iter()
        .map(|r| format!("{}/{}\n", r.platform.key(), r.stem))
        .collect();
    let path = list_path(root, name);
    if std::fs::read(&path).map_or(text.is_empty(), |old| old == text.as_bytes()) {
        return Ok(());
    }
    atomic_write(&path, text.as_bytes())
}

pub fn write_slot_state(root: &Path, s: &SlotState) -> std::io::Result<()> {
    let mut text = format!(
        "cart={}\ncart_platform={}\nbrightness={}\nblue_light={}\nvolume={}\nvolume_hp={}\nmuted={}\nmuted_hp={}\nclock_set={}\nutc_offset_min={}\nrumble={}\nff_speed={}\nff_sound={}\neject_save={}\nturbo={}\nrewind={}\nrunahead={}\ngb_palettes={}\ngb_palette={}\nappearance={}\nwallpaper={}\n",
        s.cart.as_deref().unwrap_or(""),
        s.cart_platform.map_or(String::new(), Platform::key),
        s.brightness,
        s.blue_light,
        s.volume,
        s.volume_hp,
        s.muted as u8,
        s.muted_hp as u8,
        s.clock_set as u8,
        s.utc_offset_min,
        s.rumble as u8,
        s.ff_speed,
        s.ff_sound as u8,
        s.eject_save as u8,
        s.turbo as u8,
        s.rewind as u8,
        s.runahead,
        s.gb_palettes as u8,
        s.gb_palette.core_name(),
        s.appearance.name(),
        s.wallpaper.key()
    );
    for p in Platform::ALL {
        let (key, screen) = (p.key(), s.screen(p));
        text.push_str(&format!(
            "colour_correction_{key}={}\n",
            screen.colour_correction as u8
        ));
        text.push_str(&format!("shader_{key}={}\n", screen.shader.name()));
        text.push_str(&format!("bezel_{key}={}\n", screen.bezel.key()));
    }
    text.push_str(&format!("wallpaper_tint={}\n", s.wallpaper_tint.key()));
    text.push_str(&format!("show_recents={}\n", s.show_recents as u8));
    text.push_str(&format!("show_titles={}\n", s.show_titles as u8));
    text.push_str(&format!("wifi={}\n", s.wifi as u8));
    text.push_str(&format!("bluetooth={}\n", s.bluetooth as u8));
    text.push_str(&format!("ssh={}\n", s.ssh as u8));
    text.push_str(&format!("samba={}\n", s.samba as u8));
    text.push_str(&format!("tailscale={}\n", s.tailscale as u8));
    text.push_str(&format!("vpn={}\n", s.vpn.as_deref().unwrap_or("")));
    text.push_str(&format!(
        "password={}\n",
        s.password.as_deref().unwrap_or("")
    ));
    text.push_str(&format!("shelf={}\n", s.shelf.as_deref().unwrap_or("")));
    text.push_str(&format!(
        "shelf_cart={}\n",
        s.shelf_cart.as_ref().map_or(String::new(), |r| format!(
            "{}/{}",
            r.platform.key(),
            r.stem
        ))
    ));
    atomic_write(&state_path(root), text.as_bytes())?;
    write_list(root, RECENTS_FILE, &s.recents)?;
    write_list(root, FAVORITES_FILE, &s.favorites)
}

fn parse(text: &str) -> Option<SlotState> {
    let mut cart = None;
    let mut cart_platform = None;
    let mut brightness = None;
    let mut blue_light = None;
    let mut volume = None;
    let mut volume_hp = None;
    let mut muted = None;
    let mut muted_hp = None;
    let mut clock_set = None;
    let mut utc_offset_min = None;
    let mut rumble = None;
    let mut ff_speed = None;
    let mut ff_sound = None;
    let mut colour_correction = None;
    let mut colour_corrections: [Option<bool>; Platform::COUNT] = [None; Platform::COUNT];
    let mut shaders: [Option<Shader>; Platform::COUNT] = [None; Platform::COUNT];
    let mut bezels: [Option<BezelPick>; Platform::COUNT] = Default::default();
    let mut eject_save = None;
    let mut turbo = None;
    let mut rewind = None;
    let mut runahead = None;
    let mut gb_palettes = None;
    let mut gb_palette = None;
    let mut appearance = None;
    let mut wallpaper = None;
    let mut wallpaper_tint = None;
    let mut show_recents = None;
    let mut show_titles = None;
    let mut wifi = None;
    let mut bluetooth = None;
    let (mut ssh, mut samba, mut tailscale) = (None, None, None);
    let (mut vpn, mut password) = (None, None);
    let mut shelf = None;
    let mut shelf_cart = None;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if let Some((setting, p)) = per_platform(key) {
            match setting {
                "colour_correction" => colour_corrections[p.index()] = flag(value),
                "shader" => {
                    shaders[p.index()] =
                        Shader::parse(value).filter(|s| p.spec().shaders.contains(s))
                }
                _ => bezels[p.index()] = Some(BezelPick::parse(value)),
            }
            continue;
        }
        match key {
            "cart" => cart = Some(value.to_string()),
            "cart_platform" => cart_platform = platform_value(value),
            "brightness" => brightness = Some(level(value, BRIGHTNESS_MAX)?),
            "blue_light" => blue_light = Some(level(value, BLUE_LIGHT_MAX)?),
            "volume" => volume = Some(level(value, VOLUME_MAX)?),
            "volume_hp" => volume_hp = Some(level(value, VOLUME_MAX)?),
            "muted" => muted = Some(level(value, 1)? == 1),
            "muted_hp" => muted_hp = Some(level(value, 1)? == 1),
            "clock_set" => clock_set = Some(level(value, 1)? == 1),
            "utc_offset_min" => utc_offset_min = Some(offset(value)?),
            "rumble" => rumble = flag(value),
            "ff_speed" => ff_speed = ff_speed_value(value),
            "ff_sound" => ff_sound = flag(value),
            "colour_correction" => colour_correction = flag(value),
            "eject_save" => eject_save = flag(value),
            "turbo" => turbo = flag(value),
            "rewind" => rewind = flag(value),
            "runahead" => runahead = level(value, RUN_AHEAD_MAX),
            "gb_palettes" => gb_palettes = flag(value),
            "gb_palette" => gb_palette = GbPalette::parse(value),
            "appearance" => appearance = Appearance::parse(value),
            "wallpaper" => wallpaper = Some(WallpaperPick::parse(value)),
            "wallpaper_tint" => wallpaper_tint = WallpaperTint::parse(value),
            "show_recents" => show_recents = flag(value),
            "show_titles" => show_titles = flag(value),
            "wifi" => wifi = flag(value),
            "bluetooth" => bluetooth = flag(value),
            "ssh" => ssh = flag(value),
            "samba" => samba = flag(value),
            "tailscale" => tailscale = flag(value),
            "vpn" => vpn = Some(value.to_string()).filter(|v| !v.is_empty()),
            "password" => password = Some(value.to_string()).filter(|v| !v.is_empty()),
            "shelf" => shelf = Some(value.to_string()).filter(|v| !v.is_empty()),
            "shelf_cart" => shelf_cart = recent_value(value),
            _ => {}
        }
    }
    let cart = cart?;
    let fallback = SlotState::default();
    let screens = Platform::ALL.map(|p| {
        let at = p.index();
        let parent = settings_parent(p).map(Platform::index);
        let own = Screen::default_for(p);
        Screen {
            colour_correction: colour_corrections[at]
                .or_else(|| parent.and_then(|q| colour_corrections[q]))
                .or(colour_correction)
                .unwrap_or(own.colour_correction),
            shader: shaders[at]
                .or_else(|| parent.and_then(|q| shaders[q]))
                .filter(|s| p.spec().shaders.contains(s))
                .unwrap_or(own.shader),
            bezel: bezels[at].clone().unwrap_or(own.bezel),
        }
    });
    Some(SlotState {
        cart: (!cart.is_empty()).then_some(cart),
        cart_platform,
        brightness: brightness?,
        blue_light: blue_light?,
        volume: volume?,
        volume_hp: volume_hp.or(volume)?,
        muted: muted?,
        muted_hp: muted_hp.or(muted)?,
        clock_set: clock_set?,
        utc_offset_min: utc_offset_min?,
        rumble: rumble.unwrap_or(fallback.rumble),
        ff_speed: ff_speed.unwrap_or(fallback.ff_speed),
        ff_sound: ff_sound.unwrap_or(fallback.ff_sound),
        screens,
        eject_save: eject_save.unwrap_or(fallback.eject_save),
        turbo: turbo.unwrap_or(fallback.turbo),
        rewind: rewind.unwrap_or(fallback.rewind),
        runahead: runahead.unwrap_or(fallback.runahead),
        gb_palettes: gb_palettes.unwrap_or(fallback.gb_palettes),
        gb_palette: gb_palette.unwrap_or(fallback.gb_palette),
        appearance: appearance.unwrap_or(fallback.appearance),
        wallpaper: wallpaper.unwrap_or(fallback.wallpaper),
        wallpaper_tint: wallpaper_tint.unwrap_or(fallback.wallpaper_tint),
        show_recents: show_recents.unwrap_or(fallback.show_recents),
        show_titles: show_titles.unwrap_or(fallback.show_titles),
        wifi: wifi.unwrap_or(fallback.wifi),
        bluetooth: bluetooth.unwrap_or(fallback.bluetooth),
        ssh: ssh.unwrap_or(fallback.ssh),
        samba: samba.unwrap_or(fallback.samba),
        tailscale: tailscale.unwrap_or(fallback.tailscale),
        vpn,
        password,
        recents: Vec::new(),
        favorites: Vec::new(),
        shelf,
        shelf_cart,
    })
}

/// Splits a key like `shader_gbc` into the setting and the platform it is for.
fn per_platform(key: &str) -> Option<(&str, Platform)> {
    ["colour_correction", "shader", "bezel"]
        .into_iter()
        .find_map(|setting| {
            let rest = key.strip_prefix(setting)?.strip_prefix('_')?;
            Some((setting, Platform::from_key(rest)?))
        })
}

/// Whose settings a platform starts from when the card has none of its own: a card from before
/// GBC had its own row ran Game Boy Color games on the Game Boy's.
fn settings_parent(platform: Platform) -> Option<Platform> {
    match platform {
        Platform::Gbc => Some(Platform::Gb),
        _ => None,
    }
}

fn recent_value(value: &str) -> Option<Recent> {
    let (platform, stem) = value.split_once('/')?;
    Some(Recent {
        platform: platform_value(platform)?,
        stem: stem.to_string(),
    })
    .filter(|r| !r.stem.is_empty())
}

fn platform_value(value: &str) -> Option<Platform> {
    Platform::from_key(value)
}

fn offset(value: &str) -> Option<i16> {
    value
        .parse()
        .ok()
        .filter(|n| (UTC_OFFSET_MIN..=UTC_OFFSET_MAX).contains(n))
}

fn ff_speed_value(value: &str) -> Option<u8> {
    value.parse().ok().filter(|n| FF_SPEEDS.contains(n))
}

fn level(value: &str, max: u8) -> Option<u8> {
    value.parse().ok().filter(|n| *n <= max)
}

fn flag(value: &str) -> Option<bool> {
    level(value, 1).map(|n| n == 1)
}
