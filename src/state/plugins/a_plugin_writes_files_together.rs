//! A plugin that writes several files of the project writes all of them or none, as one step of the
//! project's History: Undo takes every file back together and Redo puts them all back, a file changed
//! since is not overwritten, and a refused set or a write that fails leaves the project as it was.
//!
//! ⚠ The guest is the sample built with its `writes` feature by a nested `cargo build` for
//! wasm32-unknown-unknown, as the other plugin suites build theirs; it writes `notes/first.txt` and
//! `notes/second.txt` when it runs on the open document.

use std::path::{Path, PathBuf};

use or_plugin::{Capability, FileWrite, Manifest, Transaction, ABI_VERSION};

use super::{Consent, InstalledPlugin};
use crate::state::a_plugin_asks_before_it_runs::built_sample;
use crate::state::importing_game_files_from_the_app::project;
use crate::state::{AppState, Document, DocumentKind};

const ID: &str = "writer";

fn guest() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| built_sample("writes", "openrage-plugin-sample-writes"))
        .clone()
}

fn writer() -> InstalledPlugin {
    InstalledPlugin {
        manifest: Manifest {
            id: ID.into(),
            name: "Writer".into(),
            abi: ABI_VERSION,
            wants: vec![Capability::WriteFiles],
        },
        wasm: guest(),
        refused: None,
    }
}

/// A blank declared resource open, which is the document a plugin of the `go` kind runs on.
fn blank(id: u64) -> Document {
    let kind = DocumentKind::Ybn;
    let resource = kind.resource_type().expect("a paged format");
    let handler = crate::state::payload_handlers::handler_for(resource, or_io::Generation::Gen8)
        .expect("a payload handler");
    let payload = (handler.empty)();
    let bytes = handler.save(&payload).expect("a blank serializes");
    // A path that names no folder of any install, and nothing is written there.
    let path = PathBuf::from(format!("writer{id}.ybn"));
    let mut doc = Document::new(id, Some(path), payload);
    doc.set_resource(crate::state::document_io::resource_image_for(kind, &bytes));
    doc
}

/// A project in `dir` and a document open, with the writer installed and answered yes for.
fn state_in(dir: &Path) -> (AppState, PathBuf) {
    let (mut state, folder) = project(dir);
    let plugin = writer();
    state.plugins.scanned = Some(vec![plugin.clone()]);
    state.prefs.plugin_consent.insert(
        ID.into(),
        Consent::Granted(
            plugin
                .manifest
                .wants
                .iter()
                .map(|c| c.key().to_string())
                .collect(),
        ),
    );
    state.documents.push(blank(1));
    state.active_document = Some(1);
    (state, folder)
}

fn keys(state: &AppState) -> Vec<String> {
    (state.plugin_report(ID).unwrap_or_default().iter())
        .map(|d| d.key.to_string())
        .collect()
}

fn read(folder: &Path, path: &str) -> Option<Vec<u8>> {
    std::fs::read(folder.join(path)).ok()
}

#[test]
fn the_files_a_plugin_writes_together_are_one_step_that_undo_takes_back_and_redo_puts_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = state_in(dir.path());
    let steps_before = state.workbench.history().len();

    state.request_plugin_run(ID);

    assert_eq!(
        read(&folder, "notes/first.txt").as_deref(),
        Some(&b"one"[..])
    );
    assert_eq!(
        read(&folder, "notes/second.txt").as_deref(),
        Some(&b"two"[..])
    );
    let keys = keys(&state);
    assert!(keys.contains(&"plugin.wrote".to_string()), "{keys:?}");
    assert!(
        !keys.contains(&"plugin.unchanged".to_string()),
        "a run that wrote files did not change nothing: {keys:?}"
    );
    assert_eq!(
        state.project_undo_label().as_deref(),
        Some("Writer wrote 2 files")
    );
    assert!(
        state.workbench.history().len() <= steps_before + 2,
        "the files are one step (the first step of a history is the project as it was opened)"
    );

    let undone = state.undo_project().expect("one Undo");
    assert_eq!(undone, "Writer wrote 2 files");
    assert_eq!(read(&folder, "notes/first.txt"), None);
    assert_eq!(read(&folder, "notes/second.txt"), None);

    let redone = state.redo_project().expect("one Redo");
    assert_eq!(redone, "Writer wrote 2 files");
    assert_eq!(
        read(&folder, "notes/first.txt").as_deref(),
        Some(&b"one"[..])
    );
    assert_eq!(
        read(&folder, "notes/second.txt").as_deref(),
        Some(&b"two"[..])
    );
}

#[test]
fn a_file_the_plugin_replaced_comes_back_as_it_was_and_one_changed_since_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = state_in(dir.path());
    std::fs::create_dir_all(folder.join("notes")).unwrap();
    std::fs::write(folder.join("notes/first.txt"), b"mine").unwrap();

    state.request_plugin_run(ID);
    assert_eq!(
        read(&folder, "notes/first.txt").as_deref(),
        Some(&b"one"[..])
    );

    // Changed since the plugin wrote it: Undo says so and moves nothing.
    std::fs::write(folder.join("notes/second.txt"), b"edited by hand").unwrap();
    let why = state.undo_project().expect_err("a file changed since");
    assert!(
        why.contains("notes/second.txt") && why.contains("nothing was changed"),
        "{why}"
    );
    assert_eq!(
        read(&folder, "notes/first.txt").as_deref(),
        Some(&b"one"[..])
    );
    assert_eq!(
        read(&folder, "notes/second.txt").as_deref(),
        Some(&b"edited by hand"[..])
    );

    // Put back by hand, Undo brings the file it replaced back, and the one it made goes.
    std::fs::write(folder.join("notes/second.txt"), b"two").unwrap();
    state.undo_project().expect("it moves now");
    assert_eq!(
        read(&folder, "notes/first.txt").as_deref(),
        Some(&b"mine"[..])
    );
    assert_eq!(read(&folder, "notes/second.txt"), None);
}

#[test]
fn a_set_the_project_refuses_writes_none_of_its_files_and_makes_no_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = state_in(dir.path());
    // The second file's place is a folder, so the set cannot be written, and the first must not be.
    std::fs::create_dir_all(folder.join("notes/second.txt")).unwrap();
    let label_before = state.project_undo_label();

    state.request_plugin_run(ID);

    assert_eq!(
        read(&folder, "notes/first.txt"),
        None,
        "the first of the set was written"
    );
    assert_eq!(
        state.project_undo_label(),
        label_before,
        "a refused set made a step"
    );
    let keys = keys(&state);
    assert!(!keys.contains(&"plugin.wrote".to_string()), "{keys:?}");
    let said: Vec<String> = (state.plugin_report(ID).unwrap().iter())
        .map(|d| d.message.clone())
        .collect();
    assert!(
        said.iter()
            .any(|m| m.contains("no file was written") && m.contains("notes/second.txt")),
        "the refusal names the file and says nothing was written: {said:?}"
    );
}

#[test]
fn a_write_that_fails_part_way_puts_back_what_it_had_written_and_makes_no_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = state_in(dir.path());
    std::fs::create_dir_all(folder.join("notes")).unwrap();
    std::fs::write(folder.join("notes/first.txt"), b"mine").unwrap();
    // The check cannot see this: writing `second.txt` goes through `second.txt.tmp`, which is a folder.
    std::fs::create_dir_all(folder.join("notes/second.txt.tmp")).unwrap();
    let label_before = state.project_undo_label();

    state.request_plugin_run(ID);

    assert_eq!(
        read(&folder, "notes/first.txt").as_deref(),
        Some(&b"mine"[..]),
        "the first file was written and not put back"
    );
    assert_eq!(read(&folder, "notes/second.txt"), None);
    assert_eq!(state.project_undo_label(), label_before);
    let said: Vec<String> = (state.plugin_report(ID).unwrap().iter())
        .map(|d| d.message.clone())
        .collect();
    assert!(
        said.iter().any(|m| m.contains("No file was written")),
        "{said:?}"
    );
}

#[test]
fn a_project_another_window_holds_refuses_the_write_and_so_does_no_project_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = state_in(dir.path());
    state
        .workbench
        .held_elsewhere_for_a_test("Another window has this project open");

    state.request_plugin_run(ID);

    assert_eq!(read(&folder, "notes/first.txt"), None);
    let said: Vec<String> = (state.plugin_report(ID).unwrap().iter())
        .map(|d| d.message.clone())
        .collect();
    assert!(
        said.iter().any(|m| m.contains("Another window")),
        "the gate's reason is the one shown: {said:?}"
    );

    let mut none = AppState::default();
    let plugin = writer();
    none.plugins.scanned = Some(vec![plugin.clone()]);
    none.prefs.plugin_consent.insert(
        ID.into(),
        Consent::Granted(
            plugin
                .manifest
                .wants
                .iter()
                .map(|c| c.key().to_string())
                .collect(),
        ),
    );
    none.documents.push(blank(1));
    none.active_document = Some(1);
    none.request_plugin_run(ID);
    let said: Vec<String> = (none.plugin_report(ID).unwrap().iter())
        .map(|d| d.message.clone())
        .collect();
    assert!(
        said.iter().any(|m| m.contains("no project is open")),
        "{said:?}"
    );
}

#[test]
fn a_game_file_or_the_projects_own_files_are_not_written_by_a_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let (state, folder) = state_in(dir.path());
    let check = super::transaction::ProjectFiles::of(&state).expect("a project is open");

    for (path, why) in [
        ("models/jacket.ydd", "game file"),
        ("a.rpf", "game file"),
        (".openrage/history/000001.snapshot", "keeps for itself"),
        ("build/x/y.txt", "keeps for itself"),
        ("police_pack.openrage-proj", "project file"),
        ("data/broken.json", "JSON"),
        ("clothing/wardrobe.json", "pack"),
    ] {
        let bytes: &[u8] = if path.ends_with(".json") { b"{" } else { b"x" };
        let refused = check
            .problem(&[(path, bytes)])
            .unwrap_or_else(|| panic!("{path} was accepted"));
        assert!(refused.contains(why), "{path}: {refused}");
    }
    let fine: [(&str, &[u8]); 2] = [("notes/a.txt", b"fine"), ("data/ok.json", b"{}")];
    assert_eq!(check.problem(&fine), None);
    assert!(folder.exists());
}

#[test]
fn the_projects_own_folders_are_refused_in_any_letters_at_any_depth_and_by_their_short_names() {
    let dir = tempfile::tempdir().unwrap();
    let (state, _) = project(dir.path());
    let check = super::transaction::ProjectFiles::of(&state).expect("a project is open");

    // The same folders on NTFS and on a default Mac, which fold case, and on NTFS under an 8.3 name.
    for path in [
        ".GIT/config",
        ".Git/hooks/pre-commit",
        ".OpenRage/projects/x/history/000001.attached",
        "Build/x/y.txt",
        "sub/.git/hooks/pre-commit",
        "sub/.GIT/config",
        "sub/BUILD/y.txt",
        "GIT~1/config",
        "git~2/hooks/pre-commit",
        "OPENRA~1/projects/x/history/000001.attached",
        "sub/OPENR~10/x",
    ] {
        let refused = check
            .problem(&[(path, b"x")])
            .unwrap_or_else(|| panic!("{path} was accepted"));
        assert!(refused.contains("keeps for itself"), "{path}: {refused}");
    }
    // Names that only look like them stay writable.
    let fine: [(&str, &[u8]); 5] = [
        (".gitignore", b"x"),
        (".gitattributes", b"x"),
        ("notes/builder/a.txt", b"x"),
        ("notes/jacket~1.txt", b"x"),
        ("notes/gitx~1/a.txt", b"x"),
    ];
    assert_eq!(check.problem(&fine), None);
}

#[test]
fn a_set_naming_a_folder_the_project_keeps_in_other_letters_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let file = |path: &str, bytes: &[u8]| FileWrite {
        path: path.into(),
        bytes: bytes.to_vec(),
    };
    let set = Transaction {
        files: vec![
            file("notes/a.txt", b"fine"),
            file(".GIT/hooks/pre-commit", b"#!/bin/sh"),
        ],
    };

    let why = state
        .apply_plugin_files("Writer wrote 2 files", &set)
        .expect_err("a hook of git is a place the project keeps");

    assert!(
        why.contains("No file was written") && why.contains("keeps for itself"),
        "{why}"
    );
    assert_eq!(read(&folder, "notes/a.txt"), None);
    assert_eq!(read(&folder, ".git/hooks/pre-commit"), None);
}

#[test]
fn a_step_whose_record_names_a_place_a_plugin_may_not_write_is_not_moved() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let set = Transaction {
        files: vec![FileWrite {
            path: "notes/a.txt".into(),
            bytes: b"one".to_vec(),
        }],
    };
    state
        .apply_plugin_files("Writer wrote a file", &set)
        .expect("a file of the project");
    let step = state.workbench.step_here().expect("the files are a step");
    state.undo_project().expect("Undo takes the file back");
    assert_eq!(read(&folder, "notes/a.txt"), None);
    let aside = state.workbench.step_aside(step).expect("history is kept");

    // A record made by hand: Redo would write what the step kept at the path it names.
    for forged in [
        "../outside.txt",
        "notes/../../outside.txt",
        ".GIT/hooks/pre-commit",
        ".OpenRage/projects/x/history/000001.attached",
    ] {
        std::fs::create_dir_all(&aside).unwrap();
        std::fs::write(aside.join("0.after"), b"planted").unwrap();
        let record = serde_json::json!({ "plugin_files": [{ "path": forged, "before": false }] });
        state
            .workbench
            .attach_to_step(step, &serde_json::to_vec(&record).unwrap())
            .unwrap();

        let why = state.redo_project().expect_err(forged);

        assert!(why.contains("nothing was changed"), "{forged}: {why}");
        assert!(!dir.path().join("outside.txt").exists(), "{forged}");
        assert_eq!(read(&folder, ".git/hooks/pre-commit"), None, "{forged}");
        assert_eq!(read(&folder, "notes/a.txt"), None, "{forged}");
    }
}
