//! A document is marked unsaved exactly while a save would write something,
//! the marker clears once a save has written it, and nothing holding unsaved
//! work is dropped without asking.
//!
//! ⚠ Every case runs before any is judged, so one run names each that fails.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::resource::Rsc7;
use or_io::desc::{IntWidth, ScalarValue};

use super::nested_archives_survive_a_save::{archive, errors, save};
use super::opening_a_ydd::{finish_load, node_at};
use super::prefs::{ExportFixes, Game, Prefs};
use super::project::load_project;
use super::save_destination::SaveDestination;
use super::texture_resolution::AttachedDictionary;
use super::{
    AppState, CloseAnswer, DocPayload, Document, DocumentId, DocumentKind, EnhancedOpenDecision,
    PendingEnhancedOpen, ResourceImage, RpfEntryBinding, TextDocument,
};
use crate::platform::recycle_bin;

/// What the tab strip, the status bar and the hierarchy mark `doc` with.
fn marked(state: &AppState, doc: DocumentId) -> bool {
    state.document(doc).is_some_and(|d| d.dirty)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[derive(Debug, Clone, Copy)]
enum Tree {
    OpenFolder,
    Install,
}

/// A delete, a rename and a move on disk leave nothing to save; an edit inside an archive does, until the save writes it.
fn a_tree_on_disk(how: Tree) -> Vec<String> {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let root = dir.path().join("mods");
    std::fs::create_dir_all(root.join("sub")).expect("the folders");
    std::fs::write(root.join("delete_me.txt"), b"to the bin").expect("a file");
    std::fs::write(root.join("rename_me.txt"), b"renamed").expect("a file");
    let x = archive(&[("a.txt", b"in x")]);
    std::fs::write(root.join("x.rpf"), &x).expect("x.rpf");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let mut state = AppState::new();
    let kind = match how {
        Tree::OpenFolder => {
            state.open_folder(root.clone());
            DocumentKind::Folder
        }
        Tree::Install => {
            state.open_game_root(root.clone());
            DocumentKind::GameRoot
        }
    };
    let doc = state
        .documents
        .iter()
        .find(|d| d.kind == kind)
        .expect("the tree's document")
        .id;

    let mut failed = Vec::new();
    // ⚠ An edit that did nothing would leave nothing to save and pass.
    let mut judge = |state: &AppState, what: &str, done: Result<(), String>, expected: bool| {
        if let Err(e) = done {
            failed.push(format!("{how:?}: {what} failed: {e}"));
        } else if marked(state, doc) != expected {
            let now = if expected { "unmarked" } else { "marked" };
            failed.push(format!(
                "{how:?}: {what} left it {now}: {:?}",
                errors(state)
            ));
        }
    };
    let gone = node_at(&mut state, doc, "delete_me.txt");
    state.rpf_delete_node(doc, gone).expect("the delete asks");
    state.answer_delete(true);
    let done = if root.join("delete_me.txt").exists() {
        Err(format!("{:?}", errors(&state)))
    } else {
        Ok(())
    };
    judge(&state, "a delete on disk", done, false);

    let renamed = node_at(&mut state, doc, "rename_me.txt");
    let done = state.rpf_rename_node(doc, renamed, "renamed.txt");
    judge(&state, "a rename on disk", done, false);

    let sub = node_at(&mut state, doc, "sub");
    let done = state.rpf_move_node(doc, renamed, sub);
    judge(&state, "a move on disk", done, false);

    let x_row = node_at(&mut state, doc, "x.rpf");
    let done = state.rpf_add_folder(doc, x_row, "added").map(|_| ());
    judge(&state, "a folder added inside x.rpf", done, true);

    save(&mut state, doc);
    judge(&state, "the save that wrote x.rpf", Ok(()), false);
    if read(&root.join("x.rpf")) == x {
        failed.push(format!("{how:?}: the save did not write x.rpf"));
    }
    failed
}

fn edit_text(state: &mut AppState, doc: DocumentId, text: &[u8]) {
    let doc = state.document_mut(doc).expect("the entry is open");
    doc.payload = DocPayload::Text(TextDocument::decode(text));
    doc.mark_edited();
}

/// Ctrl+S on the entry, answered "As a loose file" at `path`.
fn save_loose(state: &mut AppState, entry: DocumentId, path: &Path) {
    state.set_active_document(entry);
    state.save_active_document();
    state.choose_save_destination(SaveDestination::LooseFile(path.to_path_buf()));
}

/// An archive entry saved as a loose file has nothing left to lose, so the close prompt's Save closes it.
fn an_entry_saved_as_a_loose_file() -> Vec<String> {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let root = dir.path().join("mods");
    std::fs::create_dir_all(&root).expect("the folder");
    let x = archive(&[("a.txt", b"in x")]);
    std::fs::write(root.join("x.rpf"), &x).expect("x.rpf");
    let mut state = AppState::new();
    state.prefs.export_fixes = ExportFixes::Ask;
    state.open_folder(root.clone());
    let tree = state.active_document.expect("the folder's tree");
    let a = node_at(&mut state, tree, "x.rpf/a.txt");
    state.open_rpf_entry(tree, a);
    let entry = finish_load(&mut state).expect("a.txt opens");
    let mut failed = Vec::new();

    edit_text(&mut state, entry, b"edited");
    let first = dir.path().join("first.txt");
    save_loose(&mut state, entry, &first);
    if marked(&state, entry) {
        failed.push("a loose save left the entry marked".to_string());
    }
    // ⚠ The archive still holds the bytes it was opened from, and the edit is no less an edit.
    let second = dir.path().join("second.txt");
    save_loose(&mut state, entry, &second);
    if state.pending_export_fixes.is_some() || !second.is_file() || read(&second) != b"edited" {
        failed.push("a second loose save offered the archive's bytes for the edit".to_string());
    }

    edit_text(&mut state, entry, b"edited again");
    state.close_document(entry);
    if state.pending_close.is_none() {
        failed.push("closing an edited entry asked nothing".to_string());
    }
    state.answer_close(CloseAnswer::Save);
    state.drive_close(|_, _| None);
    let third = dir.path().join("third.txt");
    if state.save_destination.is_some() {
        state.choose_save_destination(SaveDestination::LooseFile(third.clone()));
    } else {
        failed.push("the close prompt's Save asked nowhere to save the entry".to_string());
    }
    state.drive_close(|_, _| None);
    if state.document(entry).is_some() {
        let toasts: Vec<&str> = state.toasts.iter().map(|t| t.message.as_str()).collect();
        failed.push(format!(
            "Save, then As a loose file, closed nothing: {toasts:?}"
        ));
    }
    if !third.is_file() || read(&third) != b"edited again" {
        failed.push("the close prompt's loose save did not write the edit".to_string());
    }
    for path in [&first, &second] {
        if !path.is_file() || read(path) != b"edited" {
            failed.push(format!("{} does not hold the edit", path.display()));
        }
    }
    if read(&root.join("x.rpf")) != x {
        failed.push("a loose save wrote the archive".to_string());
    }
    failed
}

/// A member removed marks the project, exiting offers to save it, and saving it clears the mark.
fn a_project_whose_membership_changed() -> Vec<String> {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, b"first").expect("a.txt");
    std::fs::write(&b, b"second").expect("b.txt");
    let project = dir.path().join("p.openrage-proj");
    let mut state = AppState::new();
    for path in [&a, &b] {
        state.load_file(path.clone());
        finish_load(&mut state).expect("the file opens");
    }
    state
        .save_project_to(project.clone())
        .expect("the project is written");
    let mut failed = Vec::new();
    let project_marked = |state: &AppState| state.window_title().contains("p ●");

    if !state.remove_from_project(&b) || !project_marked(&state) {
        failed.push(format!(
            "removing a member did not mark the project: {}",
            state.window_title()
        ));
    }
    state.request_exit();
    if state.should_exit || state.pending_close.is_none() {
        failed.push("exiting dropped the project's removed member without asking".to_string());
        return failed;
    }
    state.answer_close(CloseAnswer::Save);
    state.drive_close(|_, _| None);
    if !state.should_exit {
        failed.push(format!(
            "Save did not exit: {:?}",
            state
                .toasts
                .iter()
                .map(|t| t.message.as_str())
                .collect::<Vec<_>>()
        ));
    }
    if project_marked(&state) {
        failed.push(format!(
            "saving the project left it marked: {}",
            state.window_title()
        ));
    }
    let members: Vec<PathBuf> = load_project(&project)
        .expect("the project reads")
        .project
        .documents
        .into_iter()
        .map(|d| d.path)
        .collect();
    if members != [a] {
        failed.push(format!("the saved project holds {members:?}"));
    }
    failed
}

/// A patch to a resource image is an edit like any other, and its save clears the mark.
fn a_patched_resource_image() -> Vec<String> {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("probe.ydr");
    let seal = Rsc7::invent_single_chunk_seal(165, 64).expect("a header for 64 bytes");
    let data = vec![0u8; seal.total_size() as usize];
    let image = Arc::new(ResourceImage {
        virtual_size: data.len(),
        data,
        seal,
    });
    let mut state = AppState::new();
    let id = state.alloc_doc_id();
    state.documents.push(
        Document::new(
            id,
            Some(path.clone()),
            DocPayload::Resource(or_io::Asset::Drawable(Default::default())),
        )
        .with_resource(Some(image)),
    );
    state.active_document = Some(id);
    let mut failed = Vec::new();

    let u16 = or_editor::Edit {
        offset: 4,
        kind: or_editor::EditKind::Int(IntWidth::U16),
    };
    if !state.patch_scalar(id, "probe", u16, ScalarValue::Int(7)) {
        failed.push("the patch was refused".to_string());
        return failed;
    }
    let doc = state.document(id).expect("the document");
    if !marked(&state, id) || !doc.patched() {
        failed.push("a patch left the document unmarked or unpatched".to_string());
    }
    if doc.edit_epoch() == 0 {
        failed.push("a patch bypassed mark_edited: the edit epoch did not advance".to_string());
    }
    save(&mut state, id);
    if marked(&state, id) || !path.is_file() {
        failed.push(format!(
            "the save left the patch marked or unwritten: {:?}",
            errors(&state)
        ));
    }
    failed
}

#[test]
fn each_edit_is_marked_unsaved_exactly_until_a_save_writes_it() {
    let mut failed = a_tree_on_disk(Tree::OpenFolder);
    failed.extend(a_tree_on_disk(Tree::Install));
    failed.extend(an_entry_saved_as_a_loose_file());
    failed.extend(a_project_whose_membership_changed());
    failed.extend(a_patched_resource_image());
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn detaching_a_dictionary_with_unsaved_edits_asks_first() {
    let mut state = AppState::new();
    let id = state.alloc_doc_id();
    state.documents.push(Document::new(
        id,
        Some(PathBuf::from("C:/mods/attached.ytd")),
        DocPayload::Resource(or_io::Asset::TextureDict(Default::default())),
    ));
    state.attached_texture_docs.push(AttachedDictionary {
        document: id,
        opened_by_attach: true,
    });
    state.document_mut(id).expect("open").mark_edited();

    state.detach_texture_dictionary(id);
    assert!(
        marked(&state, id),
        "detaching dropped the dictionary's edits before the user answered"
    );
    assert!(state.pending_close.is_some(), "detaching asked nothing");
    state.answer_close(CloseAnswer::Cancel);
    assert!(
        state.is_attached_texture_dictionary(id) && marked(&state, id),
        "Cancel went ahead"
    );

    state.detach_texture_dictionary(id);
    state.answer_close(CloseAnswer::DontSave);
    assert!(
        state.document(id).is_none(),
        "Don't save kept the dictionary"
    );
    assert!(state.attached_texture_dictionaries().is_empty());
}

/// ⚠ Answered Cancel only: a switch that went ahead would remember an install
/// that exists in the test binary's shared preferences, and a parallel
/// `AppState::new` would mount it.
#[test]
fn choosing_another_game_asks_before_it_drops_a_file_opened_from_the_install() {
    type Choose = fn(&mut AppState);

    let dir = tempfile::tempdir().expect("a scratch folder");
    let (legacy, enhanced) = (dir.path().join("legacy"), dir.path().join("enhanced"));
    for root in [&legacy, &enhanced] {
        std::fs::create_dir(root).expect("an install folder");
    }
    let mut prefs = Prefs::default();
    prefs.game_paths.set(Game::GtaVLegacy, Some(legacy.clone()));
    prefs.game_paths.set(Game::GtaVEnhanced, Some(enhanced));
    prefs.active_game = Some(Game::GtaVLegacy);
    let mut state = AppState::from_prefs(prefs);
    let old = state
        .documents
        .iter()
        .find(|d| d.kind == DocumentKind::GameRoot)
        .expect("the Legacy install is open")
        .id;
    let file = state.insert_document(
        legacy.join("commandline.txt"),
        DocPayload::Text(TextDocument::decode(b"-windowed")),
        Some(RpfEntryBinding {
            parent_doc_id: old,
            entry_id: 1,
        }),
        None,
    );
    state.document_mut(file).expect("open").mark_edited();

    let choices: [(&str, Choose); 2] = [
        ("the edition prompt's Switch", |state| {
            state.pending_enhanced_open = Some(PendingEnhancedOpen {
                path: PathBuf::from("enhanced.ydr"),
                game: Game::GtaVEnhanced,
                version: 0,
                enhanced_root_known: true,
            });
            state.apply_pending_enhanced_open(EnhancedOpenDecision::Switch);
        }),
        ("Settings' Set active", |state| {
            state.switch_active_game(Game::GtaVEnhanced)
        }),
    ];
    for (choice, choose) in choices {
        choose(&mut state);
        assert!(
            state.document(old).is_some() && marked(&state, file),
            "{choice} dropped the install's unsaved file before the user answered"
        );
        assert!(state.pending_close.is_some(), "{choice} asked nothing");
        state.answer_close(CloseAnswer::Cancel);
        assert_eq!(
            state.prefs.active_game,
            Some(Game::GtaVLegacy),
            "{choice}: Cancel switched"
        );
    }
}
