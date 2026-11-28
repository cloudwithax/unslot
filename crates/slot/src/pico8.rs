//! The owner's own PICO-8, which slot drives as it does DraStic (crate::standalone). They copy the
//! Raspberry Pi build they bought into BIOS/PICO-8, zipped or not, and a game set to the PICO-8
//! core, or the Splore card on the shelf, runs through System/pico8/launch.sh. That runs
//! `slot --pico8`, which starts PICO-8 with slot's shim for its arch preloaded, the shim handing
//! slot each frame, its sound and the pad. Carts favourited in Splore go on the
//! PICO-8 shelf once it closes, and every cart's label is drawn from the cart itself.

use std::path::{Path, PathBuf};

use slot_input::Btn;
use slot_store::{core_for_platform, pico8_installed, Cart, Core, Platform, PICO8_DIR};

/// The shelf's folder in Games, Labels, Saves and Screenshots.
pub const PLATFORM_DIR: &str = "PICO8";
/// The binaries in the Raspberry Pi zip, the one that runs on the handheld's own 32-bit SDL
/// first. `pico8` has SDL built in and `pico8_64` wants a 64-bit SDL BaseOS has not got, so they
/// only run with libraries the owner puts in BIOS/PICO-8/lib.
pub const BINARIES: [(&str, Arch); 3] = [
    ("pico8_dyn", Arch::Armhf),
    ("pico8", Arch::Armhf),
    ("pico8_64", Arch::Aarch64),
];
/// Preloaded into the 32-bit PICO-8, beside launch.sh: it hands slot PICO-8's frames, sound and
/// pad, and BaseOS's SDL has no haptics, which SDL will not start for a program that asks for.
pub const SHIM: &str = "System/pico8/libunslot-pico8.so";
/// The same, built for `pico8_64`.
pub const SHIM_64: &str = "System/pico8/libunslot-pico8-64.so";
/// Where launch.sh keeps the `wget` Splore downloads with.
pub const WGET_DIR: &str = "System/pico8/bin";

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Arch {
    Armhf,
    Aarch64,
}

impl Arch {
    pub fn loader(self) -> &'static str {
        match self {
            Arch::Armhf => "/lib/ld-linux-armhf.so.3",
            Arch::Aarch64 => "/lib/ld-linux-aarch64.so.1",
        }
    }

    pub fn library_path(self) -> &'static [&'static str] {
        match self {
            Arch::Armhf => &["/usr/lib32", "/usr/lib/arm-linux-gnueabihf"],
            Arch::Aarch64 => &["/usr/lib", "/usr/lib/aarch64-linux-gnu"],
        }
    }
}

/// The Splore card: a real cart, so it can be favourited, labelled and deleted as any other.
/// Playing it opens Splore instead.
pub const SPLORE_CART: &[u8] = include_bytes!("../assets/Splore.p8");
pub const SPLORE_FILE: &str = "Splore.p8";
/// Left once the Splore card has first gone on the shelf, so one deleted stays deleted.
const SPLORE_STOCKED: &str = "Config/pico8_splore_stocked";

pub fn is_splore(rom: &Path) -> bool {
    rom.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(SPLORE_FILE))
}

pub fn install_dir(root: &Path) -> PathBuf {
    root.join(PICO8_DIR)
}

pub fn carts_dir(root: &Path) -> PathBuf {
    root.join("Games").join(PLATFORM_DIR)
}

/// PICO-8's home: config.txt, the carts' saved data, Splore's downloads and favourites. It sits
/// with the other saves, so it is backed up with them, and fake-08 keeps its data here too.
pub fn home_dir(root: &Path) -> PathBuf {
    crate::root::saves_dir(root).join(PLATFORM_DIR)
}

pub fn screenshots_dir(root: &Path) -> PathBuf {
    root.join("Screenshots").join(PLATFORM_DIR)
}

/// Whether `cart` is left for the owner's PICO-8: Splore always is, any other cart when the
/// PICO-8 core is the one picked for it, while there is a PICO-8 on the card.
pub fn runs_native(root: &Path, cart: &Cart) -> bool {
    cart.platform == Platform::Pico8
        && pico8_installed(root)
        && (is_splore(&cart.rom)
            || core_for_platform(root, &cart.stem, cart.platform) == Core::Pico8)
}

/// At every boot: the Splore card on the shelf the first time there is a PICO-8, Splore's
/// favourites it has already downloaded, and labels for the carts that have none.
pub fn stock_shelf(root: &Path) {
    let stocked = root.join(SPLORE_STOCKED);
    if !stocked.exists() && pico8_installed(root) {
        let cart = carts_dir(root).join(SPLORE_FILE);
        let _ = std::fs::create_dir_all(carts_dir(root));
        match cart.exists() || slot_store::atomic_write(&cart, SPLORE_CART).is_ok() {
            true => {
                let _ = std::fs::create_dir_all(root.join("Config"));
                let _ = std::fs::write(&stocked, b"");
            }
            false => eprintln!("slot: pico8: could not write {}", cart.display()),
        }
    }
    shelve_favourites(root, |_| Fetched::Offline);
    write_labels(root);
}

/// The binary to run and what it was built for, in `dir` or the zip's own `pico-8` folder in it.
pub fn find_binary(dir: &Path) -> Option<(PathBuf, Arch)> {
    let dirs = [dir.to_path_buf(), dir.join("pico-8")];
    BINARIES.iter().find_map(|(name, arch)| {
        dirs.iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file() && p.with_file_name("pico8.dat").is_file())
            .map(|p| (p, *arch))
    })
}

/// The binary, unpacking the Raspberry Pi zip into BIOS/PICO-8 first when there is none.
pub fn unpack(root: &Path) -> Result<Option<(PathBuf, Arch)>, String> {
    let dir = install_dir(root);
    if let Some(found) = find_binary(&dir) {
        return Ok(Some(found));
    }
    let Some(zip) = slot_store::pico8_zip(root) else {
        return Ok(None);
    };
    eprintln!("slot: pico8: unpacking {}", zip.display());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    crate::unzip::extract(&zip, &dir, "pico-8/")?;
    Ok(find_binary(&dir))
}

/// The arguments PICO-8 is started with: its home, the shelf to browse, where screenshots go,
/// full screen, the volume, and the cart or Splore. Through the loader when the card will not
/// run the binary itself.
pub fn command_line(
    root: &Path,
    binary: &Path,
    arch: Arch,
    rom: &Path,
    direct: bool,
) -> Vec<String> {
    let mut args = Vec::new();
    if !direct {
        args.push(arch.loader().to_string());
    }
    args.extend([
        binary.display().to_string(),
        "-home".into(),
        home_dir(root).display().to_string(),
        "-root_path".into(),
        carts_dir(root).display().to_string(),
        "-desktop".into(),
        screenshots_dir(root).display().to_string(),
        "-windowed".into(),
        "0".into(),
        // At full, slot scaling its sound as it does a core's.
        "-volume".into(),
        "256".into(),
    ]);
    match is_splore(rom) {
        true => args.push("-splore".into()),
        false => args.extend(["-run".into(), rom.display().to_string()]),
    }
    args
}

/// Makes `path` executable when the filesystem allows it: on an ext4 card a binary copied over
/// Samba or unzipped without Unix modes lacks `+x`, which would otherwise force PICO-8 through
/// the dynamic loader where it fails to find `pico8.dat`.
pub fn ensure_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let mode = meta.permissions().mode();
    if meta.is_file() && mode & 0o111 == 0 {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o755));
    }
}

/// Whether the card lets `binary` run as a program of its own. Through the loader PICO-8 looks
/// for pico8.dat beside the loader, so it is only run that way when it has to be.
pub fn runs_directly(binary: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(binary).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Where PICO-8 looks for libraries, the owner's own first.
pub fn library_dirs(install: &Path, arch: Arch) -> Vec<PathBuf> {
    std::iter::once(install.join("lib"))
        .chain(arch.library_path().iter().map(PathBuf::from))
        .collect()
}

/// The GLES and EGL libraries SDL should load. Left to itself the 32-bit SDL asks for
/// libGLESv2.so, which no folder has, and PICO-8 opens no window. Each is the first of `dirs`
/// that has it, so libraries the owner adds win.
pub fn gl_drivers(dirs: &[PathBuf]) -> Vec<(&'static str, PathBuf)> {
    [
        ("SDL_VIDEO_GL_DRIVER", "libGLESv2.so.2"),
        ("SDL_VIDEO_EGL_DRIVER", "libEGL.so.1"),
    ]
    .into_iter()
    .filter_map(|(var, name)| {
        dirs.iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file())
            .map(|p| (var, p))
    })
    .collect()
}

/// `PATH` with `dir` first.
pub fn path_with(dir: &Path, path: Option<&std::ffi::OsStr>) -> std::ffi::OsString {
    let mut out = std::ffi::OsString::from(dir);
    out.push(":");
    out.push(path.unwrap_or(std::ffi::OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin")));
    out
}

/// `slot --pico8 ROM`, for System/pico8/launch.sh: plays ROM, or Splore, on the owner's PICO-8,
/// which slot drives through the shim (crate::standalone), until it closes.
pub fn run(root: &Path, rom: &Path) -> i32 {
    let (binary, arch) = match unpack(root) {
        Ok(Some(found)) => found,
        Ok(None) => {
            eprintln!(
                "slot: pico8: no PICO-8 in {}; copy the Raspberry Pi zip there",
                install_dir(root).display()
            );
            return 1;
        }
        Err(e) => {
            eprintln!("slot: pico8: unpack: {e}");
            return 1;
        }
    };
    for dir in [home_dir(root), carts_dir(root), screenshots_dir(root)] {
        let _ = std::fs::create_dir_all(dir);
    }
    let shim = root.join(match arch {
        Arch::Armhf => SHIM,
        Arch::Aarch64 => SHIM_64,
    });
    if !shim.is_file() {
        eprintln!("slot: pico8: {} is not on the card", shim.display());
        return 1;
    }
    ensure_executable(&binary);
    ensure_executable(&root.join(WGET_DIR).join("wget"));
    let line = command_line(root, &binary, arch, rom, runs_directly(&binary));
    eprintln!("slot: pico8: {}", line.join(" "));
    let install = binary.parent().unwrap_or(root).to_path_buf();
    let dirs = library_dirs(&install, arch);
    let lib_path = dirs
        .iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(":");
    let mut command = std::process::Command::new(&line[0]);
    command
        .args(&line[1..])
        .current_dir(&install)
        .env("LD_LIBRARY_PATH", lib_path)
        .env("HOME", home_dir(root))
        .env(
            "PATH",
            path_with(&root.join(WGET_DIR), std::env::var_os("PATH").as_deref()),
        );
    for (var, path) in gl_drivers(&dirs) {
        command.env(var, path);
    }
    let pads = controllers(Path::new(DEV), Path::new(SYS));
    if !pads.is_empty() {
        let mappings: Vec<&str> = pads.iter().map(|(_, m)| m.as_str()).collect();
        let nodes: Vec<String> = pads.iter().map(|(n, _)| n.display().to_string()).collect();
        command
            .env("SDL_GAMECONTROLLERCONFIG", mappings.join("\n"))
            .env("SDL_JOYSTICK_DEVICE", nodes.join(":"));
    }
    command.env("LD_PRELOAD", &shim);
    // For single carts (non-Splore), replace this launcher process directly with PICO-8 so there
    // is no extra parent waiting on exit. Splore needs to run `shelve_favourites` after exit.
    #[cfg(unix)]
    if !is_splore(rom) {
        use std::os::unix::process::CommandExt;
        let err = command.exec();
        eprintln!("slot: pico8: {}: {err}", line[0]);
        return 1;
    }
    // PICO-8 closes when slot asks the shim to, and slot kills it with this if it will not.
    match command.spawn() {
        Ok(mut child) => {
            if let Ok(status) = child.wait() {
                if !status.success() {
                    eprintln!("slot: pico8: exited with {status}");
                }
            }
        }
        Err(e) => {
            eprintln!("slot: pico8: {}: {e}", line[0]);
            return 1;
        }
    }
    // The carts favourited in Splore go on the shelf while the network is still up for the ones
    // Splore never downloaded; slot finds them when it starts again.
    if is_splore(rom) {
        let added = shelve_favourites(root, fetch_bbs);
        if added > 0 {
            eprintln!("slot: pico8: {added} favourite(s) put on the shelf");
        }
    }
    0
}

/// Where PICO-8 lists the carts favourited in Splore, in its home.
const FAVOURITES: &str = "favourites.txt";
/// The favourites slot has put on the shelf, one id a line, so each goes on once and one taken
/// off the shelf stays off.
const SHELVED: &str = "Config/pico8_favourites.txt";

/// A cart favourited in Splore.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Favourite {
    /// The BBS cart id with its revision, as in `celeste_classic-2` or `15133`.
    pub id: String,
    pub title: String,
}

/// The name a cart file is listed under: `celeste.p8.png` is `celeste`.
pub fn cart_stem(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let lower = name.to_ascii_lowercase();
    let cut = [".p8.png", ".p8"]
        .iter()
        .find(|ext| lower.ends_with(*ext))
        .map(|ext| name.len() - ext.len())?;
    Some(name[..cut].to_string())
}

/// Reads PICO-8's favourites.txt: a cart a line, fields between `|`, the cart's id second and its
/// title seventh. A cart PICO-8 took for a local one has no id, only its path in bbs, which names
/// it as well.
pub fn parse_favourites(text: &str) -> Vec<Favourite> {
    let mut out: Vec<Favourite> = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if fields.len() < 7 {
            continue;
        }
        let id = match fields[1] {
            "" => Path::new(fields[5])
                .starts_with("bbs")
                .then(|| cart_stem(Path::new(fields[5])))
                .flatten()
                .unwrap_or_default(),
            id => id.to_string(),
        };
        if !is_bbs_id(&id) || out.iter().any(|f| f.id == id) {
            continue;
        }
        let title = match fields[6] {
            "" => id.clone(),
            t => t.to_string(),
        };
        out.push(Favourite { id, title });
    }
    out
}

/// BBS ids are letters, digits, `_` and `-`: nothing that reaches out of a folder or a URL path.
fn is_bbs_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Splore's own copy of a cart it has run: bbs/carts/<id>.p8.png, or, for carts numbered, in a
/// numbered folder of bbs.
pub fn bbs_cart(home: &Path, id: &str) -> Option<PathBuf> {
    let name = format!("{id}.p8.png");
    let bbs = home.join("bbs");
    let mut dirs = vec![bbs.join("carts")];
    if let Ok(entries) = std::fs::read_dir(&bbs) {
        let mut more: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir() && !p.ends_with("carts"))
            .collect();
        more.sort();
        dirs.extend(more);
    }
    dirs.into_iter()
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
}

/// Where the BBS keeps a cart: under the first character of a numbered id, the first two of any
/// other.
pub fn bbs_url(id: &str) -> Option<String> {
    if !is_bbs_id(id) || id.len() < 2 {
        return None;
    }
    let numbered = id.as_bytes()[..2].iter().all(u8::is_ascii_digit);
    let folder = &id[..if numbered { 1 } else { 2 }];
    Some(format!(
        "https://www.lexaloffle.com/bbs/cposts/{folder}/{id}.p8.png"
    ))
}

/// What became of asking the BBS for a cart.
pub enum Fetched {
    Cart(Vec<u8>),
    /// The BBS answered, without the cart.
    Missing,
    /// No answer: asking for more now would only wait as long again.
    Offline,
}

/// The name a favourite gets on the shelf: its title with its words capitalised, as Splore shows
/// it, without what a FAT card cannot have in a file's name.
pub fn shelf_name(title: &str, id: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for c in title.chars() {
        if c.is_control() || "<>:\"/\\|?*".contains(c) {
            continue;
        }
        match start {
            true => out.extend(c.to_uppercase()),
            false => out.push(c),
        }
        start = c.is_whitespace();
    }
    let words: Vec<&str> = out.split_whitespace().collect();
    let mut name: String = words.join(" ");
    if name.chars().count() > 60 {
        name = name
            .chars()
            .take(60)
            .collect::<String>()
            .trim_end()
            .to_string();
    }
    let name = name.trim_end_matches(['.', ' ']).to_string();
    match name.is_empty() || name.starts_with('.') || is_splore(Path::new(&format!("{name}.p8"))) {
        true => id.to_string(),
        false => name,
    }
}

/// Puts every cart favourited in Splore on the PICO-8 shelf, once each, as a .p8.png under its
/// title: Splore's own copy when it has one, or what `fetch` gets from the BBS. Answers how many
/// went on; their labels are drawn at the next boot.
pub fn shelve_favourites(root: &Path, mut fetch: impl FnMut(&str) -> Fetched) -> usize {
    let home = home_dir(root);
    let Ok(text) = std::fs::read_to_string(home.join(FAVOURITES)) else {
        return 0;
    };
    let record = root.join(SHELVED);
    let mut shelved: Vec<String> = std::fs::read_to_string(&record)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let mut offline = false;
    let mut added = 0;
    for fav in parse_favourites(&text) {
        if shelved.contains(&fav.id) {
            continue;
        }
        let bytes = match bbs_cart(&home, &fav.id).and_then(|p| std::fs::read(p).ok()) {
            Some(bytes) => bytes,
            None if offline => continue,
            None => match fetch(&fav.id) {
                Fetched::Cart(bytes) => bytes,
                Fetched::Missing => {
                    eprintln!("slot: pico8: {} is not on the BBS", fav.id);
                    continue;
                }
                Fetched::Offline => {
                    offline = true;
                    continue;
                }
            },
        };
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            eprintln!("slot: pico8: {} is not a cart", fav.id);
            continue;
        }
        let dir = carts_dir(root);
        let name = shelf_name(&fav.title, &fav.id);
        let cart = [name.clone(), format!("{name} ({})", fav.id)]
            .into_iter()
            .map(|n| dir.join(format!("{n}.p8.png")))
            .find(|p| !p.exists() || std::fs::read(p).is_ok_and(|b| b == bytes));
        let Some(cart) = cart else {
            eprintln!("slot: pico8: no free name for {}", fav.id);
            continue;
        };
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = slot_store::atomic_write(&cart, &bytes) {
            eprintln!("slot: pico8: {}: {e}", cart.display());
            continue;
        }
        eprintln!(
            "slot: pico8: favourite {} on the shelf as {}",
            fav.id,
            cart.display()
        );
        shelved.push(fav.id);
        added += 1;
        let _ = std::fs::create_dir_all(root.join("Config"));
        let text = shelved.join("\n") + "\n";
        if let Err(e) = slot_store::atomic_write(&record, text.as_bytes()) {
            eprintln!("slot: pico8: {}: {e}", record.display());
        }
    }
    added
}

/// A .p8.png is 160 by 205 pixels; nothing honest comes near this.
const BBS_CART_LIMIT: u64 = 1 << 20;

/// Asks the BBS for a cart, through BaseOS's curl.
pub fn fetch_bbs(id: &str) -> Fetched {
    let Some(url) = bbs_url(id) else {
        return Fetched::Missing;
    };
    let tmp = std::env::temp_dir().join(format!("slot-pico8-{}", std::process::id()));
    let out = std::process::Command::new("curl")
        .args(["-sSfL", "--connect-timeout", "10", "--max-time", "15"])
        .arg("--max-filesize")
        .arg(BBS_CART_LIMIT.to_string())
        .arg("-o")
        .arg(&tmp)
        .arg(&url)
        .stdin(std::process::Stdio::null())
        .status();
    let bytes = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    match out {
        Ok(s) if s.success() => bytes.map_or(Fetched::Offline, Fetched::Cart),
        // curl's -f: the server answered with an error.
        Ok(s) if s.code() == Some(22) => {
            eprintln!("slot: pico8: {url}: not found");
            Fetched::Missing
        }
        Ok(s) => {
            eprintln!("slot: pico8: {url}: curl {s}");
            Fetched::Offline
        }
        Err(e) => {
            eprintln!("slot: pico8: curl: {e}");
            Fetched::Offline
        }
    }
}

pub const LABEL: u32 = 128;

const PALETTE: [[u8; 3]; 32] = [
    [0x00, 0x00, 0x00],
    [0x1d, 0x2b, 0x53],
    [0x7e, 0x25, 0x53],
    [0x00, 0x87, 0x51],
    [0xab, 0x52, 0x36],
    [0x5f, 0x57, 0x4f],
    [0xc2, 0xc3, 0xc7],
    [0xff, 0xf1, 0xe8],
    [0xff, 0x00, 0x4d],
    [0xff, 0xa3, 0x00],
    [0xff, 0xec, 0x27],
    [0x00, 0xe4, 0x36],
    [0x29, 0xad, 0xff],
    [0x83, 0x76, 0x9c],
    [0xff, 0x77, 0xa8],
    [0xff, 0xcc, 0xaa],
    [0x29, 0x18, 0x14],
    [0x11, 0x1d, 0x35],
    [0x42, 0x21, 0x36],
    [0x12, 0x53, 0x59],
    [0x74, 0x2f, 0x29],
    [0x49, 0x33, 0x3b],
    [0xa2, 0x88, 0x79],
    [0xf3, 0xef, 0x7d],
    [0xbe, 0x12, 0x50],
    [0xff, 0x6c, 0x24],
    [0xa8, 0xe7, 0x2e],
    [0x00, 0xb5, 0x43],
    [0x06, 0x5a, 0xb5],
    [0x75, 0x46, 0x65],
    [0xff, 0x6e, 0x59],
    [0xff, 0x9d, 0x81],
];

/// A cart's 128 by 128 label as RGBA: the `__label__` of a .p8, or the label window of a
/// .p8.png's picture.
pub fn label(path: &Path) -> Option<Vec<u8>> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let bytes = std::fs::read(path).ok()?;
    match name.ends_with(".p8.png") {
        true => label_from_png(&bytes),
        false => label_from_p8(&String::from_utf8_lossy(&bytes)),
    }
}

pub fn label_from_p8(text: &str) -> Option<Vec<u8>> {
    let mut lines = text.lines().skip_while(|l| l.trim() != "__label__").skip(1);
    let mut out = Vec::with_capacity((LABEL * LABEL * 4) as usize);
    for _ in 0..LABEL {
        let line = lines.next()?.trim();
        if line.starts_with("__") {
            return None;
        }
        let mut row = line.chars();
        for _ in 0..LABEL {
            let index = row.next().and_then(|c| c.to_digit(32)).unwrap_or(0) as usize;
            let [r, g, b] = PALETTE[index.min(PALETTE.len() - 1)];
            out.extend_from_slice(&[r, g, b, 0xff]);
        }
    }
    // An all-black label is what PICO-8 writes before one is captured.
    out.chunks_exact(4)
        .any(|px| px[..3] != [0, 0, 0])
        .then_some(out)
}

/// The label window in the 160 by 205 cart picture PICO-8 saves.
const PNG_LABEL_AT: (u32, u32) = (16, 24);
const PNG_SIZE: (u32, u32) = (160, 205);

pub fn label_from_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if (info.width, info.height) != PNG_SIZE {
        return None;
    }
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        _ => return None,
    };
    let (x0, y0) = PNG_LABEL_AT;
    let mut out = Vec::with_capacity((LABEL * LABEL * 4) as usize);
    for y in y0..y0 + LABEL {
        for x in x0..x0 + LABEL {
            let at = (y as usize * info.line_size) + x as usize * channels;
            let px = buf.get(at..at + 3)?;
            out.extend_from_slice(&[px[0], px[1], px[2], 0xff]);
        }
    }
    Some(out)
}

/// Writes a label for every cart in Games/PICO8 that has none in Labels/PICO8, named as the
/// shelf looks it up, so the shelf shows the art the cart carries.
pub fn write_labels(root: &Path) {
    let labels = root.join("Labels").join(PLATFORM_DIR);
    let Ok(entries) = std::fs::read_dir(carts_dir(root)) else {
        return;
    };
    for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        if cart_stem(&path).is_none() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let out = labels.join(format!("{stem}.png"));
        if out.exists() {
            continue;
        }
        let Some(rgba) = label(&path) else {
            continue;
        };
        let _ = std::fs::create_dir_all(&labels);
        match encode_png(&rgba, LABEL, LABEL) {
            Some(png) => {
                if let Err(e) = slot_store::atomic_write(&out, &png) {
                    eprintln!("slot: pico8: {}: {e}", out.display());
                }
            }
            None => eprintln!("slot: pico8: could not encode a label for {stem}"),
        }
    }
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

const DEV: &str = "/dev/input";
const SYS: &str = "/sys/class/input";

/// What SDL learns of a pad from sysfs, enough to name it in a `SDL_GAMECONTROLLERCONFIG` line.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Pad {
    pub name: String,
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
    /// The key codes it reports, lowest first.
    pub keys: Vec<u16>,
    /// The absolute axes it reports, lowest first.
    pub axes: Vec<u16>,
}

const BTN_JOYSTICK: u16 = 0x120;
const KEY_MAX: u16 = 0x2ff;
const ABS_MAX: u16 = 0x3f;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT3Y: u16 = 0x17;

impl Pad {
    pub fn read(sys_device: &Path) -> Option<Pad> {
        let text = |p: &str| {
            std::fs::read_to_string(sys_device.join(p))
                .ok()
                .map(|s| s.trim().to_string())
        };
        let hex = |p: &str| text(p).and_then(|s| u16::from_str_radix(&s, 16).ok());
        let bits = |p: &str, max: u16| {
            let mask = text(p).unwrap_or_default();
            (0..=max)
                .filter(|b| slot_power::has_bit(&mask, *b))
                .collect::<Vec<u16>>()
        };
        Some(Pad {
            name: text("name")?,
            bus: hex("id/bustype").unwrap_or(0),
            vendor: hex("id/vendor").unwrap_or(0),
            product: hex("id/product").unwrap_or(0),
            version: hex("id/version").unwrap_or(0),
            keys: bits("capabilities/key", KEY_MAX),
            axes: bits("capabilities/abs", ABS_MAX),
        })
    }

    /// SDL's joystick GUID for the pad, as SDL before 2.26 makes it. Later SDL puts a checksum of
    /// the name in bytes 2 and 3 but still takes a mapping that leaves them zero.
    pub fn guid(&self) -> String {
        let mut g = [0u8; 16];
        g[0..2].copy_from_slice(&self.bus.to_le_bytes());
        if self.vendor != 0 && self.product != 0 {
            g[4..6].copy_from_slice(&self.vendor.to_le_bytes());
            g[8..10].copy_from_slice(&self.product.to_le_bytes());
            g[12..14].copy_from_slice(&self.version.to_le_bytes());
        } else {
            let name = self.name.as_bytes();
            let n = name.len().min(12);
            g[4..4 + n].copy_from_slice(&name[..n]);
        }
        g.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// SDL numbers buttons from BTN_JOYSTICK up, then the codes below it.
    fn button(&self, code: u16) -> Option<usize> {
        let order = self
            .keys
            .iter()
            .filter(|&&k| k >= BTN_JOYSTICK)
            .chain(self.keys.iter().filter(|&&k| k < BTN_JOYSTICK));
        order.enumerate().find(|(_, &k)| k == code).map(|(i, _)| i)
    }

    /// SDL numbers every absolute axis but the hats in code order.
    fn axis(&self, code: u16) -> Option<usize> {
        self.axes
            .iter()
            .filter(|&&a| !(ABS_HAT0X..=ABS_HAT3Y).contains(&a))
            .position(|&a| a == code)
    }

    fn has_hat(&self) -> bool {
        self.axes.contains(&ABS_HAT0X) && self.axes.contains(&(ABS_HAT0X + 1))
    }

    /// The pad's `SDL_GAMECONTROLLERCONFIG` line, its buttons read as `btn` reads them. SDL names
    /// them by where they sit: its `a` is the bottom face button, which PICO-8 takes for O, and
    /// `b` the right one, PICO-8's X.
    pub fn mapping(&self, btn: fn(u16) -> Option<Btn>, stick: Option<[u16; 4]>) -> Option<String> {
        let code_of = |b: Btn| self.keys.iter().copied().find(|&code| btn(code) == Some(b));
        let mut fields = vec![self.guid(), self.name.replace(',', " ")];
        let buttons = [
            ("a", Btn::B),
            ("b", Btn::A),
            ("x", Btn::Y),
            ("y", Btn::X),
            ("back", Btn::Select),
            ("start", Btn::Start),
            ("leftshoulder", Btn::L1),
            ("rightshoulder", Btn::R1),
            ("lefttrigger", Btn::L2),
            ("righttrigger", Btn::R2),
        ];
        let mut mapped = 0;
        for (field, b) in buttons {
            if let Some(index) = code_of(b).and_then(|code| self.button(code)) {
                fields.push(format!("{field}:b{index}"));
                mapped += 1;
            }
        }
        if mapped < 2 {
            return None;
        }
        if self.has_hat() {
            fields.extend(
                ["dpup:h0.1", "dpright:h0.2", "dpdown:h0.4", "dpleft:h0.8"].map(String::from),
            );
        } else {
            for (field, b) in [
                ("dpup", Btn::Up),
                ("dpdown", Btn::Down),
                ("dpleft", Btn::Left),
                ("dpright", Btn::Right),
            ] {
                if let Some(index) = code_of(b).and_then(|code| self.button(code)) {
                    fields.push(format!("{field}:b{index}"));
                }
            }
        }
        if let Some([x, y, _, _]) = stick {
            if let (Some(x), Some(y)) = (self.axis(x), self.axis(y)) {
                fields.push(format!("leftx:a{x}"));
                fields.push(format!("lefty:a{y}"));
            }
        }
        fields.push("platform:Linux".into());
        Some(fields.join(",") + ",")
    }
}

/// Every pad slot reads, with its device node and SDL mapping line. BaseOS runs no udev, so
/// nothing tags a node as a joystick for SDL to find: SDL_JOYSTICK_DEVICE names them instead.
pub fn controllers(dev: &Path, sys: &Path) -> Vec<(PathBuf, String)> {
    use crate::input::evdev;
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for node in evdev::pick_devices(dev, sys) {
        let Some(name) = node.file_name() else {
            continue;
        };
        let device = sys.join(name).join("device");
        let Some(pad) = Pad::read(&device) else {
            continue;
        };
        let btn = match () {
            _ if evdev::is_trimui_pad(sys, &node) => evdev::trimui_code_to_btn,
            _ if evdev::is_pad(sys, &node) => evdev::pad_code_to_btn,
            _ => evdev::code_to_btn,
        };
        let abs = std::fs::read_to_string(device.join("capabilities/abs")).unwrap_or_default();
        let Some(line) = pad.mapping(btn, crate::input::stick::layout(&abs)) else {
            continue;
        };
        if !out.iter().any(|(_, l)| *l == line) {
            out.push((node, line));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_cart_is_listed_without_both_extensions() {
        assert_eq!(
            cart_stem(Path::new("Games/PICO8/celeste.p8.png")).as_deref(),
            Some("celeste")
        );
        assert_eq!(
            cart_stem(Path::new("Games/PICO8/Jelpi.P8")).as_deref(),
            Some("Jelpi")
        );
        assert_eq!(cart_stem(Path::new("Games/PICO8/notes.png")), None);
    }

    #[test]
    fn splore_is_the_card_by_its_name() {
        assert!(is_splore(Path::new("/mnt/sdcard/Games/PICO8/Splore.p8")));
        assert!(is_splore(Path::new("splore.P8")));
        assert!(!is_splore(Path::new("Splore.p8.png")));
    }

    #[test]
    fn favourites_are_read_by_id_and_title() {
        let text = "\
|celeste_classic-2| 0 | 0 | 0 | bbs/carts/celeste_classic-2.p8.png | celeste classic |
|| 0 | 0 | 0 | bbs/4/42.p8.png | the answer |
|celeste_classic-2| 0 | 0 | 0 | | again |
|../etc| 0 | 0 | 0 | | climbs |
too few|fields
";
        assert_eq!(
            parse_favourites(text),
            vec![
                Favourite {
                    id: "celeste_classic-2".into(),
                    title: "celeste classic".into()
                },
                Favourite {
                    id: "42".into(),
                    title: "the answer".into()
                },
            ]
        );
    }

    #[test]
    fn the_bbs_files_carts_by_their_ids_first_letters() {
        assert_eq!(
            bbs_url("celeste_classic-2").as_deref(),
            Some("https://www.lexaloffle.com/bbs/cposts/ce/celeste_classic-2.p8.png")
        );
        assert_eq!(
            bbs_url("15133").as_deref(),
            Some("https://www.lexaloffle.com/bbs/cposts/1/15133.p8.png")
        );
        assert_eq!(bbs_url("a/b"), None);
    }

    #[test]
    fn a_favourite_is_shelved_under_its_title_as_splore_shows_it() {
        assert_eq!(shelf_name("celeste classic", "x"), "Celeste Classic");
        assert_eq!(
            shelf_name("what? a/b: c", "x"),
            "What? Ab C".replace('?', "")
        );
        assert_eq!(shelf_name("...", "the_id"), "the_id");
        assert_eq!(shelf_name("splore", "the_id"), "the_id");
    }

    #[test]
    fn a_favourite_goes_on_the_shelf_once_and_stays_off_once_taken_off() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let home = home_dir(root);
        std::fs::create_dir_all(home.join("bbs/carts")).unwrap();
        let cart = b"\x89PNG\r\n\x1a\nnot really".to_vec();
        std::fs::write(home.join("bbs/carts/jelpi-0.p8.png"), &cart).unwrap();
        std::fs::write(
            home.join(FAVOURITES),
            "|jelpi-0| 0 | 0 | 0 | | jelpi |\n|other-1| 0 | 0 | 0 | | other |\n",
        )
        .unwrap();
        let mut asked = Vec::new();
        let added = shelve_favourites(root, |id| {
            asked.push(id.to_string());
            Fetched::Offline
        });
        assert_eq!(added, 1);
        assert_eq!(asked, ["other-1"]);
        let shelved = carts_dir(root).join("Jelpi.p8.png");
        assert_eq!(std::fs::read(&shelved).unwrap(), cart);
        std::fs::remove_file(&shelved).unwrap();
        assert_eq!(shelve_favourites(root, |_| Fetched::Offline), 0);
        assert!(!shelved.exists());
    }

    #[test]
    fn a_p8_label_is_read_in_pico8_colours() {
        let mut text = String::from("__lua__\n__label__\n");
        for _ in 0..LABEL {
            text.push_str(&"8".repeat(LABEL as usize));
            text.push('\n');
        }
        let rgba = label_from_p8(&text).unwrap();
        assert_eq!(rgba.len(), (LABEL * LABEL * 4) as usize);
        assert_eq!(&rgba[..4], &[0xff, 0x00, 0x4d, 0xff]);
        let black = text.replace('8', "0");
        assert_eq!(label_from_p8(&black), None);
    }

    #[test]
    fn the_splore_card_carries_its_label() {
        assert!(label_from_p8(&String::from_utf8_lossy(SPLORE_CART)).is_some());
    }

    #[test]
    fn a_png_label_is_the_window_in_the_cart_picture() {
        let (w, h) = PNG_SIZE;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let at = ((PNG_LABEL_AT.1 * w + PNG_LABEL_AT.0) * 4) as usize;
        rgba[at..at + 4].copy_from_slice(&[1, 2, 3, 255]);
        let png = encode_png(&rgba, w, h).unwrap();
        let label = label_from_png(&png).unwrap();
        assert_eq!(&label[..4], &[1, 2, 3, 255]);
        assert_eq!(label_from_png(&encode_png(&[0; 4], 1, 1).unwrap()), None);
    }

    #[test]
    fn labels_are_named_as_the_shelf_looks_them_up() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(carts_dir(root)).unwrap();
        std::fs::write(carts_dir(root).join(SPLORE_FILE), SPLORE_CART).unwrap();
        write_labels(root);
        assert!(root.join("Labels/PICO8/Splore.png").is_file());
    }

    #[test]
    fn pico8_plays_a_cart_or_opens_splore() {
        let root = Path::new("/sd");
        let bin = Path::new("/sd/BIOS/PICO-8/pico8_dyn");
        let cart = command_line(
            root,
            bin,
            Arch::Armhf,
            Path::new("/sd/Games/PICO8/a.p8"),
            true,
        );
        assert_eq!(cart[0], "/sd/BIOS/PICO-8/pico8_dyn");
        assert_eq!(cart[cart.len() - 2..], ["-run", "/sd/Games/PICO8/a.p8"]);
        assert!(cart.windows(2).any(|w| w == ["-home", "/sd/Saves/PICO8"]));
        let splore = command_line(
            root,
            bin,
            Arch::Armhf,
            Path::new("/sd/Games/PICO8/Splore.p8"),
            false,
        );
        assert_eq!(splore[0], "/lib/ld-linux-armhf.so.3");
        assert_eq!(splore.last().map(String::as_str), Some("-splore"));
    }

    #[test]
    fn a_binary_needs_its_dat_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("pico-8");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("pico8_64"), b"").unwrap();
        assert_eq!(find_binary(dir.path()), None);
        std::fs::write(inner.join("pico8.dat"), b"").unwrap();
        std::fs::write(inner.join("pico8_dyn"), b"").unwrap();
        assert_eq!(
            find_binary(dir.path()),
            Some((inner.join("pico8_dyn"), Arch::Armhf))
        );
    }

    #[test]
    fn ensure_executable_marks_a_copied_binary_to_run_directly() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("pico8_64");
        std::fs::write(&bin, b"elf").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!runs_directly(&bin));
        ensure_executable(&bin);
        assert!(runs_directly(&bin));
    }

    #[test]
    fn the_handhelds_pad_maps_its_bottom_button_to_o() {
        let pad = Pad {
            name: "H700 Gamepad".into(),
            bus: 0x19,
            vendor: 0x484b,
            product: 0x1688,
            version: 0x100,
            keys: vec![0x130, 0x131, 0x132, 0x133, 0x136, 0x137],
            axes: vec![ABS_HAT0X, ABS_HAT0X + 1],
        };
        let line = pad.mapping(crate::input::evdev::code_to_btn, None).unwrap();
        assert!(line.starts_with("19000000"), "{line}");
        // The H700's B, its bottom button, is code 0x131, SDL's button 1.
        assert!(line.contains(",a:b1,b:b0,"), "{line}");
        assert!(line.contains("dpup:h0.1"));
        assert!(line.ends_with("platform:Linux,"));
    }
}
