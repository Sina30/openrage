//! The Build tab and the Builds window as a person reads them: why a build stopped beside the word Failed, the verdict of a good one
//! before the plan, a Cancel while it runs, the matrix before the form, a name the form accepts, and no engine word or raw byte count.
//!
//! Read through AccessKit from whole frames of the shell, in the smallest window the editor has.

use std::time::{Duration, Instant};

use super::frames_of_the_shell::Shell;
use super::the_build_tab_on_screen::a_files_project;
use super::the_front_door_on_screen::a_project;
use super::the_project_screens_on_screen::{a_project_shell, names, shows, until};
use crate::state::front_door::Template;
use crate::state::workbench::{LastBuild, NewBuild, Question, Scope};
use crate::state::ThemeChoice;
use crate::ui::commands;
use crate::ui::dock::Workspace;
use crate::ui::panels::registry::ids;
use or_project::OutputKind;

const OUTPUT: &str = "fivem";
const SMALLEST: (u32, u32) = (1280, 720);

fn y_of(shell: &Shell, name: &str) -> f32 {
    (shell.described.iter())
        .filter(|w| w.name.as_deref() == Some(name))
        .map(|w| w.bounds.center().y)
        .min_by(f32::total_cmp)
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
}

fn y_of_containing(shell: &Shell, text: &str) -> f32 {
    (shell.described.iter())
        .filter(|w| w.name.as_deref().is_some_and(|n| n.contains(text)))
        .map(|w| w.bounds.center().y)
        .min_by(f32::total_cmp)
        .unwrap_or_else(|| panic!("nothing shows `{text}`: {:?}", names(shell)))
}

fn disabled(shell: &Shell, name: &str) -> bool {
    (shell.described.iter())
        .find(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n == name || n.starts_with(name))
        })
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
        .disabled
}

/// Draw frames, polling what the worker said, until the build of `OUTPUT` has ended.
fn until_built(shell: &mut Shell) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while shell.state.workbench.builds().last(OUTPUT).is_none() {
        assert!(Instant::now() < deadline, "the build never ended");
        shell.state.poll_workbench();
        shell.draw(Vec::new());
        std::thread::sleep(Duration::from_millis(5));
    }
    shell.settled_sized(SMALLEST);
}

#[test]
fn a_build_that_stopped_says_where_and_why_beside_the_word_failed_and_marks_the_step() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_files_project(dir.path());
    // A ceiling the plan breaks: the build is refused before it writes anything.
    shell
        .state
        .edit_build(OUTPUT, "limit".into(), |o| o.max_files = Some(0))
        .expect("an edit");
    until(&mut shell, "the plan's problem", |s| {
        s.state.workbench.counts(Scope::Build(OUTPUT)).errors > 0
    });
    shell.state.build_selected();
    until_built(&mut shell);

    let Some(LastBuild::Failed(reasons)) = shell.state.workbench.builds().last(OUTPUT).cloned()
    else {
        panic!(
            "the build did not fail: {:?}",
            shell.state.workbench.builds().last(OUTPUT)
        );
    };
    let said = names(&shell);
    // The reason as a person reads it: the crates that refuse a build write for a document, with backticks.
    let reason = crate::state::front_door::spoken(&reasons[0]);
    for text in [
        "The build stopped at step 1, Plan.",
        "The last good build is as it was.",
        reason.as_str(),
        &format!("{} 1 Plan", egui_phosphor::regular::X),
    ] {
        assert!(shows(&shell, text), "the tab lacks `{text}`: {said:?}");
    }
    assert!(
        y_of_containing(&shell, &reason) < y_of(&shell, "Plan"),
        "the reason is below the plan, three screens from the word Failed"
    );
}

#[test]
fn a_good_build_says_its_verdict_first_folds_the_plan_and_puts_the_size_map_last() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_files_project(dir.path());
    shell.settled_sized(SMALLEST);
    assert!(
        shows(&shell, "Files: "),
        "before a build the plan is open: {:?}",
        names(&shell)
    );

    shell.state.build_selected();
    until_built(&mut shell);
    shell.settled_sized(SMALLEST);

    let said = names(&shell);
    assert!(
        said.iter()
            .any(|n| n.contains("written and read back")
                && n.starts_with(egui_phosphor::regular::CHECK)),
        "no verdict: {said:?}"
    );
    assert!(shows(&shell, "loads in game: not tested"), "{said:?}");
    assert!(
        !shows(&shell, "Files: "),
        "the plan is still open after a build: {said:?}"
    );
    let (changed, verified, map) = (
        y_of(&shell, "What changed"),
        y_of(&shell, "Verification"),
        y_of(&shell, "Size map"),
    );
    assert!(
        changed < verified && verified < map,
        "{changed} {verified} {map}"
    );
    assert!(
        !said.iter().any(|n| n.contains(". Since the last build")),
        "the counts are glued onto the totals line: {said:?}"
    );
}

#[test]
fn a_running_build_is_stopped_from_the_tab_and_the_steps_behind_it_are_ticked() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_files_project(dir.path());
    shell.state.build_selected();
    // ⚠ Not polled: what the worker said stays on screen, as a build that is still running reads.
    let file = shell.state.project_path.clone().expect("a project");
    let live = or_project::project_folder(&file).join("build/shopcase/fxmanifest.lua");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !live.is_file() {
        assert!(Instant::now() < deadline, "the build never swapped in");
        shell.draw(Vec::new());
        std::thread::sleep(Duration::from_millis(5));
    }
    shell.settled_sized(SMALLEST);

    let said = names(&shell);
    let tick = egui_phosphor::regular::CHECK;
    for text in [
        "Cancel build",
        &format!("{tick} 1 Plan"),
        &format!("{tick} 4 Read every file back"),
        "▶ 5 Swap it in",
    ] {
        assert!(
            shows(&shell, text),
            "the tab lacks `{text}` while it builds: {said:?}"
        );
    }
    assert!(
        disabled(&shell, "Edit build"),
        "Edit build… is offered while a build runs, as Remove build is not"
    );
    super::the_wardrobe_operations_on_screen::click_on(&mut shell, "Cancel build");
    assert!(
        shell.state.workbench.builds().is_stopping(),
        "Cancel build stopped nothing"
    );
}

#[test]
fn a_finished_build_offers_the_folder_in_a_button_and_in_its_toast_in_plain_words() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_files_project(dir.path());
    shell.state.build_selected();
    until_built(&mut shell);

    assert!(
        commands::all(&shell.state)
            .iter()
            .any(|c| c.label == "Show the build"),
        "no command shows the build"
    );
    assert!(shows(&shell, "Show the build"), "{:?}", names(&shell));
    let Some(LastBuild::Built(done)) = shell.state.workbench.builds().last(OUTPUT).cloned() else {
        panic!("the build did not go through");
    };
    let toast = shell.state.toasts.iter().last().expect("a toast").clone();
    let said = format!(
        "Built shopcase: {}, all read back",
        crate::state::front_door::counted(done.files() as usize, "file")
    );
    assert!(toast.message.starts_with(&said), "{}", toast.message);
    assert!(
        !toast.message.contains("reader") && !toast.message.contains("(s)"),
        "{}",
        toast.message
    );
    assert_eq!(toast.action.map(|a| a.label()), Some("Show build"));
}

/// A project of loose files that declares a second build, a singleplayer DLC.
fn with_a_dlc(dir: &std::path::Path) -> Shell {
    let mut shell = a_files_project(dir);
    shell
        .state
        .add_build_taking(
            "shopcase_dlc",
            OutputKind::SingleplayerDlc,
            None,
            &["files".into()],
        )
        .expect("a build");
    until(&mut shell, "the lock", |s| {
        s.state
            .workbench
            .cannot_ship("files", &OutputKind::SingleplayerDlc)
            .is_some()
    });
    shell
}

#[test]
fn the_builds_window_shows_the_matrix_before_the_form_folds_the_limits_and_shows_one_form_to_add() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_a_dlc(dir.path());
    shell.state.ask_about_builds(false);
    shell.settled_sized(SMALLEST);

    assert!(
        y_of(&shell, "Content by builds")
            < y_of(&shell, "Divide into several resources when over a ceiling"),
        "the matrix, which this window is for, is below a form: {:?}",
        names(&shell)
    );
    assert!(
        !shows(&shell, "Files at most"),
        "twelve limits nobody set open the window: {:?}",
        names(&shell)
    );
    assert!(shows(&shell, "Limits"), "{:?}", names(&shell));

    shell.state.ask_about_builds(true);
    shell.settled_sized(SMALLEST);
    for gone in [
        "Divide into several resources",
        "Content by builds",
        "Remove build",
    ] {
        assert!(
            !shows(&shell, gone),
            "`{gone}` is drawn over the form of a new build"
        );
    }
    for text in ["Add build", "Cancel"] {
        assert!(shows(&shell, text), "{text}: {:?}", names(&shell));
    }
}

#[test]
fn a_lock_is_said_in_plain_words_with_no_markup_and_the_window_offers_the_way_to_the_types() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_a_dlc(dir.path());
    let why = shell
        .state
        .workbench
        .cannot_ship("files", &OutputKind::SingleplayerDlc)
        .expect("a lock")
        .to_string();
    for jargon in ["`", "fileType", "file_types", "data_file"] {
        assert!(!why.contains(jargon), "the lock says `{jargon}`: {why}");
    }
    assert!(
        why.contains("File types") && why.contains("client.lua"),
        "{why}"
    );

    shell.state.ask_about_builds(false);
    shell.settled_sized(SMALLEST);
    assert!(shows(&shell, "Go to File types"), "{:?}", names(&shell));
}

#[test]
fn a_name_offered_for_a_new_build_is_one_the_form_accepts_and_the_form_calls_it_name() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let file = a_project(dir.path(), "street-wear", Template::Files);
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Project;
    shell.settled_sized(SMALLEST);

    let kind = OutputKind::SingleplayerDlc;
    let offered = shell.state.default_build_name(&kind, None);
    assert!(
        !offered.is_empty()
            && offered
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "`{offered}` is refused by the form that offers it"
    );
    let mut draft: NewBuild = shell.state.new_build_draft();
    draft.kind = kind;
    draft.name = offered;
    shell.state.workbench.asking = Some(Question::Builds {
        chosen: None,
        adding: Some(draft),
    });
    shell.settled_sized(SMALLEST);
    assert!(
        !disabled(&shell, "Add build"),
        "the first Add build does nothing"
    );
    assert!(!shows(&shell, "device name"), "{:?}", names(&shell));
    assert!(
        !shows(&shell, "Resource name"),
        "one thing, one name: {:?}",
        names(&shell)
    );
    assert!(shows(&shell, "Written to build/"), "{:?}", names(&shell));
}

#[test]
fn the_plan_says_memory_in_the_units_of_the_size_one_line_of_limits_and_words_for_each_file() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell
        .state
        .workspace_docks
        .reveal(Workspace::Project, ids::BUILD);
    shell.settled_sized(SMALLEST);

    let said = names(&shell);
    for row in said.iter().filter(|n| n.contains("memory")) {
        let digits = row.chars().filter(char::is_ascii_digit).count();
        assert!(
            digits < 7 || row.contains("MiB") || row.contains("KiB"),
            "memory in raw bytes next to sizes in MiB: {row}"
        );
    }
    assert!(!shows(&shell, "(no limit set)"), "{said:?}");
    assert!(shows(&shell, "No limits set"), "{said:?}");
    assert!(
        !said
            .iter()
            .any(|n| n.starts_with("cook ") || n.starts_with("retarget ")),
        "the file column speaks the engine's words: {said:?}"
    );
    // A file is named by its own name first, where the rows differ.
    let first = shell.state.workbench.resources_of(OUTPUT)[0].plan.files[0]
        .path
        .clone();
    let leaf = first.rsplit('/').next().unwrap();
    assert!(
        said.iter().any(|n| n.starts_with(leaf)),
        "a row does not start with `{leaf}`: {said:?}"
    );
}
