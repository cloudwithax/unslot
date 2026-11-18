//! RetroAchievements Trophy Room controller: the shelf's carousel of the games on the card that
//! have a cached set, then a carousel of each game's trophies, or of every trophy in the scope.
//! Groups cached sets by system first then game, and persists custom game sets in
//! `Config/trophies_set.txt`.

use std::collections::BTreeSet;
use std::path::Path;

use slot_cheevos::TrophyItem;
use slot_input::{Action, Btn};
use slot_store::{Cart, Platform};
use slot_ui::{ButtonLegend, Legend, ListPage, ListRow, TrophyTier, LIST_ROWS};

pub const SET_FILE: &str = "Config/trophies_set.txt";

/// Filtering & grouping scope for the Trophy Room.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TrophyScope {
    /// Grouped System first (shelf order), then Game title.
    AllSystems,
    /// Filtered to a single handheld/console system, ordered by Game title.
    SingleSystem(Platform),
    /// User-picked subset of games (`Config/trophies_set.txt`), grouped System first then Game.
    GameSet,
    /// Flat catalog of all cached games across every system, ordered by Game title.
    AllGames,
}

/// Active shelf zoom level inside the Trophy Room.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TrophyViewMode {
    /// The carousel of the card's games, each cart with the trophies it has earned.
    Games,
    /// Drilled into one Game's trophies, a carousel of them.
    Game(u64),
    /// A carousel of the trophies across the active `TrophyScope`.
    AllTrophies,
}

/// Cached RA game trophy set enriched with its resolved handheld platform metadata.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrophyGameEntry {
    pub game_id: u64,
    pub console_id: u32,
    pub platform: Option<Platform>,
    pub system_name: String,
    pub short_system: String,
    pub title: String,
    pub trophies: Vec<TrophyItem>,
}

impl TrophyGameEntry {
    pub fn earned_count(&self) -> usize {
        self.trophies
            .iter()
            .filter(|t| t.unlocked_at.is_some())
            .count()
    }

    /// This game's cart on the card, by its name against the cart's.
    fn cart_in<'a>(&self, carts: &'a [Cart]) -> Option<&'a Cart> {
        let want = norm_title(&self.title);
        if want.is_empty() {
            return None;
        }
        carts
            .iter()
            .filter(|c| self.platform.is_none_or(|p| p == c.platform))
            .find(|c| {
                let stem = norm_title(&c.stem);
                stem == want || (!c.title.is_empty() && norm_title(&c.title) == want)
            })
            .or_else(|| {
                // "Advance Wars 2" against "Advance Wars 2 - Black Hole Rising": the shorter name
                // is what a scraper wrote, so it is taken when nothing else matches.
                carts
                    .iter()
                    .filter(|c| self.platform.is_none_or(|p| p == c.platform))
                    .find(|c| {
                        let stem = norm_title(&c.stem);
                        !stem.is_empty()
                            && (stem.starts_with(&want) || want.starts_with(&stem))
                            && stem.len().min(want.len()) * 4 >= stem.len().max(want.len()) * 3
                    })
            })
    }

    pub fn tier(&self) -> TrophyTier {
        let total = self.trophies.len();
        let earned = self.earned_count();
        if earned == 0 || total == 0 {
            TrophyTier::Locked
        } else if earned == total {
            TrophyTier::Platinum
        } else if earned * 100 >= total * 75 {
            TrophyTier::Gold
        } else if earned * 100 >= total * 35 {
            TrophyTier::Silver
        } else {
            TrophyTier::Bronze
        }
    }

    fn sys_key(&self) -> (usize, u32) {
        (
            self.platform.map_or(usize::MAX, Platform::index),
            self.console_id,
        )
    }
}

fn item_tier(t: &TrophyItem) -> TrophyTier {
    if t.unlocked_at.is_none() {
        TrophyTier::Locked
    } else if t.points >= 50 {
        TrophyTier::Platinum
    } else if t.points >= 25 {
        TrophyTier::Gold
    } else if t.points >= 10 {
        TrophyTier::Silver
    } else {
        TrophyTier::Bronze
    }
}

/// How far along the way in from the shelf the room is.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum TrophyIntro {
    /// The shelf itself, for a beat, before anything moves.
    Beat,
    /// The carts going down behind the slot's front.
    Duck,
    /// The room and its carts coming up from behind it.
    Rise,
    /// In.
    #[default]
    Done,
}

/// How long each step of the way in takes, in seconds.
const BEAT_S: f32 = 0.55;
const DUCK_S: f32 = 0.34;
const RISE_S: f32 = 0.46;

impl TrophyIntro {
    fn after(self) -> (TrophyIntro, f32) {
        match self {
            TrophyIntro::Beat => (TrophyIntro::Duck, DUCK_S),
            TrophyIntro::Duck => (TrophyIntro::Rise, RISE_S),
            TrophyIntro::Rise | TrophyIntro::Done => (TrophyIntro::Done, 0.0),
        }
    }

    /// Whether the shelf, rather than the room, is what the screen still shows.
    pub fn on_shelf(self) -> bool {
        matches!(self, TrophyIntro::Beat | TrophyIntro::Duck)
    }
}

/// Persistent state and view model generator for `Phase::Trophies`.
#[derive(Clone, PartialEq, Debug)]
pub struct TrophyState {
    pub games: Vec<TrophyGameEntry>,
    pub selected_set: BTreeSet<u64>,
    pub scope: TrophyScope,
    pub mode: TrophyViewMode,
    pub cursor: usize,
    pub saved_game_cursor: usize,
    /// When `Some(cur)`, the Game Set multi-select `ListPage` overlay is open.
    pub picker: Option<usize>,
    /// The card's games, of which the carousel shows those the scope picks that have trophies.
    pub carts: Vec<Cart>,
    pub intro: TrophyIntro,
    /// Seconds spent in the current step.
    intro_t: f32,
    /// Where the trophy carousel is, in trophies: eased toward `cursor` as it moves.
    scroll: f32,
    /// How far the trophies have come in, from 0 as the carousel is entered to 1.
    enter: f32,
}

/// The three lines the room writes where the shelf writes a game's name: the top two small, the
/// last the shelf's own title line.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TrophyLabels {
    pub note: Option<String>,
    pub detail: String,
    pub title: String,
}

/// How far the trophy carousel moves a press, and how fast it follows.
const JUMP: usize = 10;
const FOLLOW: f32 = 16.0;
const ENTER_S: f32 = 0.3;

impl TrophyState {
    /// Loads cached RetroAchievements sets, the card's games and the saved Game Set.
    pub fn load(root: Option<&Path>, user: Option<&str>, carts: Vec<Cart>) -> Self {
        let mut games = Vec::new();
        let mut selected_set = BTreeSet::new();
        if let Some(root) = root {
            selected_set = read_game_set(root);
            let raw = slot_cheevos::read_trophy_catalog(&root.join(crate::cheevos::OFFLINE), user);
            games.reserve(raw.len());
            for g in raw {
                let platform = crate::cheevos::platform_for_console(g.console_id);
                let (system_name, short_system) = match platform {
                    Some(p) => (p.name().to_string(), p.dir_name().to_string()),
                    None => (format!("Console #{}", g.console_id), "RA".to_string()),
                };
                games.push(TrophyGameEntry {
                    game_id: g.game_id,
                    console_id: g.console_id,
                    platform,
                    system_name,
                    short_system,
                    title: if g.title.is_empty() {
                        format!("Game #{}", g.game_id)
                    } else {
                        g.title
                    },
                    trophies: g.trophies,
                });
            }
            games.sort_by(|a, b| {
                a.sys_key()
                    .cmp(&b.sys_key())
                    .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                    .then_with(|| a.game_id.cmp(&b.game_id))
            });
        }
        TrophyState {
            games,
            selected_set,
            scope: TrophyScope::AllSystems,
            mode: TrophyViewMode::Games,
            cursor: 0,
            saved_game_cursor: 0,
            picker: None,
            carts,
            intro: TrophyIntro::Beat,
            intro_t: 0.0,
            scroll: 0.0,
            enter: 1.0,
        }
    }

    /// Advances the way in from the shelf and the trophy carousel's follow. Returns whether what
    /// is drawn moved.
    pub fn tick(&mut self, dt: f32) -> bool {
        let moved = self.follow(dt);
        self.tick_intro(dt) || moved
    }

    fn follow(&mut self, dt: f32) -> bool {
        if matches!(self.mode, TrophyViewMode::Games) {
            return false;
        }
        let mut moved = false;
        if self.enter < 1.0 {
            self.enter = (self.enter + dt / ENTER_S).min(1.0);
            moved = true;
        }
        let goal = self.cursor as f32;
        if (goal - self.scroll).abs() > 0.002 {
            self.scroll += (goal - self.scroll) * (1.0 - (-dt * FOLLOW).exp());
            moved = true;
        } else {
            self.scroll = goal;
        }
        moved
    }

    fn tick_intro(&mut self, dt: f32) -> bool {
        if self.intro == TrophyIntro::Done {
            return false;
        }
        self.intro_t += dt;
        let span = match self.intro {
            TrophyIntro::Beat => BEAT_S,
            TrophyIntro::Duck => DUCK_S,
            TrophyIntro::Rise => RISE_S,
            TrophyIntro::Done => return false,
        };
        while self.intro != TrophyIntro::Done && self.intro_t >= span {
            self.intro_t -= span;
            self.intro = self.intro.after().0;
            let span = match self.intro {
                TrophyIntro::Duck => DUCK_S,
                TrophyIntro::Rise => RISE_S,
                _ => 0.0,
            };
            if span <= 0.0 {
                break;
            }
        }
        true
    }

    /// How far the way in has come, from 0 on the shelf to 1 in the room. The room comes up as
    /// the carts do, after they have gone down.
    pub fn brass(&self) -> f32 {
        match self.intro {
            TrophyIntro::Beat | TrophyIntro::Duck => 0.0,
            TrophyIntro::Rise => slot_ui::ease((self.intro_t / RISE_S).clamp(0.0, 1.0)),
            TrophyIntro::Done => 1.0,
        }
    }

    /// The carts' way down behind the slot's front, in pixels, for a cart `h` tall.
    pub fn sink(&self, h: f32) -> f32 {
        match self.intro {
            TrophyIntro::Beat => 0.0,
            TrophyIntro::Duck => h * slot_ui::ease((self.intro_t / DUCK_S).clamp(0.0, 1.0)),
            TrophyIntro::Rise => h * (1.0 - slot_ui::ease((self.intro_t / RISE_S).clamp(0.0, 1.0))),
            TrophyIntro::Done => 0.0,
        }
    }

    /// Whether the room's carts, rather than the shelf's own, are what moves.
    pub fn rising(&self) -> bool {
        matches!(self.intro, TrophyIntro::Rise | TrophyIntro::Done)
    }

    /// Skips the rest of the way in.
    pub fn settle(&mut self) {
        self.intro = TrophyIntro::Done;
        self.intro_t = 0.0;
    }

    /// The scope's games that are on the card, as its carts, in the order the carousel shows them.
    pub fn carousel_carts(&self) -> Vec<Cart> {
        let mut out: Vec<Cart> = Vec::new();
        for game in self.scoped_games() {
            if let Some(cart) = game.cart_in(&self.carts) {
                if !out
                    .iter()
                    .any(|c| c.platform == cart.platform && c.stem == cart.stem)
                {
                    out.push(cart.clone());
                }
            }
        }
        out
    }

    /// The games the carousel shows, in the carousel's order.
    pub fn carousel_games(&self) -> Vec<&TrophyGameEntry> {
        let carts = self.carousel_carts();
        self.scoped_games()
            .into_iter()
            .filter(|g| {
                g.cart_in(&carts).is_some_and(|c| {
                    carts
                        .iter()
                        .any(|k| k.platform == c.platform && k.stem == c.stem)
                })
            })
            .collect()
    }

    /// Where the trophy carousel is, in trophies.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// How far the trophies have come in, from 0 to 1.
    pub fn enter(&self) -> f32 {
        slot_ui::ease(self.enter)
    }

    /// Into a trophy carousel, from its first trophy, or back out to the games.
    fn go(&mut self, mode: TrophyViewMode) {
        self.mode = mode;
        if !matches!(mode, TrophyViewMode::Games) {
            self.cursor = 0;
            self.scroll = 0.0;
            self.enter = 0.0;
        }
    }

    /// The tier of each trophy the trophy carousel stands, in its order.
    pub fn strip_tiers(&self) -> Vec<TrophyTier> {
        match self.mode {
            TrophyViewMode::Games => Vec::new(),
            TrophyViewMode::Game(id) => self
                .current_game(id)
                .map(|g| g.trophies.iter().map(item_tier).collect())
                .unwrap_or_default(),
            TrophyViewMode::AllTrophies => self
                .scoped_trophies()
                .into_iter()
                .map(|(_, t)| item_tier(t))
                .collect(),
        }
    }

    /// Drills into the game the carousel has at `picked`.
    pub fn drill(&mut self, picked: usize) -> bool {
        let games = self.carousel_games();
        let Some(game) = games.get(picked) else {
            return false;
        };
        let id = game.game_id;
        self.saved_game_cursor = picked;
        self.go(TrophyViewMode::Game(id));
        true
    }

    /// What the room writes under its carts or trophies: for the games, the one at `picked`.
    pub fn labels(&self, picked: usize) -> TrophyLabels {
        let scope = self.scope_label().to_uppercase();
        match self.mode {
            TrophyViewMode::Games => {
                let games = self.carousel_games();
                let Some(game) = games.get(picked) else {
                    let title = match self.games.is_empty() {
                        true => "Play a game signed in to RetroAchievements to fill it",
                        false if self.scope == TrophyScope::GameSet => {
                            "Your Game Set has no games on this card"
                        }
                        false => "No games on this card have trophies here",
                    };
                    return TrophyLabels {
                        note: None,
                        detail: format!("TROPHY ROOM · {scope}"),
                        title: title.into(),
                    };
                };
                let (earned, total) = (game.earned_count(), game.trophies.len());
                TrophyLabels {
                    note: None,
                    detail: format!("TROPHY ROOM — {} OF {} · {scope}", picked + 1, games.len()),
                    title: match total {
                        0 => format!("{} · no achievements", game.title),
                        _ => format!("{} · {earned}/{total}", game.title),
                    },
                }
            }
            TrophyViewMode::Game(id) => {
                let Some(game) = self.current_game(id) else {
                    return TrophyLabels::default();
                };
                let Some(item) = game.trophies.get(self.cursor) else {
                    return TrophyLabels {
                        note: Some(game.system_name.to_uppercase()),
                        detail: "No official achievements in the cached set".into(),
                        title: game.title.clone(),
                    };
                };
                TrophyLabels {
                    note: Some(format!(
                        "{} · {} OF {}",
                        game.title.to_uppercase(),
                        self.cursor + 1,
                        game.trophies.len()
                    )),
                    detail: item.description.clone(),
                    title: format!(
                        "{} · {} pts · {}",
                        item.title,
                        item.points,
                        trophy_status(item)
                    ),
                }
            }
            TrophyViewMode::AllTrophies => {
                let items = self.scoped_trophies();
                let Some((game, item)) = items.get(self.cursor) else {
                    return TrophyLabels {
                        note: None,
                        detail: format!("ALL TROPHIES · {scope}"),
                        title: "No cached trophies in this view".into(),
                    };
                };
                TrophyLabels {
                    note: Some(format!(
                        "{} · {} · {} OF {}",
                        game.short_system.to_uppercase(),
                        game.title.to_uppercase(),
                        self.cursor + 1,
                        items.len()
                    )),
                    detail: item.description.clone(),
                    title: format!(
                        "{} · {} pts · {}",
                        item.title,
                        item.points,
                        trophy_status(item)
                    ),
                }
            }
        }
    }

    fn available_platforms(&self) -> Vec<Platform> {
        let mut out = Vec::new();
        for g in &self.games {
            if let Some(p) = g.platform {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }

    fn scoped_games(&self) -> Vec<&TrophyGameEntry> {
        let mut list: Vec<&TrophyGameEntry> = match self.scope {
            TrophyScope::AllSystems | TrophyScope::AllGames => self.games.iter().collect(),
            TrophyScope::SingleSystem(p) => self
                .games
                .iter()
                .filter(|g| g.platform == Some(p))
                .collect(),
            TrophyScope::GameSet => self
                .games
                .iter()
                .filter(|g| self.selected_set.contains(&g.game_id))
                .collect(),
        };
        if self.scope == TrophyScope::AllGames {
            list.sort_by(|a, b| {
                a.title
                    .to_lowercase()
                    .cmp(&b.title.to_lowercase())
                    .then_with(|| a.sys_key().cmp(&b.sys_key()))
                    .then_with(|| a.game_id.cmp(&b.game_id))
            });
        }
        list
    }

    fn scoped_trophies(&self) -> Vec<(&TrophyGameEntry, &TrophyItem)> {
        let games = self.scoped_games();
        let mut items = Vec::new();
        for g in games {
            for t in &g.trophies {
                items.push((g, t));
            }
        }
        items
    }

    fn current_game(&self, game_id: u64) -> Option<&TrophyGameEntry> {
        self.games.iter().find(|g| g.game_id == game_id)
    }

    fn item_len(&self) -> usize {
        match self.mode {
            TrophyViewMode::Games => self.scoped_games().len(),
            TrophyViewMode::Game(id) => self.current_game(id).map_or(0, |g| g.trophies.len()),
            TrophyViewMode::AllTrophies => self.scoped_trophies().len(),
        }
    }

    fn clamp_cursor(&mut self) {
        let len = self.item_len();
        self.cursor = if len == 0 {
            0
        } else {
            self.cursor.min(len - 1)
        };
    }

    fn next_system_index(&self) -> Option<usize> {
        if self.scope != TrophyScope::AllSystems {
            return None;
        }
        match self.mode {
            TrophyViewMode::Games => {
                let games = self.scoped_games();
                if games.is_empty() {
                    return None;
                }
                let cur_key = games[self.cursor.min(games.len() - 1)].sys_key();
                (self.cursor + 1..games.len())
                    .chain(0..=self.cursor)
                    .find(|&i| games[i].sys_key() != cur_key)
            }
            TrophyViewMode::AllTrophies => {
                let items = self.scoped_trophies();
                if items.is_empty() {
                    return None;
                }
                let cur_key = items[self.cursor.min(items.len() - 1)].0.sys_key();
                (self.cursor + 1..items.len())
                    .chain(0..=self.cursor)
                    .find(|&i| items[i].0.sys_key() != cur_key)
            }
            TrophyViewMode::Game(_) => None,
        }
    }

    /// Handles one input action; returns `false` when the Trophy Room should close back to the menu.
    pub fn input(&mut self, root: Option<&Path>, action: Action) -> bool {
        if let Some(cur) = self.picker {
            let n = self.games.len();
            match action {
                Action::GbaDown(Btn::Up) if n > 0 => self.picker = Some((cur + n - 1) % n),
                Action::GbaDown(Btn::Down) if n > 0 => self.picker = Some((cur + 1) % n),
                Action::GbaDown(Btn::L1) if n > 0 => {
                    self.picker = Some(cur.saturating_sub(LIST_ROWS));
                }
                Action::GbaDown(Btn::R1) if n > 0 => {
                    self.picker = Some((cur + LIST_ROWS).min(n - 1));
                }
                Action::GbaDown(Btn::A) if n > 0 => {
                    let id = self.games[cur.min(n - 1)].game_id;
                    if !self.selected_set.insert(id) {
                        self.selected_set.remove(&id);
                    }
                    if let Some(root) = root {
                        write_game_set(root, &self.selected_set);
                    }
                }
                Action::GbaDown(Btn::B | Btn::Select | Btn::Start) | Action::QuickMenu => {
                    self.picker = None;
                    self.clamp_cursor();
                }
                _ => {}
            }
            return true;
        }

        let len = self.item_len();
        match action {
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                if matches!(
                    self.mode,
                    TrophyViewMode::Game(_) | TrophyViewMode::AllTrophies
                ) {
                    self.mode = TrophyViewMode::Games;
                    self.cursor = self.saved_game_cursor;
                    self.clamp_cursor();
                    return true;
                }
                return false;
            }
            Action::GbaDown(Btn::Left) | Action::ShelfLeft if len > 0 => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            Action::GbaDown(Btn::Right) | Action::ShelfRight if len > 0 => {
                self.cursor = (self.cursor + 1).min(len - 1);
            }
            Action::GbaDown(Btn::L1) if len > 0 => {
                self.cursor = self.cursor.saturating_sub(JUMP);
            }
            Action::GbaDown(Btn::R1) if len > 0 => {
                self.cursor = (self.cursor + JUMP).min(len - 1);
            }
            Action::GbaDown(Btn::A) if self.mode == TrophyViewMode::Games => {
                let games = self.scoped_games();
                if let Some(g) = games.get(self.cursor) {
                    let id = g.game_id;
                    self.saved_game_cursor = self.cursor;
                    self.go(TrophyViewMode::Game(id));
                }
            }
            Action::GbaDown(Btn::X) => {
                self.cycle_scope();
            }
            Action::GbaDown(Btn::Y) if !matches!(self.mode, TrophyViewMode::Game(_)) => {
                self.y_action();
            }
            Action::GbaDown(Btn::Start) => {
                self.toggle_all_trophies();
            }
            Action::GbaDown(Btn::Select) => {
                self.open_set();
            }
            _ => {}
        }
        true
    }

    /// X: the next scope. Returns whether the set of games changed.
    pub fn cycle_scope(&mut self) -> bool {
        let platforms = self.available_platforms();
        self.scope = match self.scope {
            TrophyScope::AllSystems => {
                TrophyScope::SingleSystem(platforms.first().copied().unwrap_or(Platform::Gba))
            }
            TrophyScope::SingleSystem(_) => TrophyScope::GameSet,
            TrophyScope::GameSet => TrophyScope::AllGames,
            TrophyScope::AllGames => TrophyScope::AllSystems,
        };
        if matches!(self.mode, TrophyViewMode::Game(_)) {
            self.mode = TrophyViewMode::Games;
        }
        self.cursor = 0;
        self.clamp_cursor();
        true
    }

    /// Y: the next system, the game set, or all trophies. Returns whether the set changed.
    pub fn y_action(&mut self) -> bool {
        match self.scope {
            TrophyScope::SingleSystem(cur) => {
                let platforms = self.available_platforms();
                if !platforms.is_empty() {
                    let idx = platforms
                        .iter()
                        .position(|p| *p == cur)
                        .map_or(0, |i| (i + 1) % platforms.len());
                    self.scope = TrophyScope::SingleSystem(platforms[idx]);
                    self.cursor = 0;
                    return true;
                }
                false
            }
            TrophyScope::GameSet => {
                self.picker = Some(0);
                false
            }
            TrophyScope::AllSystems => {
                if let Some(next) = self.next_system_index() {
                    self.cursor = next;
                }
                false
            }
            TrophyScope::AllGames => {
                self.toggle_all_trophies();
                false
            }
        }
    }

    /// Select: the game set's checklist.
    pub fn open_set(&mut self) {
        self.scope = TrophyScope::GameSet;
        if matches!(self.mode, TrophyViewMode::Game(_)) {
            self.mode = TrophyViewMode::Games;
        }
        self.cursor = 0;
        self.picker = Some(0);
    }

    /// Start: the flat list of every trophy in the scope, and back.
    pub fn all_trophies(&mut self) {
        self.toggle_all_trophies();
    }

    fn toggle_all_trophies(&mut self) {
        match self.mode {
            TrophyViewMode::Games => {
                self.saved_game_cursor = self.cursor;
                self.go(TrophyViewMode::AllTrophies);
            }
            TrophyViewMode::AllTrophies | TrophyViewMode::Game(_) => {
                self.mode = TrophyViewMode::Games;
                self.cursor = self.saved_game_cursor;
                self.clamp_cursor();
            }
        }
    }

    fn scope_label(&self) -> String {
        match self.scope {
            TrophyScope::AllSystems => "All Systems".into(),
            TrophyScope::SingleSystem(p) => p.name().into(),
            TrophyScope::GameSet => "Game Set".into(),
            TrophyScope::AllGames => "All Games".into(),
        }
    }

    /// Checklist page shown when editing `Config/trophies_set.txt`.
    pub fn picker_page(&self) -> ListPage {
        let cursor = self.picker.unwrap_or(0);
        let rows: Vec<ListRow> = self
            .games
            .iter()
            .map(|g| {
                let on = self.selected_set.contains(&g.game_id);
                ListRow {
                    label: format!("{} · {}", g.short_system, g.title),
                    detail: if on { "[✓]".into() } else { "[ ]".into() },
                    bars: None,
                    icon: None,
                }
            })
            .collect();
        let status = if rows.is_empty() {
            "Play any RetroAchievements game first to add it here".into()
        } else {
            format!("{} selected for your cabinet", self.selected_set.len())
        };
        ListPage {
            title: "Trophy Game Set".into(),
            status,
            cursor: cursor.min(rows.len().saturating_sub(1)),
            rows,
            legends: vec![
                Legend::new(ButtonLegend::B, "Done"),
                Legend::new(ButtonLegend::A, "Toggle"),
            ],
        }
    }
}

/// A title or a cart's name as the two are compared: without its region tags, its punctuation or
/// its case, so `Advance Wars 2 - Black Hole Rising (USA)` meets `Advance Wars 2: Black Hole Rising`.
pub fn norm_title(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut depth = 0usize;
    for ch in name.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            _ if ch.is_ascii_alphanumeric() => out.extend(ch.to_lowercase()),
            _ => {}
        }
    }
    out
}

fn trophy_status(item: &TrophyItem) -> &'static str {
    match (item.unlocked_at.is_some(), item.hardcore) {
        (true, true) => "Hardcore",
        (true, false) => "Unlocked",
        (false, _) => "Locked",
    }
}

pub fn read_game_set(root: &Path) -> BTreeSet<u64> {
    let Ok(text) = std::fs::read_to_string(root.join(SET_FILE)) else {
        return BTreeSet::new();
    };
    text.lines()
        .filter_map(|l| l.trim().parse::<u64>().ok())
        .collect()
}

pub fn write_game_set(root: &Path, ids: &BTreeSet<u64>) {
    let mut out = String::new();
    for id in ids {
        out.push_str(&format!("{id}\n"));
    }
    let _ = std::fs::create_dir_all(root.join(slot_store::CONFIG_DIR));
    if let Err(e) = slot_store::atomic_write(&root.join(SET_FILE), out.as_bytes()) {
        eprintln!("slot: trophies: could not write {SET_FILE}: {e}");
    }
}
