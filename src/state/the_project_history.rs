//! The project's History: every change to what a project holds is a step kept beside the project file,
//! Undo, Redo and a rollback move along the steps, and a Save writes only what the person changed.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys, and a
//! worker for each scan and import that a case waits on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_source::asset::folder_name;
use or_source::textures;
use or_wardrobe::Wardrobe;
use serde_json::Value;

use super::front_door::{NewProject, Template};
use super::importing_game_files_from_the_app::{imported, ytd};
use super::project::ProjectDocument;
use super::{AppState, Prefs};
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;

pub(super) fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

pub(super) fn until(state: &mut AppState, what: &str, done: impl Fn(&AppState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        state.poll_workbench();
        if done(state) {
            return;
        }
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The Files project `harbor` in `dir`, opened in a state, and its project file.
pub(super) fn a_project(dir: &Path) -> (AppState, PathBuf) {
    let mut draft = NewProject::new(Template::Files, dir.to_path_buf());
    draft.set_name("harbor".into());
    let file = draft.create().expect("a project").file;
    (open(&file), file)
}

pub(super) fn open(file: &Path) -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state.load_project_from(file.to_path_buf());
    until(&mut state, "the first scan", |s| s.workbench.is_scanned());
    state
}

/// Save the project where it is and read the file back.
pub(super) fn saved(state: &mut AppState, file: &Path) -> Vec<u8> {
    assert!(state.save_active_project(|_| None));
    std::fs::read(file).expect("the project file")
}

pub(super) fn script(folder: &Path, name: &str) -> PathBuf {
    let path = folder.join(name);
    write(&path, name.as_bytes());
    path
}

pub(super) fn members(state: &AppState) -> Vec<String> {
    (state.project_carried.members.iter())
        .map(|m| m.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

pub(super) fn builds(state: &AppState) -> Vec<String> {
    (state.project_carried.container.outputs.iter())
        .map(|o| o.id.clone())
        .collect()
}

#[test]
fn undoing_add_content_puts_the_project_file_back_byte_for_byte_and_redo_does_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let before = saved(&mut state, &file);

    let added = state.add_files_to_project(vec![script(&folder, "client.lua")]);
    assert_eq!(added.added, 1);
    let after = saved(&mut state, &file);
    assert_ne!(before, after, "adding content changed the file");

    assert_eq!(state.undo_project(), Ok("Added 1 file(s)".to_string()));
    assert_eq!(saved(&mut state, &file), before, "Undo restores the bytes");
    assert_eq!(state.redo_project(), Ok("Added 1 file(s)".to_string()));
    assert_eq!(saved(&mut state, &file), after, "Redo makes them again");
}

#[test]
fn undo_is_the_project_menus_and_the_edit_menus_in_the_project_workspace_and_says_what_it_takes_back(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let ctx = egui::Context::default();
    let folder = file.parent().unwrap().to_path_buf();
    let command = |state: &AppState, action| commands::of(state, action).expect("a command");
    assert_eq!(
        command(&state, Action::UndoProject).enabled,
        Err("Nothing to undo in the project yet")
    );

    state.add_files_to_project(vec![script(&folder, "client.lua")]);
    assert_eq!(command(&state, Action::UndoProject).enabled, Ok(()));
    assert_eq!(
        command(&state, Action::UndoProject).label,
        "Undo \"Added 1 file(s)\""
    );
    assert_eq!(
        command(&state, Action::RedoProject).enabled,
        Err("Nothing to redo in the project")
    );

    // The Edit menu's Undo is the document's, except where the project is what is on screen.
    state.active_workspace = Workspace::Project;
    commands::run_action(&mut state, &ctx, Action::Undo);
    assert!(
        members(&state).is_empty(),
        "Undo in the Project workspace took the file out"
    );
    assert_eq!(command(&state, Action::RedoProject).enabled, Ok(()));
    commands::run_action(&mut state, &ctx, Action::Redo);
    assert_eq!(members(&state), ["client.lua"]);
}

#[test]
fn a_save_with_no_change_of_the_project_leaves_the_file_byte_identical_whatever_workspace_is_on_screen(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    state.active_workspace = Workspace::Project;
    let first = saved(&mut state, &file);

    state.active_workspace = Workspace::Texture;
    assert_eq!(
        saved(&mut state, &file),
        first,
        "switching workspace is not a change"
    );
    state.active_workspace = Workspace::Model;
    assert_eq!(saved(&mut state, &file), first);

    let stored: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(
        stored["workspace"],
        Value::String("Project".into()),
        "the file states the workspace it was made for"
    );
}

#[test]
fn the_workspace_a_project_file_states_changes_only_when_the_person_says_it_opens_there() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let first = saved(&mut state, &file);
    let ctx = egui::Context::default();

    state.active_workspace = Workspace::Texture;
    let command =
        |state: &AppState| commands::of(state, Action::OpenProjectHere).expect("a command");
    assert_eq!(command(&state).enabled, Ok(()));
    commands::run_action(&mut state, &ctx, Action::OpenProjectHere);
    assert!(
        state.project_carried.changed,
        "the file has a change to write"
    );
    assert_eq!(
        command(&state).enabled,
        Err("The project opens on this workspace already")
    );

    let second = saved(&mut state, &file);
    let stored: Value = serde_json::from_slice(&second).unwrap();
    assert_eq!(stored["workspace"], Value::String("Texture".into()));
    assert_ne!(first, second);

    assert!(state.undo_project().is_ok());
    assert_eq!(
        saved(&mut state, &file),
        first,
        "and Undo takes the choice back"
    );
}

#[test]
fn a_rollback_restores_an_earlier_step_and_the_steps_after_it_stay_to_be_gone_forward_to() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let declared = builds(&state);

    state.add_files_to_project(vec![script(&folder, "a.lua")]);
    let after_a = state.workbench.step_here().expect("a step");
    state
        .add_build("harbor_dlc", or_project::OutputKind::SingleplayerDlc, None)
        .expect("a build");
    state.add_files_to_project(vec![script(&folder, "b.lua")]);
    assert_eq!(members(&state), ["a.lua", "b.lua"]);
    assert_eq!(builds(&state).len(), declared.len() + 1);

    assert!(state.roll_back_project_to(after_a).is_ok());
    assert_eq!(members(&state), ["a.lua"]);
    assert_eq!(
        builds(&state),
        declared,
        "the build declared after it is gone"
    );
    assert_eq!(state.workbench.step_here(), Some(after_a));

    // Going back to the start, then forward all the way.
    let first = state
        .workbench
        .history()
        .iter()
        .find_map(|e| e.step)
        .expect("the first step");
    assert!(state.roll_back_project_to(first).is_ok());
    assert!(members(&state).is_empty());
    let last = state
        .workbench
        .history()
        .iter()
        .rev()
        .find_map(|e| e.step)
        .unwrap();
    assert!(state.roll_back_project_to(last).is_ok());
    assert_eq!(members(&state), ["a.lua", "b.lua"]);
    assert_eq!(builds(&state).len(), declared.len() + 1);

    // A new change after going back drops what was undone.
    state.roll_back_project_to(after_a).unwrap();
    state.add_files_to_project(vec![script(&folder, "c.lua")]);
    assert_eq!(members(&state), ["a.lua", "c.lua"]);
    assert!(
        state.redo_project().is_err(),
        "nothing to redo once something new is done"
    );
}

#[test]
fn the_history_of_a_project_is_there_when_it_is_opened_again_and_still_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    state.add_files_to_project(vec![script(&folder, "a.lua")]);
    state
        .add_build("harbor_dlc", or_project::OutputKind::SingleplayerDlc, None)
        .unwrap();
    assert!(state.save_active_project(|_| None));
    let before: Vec<String> = (state.workbench.history().iter())
        .filter(|e| e.step.is_some())
        .map(|e| e.text.clone())
        .collect();
    assert_eq!(
        before.len(),
        3,
        "the opened project, the file and the build: {before:?}"
    );
    drop(state);

    let mut state = open(&file);
    state.poll_workbench();
    let again: Vec<String> = (state.workbench.history().iter())
        .filter(|e| e.step.is_some())
        .map(|e| e.text.clone())
        .collect();
    assert_eq!(again, before, "the steps of the earlier session are listed");
    assert!(
        !state.project_carried.changed,
        "the file holds the last step"
    );
    assert_eq!(
        state.undo_project(),
        Ok("Added the build harbor_dlc".to_string())
    );
    assert_eq!(builds(&state).len(), 1);
    assert!(
        state.project_carried.changed,
        "and what was undone is a change to save"
    );
}

#[test]
fn the_history_folder_is_ignored_by_git_and_a_project_file_edited_outside_is_a_step_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    state.add_files_to_project(vec![script(&folder, "a.lua")]);
    let history = or_project::snapshot::history_dir(&file);
    assert!(history.join("index.json").is_file());
    assert_eq!(std::fs::read(history.join(".gitignore")).unwrap(), b"*\n");
    assert!(!saved(&mut state, &file).is_empty());
    drop(state);

    // Somebody pulls a change of the project file.
    let mut text: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    text["game"] = Value::String("enhanced".into());
    std::fs::write(&file, serde_json::to_vec_pretty(&text).unwrap()).unwrap();

    let mut state = open(&file);
    state.poll_workbench();
    let last = state
        .workbench
        .history()
        .iter()
        .rev()
        .find(|e| e.step.is_some())
        .unwrap();
    assert_eq!(last.text, "The project file changed outside OpenRage");
    let game = |state: &AppState| state.project_carried.top.get("game").cloned();
    assert_eq!(game(&state), Some(Value::String("enhanced".into())));
    assert_eq!(members(&state), ["a.lua"], "the pull changed nothing else");
    assert!(state.undo_project().is_ok());
    assert_eq!(
        game(&state),
        Some(Value::String("both".into())),
        "Undo goes back to what OpenRage last held"
    );
}

#[test]
fn removing_a_file_has_no_ten_second_button_and_undo_puts_it_back_in_its_place() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let (a, b) = (script(&folder, "a.lua"), script(&folder, "b.lua"));
    state.add_files_to_project(vec![a.clone(), b]);
    assert_eq!(members(&state), ["a.lua", "b.lua"]);

    assert!(state.remove_from_project(&a));
    assert_eq!(members(&state), ["b.lua"]);
    assert!(
        state.toasts.iter().all(|toast| toast.action.is_none()),
        "the way back is the history, which does not run out"
    );
    for _ in 0..3 {
        state.poll_workbench();
    }
    assert!(state.undo_project().is_ok());
    assert_eq!(members(&state), ["a.lua", "b.lua"]);
    assert!(state.redo_project().is_ok());
    assert_eq!(members(&state), ["b.lua"]);
}

#[test]
fn a_build_added_edited_and_removed_is_three_steps_that_undo_one_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_project(dir.path());
    let id = state
        .add_build("harbor_dlc", or_project::OutputKind::SingleplayerDlc, None)
        .expect("a build");
    state
        .edit_build(&id, "Renamed the build".into(), |o| {
            o.name = "harbor_dlc2".into()
        })
        .expect("an edit");
    state.remove_build(&id);
    assert_eq!(builds(&state).len(), 1);

    assert!(state.undo_project().is_ok());
    assert_eq!(
        state.project_carried.container.output(&id).unwrap().name,
        "harbor_dlc2"
    );
    assert!(state.undo_project().is_ok());
    assert_eq!(
        state.project_carried.container.output(&id).unwrap().name,
        "harbor_dlc"
    );
    assert!(state.undo_project().is_ok());
    assert!(state.project_carried.container.output(&id).is_none());
    assert_eq!(
        state.redo_project(),
        Ok("Added the build harbor_dlc".to_string())
    );
}

#[test]
fn a_drag_of_a_ceiling_is_one_step_whatever_the_number_of_frames() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_project(dir.path());
    let id = state.project_carried.container.outputs[0].id.clone();
    let steps = |state: &AppState| {
        state
            .workbench
            .history()
            .iter()
            .filter(|e| e.step.is_some())
            .count()
    };
    let before = steps(&state);

    for bytes in [10u64, 20, 30, 40, 50] {
        state
            .edit_build_live(&id, "bytes", format!("Capped {id} at {bytes} bytes"), |o| {
                o.max_bytes = Some(bytes)
            })
            .unwrap();
    }
    assert_eq!(
        steps(&state),
        before,
        "nothing is said while the number moves"
    );
    state.settle_build_edit();
    assert_eq!(
        steps(&state),
        before + 2,
        "the project as it was opened, and the drag"
    );
    assert_eq!(
        state.project_carried.container.outputs[0].max_bytes,
        Some(50)
    );

    assert_eq!(state.undo_project(), Ok(format!("Capped {id} at 50 bytes")));
    assert_eq!(state.project_carried.container.outputs[0].max_bytes, None);
}

#[test]
fn a_drag_still_going_when_undo_is_pressed_is_settled_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    state.add_files_to_project(vec![script(&folder, "a.lua")]);
    let id = state.project_carried.container.outputs[0].id.clone();
    state
        .edit_build_live(&id, "files", "Capped the files at 7".into(), |o| {
            o.max_files = Some(7)
        })
        .unwrap();

    assert_eq!(
        state.undo_project(),
        Ok("Capped the files at 7".to_string())
    );
    assert_eq!(state.project_carried.container.outputs[0].max_files, None);
    assert_eq!(members(&state), ["a.lua"], "and the file is still there");
}

/// A pack another project holds, and the open project that links it.
fn linked(dir: &Path) -> (AppState, PathBuf, PathBuf) {
    let pack = dir
        .join("shared")
        .join("brand")
        .join("modules")
        .join("uniforms");
    write(&pack.join("models/m.ydd"), b"m");
    Wardrobe::new("brand").write(&pack).unwrap();
    write(
        &dir.join("shared").join("brand").join("brand.openrage-proj"),
        b"{}",
    );
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut draft = NewProject::new(Template::Files, work);
    draft.set_name("police".into());
    let file = draft.create().expect("police is made").file;
    (open(&file), file, pack)
}

#[test]
fn a_link_an_unlink_and_a_locate_are_steps_that_undo_and_redo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file, pack) = linked(dir.path());
    let folder = file.parent().unwrap().to_path_buf();

    let id = state.link_content(&pack).expect("a link");
    let linked_modules = |state: &AppState| -> Vec<String> {
        (state.project_carried.container.modules.iter())
            .filter(|m| m.is_link())
            .map(|m| m.id.clone())
            .collect()
    };
    assert_eq!(linked_modules(&state), std::slice::from_ref(&id));
    assert!(state.project_carried.container.outputs[0]
        .take
        .contains(&id));

    assert!(state.undo_project().is_ok());
    assert!(linked_modules(&state).is_empty(), "Undo unlinks");
    assert!(!state.project_carried.container.outputs[0]
        .take
        .contains(&id));
    assert!(state.redo_project().is_ok());
    assert_eq!(
        linked_modules(&state),
        std::slice::from_ref(&id),
        "Redo links again"
    );

    state.unlink_content(&id).expect("unlinked");
    assert!(linked_modules(&state).is_empty());
    assert!(state.undo_project().is_ok());
    assert_eq!(
        linked_modules(&state),
        [id],
        "Undo of an unlink links it back"
    );

    let (old, new) = (script(&folder, "old.lua"), script(&folder, "new.lua"));
    state.add_files_to_project(vec![old.clone()]);
    state.locate_member(&old, new);
    assert_eq!(members(&state), ["new.lua"]);
    assert!(state.undo_project().is_ok());
    assert_eq!(members(&state), ["old.lua"], "Undo of a locate points back");
    assert!(state.redo_project().is_ok());
    assert_eq!(members(&state), ["new.lua"]);
}

#[test]
fn an_import_is_a_step_of_the_same_stack_which_undo_takes_back_and_redo_makes_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let picked = dir.path().join("downloads/road.ytd");
    write(&picked, &ytd("road", 3));

    state.import_game_files(vec![picked]);
    imported(&mut state);
    let made = folder
        .join("sources")
        .join(folder_name("road", textures::KIND));
    assert!(made.is_dir());
    let last = state
        .workbench
        .history()
        .iter()
        .rev()
        .find(|e| e.step.is_some())
        .unwrap();
    assert!(
        last.text.starts_with("Imported 1 file: 1 became a source"),
        "{}",
        last.text
    );

    let said = state.undo_project().expect("the import is taken back");
    assert!(
        said.starts_with("Imported 1 file: 1 became a source"),
        "{said}"
    );
    assert!(!made.exists(), "Undo removed what the import made");

    let redone = state.redo_project().expect("the step is gone forward to");
    assert!(
        redone.starts_with("Imported 1 file: 1 became a source"),
        "{redone}"
    );
    assert!(made.is_dir(), "Redo makes the source again");
    assert!(
        !state
            .toasts
            .iter()
            .any(|toast| toast.message.contains("Import game files")),
        "and has nothing to say: {:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
    assert_eq!(
        state
            .workbench
            .step_here()
            .and_then(|n| state.workbench.label_of(n)),
        Some(redone),
        "the project stands at the step it was redone to"
    );
}

#[test]
fn a_project_file_carries_no_absolute_path_of_this_machine_and_the_files_it_cannot_name_come_back()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let inside = script(&folder, "client.lua");
    let outside = dir.path().join("downloads").join("hat.ydd");
    write(&outside, b"hat");
    state.add_files_to_project(vec![inside]);
    state
        .project_carried
        .members
        .push(ProjectDocument::new(outside.clone(), None));
    state.project_carried.changed = true;

    let bytes = saved(&mut state, &file);
    let text = String::from_utf8(bytes).unwrap();
    for root in [dir.path(), &folder] {
        let shown = root.to_string_lossy();
        for form in [
            shown.to_string(),
            shown.replace('\\', "/"),
            shown.replace('\\', "\\\\"),
        ] {
            assert!(
                !text.contains(&form),
                "{form} is in the shared file:\n{text}"
            );
        }
    }
    assert!(!text.contains("hat.ydd"), "{text}");
    assert!(text.contains("client.lua"));

    // This machine still holds it, beside the project and out of Git.
    let again = open(&file);
    assert!(
        again.member_paths().contains(&outside),
        "{:?}",
        again.member_paths()
    );
    let machine = or_project::snapshot::private_dir(&file).join("machine.json");
    assert!(machine.is_file());
    assert_eq!(
        std::fs::read(or_project::snapshot::private_dir(&file).join(".gitignore")).unwrap(),
        b"*\n"
    );
}

#[test]
fn a_save_that_changes_nothing_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    saved(&mut state, &file);
    let then = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(30));
    saved(&mut state, &file);
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        then,
        "the file was not written again, so a repository and an editor see no change"
    );
}

#[test]
fn undoing_back_to_what_was_saved_leaves_nothing_unsaved() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    saved(&mut state, &file);
    assert!(!state.project_carried.changed);

    state.add_files_to_project(vec![script(&folder, "a.lua")]);
    assert!(state.project_carried.changed);
    state.undo_project().unwrap();
    assert!(
        !state.project_carried.changed,
        "the file on disk holds this step"
    );
    state.redo_project().unwrap();
    assert!(state.project_carried.changed);
    saved(&mut state, &file);
    assert!(!state.project_carried.changed);
    state.undo_project().unwrap();
    assert!(
        state.project_carried.changed,
        "the file holds the step after this one"
    );
}

#[test]
fn a_pinned_button_a_clothing_content_and_a_state_file_type_are_steps_too() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let before = state.project_carried.shown.clone();
    state.toggle_pinned(Workspace::Texture);
    assert_ne!(state.project_carried.shown, before);
    state.undo_project().unwrap();
    assert_eq!(state.project_carried.shown, before);

    state.add_files_to_project(vec![script(&folder, "a.meta")]);
    state
        .state_file_type("a.meta", Some("VEHICLE_METADATA_FILE"))
        .expect("a type");
    assert!(state
        .own_files_content()
        .unwrap()
        .file_types()
        .contains_key("a.meta"));
    state.undo_project().unwrap();
    assert!(state.own_files_content().unwrap().file_types().is_empty());
}

#[test]
fn a_build_still_writing_when_its_project_is_closed_ends_for_that_project_and_reports_to_no_other()
{
    use or_project::{Container, Output, Project};

    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("big_pack");
    // Big enough that the build is still writing when the project is closed.
    let big = folder.join("stream/big.bin");
    write(&big, &vec![7u8; 96 * 1024 * 1024]);
    let mut container = Container::default();
    container.outputs.push(or_project::Output::fivem_resource(
        "fivem",
        "big_pack",
        &["files"],
    ));
    let file = folder.join("big_pack.openrage-proj");
    Project::create(&file, &container, &[big]).expect("a new project");
    let tiny = dir.path().join("tiny");
    let mut small = Container::default();
    small
        .outputs
        .push(Output::fivem_resource("fivem", "tiny", &["files"]));
    let tiny_file = tiny.join("tiny.openrage-proj");
    Project::create(&tiny_file, &small, &[]).expect("a second project");

    let mut state = open(&file);
    state.build_selected();
    assert!(state.workbench.builds().is_running());

    state.close_project();
    state.poll_workbench();
    assert_eq!(
        state.project_path, None,
        "Close project did not wait for the build"
    );
    assert!(state.workbench.builds().is_running(), "the worker goes on");

    state.load_project_from(tiny_file);
    until(&mut state, "the second project's scan", |s| {
        s.workbench.is_scanned()
    });
    until(&mut state, "the build to end", |s| {
        !s.workbench.builds().is_running()
    });

    assert!(
        state.workbench.builds().last("fivem").is_none(),
        "the result of the closed project's build is not the open project's"
    );
    assert!(
        state
            .workbench
            .history()
            .iter()
            .all(|e| !e.text.starts_with("Built fivem") && !e.text.starts_with("Built big_pack")),
        "{:?}",
        state.workbench.history()
    );
    assert!(
        state.toasts.iter().any(|t| t.message.contains("big_pack")),
        "it is said where it ended: {:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
    // It stood: the resource is whole, and no half-written folder is left beside it.
    let built = folder.join("build/big_pack");
    assert!(built.join("stream/big.bin").is_file(), "{built:?}");
    let hidden: Vec<_> = std::fs::read_dir(folder.join("build"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".new"))
        .collect();
    assert!(hidden.is_empty(), "{hidden:?}");
}

#[test]
fn a_project_closed_after_an_undo_that_was_not_saved_opens_at_the_step_its_file_holds() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    state.add_files_to_project(vec![script(&folder, "a.lua")]);
    state
        .add_build("harbor_dlc", or_project::OutputKind::SingleplayerDlc, None)
        .unwrap();
    assert!(state.save_active_project(|_| None));
    state.undo_project().unwrap();
    // The person closes without saving: the file holds the build, the history stands before it.
    drop(state);

    let state = open(&file);
    assert_eq!(builds(&state).len(), 2, "the file holds the build");
    assert!(!state.project_carried.changed);
    assert_eq!(
        state
            .workbench
            .step_here()
            .and_then(|n| state.workbench.label_of(n)),
        Some("Added the build harbor_dlc".to_string()),
        "the history stands where the file does"
    );
    assert!(
        (state.workbench.history().iter())
            .all(|e| e.text != "The project file changed outside OpenRage"),
        "an undo that was never saved is no change made outside"
    );
}
