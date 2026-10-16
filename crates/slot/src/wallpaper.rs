use std::path::{Path, PathBuf};

/// Your own wallpapers, on the card.
pub const OWN: &str = "Wallpapers";
/// The ones unslot ships, which come and go with System and so with its updates.
pub const BUNDLED: &str = "System/Wallpapers";

/// Every wallpaper there is to pick, yours and the bundled ones, by name. A name in both is yours.
pub fn list(root: &Path) -> Vec<String> {
    let mut files = pictures(&root.join(OWN));
    for name in pictures(&root.join(BUNDLED)) {
        if !files.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
            files.push(name);
        }
    }
    files.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    files
}

/// Where the wallpaper called `name` is: your folder's, else the bundled one.
pub fn path(root: &Path, name: &str) -> PathBuf {
    let own = root.join(OWN).join(name);
    match own.exists() {
        true => own,
        false => root.join(BUNDLED).join(name),
    }
}

pub fn pick(root: &Path, seed: u64) -> Option<PathBuf> {
    let mut files = list(root);
    if files.is_empty() {
        return None;
    }
    files.sort();
    let mut rng = Rng(seed);
    let deck = root.join("Config").join("wallpaper.deck");
    let (mut shown, mut queued) = read_deck(&deck);
    shown.retain(|n| files.contains(n));
    queued.retain(|n| files.contains(n));
    for n in &files {
        if !shown.contains(n) && !queued.contains(n) {
            let at = rng.below(queued.len() + 1);
            queued.insert(at, n.clone());
        }
    }
    if queued.is_empty() {
        let last = shown.pop();
        queued = files;
        for i in (1..queued.len()).rev() {
            queued.swap(i, rng.below(i + 1));
        }
        if queued.len() > 1 && last.as_ref() == queued.first() {
            let end = queued.len() - 1;
            queued.swap(0, end);
        }
        shown.clear();
    }
    let next = queued.remove(0);
    shown.push(next.clone());
    if let Err(e) = slot_store::atomic_write(&deck, write_deck(&shown, &queued).as_bytes()) {
        eprintln!("slot: wallpaper.deck: {e}");
    }
    Some(path(root, &next))
}

fn pictures(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !slot_store::is_hidden(p))
        .filter(|p| {
            p.extension().is_some_and(|e| {
                ["png", "jpg", "jpeg"]
                    .iter()
                    .any(|x| e.eq_ignore_ascii_case(x))
            })
        })
        .filter_map(|p| Some(p.file_name()?.to_str()?.to_owned()))
        .collect()
}

fn read_deck(path: &Path) -> (Vec<String>, Vec<String>) {
    let Some(text) = std::fs::read(path)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    else {
        return (Vec::new(), Vec::new());
    };
    let mut lines = text.lines();
    let Some(shown) = lines.next().and_then(|l| l.parse::<usize>().ok()) else {
        return (Vec::new(), Vec::new());
    };
    let mut order: Vec<String> = lines.map(str::to_owned).collect();
    let queued = order.split_off(shown.min(order.len()));
    (order, queued)
}

fn write_deck(shown: &[String], queued: &[String]) -> String {
    let mut text = format!("{}\n", shown.len());
    for n in shown.iter().chain(queued) {
        text.push_str(n);
        text.push('\n');
    }
    text
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z % n as u64) as usize
    }
}
