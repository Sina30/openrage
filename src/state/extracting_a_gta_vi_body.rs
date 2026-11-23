//! A body a scan of a GTA VI archive found leaves it as a file of its own, from rows of the list
//! or Save As, and a resource left as it is stored opens again from that file.

use std::io::Cursor;
use std::time::{Duration, Instant};

use crate::state::opening_an_rpf9_archive::{
    headerless_rbm, row_at, rows, rpf9_head, said, scanned, settle, written, zlib, CHANGE_SET,
};
use crate::state::rpf9_scan::Row;
use crate::state::{AppState, DocPayload, ToastType};

/// An archive of a headerless RBM resource at 0x1000 and a registration document at 0x2000.
fn archive() -> Vec<u8> {
    let mut bytes = rpf9_head(1, 0x1A0);
    for body in [zlib(&headerless_rbm(0x5000_00E0)), zlib(CHANGE_SET)] {
        bytes.resize(bytes.len().next_multiple_of(0x1000), 0);
        bytes.extend(body);
    }
    bytes
}

/// Wait for the extraction in flight to end.
fn finished(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.is_running() {
        state.poll_extract_job();
        assert!(Instant::now() < deadline, "the extraction never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn rows_extract_as_files_named_by_their_offset_and_a_resource_opens_again_from_its_own() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let doc = scanned(&mut state, written(dir.path(), &archive()));
    let found = rows(&state, doc);
    let out = dir.path().join("out");
    let picked = [row_at(&found, 0x1000), row_at(&found, 0x2000)];
    assert!(state.extract_scanned_bodies(doc, &picked, out.clone()));
    finished(&mut state);

    let stored = std::fs::read(out.join("0x1000.rsc")).expect("the resource");
    let deflated = zlib(&headerless_rbm(0x5000_00E0));
    assert_eq!(stored[..deflated.len()], deflated[..], "as it is stored");
    assert_eq!(
        std::fs::read(out.join("0x2000.xml")).expect("the document"),
        CHANGE_SET
    );

    state.load_file(out.join("0x1000.rsc"));
    settle(&mut state);
    let opened = (state.active_document)
        .filter(|&d| d != doc)
        .unwrap_or_else(|| panic!("the loose resource: {}", state.error_dialog_message));
    let d = state.document(opened).expect("open");
    let DocPayload::Meta(meta) = &d.payload else {
        panic!("the RBM reader's document");
    };
    assert!(meta.text.encode().windows(2).any(|w| w == b"77"));
    assert!(d.read_only.is_some());

    state.toasts.clear();
    assert!(!state.extract_scanned_bodies(doc, &[row_at(&found, 0)], out));
    assert!(said(&state, ToastType::Warning, &["Nothing to extract"]));
}

#[test]
fn an_extracted_bound_cell_and_animation_each_open_from_their_own_file_in_the_view_of_their_kind() {
    use crate::state::DocumentKind;
    use crate::ui::dock::Workspace;
    let _serial = crate::ui::theme::serial();
    let (root, stored) = crate::state::opening_an_rpf9_archive::animation_set();
    let bodies = [
        zlib(&crate::state::opening_a_gta_vi_body::bound_body()),
        zlib(&crate::state::opening_an_rpf9_archive::navmesh_cell()),
        zlib(&root),
        stored,
    ];
    let mut bytes = rpf9_head(1, 0x1A0);
    for body in bodies {
        // A headerless resource follows a zero 16-byte slot at the start of a 4 KiB page.
        bytes.resize(bytes.len().next_multiple_of(0x1000) + 16, 0);
        bytes.extend(body);
    }
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let doc = scanned(&mut state, written(dir.path(), &bytes));
    let found = rows(&state, doc);
    let offsets = [0x1010, 0x2010, 0x3010, 0x4010];
    let picked = offsets.map(|at| row_at(&found, at));
    let out = dir.path().join("out");
    assert!(state.extract_scanned_bodies(doc, &picked, out.clone()));
    finished(&mut state);

    let expected = [
        ("0x1010.rsc", DocumentKind::Gta6Drawable, Workspace::Model),
        ("0x2010.rsc", DocumentKind::Ynv, Workspace::Model),
        ("0x3010.rsc", DocumentKind::Gta6Anim, Workspace::Animation),
        ("0x4010.rsc8", DocumentKind::Gta6Anim, Workspace::Animation),
    ];
    for (name, kind, workspace) in expected {
        state.active_workspace = Workspace::Browse;
        state.load_file(out.join(name));
        settle(&mut state);
        let opened = (state.active_document)
            .filter(|&d| d != doc)
            .unwrap_or_else(|| panic!("{name}: {}", state.error_dialog_message));
        let d = state.document(opened).expect("open");
        assert_eq!(d.kind, kind, "{name}");
        assert!(d.read_only.is_some(), "{name}");
        assert_eq!(state.active_workspace, workspace, "{name}");
        let outline = crate::state::capabilities::for_payload(&d.payload).and_then(|r| r.outline);
        assert!(outline.is_some(), "{name} has an Outline");
        state.close_document(opened);
    }
}

#[test]
fn save_as_writes_a_copy_of_a_scanned_body_or_its_xml_and_save_still_refuses() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bytes = archive();
    let mut state = AppState::new();
    let archive_path = written(dir.path(), &bytes);
    let doc = scanned(&mut state, archive_path.clone());
    state.open_scanned_body(doc, row_at(&rows(&state, doc), 0x1000));
    settle(&mut state);
    let body = (state.active_document)
        .filter(|&d| d != doc)
        .unwrap_or_else(|| panic!("the body: {}", state.error_dialog_message));
    let path = state.document(body).and_then(|d| d.path.clone());

    assert!(state.save_active_document_as(dir.path().join("copy.xml")));
    finished(&mut state);
    let xml = std::fs::read_to_string(dir.path().join("copy.xml")).expect("the XML");
    assert!(
        xml.trim_start().starts_with('<') && xml.contains("77"),
        "{xml}"
    );

    assert!(state.save_active_document_as(dir.path().join("copy.rsc")));
    finished(&mut state);
    let (_, expected) =
        or_io::container::rpf::extract_rpf9_body(Cursor::new(&bytes), 0x1000, u64::MAX)
            .expect("it extracts");
    assert_eq!(
        std::fs::read(dir.path().join("copy.rsc")).expect("the copy"),
        expected
    );
    assert_eq!(state.document(body).and_then(|d| d.path.clone()), path);

    state.toasts.clear();
    let same = dir
        .path()
        .join("sub")
        .join("..")
        .join(archive_path.file_name().expect("named"));
    std::fs::create_dir(dir.path().join("sub")).expect("a sub folder");
    assert!(!state.save_active_document_as(same));
    finished(&mut state);
    assert!(said(&state, ToastType::Error, &["never over the archive"]));
    assert_eq!(std::fs::read(&archive_path).expect("the archive"), bytes);

    state.toasts.clear();
    assert!(!state.save_active_document());
    assert!(said(&state, ToastType::Warning, &["read-only"]));
}

#[test]
fn an_rbf_body_a_scan_finds_opens_through_the_metadata_reader() {
    use or_io::meta::{BinaryFormat, MetaInstance, Value};
    let mut root = MetaInstance::new("CTestRoot");
    root.set("count", Value::UInt(41));
    let rbf =
        or_io::meta::binary::write_meta_binary(BinaryFormat::Rbf, None, &root).expect("written");
    let mut bytes = rpf9_head(1, 0x1A0);
    bytes.resize(0x1000, 0);
    bytes.extend(zlib(&rbf));
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let doc = scanned(&mut state, written(dir.path(), &bytes));
    let found = rows(&state, doc);
    let row = row_at(&found, 0x1000);
    assert!(matches!(&found[row], Row::Body(find) if find.file_name() == "0x1000.ymt"));
    state.open_scanned_body(doc, row);
    settle(&mut state);
    let opened = (state.active_document)
        .filter(|&d| d != doc)
        .unwrap_or_else(|| panic!("the RBF body: {}", state.error_dialog_message));
    let Some(DocPayload::Meta(meta)) = state.document(opened).map(|d| &d.payload) else {
        panic!("a metadata document");
    };
    assert_eq!(meta.type_name(), "CTestRoot");
    assert!(meta.text.encode().windows(2).any(|w| w == b"41"));
}

#[test]
fn save_as_refuses_a_scanned_body_whose_edits_no_writer_takes_rather_than_drop_them() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let doc = scanned(&mut state, written(dir.path(), &archive()));
    state.open_scanned_body(doc, row_at(&rows(&state, doc), 0x1000));
    settle(&mut state);
    let body = (state.active_document)
        .filter(|&d| d != doc)
        .unwrap_or_else(|| panic!("the body: {}", state.error_dialog_message));
    // An edit of a view whose body has no writer marks the document and leaves its bytes.
    state.document_mut(body).expect("open").mark_edited();
    state.toasts.clear();
    assert!(!state.save_active_document_as(dir.path().join("copy.rsc")));
    finished(&mut state);
    assert!(said(&state, ToastType::Error, &["would drop them"]));
    assert!(!dir.path().join("copy.rsc").exists());
}
