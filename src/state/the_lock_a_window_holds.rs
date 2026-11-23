//! The window that writes a project is the one that holds its lock, however the project came to be this window's, and a window
//! that only reads one starts no source and replaces no geometry in it.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys. A second window is a second
//! `AppState` in the same process, which the system's lock tells apart from the first.

use std::path::{Path, PathBuf};

use or_source::sample::drawable_dictionary;

use super::importing_game_files_from_the_app::{imported, project, until};
use super::the_project_workspace::opened;
use super::{AppState, Prefs};
use crate::ui::dock::Workspace;

/// A window with nothing open, which loads the project at `file`.
fn a_window_on(file: &Path) -> AppState {
    let mut window = AppState::from_prefs(Prefs::default());
    window.documents.clear();
    window.load_project_from(file.to_path_buf());
    window
}

fn a_copy_path(dir: &Path, name: &str) -> PathBuf {
    let folder = dir.join(name);
    std::fs::create_dir_all(&folder).unwrap();
    folder.join(format!("{name}.openrage-proj"))
}

#[test]
fn saving_a_project_as_another_file_leaves_this_window_the_one_that_writes_the_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (mut first, _) = opened(dir.path());
    assert!(
        first.read_only_reason().is_none(),
        "the first window writes"
    );
    let copy = a_copy_path(dir.path(), "copy");

    first.save_project_to(copy.clone()).expect("saved as");
    first.poll_workbench();
    assert_eq!(first.project_path.as_ref(), Some(&copy));
    assert!(first.read_only_reason().is_none(), "and still writes");

    let second = a_window_on(&copy);
    assert!(
        second.read_only_reason().is_some(),
        "the first window saved as this project and still writes it, so a second window must read only"
    );
}

#[test]
fn a_session_saved_as_a_new_project_is_written_by_this_window_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = AppState::from_prefs(Prefs::default());
    first.documents.clear();
    assert!(first.project_path.is_none());
    let file = a_copy_path(dir.path(), "made");

    first.save_project_to(file.clone()).expect("a new project");
    first.poll_workbench();

    assert!(
        a_window_on(&file).read_only_reason().is_some(),
        "the window that made the project writes it"
    );
}

#[test]
fn a_window_that_only_reads_a_project_and_saves_a_copy_writes_the_copy_and_leaves_the_project_to_its_writer(
) {
    let dir = tempfile::tempdir().unwrap();
    let (writer, file) = opened(dir.path());
    let mut reader = a_window_on(&file);
    assert!(reader.read_only_reason().is_some());
    let copy = a_copy_path(dir.path(), "copy");

    reader.save_project_to(copy.clone()).expect("saved as");
    reader.poll_workbench();

    assert!(
        reader.read_only_reason().is_none(),
        "nobody else holds the copy"
    );
    assert!(
        a_window_on(&copy).read_only_reason().is_some(),
        "the reader is the writer of the copy now"
    );
    assert!(
        a_window_on(&file).read_only_reason().is_some(),
        "and the first window still holds the project it opened"
    );
    drop(writer);
}

/// The writer of a project that holds a drawables source, and a window that only reads the same project, its folder and the row of the source.
struct Held {
    _writer: AppState,
    reader: AppState,
    folder: PathBuf,
    model: crate::state::workbench::AssetRow,
    scratch: tempfile::TempDir,
}

fn a_project_with_a_model_read_by_a_second_window() -> Held {
    let scratch = tempfile::tempdir().unwrap();
    let (mut writer, folder) = project(scratch.path());
    let ydd = scratch.path().join("downloads/scarf.ydd");
    std::fs::create_dir_all(ydd.parent().unwrap()).unwrap();
    std::fs::write(&ydd, drawable_dictionary("scarf", 6, 0.0)).unwrap();
    writer.import_game_files(vec![ydd]);
    imported(&mut writer);

    let mut reader = a_window_on(&folder.join("police_pack.openrage-proj"));
    until(&mut reader, "the scan", |s| s.workbench.is_scanned());
    assert!(
        reader.read_only_reason().is_some(),
        "the setup: it only reads"
    );
    let model = (reader.workbench.rows().iter())
        .find(|r| r.source_kind.as_deref() == Some("drawables"))
        .expect("the imported dictionary is a row")
        .clone();
    Held {
        _writer: writer,
        reader,
        folder,
        model,
        scratch,
    }
}

#[test]
fn a_window_that_only_reads_a_project_greys_new_source_and_replace_geometry() {
    let held = a_project_with_a_model_read_by_a_second_window();

    assert!(held.reader.new_source_refused().is_some(), "New");
    assert!(
        held.reader.replace_geometry_refused(&held.model).is_some(),
        "Replace geometry"
    );
}

#[test]
fn a_window_that_only_reads_a_project_makes_no_source_folder_when_asked_for_one() {
    let mut held = a_project_with_a_model_read_by_a_second_window();
    let before = sources(&held.folder);

    let made = held.reader.create_source("textures", "intruder");

    assert!(made.is_err(), "{made:?}");
    assert_eq!(sources(&held.folder), before, "the folder is as it was");
    assert!(!held.folder.join("sources/intruder.textures.src").exists());
}

#[test]
fn a_window_that_only_reads_a_project_sends_no_model_into_a_source() {
    let mut held = a_project_with_a_model_read_by_a_second_window();
    let before = sources(&held.folder);
    let glb = held.scratch.path().join("model.glb");

    held.reader
        .start_replacing_geometry(&held.model.key, glb.clone());
    held.reader.replace_geometry(&held.model.key, None, glb);

    assert!(
        !held.reader.workbench.geometry.is_running(),
        "no worker was sent into the source"
    );
    assert_eq!(sources(&held.folder), before, "and the folder is as it was");
}

/// Every file under the project's sources folder with its size, sorted.
fn sources(folder: &Path) -> Vec<(PathBuf, u64)> {
    fn walk(at: &Path, found: &mut Vec<(PathBuf, u64)>) {
        for entry in std::fs::read_dir(at).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else {
                found.push((path, entry.metadata().map_or(0, |m| m.len())));
            }
        }
    }
    let mut found = Vec::new();
    walk(&folder.join("sources"), &mut found);
    found.sort();
    found
}

#[test]
fn the_workspace_a_project_was_left_on_is_kept_in_a_folder_that_ignores_itself() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.poll_workbench();
    state.active_workspace = if state.active_workspace == Workspace::Texture {
        Workspace::Model
    } else {
        Workspace::Texture
    };
    state.poll_workbench();

    let private = or_project::snapshot::private_dir(&file);
    assert!(private.join("workspace.json").is_file(), "the setup");
    assert_eq!(
        std::fs::read(private.join(".gitignore")).ok().as_deref(),
        Some(b"*\n".as_slice()),
        "workspace.json is written in {} with no .gitignore beside it",
        private.display()
    );
}

#[test]
fn a_window_that_only_reads_refuses_an_import_in_the_language_in_use() {
    let dir = tempfile::tempdir().unwrap();
    let (_writer, file) = opened(dir.path());
    let reader = a_window_on(&file);
    assert!(reader.read_only_reason().is_some());

    let english = reader.import_refused().expect("a refusal");
    assert!(english.contains("another OpenRage window"), "{english}");
    let _french = crate::i18n::pin("fr");
    let french = reader.import_refused().expect("a refusal");
    assert!(
        !french.contains("another OpenRage window"),
        "the refusal is English in French: {french}"
    );
}
