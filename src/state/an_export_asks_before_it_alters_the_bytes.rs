//! An export writes OpenRage's canonical form, and asks first where that
//! alters the bytes of the file as it stands.
//!
//! ⚠ Each case exports the same tables and varies only the answer, so an
//! export that ignored the answer, or wrote nothing, fails one of them.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::container::rpf::BodyForm;
use or_io::gta5::text::Gxt2;
use or_io::gta5::File as _;

use super::document::{Document, DocumentId, RpfNodeId};
use super::extract_job::{ExtractStart, PendingExportFixes, TypeFilter};
use super::prefs::{EditMode, ExportFixes};
use super::rpf_session::{RpfDocument, RpfEditSession};
use super::{AppState, DocPayload};

/// A table as OpenRage writes it.
fn canonical_gxt2() -> Vec<u8> {
    let mut table = Gxt2::new();
    table.insert_key("HELLO", "hi");
    table.to_bytes().expect("a one-string table writes")
}

/// The same table with four bytes after its strings, which a reader skips and
/// a save does not write.
fn altered_gxt2() -> Vec<u8> {
    [canonical_gxt2(), vec![0; 4]].concat()
}

fn state_holding(files: &[(&str, Vec<u8>)]) -> (AppState, DocumentId, RpfNodeId) {
    let mut state = AppState::new();
    state.documents.clear();
    state.active_document = None;
    state.prefs.edit_mode = EditMode::ReadOnly;
    state.prefs.export_fixes = ExportFixes::Ask;

    let mut session = RpfEditSession::empty();
    let root = session.root_id();
    for (name, bytes) in files {
        session
            .add_file_copy(root, name, bytes.clone())
            .expect("a file the test just named");
    }
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

fn run_to_completion(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.is_running() {
        state.poll_extract_job();
        assert!(Instant::now() < deadline, "the extraction never reported");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn export(state: &mut AppState, doc: DocumentId, root: RpfNodeId, dest: &Path) -> ExtractStart {
    state.extract_rpf_node(
        doc,
        root,
        dest.to_path_buf(),
        BodyForm::Decoded,
        TypeFilter::All,
    )
}

#[test]
fn the_fixture_is_canonical_and_its_alteration_is_not() {
    let canonical = canonical_gxt2();
    assert_eq!(
        super::document_io::canonical_form("a.gxt2", &canonical),
        Some(canonical.clone())
    );
    assert_eq!(
        super::document_io::canonical_form("b.gxt2", &altered_gxt2()),
        Some(canonical),
        "the altered table must read, or no case below reaches the prompt"
    );
}

#[test]
fn a_file_already_canonical_exports_without_asking() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (mut state, doc, root) = state_holding(&[("a.gxt2", canonical_gxt2())]);
    let dest = tmp.path().join("out");
    assert_eq!(
        export(&mut state, doc, root, &dest),
        ExtractStart::Started { planned: 1 }
    );
    assert!(state.pending_export_fixes.is_none());
    run_to_completion(&mut state);
    assert_eq!(
        std::fs::read(dest.join("a.gxt2")).expect("a.gxt2"),
        canonical_gxt2()
    );
}

#[test]
fn exporting_a_file_canonicalising_alters_asks_first() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (mut state, doc, root) =
        state_holding(&[("a.gxt2", canonical_gxt2()), ("b.gxt2", altered_gxt2())]);
    let dest = tmp.path().join("out");
    assert_eq!(
        export(&mut state, doc, root, &dest),
        ExtractStart::AwaitingConfirmation,
        "canonicalising alters b.gxt2, so the export must ask before it starts"
    );
    assert!(matches!(
        state.pending_export_fixes,
        Some(PendingExportFixes::Extract { .. })
    ));
    assert!(!dest.exists(), "nothing is written while it asks");
}

#[test]
fn declining_the_fixes_writes_the_original_bytes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (mut state, doc, root) = state_holding(&[("b.gxt2", altered_gxt2())]);
    let dest = tmp.path().join("out");
    export(&mut state, doc, root, &dest);
    state.apply_export_fixes(false, false);
    assert!(state.pending_export_fixes.is_none());
    run_to_completion(&mut state);
    assert_eq!(
        std::fs::read(dest.join("b.gxt2")).expect("b.gxt2"),
        altered_gxt2()
    );
    assert_eq!(state.prefs.export_fixes, ExportFixes::Ask, "not remembered");
}

#[test]
fn accepting_the_fixes_writes_the_canonical_bytes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (mut state, doc, root) = state_holding(&[("b.gxt2", altered_gxt2())]);
    let dest = tmp.path().join("out");
    export(&mut state, doc, root, &dest);
    state.apply_export_fixes(true, false);
    run_to_completion(&mut state);
    assert_eq!(
        std::fs::read(dest.join("b.gxt2")).expect("b.gxt2"),
        canonical_gxt2()
    );
}

#[test]
fn a_remembered_choice_is_not_asked_for() {
    for (choice, expected) in [
        (ExportFixes::Apply, canonical_gxt2()),
        (ExportFixes::Keep, altered_gxt2()),
    ] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (mut state, doc, root) = state_holding(&[("b.gxt2", altered_gxt2())]);
        state.prefs.export_fixes = choice;
        let dest = tmp.path().join("out");
        assert_eq!(
            export(&mut state, doc, root, &dest),
            ExtractStart::Started { planned: 1 },
            "{choice:?}"
        );
        run_to_completion(&mut state);
        assert_eq!(
            std::fs::read(dest.join("b.gxt2")).expect("b.gxt2"),
            expected,
            "{choice:?}"
        );
    }
}

/// A raw extraction is the archive's bytes by choice, so it is never
/// canonicalised and never asks.
#[test]
fn a_form_other_than_decoded_does_not_ask() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (mut state, doc, root) = state_holding(&[("b.gxt2", altered_gxt2())]);
    let dest = tmp.path().join("out");
    let started =
        state.extract_rpf_node(doc, root, dest.clone(), BodyForm::AsStored, TypeFilter::All);
    assert_eq!(started, ExtractStart::Started { planned: 1 });
    run_to_completion(&mut state);
    assert_eq!(
        std::fs::read(dest.join("b.gxt2")).expect("b.gxt2"),
        altered_gxt2()
    );
}

/// ⚠ The drag has ended before the answer, so the answer waits for the next
/// drag out.
#[test]
fn a_drag_out_asks_and_the_next_drag_takes_the_answer() {
    let (mut state, doc, root) = state_holding(&[("b.gxt2", altered_gxt2())]);
    let rows = {
        let DocPayload::Rpf(rpf) = &state.document(doc).expect("doc").payload else {
            unreachable!()
        };
        rpf.session.children_of(root)
    };
    let first = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        state.stage_drag_out(doc, &rows, first.path()),
        Ok(Vec::new()),
        "nothing is handed to Explorer while it asks"
    );
    assert!(matches!(
        state.pending_export_fixes,
        Some(PendingExportFixes::DragOut)
    ));
    assert!(!first.path().join("b.gxt2").exists());

    state.apply_export_fixes(true, false);
    let second = tempfile::tempdir().expect("tempdir");
    let tops = state
        .stage_drag_out(doc, &rows, second.path())
        .expect("staged");
    assert_eq!(tops, vec![second.path().join("b.gxt2")]);
    assert_eq!(
        std::fs::read(second.path().join("b.gxt2")).expect("b.gxt2"),
        canonical_gxt2()
    );
    assert!(
        state.pending_export_fixes.is_none() && state.drag_out_fixes.is_none(),
        "the answer is spent on the drag that took it"
    );
}
