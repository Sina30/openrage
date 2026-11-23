//! A file no reader opens is shown read-only as bytes, under the sentence
//! saying why there is no richer view.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{AppState, DocPayload, DocumentId, DocumentKind, HexDocument, LoadingState};

fn wait(state: &mut AppState, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.load_receiver.is_some() && Instant::now() < deadline {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(state.loading, LoadingState::Idle) && state.error_dialog_message.is_empty(),
        "{what}: {}",
        state.error_dialog_message
    );
}

fn open(state: &mut AppState, path: PathBuf) -> DocumentId {
    state.load_file(path.clone());
    wait(state, &path.display().to_string());
    state
        .active_document
        .expect("the file opened as a document")
}

fn hex(state: &AppState, doc: DocumentId) -> &HexDocument {
    match state.document(doc).map(|d| (d.kind, &d.payload)) {
        Some((DocumentKind::Hex, DocPayload::Hex(hex))) => hex,
        other => panic!("opened as {:?}", other.map(|(kind, _)| kind)),
    }
}

const BINARY_TABLE: &[u8] = b"HMAP\x01\0\0\0\x10\0\0\0\xff\xfe";

#[test]
fn a_binary_dat_with_no_reader_opens_as_hex_under_its_refusal() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("table.dat");
    std::fs::write(&path, BINARY_TABLE).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    let hex = hex(&state, doc);
    assert_eq!(
        hex.banner,
        "table.dat is a binary table, not text, and OpenRage has no reader for it"
    );
    assert_eq!(hex.bytes, BINARY_TABLE);
    assert_eq!(hex.magic(), None);
    let saved = dir.path().join("saved.dat");
    state.save_active_document_as(saved.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!saved.exists(), "the hex view is read-only");
}

#[test]
fn a_file_no_extension_names_opens_as_hex_naming_its_magic() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("probe.zzunknown");
    let bytes = [b"RSC7".as_slice(), &[0; 12]].concat();
    std::fs::write(&path, &bytes).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    let hex = hex(&state, doc);
    assert!(hex.banner.contains("probe.zzunknown"), "{}", hex.banner);
    assert_eq!(hex.magic(), Some("RAGE resource"));
}

#[test]
fn an_archive_entry_no_reader_opens_opens_as_hex() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let archive = super::document_io::serialize_archive_records(
        vec![(
            "data/probe.zzunknown".to_string(),
            BINARY_TABLE.to_vec(),
            false,
        )],
        super::RpfOutputMode::Open,
        state.keys.provider(),
    )
    .expect("archive bytes");
    let rpf = dir.path().join("probe.rpf");
    std::fs::write(&rpf, archive).expect("the archive is written");
    let archive_doc = open(&mut state, rpf);
    let entry = {
        let Some(DocPayload::Rpf(rpf)) = state.document(archive_doc).map(|d| &d.payload) else {
            panic!("the archive did not open as one");
        };
        let mut stack = vec![rpf.session.root_id()];
        let mut found = None;
        while let Some(id) = stack.pop() {
            if rpf
                .session
                .node(id)
                .is_some_and(|n| n.name == "probe.zzunknown")
            {
                found = Some(id);
            }
            stack.extend(rpf.session.children_of(id));
        }
        found.expect("the archive lists the entry")
    };
    state.open_rpf_entry(archive_doc, entry);
    wait(&mut state, "the archive entry");
    let doc = state.active_document.expect("the entry opened");
    assert_eq!(hex(&state, doc).bytes, BINARY_TABLE);
}

#[test]
fn a_file_a_reader_opens_does_not_open_as_hex() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("popcycle.dat");
    std::fs::write(&path, b"POP_SCHEDULES\n\tSCHEDULE 0\n").expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    assert_eq!(
        state.document(doc).map(|d| d.kind),
        Some(DocumentKind::Text)
    );
}

#[test]
fn an_unknown_extension_holding_text_opens_as_editable_text() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("mod_order.zzlist");
    let bytes = "; load order\nvehicles.rpf\nweapons.rpf\n".as_bytes();
    std::fs::write(&path, bytes).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    assert_eq!(
        state.document(doc).map(|d| d.kind),
        Some(DocumentKind::Text)
    );
    let saved = dir.path().join("saved.zzlist");
    state.save_active_document_as(saved.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(std::fs::read(saved).expect("the text saved"), bytes);
}

#[test]
fn an_unknown_extension_holding_invalid_utf8_opens_as_hex() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("probe.zzunknown");
    let bytes = b"\x8b\x1f\xc3\x28\xfe\xff\x80\x81";
    std::fs::write(&path, bytes).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    assert_eq!(hex(&state, doc).bytes, bytes);
}
