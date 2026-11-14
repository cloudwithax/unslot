#[cfg(not(feature = "host"))]
mod device_app;
#[cfg(feature = "host")]
mod host_app;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--init-root") => {
            let Some(dir) = args.next() else {
                eprintln!("slot: --init-root needs a directory");
                std::process::exit(2);
            };
            slot::root::ensure(std::path::Path::new(&dir));
            return;
        }
        // For System/launch_frontend.sh: checks an update pack is unslot's and writes the zip
        // inside it to OUT, printing its version, or says why not and fails.
        Some("--unpack-pak") => {
            let (Some(pak), Some(out)) = (args.next(), args.next()) else {
                eprintln!("slot: --unpack-pak needs a pack and where to put its zip");
                std::process::exit(2);
            };
            let unpacked = slot::pak::unpack(
                std::path::Path::new(&pak),
                std::path::Path::new(&out),
                &slot::pak::RELEASE_KEY,
            );
            match unpacked {
                Ok(version) => println!("{version}"),
                Err(e) => {
                    eprintln!("{pak}: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        // For System/drastic/launch.sh: the volume keys and Menu, while DraStic runs in slot's
        // place.
        Some("--drastic") => {
            let root = std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into());
            slot::drastic::run(std::path::Path::new(&root));
            return;
        }
        // The in-game menu over a stopped DraStic, for --drastic.
        Some("--drastic-menu") => {
            let root = std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into());
            let frame = std::env::args_os().nth(2).unwrap_or_default();
            std::process::exit(slot::drastic::menu(
                std::path::Path::new(&root),
                std::path::Path::new(&frame),
            ));
        }
        // Owns a port process and its volume keys, restoring the mixer before slot restarts.
        Some("--port-volume") => {
            use std::os::unix::process::ExitStatusExt;
            let mut command = std::env::args_os().skip(2);
            let Some(program) = command.next() else {
                eprintln!("slot: --port-volume needs a command");
                std::process::exit(2);
            };
            let root = std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into());
            let status = slot::port_volume::run(
                std::path::Path::new(&root),
                std::process::Command::new(program).args(command),
            );
            match status {
                Ok(s) => {
                    std::process::exit(s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0)))
                }
                Err(e) => {
                    eprintln!("slot: port volume: {e}");
                    std::process::exit(1);
                }
            }
        }
        // Before a port runs, for launch_frontend.sh with the port's script, and from its script
        // through PortMaster's harbourmaster with the runtime it asks for: the PortMaster
        // runtimes it needs that the card has not got, offered for download. Fails while one
        // is still missing.
        Some(flag @ ("--port-runtimes" | "--port-runtime")) => {
            let root = std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into());
            let root = std::path::Path::new(&root);
            let Some(arg) = std::env::args_os().nth(2) else {
                eprintln!("slot: {flag} needs a port's script or a runtime");
                std::process::exit(2);
            };
            let (files, strict) = match flag {
                "--port-runtimes" => (
                    slot::port_runtime::wanted(
                        &root.join(slot::apps::ports::DIR),
                        std::path::Path::new(&arg),
                    ),
                    false,
                ),
                _ => (
                    vec![slot::port_runtime::file_name(&arg.to_string_lossy())],
                    true,
                ),
            };
            std::process::exit(slot::port_runtime::run(root, &files, strict));
        }
        #[cfg(feature = "device")]
        Some("--core-host") => {
            let args: Vec<String> = args.collect();
            if let Err(e) = slot::core::host(&args) {
                eprintln!("slot: core host: {e}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }
    slot::crash::install();
    // Opens the trace, if SLOT_FTRACE asks for one, before a core host is spawned to join it.
    slot_ftrace::enabled();

    #[cfg(feature = "host")]
    host_app::run();

    #[cfg(not(feature = "host"))]
    device_app::run();
}
