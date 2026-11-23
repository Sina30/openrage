//! A file dropped from Explorer is put in the project on the Project workspace, added as clothes on the
//! Wardrobe, and opened anywhere else, and the hint under the pointer says which while the drag is held.
//!
//! ⚠ Drawn through the whole shell and a real drop in a frame's input, the way egui-winit delivers one,
//! over a clothing project: what is read is the project's folder, the documents and the hint's text.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::epaint::ClippedShape;

use super::frames_of_the_shell::Shell;
use super::the_project_screens_on_screen::a_project_shell;
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;

/// Where the pointer is: well inside the middle panel of the Project and the Browse workspace, clear of every row of a tree.
fn middle() -> egui::Pos2 {
    egui::pos2(900.0, 400.0)
}

fn write(path: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    path.to_path_buf()
}

fn painted(shapes: &[ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

fn until(shell: &mut Shell, what: &str, done: impl Fn(&Shell) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        shell.state.poll_workbench();
        shell.state.poll_load_result();
        shell.draw(Vec::new());
        if done(shell) {
            return;
        }
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn opened(shell: &Shell, file: &Path) -> bool {
    (shell.state.documents.iter()).any(|d| d.path.as_deref() == Some(file))
}

#[test]
fn a_drop_on_the_project_workspace_imports_and_the_same_drop_on_the_explorer_opens() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let project = shell.state.project_path.clone().expect("a project");
    let folder = or_project::project_folder(&project);
    let hull = write(&dir.path().join("downloads/hull.lua"), b"print('hull')");
    let lamp = write(&dir.path().join("downloads/lamp.lua"), b"print('lamp')");

    shell.state.active_workspace = Workspace::Project;
    shell.settled();
    shell.drop_files(middle(), vec![hull.clone()]);
    until(&mut shell, "the import", |s| {
        s.state.workbench.imports.report().is_some() && !s.state.workbench.imports.is_running()
    });
    assert!(
        folder.join("files/hull.lua").is_file(),
        "a drop on the Project workspace is put in the project: {:?}",
        shell.state.workbench.imports.report()
    );
    assert!(!opened(&shell, &hull), "and not opened");

    // The same drop at the same point of the explorer's screen.
    shell.state.active_workspace = Workspace::Browse;
    shell.settled();
    shell.drop_files(middle(), vec![lamp.clone()]);
    until(&mut shell, "the file to open", |s| opened(s, &lamp));
    assert!(
        !folder.join("files/lamp.lua").exists(),
        "a drop anywhere else opens the file and puts nothing in the project"
    );
}

#[test]
fn the_hint_under_a_drag_says_import_over_the_project_open_over_the_explorer_and_add_over_clothes()
{
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let held = vec![write(
        &dir.path().join("downloads/hull.lua"),
        b"print('hull')",
    )];
    let has = |text: &[String], wanted: &str| text.iter().any(|t| t == wanted);

    shell.state.active_workspace = Workspace::Project;
    shell.settled();
    let over_the_project = painted(&shell.hover_files(middle(), held.clone()));
    assert!(
        has(&over_the_project, "Import into the project"),
        "{over_the_project:?}"
    );
    assert!(!has(&over_the_project, "Open in OpenRage"));

    shell.state.active_workspace = Workspace::Browse;
    shell.settled();
    let over_the_explorer = painted(&shell.hover_files(middle(), held.clone()));
    assert!(
        has(&over_the_explorer, "Open in OpenRage"),
        "{over_the_explorer:?}"
    );
    assert!(!has(&over_the_explorer, "Import into the project"));

    shell.state.active_workspace = Workspace::Wardrobe;
    shell.settled();
    let drop_area = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("a drop area");
    let over_the_wardrobe = painted(&shell.hover_files(drop_area.center(), held));
    assert!(
        has(&over_the_wardrobe, "Add as clothes"),
        "{over_the_wardrobe:?}"
    );
    shell.hover_end();
}

#[test]
fn a_drop_over_the_start_page_raised_on_the_project_workspace_opens_and_does_not_import() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let folder = or_project::project_folder(&shell.state.project_path.clone().expect("a project"));
    let held = write(&dir.path().join("downloads/hull.lua"), b"print('hull')");
    shell.state.active_workspace = Workspace::Project;
    shell.state.show_start_page();
    shell.settled();

    let over_the_page = painted(&shell.hover_files(middle(), vec![held.clone()]));
    assert!(
        over_the_page.iter().any(|t| t == "Open in OpenRage"),
        "{over_the_page:?}"
    );
    shell.drop_files(middle(), vec![held.clone()]);
    until(&mut shell, "the file to open", |s| opened(s, &held));
    assert!(!folder.join("files/hull.lua").exists());
}

/// A project `name` made from `template` in `dir`, and its file.
fn a_project_file(dir: &Path, template: crate::state::front_door::Template, name: &str) -> PathBuf {
    let mut draft = crate::state::front_door::NewProject::new(template, dir.to_path_buf());
    draft.set_name(name.to_string());
    draft.create().expect("a project").file
}

fn shows(shell: &Shell, text: &str) -> bool {
    shell
        .described
        .iter()
        .any(|w| w.name.as_deref().is_some_and(|n| n.contains(text)))
}

#[test]
fn a_folder_that_holds_a_project_dropped_while_another_is_open_asks_here_or_in_a_new_window() {
    use crate::state::front_door::Template;
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let open = shell.state.project_path.clone().expect("a project");
    std::fs::create_dir_all(dir.path().join("elsewhere")).unwrap();
    let other = a_project_file(&dir.path().join("elsewhere"), Template::Files, "other");
    let folder = other.parent().unwrap().to_path_buf();

    shell.state.active_workspace = Workspace::Project;
    shell.settled();
    shell.drop_files(middle(), vec![folder.clone()]);
    shell.settled();

    assert_eq!(
        shell.state.front_door.switching.as_deref(),
        Some(other.as_path()),
        "the project in the folder is what is asked about"
    );
    assert!(shows(&shell, "Replace ") && shows(&shell, "Open in new window"));
    assert_eq!(
        shell.state.project_path.as_deref(),
        Some(open.as_path()),
        "nothing was opened yet"
    );
    assert!(
        shell
            .state
            .documents
            .iter()
            .all(|d| d.path.as_deref() != Some(folder.as_path())),
        "and the folder is not opened as a place to look in"
    );

    // The project that is open, dropped again as a folder, is already open and asks nothing.
    shell.state.front_door.switching = None;
    shell.drop_files(middle(), vec![open.parent().unwrap().to_path_buf()]);
    shell.settled();
    assert!(shell.state.front_door.switching.is_none());
}

#[test]
fn a_lone_texture_dictionary_dropped_on_a_project_with_no_clothing_still_asks_what_it_is() {
    use crate::state::front_door::Template;
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_file(dir.path(), Template::Files, "bare");
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Project;
    shell.settled();
    assert!(!shell.state.has_clothing());
    let ytd = write(
        &dir.path().join("downloads/jbib_diff_000_a_uni.ytd"),
        b"texture",
    );

    shell.drop_files(middle(), vec![ytd.clone()]);
    shell.settled();

    for answer in ["Make clothing", "Import as sources", "Cancel"] {
        assert!(
            shows(&shell, answer),
            "no `{answer}`: {:?}",
            shell
                .described
                .iter()
                .filter_map(|w| w.name.clone())
                .collect::<Vec<_>>()
        );
    }
}
