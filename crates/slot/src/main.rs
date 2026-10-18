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
            let unpacked = slot::pak::open(std::path::Path::new(&pak)).and_then(|p| {
                std::fs::write(&out, &p.zip)
                    .map(|()| p.version)
                    .map_err(|e| e.to_string())
            });
            match unpacked {
                Ok(version) => println!("{version}"),
                Err(e) => {
                    eprintln!("{pak}: {e}");
                    std::process::exit(1);
                }
            }
            return;
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

    #[cfg(feature = "host")]
    host_app::run();

    #[cfg(not(feature = "host"))]
    device_app::run();
}
