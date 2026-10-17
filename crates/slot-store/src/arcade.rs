//! Arcade games' names. An arcade game is a zip named for its set, `sf2ce.zip`, and the shelf
//! shows the name the arcade databases give it instead: Street Fighter II': Champion Edition.

use std::collections::HashMap;
use std::sync::OnceLock;

/// `set<TAB>name` a line, written by tools/arcade/names.py.
const NAMES: &str = include_str!("../assets/arcade_names.tsv");

/// The sets that are a board's BIOS or a shared device, not a game, which sit beside the games
/// for the cores to find but have no place on the shelf.
const NOT_GAMES: &[&str] = &[
    "neogeo",
    "neocdz",
    "pgm",
    "skns",
    "nmk004",
    "isgsm",
    "decocass",
    "cchip",
    "bubsys",
    "qsound",
    "qsound_hle",
    "ym2608",
    "stvbios",
    "hng64",
    "konamigx",
    "midssio",
    "namco51",
    "namco52",
    "namco53",
    "namco54",
    "namco62",
    "sp-1",
    "coh1000c",
    "coh1000a",
    "coh3002c",
    "cpzn1",
    "cpzn2",
    "tps",
    "taitofx1",
    "naomi",
    "awbios",
    "megaplay",
    "megatech",
    "playch10",
    "nss",
    "sys573",
    "f355bios",
    "hod2bios",
    "airlbios",
    "crysbios",
    "ar_bios",
    "galgbios",
    "kf2_bios",
];

fn names() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    MAP.get_or_init(|| NAMES.lines().filter_map(|l| l.split_once('\t')).collect())
}

/// The game a set is, by the zip's name without `.zip`, or None for a set nobody knows.
pub fn name(set: &str) -> Option<&'static str> {
    names().get(set.to_ascii_lowercase().as_str()).copied()
}

/// Whether a set is a BIOS or a device rather than a game.
pub fn is_bios(set: &str) -> bool {
    NOT_GAMES.iter().any(|b| set.eq_ignore_ascii_case(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_is_named_for_its_game_whatever_its_case() {
        assert_eq!(
            name("SF2CE"),
            Some("Street Fighter II': Champion Edition (World 920513)")
        );
        assert_eq!(name("mslug"), Some("Metal Slug - Super Vehicle-001"));
        assert_eq!(name("not-a-set"), None);
    }

    #[test]
    fn a_bios_is_not_a_game() {
        assert!(is_bios("neogeo") && is_bios("PGM"));
        assert!(!is_bios("mslug"));
    }
}
