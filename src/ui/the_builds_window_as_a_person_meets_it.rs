//! The Builds window as it is drawn: the matrix is there only when there is something to choose between, a cell
//! that a kind of build cannot ship is locked and says why, and the controls change the project's builds.

use egui::accesskit::Role;
use or_project::OutputKind;

use super::frames_of_the_shell::{key, Shell};
use super::the_project_screens_on_screen::{a_project_shell, names, shows, until};
use crate::state::workbench::Question;
use crate::state::ThemeChoice;
use crate::ui::commands::{self, Action};
use crate::ui::theme;

const MATRIX: &str = "Content by builds";

fn open_the_window(shell: &mut Shell) {
    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::EditBuild);
    shell.settled();
}

/// Where the widget called `name` is, whole or the start of its name.
fn bounds_of(shell: &Shell, name: &str) -> Option<egui::Rect> {
    (shell.described.iter())
        .find(|w| w.name.as_deref().is_some_and(|n| n.starts_with(name)))
        .map(|w| w.bounds)
}

/// Turn the wheel over the window until the widget called `name` is above the Done button, as a person does when the form is longer than the window.
fn scroll_to(shell: &mut Shell, name: &str) {
    for _ in 0..12 {
        let (Some(widget), Some(done)) = (bounds_of(shell, name), bounds_of(shell, "Done")) else {
            return;
        };
        if widget.max.y < done.min.y - 4.0 {
            return;
        }
        let over = egui::pos2(widget.center().x, widget.min.y - 200.0);
        shell.draw(vec![
            egui::Event::PointerMoved(over),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -2.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            },
        ]);
        shell.settled();
    }
}

fn click_on(shell: &mut Shell, name: &str) {
    scroll_to(shell, name);
    // ⚠ A field and the label beside it can say the same, and the order they are described in is the order of their ids: the field is the one to click.
    let widget = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name) && w.role != Role::Label)
        .or_else(|| (shell.described.iter()).find(|w| w.name.as_deref() == Some(name)))
        .or_else(|| {
            (shell.described.iter())
                .find(|w| w.name.as_deref().is_some_and(|n| n.starts_with(name)))
        })
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)));
    let at = widget.bounds.center();
    shell.click(at);
    shell.settled();
}

/// Open the section that holds the words and the options of the build, which starts closed.
fn open_the_options(shell: &mut Shell) {
    click_on(shell, "Author, version and options");
}

fn build_named(shell: &Shell, name: &str) -> or_project::Output {
    (shell.state.project_carried.container.outputs.iter())
        .find(|o| o.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("no build named {name}"))
}

#[test]
fn the_matrix_appears_only_from_two_contents_or_two_builds() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let container = &mut shell.state.project_carried.container;
    assert!(
        container.modules.len() >= 2,
        "the project holds files and clothing"
    );

    // One content and one build: nothing to choose between.
    container
        .modules
        .retain(|m| m.kind == or_project::ModuleKind::Files);
    container.outputs[0].take.retain(|id| id == "files");
    container.outputs.truncate(1);
    let only = container.outputs[0].name.clone();
    open_the_window(&mut shell);
    assert!(
        shows(&shell, "Resource name of the build"),
        "{:?}",
        names(&shell)
    );
    assert!(!shows(&shell, MATRIX), "{:?}", names(&shell));

    // A second build is something to choose for.
    shell
        .state
        .add_build("police_dlc", OutputKind::SingleplayerDlc, None)
        .unwrap();
    shell.settled();
    assert!(shows(&shell, MATRIX), "{:?}", names(&shell));
    assert!(
        shows(&shell, &format!("Files in {only}")),
        "{:?}",
        names(&shell)
    );
}

#[test]
fn a_cell_a_kind_of_build_cannot_ship_is_locked_and_says_why() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let dlc = shell
        .state
        .add_build("tshirts_dlc", OutputKind::SingleplayerDlc, None)
        .unwrap();
    until(&mut shell, "the lock of the script", |s| {
        s.state
            .workbench
            .cannot_ship("files", &OutputKind::SingleplayerDlc)
            .is_some()
    });
    open_the_window(&mut shell);
    assert!(shows(&shell, MATRIX));

    // Taken, the cell is still there to untick: the build would be refused for it.
    let cell = (shell.described.iter())
        .find(|w| {
            w.role == Role::CheckBox
                && w.name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("Files in tshirts_dlc, locked:"))
        })
        .unwrap_or_else(|| panic!("no locked cell: {:?}", names(&shell)));
    assert!(!cell.disabled, "a ticked cell can be unticked");
    let spoken = cell.name.clone().unwrap();
    assert!(
        spoken.contains("scripts/client.lua") && spoken.contains("File types"),
        "the reason names a file the game is told no type for: {spoken}"
    );
    assert!(
        shows(&shell, "would be refused"),
        "the reason is on the screen and not only in a hover: {:?}",
        names(&shell)
    );
    click_on(&mut shell, "Files in tshirts_dlc, locked:");
    assert!(!build_named(&shell, "tshirts_dlc")
        .take
        .contains(&"files".to_string()));

    // Left out, it cannot be taken again.
    let cell = (shell.described.iter())
        .find(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Files in tshirts_dlc, locked:"))
        })
        .expect("the cell is still drawn");
    assert!(
        cell.disabled,
        "a locked cell that is not ticked cannot be ticked"
    );
    shell.click(cell.bounds.center());
    shell.settled();
    assert!(!build_named(&shell, "tshirts_dlc")
        .take
        .contains(&"files".to_string()));
    // The other kind of build ships it.
    assert!(build_named(&shell, "tshirts")
        .take
        .contains(&"files".to_string()));
    let _ = dlc;
}

#[test]
fn the_controls_of_the_form_change_the_build_and_remove_takes_it_out() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    open_the_window(&mut shell);
    let build = "tshirts";

    click_on(
        &mut shell,
        "Divide into several resources when over a ceiling",
    );
    assert!(build_named(&shell, build).split);
    click_on(&mut shell, "Limits · none set");
    click_on(&mut shell, "Files at most");
    let ceiling = build_named(&shell, build).max_files;
    assert!(ceiling.is_some(), "turning a ceiling on sets it");
    click_on(&mut shell, "Size at most");
    assert!(build_named(&shell, build).max_bytes.is_some());
    click_on(&mut shell, "Files at most");
    assert_eq!(
        build_named(&shell, build).max_files,
        None,
        "and off takes it away"
    );
    assert!(shell.state.project_carried.changed, "Save project keeps it");

    click_on(&mut shell, "Remove build");
    assert!(
        shell.state.project_carried.container.outputs.is_empty(),
        "the build is no longer declared"
    );
    assert!(
        shows(&shell, "The project declares no build yet"),
        "{:?}",
        names(&shell)
    );
}

#[test]
fn dragging_a_ceiling_says_one_line_in_the_history_when_the_drag_stops() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    open_the_window(&mut shell);
    click_on(&mut shell, "Limits · none set");
    click_on(&mut shell, "Files at most");
    let start = build_named(&shell, "tshirts").max_files.expect("on");
    let said = shell.state.workbench.history().len();

    let grip = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Files ceiling"))
        .unwrap_or_else(|| panic!("no ceiling to drag: {:?}", names(&shell)))
        .bounds
        .center();
    let button = |at, pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw(vec![egui::Event::PointerMoved(grip), button(grip, true)]);
    let mut at = grip;
    for _ in 0..20 {
        at += egui::vec2(3.0, 0.0);
        shell.draw(vec![egui::Event::PointerMoved(at)]);
    }
    let dragged_to = build_named(&shell, "tshirts").max_files.expect("on");
    assert!(dragged_to > start, "the drag moves the ceiling: {start}");
    assert_eq!(
        shell.state.workbench.history().len(),
        said,
        "nothing is said while the value is still moving"
    );

    shell.draw(vec![button(at, false)]);
    shell.settled();
    let history = shell.state.workbench.history();
    let lines: Vec<&str> = history[said..].iter().map(|e| e.text.as_str()).collect();
    assert_eq!(
        lines,
        [format!("The build tshirts allows {dragged_to} files")],
        "one line, with the value the drag ended on"
    );
}

#[test]
fn done_and_escape_close_the_window_and_add_a_build_names_one() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    open_the_window(&mut shell);
    assert!(matches!(
        shell.state.workbench.asking,
        Some(Question::Builds { adding: None, .. })
    ));
    shell.draw(vec![key(egui::Key::Escape)]);
    shell.settled();
    assert!(shell.state.workbench.asking.is_none(), "Escape closes it");

    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::AddBuild);
    shell.settled();
    assert!(
        shows(&shell, "New build takes"),
        "the new build chooses its content: {:?}",
        names(&shell)
    );
    click_on(&mut shell, "Add build");
    let names_now: Vec<String> = (shell.state.project_carried.container.outputs.iter())
        .map(|o| o.name.clone())
        .collect();
    assert_eq!(names_now.len(), 2, "{names_now:?}");
    assert!(
        !names_now[1].ends_with("_2"),
        "the second build is not named as a part of a split: {names_now:?}"
    );
    click_on(&mut shell, "Done");
    assert!(shell.state.workbench.asking.is_none());
}

#[test]
fn the_clothing_options_are_offered_for_a_build_that_ships_a_pack_and_each_is_one_edit() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    open_the_window(&mut shell);
    assert!(
        !shows(&shell, "Creature metadata"),
        "the section starts closed so the window stays short"
    );
    open_the_options(&mut shell);
    let build = "tshirts";
    for label in [
        "A folder for each slot",
        "Creature metadata",
        "Alternate models",
        "Collection prefix",
        "Clothes dump of the build",
    ] {
        assert!(shows(&shell, label), "{label} in {:?}", names(&shell));
    }
    assert!(
        build_named(&shell, build).extra.get("clothing").is_none(),
        "a build that states none holds no key"
    );

    click_on(&mut shell, "A folder for each slot");
    click_on(&mut shell, "Creature metadata");
    let stated = build_named(&shell, build).extra["clothing"].clone();
    assert_eq!(
        stated,
        serde_json::json!({ "subfolders": false, "creature_metadata": false }),
        "only what differs from the default is kept"
    );
    assert!(shell.state.project_carried.changed, "Save project keeps it");

    // Back to the default is no key at all.
    click_on(&mut shell, "A folder for each slot");
    click_on(&mut shell, "Creature metadata");
    assert!(build_named(&shell, build).extra.get("clothing").is_none());
}

#[test]
fn a_pack_is_laid_out_as_the_game_lays_it_so_its_folder_option_and_its_dump_are_not_offered() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell
        .state
        .add_build("tshirts_dlc", OutputKind::SingleplayerDlc, None)
        .unwrap();
    shell.state.ask_about_builds(false);
    shell.state.workbench.asking = Some(Question::Builds {
        chosen: shell
            .state
            .project_carried
            .container
            .outputs
            .iter()
            .find(|o| o.name == "tshirts_dlc")
            .map(|o| o.id.clone()),
        adding: None,
    });
    shell.settled();
    click_on(&mut shell, "Options");

    let folder = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("A folder for each slot"))
        .expect("the option is listed");
    assert!(folder.disabled, "a pack is flat inside its archives");
    let creature = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Creature metadata"))
        .expect("the option is listed");
    assert!(!creature.disabled);
    // A pack has no place for an author: nothing is asked for one.
    assert!(!shows(&shell, "Author"), "{:?}", names(&shell));
}

#[test]
fn the_author_the_version_and_the_description_are_kept_as_they_are_entered_and_a_bad_version_is_said(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    open_the_window(&mut shell);
    open_the_options(&mut shell);
    let build = "tshirts";

    let type_into = |shell: &mut Shell, field: &str, text: &str| {
        click_on(shell, field);
        shell.draw(vec![egui::Event::Text(text.to_string())]);
        shell.draw(vec![key(egui::Key::Tab)]);
        shell.settled();
    };
    type_into(&mut shell, "Author", "Ann O'Neil");
    type_into(&mut shell, "Description", "Uniforms");
    let output = build_named(&shell, build);
    assert_eq!(output.extra["author"], "Ann O'Neil");
    assert_eq!(output.extra["description"], "Uniforms");
    assert!(shell.state.project_carried.changed);

    // A package states major.minor; anything else is said and not kept.
    shell
        .state
        .edit_build(&output.id, "A package".to_string(), |o| {
            o.kind = OutputKind::OivPackage
        })
        .unwrap();
    shell.settled();
    type_into(&mut shell, "Version", "one point two");
    assert!(shows(&shell, "is not major.minor"), "{:?}", names(&shell));
    assert!(build_named(&shell, build).extra.get("version").is_none());
}

#[test]
fn a_clothing_content_is_locked_in_no_kind_of_build_because_it_is_laid_out_for_each() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell
        .state
        .add_build("tshirts_dlc", OutputKind::SingleplayerDlc, None)
        .unwrap();
    until(&mut shell, "the lock of the script", |s| {
        s.state
            .workbench
            .cannot_ship("files", &OutputKind::SingleplayerDlc)
            .is_some()
    });
    let clothing = (shell.state.project_carried.container.modules.iter())
        .find(|m| m.kind == or_project::ModuleKind::Clothing)
        .map(|m| m.id.clone())
        .expect("the project holds clothing");
    for kind in [
        OutputKind::SingleplayerDlc,
        OutputKind::OivPackage,
        OutputKind::FivemResource,
    ] {
        assert_eq!(
            shell.state.workbench.cannot_ship(&clothing, &kind),
            None,
            "{kind:?}"
        );
    }
}
