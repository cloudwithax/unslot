use std::io;
use std::process::Command;

/// This ELF again, retaining an explicitly invoked Linux loader. `current_exe` resolves
/// the loader's symlink, so its name can be `ld-2.33.so`, not just `ld-linux-*.so.*`.
pub(crate) fn command() -> io::Result<Command> {
    let exe = std::env::current_exe()?;
    let mut command = Command::new(&exe);
    if exe
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("ld-"))
    {
        command.arg(std::env::args_os().next().unwrap_or_default());
    }
    Ok(command)
}

#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
mod tests {
    use super::*;
    use std::path::Path;

    const STAGE: &str = "SLOT_SELF_EXEC_TEST_STAGE";
    const EXPECTED: &str = "SLOT_SELF_EXEC_TEST_EXE";
    const TEST: &str =
        "self_exec::tests::relaunches_the_elf_directly_and_through_a_versioned_loader";
    const ARGS: [&str; 3] = [TEST, "--exact", "--nocapture"];

    fn passed(output: std::process::Output, marker: &str) {
        assert!(
            output.status.success(),
            "status {}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(marker));
    }

    #[test]
    fn relaunches_the_elf_directly_and_through_a_versioned_loader() {
        let exe = std::env::current_exe().unwrap();
        match std::env::var(STAGE).as_deref() {
            Ok("probe") => {
                assert_eq!(exe, Path::new(&std::env::var_os(EXPECTED).unwrap()));
                println!("self-exec reached the ELF");
                return;
            }
            Ok("loader") => {
                assert_eq!(exe.file_name().unwrap(), "ld-2.33.so");
                passed(
                    command()
                        .unwrap()
                        .args(ARGS)
                        .env(STAGE, "probe")
                        .env(EXPECTED, &exe)
                        .output()
                        .unwrap(),
                    "self-exec reached the ELF",
                );
                println!("versioned loader relaunched the ELF");
                return;
            }
            _ => {}
        }
        passed(
            command()
                .unwrap()
                .args(ARGS)
                .env(STAGE, "probe")
                .env(EXPECTED, &exe)
                .output()
                .unwrap(),
            "self-exec reached the ELF",
        );

        // Copy the host's real interpreter, not a stub. Launching through a symlink to
        // this versioned filename reproduces TrimUI's /proc/self/exe and argv[0].
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
        let loader = maps
            .lines()
            .filter_map(|line| line.split_whitespace().nth(5))
            .map(Path::new)
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("ld-"))
            })
            .expect("glibc interpreter in process maps");
        let dir = tempfile::tempdir().unwrap();
        let versioned = dir.path().join("ld-2.33.so");
        std::fs::copy(loader, &versioned).unwrap();
        let alias = dir.path().join("ld-linux-test.so.1");
        std::os::unix::fs::symlink(&versioned, &alias).unwrap();
        passed(
            Command::new(alias)
                .arg(exe)
                .args(ARGS)
                .env(STAGE, "loader")
                .output()
                .unwrap(),
            "versioned loader relaunched the ELF",
        );
    }
}
