//! A drawable inside a scanned GTA VI dictionary offers its bones as a `.ydd`'s entry does: drawn
//! and picked under its code while it is selected, stated in the inspector, edited with undo, and
//! the skeleton a clip poses it by.

use std::path::PathBuf;
use std::sync::Arc;

use or_io::gta6::headerless::{Bone, Dictionary, Drawable, Entry, Resource, Skeleton};

use super::bone_numbers::BoneEdit;
use super::{AppState, DocPayload, DocumentId};
use crate::ui::panels::selection_ids::{dict_entry_id, dict_entry_part_id, YPT_MODEL_KIND_BONE};

const CODE: u32 = 0x20;
const BARE: u32 = 0x30;

fn drawable() -> Drawable {
    Drawable {
        name: "head_test.#dr".to_string(),
        box_min: [0.0; 3],
        box_max: [1.0; 3],
        shaders: Vec::new(),
        levels: [None, None, None, None],
        lod_distances: [25.0, 0.0, 0.0, 0.0],
        bound: None,
        embedded: None,
        extra_skeletons: Vec::new(),
    }
}

/// `SKEL_ROOT` at the origin and `SKEL_Head` 1.5 m up from it.
fn skeleton() -> Skeleton {
    let bone = |name: &str, parent, y: f32| Bone {
        name: name.to_string(),
        hash: or_io::gta6::hash::name_hash32(name.as_bytes()),
        parent,
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation: [0.0, y, 0.0],
        scale: [1.0; 3],
        inverse_bind: [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, -y, 0.0],
        ],
    };
    Skeleton {
        bones: vec![
            bone("SKEL_ROOT", None, 0.0),
            bone("SKEL_Head", Some(0), 1.5),
        ],
    }
}

/// A dictionary of two drawables, [`CODE`] holding [`skeleton`] and [`BARE`] none.
fn opened() -> (AppState, DocumentId, PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("0x40.dictionary");
    let mut state = AppState::new();
    let dictionary = Dictionary {
        entries: vec![
            (CODE, Ok(Entry::Drawable(Box::new(drawable())))),
            (BARE, Ok(Entry::Drawable(Box::new(drawable())))),
        ],
    };
    let payload = DocPayload::Gta6Drawable(Arc::new(Resource::Dictionary(dictionary)));
    let doc = state.insert_document(path.clone(), payload, None, None);
    state.active_document = Some(doc);
    super::gta6_entry_skeletons::hold(&path, vec![(CODE, skeleton())]);
    (state, doc, path, dir)
}

fn held(path: &std::path::Path) -> Skeleton {
    super::gta6_entry_skeletons::of(path, CODE).expect("the entry's skeleton")
}

#[test]
fn the_entry_selected_draws_its_own_bones_under_its_code_and_no_other_does() {
    use crate::render::viewport::bones;
    let (mut state, doc, _, _dir) = opened();
    state.preview_scene.show_rest_bones = true;
    assert!(!bones::can_draw(&state), "no entry is selected");
    state.selection.select(doc, dict_entry_id(BARE));
    assert!(!bones::can_draw(&state), "that entry holds no skeleton");
    state.selection.select(doc, dict_entry_id(CODE));
    assert!(bones::can_draw(&state));
    assert_eq!(bones::drawn_from(&state), Some(doc));
    let lines = bones::lines(&state);
    assert!(
        lines.iter().any(|v| (v.position[1] - 1.5).abs() < 1e-4),
        "the head is drawn at its bind: {lines:?}"
    );
    // A bone's row states it, and the status bar names it.
    let row = dict_entry_part_id(CODE, YPT_MODEL_KIND_BONE, 1);
    let document = state.document(doc).expect("open");
    assert!(super::gta6_selected::selected_rows(document, row).is_some());
    let summary = super::gta6_selected::summary("head_test", row).expect("a summary");
    assert!(summary.contains('1'), "{summary}");
    let other = dict_entry_part_id(BARE, YPT_MODEL_KIND_BONE, 1);
    assert!(super::gta6_selected::selected_rows(document, other).is_none());
}

#[test]
fn an_entry_s_bone_moves_with_undo_and_an_entry_with_no_skeleton_is_refused() {
    let (mut state, doc, path, _dir) = opened();
    let opened_as = held(&path);
    let moved = BoneEdit::Position([0.25, 1.5, 0.0]);
    assert!(state
        .edit_gta6_entry_bone(doc, BARE, 1, moved.clone())
        .is_err());
    assert!(state
        .edit_gta6_entry_bone(doc, CODE, 2, moved.clone())
        .is_err());
    assert!(
        state.edit_gta6_bone(doc, 1, moved.clone()).is_err(),
        "a dictionary has no skeleton of its own"
    );
    assert_eq!(held(&path), opened_as);

    state
        .edit_gta6_entry_bone(doc, CODE, 1, moved)
        .expect("a finite position");
    let edited = held(&path);
    assert_eq!(edited.bones[1].translation, [0.25, 1.5, 0.0]);
    assert_eq!(edited.bones[1].inverse_bind[3], [-0.25, -1.5, 0.0]);
    assert_eq!(edited.bones[0], opened_as.bones[0]);
    state
        .edit_gta6_entry_bone(doc, CODE, 1, BoneEdit::Name("SKEL_Head_Renamed".into()))
        .expect("a name");
    assert_eq!(held(&path).bones[1].name, "SKEL_Head_Renamed");
    assert_eq!(held(&path).bones[1].hash, opened_as.bones[1].hash);

    state.active_document = Some(doc);
    assert!(state.undo().is_some());
    assert_eq!(held(&path), edited, "the rename is undone");
    assert!(state.undo().is_some());
    assert_eq!(held(&path), opened_as, "undone, it is as it opened");
    assert!(state.redo().is_some());
    assert_eq!(held(&path), edited, "redone, the move is back");
}

#[test]
fn a_clip_poses_a_dictionary_s_drawable_by_the_skeleton_of_the_entry_selected() {
    let (mut state, doc, path, _dir) = opened();
    let model = |state: &AppState| super::gta6_face::model_skeleton(state, doc, &path);
    assert!(model(&state).is_err(), "no entry is selected");
    state.selection.select(doc, dict_entry_id(CODE));
    assert_eq!(model(&state).ok(), Some(skeleton()));
    state.selection.select(doc, dict_entry_id(BARE));
    assert!(model(&state).is_err(), "that entry holds no skeleton");
}
