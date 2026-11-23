//! The viewport, its wireframe and a document's thumbnail are built again whenever the document's
//! contents change: after every edit, and after a reload from disk.

use or_io::desc::ScalarValue;
use or_test_support::skip_corpus;

use super::an_edit_stays_where_it_was_made::{
    drawable_with_a_material, edit_a_material, grid_row, open,
};
use super::document_io::empty_file_bytes_for_kind;
use super::{AppState, DocPayload, DocumentId, DocumentKind};
use crate::render::thumbnails::ThumbKey;
use crate::render::viewport::{LinesCacheKey, MeshCacheKey};
use crate::ui::panels::selection_ids::DOC_ROOT_SEL;

type Keys = (MeshCacheKey, LinesCacheKey, ThumbKey);

/// What the viewport's meshes, its wireframe and `doc`'s thumbnail are compared on before they are built again.
fn keys(state: &AppState, doc: DocumentId) -> Keys {
    let document = state.document(doc).expect("open");
    (
        MeshCacheKey::of(state, doc, 0),
        LinesCacheKey::of(state, doc),
        ThumbKey::of(document, DOC_ROOT_SEL),
    )
}

/// Whether the meshes, the wireframe and the thumbnail are built again going from `before` to `after`.
fn rebuilt(before: Keys, after: Keys) -> [bool; 3] {
    [
        before.0 != after.0,
        before.1 != after.1,
        before.2 != after.2,
    ]
}

#[test]
fn two_edits_before_a_save_each_draw_the_drawable_again() {
    const WHAT: &str = "two_edits_before_a_save_each_draw_the_drawable_again";
    let dir = tempfile::tempdir().expect("a scratch directory");
    let Some((path, material)) = drawable_with_a_material(dir.path()) else {
        skip_corpus(WHAT, "no staged Legacy .ydr carries a material value");
        return;
    };
    let mut state = AppState::new();
    state.documents.clear();
    let doc = open(&mut state, &path);
    let opened = keys(&state, doc);

    // A frame of a drag in the grid writes the image, and the gesture's end reads the payload back.
    let (label, edit) = grid_row(&state, doc, "Distances [0]");
    assert!(state.patch_scalar(doc, &label, edit, ScalarValue::F32(1234.5)));
    let dragging = keys(&state, doc);
    state.seal_edits();
    let read = keys(&state, doc);
    assert_eq!(
        rebuilt(dragging, read),
        [true; 3],
        "the grid edit read back at its gesture's end was not drawn: meshes, wireframe, thumbnail"
    );
    assert_eq!(
        rebuilt(opened, dragging),
        [false; 3],
        "a frame of the gesture built the unchanged payload again"
    );

    edit_a_material(&mut state, doc, material);
    let second = keys(&state, doc);
    assert_eq!(
        rebuilt(read, second),
        [true; 3],
        "the second edit before a save was not drawn"
    );

    assert!(state.undo().is_some());
    assert_eq!(
        rebuilt(second, keys(&state, doc)),
        [true; 3],
        "undoing the second edit was not drawn"
    );
}

#[test]
fn a_particle_file_reloaded_from_disk_over_its_edits_is_drawn_again() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let file = dir.path().join("smoke.ypt");
    let blank = empty_file_bytes_for_kind(DocumentKind::Ypt).expect("a blank .ypt");
    std::fs::write(&file, blank).expect("the file on disk");
    let mut state = AppState::new();
    state.documents.clear();
    let payload = DocPayload::Resource(or_io::Asset::Particles(Default::default()));
    let doc = state.insert_document(file.clone(), payload, None, None);
    state.document_mut(doc).expect("open").mark_edited();
    let edited = keys(&state, doc);

    state.reload_ypt_from_disk(&file, true);
    assert_eq!(
        rebuilt(edited, keys(&state, doc)),
        [true; 3],
        "the file read from disk is drawn as the edits it replaced left it"
    );
}
