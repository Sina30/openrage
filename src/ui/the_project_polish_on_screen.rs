//! The words of the project screens, and the options the Build tab and the project settings offer, as they are drawn.

use super::the_project_screens_on_screen::{a_project_shell, names, shows, until};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;

/// The names of the widgets of a frame that are a rule's code: kebab-case, as `RULES` spells them.
fn codes_drawn(names: &[String]) -> Vec<String> {
    let codes: Vec<&str> = or_wardrobe::RULES
        .iter()
        .map(|r| r.code)
        .filter(|c| c.contains('-'))
        .collect();
    names
        .iter()
        .filter(|name| codes.iter().any(|code| name.contains(code)))
        .cloned()
        .collect()
}

fn the_clothing_content(shell: &super::frames_of_the_shell::Shell) -> String {
    (shell.state.project_carried.container.modules.iter())
        .find(|m| m.kind == or_project::ModuleKind::Clothing)
        .expect("a clothing content")
        .id
        .clone()
}

#[test]
fn the_rules_window_says_each_rule_in_words_and_keeps_its_code_for_the_tooltip() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let module = the_clothing_content(&shell);
    shell.state.ask_about_rules(module);
    shell.settled();
    assert!(
        shows(&shell, "Rules of the clothing"),
        "{:?}",
        names(&shell)
    );
    assert!(
        shows(&shell, "Apply"),
        "the window is drawn: {:?}",
        names(&shell)
    );
    let drawn = codes_drawn(&names(&shell));
    assert!(
        drawn.is_empty(),
        "a rule's code is read as a label: {drawn:?}"
    );

    // The rules no reason mends are listed under a header that starts closed, at the foot of a list that scrolls.
    let bounds = |shell: &super::frames_of_the_shell::Shell, name: &str| {
        (shell.described.iter())
            .find(|w| w.name.as_deref().is_some_and(|n| n.starts_with(name)))
            .map(|w| w.bounds)
            .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
    };
    for _ in 0..40 {
        let (header, apply) = (bounds(&shell, "Always on"), bounds(&shell, "Apply"));
        if header.max.y < apply.min.y - 4.0 {
            break;
        }
        let over = egui::pos2(header.center().x, apply.min.y - 60.0);
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
    let at = bounds(&shell, "Always on").center();
    shell.click(at);
    shell.settled();
    let drawn = codes_drawn(&names(&shell));
    assert!(
        drawn.is_empty(),
        "a rule's code is read in the list of the rules that are always on: {drawn:?}"
    );
    assert!(
        shows(&shell, "A garment has no ped or no slot"),
        "the sentence of a rule that is always on is drawn: {:?}",
        names(&shell)
    );
}

#[test]
fn the_waivers_list_says_the_rule_in_words() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let module = the_clothing_content(&shell);
    shell.state.waive_finding(
        module,
        "duplicate-content".into(),
        None,
        "the same jacket in two colours".into(),
    );
    shell.state.active_workspace = Workspace::Wardrobe;
    shell.state.select_asset(None);
    until(&mut shell, "the waivers list", |s| shows(s, "Waivers (1)"));
    assert!(
        shows(&shell, "the same jacket in two colours"),
        "the reason is drawn: {:?}",
        names(&shell)
    );
    let drawn = codes_drawn(&names(&shell));
    assert!(
        drawn.is_empty(),
        "a rule's code is read as a label: {drawn:?}"
    );
}

#[test]
fn the_build_tab_can_ask_for_the_second_build_and_says_whether_it_ran() {
    use crate::state::workbench::LastBuild;
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = super::the_build_tab_on_screen::a_files_project(dir.path());
    let built = |shell: &mut super::frames_of_the_shell::Shell| {
        shell.state.build_selected();
        until(shell, "the build", |s| {
            !s.state.workbench.builds().is_running()
                && matches!(
                    s.state.workbench.builds().last("fivem"),
                    Some(LastBuild::Built(_))
                )
        });
        shell.settled();
    };

    built(&mut shell);
    assert!(
        shows(&shell, "was not run"),
        "a build that did not ask for it says it did not run it: {:?}",
        names(&shell)
    );

    let at = super::the_project_screens_on_screen::centre_of(&shell, "Verify with a second build");
    shell.click(at);
    shell.settled();
    built(&mut shell);
    assert!(
        shows(&shell, "A second build wrote the same bytes"),
        "{:?}",
        names(&shell)
    );
}

#[test]
fn the_project_settings_say_what_a_repository_holds_and_a_box_writes_the_git_block() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell.state.active_workspace = Workspace::Project;
    shell.state.workspace_docks.reveal(
        Workspace::Project,
        crate::ui::panels::registry::ids::DETAILS,
    );
    shell.state.select_asset(None);
    shell.settled();
    // The boxes are folded under a heading that says what they are for.
    let at =
        super::the_project_screens_on_screen::centre_of(&shell, "If you keep this folder in Git");
    shell.click(at);
    shell.settled();
    for said in [
        "Commit the sources",
        "Commit the build",
        "Commit sources taken from the game",
    ] {
        assert!(shows(&shell, said), "{said}: {:?}", names(&shell));
    }

    let folder = or_project::project_folder(&shell.state.project_path.clone().unwrap());
    let ignore = |folder: &std::path::Path| {
        std::fs::read_to_string(folder.join(".gitignore")).unwrap_or_default()
    };
    assert!(
        ignore(&folder).contains("/build/"),
        "a repository ignores the build to begin with"
    );
    let at = super::the_project_screens_on_screen::centre_of(&shell, "Commit the build");
    shell.click(at);
    shell.settled();
    let written = ignore(&folder);
    assert!(
        !written.contains("/build/") && written.contains("# commit: sources build"),
        "{written}"
    );
}

#[test]
fn up_and_down_move_the_selection_of_the_assets_table_and_enter_opens_the_asset() {
    use crate::state::workbench::Kept;
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell.state.active_workspace = Workspace::Project;
    shell
        .state
        .workspace_docks
        .reveal(Workspace::Project, crate::ui::panels::registry::ids::ASSETS);
    shell.state.select_asset(None);
    shell.settled();
    // The keys are the table's while the pointer is over it.
    let table = super::the_project_screens_on_screen::centre_of(&shell, "Filter assets");
    let over = table + egui::vec2(0.0, 120.0);
    shell.draw(vec![egui::Event::PointerMoved(over)]);

    let press = |shell: &mut super::frames_of_the_shell::Shell, k: egui::Key| {
        shell.draw(vec![super::frames_of_the_shell::key(k)]);
        shell.settled();
    };
    press(&mut shell, egui::Key::ArrowDown);
    let first = shell.state.selected_asset().map(|r| r.key.clone());
    assert!(first.is_some(), "Down chooses the first asset");
    press(&mut shell, egui::Key::ArrowDown);
    let second = shell.state.selected_asset().map(|r| r.key.clone());
    assert_ne!(first, second, "Down moves to the next asset");
    press(&mut shell, egui::Key::ArrowUp);
    assert_eq!(
        shell.state.selected_asset().map(|r| r.key.clone()),
        first,
        "Up moves back"
    );

    // Enter opens what is chosen: a garment opens on the Wardrobe.
    for _ in 0..shell.state.workbench.rows().len() {
        if shell
            .state
            .selected_asset()
            .is_some_and(|r| r.kept == Kept::Pack)
        {
            break;
        }
        press(&mut shell, egui::Key::ArrowDown);
    }
    assert!(
        shell
            .state
            .selected_asset()
            .is_some_and(|r| r.kept == Kept::Pack),
        "a garment was reached"
    );
    shell.state.active_workspace = Workspace::Project;
    press(&mut shell, egui::Key::Enter);
    assert_eq!(shell.state.active_workspace, Workspace::Wardrobe);
}

#[test]
fn a_second_window_on_a_project_another_holds_says_read_only_on_the_assets_panel() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    // ⚠ Made before the first, since a shell keeps its preferences, and where the lock lives, in a folder of this thread's own that the shell made last decides.
    let mut second = super::frames_of_the_shell::Shell::new(ThemeChoice::Dark);
    let first = a_project_shell(dir.path(), ThemeChoice::Dark);
    let file = first.state.project_path.clone().expect("a project");
    assert!(
        !shows(&first, "Read only"),
        "the window that writes says nothing"
    );

    second.state.load_project_from(file);
    second.state.active_workspace = Workspace::Project;
    until(&mut second, "the banner", |s| shows(s, "Read only"));
    assert!(
        shows(&second, "another OpenRage window"),
        "{:?}",
        names(&second)
    );
}

#[test]
fn the_start_page_offers_the_sample_project_and_the_tour_and_the_tour_ends_on_the_guide() {
    let _serial = crate::ui::theme::serial();
    let mut shell = super::the_front_door_on_screen::a_first_launch();
    super::the_front_door_on_screen::waits_for(&mut shell, "Open the sample project");
    assert!(shows(&shell, "Take the tour"), "{:?}", names(&shell));

    super::the_front_door_on_screen::click_on(&mut shell, "Take the tour");
    super::the_front_door_on_screen::waits_for(&mut shell, "Step 1 of 6");
    for step in 2..=6 {
        super::the_front_door_on_screen::click_on(&mut shell, "Next");
        super::the_front_door_on_screen::waits_for(&mut shell, &format!("Step {step} of 6"));
    }
    assert!(
        shows(&shell, "Open the guide"),
        "the last step goes on to the guide"
    );
    super::the_front_door_on_screen::click_on(&mut shell, "Open the guide");
    super::the_front_door_on_screen::waits_for(&mut shell, "Guide section: ");
    assert!(!shows(&shell, "Step 6 of 6"), "the tour is over");
}

#[test]
fn a_question_mark_on_problems_and_on_the_build_tab_opens_the_guide_at_the_section_that_explains_them(
) {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell.state.active_workspace = Workspace::Project;
    for (section, panel) in [
        (
            crate::ui::panels::project::GUIDE_BUILD,
            crate::ui::panels::registry::ids::BUILD,
        ),
        (
            crate::ui::panels::project::GUIDE_PROBLEMS,
            crate::ui::panels::registry::ids::PROBLEMS,
        ),
    ] {
        assert!(
            crate::ui::panels::guide::headings().contains(&section),
            "`{section}` is a heading of the guide"
        );
        shell.state.front_door.guide = None;
        shell
            .state
            .workspace_docks
            .reveal(Workspace::Project, panel);
        shell.settled();
        let help = format!("Help: {section}");
        assert!(shows(&shell, &help), "{help}: {:?}", names(&shell));
        let at = super::the_project_screens_on_screen::centre_of(&shell, &help);
        shell.click(at);
        shell.settled();
        assert_eq!(
            shell
                .state
                .front_door
                .guide
                .as_ref()
                .map(|g| g.section.as_str()),
            Some(section)
        );
    }
}
