use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::atomic::atomic_write;
use crate::{ini, Platform};

pub const KIOSK_FILE: &str = "Config/kiosk.conf";
pub const ESRB_FILE: &str = "Config/esrb.txt";
pub const ESRB_MANUAL_FILE: &str = "Config/esrb_manual.txt";

/// Age rating assigned by the Entertainment Software Rating Board.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Esrb {
    Everyone,
    Everyone10Plus,
    Teen,
    Mature,
    AdultsOnly,
}

impl Esrb {
    pub const ALL: [Esrb; 5] = [
        Esrb::Everyone,
        Esrb::Everyone10Plus,
        Esrb::Teen,
        Esrb::Mature,
        Esrb::AdultsOnly,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Esrb::Everyone => "E",
            Esrb::Everyone10Plus => "E10+",
            Esrb::Teen => "T",
            Esrb::Mature => "M",
            Esrb::AdultsOnly => "AO",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Esrb::Everyone => "E (Everyone)",
            Esrb::Everyone10Plus => "E10+ (Everyone 10+)",
            Esrb::Teen => "T (Teen)",
            Esrb::Mature => "M (Mature 17+)",
            Esrb::AdultsOnly => "AO (Adults Only)",
        }
    }

    pub fn parse(raw: &str) -> Option<Esrb> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower == "rp" || lower.contains("rating pending") || lower == "unrated" {
            return None;
        }
        if lower == "e10+"
            || lower == "e10"
            || lower.starts_with("e10+")
            || lower.contains("everyone 10")
            || lower.contains("10+")
        {
            return Some(Esrb::Everyone10Plus);
        }
        if lower == "ec"
            || lower == "k-a"
            || lower == "ka"
            || lower == "e"
            || lower == "everyone"
            || lower.starts_with("ec -")
            || lower.starts_with("k-a")
            || lower.starts_with("e (")
            || lower.starts_with("e - ")
            || lower.starts_with("early childhood")
            || lower.starts_with("kids to adults")
        {
            return Some(Esrb::Everyone);
        }
        if lower == "ao"
            || lower.starts_with("ao ")
            || lower.starts_with("ao-")
            || lower.contains("adults only")
            || lower.contains("18+")
        {
            return Some(Esrb::AdultsOnly);
        }
        if lower == "t" || lower == "teen" || lower.starts_with("t - ") || lower.starts_with("t (")
        {
            return Some(Esrb::Teen);
        }
        if lower == "m"
            || lower == "mature"
            || lower.starts_with("m (")
            || lower.starts_with("m - ")
            || lower.starts_with("mature")
            || lower.contains("17+")
        {
            return Some(Esrb::Mature);
        }
        None
    }

    pub fn next_manual(current: Option<Esrb>, right: bool) -> Option<Esrb> {
        const CHOICES: [Option<Esrb>; 6] = [
            None,
            Some(Esrb::Everyone),
            Some(Esrb::Everyone10Plus),
            Some(Esrb::Teen),
            Some(Esrb::Mature),
            Some(Esrb::AdultsOnly),
        ];
        let at = CHOICES.iter().position(|&c| c == current).unwrap_or(0);
        let n = CHOICES.len();
        CHOICES[if right {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }]
    }
}

/// Maximum allowed ESRB tier while Kiosk Mode is active.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum EsrbCap {
    #[default]
    Off,
    Everyone,
    UnderTeen,
    TeenAndUnder,
    MatureAndUnder,
}

impl EsrbCap {
    pub const ALL: [EsrbCap; 5] = [
        EsrbCap::Off,
        EsrbCap::Everyone,
        EsrbCap::UnderTeen,
        EsrbCap::TeenAndUnder,
        EsrbCap::MatureAndUnder,
    ];

    pub fn key(self) -> &'static str {
        match self {
            EsrbCap::Off => "off",
            EsrbCap::Everyone => "e",
            EsrbCap::UnderTeen => "under-t",
            EsrbCap::TeenAndUnder => "t",
            EsrbCap::MatureAndUnder => "m",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EsrbCap::Off => "Off",
            EsrbCap::Everyone => "E Only",
            EsrbCap::UnderTeen => "Under T (E & E10+)",
            EsrbCap::TeenAndUnder => "T & Under",
            EsrbCap::MatureAndUnder => "M & Under",
        }
    }

    pub fn parse(s: &str) -> Option<EsrbCap> {
        EsrbCap::ALL
            .into_iter()
            .find(|c| c.key().eq_ignore_ascii_case(s.trim()))
    }

    pub fn step(self, right: bool) -> EsrbCap {
        let n = EsrbCap::ALL.len();
        let at = EsrbCap::ALL.iter().position(|c| *c == self).unwrap_or(0);
        EsrbCap::ALL[if right {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }]
    }

    pub fn max_rating(self) -> Option<Esrb> {
        match self {
            EsrbCap::Off => None,
            EsrbCap::Everyone => Some(Esrb::Everyone),
            EsrbCap::UnderTeen => Some(Esrb::Everyone10Plus),
            EsrbCap::TeenAndUnder => Some(Esrb::Teen),
            EsrbCap::MatureAndUnder => Some(Esrb::Mature),
        }
    }

    /// Whether a game with `rating` is permitted. Unrated (`None`) games are locked out whenever
    /// any ESRB limit is enabled.
    pub fn allows(self, rating: Option<Esrb>) -> bool {
        match self.max_rating() {
            None => true,
            Some(max) => rating.is_some_and(|r| r <= max),
        }
    }
}

/// Whether Kiosk unlock uses a numeric PIN pad or a full alphanumeric password keyboard.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum KioskAuth {
    #[default]
    Pin,
    Password,
}

impl KioskAuth {
    pub fn key(self) -> &'static str {
        match self {
            KioskAuth::Pin => "pin",
            KioskAuth::Password => "password",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KioskAuth::Pin => "PIN",
            KioskAuth::Password => "Password",
        }
    }

    pub fn parse(s: &str) -> Option<KioskAuth> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pin" => Some(KioskAuth::Pin),
            "password" => Some(KioskAuth::Password),
            _ => None,
        }
    }

    pub fn toggle(self) -> KioskAuth {
        match self {
            KioskAuth::Pin => KioskAuth::Password,
            KioskAuth::Password => KioskAuth::Pin,
        }
    }
}

/// Per-game Kiosk visibility rule.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum GameRule {
    #[default]
    Default,
    Allow,
    Block,
}

impl GameRule {
    pub fn next(self) -> GameRule {
        match self {
            GameRule::Default => GameRule::Allow,
            GameRule::Allow => GameRule::Block,
            GameRule::Block => GameRule::Default,
        }
    }

    pub fn label(self, whitelist_games: bool) -> &'static str {
        match self {
            GameRule::Allow => "Allowed",
            GameRule::Block => "Blocked",
            GameRule::Default if whitelist_games => "Hidden",
            GameRule::Default => "Follows System/ESRB",
        }
    }
}

/// Persisted Kiosk Mode settings under `Config/kiosk.conf`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct KioskConfig {
    pub active: bool,
    pub auth: KioskAuth,
    pub secret: String,
    pub restrict_systems: bool,
    pub allowed_systems: BTreeSet<Platform>,
    pub whitelist_games: bool,
    pub allowed_games: BTreeSet<(Platform, String)>,
    pub blocked_games: BTreeSet<(Platform, String)>,
    pub esrb_cap: EsrbCap,
}

impl KioskConfig {
    pub fn game_rule(&self, platform: Platform, stem: &str) -> GameRule {
        let key = (platform, stem.to_string());
        if self.blocked_games.contains(&key) {
            GameRule::Block
        } else if self.allowed_games.contains(&key) {
            GameRule::Allow
        } else {
            GameRule::Default
        }
    }

    pub fn set_game_rule(&mut self, platform: Platform, stem: &str, rule: GameRule) {
        let key = (platform, stem.to_string());
        self.allowed_games.remove(&key);
        self.blocked_games.remove(&key);
        match rule {
            GameRule::Default => {}
            GameRule::Allow => {
                self.allowed_games.insert(key);
            }
            GameRule::Block => {
                self.blocked_games.insert(key);
            }
        }
    }

    pub fn allows_platform(&self, platform: Platform) -> bool {
        if !self.active {
            return true;
        }
        !self.restrict_systems || self.allowed_systems.contains(&platform)
    }

    pub fn allows_game(&self, platform: Platform, stem: &str, rating: Option<Esrb>) -> bool {
        if !self.active {
            return true;
        }
        let key = (platform, stem.to_string());
        if self.blocked_games.contains(&key) {
            return false;
        }
        if self.restrict_systems && !self.allowed_systems.contains(&platform) {
            return false;
        }
        if self.allowed_games.contains(&key) {
            return true;
        }
        if self.whitelist_games {
            return false;
        }
        self.esrb_cap.allows(rating)
    }
}

fn game_key(platform: Platform, stem: &str) -> String {
    format!("{}/{stem}", platform.key())
}

fn parse_game_key(raw: &str) -> Option<(Platform, String)> {
    let (plat, stem) = raw.trim().split_once('/')?;
    if stem.is_empty() {
        return None;
    }
    Some((Platform::from_key(plat)?, stem.to_string()))
}

pub fn read_kiosk_config(root: &Path) -> KioskConfig {
    let Ok(text) = std::fs::read_to_string(root.join(KIOSK_FILE)) else {
        return KioskConfig::default();
    };
    let mut cfg = KioskConfig::default();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, val)) = trimmed.split_once('=') else {
            continue;
        };
        let (key, val) = (key.trim(), val.trim());
        match key {
            "active" => cfg.active = val == "1" || val.eq_ignore_ascii_case("true"),
            "auth" => {
                if let Some(auth) = KioskAuth::parse(val) {
                    cfg.auth = auth;
                }
            }
            "secret" => cfg.secret = decode_secret(val),
            "restrict_systems" => {
                cfg.restrict_systems = val == "1" || val.eq_ignore_ascii_case("true")
            }
            "whitelist_games" => {
                cfg.whitelist_games = val == "1" || val.eq_ignore_ascii_case("true")
            }
            "esrb_cap" => {
                if let Some(cap) = EsrbCap::parse(val) {
                    cfg.esrb_cap = cap;
                }
            }
            "allow_system" => {
                if let Some(p) = Platform::from_key(val) {
                    cfg.allowed_systems.insert(p);
                }
            }
            "allow_game" => {
                if let Some(pair) = parse_game_key(val) {
                    cfg.allowed_games.insert(pair);
                }
            }
            "block_game" => {
                if let Some(pair) = parse_game_key(val) {
                    cfg.blocked_games.insert(pair);
                }
            }
            _ => {}
        }
    }
    cfg
}

pub fn write_kiosk_config(root: &Path, cfg: &KioskConfig) -> std::io::Result<()> {
    let mut out = format!(
        "active={}\nauth={}\nsecret={}\nrestrict_systems={}\nwhitelist_games={}\nesrb_cap={}\n",
        u8::from(cfg.active),
        cfg.auth.key(),
        encode_secret(&cfg.secret),
        u8::from(cfg.restrict_systems),
        u8::from(cfg.whitelist_games),
        cfg.esrb_cap.key(),
    );
    for p in &cfg.allowed_systems {
        out.push_str(&format!("allow_system={}\n", p.key()));
    }
    for (p, stem) in &cfg.allowed_games {
        out.push_str(&format!("allow_game={}\n", game_key(*p, stem)));
    }
    for (p, stem) in &cfg.blocked_games {
        out.push_str(&format!("block_game={}\n", game_key(*p, stem)));
    }
    std::fs::create_dir_all(root.join(crate::CONFIG_DIR))?;
    atomic_write(&root.join(KIOSK_FILE), out.as_bytes())
}

/// Escapes what the reader would trim or split on: spaces at either end are part of a password.
fn encode_secret(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '%' | '\n' | '\r' | ' ' | '\t' => out.push_str(&format!("%{:02X}", c as u8)),
            _ => out.push(c),
        }
    }
    out
}

fn decode_secret(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Scraped + manual ESRB ratings per `(Platform, stem)`. Manual entries always override scraped.
pub fn read_esrb_map(root: &Path) -> HashMap<(Platform, String), Esrb> {
    let mut map = read_esrb_file(root, ESRB_FILE);
    map.extend(read_esrb_file(root, ESRB_MANUAL_FILE));
    map
}

pub fn read_manual_esrb_map(root: &Path) -> HashMap<(Platform, String), Esrb> {
    read_esrb_file(root, ESRB_MANUAL_FILE)
}

fn read_esrb_file(root: &Path, rel: &str) -> HashMap<(Platform, String), Esrb> {
    ini::read(root, rel)
        .into_iter()
        .filter_map(|(k, v)| Some((parse_game_key(&k)?, Esrb::parse(&v)?)))
        .collect()
}

pub fn write_scraped_esrb(
    root: &Path,
    platform: Platform,
    stem: &str,
    rating: Esrb,
) -> std::io::Result<()> {
    ini::write(root, ESRB_FILE, &game_key(platform, stem), rating.code())
}

pub fn write_manual_esrb(
    root: &Path,
    platform: Platform,
    stem: &str,
    rating: Option<Esrb>,
) -> std::io::Result<()> {
    let key = game_key(platform, stem);
    match rating {
        Some(r) => ini::write(root, ESRB_MANUAL_FILE, &key, r.code()),
        None => ini::remove(root, ESRB_MANUAL_FILE, &key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esrb_parse_handles_codes_and_descriptions() {
        assert_eq!(Esrb::parse("E"), Some(Esrb::Everyone));
        assert_eq!(Esrb::parse("Everyone"), Some(Esrb::Everyone));
        assert_eq!(Esrb::parse("K-A"), Some(Esrb::Everyone));
        assert_eq!(Esrb::parse("EC - Early Childhood"), Some(Esrb::Everyone));
        assert_eq!(Esrb::parse("K-A - Kids to Adults"), Some(Esrb::Everyone));
        assert_eq!(Esrb::parse("E10+"), Some(Esrb::Everyone10Plus));
        assert_eq!(Esrb::parse("Everyone 10+"), Some(Esrb::Everyone10Plus));
        assert_eq!(Esrb::parse("T"), Some(Esrb::Teen));
        assert_eq!(Esrb::parse("Teen"), Some(Esrb::Teen));
        assert_eq!(Esrb::parse("M"), Some(Esrb::Mature));
        assert_eq!(Esrb::parse("Mature 17+"), Some(Esrb::Mature));
        assert_eq!(Esrb::parse("AO"), Some(Esrb::AdultsOnly));
        assert_eq!(Esrb::parse("Adults Only"), Some(Esrb::AdultsOnly));
        for e in Esrb::ALL {
            assert_eq!(Esrb::parse(e.label()), Some(e), "{:?}", e);
        }
        assert_eq!(Esrb::parse("RP"), None);
        assert_eq!(Esrb::parse(""), None);
    }

    #[test]
    fn esrb_cap_locks_out_unrated_and_higher_ratings() {
        assert!(EsrbCap::Off.allows(None));
        assert!(EsrbCap::Off.allows(Some(Esrb::AdultsOnly)));

        assert!(EsrbCap::UnderTeen.allows(Some(Esrb::Everyone)));
        assert!(EsrbCap::UnderTeen.allows(Some(Esrb::Everyone10Plus)));
        assert!(!EsrbCap::UnderTeen.allows(Some(Esrb::Teen)));
        assert!(!EsrbCap::UnderTeen.allows(Some(Esrb::Mature)));
        assert!(!EsrbCap::UnderTeen.allows(None));
    }

    #[test]
    fn kiosk_config_and_manual_esrb_round_trip_across_reboots() {
        let d = tempfile::tempdir().unwrap();
        let mut cfg = KioskConfig {
            active: true,
            auth: KioskAuth::Password,
            secret: " pa%ss=123 ".into(),
            restrict_systems: true,
            whitelist_games: false,
            esrb_cap: EsrbCap::UnderTeen,
            ..Default::default()
        };
        cfg.allowed_systems.insert(Platform::Gba);
        cfg.allowed_systems.insert(Platform::Snes);
        cfg.blocked_games.insert((Platform::Gba, "Doom".into()));
        cfg.allowed_games.insert((Platform::Gba, "Emerald".into()));
        write_kiosk_config(d.path(), &cfg).unwrap();

        let loaded = read_kiosk_config(d.path());
        assert_eq!(loaded, cfg);

        write_scraped_esrb(d.path(), Platform::Gba, "Doom", Esrb::Everyone).unwrap();
        write_manual_esrb(d.path(), Platform::Gba, "Doom", Some(Esrb::Mature)).unwrap();
        write_manual_esrb(d.path(), Platform::Gba, "Emerald", Some(Esrb::Everyone)).unwrap();
        let ratings = read_esrb_map(d.path());
        assert_eq!(
            ratings.get(&(Platform::Gba, "Doom".into())),
            Some(&Esrb::Mature)
        );
        assert_eq!(
            ratings.get(&(Platform::Gba, "Emerald".into())),
            Some(&Esrb::Everyone)
        );

        assert!(loaded.allows_game(Platform::Gba, "Emerald", Some(Esrb::Everyone)));
        assert!(!loaded.allows_game(Platform::Gba, "Doom", Some(Esrb::Everyone)));
        assert!(!loaded.allows_game(Platform::Gba, "Unknown", None));
        assert!(!loaded.allows_game(Platform::Nes, "Mario", Some(Esrb::Everyone)));
    }
}
