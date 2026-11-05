//! RetroArch GLSL shaders put on the card, each one pass, the game can be drawn with.

use std::path::Path;

pub const DIR: &str = "Shaders";

/// The shaders in `Shaders`: `.glsl` files, and `.glslp` presets naming one.
pub fn list(root: &Path) -> Vec<String> {
    let mut files: Vec<String> = std::fs::read_dir(root.join(DIR))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !slot_store::is_hidden(p))
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("glsl") || e.eq_ignore_ascii_case("glslp"))
        })
        .filter_map(|p| Some(p.file_name()?.to_str()?.to_owned()))
        .collect();
    files.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    files
}

/// A shader's source and whether it samples the game smoothly.
#[derive(Debug, PartialEq)]
pub struct Loaded {
    pub source: String,
    pub linear: bool,
}

/// Reads `name` from `Shaders`. A preset gives its one pass, read beside it; a preset of more
/// passes is refused, as only the first would be drawn.
pub fn load(root: &Path, name: &str) -> Result<Loaded, String> {
    let path = root.join(DIR).join(name);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{name}: {e}"))?;
    let is_preset = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("glslp"));
    if !is_preset {
        return Ok(Loaded {
            source: text,
            linear: false,
        });
    }
    let value = |key| crate::bezel::cfg_value(&text, key);
    let passes = value("shaders").and_then(|v| v.parse::<u32>().ok());
    if passes != Some(1) {
        return Err(format!("{name}: only a preset of one pass is drawn"));
    }
    let shader = value("shader0").ok_or_else(|| format!("{name}: no shader0"))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let source = std::fs::read_to_string(dir.join(shader)).map_err(|e| format!("{shader}: {e}"))?;
    Ok(Loaded {
        source,
        linear: value("filter_linear0") == Some("true"),
    })
}
