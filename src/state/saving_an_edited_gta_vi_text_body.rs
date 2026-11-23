//! An edit of a scanned GTA VI body's text, or of a metadata body in the form, is what Save As
//! writes: a plain text body as its text, a metadata body's XML with the form's edit in it.

use std::time::{Duration, Instant};

use or_io::meta::Value;

use crate::state::opening_an_rpf9_archive::{
    headerless_rbm, row_at, rows, rpf9_head, said, scanned, settle, written, zlib,
};
use crate::state::{AppState, DocPayload, DocumentId, ToastType};

const TEXT: &[u8] = b"first line\r\nsecond line\r\n";
/// A document whose root no schema declares, so it reads without one.
const XML: &[u8] = b"<?xml version=\"1.0\"?>\n<CTestRoot><name>before_edit</name></CTestRoot>\n";

fn finished(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.is_running() {
        state.poll_extract_job();
        assert!(Instant::now() < deadline, "the save never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The body at `offset` of the scan `doc`, opened.
fn opened(state: &mut AppState, doc: DocumentId, offset: u64) -> DocumentId {
    state.open_scanned_body(doc, row_at(&rows(state, doc), offset));
    settle(state);
    (state.active_document)
        .filter(|&d| d != doc)
        .unwrap_or_else(|| panic!("the body: {}", state.error_dialog_message))
}

/// An archive of `bodies`, each deflated at the start of a 4 KiB page from 0x1000.
fn archive_of(bodies: &[&[u8]]) -> Vec<u8> {
    let mut bytes = rpf9_head(1, 0x1A0);
    for body in bodies {
        bytes.resize(bytes.len().next_multiple_of(0x1000), 0);
        bytes.extend(zlib(body));
    }
    bytes
}

#[test]
fn an_edited_scanned_text_body_saves_as_its_text_and_an_unedited_one_as_it_is_stored() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let doc = scanned(&mut state, written(dir.path(), &archive_of(&[TEXT])));
    let body = opened(&mut state, doc, 0x1000);

    let copy = dir.path().join("copy.txt");
    assert!(state.save_active_document_as(copy.clone()));
    finished(&mut state);
    assert_eq!(std::fs::read(&copy).expect("the copy"), TEXT);

    let d = state.document_mut(body).expect("open");
    let DocPayload::Text(text) = &mut d.payload else {
        panic!("a text document");
    };
    text.text.insert(0, "new ");
    d.mark_edited();
    state.toasts.clear();
    let saved = dir.path().join("edited.txt");
    assert!(state.save_active_document_as(saved.clone()));
    finished(&mut state);
    assert!(!said(&state, ToastType::Error, &["would drop them"]));
    assert_eq!(
        std::fs::read(&saved).expect("the edited text"),
        b"new first line\r\nsecond line\r\n"
    );
    // The body stays one of its archive, which Save never writes into.
    state.toasts.clear();
    assert!(!state.save_active_document());
    assert!(said(&state, ToastType::Warning, &["read-only"]));
}

/// Open the body at `at` and give its root field `name`, else its first, `value` in the form.
fn form_edit(state: &mut AppState, doc: DocumentId, at: u64, name: Option<&str>, value: Value) {
    let body = opened(state, doc, at);
    let Some(DocPayload::Meta(meta)) = state.document_mut(body).map(|d| &mut d.payload) else {
        panic!("a metadata document");
    };
    let parsed = meta.parsed.as_mut().expect("it parsed");
    match name {
        Some(name) => parsed.instance.set(name, value),
        None => parsed.instance.fields[0].1 = value,
    }
    meta.after_visual_edit_in_place();
}

fn saved_text(state: &mut AppState, path: std::path::PathBuf) -> String {
    assert!(state.save_active_document_as(path.clone()));
    finished(state);
    std::fs::read_to_string(&path).expect("the saved file")
}

#[test]
fn a_form_edit_reaches_the_xml_export_and_the_save_of_an_xml_body() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let resource = headerless_rbm(0x5000_00E0);
    let bodies = [resource.as_slice(), XML];
    let doc = scanned(&mut state, written(dir.path(), &archive_of(&bodies)));

    // A binary body: the XML it exports holds the edited value, not the one it was read with.
    form_edit(&mut state, doc, 0x1000, None, Value::UInt(79));
    let xml = saved_text(&mut state, dir.path().join("resource.xml"));
    assert!(
        xml.contains("value=\"79\"") && !xml.contains("value=\"77\""),
        "{xml}"
    );

    // An XML body: both its XML export and its save write the edited text.
    for name in ["edited.xml", "edited.meta"] {
        form_edit(
            &mut state,
            doc,
            0x2000,
            Some("name"),
            Value::String("after_edit".into()),
        );
        let xml = saved_text(&mut state, dir.path().join(name));
        assert!(
            xml.contains("after_edit") && !xml.contains("before_edit"),
            "{name}: {xml}"
        );
    }
}
