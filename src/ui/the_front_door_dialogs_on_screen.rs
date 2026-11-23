//! The front door at the windows people use: the start page's chips and recent cards, and the New project dialog, drawn at 1600×900,
//! 1100×800 and 700×600 and read through AccessKit.

use std::path::{Path, PathBuf};

use egui::epaint::{ClippedShape, Shape};

use super::frames_of_the_shell::{key, Shell};
use super::the_front_door_on_screen::{
    a_first_launch, a_project, click_on, names, shows, waits_for,
};
use crate::state::front_door::{NewProject, Step, Template};
use crate::ui::dock::Workspace;
use crate::ui::theme;

/// A wide window, a laptop and a narrow one, in points.
const WINDOWS: [(u32, u32); 3] = [(1600, 900), (1100, 800), (700, 600)];

fn dialog_id() -> egui::Id {
    egui::Id::new(("question", "New project"))
}

/// Where the New project dialog is on screen.
fn dialog_rect(shell: &Shell) -> Option<egui::Rect> {
    shell.ctx.memory(|m| m.area_rect(dialog_id()))
}

/// Frames in a window of `size`, enough for a modal to size itself and hold.
fn settle(shell: &mut Shell, size: (u32, u32)) {
    for _ in 0..12 {
        shell.draw_sized(size, Vec::new());
    }
}

/// The widget AccessKit called `name`, inside the dialog while it is on screen: the screen behind it has a Back button of its own.
fn widget<'a>(shell: &'a Shell, name: &str) -> &'a crate::ui::frames_of_the_shell::Described {
    let inside = dialog_rect(shell).map(|rect| rect.expand(1.0));
    (shell.described.iter())
        .find(|w| {
            w.name.as_deref() == Some(name)
                && inside.is_none_or(|rect| rect.contains_rect(w.bounds))
        })
        .unwrap_or_else(|| panic!("nothing is called `{name}`: {:?}", names(shell)))
}

/// Click the widget called `name` inside the dialog, in a window of `size`.
fn click_in_dialog(shell: &mut Shell, size: (u32, u32), name: &str) {
    let at = widget(shell, name).bounds.center();
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw_sized(size, vec![egui::Event::PointerMoved(at), button(true)]);
    shell.draw_sized(size, vec![button(false)]);
    settle(shell, size);
}

/// The names of what is drawn inside the dialog, and not the screen behind it that happens to lie under its rectangle.
fn in_the_dialog(shell: &Shell, background: &[String]) -> Vec<String> {
    let rect = dialog_rect(shell)
        .expect("the dialog is on screen")
        .expand(1.0);
    (shell.described.iter())
        .filter(|w| rect.contains_rect(w.bounds))
        .filter_map(|w| w.name.clone())
        .filter(|name| !background.contains(name))
        .collect()
}

fn open_dialog(shell: &mut Shell, template: Template, step: Step, folder: &Path) {
    let mut draft = if template == Template::Adopt {
        NewProject::adopting(folder)
    } else {
        let mut draft = NewProject::new(template, folder.to_path_buf());
        draft.set_name("police".into());
        draft
    };
    draft.step = step;
    shell.state.front_door.new_project = Some(draft);
}

const EVERY_STEP: [(Template, Step); 8] = [
    (Template::Empty, Step::Gallery),
    (Template::Files, Step::Gallery),
    (Template::Clothing, Step::Gallery),
    (Template::Adopt, Step::Gallery),
    (Template::Empty, Step::Details),
    (Template::Files, Step::Details),
    (Template::Clothing, Step::Details),
    (Template::Adopt, Step::Details),
];

#[test]
fn escape_cancels_new_project_at_every_step_even_with_the_name_field_focused() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    for (template, step) in EVERY_STEP {
        let mut shell = a_first_launch();
        open_dialog(&mut shell, template, step, dir.path());
        shell.settled();
        if step == Step::Details {
            assert!(
                shell.ctx.memory(|m| m.focused()).is_some(),
                "{template:?}: the name field should have the keyboard"
            );
        }
        shell.draw(vec![key(egui::Key::Escape)]);
        assert!(
            shell.state.front_door.new_project.is_none(),
            "Escape left New project open on {template:?}, {step:?}"
        );
    }
}

#[test]
fn escape_cancels_new_project_whichever_field_has_the_keyboard_and_after_typing_in_it() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    for (step, field) in [
        (Step::Gallery, "Search templates"),
        (Step::Details, "Project name"),
        (Step::Details, "Folder"),
        (Step::Details, "Resource name"),
    ] {
        let mut shell = a_first_launch();
        open_dialog(&mut shell, Template::Files, step, dir.path());
        settle(&mut shell, WINDOWS[0]);
        click_in_dialog(&mut shell, WINDOWS[0], field);
        shell.draw(vec![egui::Event::Text("x".into())]);
        shell.settled();
        assert!(
            shell.ctx.memory(|m| m.focused()).is_some(),
            "{field}: the field should have the keyboard"
        );
        // As a person presses it, and as a script does: the press in one frame and the release in the next.
        let escape = |pressed| egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        shell.draw(vec![escape(true)]);
        shell.draw(vec![escape(false)]);
        assert!(
            shell.state.front_door.new_project.is_none(),
            "Escape left New project open with the keyboard in `{field}`"
        );
    }
}

#[test]
fn the_dialog_is_one_size_at_every_step_and_in_every_window() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    for size in WINDOWS {
        let window =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32));
        let mut first: Option<(egui::Rect, Template, Step)> = None;
        for (template, step) in EVERY_STEP {
            let mut shell = a_first_launch();
            open_dialog(&mut shell, template, step, dir.path());
            settle(&mut shell, size);
            let rect = dialog_rect(&shell).expect("the dialog is on screen");
            assert!(
                window.contains_rect(rect),
                "{size:?}: the dialog of {template:?} {step:?} leaves the window: {rect:?}"
            );
            match first {
                None => first = Some((rect, template, step)),
                Some((was, with, at)) => assert!(
                    (rect.min - was.min).abs().max_elem() < 0.5
                        && (rect.size() - was.size()).abs().max_elem() < 0.5,
                    "{size:?}: {template:?} {step:?} is {rect:?} and {with:?} {at:?} is {was:?}"
                ),
            }
        }
    }

    // One dialog going to and fro keeps its size as well.
    let mut shell = a_first_launch();
    open_dialog(&mut shell, Template::Files, Step::Gallery, dir.path());
    settle(&mut shell, WINDOWS[1]);
    let gallery = dialog_rect(&shell).expect("the dialog is on screen");
    click_in_dialog(&mut shell, WINDOWS[1], "Next");
    assert_eq!(
        shell.state.front_door.new_project.as_ref().map(|d| d.step),
        Some(Step::Details)
    );
    settle(&mut shell, WINDOWS[1]);
    assert_eq!(
        dialog_rect(&shell),
        Some(gallery),
        "Next resized the dialog"
    );
    click_in_dialog(&mut shell, WINDOWS[1], "Back");
    settle(&mut shell, WINDOWS[1]);
    assert_eq!(
        dialog_rect(&shell),
        Some(gallery),
        "Back resized the dialog"
    );
}

/// Every rectangle `shapes` paint with `fill`, nested shapes included.
fn filled_with(shapes: &[ClippedShape], fill: egui::Color32) -> Vec<egui::Rect> {
    fn walk(shape: &Shape, fill: egui::Color32, out: &mut Vec<egui::Rect>) {
        match shape {
            Shape::Rect(rect) if rect.fill == fill => out.push(rect.rect),
            Shape::Vec(inner) => inner.iter().for_each(|shape| walk(shape, fill, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes
        .iter()
        .for_each(|clipped| walk(&clipped.shape, fill, &mut out));
    out
}

/// The size of the type `shapes` paint the text containing `needle` in.
fn type_size_of(shapes: &[ClippedShape], needle: &str) -> Option<f32> {
    fn walk(shape: &Shape, needle: &str) -> Option<f32> {
        match shape {
            Shape::Text(text) if text.galley.job.text.contains(needle) => text
                .galley
                .job
                .sections
                .first()
                .map(|section| section.format.font_id.size),
            Shape::Vec(inner) => inner.iter().find_map(|shape| walk(shape, needle)),
            _ => None,
        }
    }
    shapes
        .iter()
        .find_map(|clipped| walk(&clipped.shape, needle))
}

#[test]
fn the_buttons_of_the_dialog_sit_on_the_right_and_the_one_that_goes_on_is_filled() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    for (step, primary, others) in [
        (Step::Gallery, "Next", &["Cancel"][..]),
        (Step::Details, "Create", &["Back", "Cancel"][..]),
    ] {
        let mut shell = a_first_launch();
        open_dialog(&mut shell, Template::Files, step, dir.path());
        settle(&mut shell, WINDOWS[0]);
        let shapes = shell.draw_sized(WINDOWS[0], Vec::new());
        let dialog = dialog_rect(&shell).expect("the dialog is on screen");

        let main = widget(&shell, primary).bounds;
        for other in others {
            let other = widget(&shell, other).bounds;
            assert!(
                main.min.x >= other.max.x - 0.5,
                "{step:?}: {primary} should be the last button along the row: {main:?} and {other:?}"
            );
            assert!(
                (main.center().y - other.center().y).abs() < 1.0,
                "{step:?}: {primary} and {other:?} are not on one row: {main:?}"
            );
        }
        assert!(
            dialog.max.x - main.max.x < 40.0,
            "{step:?}: {primary} should end where the dialog does: {main:?} in {dialog:?}"
        );
        let accent = filled_with(&shapes, theme::ACCENT.get());
        assert!(
            accent.iter().any(|fill| fill.contains(main.center())),
            "{step:?}: {primary} is not filled with the accent colour"
        );
        for other in others {
            let other = widget(&shell, other).bounds;
            assert!(
                !accent.iter().any(|fill| fill.contains(other.center())),
                "{step:?}: {other:?} is filled as the primary button is"
            );
        }
    }
}

#[test]
fn the_chips_stand_on_the_line_of_new_project_at_the_height_of_its_button() {
    for size in WINDOWS {
        let mut shell = a_first_launch();
        settle(&mut shell, size);
        // The glyph of a button is the start of its name.
        let new = (shell.described.iter())
            .find(|w| {
                w.name
                    .as_deref()
                    .is_some_and(|n| n.ends_with("New project…"))
            })
            .unwrap_or_else(|| panic!("no New project…: {:?}", names(&shell)))
            .bounds;
        for chip in ["Files project", "Clothing pack", "Weapon pack"] {
            let bounds = widget(&shell, chip).bounds;
            assert!(
                (bounds.min.y - new.min.y).abs() < 0.5 && (bounds.max.y - new.max.y).abs() < 0.5,
                "{size:?}: `{chip}` is not on the line of New project…: {bounds:?} beside {new:?}"
            );
        }
    }
}

#[test]
fn a_recent_cards_path_never_overprints_its_age_or_its_buttons() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let deep = dir
        .path()
        .join("a_rather_long_folder_name")
        .join("with_another_one_inside_it")
        .join("and_a_third_to_make_the_path_longer");
    std::fs::create_dir_all(&deep).expect("folders");
    let file = a_project(&deep, "police_pack", Template::Files);
    let sheriff = a_project(&deep, "sheriff", Template::Files);
    let moved = deep.join("gone").join("moved.openrage-proj");

    let mut shell = a_first_launch();
    shell.state.prefs.recent_projects = vec![file.clone(), sheriff, moved.clone()];
    shell.state.prefs.touch_recent_project(file.clone());
    shell.state.front_door.cards.refresh();
    waits_for(&mut shell, "Not built");
    for size in WINDOWS {
        settle(&mut shell, size);
        for path in [&file, &moved] {
            let shown = path.display().to_string();
            // ⚠ The path is cut in the middle, so its label is found by being a path that ends as a project file does: the cards are in the order of the list, the first project's and the third's.
            let mut labels: Vec<_> = (shell.described.iter())
                .filter(|w| {
                    w.role == egui::accesskit::Role::Label
                        && w.name
                            .as_deref()
                            .is_some_and(|n| n.contains(":\\") && n.ends_with("proj"))
                })
                .collect();
            labels.sort_by(|a, b| a.bounds.min.y.total_cmp(&b.bounds.min.y));
            let path_label = labels[if path == &file { 0 } else { 2 }].bounds;
            let right_hand: Vec<_> = (shell.described.iter())
                .filter(|w| {
                    w.name.as_deref().is_some_and(|name| {
                        name.starts_with("opened ")
                            || name.contains("new window")
                            || name == "Open"
                            || name == "Remove from list"
                            || name == "Locate…"
                    })
                })
                // Beside the path in a wide window and under it in a narrow one.
                .filter(|w| (w.bounds.center().y - path_label.center().y).abs() < 70.0)
                .collect();
            assert!(!right_hand.is_empty(), "{size:?}: nothing beside {shown}");
            for other in right_hand {
                assert!(
                    !path_label.intersects(other.bounds),
                    "{size:?}: the path {path_label:?} runs into {:?} at {:?}",
                    other.name,
                    other.bounds
                );
            }
        }
    }
}

#[test]
fn the_clothing_path_says_clothing_pack_and_no_project_content_or_build() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_first_launch();
    let background = names(&shell);
    click_on(&mut shell, "Clothing pack");
    settle(&mut shell, WINDOWS[0]);
    let mut checked = 0;
    for (label, mutate) in [
        ("nothing typed", 0),
        ("a name typed", 1),
        ("a name that is not one", 2),
        ("a folder that is not there", 3),
    ] {
        let draft = shell
            .state
            .front_door
            .new_project
            .as_mut()
            .expect("the dialog is open");
        draft.place = dir.path().display().to_string();
        match mutate {
            0 => draft.set_name(String::new()),
            1 => draft.set_name("harbor police".into()),
            2 => draft.set_name("a/b".into()),
            _ => {
                draft.set_name("harbor".into());
                draft.place = dir.path().join("nowhere").display().to_string();
            }
        }
        settle(&mut shell, WINDOWS[0]);
        let said = in_the_dialog(&shell, &background);
        assert!(
            said.iter()
                .any(|name| name.to_lowercase().contains("clothing pack")),
            "{label}: the dialog never says clothing pack: {said:?}"
        );
        for word in ["project", "content", "build"] {
            let found: Vec<_> = said
                .iter()
                .filter(|name| name.to_lowercase().contains(word))
                .collect();
            assert!(
                found.is_empty(),
                "{label}: the clothing path says `{word}`: {found:?}"
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn a_chip_opens_the_dialog_on_its_details_and_back_reaches_the_gallery() {
    for (action, template) in [
        ("Files project", Template::Files),
        ("Clothing pack", Template::Clothing),
        ("Project from a folder", Template::Adopt),
    ] {
        let mut shell = a_first_launch();
        click_on(&mut shell, action);
        let draft = shell
            .state
            .front_door
            .new_project
            .as_ref()
            .expect("the dialog is open");
        assert_eq!(draft.template, template);
        assert_eq!(
            draft.step,
            Step::Details,
            "{action} asked which template it is"
        );
        // The dialog names the project after its template, so Create does not wait for a name; a folder to adopt names its own.
        assert_eq!(
            draft.name.is_empty(),
            template == Template::Adopt,
            "{action}: the dialog opened with the name `{}`",
            draft.name
        );
        // What is typed next goes to the name.
        let id = shell.ctx.memory(|m| m.focused());
        assert!(id.is_some(), "{action}: nothing has the keyboard");
        shell.draw(vec![egui::Event::Text("harbor".into())]);
        shell.settled();
        let draft = shell.state.front_door.new_project.as_ref().expect("open");
        assert_eq!(
            draft.name, "harbor",
            "{action}: what was typed did not reach the name"
        );
    }
    // The gallery stays one click away.
    let mut shell = a_first_launch();
    click_on(&mut shell, "Clothing pack");
    click_in_dialog(&mut shell, WINDOWS[0], "Back");
    assert!(
        shows(&shell, "Search templates"),
        "Back did not reach the gallery: {:?}",
        names(&shell)
    );
}

#[test]
fn a_message_stands_under_the_field_it_is_about() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_first_launch();
    open_dialog(&mut shell, Template::Files, Step::Details, dir.path());
    settle(&mut shell, WINDOWS[0]);

    shell
        .state
        .front_door
        .new_project
        .as_mut()
        .expect("open")
        .set_name("a/b".into());
    settle(&mut shell, WINDOWS[0]);
    let field = widget(&shell, "Project name").bounds;
    let next = widget(&shell, "Create in").bounds;
    let message = (shell.described.iter())
        .find(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n.contains("name is a folder name"))
        })
        .unwrap_or_else(|| panic!("no message about the name: {:?}", names(&shell)))
        .bounds;
    assert!(
        message.min.y >= field.max.y - 1.0 && message.max.y <= next.min.y + 1.0,
        "the message about the name is not between its field {field:?} and the next label {next:?}: {message:?}"
    );

    {
        let draft = shell.state.front_door.new_project.as_mut().expect("open");
        draft.set_name("police".into());
        draft.place = dir.path().join("nowhere").display().to_string();
    }
    settle(&mut shell, WINDOWS[0]);
    let field = widget(&shell, "Folder").bounds;
    let next = widget(&shell, "Game").bounds;
    let message = (shell.described.iter())
        .find(|w| w.name.as_deref().is_some_and(|n| n.contains("must exist")))
        .unwrap_or_else(|| panic!("no message about the folder: {:?}", names(&shell)))
        .bounds;
    assert!(
        message.min.y >= field.max.y - 1.0 && message.max.y <= next.min.y + 1.0,
        "the message about the folder is not between its field {field:?} and the next label {next:?}: {message:?}"
    );
}

#[test]
fn the_tree_is_a_placeholder_until_there_is_a_name_and_a_folder() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_first_launch();
    let mut draft = NewProject::new(Template::Files, dir.path().to_path_buf());
    draft.step = Step::Details;
    shell.state.front_door.new_project = Some(draft);
    settle(&mut shell, WINDOWS[0]);
    assert!(
        shows(&shell, "<name>/") && shows(&shell, "<name>.openrage-proj"),
        "no name yet and the tree does not stand in for it: {:?}",
        names(&shell)
    );

    shell.state.front_door.new_project = Some({
        let mut draft = NewProject::new(Template::Adopt, dir.path().to_path_buf());
        draft.step = Step::Details;
        draft
    });
    settle(&mut shell, WINDOWS[0]);
    assert!(
        shows(&shell, "<folder to adopt>/"),
        "no folder yet and the tree does not stand in for it: {:?}",
        names(&shell)
    );
}

#[test]
fn the_preview_of_an_adopted_folder_counts_in_real_plurals_in_the_forms_type_size() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let one = dir.path().join("one");
    let two = dir.path().join("two");
    std::fs::create_dir_all(&one).expect("a folder");
    std::fs::create_dir_all(&two).expect("a folder");
    std::fs::write(one.join("a.lua"), b"x").expect("a file");
    std::fs::write(two.join("a.lua"), b"x").expect("a file");
    std::fs::write(two.join("b.lua"), b"x").expect("a file");

    for (folder, said) in [(&one, "Files: 1 file"), (&two, "Files: 2 files")] {
        let mut shell = a_first_launch();
        let background = names(&shell);
        shell.state.front_door.new_project = Some(NewProject::adopting(folder));
        waits_for(&mut shell, "The folder holds");
        let shapes = shell.draw_sized(WINDOWS[0], Vec::new());
        let said_in_the_dialog = in_the_dialog(&shell, &background);
        assert!(
            said_in_the_dialog.iter().any(|name| name == said),
            "`{said}` is not in the preview: {said_in_the_dialog:?}"
        );
        for name in &said_in_the_dialog {
            assert!(!name.contains("(s)"), "a plural left to the reader: {name}");
            assert!(!name.contains('`'), "a backtick on screen: {name}");
        }
        let form = type_size_of(&shapes, "Folder to make a project of").expect("the form's label");
        for line in [said, "The folder holds", "Nothing is moved or rewritten"] {
            let size =
                type_size_of(&shapes, line).unwrap_or_else(|| panic!("`{line}` is not painted"));
            assert!(
                (size - form).abs() < 0.1,
                "`{line}` is {size} points and the form is {form}"
            );
        }
    }
}

#[test]
fn creating_a_project_is_not_done_in_the_frame_that_asks_for_it() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_first_launch();
    open_dialog(&mut shell, Template::Files, Step::Details, dir.path());
    settle(&mut shell, WINDOWS[0]);
    let create = widget(&shell, "Create").bounds.center();
    let button = |pressed| egui::Event::PointerButton {
        pos: create,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw(vec![egui::Event::PointerMoved(create), button(true)]);
    shell.draw(vec![button(false)]);

    assert!(
        shell.state.project_path.is_none() && shell.state.is_creating(),
        "the project was made on the thread that draws"
    );
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while shell.state.project_path.is_none() {
        assert!(
            std::time::Instant::now() < until,
            "the project was never made"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
        shell.draw(Vec::new());
    }
    assert!(dir
        .path()
        .join("police")
        .join("police.openrage-proj")
        .is_file());
}

/// Two projects in a scratch folder, the first open in the shell.
fn two_projects(shell: &mut Shell, dir: &Path) -> (PathBuf, PathBuf) {
    let first = a_project(dir, "police", Template::Files);
    let second = a_project(dir, "sheriff", Template::Files);
    shell.state.documents.clear();
    shell.state.active_document = None;
    shell.state.load_project_from(first.clone());
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !shell.state.workbench.is_scanned() {
        assert!(std::time::Instant::now() < until, "the scan never ended");
        shell.state.poll_workbench();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    shell.state.prefs.recent_projects = vec![first.clone(), second.clone()];
    (first, second)
}

#[test]
fn the_places_menu_and_the_empty_state_ask_where_a_second_project_opens() {
    let dir = tempfile::tempdir().expect("a scratch folder");

    let mut shell = a_first_launch();
    let (first, second) = two_projects(&mut shell, dir.path());
    shell.state.active_workspace = Workspace::Browse;
    shell.settled();
    click_on(&mut shell, "Places");
    click_on(&mut shell, "sheriff.openrage-proj");
    assert_eq!(
        shell.state.front_door.switching.as_deref(),
        Some(second.as_path()),
        "the Places menu replaced the open project without asking"
    );
    assert_eq!(shell.state.project_path.as_deref(), Some(first.as_path()));

    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_first_launch();
    let (first, second) = two_projects(&mut shell, dir.path());
    // The Project workspace has no Document panel until someone docks one; a file open that it does not handle is what makes the panel draw the empty state.
    *shell.state.workspace_docks.get_mut(Workspace::Project) =
        egui_dock::DockState::new(vec![crate::ui::dock::Tab::DOCUMENT]);
    shell
        .state
        .new_empty_document(crate::state::DocumentKind::Ypt);
    shell.state.active_workspace = Workspace::Project;
    shell.settled();
    click_on(&mut shell, "sheriff");
    assert_eq!(
        shell.state.front_door.switching.as_deref(),
        Some(second.as_path()),
        "the empty state replaced the open project without asking"
    );
    assert_eq!(shell.state.project_path.as_deref(), Some(first.as_path()));
}

#[test]
fn back_leaves_the_start_page_with_no_document_open() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_first_launch();
    let (_, _) = two_projects(&mut shell, dir.path());
    shell.state.active_workspace = Workspace::Project;
    shell.state.show_start_page();
    shell.settled();
    assert!(
        shell.state.documents.is_empty(),
        "the scenario has no document open"
    );

    click_on(&mut shell, "Back");

    assert!(
        !shell.state.front_door.raised(shell.state.active_workspace),
        "Back left the start page up"
    );
}

/// The main button and the message under the fields are one reading of the disk: the disk changing under the dialog moves both in the same frame, never the button alone.
#[test]
fn the_main_button_changes_with_the_message_beside_it_and_not_before() {
    for (template, button, message) in [
        (Template::Files, "Create", "already exists and is not empty"),
        (Template::Adopt, "Make project", "already holds the project"),
    ] {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let mut shell = a_first_launch();
        let mut draft = if template == Template::Adopt {
            NewProject::adopting(dir.path())
        } else {
            NewProject::new(template, dir.path().to_path_buf())
        };
        draft.set_name("police".into());
        draft.step = Step::Details;
        shell.state.front_door.new_project = Some(draft);
        shell.settled();

        let reading = |shell: &Shell| {
            let disabled = (shell.described.iter())
                .find(|w| w.name.as_deref() == Some(button))
                .unwrap_or_else(|| panic!("no `{button}`: {:?}", names(shell)))
                .disabled;
            (disabled, shows(shell, message))
        };
        assert_eq!(
            reading(&shell),
            (false, false),
            "{template:?}: the dialog should start ready"
        );

        if template == Template::Adopt {
            let file = format!("police.{}", or_project::PROJECT_EXTENSION);
            std::fs::write(dir.path().join(file), b"{}").expect("a project file");
        } else {
            let taken = dir.path().join("police");
            std::fs::create_dir(&taken).expect("the project's folder");
            std::fs::write(taken.join("held.txt"), b"x").expect("a file in it");
        }

        // The dialog looks at the disk once a second; until it looks again, the button and the message both stand as they were.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            shell.draw(Vec::new());
            let (disabled, said) = reading(&shell);
            assert_eq!(
                disabled, said,
                "{template:?}: `{button}` disabled is {disabled} while the message about the disk is shown: {said}"
            );
            if said {
                break;
            }
            assert!(
                std::time::Instant::now() < until,
                "{template:?}: the dialog never saw the disk change"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
