//! Makes a release's update pack, `unslot-<version>.pak`, from the MinUI.zip tools/update/pack.sh
//! built: the zip untouched, beside a note of its version and checksum signed with the release
//! key. The key's secret half is UNSLOT_PAK_KEY, its 32-byte seed in hex; the pack is only made
//! if that key is the one unslot checks packs against.
//!
//!     UNSLOT_PAK_KEY=... cargo run -p slot --no-default-features --example make_pak -- \
//!         MinUI.zip v2.2.0 unslot-v2.2.0.pak

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [zip, version, out] = args.as_slice() else {
        eprintln!("usage: make_pak MinUI.zip VERSION OUT.pak, with UNSLOT_PAK_KEY set");
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
        eprintln!("make_pak: UNSLOT_PAK_KEY is not a 32-byte seed in hex");
        return ExitCode::FAILURE;
    };
    if slot::pak::build::public_key(seed) != slot::pak::RELEASE_KEY {
        eprintln!("make_pak: UNSLOT_PAK_KEY is not the key unslot checks packs against");
        return ExitCode::FAILURE;
    }
    let bytes = match std::fs::read(zip) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("make_pak: {zip}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let pak = slot::pak::build::signed(version, &bytes, seed);
    // What goes out is checked the way the device checks it.
    if let Err(e) = slot::pak::check(&pak, &slot::pak::RELEASE_KEY) {
        eprintln!("make_pak: the pack does not check out: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(out, pak) {
        eprintln!("make_pak: {out}: {e}");
        return ExitCode::FAILURE;
    }
    println!("{out}: unslot {version}");
    ExitCode::SUCCESS
}
