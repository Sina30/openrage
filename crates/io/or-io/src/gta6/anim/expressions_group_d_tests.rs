use crate::gta6::anim::{
    add_expression_track, lay_out_expressions, offered_facial_outputs, read_expressions,
    remove_expression_track, write_edited_expressions, AnimError, ExpressionGroup,
    ExpressionTracks, Expressions,
};
use crate::gta6::hash::name_hash32;

const D: ExpressionGroup = ExpressionGroup::FacialOutputs;

fn translation() -> u32 {
    name_hash32(b"facialTranslation")
}

fn group(tracks: &[(u32, u32)], slots: &[u16], components: &[u16]) -> ExpressionTracks {
    ExpressionTracks {
        tracks: tracks.to_vec(),
        slots: slots.to_vec(),
        components: components.to_vec(),
    }
}

/// A set reading facial controls A1 (1 component), A2 (2), A3 (1), a rotation A4 (3) and A5 (3),
/// writing `written` of them as group D, then driving one bone of three slots as group O.
fn set(written: &[(u32, u16)]) -> Expressions {
    let t = translation();
    let a = [
        (0xA1, t),
        (0xA2, t),
        (0xA3, t),
        (0xA4, name_hash32(b"facialRotation")),
        (0xA5, t),
    ];
    let d_tracks: Vec<(u32, u32)> = written.iter().map(|(id, _)| (*id, t)).collect();
    let d_components: Vec<u16> = written.iter().map(|(_, c)| *c).collect();
    let m: u16 = d_components.iter().sum();
    Expressions {
        key: 0x2468_ACE0,
        inputs: group(&a, &(0..10).collect::<Vec<u16>>(), &[1, 2, 1, 3, 3]),
        bones: group(&[], &[], &[]),
        bone_masks: Vec::new(),
        controls: group(&[], &[], &[]),
        control_slots: Vec::new(),
        control_words: Vec::new(),
        facial_outputs: group(&d_tracks, &(0..m).collect::<Vec<u16>>(), &d_components),
        outputs: Some(group(
            &[(0xE1, name_hash32(b"boneTranslation"))],
            &[m, m + 1, m + 2],
            &[0],
        )),
        output_masks: vec![0x87],
    }
}

fn laid_out_and_read(set: &Expressions) -> Vec<u8> {
    let bytes = lay_out_expressions(set, 0).expect("laid out");
    let back = read_expressions(&bytes).expect("reads");
    assert_eq!(&back, set);
    assert_eq!(lay_out_expressions(&back, 0).expect("again"), bytes);
    bytes
}

#[test]
fn a_facial_control_of_1_or_2_components_added_to_group_d_moves_group_o_up_by_them() {
    let set = set(&[(0xA3, 1)]);
    laid_out_and_read(&set);
    assert_eq!(offered_facial_outputs(&set), [0xA1, 0xA2]);
    for (id, c) in [(0xA1, 1u16), (0xA2, 2)] {
        let added = add_expression_track(&set, D, 0, id).expect("added");
        let d = &added.facial_outputs;
        // A's order: both come before A3.
        assert_eq!(d.tracks, [(id, translation()), (0xA3, translation())]);
        assert_eq!(d.components, [c, 1]);
        assert_eq!(d.slots, (0..c + 1).collect::<Vec<u16>>());
        let outputs = added.outputs.as_ref().expect("outputs");
        let moved: Vec<u16> = (set.outputs.as_ref().expect("outputs").slots.iter())
            .map(|s| s + c)
            .collect();
        assert_eq!(outputs.slots, moved);
        assert_eq!(outputs.components, [0]);
        assert_eq!(added.inputs, set.inputs);
        laid_out_and_read(&added);
        assert_eq!(remove_expression_track(&added, D, 0).expect("removed"), set);
    }
}

#[test]
fn the_only_track_of_group_d_removed_leaves_slots_13_to_16_zero_and_o_from_slot_0() {
    let set = set(&[(0xA2, 2)]);
    let removed = remove_expression_track(&set, D, 0).expect("removed");
    assert!(removed.facial_outputs.tracks.is_empty());
    assert!(removed.facial_outputs.slots.is_empty());
    assert_eq!(removed.outputs.as_ref().expect("outputs").slots, [0, 1, 2]);
    let bytes = laid_out_and_read(&removed);
    let slot = |k: usize| 0x18 + 16 * k;
    assert!(bytes[slot(13)..slot(17)].iter().all(|b| *b == 0));
}

#[test]
fn a_track_of_group_d_added_and_removed_is_written_anew_and_back_to_the_stored_bytes() {
    let set = set(&[(0xA1, 1), (0xA3, 1)]);
    let stored = lay_out_expressions(&set, 0).expect("laid out");
    let added = add_expression_track(&set, D, 1, 0xA2).expect("added");
    assert_eq!(added.facial_outputs.tracks[1], (0xA2, translation()));
    let mut bytes = stored.clone();
    write_edited_expressions(&mut bytes, 0, &added).expect("written");
    assert_eq!(bytes, lay_out_expressions(&added, 0).expect("laid out"));
    let removed = remove_expression_track(&added, D, 1).expect("removed");
    write_edited_expressions(&mut bytes, 0, &removed).expect("written");
    assert_eq!(bytes, stored);
}

#[test]
fn a_track_of_group_d_outside_what_is_known_is_refused() {
    let one = set(&[(0xA3, 1)]);
    // Held already, read by no track of A, of 3 components, or picked from no track.
    let refused = [
        add_expression_track(&one, D, 0, 0xA3),
        add_expression_track(&one, D, 0, 0xD9),
        add_expression_track(&one, D, 0, 0xA5),
        add_expression_track(&one, D, 1, 0xA1),
    ];
    for (i, r) in refused.into_iter().enumerate() {
        assert!(matches!(r, Err(AnimError::Unwritten(_))), "edit {i}: {r:?}");
    }
    // A group D out of A's order has no known place for another track.
    let unordered = set(&[(0xA3, 1), (0xA1, 1)]);
    let err = add_expression_track(&unordered, D, 0, 0xA2).expect_err("refused");
    assert!(err.to_string().contains("order it reads them"), "{err}");
    // A track of D that A does not read is not removed.
    let mut foreign = one.clone();
    foreign.facial_outputs.tracks[0].0 = 0xD9;
    let err = remove_expression_track(&foreign, D, 0).expect_err("refused");
    assert!(err.to_string().contains("does not read"), "{err}");
    let none = set(&[]);
    assert!(remove_expression_track(&none, D, 0).is_err());
}
