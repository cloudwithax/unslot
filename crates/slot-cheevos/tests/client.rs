use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slot_cheevos::{Cheevos, Console, Http, HttpRequest, Notice};

/// Answers like retroachievements.org would, by the request's `r=`.
#[derive(Default)]
struct Fake {
    seen: Mutex<Vec<String>>,
}

impl Http for Fake {
    fn send(&self, request: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
        let post = request.post.clone().unwrap_or_default();
        self.seen.lock().unwrap().push(post.clone());
        let r = post
            .split('&')
            .find_map(|kv| kv.strip_prefix("r="))
            .unwrap_or("");
        let body = match r {
            "login2" => {
                r#"{"Success":true,"User":"Benni","Token":"t0k3n","Score":10,"SoftcoreScore":2,"Messages":0,"Permissions":1,"AccountType":"Registered"}"#
            }
            _ => r#"{"Success":false,"Error":"Unknown game","Code":"not_found","Status":404}"#,
        };
        Ok((
            if r == "login2" { 200 } else { 404 },
            body.as_bytes().to_vec(),
        ))
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

#[test]
fn rcheevos_is_the_vendored_release() {
    assert_eq!(slot_cheevos::version(), "12.5");
}

#[test]
fn a_token_logs_in_and_a_rom_is_identified_by_its_hash() {
    let fake = Arc::new(Fake::default());
    let c = Cheevos::new(fake.clone()).unwrap();
    c.login("Benni", "t0k3n");
    assert_eq!(
        wait(&c, |n| matches!(n, Notice::LoggedIn(_))),
        Notice::LoggedIn(Ok(()))
    );
    assert!(
        fake.seen.lock().unwrap()[0].contains("u=Benni")
            && fake.seen.lock().unwrap()[0].contains("t=t0k3n")
    );

    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("game.gba");
    std::fs::write(&rom, (0..=255u8).cycle().take(1024).collect::<Vec<_>>()).unwrap();
    c.load_game(&rom, Console::GBA);
    assert!(matches!(
        wait(&c, |n| matches!(n, Notice::GameLoaded(_))),
        Notice::GameLoaded(Err(_))
    ));
    let seen = fake.seen.lock().unwrap();
    assert!(
        seen.iter()
            .any(|p| p.contains("b2ea9f7fcea831a4a63b213f41a8855b")),
        "{seen:?}"
    );
}

#[test]
fn a_nes_rom_is_hashed_without_its_ines_header() {
    let fake = Arc::new(Fake::default());
    let c = Cheevos::new(fake.clone()).unwrap();
    c.login("Benni", "t0k3n");
    wait(&c, |n| matches!(n, Notice::LoggedIn(_)));
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("game.nes");
    let mut bytes = b"NES\x1a\x01\x01\0\0\0\0\0\0\0\0\0\0".to_vec();
    bytes.extend((0..=255u8).cycle().take(1024));
    std::fs::write(&rom, bytes).unwrap();
    c.load_game(&rom, Console::NES);
    wait(&c, |n| matches!(n, Notice::GameLoaded(_)));
    let seen = fake.seen.lock().unwrap();
    // The same 1024 bytes as the GBA ROM above: the 16-byte header is not hashed.
    assert!(
        seen.iter()
            .any(|p| p.contains("b2ea9f7fcea831a4a63b213f41a8855b")),
        "{seen:?}"
    );
}

#[test]
fn core_memory_is_mapped_for_a_nes() {
    let mut c = Cheevos::new(Arc::new(Fake::default())).unwrap();
    let mut ram = vec![0u8; 0x800];
    let mapped = unsafe {
        c.set_memory(std::ptr::null(), 0, Console::NES, &mut |id| match id {
            2 => Some((ram.as_mut_ptr(), ram.len())),
            _ => None,
        })
    };
    assert!(mapped >= 0x800, "{mapped}");
}

#[test]
fn sega_8_bit_roms_are_hashed_whole_and_their_ram_mapped() {
    let fake = Arc::new(Fake::default());
    let c = Cheevos::new(fake.clone()).unwrap();
    c.login("Benni", "t0k3n");
    wait(&c, |n| matches!(n, Notice::LoggedIn(_)));
    let d = tempfile::tempdir().unwrap();
    for (name, console) in [("game.gg", Console::GG), ("game.sms", Console::SMS)] {
        let rom = d.path().join(name);
        std::fs::write(&rom, (0..=255u8).cycle().take(1024).collect::<Vec<_>>()).unwrap();
        c.load_game(&rom, console);
        wait(&c, |n| matches!(n, Notice::GameLoaded(_)));
    }
    let seen = fake.seen.lock().unwrap();
    // The same 1024 bytes as the GBA ROM above, hashed as they are (rcheevos asks for a
    // hash it has looked up once only).
    assert!(
        seen.iter()
            .any(|p| p.contains("b2ea9f7fcea831a4a63b213f41a8855b")),
        "{seen:?}"
    );
    drop(seen);
    let mut c = c;
    let mut ram = vec![0u8; 0x2000];
    for console in [Console::GG, Console::SMS] {
        let mapped = unsafe {
            c.set_memory(std::ptr::null(), 0, console, &mut |id| match id {
                2 => Some((ram.as_mut_ptr(), ram.len())),
                _ => None,
            })
        };
        assert!(mapped >= 0x2000, "{console:?}: {mapped}");
    }
}

#[test]
fn a_refused_token_says_so() {
    struct No;
    impl Http for No {
        fn send(&self, _: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
            Ok((401, br#"{"Success":false,"Error":"Invalid user/token combination.","Code":"invalid_credentials","Status":401}"#.to_vec()))
        }
    }
    let c = Cheevos::new(Arc::new(No)).unwrap();
    c.login("Benni", "old");
    match wait(&c, |n| matches!(n, Notice::LoggedIn(_))) {
        Notice::LoggedIn(Err(e)) => assert!(e.contains("Invalid"), "{e}"),
        n => panic!("{n:?}"),
    }
}

#[test]
fn core_memory_is_mapped_for_a_game_boy() {
    let c = Cheevos::new(Arc::new(Fake::default()));
    let mut c = c.unwrap();
    let mut wram = vec![0u8; 0x2000];
    let mut hram = vec![0u8; 0x80];
    let mapped = unsafe {
        c.set_memory(std::ptr::null(), 0, Console::GB, &mut |id| match id {
            2 => Some((wram.as_mut_ptr(), wram.len())),
            _ => None,
        })
    };
    assert!(mapped >= 0x2000, "{mapped}");
    let _ = &mut hram;
}

#[test]
fn dropping_with_a_request_in_flight_does_not_crash() {
    struct Slow;
    impl Http for Slow {
        fn send(&self, _: &HttpRequest) -> Result<(u16, Vec<u8>), String> {
            std::thread::sleep(Duration::from_millis(300));
            Err("offline".into())
        }
    }
    let c = Cheevos::new(Arc::new(Slow)).unwrap();
    c.login("Benni", "t");
    drop(c);
    std::thread::sleep(Duration::from_millis(600));
}

#[test]
fn mega_drive_and_32x_roms_are_hashed_whole_and_their_64k_of_ram_mapped() {
    let fake = Arc::new(Fake::default());
    let c = Cheevos::new(fake.clone()).unwrap();
    c.login("Benni", "t0k3n");
    wait(&c, |n| matches!(n, Notice::LoggedIn(_)));
    let d = tempfile::tempdir().unwrap();
    for (name, console) in [("game.md", Console::MD), ("game.32x", Console::S32X)] {
        let rom = d.path().join(name);
        std::fs::write(&rom, (0..=255u8).cycle().take(1024).collect::<Vec<_>>()).unwrap();
        c.load_game(&rom, console);
        wait(&c, |n| matches!(n, Notice::GameLoaded(_)));
    }
    let seen = fake.seen.lock().unwrap();
    assert!(
        seen.iter()
            .any(|p| p.contains("b2ea9f7fcea831a4a63b213f41a8855b")),
        "{seen:?}"
    );
    drop(seen);
    let mut c = c;
    let mut ram = vec![0u8; 0x10000];
    let mapped = unsafe {
        c.set_memory(std::ptr::null(), 0, Console::MD, &mut |id| match id {
            2 => Some((ram.as_mut_ptr(), ram.len())),
            _ => None,
        })
    };
    assert!(mapped >= 0x10000, "{mapped}");
}

#[test]
fn a_neo_geo_pocket_rom_is_hashed_whole_and_its_ram_mapped() {
    let fake = Arc::new(Fake::default());
    let c = Cheevos::new(fake.clone()).unwrap();
    c.login("Benni", "t0k3n");
    wait(&c, |n| matches!(n, Notice::LoggedIn(_)));
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("game.ngc");
    std::fs::write(&rom, (0..=255u8).cycle().take(1024).collect::<Vec<_>>()).unwrap();
    c.load_game(&rom, Console::NGP);
    wait(&c, |n| matches!(n, Notice::GameLoaded(_)));
    let seen = fake.seen.lock().unwrap();
    assert!(
        seen.iter()
            .any(|p| p.contains("b2ea9f7fcea831a4a63b213f41a8855b")),
        "{seen:?}"
    );
    drop(seen);
    let mut c = c;
    let mut ram = vec![0u8; 0x3000];
    let mapped = unsafe {
        c.set_memory(std::ptr::null(), 0, Console::NGP, &mut |id| match id {
            2 => Some((ram.as_mut_ptr(), ram.len())),
            _ => None,
        })
    };
    assert!(mapped >= 0x3000, "{mapped}");
}

#[test]
fn a_wonderswan_rom_is_hashed_whole_and_its_ram_mapped() {
    let fake = Arc::new(Fake::default());
    let c = Cheevos::new(fake.clone()).unwrap();
    c.login("Benni", "t0k3n");
    wait(&c, |n| matches!(n, Notice::LoggedIn(_)));
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("game.wsc");
    std::fs::write(&rom, (0..=255u8).cycle().take(1024).collect::<Vec<_>>()).unwrap();
    c.load_game(&rom, Console::WS);
    wait(&c, |n| matches!(n, Notice::GameLoaded(_)));
    let seen = fake.seen.lock().unwrap();
    assert!(
        seen.iter()
            .any(|p| p.contains("b2ea9f7fcea831a4a63b213f41a8855b")),
        "{seen:?}"
    );
    drop(seen);
    let mut c = c;
    let mut ram = vec![0u8; 0x10000];
    let mapped = unsafe {
        c.set_memory(std::ptr::null(), 0, Console::WS, &mut |id| match id {
            2 => Some((ram.as_mut_ptr(), ram.len())),
            _ => None,
        })
    };
    assert!(mapped >= 0x4000, "{mapped}");
}

#[test]
fn a_sega_cd_image_is_hashed_by_its_disc_header() {
    let fake = Arc::new(Fake::default());
    let c = Cheevos::new(fake.clone()).unwrap();
    c.login("Benni", "t0k3n");
    wait(&c, |n| matches!(n, Notice::LoggedIn(_)));
    let d = tempfile::tempdir().unwrap();
    // An .iso of 2048-byte sectors: the volume and ROM headers open sector 0.
    let mut iso = vec![0u8; 2048 * 32];
    iso[..16].copy_from_slice(b"SEGADISCSYSTEM  ");
    iso[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    let rom = d.path().join("game.iso");
    std::fs::write(&rom, iso).unwrap();
    c.load_game(&rom, Console::SCD);
    wait(&c, |n| matches!(n, Notice::GameLoaded(_)));
    let seen = fake.seen.lock().unwrap();
    assert!(
        seen.iter()
            .any(|p| p.contains("f449ada9ab4280081643362f4760286e")),
        "{seen:?}"
    );
}

/// The hash rcheevos asked the server about for `disc`.
fn disc_hash(disc: &std::path::Path, console: Console) -> String {
    let fake = Arc::new(Fake::default());
    let c = Cheevos::new(fake.clone()).unwrap();
    c.login("Benni", "t0k3n");
    wait(&c, |n| matches!(n, Notice::LoggedIn(_)));
    c.load_game(disc, console);
    wait(&c, |n| matches!(n, Notice::GameLoaded(_)));
    let seen = fake.seen.lock().unwrap();
    seen.iter()
        .flat_map(|p| p.split('&'))
        .find_map(|kv| kv.strip_prefix("m=").filter(|m| m.len() == 32))
        .unwrap_or_else(|| panic!("no hash asked for; saw {seen:?}"))
        .to_string()
}

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn a_playstation_chd_is_hashed_by_its_boot_program() {
    // The hash of the same disc as a .cue/.bin (tests/fixtures/make-discs.py): the name and
    // contents of the program SYSTEM.CNF boots.
    assert_eq!(
        disc_hash(&fixture("ps1.chd"), Console::PS),
        "7e63ae30c9290b9627764d81986c8075"
    );
}

#[test]
fn a_sega_cd_chd_is_hashed_by_its_disc_header() {
    // As the .iso above, and as the same disc as a .cue/.bin.
    assert_eq!(
        disc_hash(&fixture("segacd.chd"), Console::SCD),
        "f449ada9ab4280081643362f4760286e"
    );
}
