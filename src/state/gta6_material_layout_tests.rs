use std::io::Cursor;
use std::path::Path;
use std::time::{Duration, Instant};

use or_io::container::rpf::{read_rpf9_body, Rpf9Sections};
use or_io::gta6::headerless::{read_drawable, Resource};

use super::*;
use crate::state::gta6_edit::tests::drawable_body;
use crate::state::opening_an_rpf9_archive::{
    row_at, rows, rpf9_head, scanned, settle, written, zlib,
};
use crate::state::DocPayload;

/// Where the body's one shader lies, and its record's length.
const SHADER: usize = 0x180;
const SHADER_LEN: usize = 0x40;

fn shader(state: &AppState, doc: DocumentId) -> Shader {
    let Some(DocPayload::Gta6Drawable(resource)) = state.document(doc).map(|d| &d.payload) else {
        panic!("a GTA VI body");
    };
    let Resource::Drawable(d) = &**resource else {
        panic!("a drawable");
    };
    d.shaders[0].clone()
}

fn finished(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.is_running() {
        state.poll_extract_job();
        assert!(Instant::now() < deadline, "the save never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn read_back(path: &Path) -> Rpf9Sections {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (find, read) = read_rpf9_body(Cursor::new(bytes), 0, u64::MAX).expect("it reads");
    Rpf9Sections::of(&find, &read).expect("a resource")
}

#[test]
fn a_hash_is_typed_as_hash_and_eight_hex_digits_or_as_a_name() {
    assert_eq!(typed_hash(" hash_00C0FFEE "), Some(0x00C0_FFEE));
    assert_eq!(typed_hash("HASH_00c0ffee"), Some(0x00C0_FFEE));
    assert_eq!(typed_hash("RadiusScale"), Some(name_hash32(b"radiusscale")));
    assert_eq!(typed_hash("hash_zz"), None);
    assert_eq!(typed_hash("  "), None);
}

#[test]
fn a_material_laid_out_anew_undoes_and_saves_as_a_loose_resource_that_reads_back_to_it() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut bytes = rpf9_head(1, 0x1A0);
    bytes.resize(0x1010, 0);
    bytes.extend(zlib(&drawable_body()));
    let mut state = AppState::new();
    let archive = scanned(&mut state, written(dir.path(), &bytes));
    state.open_scanned_body(archive, row_at(&rows(&state, archive), 0x1010));
    settle(&mut state);
    let doc = (state.active_document)
        .filter(|&d| d != archive)
        .unwrap_or_else(|| panic!("the drawable: {}", state.error_dialog_message));
    let opened_as = shader(&state, doc);
    assert_eq!(opened_as.parameters.len(), 2);

    // A constant in the buffer's second row, a texture slot, another effect, the old constant gone.
    let edits = [
        LayoutEdit::AddConstant {
            hash: 0x3333_0002,
            buffer: 0,
            offset: 16,
            words: 4,
        },
        LayoutEdit::AddTexture(0x1111_0002),
        LayoutEdit::Effect(0x00C0_FFEE),
        LayoutEdit::Remove(2),
    ];
    for edit in edits.clone() {
        state
            .edit_gta6_material_layout(doc, None, 0, edit)
            .expect("laid out");
    }
    let edited = shader(&state, doc);
    assert_eq!(edited.name_hash, 0x00C0_FFEE);
    assert_eq!(edited.texture_keys, [opened_as.texture_keys[0], None]);
    let hashes: Vec<u32> = edited.parameters.iter().map(|p| p.hash).collect();
    assert_eq!(hashes, [0x1111_0001, 0x1111_0002, 0x3333_0002]);
    assert!(matches!(
        &edited.parameters[1].value,
        ParameterValue::Texture { slot: 1 }
    ));

    // Refused, and nothing changed: an overlap, a hash held twice, no such parameter.
    let refused = [
        LayoutEdit::AddConstant {
            hash: 0x3333_0003,
            buffer: 0,
            offset: 20,
            words: 1,
        },
        LayoutEdit::AddTexture(0x1111_0001),
        LayoutEdit::Remove(9),
    ];
    for edit in refused {
        let outcome = state.edit_gta6_material_layout(doc, None, 0, edit);
        assert!(outcome.is_err());
    }
    assert_eq!(
        shader(&state, doc),
        edited,
        "a refused edit changed nothing"
    );

    for _ in &edits {
        assert!(state.undo().is_some());
    }
    assert_eq!(shader(&state, doc), opened_as, "undone, it is as it opened");
    for _ in &edits {
        assert!(state.redo().is_some());
    }
    assert_eq!(shader(&state, doc), edited, "redone, an edit is lost");

    let saved = dir.path().join("saved.rsc");
    assert!(state.save_active_document_as(saved.clone()));
    finished(&mut state);
    let said: Vec<_> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(saved.exists(), "{said:?} {}", state.error_dialog_message);
    let sections = read_back(&saved);
    let body = drawable_body();
    let v = &sections.virtual_section;
    assert!(v.len() > body.len(), "the new layout follows the old end");
    let after = SHADER + SHADER_LEN;
    assert!(
        v[..SHADER] == body[..SHADER],
        "nothing before the shader changed"
    );
    assert!(
        v[after..body.len()] == body[after..],
        "nothing after the shader changed below the old end"
    );
    let read = read_drawable(v, Some(&sections.physical)).expect("reads");
    assert_eq!(read.shaders, std::slice::from_ref(&edited));
    let twice = dir.path().join("twice.rsc");
    assert!(state.save_active_document_as(twice.clone()));
    finished(&mut state);
    assert!(
        std::fs::read(&twice).ok() == std::fs::read(&saved).ok(),
        "a second save writes the same bytes"
    );
}
