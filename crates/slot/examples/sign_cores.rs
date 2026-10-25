//! Signs the list of cores in unslot's `cores` release, `cores.txt` as tools/cores/mirror.sh
//! wrote it, with the release key, beside it as `cores.txt.sig`. The key's secret half is
//! UNSLOT_PAK_KEY, its 32-byte seed in hex, as for make_pak.
//!
//!     UNSLOT_PAK_KEY=... cargo run -p slot --no-default-features --example sign_cores -- \
//!         cores.txt

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [list] = args.as_slice() else {
        eprintln!("usage: sign_cores cores.txt, with UNSLOT_PAK_KEY set");
        return ExitCode::from(2);
    };
    let seed = std::env::var("UNSLOT_PAK_KEY").ok().and_then(|hex| {
        let hex = hex.trim();
        let bytes: Option<Vec<u8>> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
            .collect();
        <[u8; 32]>::try_from(bytes?).ok()
    });
    let Some(seed) = seed else {
        eprintln!("sign_cores: UNSLOT_PAK_KEY is not a 32-byte seed in hex");
        return ExitCode::FAILURE;
    };
    if slot::pak::build::public_key(seed) != slot::pak::RELEASE_KEY {
        eprintln!("sign_cores: UNSLOT_PAK_KEY is not the key unslot checks against");
        return ExitCode::FAILURE;
    }
    let text = match std::fs::read_to_string(list) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("sign_cores: {list}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let sig = slot::pak::build::sign(text.as_bytes(), seed);
    // What goes out is read the way the device reads it, and names every core it should.
    let listed = match slot::core_downloads::parse_list(&text, &sig, &slot::pak::RELEASE_KEY) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("sign_cores: the list does not check out: {e}");
            return ExitCode::FAILURE;
        }
    };
    let lines = text
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .count();
    if listed.len() != lines {
        eprintln!(
            "sign_cores: {} of the list's {lines} cores are ones unslot downloads",
            listed.len()
        );
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(format!("{list}.sig"), sig) {
        eprintln!("sign_cores: {list}.sig: {e}");
        return ExitCode::FAILURE;
    }
    println!("{list}: {} cores", listed.len());
    ExitCode::SUCCESS
}
