pub mod arcade;
mod atomic;
pub mod cart_shell;
mod config;
mod core;
pub mod gb;
mod gb_palette;
mod gba;
pub mod ini;
mod platform;
mod ring;
mod scan;
mod slot_state;
mod stamp;

pub use atomic::atomic_write;
pub use cart_shell::{Outline, ShellChoice, ShellFinish, CART_SHELL_FILE, LABELS_SHELL_FILE};
pub use config::{move_config, CONFIG_DIR};
pub use core::{
    core_for, core_for_platform, core_for_system, read_selected_cores, write_game_core,
    write_selected_core, write_system_core, Core, CORES_DIR, SELECTED_CORE_FILE, SYSTEM_CORE_FILE,
};
pub use gb_palette::GbPalette;
pub use gba::{code, header_clean, title, title_code};
pub use platform::{Header, Platform, Spec};
pub use ring::{read_state, StateEntry, StateRing, RING_MAX};
pub use scan::{initial, is_hidden, listing, scan, sort_key, Cart, StoreError};
pub use slot_state::{
    read_slot_state, write_slot_state, Appearance, BezelPick, Recent, Screen, Shader, SlotState,
    WallpaperPick, WallpaperTint, BLUE_LIGHT_MAX, BRIGHTNESS_MAX, FF_SPEEDS, FF_SPEED_DEFAULT,
    RECENTS_MAX, RUN_AHEAD_MAX, UTC_OFFSET_MAX, UTC_OFFSET_MIN, VOLUME_MAX,
};
pub use stamp::{
    civil_from_days, days_from_civil, days_in_month, format_stamp, parse_stamp, stamp_now,
};
