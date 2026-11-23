//! The menus, the corner of the menu bar, the status bar, the tour, the toasts and the guide as a person meets them: every row of
//! a menu is on screen in the smallest window, the corner says a word only over what it guards, and a tour is true where it is taken.
//!
//! Read through AccessKit from whole frames of the shell, in the smallest window the editor has.

use egui::accesskit::Role;

use super::frames_of_the_shell::Shell;
use super::the_front_door_on_screen::{a_first_launch, a_project, names, shows};
use crate::state::front_door::{Template, TOUR};
use crate::state::toasts::{Toast, ToastAction};
use crate::state::ThemeChoice;
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;

const SMALLEST: (u32, u32) = (1280, 720);

/// A click at `at` in a window of the smallest size, and the frames after it that settle what it opened.
fn click_small(shell: &mut Shell, at: egui::Pos2) {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw_sized(SMALLEST, vec![egui::Event::PointerMoved(at), press(true)]);
    shell.draw_sized(SMALLEST, vec![press(false)]);
    shell.settled_sized(SMALLEST);
}

/// The menu of the bar named `title`, opened: the leftmost button of that name in the bar's own row.
fn open_menu(shell: &mut Shell, title: &str) {
    let at = (shell.described.iter())
        .filter(|w| {
            w.role == Role::Button && w.name.as_deref() == Some(title) && w.bounds.max.y < 25.0
        })
        .min_by(|a, b| a.bounds.min.x.total_cmp(&b.bounds.min.x))
        .unwrap_or_else(|| panic!("the bar has no {title}: {:?}", names(shell)))
        .bounds
        .center();
    click_small(shell, at);
}

fn top_of(shell: &Shell, name: &str) -> f32 {
    (shell.described.iter())
        .filter(|w| w.name.as_deref().is_some_and(|n| n.starts_with(name)))
        .map(|w| w.bounds.min.y)
        .min_by(f32::total_cmp)
        .unwrap_or_else(|| panic!("nothing starts `{name}`: {:?}", names(shell)))
}

fn a_window_with_a_project(dir: &std::path::Path) -> Shell {
    let file = a_project(dir, "police", Template::Files);
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Project;
    shell.settled_sized(SMALLEST);
    shell
}

#[test]
fn the_project_menu_fits_the_smallest_window_and_keeps_the_rest_in_submenus() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_window_with_a_project(dir.path());
    open_menu(&mut shell, "Project");

    let rows: Vec<&super::frames_of_the_shell::Described> = shell
        .described
        .iter()
        .filter(|w| matches!(w.role, Role::Button | Role::CheckBox | Role::MenuItem))
        .filter(|w| w.name.is_some() && w.bounds.min.y > 28.0)
        .collect();
    let off: Vec<&str> = rows
        .iter()
        .filter(|w| w.bounds.max.y > SMALLEST.1 as f32 + 0.5)
        .filter_map(|w| w.name.as_deref())
        .collect();
    assert!(
        off.is_empty(),
        "rows of the Project menu are off the screen: {off:?}"
    );
    for text in [
        "Add files…",
        "Add build…",
        "Build",
        "Save project",
        "Close project",
    ] {
        assert!(
            shows(&shell, text),
            "the menu lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    // The screens of a kind of content (Clothing, and the next ones) share one More.
    for submenu in ["More", "Fix", "History", "Properties and sharing"] {
        assert!(
            names(&shell).iter().any(|n| n.starts_with(submenu)
                && n.ends_with(egui::containers::menu::SubMenuButton::RIGHT_ARROW)),
            "the menu has no submenu `{submenu}`: {:?}",
            names(&shell)
        );
    }
    assert!(
        !shows(&shell, "Credit garment"),
        "a clothing tool is on the top level"
    );
    assert!(
        !names(&shell).iter().any(|n| n.starts_with("Clothing")),
        "the Clothing submenu is not folded under More: {:?}",
        names(&shell)
    );
}

#[test]
fn the_file_menu_has_open_project_beside_open_file_and_a_submenu_that_says_what_it_holds() {
    let _serial = crate::ui::theme::serial();
    let mut shell = a_first_launch();
    shell.settled_sized(SMALLEST);
    open_menu(&mut shell, "File");

    assert!(
        top_of(&shell, "Open project…") < top_of(&shell, "Open file…"),
        "opening a project, the most frequent act, is below opening a file: {:?}",
        names(&shell)
    );
    assert!(shows(&shell, "Open recent"), "{:?}", names(&shell));
    assert!(shows(&shell, "Import built pack…"), "{:?}", names(&shell));
    assert!(
        !shows(&shell, "More Project Commands"),
        "File holds a second menu of the project's commands: {:?}",
        names(&shell)
    );
}

#[test]
fn the_corner_says_read_only_over_what_the_mode_guards_and_nothing_over_what_it_does_not() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let read_only = |shell: &Shell| names(shell).iter().any(|n| n.starts_with("Read only"));

    // Nothing open: nothing to guard.
    let mut shell = a_first_launch();
    shell.settled_sized(SMALLEST);
    assert!(!read_only(&shell), "{:?}", names(&shell));

    // The install in Browse is what the mode guards.
    let install = shell.state.open_game_root(dir.path().to_path_buf());
    shell.state.set_active_document(install);
    shell.state.active_workspace = Workspace::Browse;
    shell.settled_sized(SMALLEST);
    assert!(
        read_only(&shell),
        "Read only is gone over the install: {:?}",
        names(&shell)
    );

    // The Project workspace shows no tab of the install, and says nothing about it.
    std::fs::create_dir_all(dir.path().join("p")).unwrap();
    let file = a_project(&dir.path().join("p"), "police", Template::Files);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Project;
    shell.settled_sized(SMALLEST);
    assert!(
        !read_only(&shell),
        "a project window says Read only beside a project that is edited: {:?}",
        names(&shell)
    );
}

#[test]
fn the_status_bar_has_no_brackets_counts_tabs_and_names_the_game_folder() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_first_launch();
    shell.state.documents.clear();
    let install = shell.state.open_game_root(dir.path().to_path_buf());
    shell.state.set_active_document(install);
    shell.state.active_workspace = Workspace::Browse;
    shell.settled_sized(SMALLEST);

    let said = names(&shell);
    assert!(!said.iter().any(|n| n.starts_with('[')), "{said:?}");
    assert!(
        said.iter()
            .any(|n| n.ends_with("tab open") || n.ends_with("tabs open")),
        "the count says document, which is not a screen word: {said:?}"
    );
    assert!(
        said.iter().any(|n| n.starts_with("Game folder: ")),
        "{said:?}"
    );
}

#[test]
fn a_tip_names_what_shift_f1_does_and_the_guide_has_a_way_to_each_section() {
    let _serial = crate::ui::theme::serial();
    assert_eq!(
        crate::ui::guide_links::line("Build"),
        "Shift+F1 opens the guide at \"Build\""
    );
    let mut shell = a_first_launch();
    shell.state.front_door.show_guide("");
    shell.settled_sized(SMALLEST);
    for section in crate::ui::panels::guide::headings() {
        let button = format!("Guide section: {section}");
        assert!(
            shows(&shell, &button),
            "no way to `{section}`: {:?}",
            names(&shell)
        );
    }
}

#[test]
fn the_tour_takes_the_sample_closes_the_guide_and_says_nothing_untrue_of_the_screen() {
    let _serial = crate::ui::theme::serial();
    let mut shell = a_first_launch();
    shell.state.front_door.show_guide("");
    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::TakeTour);
    shell.settled_sized(SMALLEST);

    assert_eq!(
        shell.state.project_name().as_deref(),
        Some("OpenRage sample"),
        "the tour runs on a project that is not there"
    );
    assert_eq!(shell.state.front_door.tour, Some(0));
    assert!(
        shell.state.front_door.guide.is_none(),
        "the guide sits over the panel the step names"
    );
    assert!(
        !TOUR.iter().any(|step| step.words.contains("This sample")),
        "a step claims the project is the sample"
    );
}

#[test]
fn a_project_that_is_gone_says_so_with_a_way_to_open_another_and_a_toast_never_covers_the_tour() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_first_launch();
    shell
        .state
        .project_not_found(&dir.path().join("missing.openrage-proj"));
    let toast = shell.state.toasts.iter().last().expect("a toast").clone();
    assert!(
        toast.message.contains("moved or deleted"),
        "{}",
        toast.message
    );
    assert_eq!(
        toast.action.map(|a| a.label()),
        Some("Open a project…"),
        "a project that is gone leaves the person nothing to press"
    );

    let mut shell = a_window_with_a_project(dir.path());
    shell.state.start_tour();
    shell
        .state
        .toasts
        .push(Toast::info("A toast over the tour").with_action(ToastAction::NewProject));
    shell.settled_sized(SMALLEST);
    let rect = |name: &str| {
        (shell.described.iter())
            .find(|w| w.role == Role::Button && w.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no button `{name}`: {:?}", names(&shell)))
            .bounds
    };
    let toast_button = rect("New project…");
    for covered in ["Next", "End the tour"] {
        assert!(
            !toast_button.intersects(rect(covered)),
            "the toast covers `{covered}` of the tour"
        );
    }
}

/// The y of the centre of the menu row whose name starts with `name`.
fn row_y(shell: &Shell, name: &str) -> f32 {
    (shell.described.iter())
        .filter(|w| w.name.as_deref().is_some_and(|n| n.starts_with(name)) && w.bounds.min.y > 28.0)
        .map(|w| w.bounds.center().y)
        .next()
        .unwrap_or_else(|| panic!("no row `{name}`: {:?}", names(shell)))
}

#[test]
fn the_project_menu_groups_the_ways_in_the_builds_and_the_links_and_says_one_casing() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_window_with_a_project(dir.path());
    open_menu(&mut shell, "Project");

    let pitch = row_y(&shell, "Import game files…") - row_y(&shell, "Add files…");
    for (before, after, between) in [
        (
            "New texture dictionary…",
            "Add build…",
            "the ways in and the builds",
        ),
        ("Edit build…", "Link content…", "the builds and the links"),
        ("Link content…", "Build", "the links and Build"),
    ] {
        let gap = row_y(&shell, after) - row_y(&shell, before);
        assert!(
            gap > pitch + 2.0,
            "no separator between {between}: {before} to {after} is {gap}, a row is {pitch}: {:?}",
            names(&shell)
        );
    }
    for old in [
        "Import Game Files…",
        "Import Folder…",
        "Save Project",
        "Close Project",
    ] {
        assert!(
            !shows(&shell, old),
            "`{old}` is in Title Case among rows in sentence case: {:?}",
            names(&shell)
        );
    }
    assert!(
        !shows(&shell, "Unlink content"),
        "Unlink content is offered, greyed, where no content is linked: {:?}",
        names(&shell)
    );
}

#[test]
fn ctrl_s_saves_the_project_when_no_document_is_open_and_the_project_has_changes() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_window_with_a_project(dir.path());
    shell.state.documents.clear();
    shell.state.active_document = None;
    shell.state.project_carried.changed = true;
    let save = commands::of(&shell.state, Action::Save).expect("Save");
    assert_eq!(
        save.enabled,
        Ok(()),
        "Ctrl+S is off while the project has changes and no document is open"
    );
    commands::run(&mut shell.state, &shell.ctx.clone(), &save);
    assert!(
        !shell.state.project_carried.changed,
        "Ctrl+S left the project's changes unsaved"
    );
    let save = commands::of(&shell.state, Action::Save).expect("Save");
    assert!(
        save.enabled.is_err(),
        "Ctrl+S is on with nothing to save: it would write the project again for nothing"
    );
}

#[test]
fn the_tour_card_never_covers_the_buttons_the_step_is_about_in_the_smallest_window() {
    let _serial = crate::ui::theme::serial();
    let mut shell = a_first_launch();
    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::TakeTour);
    super::the_project_screens_on_screen::until(&mut shell, "the sample's problem", |s| {
        !s.state.workbench.findings().is_empty()
    });
    shell.settled_sized(SMALLEST);
    let window = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(SMALLEST.0 as f32, SMALLEST.1 as f32),
    );
    for (step, buttons) in [
        (1, &["Fix…", "Waive…"][..]),
        (3, &["Edit build…", "Verify with a second build"][..]),
    ] {
        while shell.state.front_door.tour != Some(step) {
            shell.state.tour_next();
        }
        shell.settled_sized(SMALLEST);
        let card = shell
            .ctx
            .memory(|m| m.area_rect(egui::Id::new("the_tour")))
            .expect("the tour's card");
        assert!(
            window.contains_rect(card),
            "step {step}: the card is partly off the window: {card:?}"
        );
        for name in buttons {
            let shown: Vec<_> = (shell.described.iter())
                .filter(|w| w.name.as_deref().is_some_and(|n| n.starts_with(name)))
                .filter(|w| window.contains_rect(w.bounds))
                .collect();
            assert!(!shown.is_empty(), "step {step}: no `{name}` on screen");
            for widget in shown {
                assert!(
                    !card.intersects(widget.bounds),
                    "step {step}: the card covers `{name}`: {card:?} over {:?}",
                    widget.bounds
                );
            }
        }
    }
}

#[test]
fn the_guide_heads_the_section_that_makes_a_project_with_adding_and_never_with_open() {
    let headings = crate::ui::panels::guide::headings();
    assert!(headings.contains(&"Add your clothes"), "{headings:?}");
    assert!(
        !headings.contains(&"Open a pack"),
        "the section that makes a project is headed with the word for looking: {headings:?}"
    );
    let guide = include_str!("../../docs/guides/Workbench_Guide.md");
    assert!(
        !guide.contains("copied into the project"),
        "the guide says a dropped file is copied: it is decompiled to a source at once"
    );
}
