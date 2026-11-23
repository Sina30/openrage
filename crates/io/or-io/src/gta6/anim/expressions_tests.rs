use super::*;
use crate::gta6::hash::name_hash32;

const KEY: u32 = 0x1357_9BDF;
/// Where a set's arrays start: after its 0x1C0-byte root.
const ARRAYS: usize = 0x1C0;

/// A set's bytes and where each array of it lies.
struct Set {
    bytes: Vec<u8>,
    at: std::collections::HashMap<usize, usize>,
    next: usize,
}

impl Set {
    fn new() -> Self {
        let mut set = Set {
            bytes: vec![0; 0x400],
            at: Default::default(),
            next: ARRAYS,
        };
        set.ptr(8, 0x3F0);
        set.put(0x10, &KEY.to_le_bytes());
        set.put(0x14, &KEY.to_le_bytes());
        set
    }
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.bytes[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn ptr(&mut self, at: usize, to: usize) {
        self.put(at, &(0x5000_0000 + to as u64).to_le_bytes());
    }
    /// The slot at `at` of `n` elements whose bytes are `bytes`, placed after the arrays before.
    fn slot(&mut self, at: usize, n: usize, bytes: &[u8]) {
        self.put(at, &(n as u32).to_le_bytes());
        self.put(at + 4, &(n as u32).to_le_bytes());
        let to = self.next;
        self.ptr(at + 8, to);
        self.put(to, bytes);
        self.at.insert(at, to);
        self.next = (to + bytes.len()).next_multiple_of(8);
    }
    fn tracks(&mut self, at: usize, tracks: &[(u32, u32)]) {
        let bytes: Vec<u8> = (tracks.iter())
            .flat_map(|(id, track)| [id.to_le_bytes(), track.to_le_bytes()].concat())
            .collect();
        self.slot(at, tracks.len(), &bytes);
    }
    fn halves(&mut self, at: usize, halves: &[u16]) {
        let bytes: Vec<u8> = halves.iter().flat_map(|h| h.to_le_bytes()).collect();
        self.slot(at, halves.len(), &bytes);
    }
    fn counts(&mut self, at: usize, counts: [u32; 3]) {
        for (i, n) in counts.into_iter().enumerate() {
            self.put(at + 4 + 4 * i, &n.to_le_bytes());
        }
    }
    /// The element `index` of the slot at `slot` set to `byte`.
    fn poke(&mut self, slot: usize, index: usize, byte: u8) {
        let at = self.at[&slot] + index;
        self.bytes[at] = byte;
    }
}

fn facial() -> u32 {
    name_hash32(b"facialTranslation")
}

/// A set reading two facial controls of 1 and 2 components and one tag control and, with
/// `bones`, reading a bone's rotation and translation, writing a facial control of 2 components
/// and driving a bone's rotation in x, y and z and its translation in x.
fn set(bones: bool) -> Set {
    let mut s = Set::new();
    s.tracks(0x18, &[(0xA1, facial()), (0xA2, facial())]);
    s.halves(0x28, &[0, 1, 2]);
    s.slot(0x38, 2, &[1, 2]);
    s.counts(0x48, [1, 1, 0]);
    s.tracks(0x98, &[(0xC1, CONTROL_TRACK)]);
    s.halves(0xA8, &[3]);
    s.slot(0xB8, 1, &[5]);
    s.halves(0xC8, &[4]);
    s.slot(0xD8, 1, &0xC0DE_u32.to_le_bytes());
    if !bones {
        return s;
    }
    let (rotation, translation) = (
        name_hash32(b"boneRotation"),
        name_hash32(b"boneTranslation"),
    );
    s.tracks(0x58, &[(0xB0, rotation), (0xB0, translation)]);
    s.halves(0x68, &[5, 6, 7, 8, 9, 10]);
    s.halves(0x78, &[0, 3]);
    s.slot(0x88, 2, &[7, 0x87]);
    s.tracks(0xE8, &[(0xD1, facial())]);
    s.halves(0xF8, &[0, 1]);
    s.slot(0x108, 1, &[2]);
    s.counts(0x118, [0, 1, 0]);
    s.tracks(0x128, &[(0xE1, rotation), (0xE1, translation)]);
    s.halves(0x138, &[2, 3, 4, 5]);
    s.halves(0x148, &[0, 3]);
    s.slot(0x158, 2, &[0x87, 0x81]);
    s
}

#[test]
fn an_expression_set_reads_every_group_it_reads_and_writes() {
    let read = read_expressions(&set(true).bytes).expect("reads");
    assert_eq!(read.key, KEY);
    assert_eq!(read.inputs.tracks, [(0xA1, facial()), (0xA2, facial())]);
    assert_eq!(read.inputs.slots, [0, 1, 2]);
    assert_eq!(read.inputs.components, [1, 2]);
    assert_eq!(
        read.bones.tracks[1],
        (0xB0, name_hash32(b"boneTranslation"))
    );
    assert_eq!(read.bones.components, [0, 3]);
    assert_eq!(read.bone_masks, [7, 0x87]);
    assert_eq!(read.controls.tracks, [(0xC1, CONTROL_TRACK)]);
    assert_eq!(read.controls.components, [5]);
    assert_eq!(read.control_slots, [4]);
    assert_eq!(read.control_words, [0xC0DE]);
    assert_eq!(read.facial_outputs.slots, [0, 1]);
    let outputs = read.outputs.expect("outputs");
    assert_eq!(outputs.tracks[0], (0xE1, name_hash32(b"boneRotation")));
    assert_eq!(outputs.components, [0, 3]);
    assert_eq!(read.output_masks, [0x87, 0x81]);
    let alone = read_expressions(&set(false).bytes).expect("reads");
    assert_eq!((alone.outputs, alone.bones.tracks.len()), (None, 0));
}

#[test]
fn an_expression_set_whose_groups_break_their_rules_is_refused() {
    let mut halved = set(false);
    halved.bytes[0x14] ^= 1;
    let mut component = set(false);
    component.poke(0x38, 1, 4);
    let mut counted = set(false);
    counted.counts(0x48, [2, 0, 0]);
    let mut control = set(false);
    control.poke(0x98, 4, 0);
    let mut start = set(true);
    start.poke(0x78, 2, 4);
    let mut bone_mask = set(true);
    bone_mask.poke(0x88, 0, 6);
    let mut facial_slot = set(true);
    facial_slot.poke(0xF8, 0, 1);
    let mut output_mask = set(true);
    output_mask.poke(0x158, 1, 0x01);
    let mut output_start = set(true);
    output_start.poke(0x148, 2, 2);
    let mut output_slot = set(true);
    output_slot.poke(0x138, 0, 0);
    let mut unpaired = set(true);
    unpaired.bytes[0x128 + 4] = 7;
    // Bone inputs with no bone outputs.
    let mut undriven = set(true);
    undriven.bytes[0x128..0x138].fill(0);
    let broken = [
        halved,
        component,
        counted,
        control,
        start,
        bone_mask,
        facial_slot,
        output_mask,
        output_start,
        output_slot,
        unpaired,
        undriven,
    ];
    for (i, b) in broken.iter().enumerate() {
        assert!(
            matches!(read_expressions(&b.bytes), Err(AnimError::Expressions(_))),
            "set {i} reads"
        );
    }
}

#[test]
fn an_edited_track_id_is_written_where_it_lies_and_reads_back() {
    let built = set(true);
    let mut bytes = built.bytes.clone();
    // Nothing edited writes nothing.
    let stored = read_expressions(&bytes).expect("reads");
    write_expression_ids(&mut bytes, &stored).expect("written");
    assert_eq!(bytes, built.bytes);

    let mut edited = stored.clone();
    edited.inputs.tracks[1].0 = 0x0BAD_F00D;
    edited.outputs.as_mut().expect("outputs").tracks[0].0 = 0x1234_5678;
    write_expression_ids(&mut bytes, &edited).expect("written");
    assert_eq!(read_expressions(&bytes).expect("reads again"), edited);
    // Only the two ids moved: the second input's at 0x08 of its array, the first output's at 0.
    let mut expected = built.bytes.clone();
    let input = built.at[&0x18] + TRACK_LEN;
    expected[input..input + 4].copy_from_slice(&0x0BAD_F00D_u32.to_le_bytes());
    let output = built.at[&0x128];
    expected[output..output + 4].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
    assert_eq!(bytes, expected);
}

#[test]
fn an_edit_that_is_not_an_id_or_states_a_pair_twice_is_refused_and_writes_nothing() {
    let built = set(true);
    let stored = read_expressions(&built.bytes).expect("reads");
    let mut slot = stored.clone();
    slot.inputs.slots[0] = 9;
    let mut word = stored.clone();
    word.inputs.tracks[0].1 = 0x0F0F_0F0F;
    let mut twice = stored.clone();
    twice.inputs.tracks[1].0 = twice.inputs.tracks[0].0;
    let mut undriven = stored.clone();
    undriven.outputs = None;
    for (i, edit) in [slot, word, twice, undriven].iter().enumerate() {
        let mut bytes = built.bytes.clone();
        assert!(
            matches!(
                write_expression_ids(&mut bytes, edit),
                Err(AnimError::Unwritten(_))
            ),
            "edit {i} is written"
        );
        assert_eq!(bytes, built.bytes, "edit {i} changed the section");
    }
}

#[test]
fn each_group_names_the_slots_and_mask_a_track_states() {
    let read = read_expressions(&set(true).bytes).expect("reads");
    let slots = |group: ExpressionGroup, track: usize| group.slots(&read, track);
    // Facial tracks take their components' slots in turn.
    assert_eq!(slots(ExpressionGroup::Inputs, 0), Some(&[0u16][..]));
    assert_eq!(slots(ExpressionGroup::Inputs, 1), Some(&[1u16, 2][..]));
    // A bone input takes three slots from its start; group C one each.
    assert_eq!(slots(ExpressionGroup::Bones, 1), Some(&[8u16, 9, 10][..]));
    assert_eq!(slots(ExpressionGroup::Controls, 0), Some(&[3u16][..]));
    assert_eq!(
        slots(ExpressionGroup::FacialOutputs, 0),
        Some(&[0u16, 1][..])
    );
    // A driven bone takes as many as its mask has x, y and z bits.
    assert_eq!(slots(ExpressionGroup::Outputs, 0), Some(&[2u16, 3, 4][..]));
    assert_eq!(slots(ExpressionGroup::Outputs, 1), Some(&[5u16][..]));
    assert_eq!(ExpressionGroup::Outputs.mask(&read, 1), Some(0x81));
    assert_eq!(ExpressionGroup::Bones.mask(&read, 0), Some(7));
    assert_eq!(ExpressionGroup::Inputs.mask(&read, 0), None);
    assert_eq!(slots(ExpressionGroup::Inputs, 2), None);
    let alone = read_expressions(&set(false).bytes).expect("reads");
    assert!(ExpressionGroup::Outputs.of(&alone).is_none());
    let reading: Vec<_> = (ExpressionGroup::ALL.into_iter())
        .filter(|g| g.reads())
        .collect();
    assert_eq!(
        reading,
        [
            ExpressionGroup::Inputs,
            ExpressionGroup::Bones,
            ExpressionGroup::Controls
        ]
    );
}
