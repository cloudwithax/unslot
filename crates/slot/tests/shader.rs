use slot::shader::{
    format_param, list, load, params_file, read_values, reset_all, step_value, write_value, Loaded,
};
use slot_gfx::ShaderParam;

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
            linear: false,
            parameters: Vec::new(),
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
            linear: true,
            parameters: Vec::new(),
        })
    );
    assert!(load(d.path(), "one-of-two.glslp").is_err());
    assert!(load(d.path(), "gone.glsl").is_err());
}

#[test]
fn pragma_parameters_are_parsed_overridden_by_preset_and_persisted() {
    let shader = r#"
#pragma parameter GAIN "Brightness Gain" 1.0 0.0 2.0 0.05
#pragma parameter INT_MODE "Integer Mode" 1.0 0.0 3.0
void main() {}
"#;
    let d = card(&[
        ("param.glsl", shader),
        (
            "param.glslp",
            "shaders = 1\nshader0 = param.glsl\nGAIN = \"1.50\"\n",
        ),
    ]);
    let raw = load(d.path(), "param.glsl").unwrap();
    assert_eq!(
        raw.parameters,
        vec![
            ShaderParam {
                name: "GAIN".into(),
                label: "Brightness Gain".into(),
                default: 1.0,
                min: 0.0,
                max: 2.0,
                step: 0.05,
            },
            ShaderParam {
                name: "INT_MODE".into(),
                label: "Integer Mode".into(),
                default: 1.0,
                min: 0.0,
                max: 3.0,
                step: 0.15,
            },
        ]
    );

    let preset = load(d.path(), "param.glslp").unwrap();
    assert_eq!(preset.parameters[0].default, 1.5);
    assert_eq!(preset.parameters[1].default, 1.0);

    assert_eq!(params_file("param.glslp"), "Config/Shaders/param.glslp.txt");
    let vals = read_values(d.path(), "param.glslp", &preset.parameters);
    assert_eq!(vals, [("GAIN".into(), 1.5), ("INT_MODE".into(), 1.0)]);

    let stepped = step_value(&preset.parameters[0], 1.5, 1);
    assert!((stepped - 1.55).abs() < 1e-4, "stepped={stepped}");
    assert_eq!(format_param(stepped, preset.parameters[0].step), "1.55");
    assert_eq!(format_param(2.0, 1.0), "2");

    write_value(d.path(), "param.glslp", &preset.parameters[0], stepped).unwrap();
    let reread = read_values(d.path(), "param.glslp", &preset.parameters);
    assert!((reread[0].1 - 1.55).abs() < 1e-4);

    // Writing default removes the override entry; reset_all removes all overrides.
    write_value(d.path(), "param.glslp", &preset.parameters[1], 2.5).unwrap();
    write_value(
        d.path(),
        "param.glslp",
        &preset.parameters[0],
        preset.parameters[0].default,
    )
    .unwrap();
    let after_default = read_values(d.path(), "param.glslp", &preset.parameters);
    assert!((after_default[0].1 - 1.5).abs() < 1e-4);
    assert!((after_default[1].1 - 2.5).abs() < 1e-4);

    reset_all(d.path(), "param.glslp").unwrap();
    let after_reset = read_values(d.path(), "param.glslp", &preset.parameters);
    assert_eq!(
        after_reset,
        [("GAIN".into(), 1.5), ("INT_MODE".into(), 1.0)]
    );
}

#[test]
fn shader_parameters_row_appears_only_when_parameterized_and_persists_edits() {
    use slot::app::{App, PageModel};
    use slot_input::{Action, Btn};
    use slot_store::{write_slot_state, Platform, SlotState};
    use slot_ui::QuickRow;

    let d = card(&[
        ("plain.glsl", "void main() { gl_FragColor = vec4(1.0); }\n"),
        (
            "param.glsl",
            "#pragma parameter GAIN \"Brightness Gain\" 1.0 0.0 2.0 0.05\n#pragma parameter MODE \"Blend Mode\" 1.0 0.0 3.0 1.0\nvoid main() {}\n",
        ),
    ]);
    let mut state = SlotState {
        clock_set: true,
        ..SlotState::default()
    };
    state.screen_mut(Platform::Gba).custom_shader = Some("plain.glsl".into());
    slot::root::ensure(d.path());
    write_slot_state(d.path(), &state).unwrap();

    let mut app = App::boot(d.path());
    let press = |a: &mut App, b: Btn| {
        a.apply(Action::GbaDown(b));
        a.apply(Action::GbaUp(b));
    };
    assert!(app
        .hidden_rows()
        .contains(&QuickRow::ShaderParamsOf(Platform::Gba)));

    app.apply(Action::QuickMenu);
    while app.quick_menu() != Some(QuickRow::Screen) {
        press(&mut app, Btn::Down);
    }
    press(&mut app, Btn::A);
    press(&mut app, Btn::A);
    assert_eq!(app.quick_menu(), Some(QuickRow::ShaderOf(Platform::Gba)));
    while app.custom_shader(Platform::Gba) != Some("param.glsl") {
        press(&mut app, Btn::Right);
    }
    assert!(!app
        .hidden_rows()
        .contains(&QuickRow::ShaderParamsOf(Platform::Gba)));

    press(&mut app, Btn::Down);
    assert_eq!(
        app.quick_menu(),
        Some(QuickRow::ShaderParamsOf(Platform::Gba))
    );
    press(&mut app, Btn::A);
    let Some(PageModel::List(page)) = app.page_model() else {
        panic!("expected shader parameter list page");
    };
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.rows[0].label, "Brightness Gain");
    assert_eq!(page.rows[0].detail, "1.00");

    press(&mut app, Btn::Right);
    let Some(PageModel::List(page)) = app.page_model() else {
        panic!("expected updated list page");
    };
    assert_eq!(page.rows[0].detail, "1.05");
    assert!(d.path().join("Config/Shaders/param.glsl.txt").is_file());

    press(&mut app, Btn::Down);
    press(&mut app, Btn::Right);
    press(&mut app, Btn::X);
    let Some(PageModel::List(page)) = app.page_model() else {
        panic!("expected reset row list page");
    };
    assert_eq!(page.rows[1].detail, "1");
    assert_eq!(page.rows[0].detail, "1.05");

    press(&mut app, Btn::Y);
    let Some(PageModel::List(page)) = app.page_model() else {
        panic!("expected reset all list page");
    };
    assert_eq!(page.rows[0].detail, "1.00");

    press(&mut app, Btn::B);
    assert!(app.page_model().is_none());
    assert_eq!(
        app.quick_menu(),
        Some(QuickRow::ShaderParamsOf(Platform::Gba))
    );
}
