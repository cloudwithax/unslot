//! Ports: scripts in `Ports`, as PortMaster lays them out, run with slot gone. slot leaves the
//! script's path in `QUEUE` and quits; launch_frontend.sh, which init starts again, runs it from
//! its own folder and then starts slot once more.

use std::path::{Path, PathBuf};

use super::files::{list, Kind};

pub const DIR: &str = "Ports";
/// The port to run next, left at the card's root for launch_frontend.sh.
pub const QUEUE: &str = ".unslot-port";
/// The script that runs a DS game in DraStic, which slot quits for as it does for a port.
pub const DRASTIC: &str = "System/drastic/launch.sh";

/// The scripts in `Ports`, in name order.
pub fn scripts(root: &Path) -> Vec<PathBuf> {
    list(&root.join(DIR), Some(&[Kind::Script]))
        .into_iter()
        .filter(|e| e.kind == Kind::Script)
        .map(|e| e.path)
        .collect()
}

/// Leaves `script` for launch_frontend.sh to run once slot has quit, with `arg` on the line
/// after it to hand it.
pub fn queue(root: &Path, script: &Path, arg: Option<&Path>) -> std::io::Result<()> {
    let mut note = script.to_string_lossy().into_owned();
    if let Some(arg) = arg {
        note = format!("{note}\n{}", arg.display());
    }
    slot_store::atomic_write(&root.join(QUEUE), note.as_bytes())
}
