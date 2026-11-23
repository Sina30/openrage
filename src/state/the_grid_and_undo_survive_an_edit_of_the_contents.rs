//! The grid and undo survive an edit of the contents: a grid step undoes past an edit of the
//! contents made after it, and the declared grid is drawn and edited after one.

use std::sync::Arc;

use or_io::desc::ScalarValue;
use or_test_support::skip_corpus;

use super::an_edit_stays_where_it_was_made::{
    drawable_with_a_material, grid_row, open, texts, Inspector,
};
use super::every_edit_undoes::{drawable, float_field};
use super::{drawable_mut, AppState, Change, DocPayload, DocumentId, EditHistory, ResourceImage};
use crate::ui::panels::selection_ids::{pack_index, DOC_ROOT_SEL, SECT_SHADER};

/// A staged Legacy drawable opened from a scratch copy and active, and its material that carries a float.
struct Opened {
    _dir: tempfile::TempDir,
    state: AppState,
    doc: DocumentId,
    shader: usize,
}

impl Opened {
    fn new(what: &str) -> Option<Self> {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let Some((path, (shader, _))) = drawable_with_a_material(dir.path()) else {
            skip_corpus(what, "no staged Legacy .ydr carries a material value");
            return None;
        };
        let mut state = AppState::new();
        state.documents.clear();
        let doc = open(&mut state, &path);
        state.set_active_document(doc);
        Some(Self {
            _dir: dir,
            state,
            doc,
            shader,
        })
    }

    /// Its first switch distance, how many entries the material holds, and the whole drawable.
    fn contents(&self) -> (f32, usize, String) {
        let doc = self.state.document(self.doc).expect("open");
        let DocPayload::Resource(or_io::Asset::Drawable(drawable)) = &doc.payload else {
            panic!("not a Legacy drawable");
        };
        let shaders = &drawable
            .base
            .shader_group
            .as_deref()
            .expect("materials")
            .shaders;
        let distance = drawable.base.lod_group.lod_distances[0];
        let entries = shaders[self.shader].entries.len();
        (distance, entries, format!("{drawable:?}"))
    }

    /// A click on the material's first Remove in the inspector, as a user drops one of its values.
    fn remove_a_material_value(&mut self) {
        let shader = pack_index(SECT_SHADER, self.shader as u32);
        self.state.selection.select(self.doc, shader);
        Inspector::default().click(&mut self.state, "Remove", egui::PointerButton::Primary);
        // As the shell does once the pointer is up.
        self.state.seal_edits();
    }
}

/// The image `doc` holds now.
fn image(state: &AppState, doc: DocumentId) -> Option<Vec<u8>> {
    let doc = state.document(doc).expect("open");
    doc.resource.as_ref().map(|image| image.data.clone())
}

#[test]
fn a_grid_edit_and_the_material_edit_after_it_both_undo() {
    let Some(mut o) = Opened::new("a_grid_edit_and_the_material_edit_after_it_both_undo") else {
        return;
    };
    let (opened, opened_image) = (o.contents(), image(&o.state, o.doc));
    let (label, edit) = grid_row(&o.state, o.doc, "Distances [0]");
    let distance = ScalarValue::F32(opened.0 + 137.5);
    assert!(o.state.patch_scalar(o.doc, &label, edit, distance));
    o.state.seal_edits();
    o.remove_a_material_value();
    let edited = o.contents();
    assert!(
        edited.0 != opened.0 && edited.1 + 1 == opened.1,
        "the grid edit and the material edit did not both land"
    );

    assert_eq!(
        o.state.undo().as_deref(),
        Some("Remove a material parameter")
    );
    assert_eq!(
        o.state.undo(),
        Some(format!("Set {label}")),
        "the grid's step did not undo past the material edit after it"
    );
    assert!(
        o.contents() == opened,
        "undone twice, the drawable is not as it opened"
    );
    assert!(
        image(&o.state, o.doc) == opened_image,
        "undone twice, the image is not the one the file opened with"
    );
    assert!(
        !o.state.document(o.doc).expect("open").dirty,
        "undone to as it opened, it has something to save"
    );
}

#[test]
fn the_declared_grid_is_drawn_and_editable_after_a_material_edit() {
    let Some(mut o) = Opened::new("the_declared_grid_is_drawn_and_editable_after_a_material_edit")
    else {
        return;
    };
    let (label, _) = grid_row(&o.state, o.doc, "Distances [0]");
    o.remove_a_material_value();
    let removed = o.contents();

    o.state.selection.select(o.doc, DOC_ROOT_SEL);
    let mut inspector = Inspector::default();
    // A frame, then what the shell does once the pointer is up and nothing holds the keyboard.
    inspector.frame(&mut o.state, Vec::new());
    o.state.seal_edits();
    let shapes = inspector.frame(&mut o.state, Vec::new());
    assert!(
        texts(&shapes).iter().any(|(text, _)| *text == label),
        "the declared grid is not drawn after a material edit"
    );

    inspector.drag(&mut o.state, &label, 40.0);
    o.state.seal_edits();
    let (distance, entries, _) = o.contents();
    assert_ne!(
        distance, removed.0,
        "a drag in the grid drawn after a material edit wrote nothing"
    );
    assert_eq!(
        entries, removed.1,
        "the grid's edit took back the material edit before it"
    );
}

/// Sets the second switch distance in `doc`'s contents to `to`, as an editor of the contents does.
fn set_a_distance(state: &mut AppState, doc: DocumentId, to: f32) {
    let document = state.document_mut(doc).expect("open");
    let drawable = drawable_mut(&mut document.payload, 0).expect("a Legacy drawable");
    let was = std::mem::replace(&mut drawable.lod_group.lod_distances[1], to);
    let change = Change::part("distance 1", was, |p| {
        drawable_mut(p, 0)?.lod_group.lod_distances.get_mut(1)
    });
    state.record(doc, "Set a distance", change);
}

#[test]
fn undoing_an_edit_of_the_contents_puts_back_the_image_the_grid_step_before_it_wrote() {
    let mut state = AppState::new();
    state.documents.clear();
    let doc = drawable(&mut state, "probe.ydr");
    let opened = image(&state, doc);
    let (label, edit) = float_field(&state, doc, 0);
    assert!(state.patch_scalar(doc, &label, edit, ScalarValue::F32(137.5)));
    let written = image(&state, doc);
    set_a_distance(&mut state, doc, 5.0);

    assert!(state.undo().is_some());
    assert!(
        image(&state, doc) == written,
        "undoing the edit of the contents did not put back the image the grid wrote"
    );
    assert!(state.undo().is_some(), "the grid's step did not undo");
    assert!(image(&state, doc) == opened);
    assert!(state.redo().is_some() && state.redo().is_some());
    assert!(
        image(&state, doc) != written,
        "redone, the edit of the contents left an image its contents no longer match"
    );
    assert!(state.undo().is_some());
    assert!(
        image(&state, doc) == written,
        "undone again, the image is not the one the grid wrote"
    );
}

#[test]
fn the_history_keeps_an_image_only_for_a_grid_step_made_on_it_and_at_most_four() {
    let seal = or_io::container::resource::Rsc7::invent_single_chunk_seal(165, 16);
    let seal = seal.expect("a seal");
    let image = || {
        let (data, virtual_size, seal) = (vec![0; 16], 16, seal.clone());
        Some(Arc::new(ResourceImage {
            data,
            virtual_size,
            seal,
        }))
    };
    let grid = || Change::Bytes {
        at: 0,
        other: vec![0; 4],
    };
    let contents = || Change::Pose(or_anim::PoseEdits::new());
    let kept = |history: &EditHistory| {
        (history.done().iter())
            .filter(|s| s.image.is_some())
            .count()
    };

    let mut history = EditHistory::default();
    history.push("contents".into(), contents(), image());
    assert_eq!(kept(&history), 0, "no grid step was made on the image");
    for _ in 0..6 {
        history.push("grid".into(), grid(), None);
        history.push("contents".into(), contents(), image());
    }
    assert_eq!(kept(&history), EditHistory::IMAGES);
    assert!(
        history.base() != 0 && history.done()[0].image.is_none(),
        "the oldest steps were not forgotten, or the oldest kept holds an image nothing before it needs"
    );
}
