mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{app_booting_at, stored_zip, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::{App, PageModel, Phase};
use slot::romm::{self, Conf, Http, Req, SimRomm, Source};
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, Platform, SlotState, StateRing};
use slot_ui::{ListPage, QuickRow};

const SERVER: &str = "http://romm.lan:8080";

/// A request as `METHOD url`, with the JSON or the file it sent.
type Sent = (String, Option<String>, Option<Vec<u8>>);

/// A RomM that answers each URL starting with a given path as it was told to, and notes what
/// it was asked.
#[derive(Clone, Default)]
struct Fake {
    answers: Vec<(String, u16, Vec<u8>)>,
    asked: Arc<Mutex<Vec<String>>>,
    sent: Arc<Mutex<Vec<Sent>>>,
}

impl Fake {
    fn on(mut self, path: &str, code: u16, body: impl Into<Vec<u8>>) -> Fake {
        self.answers
            .push((format!("{SERVER}{path}"), code, body.into()));
        self
    }
}

impl Fake {
    /// What was sent to URLs starting with `start`, as `METHOD url`.
    fn sent_to(&self, start: &str) -> Vec<Sent> {
        let start = start.replacen(' ', &format!(" {SERVER}"), 1);
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(r, ..)| r.starts_with(&start))
            .cloned()
            .collect()
    }
}

impl Http for Fake {
    fn send(&mut self, req: &Req) -> Result<(u16, Vec<u8>), String> {
        self.asked.lock().unwrap().push(req.url.clone());
        let file = req.file.map(|(path, _)| std::fs::read(path).unwrap());
        self.sent.lock().unwrap().push((
            format!("{} {}", req.method, req.url),
            req.json.clone(),
            file,
        ));
        let (_, code, body) = self
            .answers
            .iter()
            .filter(|(url, ..)| req.url.starts_with(url.as_str()))
            .max_by_key(|(url, ..)| url.len())
            .cloned()
            .ok_or_else(|| format!("nothing at {}", req.url))?;
        match req.to {
            Some(to) => {
                std::fs::write(to, &body).unwrap();
                Ok((code, Vec::new()))
            }
            None => Ok((code, body)),
        }
    }
}

fn signed_in() -> Conf {
    Conf {
        server: SERVER.into(),
        token: "rmm_abc".into(),
        user: "admin".into(),
        device: "dae4".into(),
        art: Source::Unslot,
        saves: true,
    }
}

const PLATFORMS: &str = r#"[
  {"id":1,"slug":"gb","fs_slug":"gb","name":"Game Boy","display_name":"Game Boy","rom_count":2},
  {"id":4,"slug":"genesis","fs_slug":"megadrive","name":"Sega Mega Drive/Genesis","display_name":"Mega Drive","rom_count":1},
  {"id":8,"slug":"psx","fs_slug":"psx","name":"PlayStation","display_name":"PlayStation","rom_count":1},
  {"id":9,"slug":"ps3","fs_slug":"ps3","name":"PlayStation 3","display_name":"PlayStation 3","rom_count":4},
  {"id":10,"slug":"nes","fs_slug":"nes","name":"Nintendo","display_name":"NES","rom_count":0}
]"#;

const GB_ROMS: &str = r#"{"items":[
  {"id":1,"platform_id":1,"platform_slug":"gb","fs_name":"Tennis (World).gb","fs_name_no_ext":"Tennis (World)","fs_extension":"gb","fs_size_bytes":32768,"name":"Tennis","path_cover_small":"/assets/romm/resources/roms/1/1/cover/small.png?ts=2026-11-22 22:15:52","path_cover_large":"/assets/romm/resources/roms/1/1/cover/big.png?ts=2026-11-22 22:15:52","has_multiple_files":false,"files":[]},
  {"id":2,"platform_id":1,"platform_slug":"gb","fs_name":"Tetris (World) (Rev 1).gb","fs_name_no_ext":"Tetris (World) (Rev 1)","fs_extension":"gb","fs_size_bytes":32768,"name":"Tetris","path_cover_small":null,"path_cover_large":null,"has_multiple_files":false,"files":[]}
],"total":2,"limit":500,"offset":0}"#;

const DISC_ROM: &str = r#"{"id":10,"fs_name":"Fake Disc Game (USA)","full_path":"roms/psx/Fake Disc Game (USA)","files":[
  {"id":13,"file_name":"Fake Disc Game (USA) (Disc 1).bin","file_path":"roms/psx/Fake Disc Game (USA)"},
  {"id":11,"file_name":"Fake Disc Game (USA) (Disc 1).cue","file_path":"roms/psx/Fake Disc Game (USA)"},
  {"id":12,"file_name":"Fake Disc Game (USA) (Disc 2).bin","file_path":"roms/psx/Fake Disc Game (USA)"},
  {"id":10,"file_name":"Fake Disc Game (USA) (Disc 2).cue","file_path":"roms/psx/Fake Disc Game (USA)"}
]}"#;

#[test]
fn a_server_typed_without_a_scheme_is_asked_over_http() {
    assert_eq!(
        romm::normalise(" 192.168.1.4:8080/ ").as_deref(),
        Some("http://192.168.1.4:8080")
    );
    assert_eq!(
        romm::normalise("https://romm.example.com").as_deref(),
        Some("https://romm.example.com")
    );
    assert_eq!(romm::normalise(""), None);
    assert_eq!(romm::host("https://romm.example.com"), "romm.example.com");
}

#[test]
fn the_sign_in_and_the_choice_of_art_are_kept_on_the_card() {
    let d = tempfile::tempdir().unwrap();
    assert!(!romm::read(d.path()).signed_in());
    let conf = Conf {
        art: Source::Romm,
        ..signed_in()
    };
    romm::write(d.path(), &conf).unwrap();
    assert_eq!(romm::read(d.path()), conf);
    assert!(romm::read(d.path()).art_from_romm());

    romm::sign_out(d.path()).unwrap();
    let out = romm::read(d.path());
    assert!(!out.signed_in() && !out.art_from_romm());
    assert_eq!((out.server.as_str(), out.art), (SERVER, Source::Romm));
}

#[test]
fn signing_in_shows_a_code_and_waits_for_it_to_be_approved() {
    let mut http = Fake::default()
        .on(
            "/api/auth/device/init",
            200,
            r#"{"device_code":"dc1","user_code":"JNPZ4Y2F","verification_path":"/pair/device","verification_path_complete":"/pair/device?user_code=JNPZ4Y2F","expires_in":600,"interval":5}"#,
        )
        .on("/api/auth/device/token", 400, r#"{"detail":"authorization_pending"}"#);
    let p = romm::pair(&mut http, SERVER, "cube").unwrap();
    assert_eq!(p.user_code, "JNPZ4Y2F");
    assert_eq!(p.url, format!("{SERVER}/pair/device?user_code=JNPZ4Y2F"));
    assert_eq!(p.interval.as_secs(), 5);
    assert_eq!(romm::token(&mut http, SERVER, "dc1"), Ok(None));

    let mut http = http.on(
        "/api/auth/device/token",
        200,
        r#"{"access_token":"rmm_04c1","device_id":"dae4","scopes":["roms.read"],"expires_at":null}"#,
    );
    assert_eq!(
        romm::token(&mut http, SERVER, "dc1"),
        Ok(Some(romm::Approved {
            token: "rmm_04c1".into(),
            device: "dae4".into(),
        }))
    );

    let mut http = Fake::default().on(
        "/api/auth/device/token",
        400,
        r#"{"detail":"expired_token"}"#,
    );
    assert!(romm::token(&mut http, SERVER, "dc1").is_err());
}

#[test]
fn the_library_lists_the_systems_unslot_plays_that_have_games() {
    let mut http = Fake::default().on("/api/platforms", 200, PLATFORMS);
    let shelves = romm::shelves(&mut http, &signed_in()).unwrap();
    let got: Vec<_> = shelves
        .iter()
        .map(|s| (s.name.as_str(), s.platform))
        .collect();
    assert_eq!(
        got,
        [
            ("Game Boy", Platform::Gb),
            ("Mega Drive", Platform::Genesis),
            ("PlayStation", Platform::PlayStation),
        ]
    );
}

#[test]
fn romm_names_its_systems_as_grout_and_its_folders_do() {
    assert_eq!(romm::platform_for("tg16", ""), Some(Platform::PcEngine));
    assert_eq!(
        romm::platform_for("neo-geo-pocket-color", ""),
        Some(Platform::NeoGeoPocket)
    );
    assert_eq!(
        romm::platform_for("custom", "megadrive"),
        Some(Platform::Genesis)
    );
    // A folder named the way unslot names it.
    assert_eq!(
        romm::platform_for("custom", "SGFX"),
        Some(Platform::SuperGrafx)
    );
    assert_eq!(romm::platform_for("ps3", "ps3"), None);
    assert_eq!(romm::platform_for("ports", "ports"), None);
}

#[test]
fn a_systems_games_come_with_their_covers_and_where_they_go() {
    let mut http = Fake::default().on("/api/platforms", 200, PLATFORMS).on(
        "/api/roms?platform_ids=1",
        200,
        GB_ROMS,
    );
    let conf = signed_in();
    let gb = romm::shelves(&mut http, &conf).unwrap().remove(0);
    let games = romm::games(&mut http, &conf, &gb).unwrap();
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].name, "Tennis");
    assert_eq!(
        games[0].cover.as_deref(),
        Some("/assets/romm/resources/roms/1/1/cover/big.png")
    );
    assert_eq!(games[1].cover, None);
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        games[0].path(d.path()),
        d.path().join("Games/GB/Tennis (World).gb")
    );
    assert_eq!(games[0].stem(), "Tennis (World)");
}

#[test]
fn a_game_is_fetched_into_its_systems_folder() {
    let mut http = Fake::default()
        .on("/api/platforms", 200, PLATFORMS)
        .on("/api/roms?platform_ids=1", 200, GB_ROMS)
        .on(
            "/api/roms/1/content/Tennis%20%28World%29.gb",
            200,
            b"GBROM".to_vec(),
        );
    let conf = signed_in();
    let gb = romm::shelves(&mut http, &conf).unwrap().remove(0);
    let tennis = romm::games(&mut http, &conf, &gb).unwrap().remove(0);
    let d = tempfile::tempdir().unwrap();
    assert!(!tennis.on_card(d.path()));
    romm::download(&mut http, &conf, d.path(), &tennis).unwrap();
    assert!(tennis.on_card(d.path()));
    assert_eq!(
        std::fs::read(d.path().join("Games/GB/Tennis (World).gb")).unwrap(),
        b"GBROM"
    );
    let carts = slot_store::scan(d.path()).unwrap();
    assert_eq!(carts.len(), 1);
    assert_eq!(carts[0].stem, tennis.stem());
}

#[test]
fn a_failed_fetch_leaves_nothing_on_the_card() {
    let mut http = Fake::default().on("/api/roms/1/content/", 500, b"oops".to_vec());
    let game = romm::Game {
        id: 1,
        name: "Tennis".into(),
        platform: Platform::Gb,
        fs_name: "Tennis (World).gb".into(),
        fs_name_no_ext: "Tennis (World)".into(),
        size: 5,
        multi: false,
        cover: None,
    };
    let d = tempfile::tempdir().unwrap();
    assert!(romm::download(&mut http, &signed_in(), d.path(), &game).is_err());
    let left: Vec<_> = std::fs::read_dir(d.path().join("Games/GB"))
        .unwrap()
        .collect();
    assert!(left.is_empty());
}

#[test]
fn a_game_of_many_discs_goes_in_a_folder_with_a_playlist() {
    let mut http = Fake::default().on("/api/roms/10", 200, DISC_ROM).on(
        "/api/roms/10/content/",
        200,
        b"DISC".to_vec(),
    );
    let game = romm::Game {
        id: 10,
        name: "Fake Disc Game".into(),
        platform: Platform::PlayStation,
        fs_name: "Fake Disc Game (USA)".into(),
        fs_name_no_ext: "Fake Disc Game (USA)".into(),
        size: 8376,
        multi: true,
        cover: None,
    };
    let d = tempfile::tempdir().unwrap();
    romm::download(&mut http, &signed_in(), d.path(), &game).unwrap();
    let dir = d.path().join("Games/PS/Fake Disc Game (USA)");
    assert_eq!(
        std::fs::read_to_string(dir.join("Fake Disc Game (USA).m3u")).unwrap(),
        "Fake Disc Game (USA) (Disc 1).cue\nFake Disc Game (USA) (Disc 2).cue\n"
    );
    // Each file is asked for by its own id, as RomM's names for them can cross.
    let asked = http.asked.lock().unwrap().clone();
    assert!(asked.iter().any(|u| u.ends_with("?file_ids=13")));
    let carts = slot_store::scan(d.path()).unwrap();
    assert_eq!(carts.len(), 1);
    assert_eq!(carts[0].stem, game.stem());
    assert!(!d
        .path()
        .join("Games/PS/.Fake Disc Game (USA).part")
        .exists());
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().unwrap();
    writer
        .write_image_data(&vec![200u8; (w * h * 4) as usize])
        .unwrap();
    drop(writer);
    out
}

#[test]
fn a_card_game_takes_its_label_from_its_cover_in_romm() {
    let mut http = Fake::default()
        .on("/api/platforms", 200, PLATFORMS)
        .on("/api/roms?platform_ids=1", 200, GB_ROMS)
        .on("/api/roms?platform_ids=4", 200, r#"{"items":[],"total":0}"#)
        .on("/api/roms?platform_ids=8", 200, r#"{"items":[],"total":0}"#)
        .on(
            "/assets/romm/resources/roms/1/1/cover/big.png",
            200,
            png(81, 108),
        );
    let d = tempfile::tempdir().unwrap();
    let games = d.path().join("Games/GB");
    std::fs::create_dir_all(&games).unwrap();
    std::fs::write(games.join("Tennis (World).gb"), b"x").unwrap();
    std::fs::write(games.join("Tetris (World) (Rev 1).gb"), b"x").unwrap();
    std::fs::write(games.join("Homebrew.gb"), b"x").unwrap();
    let carts = slot_store::scan(d.path()).unwrap();
    let mut library = None;
    let conf = signed_in();
    let labels: Vec<bool> = carts
        .iter()
        .map(|c| {
            romm::label_for(&mut http, &conf, &mut library, c)
                .unwrap()
                .is_some()
        })
        .collect();
    // Homebrew is not in the library and Tetris has no cover there.
    assert_eq!(labels, [false, true, false]);
    let asked = http.asked.lock().unwrap().clone();
    assert_eq!(
        asked
            .iter()
            .filter(|u| u.ends_with("/api/platforms"))
            .count(),
        1,
        "the library is asked for once a run"
    );
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

/// Ticks the app until `done` holds, so RomM's thread has had its say.
fn until(a: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut now = 0;
    while !done(a) && Instant::now() < deadline {
        now += 16;
        a.tick_ms(now);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(done(a), "it never settled: {:?}", a.phase());
}

fn list(a: &App) -> ListPage {
    match a.page_model() {
        Some(PageModel::List(list)) => list,
        other => panic!("not on a list: {other:?}"),
    }
}

fn detail(a: &App, label: &str) -> String {
    list(a)
        .rows
        .iter()
        .find(|r| r.label == label)
        .map(|r| r.detail.clone())
        .unwrap_or_default()
}

/// Moves the cursor down `n` rows and presses A.
fn pick(a: &mut App, n: usize) {
    for _ in 0..n {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
}

/// Types lower-case letters, which the keyboard opens on, and presses Start.
fn type_text(a: &mut App, text: &str) {
    for ch in text.chars() {
        let Phase::Keyboard { keys, .. } = a.phase() else {
            panic!("not on the keyboard: {:?}", a.phase());
        };
        let (r, c) = (0..keys.rows().len())
            .find_map(|r| {
                (0..keys.rows()[r].len())
                    .find(|&c| keys.label(keys.rows()[r][c]) == ch.to_string())
                    .map(|c| (r, c))
            })
            .unwrap_or_else(|| panic!("no {ch:?} key"));
        let at = |a: &App| match a.phase() {
            Phase::Keyboard { keys, .. } => (keys.row(), keys.col()),
            _ => unreachable!(),
        };
        while at(a).0 != r {
            press(a, if at(a).0 < r { Btn::Down } else { Btn::Up });
        }
        while at(a).1 != c {
            press(a, if at(a).1 < c { Btn::Right } else { Btn::Left });
        }
        press(a, Btn::A);
    }
    press(a, Btn::Start);
}

/// Opens RomM under Online, on a card whose RomM settings are `conf`.
fn on_romm_page(conf: &Conf) -> (tempfile::TempDir, App) {
    on_romm_page_with(
        conf,
        SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
}

/// Opens RomM, as `on_romm_page`, with the device's settings `set`.
fn on_romm_page_with(conf: &Conf, set: SlotState) -> (tempfile::TempDir, App) {
    // Two games, or slot goes straight into the one there is.
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(d.path(), &set).unwrap();
    romm::write(d.path(), conf).unwrap();
    let (mut a, _clock) = app_booting_at(d.path(), CLOCK_IS_SET);
    a.apply(Action::QuickMenu);
    for (rows, want) in [
        (QuickRow::MAIN.len(), QuickRow::Online),
        (QuickRow::ONLINE.len(), QuickRow::Romm),
    ] {
        for _ in 0..rows {
            if a.quick_menu() == Some(want) {
                break;
            }
            press(&mut a, Btn::Down);
        }
        assert_eq!(a.quick_menu(), Some(want));
        press(&mut a, Btn::A);
    }
    assert!(matches!(a.phase(), Phase::Romm { .. }), "{:?}", a.phase());
    (d, a)
}

#[test]
fn the_device_signs_in_to_romm_by_a_code_and_fetches_a_game_onto_the_shelf() {
    let (d, mut a) = on_romm_page(&Conf::default());
    assert_eq!(detail(&a, "Server"), "Not set");
    assert_eq!(detail(&a, "Account"), "Sign in");

    pick(&mut a, 1);
    assert_eq!(list(&a).status, "Set the server first");
    press(&mut a, Btn::Up);
    press(&mut a, Btn::A);
    type_text(&mut a, "romm");
    assert_eq!(detail(&a, "Server"), "romm");
    assert_eq!(romm::read(d.path()).server, "http://romm");

    // The code shows until the phone approves it, then the page says who signed in.
    pick(&mut a, 1);
    until(&mut a, |a| match a.page_model() {
        Some(PageModel::Qr(qr)) => qr.url.ends_with(SimRomm::CODE),
        _ => false,
    });
    until(&mut a, |a| {
        matches!(a.page_model(), Some(PageModel::List(_)))
    });
    assert_eq!(detail(&a, "Account"), "player");
    assert!(romm::read(d.path()).signed_in());

    pick(&mut a, 1);
    assert_eq!(detail(&a, "Game Art"), "RomM");
    assert!(romm::read(d.path()).art_from_romm());
    // The sign in kept the device RomM knows it as, for its saves.
    assert_eq!(romm::read(d.path()).device, SimRomm::DEVICE);
    assert_eq!(detail(&a, "Save Sync"), "Off");

    pick(&mut a, 2);
    until(&mut a, |a| detail(a, "Game Boy") == "1 game");
    press(&mut a, Btn::A);
    until(&mut a, |a| detail(a, "Sim Game") == "1 KB");
    press(&mut a, Btn::A);
    until(&mut a, |a| detail(a, "Sim Game") == "On card");
    assert!(d.path().join("Games/GB").join(SimRomm::GAME).is_file());
    assert_eq!(list(&a).status, "Sim Game is on the shelf");

    // B goes back a page at a time.
    press(&mut a, Btn::B);
    assert_eq!(list(&a).title, "RomM Library");
    press(&mut a, Btn::B);
    assert_eq!(list(&a).title, "RomM");
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::Romm));
}

#[test]
fn signing_out_of_romm_keeps_the_server_and_closes_the_library() {
    let (d, mut a) = on_romm_page(&Conf {
        server: "http://romm".into(),
        token: "rmm_sim".into(),
        user: "player".into(),
        device: SimRomm::DEVICE.into(),
        art: Source::Romm,
        saves: false,
    });
    assert_eq!(detail(&a, "Account"), "player");
    pick(&mut a, 1);
    assert_eq!(list(&a).status, "Signed out of RomM");
    assert_eq!(detail(&a, "Account"), "Sign in");
    assert_eq!(detail(&a, "Server"), "romm");
    pick(&mut a, 3);
    assert_eq!(list(&a).status, "Sign in first");
    let conf = romm::read(d.path());
    assert_eq!(
        (conf.server.as_str(), conf.signed_in()),
        ("http://romm", false)
    );
    assert!(conf.device.is_empty());
    // The choice of art is kept, though it takes unslot's until signed in again.
    assert_eq!(conf.art, Source::Romm);
    assert!(!conf.art_from_romm());
}

/// A RomM whose library is `GB_ROMS`.
fn library() -> Fake {
    Fake::default()
        .on("/api/platforms", 200, PLATFORMS)
        .on("/api/roms?platform_ids=1", 200, GB_ROMS)
        .on("/api/roms?platform_ids=4", 200, r#"{"items":[],"total":0}"#)
        .on("/api/roms?platform_ids=8", 200, r#"{"items":[],"total":0}"#)
        .on("/api/sync/sessions/5/complete", 200, "{}")
        .on("/api/saves?", 201, r#"{"id":3}"#)
}

/// A card with the library's Game Boy games, each with its save if it has one.
fn card_with(games: &[(&str, Option<&[u8]>)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("Games/GB")).unwrap();
    std::fs::create_dir_all(d.path().join("Saves/GB")).unwrap();
    for (stem, sav) in games {
        std::fs::write(d.path().join(format!("Games/GB/{stem}.gb")), b"GBROM").unwrap();
        if let Some(sav) = sav {
            std::fs::write(d.path().join(format!("Saves/GB/{stem}.sav")), sav).unwrap();
        }
    }
    d
}

fn json_of(body: &Option<String>) -> serde_json::Value {
    serde_json::from_str(body.as_deref().unwrap()).unwrap()
}

#[test]
fn a_save_only_on_the_card_goes_to_romm_in_grouts_slot() {
    let d = card_with(&[
        ("Tennis (World)", Some(b"SRAM")),
        ("Tetris (World) (Rev 1)", None),
    ]);
    let mut http = library().on(
        "/api/sync/negotiate",
        200,
        r#"{"session_id":5,"operations":[{"action":"upload","rom_id":1,"save_id":null,"slot":"autosave"}]}"#,
    );
    let synced = romm::sync_saves(&mut http, &signed_in(), d.path()).unwrap();
    assert_eq!((synced.sent, synced.fetched, synced.failed), (1, 0, 0));

    let asked = http.sent_to("POST /api/sync/negotiate");
    let asked = json_of(&asked[0].1);
    assert_eq!(asked["device_id"], "dae4");
    assert_eq!(asked["rom_ids"], serde_json::json!([1, 2]));
    let save = &asked["saves"][0];
    assert_eq!(
        asked["saves"].as_array().unwrap().len(),
        1,
        "Tetris has no save"
    );
    assert_eq!(save["rom_id"], 1);
    assert_eq!(save["slot"], "autosave");
    assert_eq!(save["content_hash"], "76bc59a384b8d14a16d9060104ffa81f");
    assert_eq!(save["file_size_bytes"], 4);
    let core = slot_store::core_for_platform(d.path(), "Tennis (World)", Platform::Gb);
    assert_eq!(save["emulator"], core.as_str());
    assert!(save["updated_at"].as_str().unwrap().ends_with('Z'));

    let up = http.sent_to("POST /api/saves?");
    assert_eq!(up.len(), 1);
    let (url, _, file) = &up[0];
    assert!(
        url.contains("rom_id=1&") && url.contains("&slot=autosave"),
        "{url}"
    );
    assert!(
        url.contains("&session_id=5") && url.contains("&device_id=dae4"),
        "{url}"
    );
    assert!(!url.contains("overwrite"), "{url}");
    assert_eq!(file.as_deref(), Some(&b"SRAM"[..]));

    let done = http.sent_to("POST /api/sync/sessions/5/complete");
    assert_eq!(json_of(&done[0].1)["operations_completed"], 1);
}

#[test]
fn a_newer_save_from_romm_comes_to_the_card_and_puts_its_resume_aside() {
    let d = card_with(&[("Tennis (World)", Some(b"SRAM"))]);
    let core = slot_store::core_for_platform(d.path(), "Tennis (World)", Platform::Gb);
    let ring = StateRing::new(d.path(), Platform::Gb, core, "Tennis (World)");
    ring.write_resume(b"old state").unwrap();
    let mut http = library()
        .on(
            "/api/sync/negotiate",
            200,
            r#"{"session_id":5,"operations":[{"action":"download","rom_id":1,"save_id":7}]}"#,
        )
        .on("/api/saves/7/content", 200, b"NEWER".to_vec())
        .on("/api/saves/7/downloaded", 200, "{}");
    let synced = romm::sync_saves(&mut http, &signed_in(), d.path()).unwrap();
    assert_eq!((synced.sent, synced.fetched), (0, 1));
    assert_eq!(
        slot::persist::read_sav(d.path(), Platform::Gb, "Tennis (World)").unwrap(),
        Some(b"NEWER".to_vec())
    );
    // The resume would have put the old save back as it loaded.
    assert_eq!(ring.read_resume().unwrap(), None);
    let got = http.sent_to("GET /api/saves/7/content");
    assert!(got[0].0.contains("optimistic=false"), "{}", got[0].0);
    let told = http.sent_to("POST /api/saves/7/downloaded");
    assert_eq!(
        json_of(&told[0].1)["content_hash"],
        "c62122d172b000396b6b38a9e959fad3"
    );
}

#[test]
fn of_two_saves_changed_apart_the_newer_wins_and_the_other_is_kept_in_romm() {
    let d = card_with(&[
        ("Tennis (World)", Some(b"CARD1")),
        ("Tetris (World) (Rev 1)", Some(b"CARD2")),
    ]);
    let mut http = library()
        .on(
            "/api/sync/negotiate",
            200,
            r#"{"session_id":5,"operations":[
              {"action":"conflict","rom_id":1,"save_id":7,"server_updated_at":"2099-01-01T00:00:00+00:00"},
              {"action":"conflict","rom_id":2,"save_id":8,"server_updated_at":"2000-01-01T00:00:00Z"}
            ]}"#,
        )
        .on("/api/saves/7/content", 200, b"ROMM1".to_vec())
        .on("/api/saves/7/downloaded", 200, "{}");
    let synced = romm::sync_saves(&mut http, &signed_in(), d.path()).unwrap();
    assert_eq!((synced.sent, synced.fetched, synced.failed), (1, 1, 0));
    let up = http.sent_to("POST /api/saves?");
    let of = |rom: &str| {
        up.iter()
            .find(|(u, ..)| u.contains(&format!("rom_id={rom}&")))
            .unwrap()
            .clone()
    };
    // RomM's is newer: the card's goes to RomM out of the slot, then RomM's comes.
    let (url, _, file) = of("1");
    assert!(!url.contains("slot="), "{url}");
    assert_eq!(file.as_deref(), Some(&b"CARD1"[..]));
    assert_eq!(
        slot::persist::read_sav(d.path(), Platform::Gb, "Tennis (World)").unwrap(),
        Some(b"ROMM1".to_vec())
    );
    // The card's is newer: it takes the slot, where RomM keeps the one before.
    let (url, _, file) = of("2");
    assert!(
        url.contains("&slot=autosave") && url.contains("&overwrite=true"),
        "{url}"
    );
    assert_eq!(file.as_deref(), Some(&b"CARD2"[..]));
}

#[test]
fn the_saves_of_systems_played_outside_slot_stay_out_of_the_sync() {
    let d = tempfile::tempdir().unwrap();
    let dir = Platform::Nds.dir_name();
    std::fs::create_dir_all(d.path().join(format!("Games/{dir}"))).unwrap();
    std::fs::create_dir_all(d.path().join(format!("Saves/{dir}"))).unwrap();
    std::fs::write(d.path().join(format!("Games/{dir}/Game.nds")), b"x").unwrap();
    std::fs::write(d.path().join(format!("Saves/{dir}/Game.sav")), b"x").unwrap();
    let mut http = Fake::default()
        .on(
            "/api/platforms",
            200,
            r#"[{"id":20,"slug":"nds","fs_slug":"nds","name":"Nintendo DS","display_name":"DS","rom_count":1}]"#,
        )
        .on(
            "/api/roms?platform_ids=20",
            200,
            r#"{"items":[{"id":40,"fs_name":"Game.nds","fs_name_no_ext":"Game","name":"Game"}],"total":1}"#,
        )
        .on("/api/sync/negotiate", 200, r#"{"session_id":5,"operations":[]}"#)
        .on("/api/sync/sessions/5/complete", 200, "{}");
    romm::sync_saves(&mut http, &signed_in(), d.path()).unwrap();
    let asked = json_of(&http.sent_to("POST /api/sync/negotiate")[0].1);
    assert_eq!(asked["saves"], serde_json::json!([]));
    assert_eq!(asked["rom_ids"], serde_json::json!([]));
}

#[test]
fn romms_times_are_read_with_their_offsets() {
    assert_eq!(romm::parse_rfc3339("1970-01-02T00:00:00Z"), Some(86_400));
    assert_eq!(
        romm::parse_rfc3339("1970-01-02T01:00:00.123456+01:00"),
        Some(86_400)
    );
    assert_eq!(
        romm::parse_rfc3339("1970-01-01T23:00:00-01:00"),
        Some(86_400)
    );
    assert_eq!(romm::parse_rfc3339("soon"), None);
}

const TENNIS: &str = r#"{"id":1,"platform_id":1,"platform_slug":"gb","platform_fs_slug":"gb","platform_display_name":"Game Boy","fs_name":"Tennis (World).gb","fs_name_no_ext":"Tennis (World)","fs_size_bytes":5,"name":"Tennis","has_multiple_files":false}"#;

#[test]
fn games_sent_to_the_device_from_romm_land_on_the_shelf() {
    let mut http = Fake::default()
        .on(
            "/api/devices/dae4/installs/claim",
            200,
            r#"[{"id":"r1","rom_id":1},{"id":"r2","rom_id":30},{"id":"r3","rom_id":1}]"#,
        )
        .on("/api/devices/dae4/installs/", 200, "{}")
        .on("/api/roms/1", 200, TENNIS)
        .on("/api/roms/1/content/", 200, b"GBROM".to_vec())
        .on(
            "/api/roms/30",
            200,
            r#"{"id":30,"platform_slug":"ps3","platform_fs_slug":"ps3","platform_display_name":"PlayStation 3","fs_name":"Game.iso"}"#,
        );
    let d = tempfile::tempdir().unwrap();
    let got = romm::installs(&mut http, &signed_in(), d.path()).unwrap();
    assert_eq!(got, ["Tennis"]);
    let carts = slot_store::scan(d.path()).unwrap();
    assert_eq!(carts.len(), 1);
    assert_eq!(carts[0].stem, "Tennis (World)");

    let told = |id: &str| {
        let put = http.sent_to(&format!("PUT /api/devices/dae4/installs/{id}"));
        json_of(&put[0].1)
    };
    assert_eq!(told("r1")["status"], "done");
    assert_eq!(told("r2")["status"], "failed");
    assert_eq!(
        told("r2")["reason"],
        "unslot has no shelf for PlayStation 3"
    );
    assert_eq!(told("r3")["status"], "already_installed");
}

fn zipped(fs_name: &str) -> romm::Game {
    romm::Game {
        id: 1,
        name: "Tennis".into(),
        platform: Platform::Gb,
        fs_name: fs_name.into(),
        fs_name_no_ext: "Tennis (World)".into(),
        size: 5,
        multi: false,
        cover: None,
    }
}

fn left_in(d: &tempfile::TempDir, dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(d.path().join(dir))
        .map(|r| {
            r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn a_zipped_game_for_a_system_that_plays_no_zips_is_unpacked_onto_the_shelf() {
    let zip = stored_zip(&[("Tennis (World).gb", "GBROM")]);
    let mut http = Fake::default().on("/api/roms/1/content/", 200, zip);
    let d = tempfile::tempdir().unwrap();
    let game = zipped("Tennis (World).zip");
    let rom = romm::download(&mut http, &signed_in(), d.path(), &game).unwrap();
    assert_eq!(rom, d.path().join("Games/GB/Tennis (World).gb"));
    assert_eq!(left_in(&d, "Games/GB"), ["Tennis (World).gb"]);
    assert!(game.on_card(d.path()));
    assert_eq!(slot_store::scan(d.path()).unwrap().len(), 1);
}

#[test]
fn a_game_the_system_cannot_play_is_not_left_on_the_card() {
    let zip = stored_zip(&[("readme.txt", "hi")]);
    let mut http = Fake::default().on("/api/roms/1/content/", 200, zip);
    let d = tempfile::tempdir().unwrap();
    let e = romm::download(
        &mut http,
        &signed_in(),
        d.path(),
        &zipped("Tennis (World).zip"),
    );
    assert!(e.is_err());
    assert!(
        left_in(&d, "Games/GB").is_empty(),
        "{:?}",
        left_in(&d, "Games/GB")
    );

    let e = romm::download(
        &mut http,
        &signed_in(),
        d.path(),
        &zipped("Tennis (World).iso"),
    );
    assert_eq!(e, Err("unslot cannot play .iso games for Game Boy".into()));
    assert_eq!(http.asked.lock().unwrap().len(), 1, "the .iso was fetched");
}

fn sim_signed_in(saves: bool, device: &str) -> Conf {
    Conf {
        server: "http://romm".into(),
        token: "rmm_sim".into(),
        user: "player".into(),
        device: device.into(),
        art: Source::Unslot,
        saves,
    }
}

#[test]
fn turning_romms_save_sync_on_turns_syncthing_off() {
    let (d, mut a) = on_romm_page_with(
        &sim_signed_in(false, SimRomm::DEVICE),
        SlotState {
            clock_set: true,
            syncthing: true,
            password: Some("pass".into()),
            ..SlotState::default()
        },
    );
    assert_eq!(detail(&a, "Save Sync"), "Off");
    pick(&mut a, 3);
    assert_eq!(detail(&a, "Save Sync"), "On");
    assert_eq!(
        list(&a).status,
        "Syncthing is off now: it and RomM would both change saves"
    );
    assert!(romm::read(d.path()).syncs_saves());
    assert!(!read_slot_state(d.path()).syncthing);
    let legends: Vec<String> = list(&a).legends.iter().map(|l| l.label.clone()).collect();
    assert!(legends.contains(&"Sync Now".to_string()), "{legends:?}");

    press(&mut a, Btn::A);
    assert_eq!(detail(&a, "Save Sync"), "Off");
    assert!(!romm::read(d.path()).saves);
}

#[test]
fn save_sync_needs_a_sign_in_that_named_the_device() {
    let (d, mut a) = on_romm_page(&sim_signed_in(false, ""));
    pick(&mut a, 3);
    assert_eq!(
        list(&a).status,
        "Sign in again for RomM to know this device"
    );
    assert!(!romm::read(d.path()).saves);
}

#[test]
fn the_device_tells_romm_it_takes_games_sent_to_it() {
    let mut http = Fake::default().on("/api/devices/dae4", 200, "{}");
    romm::announce(&mut http, &signed_in()).unwrap();
    let put = http.sent_to("PUT /api/devices/dae4");
    assert_eq!(json_of(&put[0].1)["capabilities"]["remote_install"], true);
}
