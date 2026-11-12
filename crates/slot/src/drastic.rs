//! slot's keys while it has quit for DraStic. The volume keys are read as slot reads them, and
//! the level they leave is slot's, kept in slot.state, and written for libunslot-drastic.so,
//! preloaded into DraStic, to scale its sound by and show a bar for, so a game sounds as loud in
//! DraStic as it would in slot. A tap of Menu stops DraStic where it is and opens slot's in-game
//! menu over its picture, which DraStic only draws where it changes, so the picture is put back
//! as it was before DraStic goes on.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use slot_input::{Action, Btn, Gestures, InputSource, Millis, MUTE_CHORD_MS};
use slot_store::{read_slot_state, write_slot_state, SlotState, VOLUME_MAX};
use slot_ui::QuickRow;

use crate::app::VOLUME_STEP;

/// Names the file the level goes to, which launch.sh hands DraStic's shim as well.
pub const FILE_VAR: &str = "UNSLOT_VOLUME";
const HEADPHONES_EVERY: Duration = Duration::from_secs(1);
/// What `--drastic-menu` exits with when Exit Game was picked; anything else goes back to the game.
pub const MENU_EXIT: i32 = 3;
/// How long DraStic has to save and close once asked, before it is made to.
const QUIT_GRACE: Duration = Duration::from_secs(10);
const FB: &str = "/dev/fb0";

/// slot's volume and mute, for the speaker or the headphones, as the keys move them.
pub struct Volume {
    pub state: SlotState,
    pub headphones: bool,
    before: Vec<(u8, bool, Millis)>,
}

impl Volume {
    pub fn new(state: SlotState, headphones: bool) -> Self {
        Volume {
            state,
            headphones,
            before: Vec::new(),
        }
    }

    fn level_and_muted(&mut self) -> (&mut u8, &mut bool) {
        match self.headphones {
            true => (&mut self.state.volume_hp, &mut self.state.muted_hp),
            false => (&mut self.state.volume, &mut self.state.muted),
        }
    }

    /// The level DraStic plays at: nothing when muted.
    pub fn output(&mut self) -> u8 {
        let (level, muted) = self.level_and_muted();
        if *muted {
            0
        } else {
            *level
        }
    }

    /// Moves the volume as slot does for `action`, the mute chord putting back what its two
    /// presses moved; whether anything changed.
    pub fn act(&mut self, action: Action, now: Millis) -> bool {
        let before = self.state.clone();
        match action {
            Action::VolumeUp | Action::VolumeDown => {
                let (level, muted) = self.level_and_muted();
                let was = (*level, *muted);
                *level = match action {
                    Action::VolumeUp => level.saturating_add(VOLUME_STEP).min(VOLUME_MAX),
                    _ => level.saturating_sub(VOLUME_STEP),
                };
                *muted = false;
                if self.before.len() == 2 {
                    self.before.remove(0);
                }
                self.before.push((was.0, was.1, now));
            }
            Action::MuteToggle => {
                let undo = self
                    .before
                    .iter()
                    .find(|(_, _, at)| now.saturating_sub(*at) <= MUTE_CHORD_MS)
                    .copied();
                self.before.clear();
                let (level, muted) = self.level_and_muted();
                if let Some((was, was_muted, _)) = undo {
                    *level = was;
                    *muted = was_muted;
                }
                *muted = !*muted;
            }
            _ => return false,
        }
        self.state != before
    }
}

/// What the shim reads: the level DraStic plays at, how many times the keys have been pressed,
/// which shows the bar even when a press leaves the level where it was, and 1 once DraStic is to
/// save and close. Written whole and renamed into place, so the shim never reads half of it.
pub fn line(level: u8, presses: u32, quit: bool) -> String {
    format!("{level} {presses} {}\n", quit as u8)
}

fn write_level(file: &Path, level: u8, presses: u32, quit: bool) {
    let part = file.with_extension("part");
    let wrote = std::fs::write(&part, line(level, presses, quit))
        .and_then(|()| std::fs::rename(&part, file));
    if let Err(e) = wrote {
        eprintln!("slot: drastic volume: {}: {e}", file.display());
    }
}

/// Runs until the script that started it is gone.
pub fn run(root: &Path) {
    use slot_power::Platform;
    let Some(file) = std::env::var_os(FILE_VAR) else {
        eprintln!("slot: drastic volume: {FILE_VAR} is not set");
        return;
    };
    let file = Path::new(&file);
    let power = slot_power::DevicePlatform::new(root.to_path_buf());
    let mut input = crate::input::DeviceInput::open(root);
    let mut gestures = Gestures::new();
    let mut volume = Volume::new(read_slot_state(root), power.headphones());
    let start = Instant::now();
    let mut next_jack = start + HEADPHONES_EVERY;
    let mut presses = 0u32;
    let mut written = volume.output();
    write_level(file, written, presses, false);
    let parent = std::os::unix::process::parent_id();
    while std::os::unix::process::parent_id() == parent {
        std::thread::sleep(Duration::from_millis(10));
        let now = start.elapsed().as_millis() as Millis;
        let mut actions = gestures.tick(now);
        for ev in input.poll(now) {
            actions.extend(gestures.feed(ev, now));
        }
        if actions.contains(&Action::QuickMenu) {
            if let Some(game) = drastic_pid(parent) {
                if pause_for_menu(root, game) {
                    write_level(file, volume.output(), presses, true);
                    quit(game, parent);
                    return;
                }
            }
            // What was pressed in the menu is the menu's.
            input.poll(start.elapsed().as_millis() as Millis);
            gestures = Gestures::new();
            continue;
        }
        let mut pressed = false;
        for a in actions {
            pressed |= matches!(
                a,
                Action::VolumeUp | Action::VolumeDown | Action::MuteToggle
            );
            if volume.act(a, now) {
                if let Err(e) = write_slot_state(root, &volume.state) {
                    eprintln!("slot: drastic volume: {e}");
                }
            }
        }
        if Instant::now() >= next_jack {
            next_jack += HEADPHONES_EVERY;
            volume.headphones = power.headphones();
        }
        if pressed {
            presses = presses.wrapping_add(1);
        }
        if pressed || volume.output() != written {
            written = volume.output();
            write_level(file, written, presses, false);
        }
    }
}

/// DraStic, as launch.sh started it beside this.
fn drastic_pid(parent: u32) -> Option<i32> {
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let pid: i32 = e.file_name().to_str()?.parse().ok()?;
        let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
        // pid (comm) state ppid ...
        let (comm, rest) = stat.split_once(" (")?.1.rsplit_once(") ")?;
        let ppid: u32 = rest.split_whitespace().nth(1)?.parse().ok()?;
        (comm == "drastic" && ppid == parent).then_some(pid)
    })
}

/// Stops DraStic, opens the menu over its picture and puts the picture back once the menu is
/// gone; whether Exit Game was picked. DraStic goes on either way, to close itself if so.
fn pause_for_menu(root: &Path, game: i32) -> bool {
    unsafe { libc::kill(game, libc::SIGSTOP) };
    let picture = std::fs::read(FB);
    let frame = frame_file();
    if let Ok(p) = &picture {
        if let Err(e) = std::fs::write(&frame, p) {
            eprintln!("slot: drastic menu: {}: {e}", frame.display());
        }
    }
    let exited = menu_command(root, &frame)
        .status()
        .map_err(|e| eprintln!("slot: drastic menu: {e}"))
        .is_ok_and(|s| s.code() == Some(MENU_EXIT));
    let _ = std::fs::remove_file(&frame);
    if let Ok(p) = picture {
        let put = std::fs::OpenOptions::new()
            .write(true)
            .open(FB)
            .and_then(|mut fb| fb.write_all(&p));
        if let Err(e) = put {
            eprintln!("slot: drastic menu: {FB}: {e}");
        }
    }
    unsafe { libc::kill(game, libc::SIGCONT) };
    exited
}

fn frame_file() -> PathBuf {
    let file = std::env::var_os(FILE_VAR)
        .map(PathBuf::from)
        .unwrap_or_else(|| "/run/unslot".into());
    file.with_extension("frame")
}

/// This binary again, through the loader it was started with when it was.
fn menu_command(root: &Path, frame: &Path) -> std::process::Command {
    let me = std::env::args_os().next().unwrap_or_default();
    let exe = std::env::current_exe().unwrap_or_default();
    let loader = exe
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("ld-linux"));
    let mut c = match loader {
        true => {
            let mut c = std::process::Command::new(&exe);
            c.arg(&me);
            c
        }
        false => std::process::Command::new(&me),
    };
    c.arg("--drastic-menu").arg(frame).env("SLOT_ROOT", root);
    c
}

/// Waits for DraStic to close itself, as the shim asks it to, and closes it if it does not.
fn quit(game: i32, parent: u32) {
    let asked = Instant::now();
    while drastic_pid(parent) == Some(game) {
        if asked.elapsed() >= QUIT_GRACE {
            eprintln!("slot: drastic menu: DraStic did not close, so it is made to");
            unsafe { libc::kill(game, libc::SIGKILL) };
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The in-game menu's rows for a DS game: DraStic keeps its own states and settings, in its
/// menu on Select + X, and the picture never leaves DraStic for a TV.
const HIDDEN: [QuickRow; 3] = [QuickRow::States, QuickRow::Settings, QuickRow::PlayCast];

/// What a press does in the menu.
#[derive(Debug, PartialEq)]
pub enum MenuStep {
    Move(QuickRow),
    Resume,
    Exit,
    Nothing,
}

pub fn menu_step(row: QuickRow, action: Action) -> MenuStep {
    match action {
        Action::GbaDown(Btn::Up) => MenuStep::Move(row.up(&HIDDEN)),
        Action::GbaDown(Btn::Down) => MenuStep::Move(row.down(&HIDDEN)),
        Action::GbaDown(Btn::A) if row == QuickRow::ExitGame => MenuStep::Exit,
        Action::GbaDown(Btn::A | Btn::B) | Action::QuickMenu => MenuStep::Resume,
        _ => MenuStep::Nothing,
    }
}

/// `--drastic-menu`: slot's in-game menu over the picture DraStic was stopped on, which `frame`
/// holds as the screen's memory had it; the exit code is MENU_EXIT for Exit Game.
pub fn menu(root: &Path, frame: &Path) -> i32 {
    use slot_gfx::{Canvas, Compositor, FbdevSurface, Layout, Surface};
    use slot_ui::{quick_caret_face, quick_label_face, quick_legend_faces, quick_pointer_face};
    use slot_ui::{Draw, QuickMenu, QuickMenuFaces, UndoFace};

    let mut surface = match FbdevSurface::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("slot: drastic menu: {e}");
            return 0;
        }
    };
    let mut compositor = match Compositor::new(&surface) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("slot: drastic menu: {e}");
            return 0;
        }
    };
    let window = surface.window_size();
    compositor.fit(window);
    let canvas = Canvas::of(Layout::Native, window);
    let (w, h) = (window.0 as usize, window.1 as usize);
    // The screen's memory is BGRA, the first screenful the one shown.
    let picture = std::fs::read(frame)
        .ok()
        .filter(|p| p.len() >= w * h * 4)
        .map(|p| {
            let mut rgba = p[..w * h * 4].to_vec();
            for px in rgba.chunks_exact_mut(4) {
                px.swap(0, 2);
                px[3] = 255;
            }
            compositor.create_texture(window.0, window.1, &rgba)
        });
    let mut up = |f: UndoFace| (compositor.create_texture(f.w, f.h, &f.rgba), f.w, f.h);
    let blank = up(UndoFace {
        rgba: vec![0; 4],
        w: 1,
        h: 1,
    });
    let labels = QuickRow::ALL
        .iter()
        .map(|&r| match QuickRow::PLAY.contains(&r) {
            true => [
                up(quick_label_face(r, false)),
                up(quick_label_face(r, true)),
            ],
            false => [blank; 2],
        })
        .collect();
    let pointer = up(quick_pointer_face());
    let carets = [up(quick_caret_face(false)), up(quick_caret_face(true))];
    let legend = quick_legend_faces([255; 3]).map(|f| {
        let (t, w, _) = up(f);
        (t, w)
    });
    let faces = QuickMenuFaces {
        labels,
        pointer,
        values: Vec::new(),
        carets,
        legend,
        legend_light: legend,
    };

    let mut input = crate::input::DeviceInput::open(root);
    let mut gestures = Gestures::new();
    let start = Instant::now();
    let mut row = QuickRow::Resume;
    let mut draws = Vec::new();
    loop {
        let now = start.elapsed().as_millis() as Millis;
        let mut actions = gestures.tick(now);
        for ev in input.poll(now) {
            actions.extend(gestures.feed(ev, now));
        }
        for a in actions {
            match menu_step(row, a) {
                MenuStep::Move(to) => row = to,
                MenuStep::Resume => return 0,
                MenuStep::Exit => return MENU_EXIT,
                MenuStep::Nothing => {}
            }
        }
        draws.clear();
        if let Some(tex) = picture {
            draws.push(Draw::Tex {
                x: 0.0,
                y: 0.0,
                w: canvas.w,
                h: canvas.h,
                tex,
                alpha: 1.0,
            });
        }
        QuickMenu {
            row,
            hidden: &HIDDEN,
            values: [None; QuickRow::COUNT],
            clock: None,
            wallpaper: None,
            theme: None,
            faces: Some(&faces),
            ground: None,
            palette: None,
            bezels: [None; slot_store::Platform::COUNT],
            bezel: None,
            shaders: [None; slot_store::Platform::COUNT],
            shader: None,
            over_game: true,
            footer_legends: false,
            footer_top: canvas.h,
            canvas,
        }
        .draw(&mut draws);
        compositor.begin_frame(Layout::Native);
        compositor.set_screen_power(1.0);
        compositor.draw_list(&draws);
        compositor.end_frame(window);
        if let Err(e) = surface.swap() {
            eprintln!("slot: drastic menu: {e}");
            return 0;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shim_reads_the_level_then_the_presses() {
        assert_eq!(line(55, 3, false), "55 3 0\n");
        assert_eq!(line(0, 4, true), "0 4 1\n");
    }

    #[test]
    fn the_menu_moves_between_resume_and_exit_game_alone() {
        assert_eq!(
            menu_step(QuickRow::Resume, Action::GbaDown(Btn::Down)),
            MenuStep::Move(QuickRow::ExitGame)
        );
        assert_eq!(
            menu_step(QuickRow::ExitGame, Action::GbaDown(Btn::Up)),
            MenuStep::Move(QuickRow::Resume)
        );
    }

    #[test]
    fn a_picks_the_row_and_b_or_menu_go_back_to_the_game() {
        assert_eq!(
            menu_step(QuickRow::ExitGame, Action::GbaDown(Btn::A)),
            MenuStep::Exit
        );
        assert_eq!(
            menu_step(QuickRow::Resume, Action::GbaDown(Btn::A)),
            MenuStep::Resume
        );
        assert_eq!(
            menu_step(QuickRow::ExitGame, Action::GbaDown(Btn::B)),
            MenuStep::Resume
        );
        assert_eq!(
            menu_step(QuickRow::ExitGame, Action::QuickMenu),
            MenuStep::Resume
        );
        assert_eq!(
            menu_step(QuickRow::Resume, Action::GbaDown(Btn::Select)),
            MenuStep::Nothing
        );
    }

    #[test]
    fn the_keys_move_the_speakers_level_and_unmute_it() {
        let mut v = Volume::new(
            SlotState {
                volume: 50,
                muted: true,
                ..Default::default()
            },
            false,
        );
        assert_eq!(v.output(), 0);
        assert!(v.act(Action::VolumeUp, 0));
        assert_eq!((v.state.volume, v.state.muted, v.output()), (55, false, 55));
        assert!(v.act(Action::VolumeDown, 1000));
        assert_eq!(v.state.volume, 50);
    }

    #[test]
    fn the_headphones_keep_their_own_level() {
        let mut v = Volume::new(
            SlotState {
                volume: 50,
                volume_hp: 20,
                ..Default::default()
            },
            true,
        );
        v.act(Action::VolumeUp, 0);
        assert_eq!((v.state.volume, v.state.volume_hp), (50, 25));
    }

    #[test]
    fn the_mute_chord_mutes_where_the_volume_was() {
        let mut v = Volume::new(
            SlotState {
                volume: 50,
                ..Default::default()
            },
            false,
        );
        v.act(Action::VolumeUp, 1000);
        v.act(Action::VolumeDown, 1100);
        v.act(Action::MuteToggle, 1100);
        assert_eq!((v.state.volume, v.state.muted, v.output()), (50, true, 0));
        v.act(Action::MuteToggle, 5000);
        assert_eq!(v.output(), 50);
    }
}
