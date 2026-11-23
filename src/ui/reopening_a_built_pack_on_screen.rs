//! A built pack imported as a person meets it: File ▸ Import built pack… and a drop on the Project workspace ask "Import it
//! as a project?", the answer runs on a worker, and the report names what could not be placed and where the file was kept; a
//! garment is packaged and a package added from the Garment panel.
//!
//! ⚠ Read through AccessKit from whole frames of the shell and clicked where the names are; the file dialogs are the
//! stand-ins of a test build, which answer with the path a case names. The packs are built by the test, so this holds that
//! a pack imports as sources, not that the game loads it.

use std::path::{Path, PathBuf};

use super::frames_of_the_shell::Shell;
use super::the_project_screens_on_screen::a_project_shell;
use super::the_wardrobe_operations_on_screen::{
    a_shell_with_two_jackets, click_on, garments, shows, until,
};
use crate::platform::file_dialogs;
use crate::state::reopening_a_built_pack_from_the_app::a_built_pack;
use crate::state::workbench::Question;
use crate::state::ThemeChoice;
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;
use crate::ui::theme;
use or_project::OutputKind;

/// Where the pointer is: well inside the middle panel of the Project workspace, clear of every row of a tree.
fn middle() -> egui::Pos2 {
    egui::pos2(900.0, 400.0)
}

fn painted(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
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

/// The question with its folder changed to `place`, so that no test makes a folder in the person's documents.
fn made_in(shell: &mut Shell, place: &Path) {
    let Some(Question::Reopen { place: shown, .. }) = shell.state.workbench.asking.as_mut() else {
        panic!("no question: {:?}", shell.state.workbench.asking);
    };
    *shown = place.display().to_string();
}

#[test]
fn import_built_pack_is_in_file_and_the_palette_and_asks_what_to_import() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let pack = a_built_pack(dir.path(), OutputKind::SingleplayerDlc);
    let mut shell = Shell::new(ThemeChoice::Dark);

    let command = commands::of(&shell.state, Action::OpenBuiltPack).expect("a command");
    assert_eq!(command.label, "Import built pack…");
    assert_eq!(command.category, "File");
    assert_eq!(
        command.enabled,
        Ok(()),
        "it needs no project: a pack is imported into a new one"
    );

    file_dialogs::answer_next(pack.clone());
    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::OpenBuiltPack);
    assert_eq!(
        file_dialogs::last_dialog().as_deref(),
        Some("Import Built Pack")
    );
    shell.settled();

    for text in [
        "Import dlc.rpf as a new project?",
        "dlc.rpf",
        "Name of the project folder",
        "Folder the project is made in",
        "Browse…",
        "Import as project",
        "Cancel",
    ] {
        assert!(
            shows(&shell, text),
            "the question lacks `{text}`: {:?}",
            super::the_wardrobe_operations_on_screen::names(&shell)
        );
    }
    click_on(&mut shell, "Cancel");
    assert!(shell.state.workbench.asking.is_none(), "Cancel closes it");
    assert!(
        !shell.state.workbench.reopen.is_running(),
        "and reads nothing"
    );
}

#[test]
fn a_dlc_dropped_on_the_project_workspace_asks_runs_and_reports_where_it_kept_what_it_could_not_place(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let pack = a_built_pack(dir.path(), OutputKind::SingleplayerDlc);
    let before = std::fs::read(&pack).unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut shell = a_project_shell(&work, ThemeChoice::Dark);
    let open = shell.state.project_path.clone().expect("a project");
    shell.state.active_workspace = Workspace::Project;
    shell.settled();

    // The hint says what the drop will do before it lets go.
    let hint = painted(&shell.hover_files(middle(), vec![pack.clone()]));
    assert!(hint.iter().any(|t| t == "Import as a project"), "{hint:?}");
    assert!(
        !hint.iter().any(|t| t == "Import into the project"),
        "{hint:?}"
    );
    shell.hover_end();

    shell.drop_files(middle(), vec![pack.clone()]);
    shell.settled();
    assert!(shows(&shell, "as a new project?"));
    assert!(
        !shell.state.workbench.imports.is_running(),
        "an archive is not imported whole: it asks to be imported"
    );
    let made = dir.path().join("made");
    std::fs::create_dir_all(&made).unwrap();
    made_in(&mut shell, &made);
    click_on(&mut shell, "Import as project");
    assert!(shell.state.workbench.reopen.is_running());

    until(&mut shell, "the report", |s| s.state.workbench.reopen.shown);
    shell.settled();
    for text in [
        "Pack imported",
        "Open project",
        "Reveal in the file manager",
        "could not be placed",
        "There is no Undo",
    ] {
        assert!(
            shows(&shell, text),
            "the report lacks `{text}`: {:?}",
            super::the_wardrobe_operations_on_screen::names(&shell)
        );
    }
    // The file the clothing cannot read is named with where it was kept.
    assert!(shows(&shell, "data/handling.meta"));
    assert!(
        shows(&shell, "files/data/handling.meta"),
        "{:?}",
        super::the_wardrobe_operations_on_screen::names(&shell)
    );
    assert_eq!(
        std::fs::read(&pack).unwrap(),
        before,
        "the pack was not written"
    );
    assert_eq!(shell.state.project_path.as_deref(), Some(open.as_path()));

    // Open project asks whether it replaces the open one or opens in a second window.
    click_on(&mut shell, "Open project");
    shell.settled();
    assert!(
        shows(&shell, "Replace ") && shows(&shell, "Open in new window"),
        "one project per window: {:?}",
        super::the_wardrobe_operations_on_screen::names(&shell)
    );
}

#[test]
fn an_oiv_dropped_anywhere_asks_and_cancel_makes_nothing() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let oiv = a_built_pack(dir.path(), OutputKind::OivPackage);
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.settled();

    shell.drop_files(middle(), vec![oiv]);
    shell.settled();
    assert!(shows(&shell, "as a new project?"));
    assert!(
        shows(&shell, "is an OIV package"),
        "{:?}",
        super::the_wardrobe_operations_on_screen::names(&shell)
    );
    click_on(&mut shell, "Cancel");
    assert!(shell.state.workbench.asking.is_none());
    assert!(!shell.state.workbench.reopen.is_running());
    assert!(shell.state.workbench.reopen.report().is_none());
}

/// Turn the wheel over the Garment panel until the button called `name` is inside the window, as a person does when the panel is longer than it.
fn scroll_into_view(shell: &mut Shell, name: &str) {
    for _ in 0..20 {
        let Some(widget) = (shell.described.iter()).find(|w| w.name.as_deref() == Some(name))
        else {
            return;
        };
        if widget.bounds.max.y < 840.0 {
            return;
        }
        let over = egui::pos2(widget.bounds.center().x, 500.0);
        shell.draw(vec![
            egui::Event::PointerMoved(over),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -3.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            },
        ]);
        shell.settled();
    }
}

/// The folder a package was written into, the only one under `to`.
fn package_in(to: &Path) -> PathBuf {
    if to.join("garment.json").is_file() {
        return to.to_path_buf();
    }
    std::fs::read_dir(to)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("garment.json").is_file())
        .unwrap_or_else(|| panic!("no package under {}", to.display()))
}

#[test]
fn a_garment_is_packaged_from_its_panel_and_the_package_is_added_with_one_undo() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    assert!(
        shows(&shell, "Package garment…"),
        "the Garment panel lacks Package garment…: {:?}",
        super::the_wardrobe_operations_on_screen::names(&shell)
    );
    assert!(
        !shows(&shell, "Add package…"),
        "Add package… acts on the pack, so the panel of a garment has none: {:?}",
        super::the_wardrobe_operations_on_screen::names(&shell)
    );
    let before = garments(&shell);
    // The toast of the drop sits over the bottom of the panel until a person puts it away.
    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::ClearToasts);
    shell.settled();

    // Package garment… asks for a folder and writes the package in it; the pack is not changed.
    let out = dir.path().join("packages");
    std::fs::create_dir_all(&out).unwrap();
    file_dialogs::answer_next(out.clone());
    scroll_into_view(&mut shell, "Package garment…");
    click_on(&mut shell, "Package garment…");
    assert_eq!(
        file_dialogs::last_dialog().as_deref(),
        Some("Package Garment Into Folder")
    );
    until(&mut shell, "the package", |_| {
        out.join("garment.json").is_file()
    });
    let package = package_in(&out);
    assert_eq!(
        garments(&shell),
        before,
        "packaging adds nothing to the pack"
    );

    // Add package… takes the garment the folder holds, and Undo package takes it out whole. It is the pack's, so it waits where no garment is selected.
    let module = shell.state.wardrobe_module_id().unwrap();
    shell.state.select_garment(&module, None);
    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::ClearToasts);
    shell.settled();
    file_dialogs::answer_next(package);
    scroll_into_view(&mut shell, "Add package…");
    click_on(&mut shell, "Add package…");
    assert_eq!(
        file_dialogs::last_dialog().as_deref(),
        Some("Add Garment Package")
    );
    until(&mut shell, "the garment", |s| garments(s) == before + 1);
    until(&mut shell, "its step", |s| shows(s, "Undo package"));
    click_on(&mut shell, "Undo package");
    until(&mut shell, "the undo", |s| garments(s) == before);
}

#[test]
fn add_package_is_offered_on_the_pack_when_no_garment_is_selected_and_adds_the_garment() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    let before = garments(&shell);
    let module = shell.state.wardrobe_module_id().unwrap();
    let id = shell
        .state
        .selected_garment()
        .map(|(pack, at)| pack.read.as_ref().unwrap().wardrobe.garments[at].id.clone());
    let out = dir.path().join("packages");
    std::fs::create_dir_all(&out).unwrap();
    shell
        .state
        .package_garment(id.expect("a garment"), out.clone());
    until(&mut shell, "the package", |_| {
        out.join("garment.json").is_file()
    });

    // ⚠ A package adds a garment to a pack, so nothing needs to be selected to add one.
    shell.state.select_garment(&module, None);
    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::ClearToasts);
    shell.settled();
    assert!(
        shows(&shell, "Add package…"),
        "the pack lacks Add package…: {:?}",
        super::the_wardrobe_operations_on_screen::names(&shell)
    );
    assert!(
        !shows(&shell, "Package garment…"),
        "with no garment selected there is none to package"
    );
    file_dialogs::answer_next(package_in(&out));
    scroll_into_view(&mut shell, "Add package…");
    click_on(&mut shell, "Add package…");
    assert_eq!(
        file_dialogs::last_dialog().as_deref(),
        Some("Add Garment Package")
    );
    until(&mut shell, "the garment", |s| garments(s) == before + 1);
}
