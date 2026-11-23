//! A rename, move or delete of a file on disk leaves the history of the install or folder it is
//! in as it was, so the steps recorded inside its archives still undo.

use std::path::Path;

use or_io::container::rpf::BodyForm;

use super::every_add_lands_where_the_tree_shows_it::opened;
use super::nested_archives_survive_a_save::{archive, body, errors, on_disk, save};
use super::opening_a_ydd::node_at;
use super::undo_from_files_acts_on_what_files_shows::name_of;
use super::{AppState, DocPayload, DocumentId, EditHistory};
use crate::platform::recycle_bin;

/// `files` written under `root/mods` and opened with Open Folder, with `root/bin` this thread's Recycle Bin.
pub(super) fn a_folder(root: &Path, files: &[(&str, &[u8])]) -> (AppState, DocumentId) {
    let mods = root.join("mods");
    for (path, bytes) in files {
        let path = mods.join(path);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folders");
        std::fs::write(&path, bytes).expect("a file");
    }
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let mut state = AppState::new();
    let doc = opened(&mut state, &mods);
    (state, doc)
}

pub(super) fn marked(state: &AppState, doc: DocumentId) -> bool {
    state.document(doc).is_some_and(|d| d.dirty)
}

#[derive(Debug, Clone, Copy)]
enum OnDisk {
    Rename,
    Move,
    Delete,
}

/// What is wrong once `b.txt` is deleted and `a.txt` renamed inside `x.rpf`, `loose.txt` beside it changed on disk `how`, and both steps undone.
///
/// ⚠ The delete leaves a step holding rows taken out of `x.rpf`, which empties
/// the history when a change on disk reaches `x.rpf`; this one reaches only
/// `loose.txt`.
fn the_archives_steps_after(how: OnDisk) -> Vec<String> {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let x = archive(&[("a.txt", b"a"), ("b.txt", b"b")]);
    let (mut state, doc) = a_folder(
        dir.path(),
        &[
            ("x.rpf", &x),
            ("loose.txt", b"loose"),
            ("sub/kept.txt", b"kept"),
        ],
    );
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    let b = node_at(&mut state, doc, "x.rpf/b.txt");
    let loose = node_at(&mut state, doc, "loose.txt");
    let sub = node_at(&mut state, doc, "sub");
    state.rpf_delete_node(doc, b).expect("the delete asks");
    state.answer_delete(true);
    state
        .rpf_rename_node(doc, a, "renamed.txt")
        .expect("the rename inside x.rpf");

    let mods = dir.path().join("mods");
    let (done, now) = match how {
        OnDisk::Rename => (
            state.rpf_rename_node(doc, loose, "readme.txt"),
            mods.join("readme.txt"),
        ),
        OnDisk::Move => (
            state.rpf_move_node(doc, loose, sub),
            mods.join("sub").join("loose.txt"),
        ),
        OnDisk::Delete => (
            (state.rpf_delete_node(doc, loose)).map(|()| state.answer_delete(true)),
            dir.path().join("bin").join("loose.txt"),
        ),
    };
    let mut failed = Vec::new();
    if done.is_err() || !now.is_file() {
        failed.push(format!(
            "{how:?}: loose.txt was not changed on disk: {done:?} {:?}",
            errors(&state)
        ));
    }

    state.set_active_document(doc);
    let undone = [state.undo(), state.undo()];
    let expected = ["Rename a.txt to renamed.txt", "Delete b.txt"].map(|l| Some(l.to_string()));
    if undone != expected {
        failed.push(format!("{how:?}: Undo undid {undone:?}"));
    }
    let names = [name_of(&state, doc, a), name_of(&state, doc, b)];
    if names != ["a.txt", "b.txt"] {
        failed.push(format!("{how:?}: x.rpf holds {names:?}"));
    }
    if marked(&state, doc) {
        failed.push(format!(
            "{how:?}: undone to as it opened, the folder is still marked"
        ));
    }
    failed
}

#[test]
fn a_rename_move_or_delete_on_disk_beside_an_archive_leaves_the_steps_inside_it_to_undo() {
    let mut failed = the_archives_steps_after(OnDisk::Rename);
    failed.extend(the_archives_steps_after(OnDisk::Move));
    failed.extend(the_archives_steps_after(OnDisk::Delete));
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// ⚠ Steps name rows by id, which a rename or move on disk keeps, and the
/// tree points the archive at its new file; no step here holds rows taken
/// out of it.
#[test]
fn an_archive_renamed_and_moved_on_disk_undoes_and_redoes_the_rename_inside_it_where_it_now_is() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let x = archive(&[("a.txt", b"a")]);
    let (mut state, doc) = a_folder(dir.path(), &[("x.rpf", &x), ("sub/kept.txt", b"kept")]);
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    let row = node_at(&mut state, doc, "x.rpf");
    let sub = node_at(&mut state, doc, "sub");
    state
        .rpf_rename_node(doc, a, "renamed.txt")
        .expect("the rename inside x.rpf");
    state
        .rpf_rename_node(doc, row, "y.rpf")
        .expect("the archive's rename");
    state
        .rpf_move_node(doc, row, sub)
        .expect("the archive's move");
    state
        .rpf_rename_node(doc, sub, "moved")
        .expect("the rename of the folder holding it");

    state.set_active_document(doc);
    assert_eq!(
        state.undo().as_deref(),
        Some("Rename a.txt to renamed.txt"),
        "a change on disk of the archive emptied the history"
    );
    assert_eq!(name_of(&state, doc, a), "a.txt");
    assert!(
        !marked(&state, doc),
        "undone, y.rpf holds what its file does"
    );
    assert!(state.redo().is_some());
    save(&mut state, doc);
    let saved = on_disk(&state, &dir.path().join("mods").join("moved").join("y.rpf"));
    assert_eq!(
        body(&saved, "renamed.txt", BodyForm::Decoded),
        b"a",
        "{:?}",
        errors(&state)
    );
}

/// ⚠ `sub` holds `x.rpf`, opened, with a rename made inside it, so the delete
/// takes the row the step names.
#[test]
fn a_delete_on_disk_of_a_folder_holding_an_opened_archive_leaves_no_step_that_cannot_undo() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let x = archive(&[("a.txt", b"a")]);
    let (mut state, doc) = a_folder(dir.path(), &[("sub/x.rpf", &x)]);
    let a = node_at(&mut state, doc, "sub/x.rpf/a.txt");
    let sub = node_at(&mut state, doc, "sub");
    state
        .rpf_rename_node(doc, a, "renamed.txt")
        .expect("the rename inside x.rpf");
    state.rpf_delete_node(doc, sub).expect("the delete asks");
    state.answer_delete(true);
    assert!(
        dir.path().join("bin").join("sub").join("x.rpf").is_file(),
        "{:?}",
        errors(&state)
    );

    state.set_active_document(doc);
    while state.focused_history().is_some_and(EditHistory::can_undo) {
        assert!(
            state.undo().is_some(),
            "a step the delete left no longer finds its rows"
        );
    }
    assert!(
        !marked(&state, doc),
        "the delete took the only archive left to save"
    );
}

/// The paths of the files in `doc`'s tree whose bytes no longer read.
fn unreadable(state: &AppState, doc: DocumentId) -> Vec<String> {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        panic!("the folder's tree");
    };
    let session = &rpf.session;
    (session.all_nodes().into_iter())
        .filter(|&id| session.node(id).is_some_and(|node| !node.is_dir()))
        .filter(|&id| session.bytes_for(id).is_none())
        .filter_map(|id| session.path_of(id))
        .collect()
}

/// ⚠ `n.rpf`, deleted from `x.rpf`, is held by its step outside the tree and
/// reads `x.rpf` by the path it was opened from: a rename on disk points the
/// tree's archives at the new file, and not it.
#[test]
fn an_archive_renamed_on_disk_never_has_a_row_put_back_that_reads_its_old_file() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let n = archive(&[("inside.txt", b"inside")]);
    let x = archive(&[("n.rpf", &n), ("a.txt", b"a")]);
    let (mut state, doc) = a_folder(dir.path(), &[("x.rpf", &x)]);
    node_at(&mut state, doc, "x.rpf/n.rpf/inside.txt");
    let nested = node_at(&mut state, doc, "x.rpf/n.rpf");
    let row = node_at(&mut state, doc, "x.rpf");
    state.rpf_delete_node(doc, nested).expect("the delete asks");
    state.answer_delete(true);
    state
        .rpf_rename_node(doc, row, "y.rpf")
        .expect("the archive's rename");

    state.set_active_document(doc);
    while state.undo().is_some() {}
    let unread = unreadable(&state, doc);
    assert!(unread.is_empty(), "{unread:?} read x.rpf, which is gone");
}
