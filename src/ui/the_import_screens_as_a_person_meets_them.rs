//! What a person reads while they bring a game file into a project: the hint under a held drag, the report and toast an import
//! ends with, the question of a file from elsewhere, the picker that locates a model and the name of a new texture dictionary.
//!
//! ⚠ Drawn through the whole shell at the two window sizes the screens are judged at, and read as the text and rectangles the
//! frames paint, so a hint that prints over a table, a button hidden under a toast or a default that answers for the person fails here.

use std::path::{Path, PathBuf};

use egui::epaint::ClippedShape;

use super::frames_of_the_shell::Shell;
use super::the_project_screens_on_screen::{a_project_shell, names, shows};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::theme;

/// The two sizes of the window the screens are judged at: the real minimum and a common one.
const SMALL: (u32, u32) = (1280, 720);
const LARGE: (u32, u32) = (1600, 900);

fn write(path: &Path, body: &[u8]) -> PathBuf {
    super::the_project_screens_on_screen::write(path, body)
}

/// Every text the frames painted, with the rectangle it fills.
fn texts(shapes: &[ClippedShape]) -> Vec<(egui::Rect, String)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(egui::Rect, String)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.rect.translate(t.pos.to_vec2()),
                t.galley.text().to_string(),
            )),
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// Every filled rectangle the frames painted, with its fill.
fn plates(shapes: &[ClippedShape]) -> Vec<(egui::Rect, egui::Color32)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Color32)>) {
        match shape {
            egui::Shape::Rect(r) => out.push((r.rect, r.fill)),
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

fn window(size: (u32, u32)) -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32))
}

/// The plate the hint is drawn on: the filled rectangle that holds `title`, and the text of it.
fn the_plate_of(shapes: &[ClippedShape], title: &str) -> (egui::Rect, Vec<String>) {
    let all = texts(shapes);
    let (at, _) = all
        .iter()
        .find(|(_, text)| text == title)
        .unwrap_or_else(|| panic!("no hint says `{title}`"));
    let plate = plates(shapes)
        .into_iter()
        .filter(|(rect, fill)| *fill == theme::BG_L3.get() && rect.contains_rect(*at))
        .map(|(rect, _)| rect)
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap_or_else(|| panic!("`{title}` is bare text with no plate behind it"));
    let inside = all
        .into_iter()
        .filter(|(rect, _)| plate.contains_rect(*rect))
        .map(|(_, text)| text)
        .collect();
    (plate, inside)
}

#[test]
fn the_hint_of_a_held_drag_is_on_a_plate_inside_the_window_and_says_what_the_drop_changes() {
    let _serial = theme::serial();
    for size in [SMALL, LARGE] {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
        let held = vec![write(&dir.path().join("downloads/hull.ydd"), b"x")];
        shell.state.active_workspace = Workspace::Project;
        shell.settled_sized(size);

        // Over the middle of the table, where rows of text are.
        let shapes = hover(
            &mut shell,
            size,
            egui::pos2(size.0 as f32 * 0.5, 300.0),
            &held,
        );
        let (plate, said) = the_plate_of(&shapes, "Import into the project");
        assert!(
            said.iter().any(|t| t.contains("originals are not touched")),
            "the hint says what the drop will do to the files it is given: {said:?}"
        );
        assert!(
            window(size).contains_rect(plate),
            "{plate:?} leaves {size:?}"
        );

        // At the corner of the Project workspace, where a hint at the pointer would run off the window.
        let corner = egui::pos2(size.0 as f32 - 3.0, size.1 as f32 - 30.0);
        let shapes = hover(&mut shell, size, corner, &held);
        let (plate, _) = the_plate_of(&shapes, "Import into the project");
        assert!(
            window(size).contains_rect(plate),
            "at the corner {plate:?} leaves {size:?}"
        );

        shell.state.active_workspace = Workspace::Browse;
        shell.settled_sized(size);
        let shapes = hover(
            &mut shell,
            size,
            egui::pos2(size.0 as f32 * 0.5, 300.0),
            &held,
        );
        let (_, said) = the_plate_of(&shapes, "Open in OpenRage");
        assert!(
            said.iter().any(|t| t.contains("Nothing is changed")),
            "a drop that only opens says it changes nothing: {said:?}"
        );
        shell.hover_end();
    }
}

fn hover(
    shell: &mut Shell,
    size: (u32, u32),
    at: egui::Pos2,
    held: &[PathBuf],
) -> Vec<ClippedShape> {
    shell.hover_files_sized(size, at, held.to_vec())
}

/// Frames, polling what the workers left, until the import that was asked for has ended and its report is in.
fn until_imported(shell: &mut Shell, size: (u32, u32)) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        shell.state.poll_workbench();
        shell.draw_sized(size, Vec::new());
        let imports = &shell.state.workbench.imports;
        if imports.report().is_some() && !imports.is_running() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the import never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    shell.settled_sized(size);
}

fn said_by(shapes: &[ClippedShape]) -> Vec<String> {
    texts(shapes).into_iter().map(|(_, text)| text).collect()
}

#[test]
fn an_import_with_something_to_read_opens_a_window_of_plain_sentences_below_the_table_and_raises_no_toast(
) {
    let _serial = theme::serial();
    for size in [SMALL, LARGE] {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
        let folder = dir.path().join("downloads");
        write(
            &folder.join("road.ytd"),
            &or_source::sample::textures_of("road"),
        );
        write(&folder.join("broken_dict.ytd"), &[0xC4; 64]);
        write(&folder.join("readme.txt"), b"hello");
        shell.state.import_game_files(vec![folder]);
        until_imported(&mut shell, size);

        let shapes = shell.settled_sized(size);
        let said = said_by(&shapes);
        let has = |part: &str| said.iter().any(|t| t.contains(part));
        assert!(
            has("3 files: 1 became a source, 1 kept as it is, 1 refused"),
            "the summary counts what happened and no nought: {said:?}"
        );
        assert!(
            has("OpenRage can read") && has("has no editable form yet"),
            "a refused file and a text file are told in sentences: {said:?}"
        );
        for jargon in [
            "this build reads",
            "RSC7",
            "0x",
            "compiled file",
            "file(s)",
            "0 already",
        ] {
            assert!(!has(jargon), "`{jargon}` is on screen: {said:?}");
        }
        let (title, _) = (texts(&shapes).into_iter())
            .find(|(_, text)| text == "Import report")
            .expect("the report window is open");
        assert!(
            title.min.y > size.1 as f32 * 0.35,
            "the window sits at the foot of the screen, so the table it filled stays in view: {title:?}"
        );
        assert!(
            shell.state.toasts.iter().all(|t| t.action.is_none()),
            "the window has Undo import itself, so no toast offers it again"
        );
    }
}

#[test]
fn an_import_that_worked_is_a_toast_with_two_buttons_and_opens_no_window() {
    let _serial = theme::serial();
    for size in [SMALL, LARGE] {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
        let one = write(
            &dir.path().join("downloads/road.ytd"),
            &or_source::sample::textures_of("road"),
        );
        shell.state.import_game_files(vec![one]);
        until_imported(&mut shell, size);

        assert!(
            shows(&shell, "Undo import") && shows(&shell, "Show report"),
            "the toast offers both: {:?}",
            names(&shell)
        );
        let shapes = shell.settled_sized(size);
        let said = said_by(&shapes);
        assert!(
            said.iter()
                .any(|t| t.starts_with("Imported 1 file: 1 became a source")),
            "{said:?}"
        );
        assert!(
            !said.iter().any(|t| t == "Import report"),
            "no window covers the table the import filled"
        );
    }
}

#[test]
fn a_name_a_folder_cannot_have_is_said_before_create_is_pressed_and_in_plain_words() {
    let _serial = theme::serial();
    for size in [SMALL, LARGE] {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
        shell.state.ask_for_a_new_source("textures");
        shell.settled_sized(size);

        let said = said_by(&shell.settled_sized(size));
        assert!(
            said.iter().all(|t| !t.contains("import files")),
            "Import is for game files: {said:?}"
        );
        shell.draw_sized(size, vec![egui::Event::Text("a/b".into())]);
        let said = said_by(&shell.settled_sized(size));
        assert!(
            said.iter()
                .any(|t| t.contains("A name cannot hold any of these")),
            "the name is refused as it is typed: {said:?}"
        );
        assert!(
            said.iter().all(|t| !t.contains("asset.json")),
            "no file of the source's is named: {said:?}"
        );
        let create = (shell.described.iter())
            .find(|w| w.name.as_deref() == Some("Create"))
            .expect("a Create button");
        assert!(create.disabled, "Create waits for a name a folder can have");

        // A name that can be one lets it through.
        for _ in 0..3 {
            shell.draw_sized(
                size,
                vec![super::frames_of_the_shell::key(egui::Key::Backspace)],
            );
        }
        shell.draw_sized(size, vec![egui::Event::Text("faces".into())]);
        shell.settled_sized(size);
        let create = (shell.described.iter())
            .find(|w| w.name.as_deref() == Some("Create"))
            .expect("a Create button");
        assert!(!create.disabled);
    }
}

#[test]
fn the_question_of_a_file_from_elsewhere_puts_each_consequence_under_its_button_and_enter_takes_the_safe_one(
) {
    use crate::state::workbench::{Question, Wanting};
    let _serial = theme::serial();
    for size in [SMALL, LARGE] {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
        let module = shell.state.wardrobe_module_id().unwrap();
        let garment = (shell.state.wardrobe_pack())
            .and_then(|p| p.read.as_ref().ok())
            .map(|read| read.wardrobe.garments[0].id.clone())
            .unwrap();
        shell.state.workbench.asking = Some(Question::ImportOrReference {
            wanting: Wanting::Model { module, garment },
            path: write(&dir.path().join("elsewhere/hull.ydd"), b"x"),
            whose: None,
        });
        let shapes = shell.settled_sized(size);

        let said = said_by(&shapes);
        assert!(
            said.iter()
                .any(|t| t.starts_with("Use hull.ydd as the model of ") && t.ends_with("?")),
            "{:?}",
            names(&shell)
        );
        for plain in [
            "The original file is not changed",
            "Undo brings that back",
            "it is missing until you Locate it",
        ] {
            assert!(
                said.iter().any(|t| t.contains(plain)),
                "`{plain}` is not under its button: {said:?}"
            );
        }
        for jargon in ["decompiled", "for the platform it states", "by its place"] {
            assert!(
                said.iter().all(|t| !t.contains(jargon)),
                "`{jargon}` is on screen: {said:?}"
            );
        }
        let (_, focused) = shell.focused().expect("a button has the keyboard");
        assert_eq!(
            focused, "Import into the project",
            "the answer that only adds is the one Enter takes"
        );
        shell.draw_sized(
            size,
            vec![super::frames_of_the_shell::key(egui::Key::Escape)],
        );
        shell.settled_sized(size);
        assert!(shell.state.workbench.asking.is_none(), "Escape closes it");
    }
}
