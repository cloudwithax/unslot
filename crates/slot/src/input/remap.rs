use std::path::{Path, PathBuf};

use slot_retro::ButtonMask;
use slot_store::Platform;

/// Where the maps are kept: in Config, which an update leaves alone. `All.txt` for every game,
/// `<SYSTEM>.txt` for a system's, and `<SYSTEM>/<game>.txt` for one game's.
pub const DIR: &str = "Config/Remaps";

/// The game's buttons, by their libretro number, as the Controls page lists them.
pub const BUTTONS: [(u8, &str); 16] = [
    (4, "Up"),
    (5, "Down"),
    (6, "Left"),
    (7, "Right"),
    (8, "A"),
    (0, "B"),
    (9, "X"),
    (1, "Y"),
    (10, "L"),
    (11, "R"),
    (12, "L2"),
    (13, "R2"),
    (14, "L3"),
    (15, "R3"),
    (3, "Start"),
    (2, "Select"),
];

/// A game button left with no button of the device's.
pub const NONE: u8 = u8::MAX;

/// Which of the device's buttons press each of the game's: `0[game]` is the device's button,
/// both by libretro number, or `NONE`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Remap(pub [u8; 16]);

impl Default for Remap {
    fn default() -> Self {
        Remap(std::array::from_fn(|i| i as u8))
    }
}

impl Remap {
    /// What the game sees with the device's buttons held as `held`.
    pub fn apply(&self, held: ButtonMask) -> ButtonMask {
        if *self == Remap::default() {
            return held;
        }
        let mut mask = 0;
        for (game, &device) in self.0.iter().enumerate() {
            if device != NONE && held.0 & (1 << device) != 0 {
                mask |= 1 << game;
            }
        }
        ButtonMask(mask)
    }

    /// The device's button for the game's, one step along the choices: every button, then none.
    pub fn step(&mut self, game: u8, right: bool) {
        let order: Vec<u8> = BUTTONS.iter().map(|b| b.0).chain([NONE]).collect();
        let at = order
            .iter()
            .position(|b| *b == self.0[game as usize])
            .unwrap_or(0);
        let n = order.len();
        self.0[game as usize] = order[if right {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }];
    }

    /// Reads `game=device` lines by name. A button a line does not name keeps its own.
    pub fn parse(text: &str) -> Remap {
        let mut remap = Remap::default();
        for line in text.lines() {
            let Some((game, device)) = line.split_once('=') else {
                continue;
            };
            let device = match device.trim() {
                d if d.eq_ignore_ascii_case("none") => Some(NONE),
                d => id(d),
            };
            if let (Some(game), Some(device)) = (id(game.trim()), device) {
                remap.0[game as usize] = device;
            }
        }
        remap
    }

    pub fn text(&self) -> String {
        BUTTONS
            .iter()
            .map(|&(game, name)| format!("{name}={}\n", label(self.0[game as usize])))
            .collect()
    }
}

fn id(name: &str) -> Option<u8> {
    BUTTONS
        .iter()
        .find(|b| b.1.eq_ignore_ascii_case(name))
        .map(|b| b.0)
}

/// A button's name, by its libretro number, or "None".
pub fn label(id: u8) -> &'static str {
    BUTTONS.iter().find(|b| b.0 == id).map_or("None", |b| b.1)
}

/// The games a map is for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Scope {
    All,
    System(Platform),
    /// A game, by its system and its file's stem.
    Game(Platform, String),
}

impl Scope {
    pub fn label(&self) -> String {
        match self {
            Scope::All => "All Games".into(),
            Scope::System(p) => p.name().into(),
            Scope::Game(..) => "This Game".into(),
        }
    }

    pub fn path(&self, root: &Path) -> PathBuf {
        let dir = root.join(DIR);
        match self {
            Scope::All => dir.join("All.txt"),
            Scope::System(p) => dir.join(format!("{}.txt", p.dir_name())),
            Scope::Game(p, stem) => dir.join(p.dir_name()).join(format!("{stem}.txt")),
        }
    }

    /// The scope this one falls back on.
    pub fn wider(&self) -> Option<Scope> {
        match self {
            Scope::All => None,
            Scope::System(_) => Some(Scope::All),
            Scope::Game(p, _) => Some(Scope::System(*p)),
        }
    }
}

/// The scope's own map, if it has one on the card.
pub fn read(root: &Path, scope: &Scope) -> Option<Remap> {
    std::fs::read_to_string(scope.path(root))
        .ok()
        .map(|t| Remap::parse(&t))
}

/// The map that holds for a scope: its own, else the nearest wider one's, else none at all.
pub fn resolve(root: &Path, scope: &Scope) -> Remap {
    let mut at = Some(scope.clone());
    while let Some(scope) = at {
        if let Some(remap) = read(root, &scope) {
            return remap;
        }
        at = scope.wider();
    }
    Remap::default()
}

pub fn write(root: &Path, scope: &Scope, remap: &Remap) {
    let path = scope.path(root);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = slot_store::atomic_write(&path, remap.text().as_bytes()) {
        eprintln!("slot: remap: {}: {e}", path.display());
    }
}

/// Drops the scope's own map, so it falls back on the wider one.
pub fn clear(root: &Path, scope: &Scope) {
    let _ = std::fs::remove_file(scope.path(root));
}
