//! RetroArch GLSL shaders put on the card, each one pass, the game can be drawn with.

use std::path::Path;

use slot_gfx::{parse_shader_parameters, ShaderParam};

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

/// A shader's source, whether it samples the game smoothly, and the parameters it declares.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub source: String,
    pub linear: bool,
    pub parameters: Vec<ShaderParam>,
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
        let parameters = parse_shader_parameters(&text);
        return Ok(Loaded {
            source: text,
            linear: false,
            parameters,
        });
    }
    let value = |key: &str| crate::bezel::cfg_value(&text, key);
    let passes = value("shaders").and_then(|v| v.parse::<u32>().ok());
    if passes != Some(1) {
        return Err(format!("{name}: only a preset of one pass is drawn"));
    }
    let shader = value("shader0").ok_or_else(|| format!("{name}: no shader0"))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let source = std::fs::read_to_string(dir.join(shader)).map_err(|e| format!("{shader}: {e}"))?;
    let mut parameters = parse_shader_parameters(&source);
    for p in &mut parameters {
        if let Some(v) = value(&p.name).and_then(|v| v.parse::<f32>().ok()) {
            p.default = v.clamp(p.min, p.max);
        }
    }
    Ok(Loaded {
        source,
        linear: value("filter_linear0") == Some("true"),
        parameters,
    })
}

/// Where user parameter overrides for `name` are kept on the card.
pub fn params_file(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| match c == '/' || c == '\\' {
            true => '_',
            false => c,
        })
        .collect();
    format!("{}/Shaders/{clean}.txt", slot_store::CONFIG_DIR)
}

/// Reads the current value for each declared parameter, falling back to its default.
pub fn read_values(root: &Path, name: &str, params: &[ShaderParam]) -> Vec<(String, f32)> {
    let saved = slot_store::ini::read(root, &params_file(name));
    params
        .iter()
        .map(|p| {
            let v = saved
                .get(&p.name)
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(p.default)
                .clamp(p.min, p.max);
            (p.name.clone(), v)
        })
        .collect()
}

pub fn write_value(
    root: &Path,
    name: &str,
    param: &ShaderParam,
    value: f32,
) -> std::io::Result<()> {
    let file = params_file(name);
    if (value - param.default).abs() < 1e-5 {
        slot_store::ini::remove(root, &file, &param.name)
    } else {
        slot_store::ini::write(root, &file, &param.name, &format_param(value, param.step))
    }
}

pub fn reset_all(root: &Path, name: &str) -> std::io::Result<()> {
    let path = root.join(params_file(name));
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

pub fn step_value(param: &ShaderParam, current: f32, dir: i32) -> f32 {
    if param.step <= 0.0 || param.max <= param.min {
        return current.clamp(param.min, param.max);
    }
    let raw = current + dir as f32 * param.step;
    let steps = ((raw - param.min) / param.step).round();
    (param.min + steps * param.step).clamp(param.min, param.max)
}

pub fn decimals_for(step: f32) -> usize {
    let s = step.abs();
    if (s - s.round()).abs() < 1e-5 {
        0
    } else if (s * 10.0 - (s * 10.0).round()).abs() < 1e-4 {
        1
    } else if (s * 100.0 - (s * 100.0).round()).abs() < 1e-3 {
        2
    } else {
        3
    }
}

pub fn format_param(value: f32, step: f32) -> String {
    let v = if value.abs() < 1e-6 { 0.0 } else { value };
    let dec = decimals_for(step);
    format!("{v:.dec$}")
}
