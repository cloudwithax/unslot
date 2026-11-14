#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("mixer"), "31\n").unwrap();
    // Model a slow mixer read, as on a busy device. All mixer changes stay in this fixture.
    std::fs::write(
        root.join("amixer"),
        "#!/bin/sh\ncase \"$*\" in\n*cget*) sleep 0.2; printf '  : values=%s\\n' \"$(cat \"$MIXER\")\";;\n*cset*) for value do :; done; printf '%s\\n' \"$value\" > \"$MIXER\";;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(root.join("amixer"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let state = slot_store::SlotState {
        volume: 0,
        volume_hp: 0,
        ..Default::default()
    };
    std::fs::create_dir(root.join("Config")).unwrap();
    slot_store::write_slot_state(root, &state).unwrap();
    dir
}

fn run(root: &Path, program: &str, args: &[&str]) -> ExitStatus {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slot"))
        .arg("--port-volume")
        .arg(program)
        .args(args)
        .env("SLOT_ROOT", root)
        .env("MIXER", root.join("mixer"))
        .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("volume supervisor outlived the port");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn immediate_port_exit_restores_mixer_before_supervisor_returns() {
    let dir = fixture();
    let status = run(
        dir.path(),
        "sh",
        &["-c", "test \"$(cat \"$MIXER\")\" = 0 || exit 90; exit 23"],
    );
    assert_eq!(status.code(), Some(23));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mixer")).unwrap(),
        "31\n"
    );
}

#[test]
fn port_that_cannot_start_still_restores_mixer() {
    let dir = fixture();
    let program = dir.path().join("missing-port");
    let status = run(dir.path(), program.to_str().unwrap(), &[]);
    assert_eq!(status.code(), Some(1));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mixer")).unwrap(),
        "31\n"
    );
}
