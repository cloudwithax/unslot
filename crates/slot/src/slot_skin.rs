use std::path::{Path, PathBuf};

use slot_store::Appearance;
use slot_ui::SlotSkin;

/// Slots of your own, on the card: a folder each, named as the Theme row shows it, holding
/// `slot_back.png` and `slot_front.png` and, if it likes, `link_port.png`. It is outside System,
/// so an update never touches it.
pub const OWN: &str = "Slots";
/// The sets a build ships beside the SP colours, which an update carries over (see h700.sh).
pub const BUNDLED: &str = "System/Slots";

const BACK: &str = "slot_back.png";
const FRONT: &str = "slot_front.png";
const PORT: &str = "link_port.png";

/// The names of the slots on the card that have both their strips, yours and the bundled ones,
/// sorted. A name in both is yours.
pub fn list(root: &Path) -> Vec<String> {
    let mut names = sets(&root.join(OWN));
    for name in sets(&root.join(BUNDLED)) {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    names
}

fn sets(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !slot_store::is_hidden(p))
        .filter(|p| p.join(BACK).is_file() && p.join(FRONT).is_file())
        .filter_map(|p| Some(p.file_name()?.to_str()?.to_owned()))
        .collect()
}

/// Where the set called `name` is: your folder's, else the bundled one.
pub fn path(root: &Path, name: &str) -> PathBuf {
    let own = root.join(OWN).join(name);
    match own.join(FRONT).is_file() {
        true => own,
        false => root.join(BUNDLED).join(name),
    }
}

/// The strips for a look, or None for the plain dark and light, which are always uploaded, and
/// for a set on the card that is gone or will not decode.
pub fn load(root: Option<&Path>, look: &Appearance) -> Option<SlotSkin> {
    match look {
        Appearance::Dark | Appearance::Light => None,
        Appearance::Sp(colour) => Some(slot_ui::sp_slot(*colour)),
        Appearance::Own(name) => {
            let dir = path(root?, name);
            let read = |file: &str| std::fs::read(dir.join(file)).ok();
            let skin = slot_ui::slot_skin(&read(BACK)?, &read(FRONT)?, read(PORT).as_deref());
            if skin.is_none() {
                eprintln!("slot: {}: a strip did not decode", dir.display());
            }
            skin
        }
    }
}
