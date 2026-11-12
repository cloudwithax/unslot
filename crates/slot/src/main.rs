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
