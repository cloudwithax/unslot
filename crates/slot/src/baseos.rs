use std::path::Path;

const RELEASE: &str = "/etc/baseos-release";

pub fn var(key: &str) -> Option<String> {
    var_in(key, std::env::var(key).ok(), Path::new(RELEASE))
}

pub fn var_in(key: &str, env: Option<String>, release: &Path) -> Option<String> {
    env.or_else(|| lookup(&std::fs::read_to_string(release).ok()?, key))
}

pub fn lookup(release: &str, key: &str) -> Option<String> {
    release.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        if k == key {
            Some(v.trim_matches('"').to_string())
        } else {
            None
        }
    })
}

/// Shown on the one-line notice strip, which fits about 50 characters.
pub const HOSTNAME_HINT: &str = "Letters, digits and hyphens (not at either end)";

pub fn valid_hostname(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn configured_hostname(root: &Path) -> String {
    with_config_root(root, |root| {
        std::fs::read_to_string(root.join("baseos.conf"))
    })
    .ok()
    .and_then(|text| {
        text.lines()
            .filter_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "hostname")
                    .then(|| value.split('#').next().unwrap_or("").trim().to_string())
            })
            .next_back()
    })
    .filter(|name| valid_hostname(name))
    .unwrap_or_else(crate::services::hostname)
}

/// Changes only hostname assignments, retaining all other bytes, including CRLF.
pub fn rewrite_hostname(text: &str, name: &str) -> String {
    let assignment = |line: &str| {
        line.split_once('=')
            .is_some_and(|(key, _)| key.trim() == "hostname")
    };
    let active = text.lines().any(assignment);
    let mut replaced = false;
    let mut out = String::with_capacity(text.len() + name.len() + 12);
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let candidate = bare.trim_start();
        let matches = assignment(candidate)
            || (!active
                && candidate
                    .strip_prefix('#')
                    .is_some_and(|s| assignment(s.trim_start())));
        if matches {
            out.push_str("hostname=");
            out.push_str(name);
            if let Some((_, comment)) = bare
                .split_once('=')
                .and_then(|(_, value)| value.split_once('#'))
            {
                out.push_str(" #");
                out.push_str(comment);
            }
            out.push_str(&line[bare.len()..]);
            replaced = true;
        } else {
            out.push_str(line);
        }
    }
    if !replaced {
        let ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(ending);
        }
        out.push_str("hostname=");
        out.push_str(name);
        out.push_str(ending);
    }
    out
}

pub fn set_hostname(root: &Path, name: &str) -> std::io::Result<()> {
    if !valid_hostname(name) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            HOSTNAME_HINT,
        ));
    }
    with_config_root(root, |root| {
        let path = root.join("baseos.conf");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        slot_store::atomic_write(&path, rewrite_hostname(&text, name).as_bytes())
    })
}

/// BaseOS mounts TF2 at /mnt/sdcard when present, but always reads settings from TF1 p7.
fn with_config_root<T>(
    root: &Path,
    f: impl FnOnce(&Path) -> std::io::Result<T>,
) -> std::io::Result<T> {
    const DEVICE: &str = "/dev/mmcblk0p7";
    const MOUNT: &str = "/tmp/slot-tf1-config";
    if crate::pak::CHIP != "h700" || !Path::new(DEVICE).exists() {
        return f(root);
    }
    let mounts = std::fs::read_to_string("/proc/self/mounts")?;
    if let Some(path) = mounts.lines().find_map(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        (fields.len() >= 4 && fields[0] == DEVICE && fields[3].split(',').any(|s| s == "rw"))
            .then(|| fields[1].replace("\\040", " "))
    }) {
        return f(Path::new(&path));
    }
    std::fs::create_dir_all(MOUNT)?;
    if !std::process::Command::new("mount")
        .args(["-t", "vfat", "-o", "rw,noatime", DEVICE, MOUNT])
        .status()?
        .success()
    {
        return Err(std::io::Error::other(
            "Could not mount TF1 for its settings",
        ));
    }
    let result = f(Path::new(MOUNT));
    let unmounted = std::process::Command::new("umount").arg(MOUNT).status()?;
    if !unmounted.success() {
        return Err(std::io::Error::other("Could not unmount TF1"));
    }
    result
}
