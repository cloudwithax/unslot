//! Achievements earned with no connection: rcheevos' client, through `Offline`, against a
//! server that comes and goes.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slot_cheevos::offline::Offline;
use slot_cheevos::{Cheevos, Console, Http, HttpRequest, Notice};

const GAME: &str = r#"{"Success":true,"GameId":100,"Title":"Test Game","ConsoleId":7,"ImageIconUrl":"https://media.retroachievements.org/Images/000001.png","RichPresenceGameId":100,"RichPresencePatch":"","Sets":[{"AchievementSetId":1,"GameId":100,"Title":"Test Game","Type":"core","ImageIconUrl":"https://media.retroachievements.org/Images/000001.png","Achievements":[{"ID":5001,"Title":"First","Description":"Set the byte","Flags":3,"Points":10,"MemAddr":"0xH0000=1","Author":"me","BadgeName":"00001","Created":1700000000,"Modified":1700000000},{"ID":5002,"Title":"Second","Description":"Set the other","Flags":3,"Points":5,"MemAddr":"0xH0001=1","Author":"me","BadgeName":"00002","Created":1700000000,"Modified":1700000000}],"Leaderboards":[]}]}"#;

/// retroachievements.org, while `online`.
#[derive(Default)]
struct Server {
    online: AtomicBool,
    seen: Mutex<Vec<String>>,
}

impl Http for Server {
    fn send(&self, request: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
        if !self.online.load(Ordering::SeqCst) {
            return Err("RetroAchievements could not be reached".into());
        }
        let post = request.post.clone().unwrap_or_default();
        self.seen.lock().unwrap().push(post.clone());
        let r = post
            .split('&')
            .find_map(|kv| kv.strip_prefix("r="))
            .unwrap_or("");
        let body = match r {
            "login2" => r#"{"Success":true,"User":"Benni","Token":"t0k3n","Score":10,"SoftcoreScore":2,"Messages":0,"Permissions":1,"AccountType":"Registered"}"#.to_string(),
            "achievementsets" => GAME.to_string(),
            "startsession" => r#"{"Success":true,"Unlocks":[],"HardcoreUnlocks":[],"ServerNow":1700000000}"#.to_string(),
            "awardachievement" => {
                let a = post.split('&').find_map(|kv| kv.strip_prefix("a=")).unwrap_or("0");
                format!(r#"{{"Success":true,"Score":20,"SoftcoreScore":12,"AchievementID":{a},"AchievementsRemaining":1}}"#)
            }
            "ping" => r#"{"Success":true}"#.to_string(),
            _ => r#"{"Success":false,"Error":"Unknown","Code":"not_found","Status":404}"#.to_string(),
        };
        Ok((200, body.into_bytes()))
    }
}

impl Server {
    fn awards(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.contains("r=awardachievement"))
            .cloned()
            .collect()
    }
}

fn wait(c: &Cheevos, mut want: impl FnMut(&Notice) -> bool) -> Notice {
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < Duration::from_secs(10) {
        for n in c.take() {
            if want(&n) {
                return n;
            }
            seen.push(n);
        }
        c.idle();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no such notice; saw {seen:?}");
}

/// Runs frames until `want` comes, or a second has gone by.
fn play(c: &Cheevos, want: impl Fn(&Notice) -> bool) -> Vec<Notice> {
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < Duration::from_secs(1) {
        c.frame();
        seen.extend(c.take());
        if seen.iter().any(&want) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    seen
}

/// Signs in, loads the game and maps `ram` as the NES' memory.
fn start(http: Arc<dyn Http>, rom: &Path, ram: &mut [u8], hardcore: bool) -> (Cheevos, Notice) {
    let mut c = Cheevos::new(http).unwrap();
    let (ptr, len) = (ram.as_mut_ptr(), ram.len());
    let mapped = unsafe {
        c.set_memory(std::ptr::null(), 0, Console::NES, &mut |id| match id {
            2 => Some((ptr, len)),
            _ => None,
        })
    };
    assert!(mapped >= 0x800);
    c.login("Benni", "t0k3n");
    assert_eq!(
        wait(&c, |n| matches!(n, Notice::LoggedIn(_))),
        Notice::LoggedIn(Ok(()))
    );
    c.set_hardcore(hardcore);
    c.load_game(rom, Console::NES);
    let loaded = wait(&c, |n| matches!(n, Notice::GameLoaded(_)));
    (c, loaded)
}

fn unlocked(seen: &[Notice], title: &str) -> bool {
    seen.iter()
        .any(|n| matches!(n, Notice::Unlocked { title: t, .. } if t == title))
}

#[test]
fn achievements_are_earned_offline_and_sent_once_back_online() {
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("game.nes");
    std::fs::write(&rom, (0..=255u8).cycle().take(1024).collect::<Vec<_>>()).unwrap();
    let dir = d.path().join("ra");
    let server = Arc::new(Server::default());
    let clock = Arc::new(AtomicU64::new(1_800_000_000));
    let tick = clock.clone();
    let offline = Arc::new(Offline::with_clock(
        &dir,
        server.clone(),
        Box::new(move || tick.load(Ordering::SeqCst)),
    ));

    // Online once, which keeps the account and the game's achievements on the card.
    server.online.store(true, Ordering::SeqCst);
    let mut ram = vec![0u8; 0x800];
    let (c, loaded) = start(offline.clone(), &rom, &mut ram, false);
    assert!(matches!(loaded, Notice::GameLoaded(Ok(_))), "{loaded:?}");
    drop(c);

    // The Wi-Fi goes. The game still loads, and an unlock is queued instead of lost.
    server.online.store(false, Ordering::SeqCst);
    let mut ram = vec![0u8; 0x800];
    let (c, loaded) = start(offline.clone(), &rom, &mut ram, false);
    assert!(matches!(loaded, Notice::GameLoaded(Ok(_))), "{loaded:?}");
    play(&c, |_| false);
    ram[0] = 1;
    let seen = play(&c, |n| matches!(n, Notice::Unlocked { .. }));
    assert!(unlocked(&seen, "First"), "{seen:?}");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(Offline::pending(&dir), 1);
    assert!(server.awards().is_empty());
    drop(c);

    // Started again, still offline: what was earned is not earned twice.
    let mut ram = vec![0u8; 0x800];
    let (c, loaded) = start(offline.clone(), &rom, &mut ram, false);
    match &loaded {
        Notice::GameLoaded(Ok((summary, _))) => assert!(summary.contains('1'), "{summary}"),
        n => panic!("{n:?}"),
    }
    play(&c, |_| false);
    ram[0] = 1;
    let seen = play(&c, |n| matches!(n, Notice::Unlocked { .. }));
    assert!(!unlocked(&seen, "First"), "{seen:?}");
    assert_eq!(Offline::pending(&dir), 1);
    drop(c);

    // Back online an hour later: the unlock goes out, aged by the hour and signed to match.
    clock.fetch_add(3600, Ordering::SeqCst);
    server.online.store(true, Ordering::SeqCst);
    assert_eq!(offline.sync(), 1);
    assert_eq!(Offline::pending(&dir), 0);
    let awards = server.awards();
    assert_eq!(awards.len(), 1);
    let award = &awards[0];
    assert!(award.contains("a=5001"), "{award}");
    let o: u64 = award
        .split('&')
        .find_map(|kv| kv.strip_prefix("o="))
        .expect("an unlock time")
        .parse()
        .unwrap();
    assert!((3600..3700).contains(&o), "{award}");
    assert_eq!(offline.sync(), 0);
}

#[test]
fn hardcore_unlocks_survive_offline_restarts_after_a_softcore_session() {
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("game.nes");
    std::fs::write(&rom, (0..=255u8).cycle().take(1024).collect::<Vec<_>>()).unwrap();
    let dir = d.path().join("ra");
    let server = Arc::new(Server::default());
    let offline = Arc::new(Offline::with_clock(
        &dir,
        server.clone(),
        Box::new(|| 1_800_000_000),
    ));

    server.online.store(true, Ordering::SeqCst);
    let mut ram = vec![0u8; 0x800];
    let (c, loaded) = start(offline.clone(), &rom, &mut ram, false);
    assert!(matches!(loaded, Notice::GameLoaded(Ok(_))), "{loaded:?}");
    drop(c);

    server.online.store(false, Ordering::SeqCst);
    let mut ram = vec![0u8; 0x800];
    let (c, loaded) = start(offline.clone(), &rom, &mut ram, true);
    assert!(matches!(loaded, Notice::GameLoaded(Ok(_))), "{loaded:?}");
    play(&c, |_| false);
    ram[0] = 1;
    let seen = play(&c, |n| matches!(n, Notice::Unlocked { .. }));
    assert!(unlocked(&seen, "First"), "{seen:?}");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(Offline::pending(&dir), 1);
    let queued = walk(&dir.join("queue"));
    let body = std::fs::read_to_string(&queued[0]).unwrap();
    assert!(body.contains("&h=1&"), "{body}");
    drop(c);

    let mut ram = vec![0u8; 0x800];
    let (c, loaded) = start(offline, &rom, &mut ram, true);
    assert!(matches!(loaded, Notice::GameLoaded(Ok(_))), "{loaded:?}");
    play(&c, |_| false);
    ram[0] = 1;
    let seen = play(&c, |n| matches!(n, Notice::Unlocked { .. }));
    assert!(!unlocked(&seen, "First"), "{seen:?}");
    assert_eq!(Offline::pending(&dir), 1);
}

#[test]
fn a_queued_unlock_goes_out_with_the_next_request_that_gets_through() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("ra");
    let server = Arc::new(Server::default());
    let offline = Offline::new(&dir, server.clone());
    let award = HttpRequest {
        url: "https://retroachievements.org/dorequest.php".into(),
        post: Some("r=awardachievement&u=Benni&t=t0k3n&a=7&h=0&v=x".into()),
        content_type: Some("application/x-www-form-urlencoded".into()),
    };
    let (code, body) = offline.send(&award).unwrap();
    assert_eq!(code, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"Success\":true"));
    assert_eq!(Offline::pending(&dir), 1);

    server.online.store(true, Ordering::SeqCst);
    let ping = HttpRequest {
        post: Some("r=ping&u=Benni&t=t0k3n&g=100".into()),
        ..award
    };
    offline.send(&ping).unwrap();
    assert_eq!(Offline::pending(&dir), 0);
    assert_eq!(server.awards().len(), 1);
}

#[test]
fn a_password_sign_in_is_never_answered_from_the_card() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("ra");
    let server = Arc::new(Server::default());
    let offline = Offline::new(&dir, server.clone());
    let login = |body: &str| HttpRequest {
        url: "https://retroachievements.org/dorequest.php".into(),
        post: Some(body.into()),
        content_type: None,
    };
    server.online.store(true, Ordering::SeqCst);
    offline.send(&login("r=login2&u=Benni&p=secret")).unwrap();
    offline.send(&login("r=login2&u=Benni&t=t0k3n")).unwrap();
    server.online.store(false, Ordering::SeqCst);
    assert!(offline.send(&login("r=login2&u=Benni&p=secret")).is_err());
    assert!(offline.send(&login("r=login2&u=Benni&t=t0k3n")).is_ok());
    // The password is in nothing written to the card.
    for entry in walk(&dir) {
        let text = std::fs::read_to_string(&entry).unwrap_or_default();
        assert!(!text.contains("secret"), "{}", entry.display());
    }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        match p.is_dir() {
            true => out.extend(walk(&p)),
            false => out.push(p),
        }
    }
    out
}

struct Reply(u16, &'static str);
impl Http for Reply {
    fn send(&self, _: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
        Ok((self.0, self.1.as_bytes().to_vec()))
    }
}

fn award(id: u32) -> HttpRequest {
    HttpRequest {
        url: "https://retroachievements.org/dorequest.php".into(),
        post: Some(format!("r=awardachievement&u=Benni&t=tok&a={id}&h=0&v=x")),
        content_type: Some("application/x-www-form-urlencoded".into()),
    }
}

#[test]
fn unconfirmed_awards_survive_rate_limits_refusals_and_bad_responses() {
    let d = tempfile::tempdir().unwrap();
    let offline = Offline::new(d.path(), Arc::new(Server::default()));
    offline.send(&award(1)).unwrap();
    for (status, body) in [
        (429, "slow down"),
        (503, "later"),
        (200, "<html>proxy</html>"),
        (401, r#"{"Success":false}"#),
        (200, r#"{"Success":false,"Error":"retry"}"#),
    ] {
        let retry = Offline::new(d.path(), Arc::new(Reply(status, body)));
        assert_eq!(retry.sync(), 0);
        assert_eq!(Offline::pending(d.path()), 1);
    }
    let success = Offline::new(d.path(), Arc::new(Reply(200, "{\n\"Success\" : true\n}")));
    assert_eq!(success.sync(), 1);
    assert_eq!(Offline::pending(d.path()), 0);
}

#[test]
fn a_full_or_unwritable_queue_never_reports_an_award_as_saved() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("queue"), "not a directory").unwrap();
    let offline = Offline::new(d.path(), Arc::new(Server::default()));
    assert!(offline.send(&award(1)).is_err());
    assert!(!d.path().join("unlocks/Benni.txt").exists());
}

#[test]
fn independent_clients_do_not_overwrite_simultaneous_awards() {
    let d = tempfile::tempdir().unwrap();
    let a = Offline::with_clock(d.path(), Arc::new(Server::default()), Box::new(|| 100));
    let b = Offline::with_clock(d.path(), Arc::new(Server::default()), Box::new(|| 100));
    std::thread::scope(|s| {
        s.spawn(|| a.send(&award(1)).unwrap());
        s.spawn(|| b.send(&award(2)).unwrap());
    });
    assert_eq!(Offline::pending(d.path()), 2);
    let unlocks = std::fs::read_to_string(d.path().join("unlocks/Benni.txt")).unwrap();
    assert!(unlocks.contains("1 100\n"));
    assert!(unlocks.contains("2 100\n"));
    // A fresh wrapper at the same timestamp must not reuse an existing queue filename.
    drop(a);
    drop(b);
    Offline::with_clock(d.path(), Arc::new(Server::default()), Box::new(|| 100))
        .send(&award(3))
        .unwrap();
    assert_eq!(Offline::pending(d.path()), 3);
}

#[test]
fn a_refused_award_is_not_remembered_as_unlocked() {
    let d = tempfile::tempdir().unwrap();
    let offline = Offline::new(d.path(), Arc::new(Reply(200, r#"{"Success":false}"#)));
    offline.send(&award(1)).unwrap();
    assert!(!d.path().join("unlocks/Benni.txt").exists());
}

#[test]
fn separate_sync_workers_send_each_queued_award_once() {
    struct Slow(AtomicU64);
    impl Http for Slow {
        fn send(&self, _: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            Ok((200, br#"{"Success":true}"#.to_vec()))
        }
    }
    let d = tempfile::tempdir().unwrap();
    Offline::new(d.path(), Arc::new(Server::default()))
        .send(&award(1))
        .unwrap();
    let server = Arc::new(Slow(AtomicU64::new(0)));
    let a = Offline::new(d.path(), server.clone());
    let b = Offline::new(d.path(), server.clone());
    std::thread::scope(|s| {
        s.spawn(|| a.sync());
        s.spawn(|| b.sync());
    });
    assert_eq!(server.0.load(Ordering::SeqCst), 1);
    assert_eq!(Offline::pending(d.path()), 0);
}

#[test]
fn a_new_login_refreshes_the_token_of_old_queued_awards() {
    let d = tempfile::tempdir().unwrap();
    Offline::new(d.path(), Arc::new(Server::default()))
        .send(&award(1))
        .unwrap();
    let server = Arc::new(Server::default());
    server.online.store(true, Ordering::SeqCst);
    let offline = Offline::new(d.path(), server.clone());
    offline
        .send(&HttpRequest {
            url: "https://retroachievements.org/dorequest.php".into(),
            post: Some("r=login2&u=Benni&t=new-token".into()),
            content_type: None,
        })
        .unwrap();
    assert!(server.awards()[0].contains("t=t0k3n"));
    assert_eq!(Offline::pending(d.path()), 0);
}

#[test]
fn formatted_cached_json_merges_unlocks_without_duplicate_keys() {
    struct Session(AtomicBool);
    impl Http for Session {
        fn send(&self, _: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
            if !self.0.load(Ordering::SeqCst) {
                return Err("offline".into());
            }
            Ok((
                200,
                b"{\n  \"Success\" : true, \"Unlocks\" : [], \"ServerNow\" : 100\n}".to_vec(),
            ))
        }
    }
    let d = tempfile::tempdir().unwrap();
    let server = Arc::new(Session(AtomicBool::new(true)));
    let offline = Offline::with_clock(d.path(), server.clone(), Box::new(|| 200));
    let start = HttpRequest {
        url: "https://retroachievements.org/dorequest.php".into(),
        post: Some("r=startsession&u=Benni&g=100&t=tok".into()),
        content_type: None,
    };
    offline.send(&start).unwrap();
    server.0.store(false, Ordering::SeqCst);
    offline.send(&award(1)).unwrap();
    let (_, body) = offline.send(&start).unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["ServerNow"], 200);
    assert_eq!(data["Unlocks"][0]["ID"], 1);
    assert_eq!(data["Unlocks"].as_array().unwrap().len(), 1);
}

#[test]
fn cached_sessions_merge_unlock_modes_and_keep_hardcore_upgrades() {
    struct Session {
        online: AtomicBool,
        body: &'static str,
    }
    impl Http for Session {
        fn send(&self, _: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
            if !self.online.load(Ordering::SeqCst) {
                return Err("offline".into());
            }
            Ok((200, self.body.as_bytes().to_vec()))
        }
    }
    for body in [
        r#"{"Success":true,"Unlocks":[]}"#,
        r#"{"Success":true,"Unlocks":null,"HardcoreUnlocks":false}"#,
        r#"{"Success":true,"Unlocks":[{"ID":1,"When":100}],"HardcoreUnlocks":[{"ID":1,"When":100}]}"#,
    ] {
        let d = tempfile::tempdir().unwrap();
        let server = Arc::new(Session {
            online: AtomicBool::new(true),
            body,
        });
        let offline = Offline::with_clock(d.path(), server.clone(), Box::new(|| 200));
        let start = HttpRequest {
            url: "https://retroachievements.org/dorequest.php".into(),
            post: Some("r=startsession&u=Benni&g=100&t=tok&h=0".into()),
            content_type: None,
        };
        offline.send(&start).unwrap();
        offline.send(&award(1)).unwrap();
        let hardcore = |id| HttpRequest {
            post: Some(format!("r=awardachievement&u=Benni&t=tok&a={id}&h=1&v=x")),
            ..award(id)
        };
        offline.send(&hardcore(2)).unwrap();
        server.online.store(false, Ordering::SeqCst);
        offline.send(&award(1)).unwrap();
        offline.send(&hardcore(1)).unwrap();
        offline.send(&hardcore(1)).unwrap();
        offline.send(&award(1)).unwrap();
        offline.send(&award(2)).unwrap();
        offline.send(&award(3)).unwrap();
        let recorded = std::fs::read_to_string(d.path().join("unlocks/Benni.txt")).unwrap();
        assert_eq!(recorded, "1 200\n2 200 h\n1 200 h\n3 200\n");

        let (_, body) = offline.send(&start).unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["ServerNow"], 200);
        for (key, ids) in [("Unlocks", [1, 3]), ("HardcoreUnlocks", [1, 2])] {
            let unlocks = data[key].as_array().unwrap();
            assert_eq!(unlocks.len(), 2, "{data}");
            for id in ids {
                assert_eq!(
                    unlocks.iter().filter(|v| v["ID"] == id).count(),
                    1,
                    "{data}"
                );
            }
        }
    }
}
