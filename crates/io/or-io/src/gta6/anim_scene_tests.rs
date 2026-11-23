use std::collections::HashMap;

use super::*;
use crate::meta::binary::rbm::schema::{PsoType, RbmClassEntry, RbmDataBlock};

const ORIGIN: u32 = 0xE19E_0658;
const STATES: u32 = 0xA2F9_7C23;
const HASH_PAIR: u32 = 0x1561_DAC7;
const ENTITY_PAIR: u32 = 0x0000_0101;
const ENTITY_GROUP: u32 = 0x0000_0102;
const SCENE_PAIR: u32 = 0x0000_0103;
const PED: u32 = 0x5EED_0001;
const PLAY: u32 = 0x5EED_0002;
const TIMED: u32 = 0x5EED_0003;
const KEY_A: u32 = 0x5EED_00A5;
const KEY_B: u32 = 0x5EED_01B6;
const ACTOR: u32 = 0x5EED_0C07;
const DICTIONARY: u32 = 0x5EED_0D08;
const CLIP_1: u32 = 0x5EED_0E09;
const CLIP_2: u32 = 0x5EED_0E19;
const CLIP_3: u32 = 0x5EED_0E29;
const SCENE_KEY: u32 = 0x5EED_0F0A;
const PLAY_SIZE: usize = 0x50;
const PED_SIZE: usize = 0x38;
const POINTERS: u32 = 0x07;

/// A member: name hash, offset, type code, `union_ah` and `union_ch`.
type Member = (u32, u32, u8, u16, u32);

fn class(name: u32, size: u32, members: &[Member]) -> RbmClass {
    RbmClass {
        name_hash: name,
        signature: 0,
        flags: 0,
        align_power: 4,
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

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn pointers(p: &[u64]) -> Vec<u8> {
    p.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A counted array `{u32 count, u32 capacity, u64 pointer}` at `at`.
fn counted(buf: &mut [u8], at: usize, count: u32, pointer: u64) {
    put(buf, at, &words(&[count, count]));
    put(buf, at + 8, &pointer.to_le_bytes());
}

fn block(data_type: u32, data: Vec<u8>) -> RbmDataBlock {
    RbmDataBlock {
        data_type,
        data,
        data_addr: 0,
    }
}

/// The scene class at `size`, its entity map's key and value descriptors after its members.
fn scene_class(size: u32) -> RbmClass {
    let mut members: Vec<Member> = (SCENE_MEMBERS.iter())
        .map(|&(hash, offset, code)| {
            let referent = match hash {
                0xA176_F880 => ORIGIN,
                EVENTS => EVENT_LIST,
                0xE6CA_E80A => STATES,
                0x59C7_C8EE => HASH_PAIR,
                ENTITIES => 22 << 16 | 21,
                _ => 0,
            };
            (hash, offset, code, 0, referent)
        })
        .collect();
    members.extend([info(STRING_HASH, 0), info(0x07, 0)]);
    class(SCENE, size, &members)
}

fn classes() -> HashMap<u32, RbmClass> {
    let play: [Member; 8] = [
        (lit(b"pChildren"), 0x08, 0x07, 0, 0),
        (0x6C60_E551, 0x10, 0x07, 0, 0),
        (lit(b"Entity"), 0x18, 0x05, 0, ENTITY_REF),
        (lit(b"startTime"), 0x28, 0x21, 0, 0),
        (lit(b"endTime"), 0x2C, 0x21, 0, 0),
        (lit(b"preloadTime"), 0x30, 0x21, 0, 0),
        (0xAB28_F89C, 0x34, 0x01, 0, 0),
        (lit(b"clip"), 0x38, 0x05, 0, CLIP_REF),
    ];
    let clip: [Member; 3] = [
        (lit(b"clipDictionaryName"), 0x08, STRING_HASH, 0, 0),
        (lit(b"clipName"), 0x0C, STRING_HASH, 0, 0),
        (0x8217_0284, 0x10, 0x01, 0, 0),
    ];
    let group: [Member; 4] = [
        info(0x11, 0),
        (0x5EED_2000, 0, 0x50, 0, 16),
        info(0x05, ENTITY_PAIR),
        (0x5EED_2001, 16, 0x50, 2, 16),
    ];
    [
        scene_class(0x160),
        class(
            SCENE_MAP,
            0x10,
            &[
                (SCENES, 0, 0x73, 0, 2 << 16 | 1),
                info(STRING_HASH, 0),
                info(0x05, SCENE),
            ],
        ),
        class(
            SCENE_PAIR,
            0x170,
            &[
                (lit(b"Key"), 0, STRING_HASH, 0, 0),
                (lit(b"Item"), 0x10, 0x05, 0, SCENE),
            ],
        ),
        class(ORIGIN, 0x50, &[]),
        class(STATES, 0x50, &[]),
        class(HASH_PAIR, 8, &[]),
        class(
            EVENT_LIST,
            0x18,
            &[info(0x07, 0), (lit(b"events"), 0x08, COUNTED_ARRAY, 0, 0)],
        ),
        class(
            ENTITY_REF,
            0x10,
            &[(lit(b"entityId"), 0x08, STRING_HASH, 0, 0)],
        ),
        class(CLIP_REF, 0x18, &clip),
        class(PLAY, PLAY_SIZE as u32, &play),
        class(
            TIMED,
            0x10,
            &[
                (lit(b"startTime"), 0x08, 0x21, 0, 0),
                (lit(b"endTime"), 0x0C, 0x21, 0, 0),
            ],
        ),
        class(
            PED,
            PED_SIZE as u32,
            &[
                (0x5EED_1000, 0x00, STRING_HASH, 0, 0),
                (lit(b"events"), 0x20, 0x05, 0, EVENT_LIST),
            ],
        ),
        class(
            ENTITY_PAIR,
            16,
            &[
                (lit(b"Key"), 0, STRING_HASH, 0, 0),
                (lit(b"Item"), 8, 0x07, 0, 0),
            ],
        ),
        class(ENTITY_GROUP, 16 + 16 * 16, &group),
    ]
    .into_iter()
    .map(|c| (c.name_hash, c))
    .collect()
}

fn model(classes: HashMap<u32, RbmClass>, blocks: Vec<RbmDataBlock>) -> RbmModel {
    RbmModel {
        class_order: classes.keys().copied().collect(),
        classes,
        enums: HashMap::new(),
        data_blocks: blocks,
        root_block_id: 1,
        enum_order: Vec::new(),
    }
}

fn play(actor: u32, start: f32, end: f32, preload: f32, clip: u32) -> Vec<u8> {
    let mut event = vec![0u8; PLAY_SIZE];
    put(&mut event, 0x20, &actor.to_le_bytes());
    let times = [start, end, preload].map(f32::to_le_bytes).concat();
    put(&mut event, 0x28, &times);
    put(&mut event, 0x40, &words(&[DICTIONARY, clip]));
    event
}

/// One scene: two events of its own, and entities A and B with one event each, B's starting
/// after its end; `patch` edits the classes and blocks before they are read.
fn single(patch: impl FnOnce(&mut HashMap<u32, RbmClass>, &mut Vec<RbmDataBlock>)) -> RbmModel {
    let mut root = vec![0u8; 0x160];
    put(&mut root, 0x88, &words(&[1, 2]));
    put(&mut root, 0x90, &ptr(2, 0).to_le_bytes());
    counted(&mut root, 0xC0, 2, ptr(6, 0));

    // Sixteen control bytes, 0x80 for an empty pair and the key's low 7 bits for a full one.
    let mut group = vec![0x80u8; 16];
    group[3] = (KEY_A & 0x7F) as u8;
    group[7] = (KEY_B & 0x7F) as u8;
    group.resize(16 + 16 * 16, 0);
    put(&mut group, 16 + 3 * 16, &KEY_A.to_le_bytes());
    put(&mut group, 16 + 3 * 16 + 8, &ptr(3, 0).to_le_bytes());
    put(&mut group, 16 + 7 * 16, &KEY_B.to_le_bytes());
    put(
        &mut group,
        16 + 7 * 16 + 8,
        &ptr(3, PED_SIZE as u64).to_le_bytes(),
    );

    let mut peds = vec![0u8; 2 * PED_SIZE];
    put(&mut peds, 0, &0x5EED_1001u32.to_le_bytes());
    counted(&mut peds, 0x28, 1, ptr(7, 0));
    put(&mut peds, PED_SIZE, &0x5EED_1002u32.to_le_bytes());
    counted(&mut peds, PED_SIZE + 0x28, 1, ptr(8, 0));

    let plays = [
        play(ACTOR, 1.5, 4.0, 0.25, CLIP_1),
        play(KEY_A, 2.0, 2.5, 0.0, CLIP_2),
        play(KEY_B, 3.0, 2.0, 0.5, CLIP_3),
    ]
    .concat();
    let mut timed = vec![0u8; 0x10];
    put(
        &mut timed,
        0x08,
        &[0.5f32, 9.0].map(f32::to_le_bytes).concat(),
    );

    let mut classes = classes();
    let mut blocks = vec![
        block(SCENE, root),
        block(ENTITY_GROUP, group),
        block(PED, peds),
        block(PLAY, plays),
        block(TIMED, timed),
        block(POINTERS, pointers(&[ptr(4, 0), ptr(5, 0)])),
        block(POINTERS, pointers(&[ptr(4, PLAY_SIZE as u64)])),
        block(POINTERS, pointers(&[ptr(4, 2 * PLAY_SIZE as u64)])),
    ];
    patch(&mut classes, &mut blocks);
    model(classes, blocks)
}

fn hash(h: u32) -> String {
    format!("hash_{h:08X}")
}

fn name(h: u32) -> String {
    default_hash_index().name_or_literal(h)
}

fn played(actor: u32, start: f32, end: f32, preload: f32, clip: u32) -> Event {
    Event {
        class: name(PLAY),
        start: Some(start),
        end: Some(end),
        preload: Some(preload),
        entity: Some(hash(actor)),
        clip: Some(Clip {
            dictionary: hash(DICTIONARY),
            name: hash(clip),
        }),
    }
}

#[test]
fn one_scene_reads_its_events_and_its_entities_events_as_stated() {
    let read = read(&single(|_, _| {})).expect("the scene reads");
    let timed = Event {
        class: name(TIMED),
        start: Some(0.5),
        end: Some(9.0),
        preload: None,
        entity: None,
        clip: None,
    };
    let entity = |key: u32, event: Event| Entity {
        key: hash(key),
        class: name(PED),
        events: vec![event],
    };
    let expected = AnimSceneBody {
        map: false,
        scenes: vec![Scene {
            key: None,
            events: vec![played(ACTOR, 1.5, 4.0, 0.25, CLIP_1), timed],
            entities: vec![
                entity(KEY_A, played(KEY_A, 2.0, 2.5, 0.0, CLIP_2)),
                entity(KEY_B, played(KEY_B, 3.0, 2.0, 0.5, CLIP_3)),
            ],
        }],
    };
    assert_eq!(read, expected);
    let scene = &read.scenes[0];
    assert_eq!(scene.all_events().count(), 4);
    // A start after its end is kept as the body states it.
    assert_eq!(scene.entities[1].events[0].span(), Some((3.0, 2.0)));
    assert_eq!(scene.events[1].span(), Some((0.5, 9.0)));
}

#[test]
fn the_typed_scene_follows_an_edit_of_its_value_tree() {
    let model = single(|_, _| {});
    let mut tree = table_form::decode(&model).expect("the tree reads");
    let Some(Value::Struct(list)) = tree.get_mut(&name(EVENTS)) else {
        panic!("the scene's events are not a structure");
    };
    let Some(Value::Array(events)) = list.get_mut(&name(lit(b"events"))) else {
        panic!("the event list is not an array");
    };
    let Some(Value::Pointer(Some(first))) = events.first_mut() else {
        panic!("the first event is not a pointer");
    };
    first.set(name(lit(b"startTime")), Value::Float(0.75));
    let read = read_tree(&model, &tree).expect("the edited tree reads");
    assert_eq!(read.scenes[0].events[0].start, Some(0.75));
    assert_eq!(read.scenes[0].events[0].end, Some(4.0));
}

#[test]
fn a_map_reads_each_scene_in_place_with_its_key() {
    let mut root = vec![0u8; 0x10];
    put(&mut root, 0, &words(&[2, 1]));
    put(&mut root, 8, &ptr(2, 0).to_le_bytes());
    // Slot 0 is empty, all zero; slot 1 holds a key and a scene with nothing in it.
    let mut pairs = vec![0u8; 2 * 0x170];
    put(&mut pairs, 0x170, &SCENE_KEY.to_le_bytes());
    let map = model(
        classes(),
        vec![block(SCENE_MAP, root), block(SCENE_PAIR, pairs)],
    );
    let expected = AnimSceneBody {
        map: true,
        scenes: vec![Scene {
            key: Some(hash(SCENE_KEY)),
            events: Vec::new(),
            entities: Vec::new(),
        }],
    };
    assert_eq!(read(&map).expect("the map reads"), expected);
}

#[test]
fn a_body_whose_scene_layout_differs_is_refused_by_name() {
    let shrunk = single(|classes, _| {
        classes.insert(SCENE, scene_class(0x150));
    });
    assert!(matches!(read(&shrunk), Err(AnimSceneError::Layout(_))));

    let moved = single(|classes, _| {
        let mut scene = scene_class(0x160);
        scene.entries[14].offset = 0x14C;
        classes.insert(SCENE, scene);
    });
    assert!(matches!(read(&moved), Err(AnimSceneError::Layout(_))));

    let layered = |member: u32| {
        single(|classes, _| {
            let at_60: [Member; 1] = [(member, 0x60, 0x05, 0, 0x8233_6229)];
            classes.insert(LAYERED_PLAY, class(LAYERED_PLAY, 0x250, &at_60));
        })
    };
    assert!(matches!(
        read(&layered(lit(b"Layers"))),
        Err(AnimSceneError::Layout(_))
    ));
    assert!(read(&layered(0x82FE_B24D)).is_ok());
}

#[test]
fn a_root_of_another_class_is_no_scene() {
    let other = single(|_, blocks| blocks[0].data_type = PLAY);
    assert!(matches!(read(&other), Err(AnimSceneError::NotAScene(PLAY))));
}

#[test]
fn an_entity_with_no_object_or_a_list_past_its_block_is_an_error() {
    let no_object = single(|_, blocks| {
        put(&mut blocks[1].data, 16 + 7 * 16 + 8, &0u64.to_le_bytes());
    });
    assert!(matches!(
        read(&no_object),
        Err(AnimSceneError::Broken { .. })
    ));

    let past = single(|_, blocks| counted(&mut blocks[0].data, 0xC0, 3, ptr(6, 0)));
    assert!(read(&past).is_err());

    let truncated = single(|_, blocks| blocks[3].data.truncate(PLAY_SIZE + 0x10));
    assert!(read(&truncated).is_err());
}
