use super::*;

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put16(out: &mut [u8], at: usize, v: u16) {
    out[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// Writes at `field` the offset from `field` to `to`.
fn point(out: &mut [u8], field: usize, to: usize) {
    put32(out, field, (to as i64 - field as i64) as i32 as u32);
}

/// The root's offset in a head with every list empty.
const ROOT: usize = 0x3A;
const MACHINE_ID: u32 = 0x1111_A0A1;
const STATE_ID: u32 = 0x3333_B0B1;
const FILTER_ID: u32 = 0x7777_D0D1;
const TAIL_ID: u32 = 0x8888_E0E1;
const SIGNAL: [u32; 4] = [0x5555_0C01, 3, 0x5555_0C02, 0];
const EVENT: [u32; 2] = [0x6666_0E01, 1];
const KEYS: [[f32; 4]; 2] = [[1.0, 0.0, 0.5, -0.5], [2.0, 1.0, 0.25, 0.75]];

/// Where each record of [`network`] starts.
struct At {
    state: usize,
    transition: usize,
    binding: usize,
    operation: usize,
    filter: usize,
    tail: usize,
    loose_tail: usize,
}

/// A machine with one state, whose transition leads back to it and whose one binding holds a
/// signal, an event and a curve; the state's output is a filter over a tail, then a loose tail.
fn network() -> (Vec<u8>, At) {
    let mut b = b"MoVE\x13".to_vec();
    b.extend_from_slice(&[0; 15]);
    le32(&mut b, 0x4E37_A1C5);
    // The length word, set last, and the zero word.
    le32(&mut b, 0);
    le32(&mut b, 0);
    // Lists A and B, the word after B, then list D: all empty.
    for v in [0, 0, 2, 0] {
        le32(&mut b, v);
    }
    b.extend_from_slice(&[0; 10]);
    assert_eq!(b.len(), ROOT);

    let m = ROOT;
    b.resize(m + 0x70, 0);
    put16(&mut b, m, 1);
    put32(&mut b, m + 4, MACHINE_ID);
    b[m + 0x12] = 1;
    put32(&mut b, m + 0x14, 0x2222_0001);
    put32(&mut b, m + 0x44, 1.0f32.to_bits());
    put32(&mut b, m + 0x50, 1);
    point(&mut b, m + 0x54, m + 0x60);
    put32(&mut b, m + 0x58, STATE_ID);
    put16(&mut b, m + 0x60, 16);
    put16(&mut b, m + 0x62, 1);
    put32(&mut b, m + 0x68, 3);

    let s = b.len();
    b.resize(s + 0x30, 0);
    put16(&mut b, s, 22);
    put32(&mut b, s + 4, STATE_ID);
    b[s + 0x12] = 1;
    b[s + 0x13] = 1;
    point(&mut b, s + 0x24, s + 0x30);
    put32(&mut b, s + 0x2C, 1);

    let t = b.len();
    b.resize(t + 0x4C, 0);
    put32(&mut b, t + 0x0C, 0x4444_C0C1);
    put32(&mut b, t + 0x10, 0x4C);
    put32(&mut b, t + 0x14, 0xADD2_3FFF);
    put32(&mut b, t + 0x1C, 0.25f32.to_bits());
    point(&mut b, t + 0x48, s);

    let k = b.len();
    let operation_size = 8 + 8 + 16 + 16 * KEYS.len() + 8;
    le32(&mut b, (12 + 16 + 8 + operation_size) as u32);
    b.extend_from_slice(&[0, 0, 0, 1, 0, 1, 1, 0]);
    SIGNAL.iter().for_each(|v| le32(&mut b, *v));
    EVENT.iter().for_each(|v| le32(&mut b, *v));
    let op = b.len();
    for v in [7u16, 2, operation_size as u16, 9] {
        le16(&mut b, v);
    }
    le32(&mut b, Operator::FCURVE);
    le32(&mut b, 4);
    for v in [-1.0f32, 1.0] {
        le32(&mut b, v.to_bits());
    }
    le32(&mut b, KEYS.len() as u32);
    le32(&mut b, 4);
    for key in KEYS {
        le32(&mut b, key[0] as u32);
        key[1..].iter().for_each(|v| le32(&mut b, v.to_bits()));
    }
    le32(&mut b, Operator::END);
    le32(&mut b, 0);
    assert_eq!(b.len(), op + operation_size);

    let f = b.len();
    le16(&mut b, 6);
    le16(&mut b, 0);
    le32(&mut b, FILTER_ID);
    le32(&mut b, 0);
    le32(&mut b, 0);
    let tail = b.len();
    for v in [2, 0] {
        le16(&mut b, v);
    }
    le32(&mut b, TAIL_ID);
    le32(&mut b, 0);
    let loose_tail = b.len();
    for v in [2, 0] {
        le16(&mut b, v);
    }
    le32(&mut b, 0x9999_F0F1);
    le32(&mut b, 1);

    point(&mut b, m + 8, s);
    point(&mut b, m + 0x5C, s);
    point(&mut b, m + 0x64, s);
    point(&mut b, s + 8, f);
    point(&mut b, s + 0x28, k);
    point(&mut b, f + 0x0C, tail);
    let length = (b.len() - 0x2E) as u32;
    put32(&mut b, 0x18, length);
    let at = At {
        state: s,
        transition: t,
        binding: k,
        operation: op,
        filter: f,
        tail,
        loose_tail,
    };
    (b, at)
}

#[test]
fn a_network_is_walked_from_its_root_and_tiles_its_body() {
    let (bytes, at) = network();
    let outline = MoveOutline::open(&bytes).expect("opens");
    assert_eq!(outline.root, ROOT);
    assert!(outline.stops.is_empty());
    assert_eq!(outline.loose_tails, 1);
    let kinds: Vec<(usize, u16, u32)> = (outline.nodes.iter())
        .map(|n| (n.at, n.kind, n.id))
        .collect();
    assert_eq!(
        kinds,
        [
            (ROOT, 1, MACHINE_ID),
            (at.state, 22, STATE_ID),
            (at.filter, 6, FILTER_ID),
            (at.tail, 2, TAIL_ID)
        ]
    );
    assert_eq!(outline.node(ROOT).map(|n| n.size), Some(at.state - ROOT));
    assert_eq!(
        outline.node(at.state).map(|n| n.size),
        Some(at.filter - at.state)
    );
    assert_eq!(
        outline.node(at.tail).map(|n| n.at + n.size),
        Some(at.loose_tail)
    );

    let Some(MoveBody::Machine(machine)) = outline.node(ROOT).map(|n| &n.body) else {
        panic!("the root is a machine");
    };
    assert_eq!(machine.initial, Some(at.state));
    assert_eq!(machine.states, [(STATE_ID, at.state)]);
    assert!(machine.inputs.is_empty() && machine.transitions.is_empty());
    let on_flag = MoveCondition {
        kind: 3,
        flag: 0,
        words: vec![0],
    };
    let entry = MachineEntry {
        size: 16,
        flag: 0,
        state: at.state,
        conditions: vec![on_flag],
    };
    assert_eq!(machine.entries, [entry]);

    let Some(MoveBody::State(state)) = outline.node(at.state).map(|n| &n.body) else {
        panic!("a state");
    };
    assert_eq!((state.output, state.children), (at.filter, 1));
    // The words `network` writes: +0x0C, the size at +0x10, +0x14, +0x1C and the target at +0x48.
    let mut words = [0; TRANSITION_WORDS];
    words[3] = 0x4444_C0C1;
    words[4] = 0x4C;
    words[5] = 0xADD2_3FFF;
    words[7] = 0.25f32.to_bits();
    words[18] = (at.state as i64 - (at.transition + 0x48) as i64) as i32 as u32;
    let transition = MoveTransition {
        at: at.transition,
        size: 0x4C,
        target: at.state,
        words,
        conditions: Vec::new(),
    };
    assert_eq!(state.transitions, [transition]);
    let binding = &state.bindings[0];
    assert_eq!(binding.child, 0);
    assert!(binding.inputs_a.is_empty() && binding.outputs.is_empty());
    assert_eq!(binding.inputs_b, [SIGNAL]);
    assert_eq!(binding.events, [EVENT]);
    let operation = &binding.operations[0];
    assert_eq!(
        (operation.node_index, operation.type_index, operation.index),
        (7, 2, 9)
    );
    let ends: Vec<u32> = operation
        .operators
        .iter()
        .map(|o| o.operator_type)
        .collect();
    assert_eq!(ends, [Operator::FCURVE, Operator::END]);
    let curve = operation.operators[0].fcurve.as_ref().expect("a curve");
    assert_eq!((curve.minimum, curve.maximum), (-1.0, 1.0));
    let keys: Vec<[f32; 4]> = (curve.keyframes.iter())
        .map(|k| {
            [
                k.keyframe_type as f32,
                k.key,
                k.constants[0],
                k.constants[1],
            ]
        })
        .collect();
    assert_eq!(keys, KEYS);

    let Some(MoveBody::Leaf { flags, inputs }) = outline.node(at.filter).map(|n| &n.body) else {
        panic!("a leaf");
    };
    assert_eq!((*flags, &inputs[..]), (0, &[at.tail][..]));
}

#[test]
fn a_keyed_node_stops_the_walk_by_name_and_the_rest_is_kept() {
    let (mut bytes, at) = network();
    put16(&mut bytes, at.tail, 28);
    put32(&mut bytes, at.tail + 8, 0x0A);
    let outline = MoveOutline::open(&bytes).expect("opens up to the keyed node");
    let stop = MoveStop {
        at: at.tail,
        kind: 28,
        index: 0,
        id: TAIL_ID,
        word: 0x0A,
        reason: StopReason::Keyed,
    };
    assert_eq!(outline.stops, [stop]);
    assert_eq!(outline.stop(at.tail), Some(&stop));
    assert_eq!(outline.nodes.len(), 3);
    assert!(outline.node(at.tail).is_none());
    assert_eq!(outline.head.save().expect("saves"), bytes);
}

#[test]
fn a_network_that_breaks_a_rule_is_refused_by_name() {
    let refused = |bytes: &[u8], why: &str| {
        let error = MoveOutline::open(bytes).unwrap_err().to_string();
        assert!(
            error.contains("GTA VI MoVE network") && error.contains(why),
            "{why}: {error}"
        );
    };
    let (bytes, at) = network();

    let mut target = bytes.clone();
    point(&mut target, at.transition + 0x48, at.filter);
    refused(&target, "lands on no state");

    let mut child = bytes.clone();
    child[at.binding + 4] = 1;
    refused(&child, "binds child 1");

    let mut size = bytes.clone();
    let stated = u16::from_le_bytes([size[at.operation + 4], size[at.operation + 5]]);
    put16(&mut size, at.operation + 4, stated - 8);
    refused(&size, "the operation at");

    let mut index = bytes.clone();
    put16(&mut index, at.filter + 2, 1);
    refused(&index, "past its 1 children");

    let mut gap = bytes.clone();
    put16(&mut gap, at.loose_tail, 16);
    refused(&gap, "a gap or overlap");

    let mut selector = bytes.clone();
    put32(&mut selector, at.filter + 8, 3);
    refused(&selector, "selector 3");

    let mut away = bytes.clone();
    put32(&mut away, at.filter + 0x0C, 0x7FFF_0000);
    refused(&away, "leaves the body");

    let mut unknown = bytes.clone();
    put16(&mut unknown, at.filter, 42);
    refused(&unknown, "kind 42");

    let mut entry_flag = bytes.clone();
    entry_flag[ROOT + 0x63] = 2;
    refused(&entry_flag, "has flag 2");

    // No condition, where the entry's 16 bytes hold one.
    let mut entry_count = bytes.clone();
    entry_count[ROOT + 0x62] = 0;
    refused(&entry_count, "not at");

    let mut entry_kind = bytes.clone();
    put16(&mut entry_kind, ROOT + 0x68, 11);
    refused(&entry_kind, "of kind 11");

    let mut version = bytes;
    version[4] = 0x14;
    refused(&version, "version 0x13");
}

/// A transition at 0 whose flags are `flags`, with `conditions` after +0x4C and its size word
/// counting them, its target the transition itself.
fn transition(flags: u32, conditions: &[u8]) -> Vec<u8> {
    let mut out = vec![0; TRANSITION_MIN as usize];
    let size = (out.len() + conditions.len()) as u32;
    put32(&mut out, 0, flags);
    put32(&mut out, 0x10, size);
    point(&mut out, 0x48, 0);
    out.extend_from_slice(conditions);
    out
}

/// A condition's bytes: its kind, its flag half, then its words.
fn condition(kind: u16, flag: u16, words: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    le16(&mut out, kind);
    le16(&mut out, flag);
    words.iter().for_each(|w| le32(&mut out, *w));
    out
}

#[test]
fn a_transition_reads_the_conditions_its_flags_count_up_to_its_size() {
    let range = [0x5555_0001, 1.0f32.to_bits(), 0.0f32.to_bits(), 0x1_0001];
    let held = [
        condition(3, 1, &[0x6666_0001]),
        condition(0, 0, &range),
        condition(12, 0, &[0x7777_0001, 1]),
    ]
    .concat();
    let bytes = transition((3 << 6) | 0x2, &held);
    let (read, end) = read_transitions(&Body(&bytes), 0, 1).expect("reads");
    assert_eq!(end, bytes.len());
    let want = [
        MoveCondition {
            kind: 3,
            flag: 1,
            words: vec![0x6666_0001],
        },
        MoveCondition {
            kind: 0,
            flag: 0,
            words: range.to_vec(),
        },
        MoveCondition {
            kind: 12,
            flag: 0,
            words: vec![0x7777_0001, 1],
        },
    ];
    assert_eq!(read[0].conditions, want);
    assert_eq!((read[0].size as usize, read[0].target), (bytes.len(), 0));
    assert_eq!(condition_name(3), Some("OnFlag"));
    assert_eq!(condition_name(13), None);

    let refused = |flags: u32, held: &[u8], why: &str| {
        let bytes = transition(flags, held);
        let error = read_transitions(&Body(&bytes), 0, 1)
            .unwrap_err()
            .to_string();
        assert!(error.contains(why), "{why}: {error}");
    };
    let one = 1 << 6;
    refused(one, &condition(11, 0, &[0, 0]), "of kind 11");
    refused(one, &condition(0, 1, &range), "has flag 1");
    refused(one, &condition(4, 2, &[0]), "has flag 2");
    let upside_down = [range[0], range[2], range[1], range[3]];
    refused(one, &condition(1, 0, &upside_down), "no measured one holds");
    let third_half = [range[0], range[1], range[2], 2];
    refused(one, &condition(0, 0, &third_half), "no measured one holds");
    refused(one, &condition(12, 0, &[0, 2]), "no measured one holds");
    // One more word than kind 2 holds, or one condition fewer than the flags count.
    refused(one, &condition(2, 0, &[0, 0]), "not at");
    refused(2 << 6, &condition(2, 0, &[0]), "runs past");
    refused(one | (1 << 9), &condition(2, 0, &[0]), "bit 9");
}

/// A 64-byte body with `kind` at 0, `flags` at 8, `word` at 12 and offsets at `inputs` to 56.
fn lone(kind: u16, flags: u32, word: u32, inputs: &[usize]) -> Vec<u8> {
    let mut out = vec![0; 64];
    put16(&mut out, 0, kind);
    put32(&mut out, 8, flags);
    put32(&mut out, 12, word);
    inputs.iter().for_each(|&at| point(&mut out, at, 56));
    out
}

#[test]
fn a_leaf_takes_the_bytes_its_selectors_state() {
    let size = |bytes: &[u8]| leaf(&Body(bytes), 0, u16::from_le_bytes([bytes[0], bytes[1]]));
    // Blend: a word for selector 0 and two hashes for selector 1.
    let blend = lone(4, 0b0101, 0, &[0x10, 0x14]);
    assert_eq!(size(&blend).unwrap(), Ok((0x1C + 4 + 8 + 8, vec![56, 56])));
    // BlendN of two inputs: a pair, the two offsets, a word of child bytes and one child's pair.
    let mut blend_n = lone(10, (2 << 25) | (1 << 2), 0, &[32, 36]);
    // The child bytes follow the two offsets at 32 and 36.
    blend_n[40] = 0x11;
    let inputs = 2;
    let n = 16 + 8 + 8 + inputs * 4 + 4 * (inputs / 4 + 1) + (4 + 8);
    assert_eq!(size(&blend_n).unwrap(), Ok((n, vec![56, 56])));
    // A clip of context 1 with a word for selector 1; context 3 is no form a body holds.
    let clip = lone(11, 1 | (1 << 2), 1, &[]);
    assert_eq!(size(&clip).unwrap(), Ok((12 + 12 + 4, Vec::new())));
    let context = lone(11, 1, 3, &[]);
    assert_eq!(size(&context).unwrap(), Err(StopReason::ClipContext));
    assert_eq!(
        size(&lone(32, 0x100, 0, &[])).unwrap(),
        Err(StopReason::WideCount)
    );
    assert_eq!(
        size(&lone(32, 2, 0, &[0x10])).unwrap(),
        Ok((20 + 8 * 2, vec![56]))
    );
    // Byte 9 set to 2 adds a word; any other value of it, or byte 10 set, is no measured form.
    assert_eq!(
        size(&lone(32, 0x201, 0, &[0x10])).unwrap(),
        Ok((20 + 8 + 4, vec![56]))
    );
    for wide in [0x101, 0x1_0001] {
        assert_eq!(
            size(&lone(32, wide, 0, &[])).unwrap(),
            Err(StopReason::WideCount)
        );
    }
    assert_eq!(
        size(&lone(13, 0, 0, &[])).unwrap(),
        Err(StopReason::Unmeasured)
    );
    for flags in [0x15, 0x16] {
        assert_eq!(
            size(&lone(13, flags, 0, &[0x0C])).unwrap(),
            Ok((28, vec![56]))
        );
    }
    assert_eq!(
        size(&lone(13, 0x17, 0, &[0x0C])).unwrap(),
        Err(StopReason::Unmeasured)
    );
    // Kind 30's word at +0x08 is its input's offset, 0x1C from the node; a word after it is no
    // measured form.
    assert_eq!(size(&lone(30, 0x14, 0, &[])).unwrap(), Ok((28, vec![0x1C])));
    assert_eq!(
        size(&lone(30, 0x14, 1, &[])).unwrap(),
        Err(StopReason::Unmeasured)
    );
    assert_eq!(size(&lone(35, 0, 0, &[0x0C])).unwrap(), Ok((40, vec![56])));
    assert_eq!(
        size(&lone(35, 1, 0, &[0x0C])).unwrap(),
        Err(StopReason::Unmeasured)
    );
    assert_eq!(size(&lone(18, 0, 0, &[])).unwrap(), Ok((8, Vec::new())));
    assert!(size(&lone(10, 1, 0, &[])).is_err());
    assert_eq!(kind_name(22), Some("State"));
    assert_eq!(kind_name(28), None);
}
