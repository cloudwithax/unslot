//! Local playtime tracking and completed session log for every game on the card.
//!
//! Data lives in `Config/playtime.json`, written atomically. Active play is ticked only while a
//! game is actually running (not paused in the in-game menu, not in overlays, not asleep), with
//! periodic checkpoints so a hard power cut loses at most a minute. Completed sessions of at
//! least [`MIN_SYNC_SECS`] seconds are kept in a bounded queue for RetroShelf to sync when online.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use slot_store::Platform;

pub const FILE: &str = "Config/playtime.json";

/// Minimum session length (in seconds) queued for RetroShelf sync, filtering accidental launches.
pub const MIN_SYNC_SECS: u64 = 10;

/// How often active playtime is checkpointed to disk while playing.
pub const CHECKPOINT_SECS: u64 = 60;

/// Maximum number of recent completed sessions kept on disk.
const MAX_SESSIONS: usize = 300;

/// Sort order on the Playtime screen.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    #[default]
    MostPlayed,
    Recent,
    Name,
}

impl Sort {
    pub fn next(self) -> Sort {
        match self {
            Sort::MostPlayed => Sort::Recent,
            Sort::Recent => Sort::Name,
            Sort::Name => Sort::MostPlayed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::MostPlayed => "Most played",
            Sort::Recent => "Recent",
            Sort::Name => "A-Z",
        }
    }
}

/// Cumulative local playtime stats for one game.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GameStats {
    /// `Platform::dir_name()`, e.g. `"GBA"`.
    pub platform: String,
    pub stem: String,
    /// Relative path on the card, e.g. `"Games/GBA/Emerald.gba"`.
    pub rom_path: String,
    /// File name, e.g. `"Emerald.gba"`.
    pub rom_name: String,
    pub total_secs: u64,
    pub sessions: u32,
    pub first_played: i64,
    pub last_played: i64,
}

/// A completed play session, optionally awaiting upload to RetroShelf.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SessionRecord {
    pub platform: String,
    pub stem: String,
    pub rom_path: String,
    pub rom_name: String,
    pub duration: u64,
    /// Unix timestamp (seconds) when the session started.
    pub played_at: i64,
    pub synced: bool,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlaytimeStore {
    pub games: Vec<GameStats>,
    pub sessions: Vec<SessionRecord>,
}

/// Tracks the currently running game session in memory.
#[derive(Clone, Debug)]
pub struct ActiveSession {
    pub platform: Platform,
    pub stem: String,
    pub rom: PathBuf,
    pub started_at: i64,
    elapsed_s: f64,
    checkpointed_secs: u64,
}

impl ActiveSession {
    pub fn start(platform: Platform, stem: String, rom: PathBuf, now_unix: i64) -> ActiveSession {
        ActiveSession {
            platform,
            stem,
            rom,
            started_at: now_unix,
            elapsed_s: 0.0,
            checkpointed_secs: 0,
        }
    }

    /// Advances active play by `dt` seconds. Returns a delta to checkpoint when at least
    /// [`CHECKPOINT_SECS`] uncheckpointed seconds have accumulated.
    pub fn tick(&mut self, dt: f32) -> Option<u64> {
        if dt <= 0.0 || !dt.is_finite() {
            return None;
        }
        // Clamp huge frame hiccups so a suspend glitch never adds phantom hours.
        self.elapsed_s += f64::from(dt.min(5.0));
        let whole = self.elapsed_s as u64;
        let delta = whole.saturating_sub(self.checkpointed_secs);
        if delta >= CHECKPOINT_SECS {
            self.checkpointed_secs = whole;
            Some(delta)
        } else {
            None
        }
    }

    /// Whole seconds played so far in this session.
    pub fn elapsed_secs(&self) -> u64 {
        self.elapsed_s as u64
    }

    /// Consumes the active session, returning `(total_session_secs, uncheckpointed_delta_secs)`.
    pub fn finish(self) -> (u64, u64) {
        let total = self.elapsed_s.round() as u64;
        let delta = total.saturating_sub(self.checkpointed_secs);
        (total, delta)
    }
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn rel_rom_path(root: &Path, rom: &Path, platform: Platform, stem: &str) -> (String, String) {
    let rel = rom
        .strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Games/{}/{stem}", platform.dir_name()));
    let name = rom
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| stem.to_string());
    (rel, name)
}

impl PlaytimeStore {
    pub fn read(root: &Path) -> PlaytimeStore {
        let Ok(text) = std::fs::read_to_string(root.join(FILE)) else {
            return PlaytimeStore::default();
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return PlaytimeStore::default();
        };
        let games = v["games"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| {
                let platform = g["platform"].as_str()?.to_string();
                let stem = g["stem"].as_str()?.to_string();
                let total_secs = g["total_secs"].as_u64().unwrap_or(0);
                if platform.is_empty() || stem.is_empty() {
                    return None;
                }
                Some(GameStats {
                    rom_path: g["rom_path"].as_str().unwrap_or_default().to_string(),
                    rom_name: g["rom_name"].as_str().unwrap_or(&stem).to_string(),
                    platform,
                    stem,
                    total_secs,
                    sessions: g["sessions"].as_u64().unwrap_or(0) as u32,
                    first_played: g["first_played"].as_i64().unwrap_or(0),
                    last_played: g["last_played"].as_i64().unwrap_or(0),
                })
            })
            .collect();
        let sessions = v["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let platform = s["platform"].as_str()?.to_string();
                let stem = s["stem"].as_str()?.to_string();
                let duration = s["duration"].as_u64()?;
                let played_at = s["played_at"].as_i64()?;
                Some(SessionRecord {
                    rom_path: s["rom_path"].as_str().unwrap_or_default().to_string(),
                    rom_name: s["rom_name"].as_str().unwrap_or(&stem).to_string(),
                    platform,
                    stem,
                    duration,
                    played_at,
                    synced: s["synced"].as_bool().unwrap_or(false),
                })
            })
            .collect();
        PlaytimeStore { games, sessions }
    }

    pub fn write(&self, root: &Path) -> std::io::Result<()> {
        let games: Vec<Value> = self
            .games
            .iter()
            .map(|g| {
                json!({
                    "platform": g.platform,
                    "stem": g.stem,
                    "rom_path": g.rom_path,
                    "rom_name": g.rom_name,
                    "total_secs": g.total_secs,
                    "sessions": g.sessions,
                    "first_played": g.first_played,
                    "last_played": g.last_played,
                })
            })
            .collect();
        let sessions: Vec<Value> = self
            .sessions
            .iter()
            .map(|s| {
                json!({
                    "platform": s.platform,
                    "stem": s.stem,
                    "rom_path": s.rom_path,
                    "rom_name": s.rom_name,
                    "duration": s.duration,
                    "played_at": s.played_at,
                    "synced": s.synced,
                })
            })
            .collect();
        let body = json!({
            "version": 1,
            "games": games,
            "sessions": sessions,
        });
        let text = serde_json::to_string_pretty(&body)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::create_dir_all(root.join("Config"))?;
        slot_store::atomic_write(&root.join(FILE), text.as_bytes())
    }

    fn game_mut(
        &mut self,
        platform: Platform,
        stem: &str,
        rom: &Path,
        root: &Path,
        now: i64,
    ) -> &mut GameStats {
        let dir = platform.dir_name();
        let (rom_path, rom_name) = rel_rom_path(root, rom, platform, stem);
        if let Some(i) = self
            .games
            .iter()
            .position(|g| g.platform == dir && g.stem == stem)
        {
            let g = &mut self.games[i];
            if !rom_path.is_empty() {
                g.rom_path = rom_path;
            }
            if !rom_name.is_empty() {
                g.rom_name = rom_name;
            }
            if g.first_played == 0 {
                g.first_played = now;
            }
            g.last_played = g.last_played.max(now);
            return g;
        }
        self.games.push(GameStats {
            platform: dir.to_string(),
            stem: stem.to_string(),
            rom_path,
            rom_name,
            total_secs: 0,
            sessions: 0,
            first_played: now,
            last_played: now,
        });
        self.games.last_mut().unwrap()
    }

    /// Adds a mid-session checkpoint to local totals without incrementing session count.
    pub fn record_checkpoint(
        &mut self,
        platform: Platform,
        stem: &str,
        rom: &Path,
        root: &Path,
        delta_secs: u64,
        now: i64,
    ) {
        if delta_secs == 0 || matches!(platform, Platform::Apps) {
            return;
        }
        let g = self.game_mut(platform, stem, rom, root, now);
        g.total_secs = g.total_secs.saturating_add(delta_secs);
    }

    /// Completes a session: adds any uncheckpointed seconds, increments the session count, and
    /// queues a [`SessionRecord`] when `total_session_secs >= MIN_SYNC_SECS`.
    #[allow(clippy::too_many_arguments)]
    pub fn finish_session(
        &mut self,
        platform: Platform,
        stem: &str,
        rom: &Path,
        root: &Path,
        total_session_secs: u64,
        delta_secs: u64,
        started_at: i64,
        now: i64,
    ) -> bool {
        if total_session_secs == 0 || matches!(platform, Platform::Apps) {
            return false;
        }
        let g = self.game_mut(platform, stem, rom, root, now);
        g.total_secs = g.total_secs.saturating_add(delta_secs);
        g.sessions = g.sessions.saturating_add(1);
        if started_at > 0 && (g.first_played == 0 || started_at < g.first_played) {
            g.first_played = started_at;
        }
        g.last_played = g.last_played.max(now);
        if total_session_secs >= MIN_SYNC_SECS {
            let (rom_path, rom_name) = rel_rom_path(root, rom, platform, stem);
            self.sessions.push(SessionRecord {
                platform: platform.dir_name().to_string(),
                stem: stem.to_string(),
                rom_path,
                rom_name,
                duration: total_session_secs,
                played_at: started_at,
                synced: false,
            });
            self.trim_sessions();
            return true;
        }
        false
    }

    fn trim_sessions(&mut self) {
        if self.sessions.len() <= MAX_SESSIONS {
            return;
        }
        // Drop oldest already-synced sessions first; if still over cap, drop oldest overall.
        let mut excess = self.sessions.len() - MAX_SESSIONS;
        self.sessions.retain(|s| {
            if excess > 0 && s.synced {
                excess -= 1;
                false
            } else {
                true
            }
        });
        if self.sessions.len() > MAX_SESSIONS {
            let drop = self.sessions.len() - MAX_SESSIONS;
            self.sessions.drain(0..drop);
        }
    }

    pub fn game_secs(&self, platform: Platform, stem: &str) -> u64 {
        let dir = platform.dir_name();
        self.games
            .iter()
            .find(|g| g.platform == dir && g.stem == stem)
            .map_or(0, |g| g.total_secs)
    }

    pub fn total_secs(&self) -> u64 {
        self.games.iter().map(|g| g.total_secs).sum()
    }

    pub fn total_sessions(&self) -> u32 {
        self.games.iter().map(|g| g.sessions).sum()
    }

    pub fn pending_count(&self) -> usize {
        self.sessions.iter().filter(|s| !s.synced).count()
    }

    pub fn ranked(&self, sort: Sort) -> Vec<&GameStats> {
        let mut out: Vec<&GameStats> = self.games.iter().filter(|g| g.total_secs > 0).collect();
        match sort {
            Sort::MostPlayed => out.sort_by(|a, b| {
                b.total_secs
                    .cmp(&a.total_secs)
                    .then_with(|| b.last_played.cmp(&a.last_played))
                    .then_with(|| a.stem.to_lowercase().cmp(&b.stem.to_lowercase()))
            }),
            Sort::Recent => out.sort_by(|a, b| {
                b.last_played
                    .cmp(&a.last_played)
                    .then_with(|| b.total_secs.cmp(&a.total_secs))
            }),
            Sort::Name => out.sort_by(|a, b| {
                a.stem
                    .to_lowercase()
                    .cmp(&b.stem.to_lowercase())
                    .then_with(|| b.total_secs.cmp(&a.total_secs))
            }),
        }
        out
    }

    /// Marks sessions matching `(played_at, platform, stem, duration)` as synced.
    pub fn mark_synced(&mut self, keys: &[(i64, String, String, u64)]) -> usize {
        let mut counts: HashMap<(i64, &str, &str, u64), usize> = HashMap::new();
        for (at, p, s, d) in keys {
            *counts.entry((*at, p.as_str(), s.as_str(), *d)).or_insert(0) += 1;
        }
        let mut marked = 0;
        for rec in &mut self.sessions {
            if rec.synced {
                continue;
            }
            if let Some(left) = counts.get_mut(&(
                rec.played_at,
                rec.platform.as_str(),
                rec.stem.as_str(),
                rec.duration,
            )) {
                if *left > 0 {
                    *left -= 1;
                    rec.synced = true;
                    marked += 1;
                }
            }
        }
        marked
    }
}

/// Formats a duration in seconds for display: `"45s"`, `"12m"`, `"2h 15m"`, `"2h"`.
pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = mins / 60;
    let rem = mins % 60;
    match rem {
        0 => format!("{hours}h"),
        r => format!("{hours}h {r}m"),
    }
}

/// Formats `ts` relative to `now` (both Unix seconds): `"Today"`, `"Yesterday"`, `"24 Nov"`, or
/// `"24 Nov 2025"` for another year.
pub fn format_last_played(ts: i64, now: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    if ts <= 0 {
        return String::new();
    }
    let day = ts.div_euclid(86_400);
    let today = now.div_euclid(86_400);
    if now > 0 {
        if day == today {
            return "Today".into();
        }
        if day + 1 == today {
            return "Yesterday".into();
        }
    }
    let (y, m, d) = slot_store::civil_from_days(day);
    let mon = MONTHS[(m as usize).clamp(1, 12) - 1];
    let (ny, _, _) = slot_store::civil_from_days(today);
    if now > 0 && y == ny {
        format!("{d} {mon}")
    } else {
        format!("{d} {mon} {y}")
    }
}

/// Looks up the `Platform` whose `dir_name()` equals `dir`.
pub fn platform_from_dir(dir: &str) -> Option<Platform> {
    Platform::ALL
        .into_iter()
        .find(|p| p.dir_name().eq_ignore_ascii_case(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_format_compactly() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(750), "12m");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(8100), "2h 15m");
    }

    #[test]
    fn active_session_checkpoints_without_double_counting() {
        let d = tempfile::tempdir().unwrap();
        let rom = d.path().join("Games/GBA/Emerald.gba");
        let mut store = PlaytimeStore::default();
        let mut active = ActiveSession::start(Platform::Gba, "Emerald".into(), rom.clone(), 1000);
        // Simulate 150 seconds of play in 1s ticks: checkpoints at 60s and 120s.
        let mut checkpoints = 0;
        for s in 1..=150 {
            if let Some(delta) = active.tick(1.0) {
                store.record_checkpoint(Platform::Gba, "Emerald", &rom, d.path(), delta, 1000 + s);
                checkpoints += 1;
            }
        }
        assert_eq!(checkpoints, 2);
        assert_eq!(store.game_secs(Platform::Gba, "Emerald"), 120);
        let (total, delta) = active.finish();
        assert_eq!(total, 150);
        assert_eq!(delta, 30);
        let queued = store.finish_session(
            Platform::Gba,
            "Emerald",
            &rom,
            d.path(),
            total,
            delta,
            1000,
            1150,
        );
        assert!(queued);
        assert_eq!(store.game_secs(Platform::Gba, "Emerald"), 150);
        assert_eq!(store.games[0].sessions, 1);
        assert_eq!(store.pending_count(), 1);
        assert_eq!(store.sessions[0].duration, 150);

        store.write(d.path()).unwrap();
        let back = PlaytimeStore::read(d.path());
        assert_eq!(back, store);
    }

    #[test]
    fn short_sessions_count_locally_but_skip_retroshelf_queue() {
        let d = tempfile::tempdir().unwrap();
        let rom = d.path().join("Games/GB/Tetris.gb");
        let mut store = PlaytimeStore::default();
        let queued = store.finish_session(Platform::Gb, "Tetris", &rom, d.path(), 5, 5, 100, 105);
        assert!(!queued);
        assert_eq!(store.game_secs(Platform::Gb, "Tetris"), 5);
        assert_eq!(store.pending_count(), 0);
    }

    #[test]
    fn ranking_sorts_by_most_played_recent_and_name() {
        let d = tempfile::tempdir().unwrap();
        let mut store = PlaytimeStore::default();
        store.finish_session(
            Platform::Gba,
            "Zelda",
            &d.path().join("Games/GBA/Zelda.gba"),
            d.path(),
            300,
            300,
            1000,
            1300,
        );
        store.finish_session(
            Platform::Snes,
            "Chrono",
            &d.path().join("Games/SNES/Chrono.sfc"),
            d.path(),
            900,
            900,
            500,
            1400,
        );
        let names = |sort| -> Vec<&str> {
            store
                .ranked(sort)
                .into_iter()
                .map(|g| g.stem.as_str())
                .collect()
        };
        assert_eq!(names(Sort::MostPlayed), ["Chrono", "Zelda"]);
        assert_eq!(names(Sort::Recent), ["Chrono", "Zelda"]);
        assert_eq!(names(Sort::Name), ["Chrono", "Zelda"]);
    }
}
