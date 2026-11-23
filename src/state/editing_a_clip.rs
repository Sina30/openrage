use super::*;

/// The active document's history.
fn history(state: &AppState) -> &EditHistory {
    state.focused_history().expect("a document is active")
}

fn state_with_clip() -> (AppState, DocumentId) {
    let mut state = AppState::new();
    state.documents.clear();
    let id: DocumentId = 1;
    state.documents.push(Document::new(
        id,
        Some(PathBuf::from("anim.ycd")),
        DocPayload::Resource(or_io::Asset::Clips(Default::default())),
    ));
    state.active_document = Some(id);
    state.preview_scene.set_clip(id, 0x1234);
    (state, id)
}

/// A dictionary of one clip, keyed `key`, that plays no animation over one second.
fn one_clip(key: u32, name: &str) -> or_io::gta5::CrClipDictionary {
    use or_io::gta5::{AtMap, AtMapEntry, AtString, CrClip, CrClipAnimation, CrClipAny, EClipType};
    let clip = CrClipAny::CrClipAnimation(CrClipAnimation {
        base: CrClip {
            r#type: or_io::decl::Open::new(EClipType::Animation),
            name: AtString::new(name),
            ..Default::default()
        },
        end_time: 1.0,
        rate: 1.0,
        ..Default::default()
    });
    or_io::gta5::CrClipDictionary {
        clips: AtMap {
            hash: vec![Some(Box::new(AtMapEntry {
                key,
                data: Some(Box::new(clip)),
                ..Default::default()
            }))],
            ..Default::default()
        },
        ..Default::default()
    }
}

fn edit_at(frame: u16, x: f32) -> (or_anim::EditKey, or_anim::EditValue) {
    (
        or_anim::EditKey {
            bone: 0,
            track: or_anim::EditTrack::Translation,
            frame,
        },
        or_anim::EditValue::Vector(glam::Vec3::new(x, 0.0, 0.0)),
    )
}

#[test]
fn undo_restores_the_previous_edit_layer() {
    let (mut state, _id) = state_with_clip();
    let (key, value) = edit_at(3, 5.0);

    let before = state.anim_edits.clone();
    state.anim_edits.set(key, value);
    state.push_anim_edit("move bone", before);

    assert_eq!(state.anim_edits.len(), 1);
    assert!(history(&state).can_undo());

    assert_eq!(state.undo().as_deref(), Some("move bone"));
    assert!(
        state.anim_edits.is_empty(),
        "undo did not restore the layer"
    );

    assert_eq!(state.redo().as_deref(), Some("move bone"));
    assert_eq!(state.anim_edits.len(), 1);
    assert_eq!(state.anim_edits.get(&key), Some(value));
}

#[test]
fn undo_walks_back_through_several_edits_in_order() {
    let (mut state, _id) = state_with_clip();
    for (index, frame) in [1u16, 2, 3].iter().enumerate() {
        let before = state.anim_edits.clone();
        let (key, value) = edit_at(*frame, index as f32);
        state.anim_edits.set(key, value);
        state.push_anim_edit(format!("edit {index}"), before);
    }
    assert_eq!(state.anim_edits.len(), 3);

    assert_eq!(state.undo().as_deref(), Some("edit 2"));
    assert_eq!(state.anim_edits.len(), 2);
    assert_eq!(state.undo().as_deref(), Some("edit 1"));
    assert_eq!(state.anim_edits.len(), 1);
    assert_eq!(state.undo().as_deref(), Some("edit 0"));
    assert!(state.anim_edits.is_empty());
    assert!(state.undo().is_none());
}

#[test]
fn an_edit_that_changes_nothing_is_not_recorded() {
    let (mut state, _id) = state_with_clip();
    let before = state.anim_edits.clone();
    state.push_anim_edit("nothing happened", before);
    assert!(!history(&state).can_undo());
}

#[test]
fn a_new_edit_clears_the_redo_branch() {
    let (mut state, _id) = state_with_clip();
    let before = state.anim_edits.clone();
    let (key, value) = edit_at(1, 1.0);
    state.anim_edits.set(key, value);
    state.push_anim_edit("first", before);

    state.undo();
    assert!(history(&state).can_redo());

    let before = state.anim_edits.clone();
    let (key, value) = edit_at(2, 2.0);
    state.anim_edits.set(key, value);
    state.push_anim_edit("second", before);

    assert!(!history(&state).can_redo());
    assert_eq!(history(&state).undo_label(), Some("second"));
}

#[test]
fn a_closed_document_takes_its_history_with_it() {
    let (mut state, id) = state_with_clip();
    let before = state.anim_edits.clone();
    let (key, value) = edit_at(1, 1.0);
    state.anim_edits.set(key, value);
    state.push_anim_edit("edit", before);

    state.documents.retain(|d| d.id != id);
    assert!(state.undo().is_none());
    assert!(state.focused_history().is_none());
}

#[test]
fn the_history_is_capped_and_discards_the_oldest() {
    let mut history = EditHistory::default();
    for index in 0..(EditHistory::CAP + 10) {
        history.push(
            format!("edit {index}"),
            Change::Pose(or_anim::PoseEdits::new()),
            None,
        );
    }
    assert_eq!(history.done().len(), EditHistory::CAP);
    assert_eq!(
        history.undo_label(),
        Some(format!("edit {}", EditHistory::CAP + 9).as_str())
    );
}

#[test]
fn clip_field_edits_undo_and_redo() {
    let mut state = AppState::new();
    state.documents.clear();

    let dictionary = one_clip(0x2000, "original");
    let id: DocumentId = 1;
    state.documents.push(Document::new(
        id,
        Some(PathBuf::from("anim.ycd")),
        DocPayload::Resource(or_io::Asset::Clips(dictionary)),
    ));
    state.active_document = Some(id);

    let before = state.clip_field_snapshot(id, 0x2000).expect("snapshot");
    assert_eq!(before.name, "original");

    let after = ClipFieldSnapshot {
        name: "renamed".to_string(),
        start_time: 0.25,
        end_time: 0.75,
        rate: 2.0,
        looped: true,
    };
    assert!(state.apply_clip_fields(id, 0x2000, &after));
    let change = Change::ClipFields {
        clip_key: 0x2000,
        other: before.clone(),
    };
    state.record(id, "clip fields", change);

    let now = state.clip_field_snapshot(id, 0x2000).expect("snapshot");
    assert_eq!(now, after);

    state.undo();
    assert_eq!(state.clip_field_snapshot(id, 0x2000).unwrap(), before);
    state.redo();
    assert_eq!(state.clip_field_snapshot(id, 0x2000).unwrap(), after);
}

#[test]
fn viewing_a_clip_whose_rate_is_past_its_fields_range_edits_nothing() {
    let mut dictionary = one_clip(0x2000, "paused");
    if let Some(or_io::gta5::CrClipAny::CrClipAnimation(clip)) =
        or_anim::clip::clip_mut(&mut dictionary, 0x2000)
    {
        clip.rate = 0.0;
    }
    let mut state = AppState::new();
    state.documents.clear();
    let id: DocumentId = 1;
    state.documents.push(Document::new(
        id,
        Some(PathBuf::from("anim.ycd")),
        DocPayload::Resource(or_io::Asset::Clips(dictionary)),
    ));
    state.active_document = Some(id);
    state.preview_scene.set_clip(id, 0x2000);
    let before = state.clip_field_snapshot(id, 0x2000).expect("snapshot");
    assert_eq!(
        before.rate, 0.0,
        "precondition: a rate the field's range excludes"
    );

    let ctx = egui::Context::default();
    for _ in 0..2 {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::panels::preview_settings::draw(ui, &mut state);
        });
    }
    assert_eq!(state.clip_field_snapshot(id, 0x2000), Some(before));
    assert!(!state.document(id).expect("open").dirty);
    assert!(
        !history(&state).can_undo(),
        "drawing recorded an edit to undo"
    );
}

#[test]
fn undoing_while_another_document_is_active_leaves_the_edited_one_as_it_is() {
    let mut state = AppState::new();
    state.documents.clear();

    let dictionary = one_clip(0x2000, "original");

    let edited: DocumentId = 1;
    let bystander: DocumentId = 2;
    state.documents.push(Document::new(
        edited,
        Some(PathBuf::from("edited.ycd")),
        DocPayload::Resource(or_io::Asset::Clips(dictionary)),
    ));
    state.documents.push(Document::new(
        bystander,
        Some(PathBuf::from("bystander.ycd")),
        DocPayload::Resource(or_io::Asset::Clips(Default::default())),
    ));

    state.active_document = Some(edited);
    let before = state.clip_field_snapshot(edited, 0x2000).expect("snapshot");
    let after = ClipFieldSnapshot {
        name: "renamed".to_string(),
        ..before.clone()
    };
    assert!(state.apply_clip_fields(edited, 0x2000, &after));
    let change = Change::ClipFields {
        clip_key: 0x2000,
        other: before.clone(),
    };
    state.record(edited, "rename clip", change);

    state.active_document = Some(bystander);
    assert!(
        !state.document(bystander).expect("bystander").dirty,
        "precondition: the bystander is clean"
    );

    assert_eq!(state.undo(), None, "the bystander has nothing to undo");

    assert_eq!(
        state.clip_field_snapshot(edited, 0x2000).unwrap(),
        after,
        "an undo in the active document reached another"
    );
    assert!(
        !state.document(bystander).expect("bystander").dirty,
        "the document the user was merely looking at must not be marked edited"
    );
    assert!(
        state.document(edited).expect("edited").history.can_undo(),
        "the edited document lost its step"
    );

    state.active_document = Some(edited);
    assert_eq!(state.undo().as_deref(), Some("rename clip"));
    assert_eq!(state.clip_field_snapshot(edited, 0x2000).unwrap(), before);
    assert!(
        !state.document(edited).expect("edited").dirty,
        "undone to where its file stands, it has nothing to save"
    );
}

#[test]
fn setting_the_loop_flag_preserves_other_flags() {
    use or_anim::clip::{flags, header, is_looped, set_looped};
    let mut clip = or_io::gta5::CrClipAny::Base(or_io::gta5::CrClip {
        flags: flags::IS_RESOURCE,
        ..Default::default()
    });
    set_looped(&mut clip, true);
    assert!(is_looped(&clip));
    assert_ne!(header(&clip).flags & flags::IS_RESOURCE, 0);
    set_looped(&mut clip, false);
    assert!(!is_looped(&clip));
    assert_ne!(header(&clip).flags & flags::IS_RESOURCE, 0);
}

#[test]
fn baking_with_no_edits_is_a_silent_no_op() {
    let (mut state, _id) = state_with_clip();
    assert_eq!(state.bake_anim_edits(), Ok(None));
}

#[test]
fn baking_without_a_skeleton_fails_and_keeps_the_edits() {
    let (mut state, _id) = state_with_clip();
    let (key, value) = edit_at(1, 1.0);
    state.anim_edits.set(key, value);

    assert!(state.bake_anim_edits().is_err());
    assert_eq!(
        state.anim_edits.len(),
        1,
        "edits were lost on a failed bake"
    );
}
