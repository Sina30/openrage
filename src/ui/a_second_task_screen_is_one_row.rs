//! A row added to the table of task screens is a second task screen in every place that shows one, with no line of its own in any of them.
//!
//! The row stands in for a screen of a kind of content the build does not have: `widgets`, shown on the Animation workspace, which the row makes a task screen for the length of
//! the test and on its thread alone. Each test names a place and fails when that place still decides by the Wardrobe.

use or_project::{Module, ModuleKind};

use super::frames_of_the_shell::Shell;
use super::task_screens::{with_a_second_row, TaskScreen};
use super::the_front_door_on_screen::{a_first_launch, a_project, shows, waits_for};
use crate::state::front_door::cards::read_card;
use crate::state::front_door::Template;
use crate::ui::commands::{self, Action};
use crate::ui::dock::{Tab, Workspace};
use crate::ui::panels::registry::ids;

const OTHER: &str = "Only widget content is shown in the Animation workspace";

fn widgets() -> TaskScreen {
    TaskScreen {
        workspace: Workspace::Animation,
        kind: ModuleKind::Unknown("widgets".into()),
        edits_files: false,
        label: "Animation",
        tooltip: "The bench of widgets",
        content: "Widgets",
        count: "widget",
        other_content: OTHER,
        its_rows: "A widget is shown in the Animation workspace",
        empty: "No widgets in this project",
        add: Action::ImportFolder,
        add_button: "Add a widget bench to this project",
        landing_note: "An empty bench, with the Animation workspace to fill it.",
        drop_hint: "Drop widgets here",
        drop: Action::ImportFolder,
        menu: &[Action::ImportFolder],
        undo: None,
        details: None,
        bar: None,
        layout: || egui_dock::DockState::new(vec![Tab::DOCUMENT]),
        must_dock: &[],
        starts: None,
        data_sources: Some("data/widgets/"),
        held: None,
        on_disk: |_, _| Some(2),
        showing: |_| None,
        show: |state, id| state.workbench.selected_output = Some(id.to_string()),
        offered_by: |_| false,
    }
}

fn widget_module() -> Module {
    Module {
        id: "bench".into(),
        kind: ModuleKind::Unknown("widgets".into()),
        extra: Default::default(),
    }
}

/// The name of the row of a content that starts with `word`: the panel also names the rows it holds together in one.
fn the_row(shell: &Shell, word: &str) -> String {
    crate::ui::the_front_door_on_screen::names(shell)
        .into_iter()
        .find(|n| n.starts_with(word) && !n.contains('\n'))
        .unwrap_or_else(|| panic!("no row of `{word}`"))
}

/// A shell on a Files project, which holds widgets when `with_widgets`.
fn a_shell(dir: &std::path::Path, with_widgets: bool) -> Shell {
    let file = a_project(dir, "gadgets", Template::Files);
    let mut shell = a_first_launch();
    shell.state.load_project_from(file);
    if with_widgets {
        shell
            .state
            .project_carried
            .container
            .modules
            .push(widget_module());
    }
    shell.state.active_workspace = Workspace::Project;
    shell.settled();
    shell
}

#[test]
fn the_strip_and_the_palette_offer_a_second_row_only_while_its_content_is_held() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    with_a_second_row(widgets(), || {
        let mut shell = a_shell(dir.path(), false);
        let palette = |shell: &Shell| {
            commands::workspaces(&shell.state)
                .iter()
                .any(|c| c.label == "Animation")
        };
        assert!(Workspace::Animation.is_task_screen());
        assert!(!shell
            .state
            .workspaces_offered()
            .contains(&Workspace::Animation));
        assert!(
            !palette(&shell),
            "the palette lists a screen with no content"
        );

        shell
            .state
            .project_carried
            .container
            .modules
            .push(widget_module());
        assert!(shell
            .state
            .workspaces_offered()
            .contains(&Workspace::Animation));
        assert!(palette(&shell), "the palette lacks a screen with content");
        assert!(shell
            .state
            .workspaces_shown()
            .contains(&Workspace::Animation));
    });
    assert!(
        !Workspace::Animation.is_task_screen(),
        "the row stays on the thread of the test"
    );
}

#[test]
fn the_window_leaves_a_second_rows_screen_when_its_content_goes_and_comes_back_with_it() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    with_a_second_row(widgets(), || {
        let mut shell = a_shell(dir.path(), true);
        shell.state.active_workspace = Workspace::Animation;
        shell.state.project_carried.container.modules.pop();
        shell.state.poll_workbench();
        assert_eq!(shell.state.active_workspace, Workspace::Project);
        shell
            .state
            .project_carried
            .container
            .modules
            .push(widget_module());
        shell.state.poll_workbench();
        assert_eq!(shell.state.active_workspace, Workspace::Animation);
    });
}

#[test]
fn the_content_panel_says_a_second_rows_words_and_a_click_opens_its_screen() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    with_a_second_row(widgets(), || {
        let mut shell = a_shell(dir.path(), true);
        assert!(
            shows(&shell, "Widgets  ·  0 widgets") || shows(&shell, "Widgets · 0 widgets"),
            "{:?}",
            crate::ui::the_front_door_on_screen::names(&shell)
        );
        let row = the_row(&shell, "Widgets");
        let at = crate::ui::the_project_screens_on_screen::centre_of(&shell, &row);
        shell.click(at);
        shell.settled();
        assert_eq!(shell.state.active_workspace, Workspace::Animation);
        assert_eq!(
            shell.state.workbench.selected_output.as_deref(),
            Some("bench")
        );
    });
}

#[test]
fn open_in_says_a_second_row_shows_its_own_content_and_no_other() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    with_a_second_row(widgets(), || {
        let mut shell = a_shell(dir.path(), true);
        let files = the_row(&shell, "Files");
        let at = crate::ui::the_project_screens_on_screen::right_click_on(&mut shell, &files);
        crate::ui::the_project_screens_on_screen::click_nearest(&mut shell, "Open in", at);
        assert!(shows(&shell, OTHER), "Files content is not widgets");
        shell.draw(vec![super::frames_of_the_shell::key(egui::Key::Escape)]);
        shell.draw(vec![super::frames_of_the_shell::key(egui::Key::Escape)]);
        shell.settled();

        let widgets = the_row(&shell, "Widgets");
        let at = crate::ui::the_project_screens_on_screen::right_click_on(&mut shell, &widgets);
        crate::ui::the_project_screens_on_screen::click_nearest(&mut shell, "Open in", at);
        assert!(!shows(&shell, OTHER), "widgets content is widgets");
    });
}

#[test]
fn the_start_page_counts_a_second_rows_content_in_its_word() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    with_a_second_row(widgets(), || {
        let file = a_project(dir.path(), "gadgets", Template::Files);
        or_project::project::edit_container(&file, |container| {
            container.modules.push(widget_module());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            read_card(&file).content,
            ["Files · 0 files", "Widgets · 2 widgets"]
        );
        let mut shell = a_first_launch();
        shell.state.prefs.touch_recent_project(file);
        shell.draw(Vec::new());
        waits_for(&mut shell, "Widgets · 2 widgets");
    });
}

#[test]
fn the_empty_state_the_drop_hint_and_the_layout_of_a_second_row_are_its_own() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    with_a_second_row(widgets(), || {
        assert_eq!(
            crate::ui::panels::empty_state::heading(Workspace::Animation),
            "No widgets in this project"
        );
        // ⚠ A document the screen does not show leaves it with nothing to draw but its empty state.
        let mut shell = a_shell(dir.path(), true);
        shell.state.documents.push(crate::state::Document::new(
            9,
            Some("notes.txt".into()),
            crate::state::DocPayload::Text(crate::state::TextDocument::decode(b"text")),
        ));
        shell.state.set_active_document(9);
        shell.state.active_workspace = Workspace::Animation;
        shell.settled();
        assert!(
            shows(&shell, "Add a widget bench to this project"),
            "{:?} {:?}",
            shell.state.active_workspace,
            crate::ui::the_front_door_on_screen::names(&shell)
        );
        let docks = &mut shell.state.workspace_docks;
        docks.get_mut(Workspace::Animation);
        assert!(docks.shows(Workspace::Animation, ids::DOCUMENT));
        assert!(
            !docks.shows(Workspace::Animation, ids::TIMELINE),
            "the layout is the row's and not the Animation workspace's"
        );
    });
}

#[test]
fn the_project_menu_lists_the_commands_of_a_second_row_under_its_name() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    with_a_second_row(widgets(), || {
        let mut shell = a_shell(dir.path(), true);
        // ⚠ A tab of the dock and a button of the strip share its name: the menu is the one on the row of File, and the first there.
        let row = (shell.described.iter())
            .find(|w| w.name.as_deref() == Some("File"))
            .expect("the File menu")
            .bounds
            .center()
            .y;
        let menu = (shell.described.iter())
            .filter(|w| {
                w.name.as_deref() == Some("Project") && (w.bounds.center().y - row).abs() < 1.0
            })
            .min_by(|a, b| a.bounds.min.x.total_cmp(&b.bounds.min.x))
            .expect("the Project menu")
            .bounds
            .center();
        shell.click(menu);
        shell.settled();
        let submenu = |shell: &Shell, name: &str| {
            (shell.described.iter())
                .filter(|w| {
                    w.name
                        .as_deref()
                        .and_then(|n| {
                            n.strip_suffix(egui::containers::menu::SubMenuButton::RIGHT_ARROW)
                        })
                        .map(str::trim_end)
                        == Some(name)
                })
                .map(|w| w.bounds.center())
                .next()
        };
        // ⚠ The submenus of the screens are folded under one More, since a popup does not scroll and a third screen would not fit.
        assert!(
            submenu(&shell, "Widgets").is_none(),
            "a screen's submenu is in the Project menu itself: {:?}",
            crate::ui::the_front_door_on_screen::names(&shell)
        );
        let more = submenu(&shell, "More").unwrap_or_else(|| {
            panic!(
                "the Project menu has no More: {:?}",
                crate::ui::the_front_door_on_screen::names(&shell)
            )
        });
        shell.click(more);
        shell.settled();
        assert!(
            submenu(&shell, "Widgets").is_some(),
            "More has no submenu of the screen: {:?}",
            crate::ui::the_front_door_on_screen::names(&shell)
        );
    });
}

#[test]
fn the_hint_under_nothing_open_says_what_a_drop_on_a_second_row_does() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    with_a_second_row(widgets(), || {
        let mut shell = a_shell(dir.path(), true);
        shell.state.documents.clear();
        shell.state.active_document = None;
        shell.state.active_workspace = Workspace::Animation;
        shell.settled();
        assert!(
            shows(&shell, "Drop widgets here, or Import folder"),
            "{:?}",
            crate::ui::the_front_door_on_screen::names(&shell)
        );
    });
}
