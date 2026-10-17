pub mod app;
pub mod audio;
pub mod baseos;
pub mod bezel;
pub mod bluetooth;
pub mod boot_time;
pub mod bootlogo;
pub mod build_info;
pub mod cable;
pub mod card_log;
pub mod cart_faces;
pub mod core;
pub mod core_picker;
#[cfg(unix)]
pub mod crash;
#[cfg(not(unix))]
pub mod crash {
    pub fn install() {}
}
pub mod drc;
pub mod emu;
pub mod face_builder;
pub mod frames;
pub mod frontend;
pub mod gpu;
pub mod input;
pub mod json;
pub mod label_cache;
pub mod latency;
pub mod link_art_builder;
pub mod link_kind;
pub mod link_net;
pub mod link_radio;
pub mod link_screen;
pub mod link_start;
pub mod link_state;
pub mod pak;
pub mod panel_turn;
pub mod persist;
pub mod prefetch;
pub mod resample;
pub mod rewind;
pub mod root;
pub mod scrape;
pub mod search;
pub mod services;
pub mod session;
pub mod state_writer;
pub mod thumb;
pub mod update;
pub mod video_mode;
pub mod wallpaper;
pub mod wifi;
pub mod wireguard;
