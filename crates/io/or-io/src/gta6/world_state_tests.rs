use std::collections::HashMap;

use super::*;
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::rbm::schema::{PsoType, RbmClass, RbmClassEntry, RbmDataBlock};

const ARRAY_INFO: u32 = 0x100;
const PAIR: u32 = 0x5EED_0001;
const GROUP: u32 = 0x5EED_0002;
const SELECTOR: u32 = 0x53F6_7801;
const CONFIG: u32 = 0x7070_1F90;
const CHANGE: u32 = 0x3984_B82E;
const RULE_SET: u32 = 0x982B_FAAD;
const RULE: u32 = 0x2089_8E21;
const NODE: u32 = 0xB555_01C9;
const POINTERS: u32 = 0x07;
const WORDS: u32 = 0x4F;
const SELECTOR_KEY: u32 = 0x5EED_0A10;
const SET_KEY: u32 = 0x5EED_0B21;
const DAY: u32 = 0x5EED_0C32;
const NIGHT: u32 = 0x5EED_0D43;
const OPEN: u64 = 0x1111_2222_3333_4444;
const CLOSED: u64 = 0x5555_6666_7777_8888;

/// A member: name hash, offset, type code, `union_ah` and `union_ch`.
type Member = (u32, u32, u8, u16, u32);

fn class(name: u32, size: u32, members: &[Member]) -> RbmClass {
    RbmClass {
        name_hash: name,
        signature: 0,
        flags: 0,
        align_power: 3,
        data_size: size,
        major_version: 0,
        entries: (members.iter())
            .map(|&(hash, offset, code, ah, ch)| {
                RbmClassEntry::synthesised(hash, offset, PsoType::from_byte(code), ah, ch)
            })
            .collect(),
    }
}

fn info(code: u8, referent: u32) -> Member {
    (ARRAY_INFO, 0, code, 0, referent)
}

/// A pointer inside data: the 1-based block in bits 0 to 11, the offset from bit 20.
fn ptr(block: u64, offset: u64) -> u64 {
    block | offset << 20
}

fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
    buf[at..at + bytes.len()].copy_from_slice(bytes);
}

fn words(w: &[u64]) -> Vec<u8> {
    w.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A counted array `{u32 count, u32 capacity, u64 pointer}` at `at`.
fn counted(buf: &mut [u8], at: usize, count: u32, pointer: u64) {
    put(
        buf,
        at,
        &[count.to_le_bytes(), count.to_le_bytes()].concat(),
    );
    put(buf, at + 8, &pointer.to_le_bytes());
}

/// An array `{u64 pointer, u16 count, u16 capacity, u32}` at `at`.
fn array_at(buf: &mut [u8], at: usize, count: u16, pointer: u64) {
    put(buf, at, &pointer.to_le_bytes());
    put(
        buf,
        at + 8,
        &[count.to_le_bytes(), count.to_le_bytes()].concat(),
    );
}

/// A grouped hash map's header `{u32 groups, u32 count, u64 pointer}` at `at`.
fn map_at(buf: &mut [u8], at: usize, count: u32, pointer: u64) {
    put(buf, at, &[1u32.to_le_bytes(), count.to_le_bytes()].concat());
    put(buf, at + 8, &pointer.to_le_bytes());
}

/// One group of sixteen pairs of a hash key and a pointer, the first `pairs.len()` full.
fn group(pairs: &[(u32, u64)]) -> Vec<u8> {
    let mut group = vec![0x80u8; 16];
    group.resize(16 + 16 * 16, 0);
    for (j, (key, pointer)) in pairs.iter().enumerate() {
        group[j] = (key & 0x7F) as u8;
        put(&mut group, 16 + 16 * j, &key.to_le_bytes());
        put(&mut group, 16 + 16 * j + 8, &pointer.to_le_bytes());
    }
    group
}

fn classes() -> HashMap<u32, RbmClass> {
    // A map of a hash to a pointer: its key and value descriptors follow the member.
    let map = |hash: u32, offset: u32, at: u32| (hash, offset, 0x75, 0, (at + 1) << 16 | at);
    let groups: [Member; 4] = [
        info(0x11, 0),
        (0x5EED_2000, 0, 0x50, 0, 16),
        info(0x05, PAIR),
        (0x5EED_2001, 16, 0x50, 2, 16),
    ];
    let time = class(
        TIME_OF_DAY,
        0x18,
        &[(START_HOUR, 0x10, 0x14, 0, 0), (END_HOUR, 0x14, 0x14, 0, 0)],
    );
    [
        class(
            WORLD_STATE,
            0x20,
            &[(NODES, 0x10, 0x5C, 1, 0), info(0x07, 0)],
        ),
        class(NODE, 0xE0, &[(COMPONENTS, 0x20, 0x5C, 1, 0), info(0x07, 0)]),
        class(
            SELECTOR_COMPONENT,
            0x18,
            &[map(SELECTORS, 0x08, 1), info(0x4A, 0), info(0x07, 0)],
        ),
        class(
            SELECTOR,
            0x40,
            &[
                (DEFAULT_CONFIG, 0x28, 0x4A, 0, 0),
                map(CONFIGS, 0x30, 2),
                info(0x4A, 0),
                info(0x07, 0),
            ],
        ),
        class(CONFIG, 0x18, &[(CHANGES, 0x08, 0x52, 1, 0), info(0x07, 0)]),
        class(
            CHANGE,
            0x38,
            &[
                (WORD_LISTS[0], 0x08, 0x52, 3, 0),
                (WORD_LISTS[1], 0x18, 0x52, 3, 0),
                (WORD_LISTS[2], 0x28, 0x52, 3, 0),
                info(0x4F, 0),
            ],
        ),
        class(
            RULE_COMPONENT,
            0x18,
            &[map(RULE_SETS, 0x08, 1), info(0x4A, 0), info(0x07, 0)],
        ),
        class(
            RULE_SET,
            0x40,
            &[
                (RULE_SELECTOR, 0x28, 0x4A, 0, 0),
                (RULES, 0x30, 0x52, 2, 0),
                info(0x05, RULE),
            ],
        ),
        class(
            RULE,
            0x10,
            &[
                (RULE_CONFIG, 0x00, 0x4A, 0, 0),
                (CONDITION, 0x08, 0x07, 0, 0),
            ],
        ),
        time,
        class(
            PAIR,
            16,
            &[(lit(b"Key"), 0, 0x4A, 0, 0), (lit(b"Item"), 8, 0x07, 0, 0)],
        ),
        class(GROUP, 16 + 16 * 16, &groups),
    ]
    .into_iter()
    .map(|c| (c.name_hash, c))
    .collect()
}

fn block(data_type: u32, data: Vec<u8>) -> RbmDataBlock {
    RbmDataBlock {
        data_type,
        data,
        data_addr: 0,
    }
}

/// One node: a selector whose default is the day, which opens a door, and a rule that picks the
/// night, which closes it and opens another to state 2, from `start` to `end` o'clock.
fn body(start: i32, end: i32) -> RbmModel {
    let mut root = vec![0u8; 0x20];
    counted(&mut root, 0x10, 1, ptr(2, 0));
    let mut node = vec![0u8; 0xE0];
    counted(&mut node, 0x20, 2, ptr(4, 0));
    let mut selectors = vec![0u8; 0x18];
    map_at(&mut selectors, 0x08, 1, ptr(6, 0));
    let mut selector = vec![0u8; 0x40];
    put(&mut selector, 0x28, &DAY.to_le_bytes());
    map_at(&mut selector, 0x30, 2, ptr(8, 0));
    let mut configs = vec![0u8; 2 * 0x18];
    array_at(&mut configs, 0x08, 1, ptr(15, 0));
    array_at(&mut configs, 0x18 + 0x08, 1, ptr(15, 8));
    let mut rule_sets = vec![0u8; 0x18];
    map_at(&mut rule_sets, 0x08, 1, ptr(11, 0));
    let mut set = vec![0u8; 0x40];
    put(&mut set, 0x28, &SELECTOR_KEY.to_le_bytes());
    array_at(&mut set, 0x30, 1, ptr(13, 0));
    let mut rule = vec![0u8; 0x10];
    put(&mut rule, 0, &NIGHT.to_le_bytes());
    put(&mut rule, 8, &ptr(14, 0).to_le_bytes());
    let mut time = vec![0u8; 0x18];
    put(
        &mut time,
        0x10,
        &[start.to_le_bytes(), end.to_le_bytes()].concat(),
    );
    let mut changes = vec![0u8; 2 * 0x38];
    array_at(&mut changes, 0x08, 1, ptr(17, 0));
    array_at(&mut changes, 0x38 + 0x18, 1, ptr(17, 0));
    array_at(&mut changes, 0x38 + 0x28, 1, ptr(17, 8));
    let blocks = vec![
        block(WORLD_STATE, root),
        block(POINTERS, words(&[ptr(3, 0)])),
        block(NODE, node),
        block(POINTERS, words(&[ptr(5, 0), ptr(10, 0)])),
        block(SELECTOR_COMPONENT, selectors),
        block(GROUP, group(&[(SELECTOR_KEY, ptr(7, 0))])),
        block(SELECTOR, selector),
        block(GROUP, group(&[(DAY, ptr(9, 0)), (NIGHT, ptr(9, 0x18))])),
        block(CONFIG, configs),
        block(RULE_COMPONENT, rule_sets),
        block(GROUP, group(&[(SET_KEY, ptr(12, 0))])),
        block(RULE_SET, set),
        block(RULE, rule),
        block(TIME_OF_DAY, time),
        block(POINTERS, words(&[ptr(16, 0), ptr(16, 0x38)])),
        block(CHANGE, changes),
        block(WORDS, words(&[OPEN, CLOSED])),
    ];
    let classes = classes();
    RbmModel {
        class_order: classes.keys().copied().collect(),
        classes,
        enums: HashMap::new(),
        data_blocks: blocks,
        root_block_id: 1,
        enum_order: Vec::new(),
    }
}

#[test]
fn a_body_reads_to_its_selectors_configurations_and_rules() {
    let state = WorldState::read(&body(22, 6)).expect("it reads");
    assert_eq!(
        state.selectors,
        [Selector {
            key: SELECTOR_KEY,
            default: DAY,
            configs: vec![
                Config {
                    key: DAY,
                    on: vec![OPEN],
                    ..Config::default()
                },
                Config {
                    key: NIGHT,
                    off: vec![OPEN],
                    state2: vec![CLOSED],
                    ..Config::default()
                },
            ],
            rules: vec![Rule {
                config: NIGHT,
                condition: Condition::TimeOfDay { start: 22, end: 6 },
            }],
        }]
    );
    let on = |hour| {
        let mut found: Vec<u64> = words_on(&state.selectors, hour).into_iter().collect();
        found.sort_unstable();
        found
    };
    assert_eq!(on(12), [OPEN], "the default by day");
    assert_eq!(on(23), [CLOSED], "the rule past midnight, state 2 on");
    assert_eq!(on(6), [CLOSED], "the end hour is inside");
    assert_eq!(on(7), [OPEN]);
}

#[test]
fn a_body_of_another_root_is_refused() {
    let mut model = body(0, 1);
    model.data_blocks[0].data_type = NODE;
    assert!(matches!(
        WorldState::read(&model),
        Err(WorldStateError::NotWorldState(NODE))
    ));
}

#[test]
fn a_condition_holds_by_three_values() {
    let time = |start, end| Condition::TimeOfDay { start, end };
    let other = || Condition::Other("made_up".into());
    assert_eq!(time(8, 20).holds(8), Some(true));
    assert_eq!(time(8, 20).holds(20), Some(true));
    assert_eq!(time(8, 20).holds(21), Some(false));
    assert_eq!(time(21, 7).holds(3), Some(true));
    assert_eq!(time(21, 7).holds(12), Some(false));
    let minutes = Condition::TimeWithMinutes {
        start: (7, 30),
        end: (8, 15),
    };
    // The half hour stands for the whole one.
    assert_eq!(minutes.holds(7), Some(true));
    assert_eq!(minutes.holds(8), Some(false));
    assert_eq!(Condition::Raining(0.5).holds(3), Some(false));
    assert_eq!(other().holds(3), None);
    assert_eq!(Condition::Not(Box::new(other())).holds(3), None);
    assert_eq!(Condition::Not(Box::new(time(1, 2))).holds(3), Some(true));
    let and = |all| Condition::And(all);
    assert_eq!(and(vec![other(), time(1, 2)]).holds(3), Some(false));
    assert_eq!(and(vec![other(), time(1, 5)]).holds(3), None);
    assert_eq!(and(vec![Condition::True, time(1, 5)]).holds(3), Some(true));
    let or = |any| Condition::Or(any);
    assert_eq!(or(vec![other(), time(1, 5)]).holds(3), Some(true));
    assert_eq!(or(vec![other(), time(1, 2)]).holds(3), None);
    assert_eq!(or(vec![]).holds(3), Some(false));
}

#[test]
fn a_rule_nothing_evaluates_never_picks_and_the_default_follows() {
    let config = |key, on: u64| Config {
        key,
        on: vec![on],
        ..Config::default()
    };
    let mut selector = Selector {
        key: 1,
        default: DAY,
        configs: vec![config(DAY, OPEN), config(NIGHT, CLOSED)],
        rules: vec![
            Rule {
                config: NIGHT,
                condition: Condition::Other("made_up".into()),
            },
            Rule {
                config: NIGHT,
                condition: Condition::TimeOfDay { start: 0, end: 5 },
            },
        ],
    };
    assert_eq!(selector.pick(3).map(|c| c.key), Some(NIGHT));
    assert_eq!(selector.pick(12).map(|c| c.key), Some(DAY));
    // A default no configuration has, or none, picks nothing.
    selector.default = 0x0BAD;
    assert_eq!(selector.pick(12), None);
    selector.default = 0;
    assert_eq!(selector.pick(12), None);
    // One selector turning a word off wins over another turning it on.
    let off = Selector {
        default: DAY,
        configs: vec![Config {
            key: DAY,
            off: vec![OPEN],
            ..Config::default()
        }],
        ..Selector::default()
    };
    let on = Selector {
        default: DAY,
        configs: vec![config(DAY, OPEN)],
        ..Selector::default()
    };
    assert_eq!(words_on([&on], 12), HashSet::from([OPEN]));
    assert!(words_on([&on, &off], 12).is_empty());
}

#[test]
fn every_changed_byte_is_read_or_refused_and_never_a_panic() {
    for whole in [body(6, 20)] {
        crate::gta6::mutated_tests::every_block_change(&whole, |m| {
            let _ = WorldState::read(m);
        });
    }
}
