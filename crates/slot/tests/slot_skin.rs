use std::path::Path;

use slot::slot_skin::{list, path, BUNDLED, OWN};

fn set(dir: &Path, name: &str) {
    let d = dir.join(name);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("slot_back.png"), b"").unwrap();
    std::fs::write(d.join("slot_front.png"), b"").unwrap();
}

#[test]
fn the_bundled_sets_are_listed_beside_yours_and_yours_win_a_name() {
    let d = tempfile::tempdir().unwrap();
    set(&d.path().join(OWN), "Mine");
    set(&d.path().join(OWN), "Teal");
    set(&d.path().join(BUNDLED), "teal");
    set(&d.path().join(BUNDLED), "Fork Orange");
    std::fs::create_dir_all(d.path().join(OWN).join("No Front")).unwrap();
    assert_eq!(list(d.path()), ["Fork Orange", "Mine", "Teal"]);
    assert_eq!(path(d.path(), "Teal"), d.path().join(OWN).join("Teal"));
    assert_eq!(
        path(d.path(), "Fork Orange"),
        d.path().join(BUNDLED).join("Fork Orange")
    );
}

#[test]
fn a_card_with_no_slots_lists_none() {
    let d = tempfile::tempdir().unwrap();
    assert!(list(d.path()).is_empty());
}
