//! A row moved into another archive belongs to it at once, a move onto an
//! archive on disk lands inside it, one `Ctrl+S` saves every archive, a deleted
//! archive leaves nothing to save, nothing reads an archive across its save or
//! defragments it while an extraction reads it, and an archive row not yet
//! expanded extracts whole.

use std::path::Path;

use or_io::container::rpf::{BodyForm, RpfArchive, RpfEntry};

use super::defragmenting_an_archive::settle;
use super::document::{DocumentId, RpfNodeId, TextDocument};
use super::every_add_lands_where_the_tree_shows_it::opened;
use super::extract_job::{ExtractStart, TypeFilter};
use super::nested_archives_survive_a_save::{archive, body, errors, nested, on_disk, save, EDITED};
use super::nothing_extracts_into_an_install::{files_under, run_to_completion};
use super::opening_a_ydd::{finish_load, node_at};
use super::prefs::EditMode;
use super::save_destination::SaveDestination;
use super::two_readers_of_one_archive_across_a_save::{install, save_alone};
use super::{AppState, DocPayload, RetailEditDecision};
use crate::platform::recycle_bin;

/// `files` written into a new folder `mods`, opened with Open Folder.
pub(super) fn folder_of(root: &Path, files: &[(&str, Vec<u8>)]) -> (AppState, DocumentId) {
    let folder = root.join("mods");
    std::fs::create_dir_all(&folder).expect("the folder");
    for (name, bytes) in files {
        std::fs::write(folder.join(name), bytes).expect("an archive");
    }
    let mut state = AppState::new();
    let doc = opened(&mut state, &folder);
    (state, doc)
}

fn bytes_of(state: &AppState, doc: DocumentId, node: RpfNodeId) -> Option<Vec<u8>> {
    match &state.document(doc)?.payload {
        DocPayload::Rpf(rpf) => rpf.session.bytes_for(node).map(|b| b.to_vec()),
        _ => None,
    }
}

fn offset_of(archive: &RpfArchive, name: &str) -> u64 {
    let node = archive.nodes.iter().find(|n| n.name() == name);
    match node.and_then(|n| archive.entries.get(n.entry_index() as usize)) {
        Some(RpfEntry::Binary(entry)) => entry.file_offset,
        _ => panic!("{name} is not a file of the archive"),
    }
}

/// ⚠ An install, so the gate asks, about a rename of the moved row and about
/// its tab's save. `a.rpf` is saved alone once the folder has left it, so
/// `b.rpf` is the one archive left unsaved, and until `b.rpf` is saved the
/// moved rows still read `a.rpf`'s file.
#[test]
fn a_row_moved_into_another_archive_saves_into_it_asks_about_it_and_is_marked_there_alone() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    let (mut state, tree) = install(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("dir/moved.txt", b"moved"), ("stays.txt", b"stays")]),
            ),
            ("b.rpf", archive(&[("data/kept.txt", b"kept")])),
        ],
    );
    state.prefs.edit_mode = EditMode::Everything;
    state.retail_edit_session_allow = true;
    let moved = node_at(&mut state, tree, "a.rpf/dir/moved.txt");
    let folder = node_at(&mut state, tree, "a.rpf/dir");
    let stays = node_at(&mut state, tree, "a.rpf/stays.txt");
    let into = node_at(&mut state, tree, "b.rpf/data");
    state.open_rpf_entry(tree, moved);
    let tab = finish_load(&mut state).expect("the moved file's tab");
    state
        .rpf_move_node(tree, folder, into)
        .expect("the folder moves");
    save_alone(&mut state, tree, stays);
    let (a, b) = (dir.path().join("a.rpf"), dir.path().join("b.rpf"));
    assert!(
        on_disk(&state, &a)
            .nodes
            .iter()
            .all(|n| n.name() != "moved.txt"),
        "a.rpf was not saved alone: {:?}",
        errors(&state)
    );

    let mut asked = Vec::new();
    state.retail_edit_session_allow = false;
    let _ = state.rpf_rename_node(tree, moved, "renamed.txt");
    asked.extend(
        state
            .pending_retail_edit
            .as_ref()
            .map(|p| p.target_path.clone()),
    );
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    state.retail_edit_session_allow = false;
    let edited = state.document_mut(tab).expect("the tab");
    edited.payload = DocPayload::Text(TextDocument::decode(EDITED));
    edited.mark_edited();
    state.set_active_document(tab);
    assert!(state.save_active_document());
    let offered = state.save_destination.clone().expect("the tab asks where");
    state.choose_save_destination(SaveDestination::Archive);
    asked.extend(
        state
            .pending_retail_edit
            .as_ref()
            .map(|p| p.target_path.clone()),
    );
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);

    assert_eq!(offered.archive.as_deref(), Some(b.as_path()), "saves into");
    assert_eq!(
        asked,
        [b.clone(), b.clone()],
        "the rename and the save asked about"
    );
    assert_eq!(
        body(&on_disk(&state, &b), "renamed.txt", BodyForm::Decoded),
        EDITED,
        "{:?}",
        errors(&state)
    );
    assert!(
        state.unsaved_documents(&[tree]).is_empty(),
        "the save left the archive the row left marked unsaved"
    );
}

/// ⚠ `b.rpf` is not opened before the move. A loose file moved onto it is
/// refused, as onto a folder inside an archive.
#[test]
fn a_file_moved_onto_an_archive_row_on_disk_is_in_that_archive_after_a_save() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = folder_of(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("moved.txt", b"moved"), ("stays.txt", b"x")]),
            ),
            ("b.rpf", archive(&[("kept.txt", b"kept")])),
            ("loose.txt", b"loose".to_vec()),
        ],
    );
    let moved = node_at(&mut state, doc, "a.rpf/moved.txt");
    let loose = node_at(&mut state, doc, "loose.txt");
    let row = node_at(&mut state, doc, "b.rpf");

    let into = state.rpf_move_node(doc, moved, row);
    let loose_into = state.rpf_move_node(doc, loose, row);
    save(&mut state, doc);
    assert!(into.is_ok(), "the move onto b.rpf: {into:?}");
    let b = on_disk(&state, &dir.path().join("mods").join("b.rpf"));
    assert_eq!(body(&b, "moved.txt", BodyForm::Decoded), b"moved");
    assert_eq!(body(&b, "kept.txt", BodyForm::Decoded), b"kept");
    let a = on_disk(&state, &dir.path().join("mods").join("a.rpf"));
    assert!(a.nodes.iter().all(|n| n.name() != "moved.txt"));
    assert!(loose_into.is_err(), "a loose file moved into an archive");
    assert!(dir.path().join("mods").join("loose.txt").is_file());
    assert!(
        state.unsaved_documents(&[doc]).is_empty(),
        "{:?}",
        errors(&state)
    );
}

/// ⚠ Each archive's nested archive is moved into the next, so each is saved
/// only after the next one; a fixed order of five has one chance in 120.
#[test]
fn one_ctrl_s_saves_every_archive_whatever_nested_archive_moved_between_them() {
    const COUNT: usize = 5;
    let nested = archive(&[("inside.txt", b"inside")]);
    let names: Vec<(String, String)> = (0..COUNT)
        .map(|i| (format!("a{i}.rpf"), format!("n{i}.rpf")))
        .collect();
    let archives: Vec<(&str, Vec<u8>)> = names
        .iter()
        .map(|(a, n)| {
            (
                a.as_str(),
                archive(&[(n, &nested), ("data/kept.txt", b"kept")]),
            )
        })
        .collect();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = install(dir.path(), &archives);
    for (i, (a, n)) in names.iter().enumerate().take(COUNT - 1) {
        let moving = node_at(&mut state, tree, &format!("{a}/{n}"));
        let into = node_at(&mut state, tree, &format!("{}/data", names[i + 1].0));
        state.rpf_move_node(tree, moving, into).expect("the move");
    }

    save(&mut state, tree);
    assert!(errors(&state).is_empty(), "{:?}", errors(&state));
    assert!(state.unsaved_documents(&[tree]).is_empty());
    for (i, (a, n)) in names.iter().enumerate().take(COUNT - 1) {
        let next = on_disk(&state, &dir.path().join(&names[i + 1].0));
        assert_eq!(body(&next, n, BodyForm::AsStored), nested);
        let left = on_disk(&state, &dir.path().join(a));
        assert!(left.nodes.iter().all(|node| node.name() != n.as_str()));
    }
}

/// ⚠ `b.rpf` is saved alone first, so its save reopens `n.rpf` from the file
/// it wrote, without `f.txt`, while `c.rpf` still reads `f.txt` at the entry
/// `n.rpf` listed it at.
#[test]
fn a_file_moved_out_of_a_moved_nested_archive_saves_whole_whichever_archive_is_saved_first() {
    let n = archive(&[("f.txt", b"the moved file"), ("g.txt", b"stays in n")]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = install(
        dir.path(),
        &[
            ("a.rpf", archive(&[("n.rpf", &n), ("stays.txt", b"stays")])),
            ("b.rpf", archive(&[("data/kept.txt", b"kept")])),
            ("c.rpf", archive(&[("data/other.txt", b"other")])),
        ],
    );
    let f = node_at(&mut state, tree, "a.rpf/n.rpf/f.txt");
    let row = node_at(&mut state, tree, "a.rpf/n.rpf");
    let kept = node_at(&mut state, tree, "b.rpf/data/kept.txt");
    let into_b = node_at(&mut state, tree, "b.rpf/data");
    let into_c = node_at(&mut state, tree, "c.rpf/data");
    state.rpf_move_node(tree, row, into_b).expect("n.rpf moves");
    state.rpf_move_node(tree, f, into_c).expect("f.txt moves");

    save_alone(&mut state, tree, kept);
    save(&mut state, tree);
    assert!(errors(&state).is_empty(), "{:?}", errors(&state));
    let c = on_disk(&state, &dir.path().join("c.rpf"));
    assert_eq!(body(&c, "f.txt", BodyForm::Decoded), b"the moved file");
    let b = on_disk(&state, &dir.path().join("b.rpf"));
    let moved = nested(&b, "n.rpf");
    assert_eq!(body(&moved, "g.txt", BodyForm::Decoded), b"stays in n");
    assert!(moved.nodes.iter().all(|node| node.name() != "f.txt"));
}

/// ⚠ `a.rpf` goes from disk after a file was moved out of it, and `n.rpf`,
/// edited inside `b.rpf`, goes from the session: a slot left behind stays
/// unsaved with no rows to save, and the moved file reads a file in the bin.
#[test]
fn deleting_an_opened_archive_with_unsaved_edits_leaves_nothing_to_save() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let n = archive(&[("deep.txt", b"deep")]);
    let (mut state, doc) = folder_of(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("moved.txt", b"moved"), ("edited.txt", b"x")]),
            ),
            (
                "b.rpf",
                archive(&[("n.rpf", &n), ("data/kept.txt", b"kept")]),
            ),
        ],
    );
    let edited = node_at(&mut state, doc, "a.rpf/edited.txt");
    let moved = node_at(&mut state, doc, "a.rpf/moved.txt");
    let deep = node_at(&mut state, doc, "b.rpf/n.rpf/deep.txt");
    let into = node_at(&mut state, doc, "b.rpf/data");
    for row in [edited, deep] {
        state
            .update_rpf_entry_bytes(doc, row, EDITED.to_vec())
            .expect("the edit");
    }
    state.rpf_move_node(doc, moved, into).expect("the move");
    for path in ["a.rpf", "b.rpf/n.rpf"] {
        let row = node_at(&mut state, doc, path);
        state.rpf_delete_node(doc, row).expect("the delete asks");
        state.answer_delete(true);
    }

    save(&mut state, doc);
    assert!(errors(&state).is_empty(), "{:?}", errors(&state));
    assert!(bin.join("a.rpf").is_file(), "a.rpf went to the bin");
    let b = on_disk(&state, &dir.path().join("mods").join("b.rpf"));
    assert_eq!(body(&b, "moved.txt", BodyForm::Decoded), b"moved");
    assert!(b.nodes.iter().all(|node| node.name() != "n.rpf"));
    assert!(state.unsaved_documents(&[doc]).is_empty(), "left to save");
}

/// ⚠ The extraction counts as reading until the per-frame tick takes its
/// report, so a fast one still holds its archive here.
#[test]
fn a_defragment_started_during_an_extraction_is_refused() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = folder_of(
        dir.path(),
        &[("x.rpf", archive(&[("a.txt", b"a"), ("b.txt", b"b")]))],
    );
    let row = node_at(&mut state, doc, "x.rpf");
    node_at(&mut state, doc, "x.rpf/a.txt");
    let out = dir.path().join("out");

    let started = state.extract_rpf_node(doc, row, out, BodyForm::Decoded, TypeFilter::All);
    assert_eq!(started, ExtractStart::Started { planned: 2 });
    state.defragment_archive(doc, row);
    let refused = !state.defrag.is_running() && state.defrag.offer.is_none();
    let said = errors(&state);
    run_to_completion(&mut state);
    assert!(
        refused,
        "the defragment started while the extraction read x.rpf"
    );
    assert!(
        said.iter().any(|e| e.contains("extraction of x.rpf")),
        "the refusal does not name the extraction: {said:?}"
    );

    state.defragment_archive(doc, row);
    settle(&mut state);
    assert!(state.defrag.offer.is_some(), "{:?}", errors(&state));
    let during = state.extract_rpf_node(
        doc,
        row,
        dir.path().join("during"),
        BodyForm::Decoded,
        TypeFilter::All,
    );
    assert_eq!(
        during,
        ExtractStart::Refused,
        "extracted while defragmenting"
    );
}

/// ⚠ The edit grows `a.bin`, which lies before `n.rpf` in the file, so the
/// save moves `n.rpf`, and a listing taken before it reads the new file at
/// the old offsets.
#[test]
fn an_archive_expanding_across_the_save_of_the_archive_holding_it_reads_what_the_save_wrote() {
    const INSIDE: &[u8] = b"inside the nested archive";
    let n = archive(&[("inside.txt", INSIDE)]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = install(
        dir.path(),
        &[(
            "outer.rpf",
            archive(&[("a.bin", &[0x11; 1024]), ("n.rpf", &n)]),
        )],
    );
    let path = dir.path().join("outer.rpf");
    let a = node_at(&mut state, tree, "outer.rpf/a.bin");
    let row = node_at(&mut state, tree, "outer.rpf/n.rpf");
    let before = offset_of(&on_disk(&state, &path), "n.rpf");
    let grown: Vec<u8> = (0..128 * 1024u32)
        .map(|n| (n.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    state
        .update_rpf_entry_bytes(tree, a, grown)
        .expect("the edit");

    state.request_expand(tree, row);
    save(&mut state, tree);
    assert_ne!(
        offset_of(&on_disk(&state, &path), "n.rpf"),
        before,
        "the save left n.rpf where it was: {:?}",
        errors(&state)
    );
    let inside = node_at(&mut state, tree, "outer.rpf/n.rpf/inside.txt");
    assert_eq!(bytes_of(&state, tree, inside).as_deref(), Some(INSIDE));
}

#[test]
fn an_extraction_of_an_archive_row_not_yet_expanded_extracts_its_whole_contents() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = folder_of(
        dir.path(),
        &[("x.rpf", archive(&[("a.txt", b"a"), ("data/b.txt", b"b")]))],
    );
    let row = node_at(&mut state, doc, "x.rpf");
    let out = dir.path().join("out");

    let started = state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    run_to_completion(&mut state);
    assert_eq!(started, ExtractStart::Planning);
    assert_eq!(
        files_under(&out),
        ["a.txt", "data/b.txt"],
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
    assert_eq!(std::fs::read(out.join("data/b.txt")).expect("b.txt"), b"b");
}
