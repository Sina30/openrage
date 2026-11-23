//! A `.nametable` opens as its own kind, its names one per line, and saves
//! back through the reader's writer.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use or_io::gta5::data::nametable;
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

use super::opening_a_ydd::{finish_load, node_at, with_keys};
use super::{AppState, DocPayload, DocumentId, DocumentKind, LoadingState};

/// The owner's report: this file, read through the tree, did not open.
const OWNERS_NAMETABLE: &str =
    "update/x64/dlcpacks/mp2023_01/dlc.rpf/x64/audio/dlcmp2023_1_amp.dat10.nametable";

fn open(state: &mut AppState, path: PathBuf) -> DocumentId {
    state.load_file(path);
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.load_receiver.is_some() && Instant::now() < deadline {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(state.loading, LoadingState::Idle),
        "the load did not finish: {}",
        state.error_dialog_message
    );
    state.active_document.expect("the .nametable opened")
}

/// The document's lines, or a panic naming what it opened as instead.
fn lines(state: &AppState, doc: DocumentId) -> Vec<String> {
    let doc = state.document(doc).expect("the document");
    assert_eq!(doc.kind, DocumentKind::NameTable);
    let DocPayload::NameTable(text) = &doc.payload else {
        panic!("opened as {:?}", doc.payload.kind());
    };
    text.text.to_string().lines().map(str::to_owned).collect()
}

fn set_text(state: &mut AppState, doc: DocumentId, text: &str) {
    let doc = state.document_mut(doc).expect("the document");
    let DocPayload::NameTable(table) = &mut doc.payload else {
        panic!("not a name table");
    };
    table.text = ropey::Rope::from_str(text);
    doc.mark_edited();
}

/// The bytes `state` saved to `path`, or the error it reported.
fn save(state: &mut AppState, path: PathBuf) -> Result<Vec<u8>, String> {
    state.clear_error();
    assert!(state.save_active_document_as(path.clone()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    if !state.error_dialog_message.is_empty() {
        return Err(state.error_dialog_message.clone());
    }
    std::fs::read(&path).map_err(|e| e.to_string())
}

#[test]
fn a_nametable_opens_as_its_own_kind_listing_its_names() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("dlctest_amp.dat10.nametable");
    std::fs::write(&path, nametable::write(&["BIFTA", "KALAHARI"])).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    assert_eq!(lines(&state, doc), ["BIFTA", "KALAHARI"]);
    assert!(!state.document(doc).expect("doc").dirty);
}

#[test]
fn an_edited_nametable_saves_and_reopens_equal() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("dlctest_amp.dat10.nametable");
    std::fs::write(&path, nametable::write(&["BIFTA", "KALAHARI"])).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);

    set_text(&mut state, doc, "KALAHARI\nBIFTA\n\nNEW_NAME\n");
    let saved_path = dir.path().join("saved.dat10.nametable");
    let saved = save(&mut state, saved_path.clone()).expect("the save");
    assert_eq!(
        nametable::parse(&saved)
            .expect("the reader reads the save")
            .names,
        ["KALAHARI", "BIFTA", "NEW_NAME"]
    );

    let mut again = AppState::new();
    let reopened = open(&mut again, saved_path);
    assert_eq!(lines(&again, reopened), ["KALAHARI", "BIFTA", "NEW_NAME"]);
}

/// ⚠ The reader skips such an entry, so writing it would lose it on the next open.
#[test]
fn a_line_that_cannot_be_a_name_is_refused_by_name_at_save() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("dlctest_amp.dat10.nametable");
    std::fs::write(&path, nametable::write(&["BIFTA"])).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);

    set_text(&mut state, doc, "BIFTA\nBAD\tNAME\n");
    let error = save(&mut state, dir.path().join("saved.nametable")).expect_err("refused");
    assert!(
        error.contains("line 2") && error.contains("BAD\\tNAME"),
        "{error}"
    );
    assert!(state.document(doc).expect("doc").dirty);
}

#[test]
fn the_owners_nametable_opens_from_the_tree() {
    const WHAT: &str = "the owner's .dat10.nametable opens from the Legacy install's tree";
    let Some(root) = game_roots().into_iter().find(|root| {
        root.join("GTA5.exe").is_file()
            && root.join("update/x64/dlcpacks/mp2023_01/dlc.rpf").is_file()
    }) else {
        skip_install(
            WHAT,
            format_args!("no Legacy install with mp2023_01; set {GAME_ROOT_ENV}"),
        );
        return;
    };

    let mut state = with_keys(&root);
    state.open_game_root(root);
    let game = state.documents[0].id;
    let node = node_at(&mut state, game, OWNERS_NAMETABLE);
    state.clear_error();
    state.open_rpf_entry(game, node);
    let doc = finish_load(&mut state).expect("the owner's .nametable opens");
    assert!(!lines(&state, doc).is_empty(), "{WHAT}: no names");
}
