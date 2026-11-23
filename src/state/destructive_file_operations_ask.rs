//! A delete, a rename, a move, an import and an extraction ask before they
//! destroy anything, and a delete on disk can be taken back.
//!
//! ⚠ Each refusal is followed by the same call aimed at a free name, which has
//! to land: an operation that did nothing at all would pass the refusals alone.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::container::rpf::BodyForm;

use super::delete_prompt::DeleteFrom;
use super::document::{Document, DocumentId, RpfNodeId};
use super::extract_job::{ExtractStart, OnClash, TypeFilter};
use super::nothing_extracts_into_an_install::{files_under, run_to_completion};
use super::rpf_session::{RpfDocument, RpfEditSession};
use super::{AppState, DocPayload};
use crate::platform::recycle_bin;

/// A folder holding `files`, opened as Open Folder opens one.
fn loose_folder(files: &[(&str, &[u8])]) -> (tempfile::TempDir, PathBuf, AppState, DocumentId) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let loose = tmp.path().join("loose");
    std::fs::create_dir_all(&loose).expect("the folder");
    for (rel, bytes) in files {
        let path = loose.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("its folders");
        std::fs::write(&path, bytes).expect("a file the test names");
    }
    let mut state = AppState::new();
    state.open_folder(loose.clone());
    let doc = state
        .documents
        .iter()
        .find(|d| d.path.as_deref() == Some(loose.as_path()))
        .expect("the folder opens as a document")
        .id;
    (tmp, loose, state, doc)
}

/// An archive document holding `files`, none of them on disk.
fn archive_of(files: &[(&str, &[u8])]) -> (AppState, DocumentId, RpfNodeId) {
    let mut session = RpfEditSession::empty();
    let root = session.root_id();
    for (path, bytes) in files {
        let (folder, name) = match path.rsplit_once('/') {
            Some((folder, name)) => {
                let existing = session
                    .children_of(root)
                    .into_iter()
                    .find(|&id| session.node(id).is_some_and(|n| n.name == folder));
                let id = existing.unwrap_or_else(|| {
                    session
                        .add_folder(root, folder)
                        .expect("a folder the test names")
                });
                (id, name)
            }
            None => (root, *path),
        };
        session
            .add_file_copy(folder, name, bytes.to_vec())
            .expect("a file the test names");
    }
    let mut state = AppState::new();
    let doc_id = state.alloc_doc_id();
    let mut doc = Document::new(
        doc_id,
        Some(PathBuf::from("fixture.rpf")),
        DocPayload::Rpf(RpfDocument {
            session,
            expanded: Default::default(),
            focused_node: None,
            is_game_root: false,
            entry_documents: Default::default(),
        }),
    );
    doc.dirty = false;
    state.documents.push(doc);
    (state, doc_id, root)
}

fn session(state: &AppState, doc: DocumentId) -> &RpfEditSession {
    match &state.document(doc).expect("the document").payload {
        DocPayload::Rpf(rpf) => &rpf.session,
        _ => panic!("an archive tree"),
    }
}

fn child(state: &AppState, doc: DocumentId, folder: RpfNodeId, name: &str) -> Option<RpfNodeId> {
    let session = session(state, doc);
    session
        .children_of(folder)
        .into_iter()
        .find(|&id| session.node(id).is_some_and(|n| n.name == name))
}

/// The row at `path`, each folder on the way scanned as the tree scans it.
pub(super) fn node_at(state: &mut AppState, doc: DocumentId, path: &str) -> RpfNodeId {
    let mut node = session(state, doc).root_id();
    for name in path.split('/') {
        state.request_expand(doc, node);
        let deadline = Instant::now() + Duration::from_secs(30);
        while state.expander.is_in_flight(doc, node) {
            state.poll_expansions();
            assert!(Instant::now() < deadline, "the tree's scan never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
        node = child(state, doc, node, name).unwrap_or_else(|| panic!("the tree has no {name}"));
    }
    node
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn deleting_a_loose_folder_asks_first_and_sends_it_to_the_recycle_bin() {
    let (tmp, loose, mut state, doc) = loose_folder(&[
        ("props/chair.ydr", &[7; 12]),
        ("props/lod/chair_lod.ydr", b"lod"),
        ("keep.txt", b"stays"),
    ]);
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let props = node_at(&mut state, doc, "props");

    state
        .rpf_delete_node(doc, props)
        .expect("a delete outside an install asks rather than refuses");
    let asked = state.pending_delete.clone().expect("the delete asks first");
    assert!(
        loose.join("props/lod/chair_lod.ydr").is_file(),
        "nothing goes before the answer"
    );
    assert_eq!(asked.from, DeleteFrom::Disk);
    assert_eq!(
        asked.contents(),
        "2 files in 2 folders, 15 B",
        "the question says how many and how much, the folder's unscanned files included"
    );
    assert_eq!(
        asked.verb(),
        if cfg!(windows) {
            "Move to Recycle Bin"
        } else {
            "Move to Trash"
        }
    );

    state.answer_delete(false);
    assert!(state.pending_delete.is_none());
    assert!(loose.join("props/chair.ydr").is_file(), "a no keeps it");
    assert!(child(&state, doc, session(&state, doc).root_id(), "props").is_some());

    state.rpf_delete_node(doc, props).expect("asks again");
    state.answer_delete(true);
    assert!(!loose.join("props").exists(), "a yes takes it away");
    assert_eq!(
        read(&bin.join("props/lod/chair_lod.ydr")),
        b"lod",
        "into the bin, whole"
    );
    assert!(
        child(&state, doc, session(&state, doc).root_id(), "props").is_none(),
        "and out of the tree"
    );
    assert_eq!(read(&loose.join("keep.txt")), b"stays");
}

#[test]
fn a_delete_inside_an_archive_asks_and_waits_for_the_save() {
    let (mut state, doc, root) = archive_of(&[
        ("a.ytd", b"one"),
        ("models/b.ydr", b"two"),
        ("models/c.ydr", b"three"),
    ]);
    let models = child(&state, doc, root, "models").expect("models");

    state.rpf_delete_node(doc, models).expect("asks");
    let asked = state.pending_delete.clone().expect("the delete asks first");
    assert_eq!(asked.from, DeleteFrom::Session(Some("fixture.rpf".into())));
    assert_eq!(asked.contents(), "2 files in 1 folder, 8 B");
    assert!(
        asked.consequence().contains("fixture.rpf"),
        "the question says the archive changes only when saved: {}",
        asked.consequence()
    );

    // No Recycle Bin is set on this thread, so a delete that reached the disk fails here.
    state.answer_delete(true);
    assert!(child(&state, doc, root, "models").is_none(), "the row goes");
    assert!(child(&state, doc, root, "a.ytd").is_some());
    assert!(
        state.document(doc).expect("the archive").dirty,
        "and the archive has a change to save"
    );
}

#[test]
fn a_rename_onto_an_existing_name_leaves_both_files_as_they_were() {
    let (_tmp, loose, mut state, doc) = loose_folder(&[("a.txt", b"first"), ("b.txt", b"second")]);
    let a = node_at(&mut state, doc, "a.txt");

    assert!(state.rpf_rename_node(doc, a, "b.txt").is_err());
    assert_eq!(read(&loose.join("a.txt")), b"first");
    assert_eq!(read(&loose.join("b.txt")), b"second");

    std::fs::write(loose.join("c.txt"), b"third").expect("a file the tree has not listed");
    assert!(
        state.rpf_rename_node(doc, a, "c.txt").is_err(),
        "a name taken on disk is taken, listed or not"
    );
    assert_eq!(read(&loose.join("a.txt")), b"first");
    assert_eq!(read(&loose.join("c.txt")), b"third");

    state
        .rpf_rename_node(doc, a, "A.txt")
        .expect("a rename that changes only the case does not clash with itself");
    state
        .rpf_rename_node(doc, a, "d.txt")
        .expect("a free name renames");
    assert_eq!(read(&loose.join("d.txt")), b"first");
    assert!(!loose.join("a.txt").exists());
}

#[test]
fn a_move_onto_an_existing_name_leaves_both_files_as_they_were() {
    let (_tmp, loose, mut state, doc) = loose_folder(&[
        ("a.txt", b"outer"),
        ("b.txt", b"free"),
        ("sub/a.txt", b"inner"),
    ]);
    let (a, b, sub) = (
        node_at(&mut state, doc, "a.txt"),
        node_at(&mut state, doc, "b.txt"),
        node_at(&mut state, doc, "sub"),
    );

    assert!(
        state.rpf_move_node(doc, a, sub).is_err(),
        "`sub` is not scanned, so only the disk knows its a.txt"
    );
    assert_eq!(read(&loose.join("a.txt")), b"outer");
    assert_eq!(read(&loose.join("sub/a.txt")), b"inner");

    state.rpf_move_node(doc, b, sub).expect("a free name moves");
    assert_eq!(read(&loose.join("sub/b.txt")), b"free");
    assert!(!loose.join("b.txt").exists());
}

#[test]
fn an_import_onto_an_existing_name_leaves_the_file_as_it_was() {
    let (tmp, loose, mut state, doc) = loose_folder(&[("a.txt", b"mine")]);
    let outside = tmp.path().join("outside");
    std::fs::create_dir(&outside).expect("outside");
    std::fs::write(outside.join("a.txt"), b"theirs").expect("a.txt");
    std::fs::write(outside.join("b.txt"), b"new").expect("b.txt");
    node_at(&mut state, doc, "a.txt");
    let root = session(&state, doc).root_id();

    assert!(state
        .rpf_import_file_to_folder(doc, root, outside.join("a.txt"))
        .is_err());
    assert_eq!(read(&loose.join("a.txt")), b"mine");

    state
        .rpf_import_file_to_folder(doc, root, outside.join("b.txt"))
        .expect("a free name imports");
    assert_eq!(read(&loose.join("b.txt")), b"new");
}

/// A destination already holding two of the three files the archive extracts.
fn a_clashing_extraction() -> (
    tempfile::TempDir,
    PathBuf,
    AppState,
    DocumentId,
    ExtractStart,
) {
    let (mut state, doc, root) = archive_of(&[
        ("a.ytd", b"one"),
        ("models/b.ydr", b"two"),
        ("models/c.ydr", b"three"),
    ]);
    let tmp = tempfile::tempdir().expect("tempdir");
    let dest = tmp.path().join("out");
    std::fs::create_dir_all(dest.join("models")).expect("dest");
    std::fs::write(dest.join("a.ytd"), b"mine").expect("a.ytd");
    std::fs::write(dest.join("models/b.ydr"), b"mine too").expect("b.ydr");
    let started =
        state.extract_rpf_node(doc, root, dest.clone(), BodyForm::Decoded, TypeFilter::All);
    (tmp, dest, state, doc, started)
}

fn contents(dest: &Path) -> Vec<(String, Vec<u8>)> {
    files_under(dest)
        .into_iter()
        .map(|rel| {
            let bytes = read(&dest.join(&rel));
            (rel, bytes)
        })
        .collect()
}

#[test]
fn extracting_over_existing_files_asks_once_and_writes_nothing_until_answered() {
    let (_tmp, dest, mut state, _, started) = a_clashing_extraction();
    assert_eq!(started, ExtractStart::AwaitingConfirmation);
    assert!(!state.extract_jobs.is_running());
    let clash = state
        .extract_jobs
        .clash
        .as_ref()
        .expect("one question for the whole job");
    assert_eq!((clash.clashes, clash.planned), (2, 3));
    let before = contents(&dest);
    assert_eq!(before.len(), 2);

    state.answer_extract_clash(None);
    assert!(!state.extract_jobs.is_running(), "cancel extracts nothing");
    assert_eq!(contents(&dest), before);
}

#[test]
fn the_answer_to_a_clash_holds_for_every_clash_in_the_job() {
    let mine: &[(&str, &[u8])] = &[("a.ytd", b"mine"), ("models/b.ydr", b"mine too")];
    let ours: &[(&str, &[u8])] = &[("a.ytd", b"one"), ("models/b.ydr", b"two")];
    let kept: &[(&str, &[u8])] = &[
        ("a (2).ytd", b"one"),
        ("a.ytd", b"mine"),
        ("models/b (2).ydr", b"two"),
        ("models/b.ydr", b"mine too"),
    ];
    for (answer, expected) in [
        (OnClash::Replace, ours),
        (OnClash::Skip, mine),
        (OnClash::KeepBoth, kept),
    ] {
        let (_tmp, dest, mut state, _, _) = a_clashing_extraction();
        state.answer_extract_clash(Some(answer));
        run_to_completion(&mut state);
        let mut expected: Vec<(String, Vec<u8>)> = expected
            .iter()
            .map(|(rel, bytes)| (rel.to_string(), bytes.to_vec()))
            .collect();
        expected.push(("models/c.ydr".into(), b"three".to_vec()));
        expected.sort();
        assert_eq!(contents(&dest), expected, "{answer:?}");
    }
}
