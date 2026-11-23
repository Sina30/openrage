//! The Wardrobe is offered only while the project holds clothing, and a window on it when the last clothing goes is moved to
//! the Project workspace, not left drawing an empty Wardrobe; Undo brings the clothing back and the window with it.
//!
//! ⚠ Built with `AppState::from_prefs` and a clothing project in a scratch folder.

use or_project::ModuleKind;

use super::the_wardrobe_workspace::{opened, until};
use super::AppState;
use crate::ui::dock::Workspace;

/// Take the clothing content out of the project the way a step of the project's history keeps it.
fn take_the_clothing_out(state: &mut AppState) {
    let container = &mut state.project_carried.container;
    let clothing: Vec<String> = (container.modules.iter())
        .filter(|m| m.kind == ModuleKind::Clothing)
        .map(|m| m.id.clone())
        .collect();
    container.modules.retain(|m| m.kind != ModuleKind::Clothing);
    for output in &mut container.outputs {
        output.take.retain(|t| !clothing.contains(t));
    }
    state.record_step("Took the clothing out of the project");
}

#[test]
fn the_wardrobe_is_left_for_the_project_when_the_last_clothing_goes_and_undo_brings_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    assert_eq!(state.active_workspace, Workspace::Wardrobe);
    assert!(state.has_clothing());

    take_the_clothing_out(&mut state);
    assert!(!state.has_clothing());
    state.poll_workbench();
    assert!(!state.workspaces_offered().contains(&Workspace::Wardrobe));
    assert_eq!(
        state.active_workspace,
        Workspace::Project,
        "no empty Wardrobe is left on screen"
    );

    state.undo_project().expect("the step is taken back");
    assert!(state.has_clothing());
    state.poll_workbench();
    assert_eq!(
        state.active_workspace,
        Workspace::Wardrobe,
        "the clothing comes back and so does the window that was on it"
    );
}

#[test]
fn a_person_who_went_elsewhere_while_the_clothing_was_gone_is_not_taken_back_to_the_wardrobe() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());

    take_the_clothing_out(&mut state);
    state.poll_workbench();
    assert_eq!(state.active_workspace, Workspace::Project);
    state.active_workspace = Workspace::Texture;
    state.poll_workbench();

    state.undo_project().expect("the step is taken back");
    state.poll_workbench();
    assert_eq!(
        state.active_workspace,
        Workspace::Texture,
        "the window stays where the person put it"
    );
}

#[test]
fn a_window_that_was_never_on_the_wardrobe_stays_where_it_is_when_clothing_comes_and_goes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    state.active_workspace = Workspace::Model;

    take_the_clothing_out(&mut state);
    state.poll_workbench();
    assert_eq!(state.active_workspace, Workspace::Model);
    state.undo_project().expect("the step is taken back");
    state.poll_workbench();
    assert_eq!(state.active_workspace, Workspace::Model);
}
