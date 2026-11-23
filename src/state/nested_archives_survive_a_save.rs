//! Saving an archive keeps every archive nested in it, and an edit made inside
//! a nested archive saves through the archive on disk that holds it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_io::container::rpf::{BodyForm, RpfArchive};

use super::document::{DocumentId, TextDocument};
use super::opening_a_ydd::{finish_load, node_at};
use super::prefs::EditMode;
use super::save_destination::SaveDestination;
use super::{AppState, DocPayload, DocumentKind, RetailEditDecision, RpfOutputMode, ToastType};

pub(super) const EDITED: &[u8] = b"edited";

/// An unencrypted archive of `entries`.
pub(super) fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    super::document_io::serialize_archive_records(
        entries
            .iter()
            .map(|(path, bytes)| (path.to_string(), bytes.to_vec(), false))
            .collect(),
        RpfOutputMode::Open,
        Arc::new(or_io::container::NoOpCryptoProvider::new()),
    )
    .expect("archive bytes")
}

/// How a test opens `outer.rpf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Opened {
    InAnInstallTree,
    AsADocument,
}

/// `outer` written to `root/outer.rpf` and opened `how`: the document holding it, the file, and the prefix of a path inside it.
pub(super) fn open(
    state: &mut AppState,
    root: &Path,
    outer: &[u8],
    how: Opened,
) -> (DocumentId, PathBuf, &'static str) {
    let path = root.join("outer.rpf");
    std::fs::write(&path, outer).expect("the archive is written");
    match how {
        Opened::InAnInstallTree => {
            state.open_game_root(root.to_path_buf());
            let tree = state
                .documents
                .iter()
                .find(|d| d.kind == DocumentKind::GameRoot)
                .expect("the install's tree")
                .id;
            (tree, path, "outer.rpf/")
        }
        Opened::AsADocument => {
            state.load_file(path.clone());
            let doc = finish_load(state).expect("the archive opens as a document");
            (doc, path, "")
        }
    }
}

/// What the app reported going wrong.
pub(super) fn errors(state: &AppState) -> Vec<String> {
    state
        .toasts
        .iter()
        .filter(|t| t.toast_type == ToastType::Error)
        .map(|t| t.message.clone())
        .collect()
}

/// Save `doc` as Ctrl+S does, and wait for a save that runs on a thread.
pub(super) fn save(state: &mut AppState, doc: DocumentId) {
    state.set_active_document(doc);
    assert!(state.save_active_document(), "{:?}", errors(state));
    let deadline = Instant::now() + Duration::from_secs(60);
    while state.save_receiver.is_some() {
        assert!(Instant::now() < deadline, "the save did not finish");
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn on_disk(state: &AppState, path: &Path) -> Arc<RpfArchive> {
    super::rpf_archive_open::open_archive_from_disk(path, state.keys.provider())
        .expect("the saved archive opens")
}

fn index_of(archive: &RpfArchive, name: &str) -> u32 {
    archive
        .nodes
        .iter()
        .find(|n| n.name() == name)
        .unwrap_or_else(|| panic!("{name} is not in the saved archive"))
        .entry_index()
}

pub(super) fn body(archive: &RpfArchive, name: &str, form: BodyForm) -> Vec<u8> {
    archive
        .read_form(index_of(archive, name), form)
        .unwrap_or_else(|e| panic!("{name} does not read: {e}"))
}

/// The archive `archive` holds as `name`, opened from its stored bytes as the game opens it.
pub(super) fn nested(archive: &Arc<RpfArchive>, name: &str) -> Arc<RpfArchive> {
    let opened = archive.open_nested(index_of(archive, name));
    Arc::new(opened.unwrap_or_else(|e| panic!("{name} no longer opens as an archive: {e}")))
}

/// ⚠ One nested archive is browsed in the tree and one is not, the edit is
/// outside both, and each ends in a block OpenRage's builder does not write, so
/// a save that wrote either from what the tree holds of it fails.
fn untouched_nested_archives_survive(how: Opened) {
    const BROWSED: &[u8] = b"inside the browsed archive";
    const UNOPENED: &[u8] = b"inside the unopened archive";
    let browsed = [archive(&[("inside.txt", BROWSED)]), vec![0x5A; 512]].concat();
    let unopened = [archive(&[("inside.txt", UNOPENED)]), vec![0x5A; 512]].concat();
    let outer = archive(&[
        ("a.txt", b"shipped"),
        ("data/browsed.rpf", &browsed),
        ("data/unopened.rpf", &unopened),
    ]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let (doc, path, prefix) = open(&mut state, dir.path(), &outer, how);
    node_at(
        &mut state,
        doc,
        &format!("{prefix}data/browsed.rpf/inside.txt"),
    );
    let a = node_at(&mut state, doc, &format!("{prefix}a.txt"));
    state
        .update_rpf_entry_bytes(doc, a, EDITED.to_vec())
        .expect("the edit");
    save(&mut state, doc);

    let saved = on_disk(&state, &path);
    assert_eq!(
        body(&saved, "a.txt", BodyForm::Decoded),
        EDITED,
        "the edit is not saved: {:?}",
        errors(&state)
    );
    for (name, bytes, inside) in [
        ("browsed.rpf", &browsed, BROWSED),
        ("unopened.rpf", &unopened, UNOPENED),
    ] {
        let stored = body(&saved, name, BodyForm::AsStored);
        assert!(
            stored == *bytes,
            "{name} is stored as {} bytes, not as its own {}",
            stored.len(),
            bytes.len()
        );
        let reopened = nested(&saved, name);
        assert_eq!(body(&reopened, "inside.txt", BodyForm::Decoded), inside);
    }
}

#[test]
fn saving_an_install_archive_keeps_each_untouched_archive_nested_in_it_byte_for_byte() {
    untouched_nested_archives_survive(Opened::InAnInstallTree);
}

#[test]
fn saving_an_archive_document_keeps_each_untouched_archive_nested_in_it_byte_for_byte() {
    untouched_nested_archives_survive(Opened::AsADocument);
}

/// How an edit two archives deep is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Saved {
    /// From the entry's tab, back into the archive, in an install's tree.
    FromItsTabInTheTree,
    /// With the install's tree, once the edit gate is answered.
    WithTheTree,
    /// From the entry's tab, back into an archive opened as a document.
    FromItsTabInADocument,
}

/// ⚠ `deep.txt` is in `inner.rpf`, in `mid.rpf`, in `outer.rpf` on disk, so
/// the save rebuilds two nested archives and keeps what sits beside the edit.
fn an_edit_two_archives_deep_saves(how: Saved) {
    let inner = archive(&[("deep.txt", b"shipped")]);
    let mid = archive(&[("inner.rpf", &inner), ("beside.txt", b"beside")]);
    let outer = archive(&[("mid.rpf", &mid), ("a.txt", b"a")]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    state.retail_edit_session_allow = false;
    let opened = match how {
        Saved::FromItsTabInADocument => Opened::AsADocument,
        _ => Opened::InAnInstallTree,
    };
    if how == Saved::WithTheTree {
        std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
        state.prefs.edit_mode = EditMode::Everything;
    }
    let (doc, path, prefix) = open(&mut state, dir.path(), &outer, opened);
    let deep = node_at(
        &mut state,
        doc,
        &format!("{prefix}mid.rpf/inner.rpf/deep.txt"),
    );

    if how == Saved::WithTheTree {
        state
            .update_rpf_entry_bytes(doc, deep, EDITED.to_vec())
            .expect("the edit");
        state.set_active_document(doc);
        assert!(state.save_active_document());
        assert!(
            state.pending_retail_edit.is_some(),
            "the gate does not ask about the archive on disk: {:?}",
            errors(&state)
        );
        assert_eq!(
            std::fs::read(&path).expect("the archive"),
            outer,
            "nothing is written before the gate is answered"
        );
        state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    } else {
        state.open_rpf_entry(doc, deep);
        let tab = finish_load(&mut state).expect("the entry opens");
        let edited = state.document_mut(tab).expect("the entry's tab");
        edited.payload = DocPayload::Text(TextDocument::decode(EDITED));
        edited.mark_edited();
        assert!(state.save_active_document());
        let choice = state
            .save_destination
            .clone()
            .expect("saving an archive entry asks where it goes");
        assert_eq!(
            choice.archive.as_deref(),
            Some(path.as_path()),
            "back into the archive does not name the archive on disk"
        );
        state.choose_save_destination(SaveDestination::Archive);
        assert!(
            !state.document(tab).expect("the entry's tab").dirty,
            "the tab is still unsaved: {:?}",
            errors(&state)
        );
    }

    let saved = on_disk(&state, &path);
    let mid = nested(&saved, "mid.rpf");
    let inner = nested(&mid, "inner.rpf");
    assert_eq!(
        body(&inner, "deep.txt", BodyForm::Decoded),
        EDITED,
        "the edit is not two archives down: {:?}",
        errors(&state)
    );
    assert_eq!(body(&mid, "beside.txt", BodyForm::Decoded), b"beside");
    assert_eq!(body(&saved, "a.txt", BodyForm::Decoded), b"a");
    if opened == Opened::InAnInstallTree {
        let Some(DocPayload::Rpf(tree)) = state.document(doc).map(|d| &d.payload) else {
            panic!("the install's tree");
        };
        assert!(
            tree.session.dirty_archive_ids().is_empty(),
            "the save left an archive it wrote unsaved"
        );
    }
}

#[test]
fn an_edit_two_archives_deep_saves_from_its_tab_into_the_install_archive_on_disk() {
    an_edit_two_archives_deep_saves(Saved::FromItsTabInTheTree);
}

#[test]
fn an_edit_two_archives_deep_saves_with_the_install_tree_once_the_gate_is_answered() {
    an_edit_two_archives_deep_saves(Saved::WithTheTree);
}

#[test]
fn an_edit_two_archives_deep_saves_from_its_tab_into_the_archive_document_on_disk() {
    an_edit_two_archives_deep_saves(Saved::FromItsTabInADocument);
}
