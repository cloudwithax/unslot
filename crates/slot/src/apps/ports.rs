//! Ports: scripts in `Ports`, as PortMaster lays them out, run with slot gone. slot leaves the
//! script's path in `QUEUE` and quits; launch_frontend.sh, which init starts again, runs it from
//! its own folder and then starts slot once more.

use std::path::{Path, PathBuf};

use super::files::{list, Kind};

pub const DIR: &str = "Ports";
/// The port to run next, left at the card's root for launch_frontend.sh.
pub const QUEUE: &str = ".unslot-port";

/// The scripts in `Ports`, in name order.
pub fn scripts(root: &Path) -> Vec<PathBuf> {
    list(&root.join(DIR), Some(&[Kind::Script]))
        .into_iter()
        .filter(|e| e.kind == Kind::Script)
        .map(|e| e.path)
        .collect()
}

/// Leaves `script` for launch_frontend.sh to run once slot has quit.
pub fn queue(root: &Path, script: &Path) -> std::io::Result<()> {
    slot_store::atomic_write(&root.join(QUEUE), script.to_string_lossy().as_bytes())
}
