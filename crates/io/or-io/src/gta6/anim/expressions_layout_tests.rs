use super::*;
use crate::gta6::anim::ExpressionTracks;
use crate::gta6::hash::name_hash32;

const KEY: u32 = 0x1357_9BDF;
/// The word every group-C track states.
const CONTROL: u32 = 0x8332_88FB;

fn facial() -> u32 {
    name_hash32(b"facialTranslation")
}

fn group(tracks: &[(u32, u32)], slots: &[u16], components: &[u16]) -> ExpressionTracks {
    ExpressionTracks {
        tracks: tracks.to_vec(),
        slots: slots.to_vec(),
        components: components.to_vec(),
    }
}

/// Two facial controls of 1 and 2 components and a control of the unnamed word, read; with
/// `bones`, a bone read and a facial control and a bone written as well.
fn set(bones: bool) -> Expressions {
    let none = || group(&[], &[], &[]);
    let (rotation, translation) = (
        name_hash32(b"boneRotation"),
        name_hash32(b"boneTranslation"),
    );
    let mut set = Expressions {
        key: KEY,
        inputs: group(&[(0xA1, facial()), (0xA2, facial())], &[0, 1, 2], &[1, 2]),
        bones: none(),
        bone_masks: Vec::new(),
        controls: group(&[(0xC1, CONTROL)], &[3], &[5]),
        control_slots: vec![4],
        control_words: vec![0xC0DE],
        facial_outputs: none(),
        outputs: None,
        output_masks: Vec::new(),
    };
    if bones {
        set.bones = group(
            &[(0xB0, rotation), (0xB0, translation)],
            &[5, 6, 7, 8, 9, 10],
            &[0, 3],
        );
        set.bone_masks = vec![7, 0x87];
        set.facial_outputs = group(&[(0xD1, facial())], &[0, 1], &[2]);
        set.outputs = Some(group(
            &[(0xE1, rotation), (0xE1, translation)],
            &[2, 3, 4, 5],
            &[0, 3],
        ));
        set.output_masks = vec![0x87, 0x81];
    }
    set
}

fn word(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

#[test]
fn a_set_laid_out_reads_back_as_it_was_and_lays_out_again_to_the_same_bytes() {
    for bones in [false, true] {
        let set = set(bones);
        let bytes = lay_out_expressions(&set, 0).expect("laid out");
        assert_eq!(read_expressions(&bytes).expect("reads"), set);
        assert_eq!(
            lay_out_expressions(&read_expressions(&bytes).expect("reads"), 0).expect("again"),
            bytes
        );
        assert!(bytes.len().is_multiple_of(16));
        assert!(bytes[0x168..ROOT_LEN].iter().all(|b| *b == 0));
    }
}

#[test]
fn the_layout_goes_by_length_then_alignment_then_slot_and_slot_12_follows_11() {
    let bytes = lay_out_expressions(&set(false), 0).expect("laid out");
    let mut end = ROOT_LEN;
    let mut next = |len: usize, align: usize| {
        let at = end.next_multiple_of(align);
        end = at + len;
        VIRTUAL_BASE + at as u64
    };
    // The map of one block, 24 bytes; A's tracks, 16; C's track, 8; A's three slots, 6; C's
    // slot and slot 11's, 2 bytes on 2, then slot 12's word; A's two components; C's byte.
    let expected = [
        (None, next(24, 16)),
        (Some(0), next(16, 8)),
        (Some(8), next(8, 8)),
        (Some(1), next(6, 2)),
        (Some(9), next(2, 2)),
        (Some(11), next(2, 2)),
        (Some(12), next(4, 4)),
        (Some(2), next(2, 1)),
        (Some(10), next(1, 1)),
    ];
    for (slot, pointer) in expected {
        let at = slot.map_or(8, |s| slot_at(s) + 8);
        assert_eq!(word(&bytes, at), pointer, "slot {slot:?}");
    }
    assert_eq!(bytes.len(), end.next_multiple_of(16));
    let map = (word(&bytes, 8) - VIRTUAL_BASE) as usize;
    assert_eq!((bytes[map + 8], bytes[map + 9]), (1, 0));
}

#[test]
fn a_facial_control_added_takes_the_slots_after_its_group_s_and_moves_the_others_up() {
    let set = set(false);
    let added = add_expression_track(&set, ExpressionGroup::Inputs, 1, 0xA3).expect("added");
    assert_eq!(
        added.inputs.tracks,
        [(0xA1, facial()), (0xA2, facial()), (0xA3, facial())]
    );
    assert_eq!(added.inputs.slots, [0, 1, 2, 3, 4]);
    assert_eq!(added.inputs.components, [1, 2, 2]);
    assert_eq!(added.controls.slots, [5]);
    assert_eq!(added.control_slots, [6]);
    let bytes = lay_out_expressions(&added, 0).expect("laid out");
    assert_eq!(read_expressions(&bytes).expect("reads"), added);

    let removed = remove_expression_track(&added, ExpressionGroup::Inputs, 0).expect("removed");
    assert_eq!(removed.inputs.slots, [0, 1, 2, 3]);
    assert_eq!(removed.inputs.components, [2, 2]);
    assert_eq!(removed.controls.slots, [4]);
    assert_eq!(removed.control_slots, [5]);
    let back = remove_expression_track(&added, ExpressionGroup::Inputs, 2).expect("removed");
    assert_eq!(back, set);
}

#[test]
fn a_control_added_or_removed_keeps_every_input_slot_once() {
    let set = set(true);
    let added = add_expression_track(&set, ExpressionGroup::Controls, 0, 0xC2).expect("added");
    assert_eq!(added.controls.tracks, [(0xC1, CONTROL), (0xC2, CONTROL)]);
    assert_eq!(added.controls.slots, [3, 4]);
    assert_eq!(added.controls.components, [5, 5]);
    assert_eq!(added.control_slots, [5]);
    assert_eq!(added.bones.slots, [6, 7, 8, 9, 10, 11]);
    let mut every: Vec<u16> = [
        &added.inputs.slots,
        &added.bones.slots,
        &added.controls.slots,
        &added.control_slots,
    ]
    .into_iter()
    .flatten()
    .copied()
    .collect();
    every.sort_unstable();
    assert_eq!(every, (0..12).collect::<Vec<u16>>());
    assert_eq!(
        remove_expression_track(&added, ExpressionGroup::Controls, 1).expect("removed"),
        set
    );
}

#[test]
fn a_driven_bone_added_or_removed_takes_its_mask_s_slots_after_the_others() {
    let set = set(true);
    let added = add_expression_track(&set, ExpressionGroup::Outputs, 0, 0xE2).expect("added");
    let outputs = added.outputs.as_ref().expect("outputs");
    assert_eq!(outputs.components, [0, 3, 4]);
    assert_eq!(outputs.slots, [2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(added.output_masks, [0x87, 0x81, 0x87]);
    let bytes = lay_out_expressions(&added, 0).expect("laid out");
    assert_eq!(read_expressions(&bytes).expect("reads"), added);

    let removed = remove_expression_track(&set, ExpressionGroup::Outputs, 0).expect("removed");
    let outputs = removed.outputs.as_ref().expect("outputs");
    assert_eq!(outputs.tracks, [(0xE1, name_hash32(b"boneTranslation"))]);
    assert_eq!(outputs.components, [0]);
    assert_eq!(outputs.slots, [2]);
    assert_eq!(removed.output_masks, [0x81]);
    // A set that reads bones drives one at least.
    let err = remove_expression_track(&removed, ExpressionGroup::Outputs, 0).expect_err("refused");
    assert!(err.to_string().contains("drives one bone"), "{err}");
}

#[test]
fn a_track_of_groups_b_and_d_a_twice_stated_id_and_the_last_facial_control_are_refused() {
    let set = set(true);
    let refused = [
        add_expression_track(&set, ExpressionGroup::Bones, 0, 0xB1),
        remove_expression_track(&set, ExpressionGroup::FacialOutputs, 0),
        add_expression_track(&set, ExpressionGroup::Inputs, 0, 0xA2),
        add_expression_track(&set, ExpressionGroup::Inputs, 5, 0xA9),
    ];
    for (i, r) in refused.into_iter().enumerate() {
        assert!(matches!(r, Err(AnimError::Unwritten(_))), "edit {i}: {r:?}");
    }
    let alone = remove_expression_track(&set, ExpressionGroup::Inputs, 0).expect("removed");
    let err = remove_expression_track(&alone, ExpressionGroup::Inputs, 0).expect_err("refused");
    assert!(err.to_string().contains("one facial control"), "{err}");
}

#[test]
fn an_edited_set_is_written_in_place_for_an_id_and_laid_out_anew_for_a_track() {
    let set = set(true);
    let stored = lay_out_expressions(&set, 0).expect("laid out");

    let mut renamed = set.clone();
    renamed.inputs.tracks[0].0 = 0x0BAD_F00D;
    let mut bytes = stored.clone();
    write_edited_expressions(&mut bytes, 0, &renamed).expect("written");
    assert_eq!(bytes.len(), stored.len());
    assert_eq!(read_expressions(&bytes).expect("reads"), renamed);

    let added = add_expression_track(&set, ExpressionGroup::Inputs, 0, 0xA3).expect("added");
    let mut bytes = stored.clone();
    write_edited_expressions(&mut bytes, 0, &added).expect("written");
    assert_eq!(bytes, lay_out_expressions(&added, 0).expect("laid out"));

    // A stored set whose arrays lie elsewhere is not laid out anew, and keeps its bytes.
    let mut elsewhere = stored.clone();
    elsewhere.extend([0u8; 16]);
    let kept = elsewhere.clone();
    let err = write_edited_expressions(&mut elsewhere, 0, &added).expect_err("refused");
    assert!(matches!(err, AnimError::Unwritten(_)), "{err}");
    assert_eq!(elsewhere, kept);
}
