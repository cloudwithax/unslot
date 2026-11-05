use slot::shader::{list, load, Loaded};

fn card(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("Shaders")).unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join("Shaders").join(name), text).unwrap();
    }
    d
}

#[test]
fn the_card_lists_its_shaders_and_presets_in_order() {
    let d = card(&[
        ("zfast.glsl", ""),
        ("Crt.glslp", ""),
        ("._crt.glsl", ""),
        ("readme.txt", ""),
    ]);
    assert_eq!(list(d.path()), ["Crt.glslp", "zfast.glsl"]);
    assert!(list(&d.path().join("nowhere")).is_empty());
}

#[test]
fn a_glsl_file_is_its_own_source_sampled_sharp() {
    let d = card(&[("a.glsl", "void main() {}")]);
    assert_eq!(
        load(d.path(), "a.glsl"),
        Ok(Loaded {
            source: "void main() {}".into(),
            linear: false
        })
    );
}

#[test]
fn a_preset_of_one_pass_gives_its_shader_and_filter() {
    let d = card(&[
        (
            "crt.glslp",
            "shaders = \"1\"\nshader0 = \"crt/easy.glsl\"\nfilter_linear0 = \"true\"\n",
        ),
        (
            "one-of-two.glslp",
            "shaders = 2\nshader0 = a.glsl\nshader1 = a.glsl\n",
        ),
    ]);
    std::fs::create_dir_all(d.path().join("Shaders/crt")).unwrap();
    std::fs::write(d.path().join("Shaders/crt/easy.glsl"), "easy").unwrap();
    assert_eq!(
        load(d.path(), "crt.glslp"),
        Ok(Loaded {
            source: "easy".into(),
            linear: true
        })
    );
    assert!(load(d.path(), "one-of-two.glslp").is_err());
    assert!(load(d.path(), "gone.glsl").is_err());
}
