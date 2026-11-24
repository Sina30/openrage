use std::collections::HashMap;

use super::super::C_BASE_ARCHETYPE_DEF;
use super::*;
use crate::meta::binary::rbm::schema::{PsoType, RbmClass, RbmClassEntry, RbmDataBlock};

fn model(blocks: Vec<RbmDataBlock>) -> RbmModel {
    let classes: HashMap<u32, RbmClass> = (LAYOUTS.iter())
        .map(|(hash, _, members, size)| {
            let class = RbmClass {
                name_hash: *hash,
                signature: 0,
                flags: 0,
                align_power: 4,
                data_size: size.unwrap_or(0x10),
                major_version: 0,
                entries: (members.iter())
                    .map(|&(h, offset, code)| {
                        RbmClassEntry::synthesised(h, offset, PsoType::from_byte(code), 0, 0)
                    })
                    .collect(),
            };
            (*hash, class)
        })
        .collect();
    RbmModel {
        class_order: classes.keys().copied().collect(),
        classes,
        enums: HashMap::new(),
        data_blocks: blocks,
        root_block_id: 1,
        enum_order: Vec::new(),
    }
}

fn ptr(block: u64, offset: u64) -> u64 {
    block | offset << 20
}

fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
    buf[at..at + bytes.len()].copy_from_slice(bytes);
}

fn floats(buf: &mut [u8], at: usize, values: &[f32]) {
    for (i, v) in values.iter().enumerate() {
        put(buf, at + 4 * i, &v.to_le_bytes());
    }
}

fn array(buf: &mut [u8], at: usize, pointer: u64, count: u16) {
    put(buf, at, &pointer.to_le_bytes());
    put(buf, at + 8, &count.to_le_bytes());
    put(buf, at + 10, &count.to_le_bytes());
}

fn block(data_type: u32, data: Vec<u8>) -> RbmDataBlock {
    RbmDataBlock {
        data_type,
        data,
        data_addr: 0x5000_0000,
    }
}

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u64s(values: &[u64]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A record whose selector, `parentIndex`, instance and `lodLevel` are the ones given.
fn linked(size: usize, selector: i8, parent: i16, instance: i16, level: u8) -> Vec<u8> {
    let mut r = vec![0u8; size];
    r[0x7F] = selector as u8;
    put(&mut r, 0x5C, &parent.to_le_bytes());
    put(&mut r, 0x5E, &instance.to_le_bytes());
    r[0x7E] = level;
    r
}

/// Two parent maps; entity 0 picks the second, entity 1 none; one prop list of two instances
/// picks the first; one grass list of two instances.
fn map_model() -> RbmModel {
    let mut root = vec![0u8; MAP_DATA_SIZE];
    put(&mut root, 0x08, &0x1111_0001u32.to_le_bytes());
    put(&mut root, 0x18, &u32s(&[2, 2]));
    put(&mut root, 0x20, &ptr(2, 0).to_le_bytes());
    array(&mut root, 0x58, ptr(3, 0), 2);
    array(&mut root, 0xD0, ptr(4, 0), 2);
    array(&mut root, 0x148, ptr(7, 0), 1);
    array(&mut root, 0x158, ptr(9, 0), 1);
    let mut props = linked(PROP_LIST_SIZE as usize, 0, 5, 2, 2);
    array(&mut props, 0x90, ptr(8, 0), 2);
    let mut instances = vec![0u8; 2 * 0x40];
    for (i, w) in [[1.0f32, 2.0, 3.0], [4.0, 5.0, 6.0]].iter().enumerate() {
        for (row, value) in w.iter().enumerate() {
            floats(&mut instances, 0x40 * i + 0x10 * row + 0x0C, &[*value]);
        }
    }
    let mut grass = linked(GRASS_LIST_SIZE as usize, 0, -1, 0, 0);
    floats(
        &mut grass,
        0x80,
        &[10.0, 20.0, 30.0, 0.0, 110.0, 220.0, 330.0],
    );
    array(&mut grass, 0x178, ptr(10, 0), 2);
    let mut grass_instances = vec![0u8; 2 * 0x10];
    put(&mut grass_instances, 0, &[0, 0, 0xFF, 0xFF, 0x33, 0x33]);
    put(&mut grass_instances, 0x10, &[0xFF, 0xFF, 0, 0, 0, 0]);
    model(vec![
        block(C_MAP_DATA, root),
        block(0x4F, u64s(&[0xAAAA_0000_0000_0001, 0xBBBB_0000_0000_0002])),
        block(0x4A, u32s(&[0x2222_0001, 0x3333_0002])),
        block(7, u64s(&[ptr(5, 0), ptr(6, 0)])),
        block(C_ENTITY_DEF, linked(0x90, 1, 3, -1, 1)),
        block(C_ENTITY_DEF, linked(0x90, -1, -1, -1, 0)),
        block(PROP_LIST, props),
        block(PROP_INSTANCE, instances),
        block(GRASS_LIST, grass),
        block(GRASS_INSTANCE, grass_instances),
    ])
}

fn map_links_of(model: &RbmModel) -> Result<MapLinks> {
    match read(model)? {
        Links::Data(links) => Ok(links),
        other => panic!("a map's links, not {other:?}"),
    }
}

#[test]
fn a_map_reads_its_parent_maps_each_element_s_pick_and_its_instances_as_points() {
    let links = map_links_of(&map_model()).expect("it reads");
    assert_eq!(
        links.lod_parents,
        [
            LodParentMap {
                name: 0x2222_0001,
                word: 0xAAAA_0000_0000_0001
            },
            LodParentMap {
                name: 0x3333_0002,
                word: 0xBBBB_0000_0000_0002
            },
        ]
    );
    let to = |map, parent_index, instance| LodLink {
        map,
        parent_index,
        instance,
    };
    assert_eq!(links.entities, [to(Some(1), 3, -1), to(None, -1, -1)]);
    assert_eq!(links.prop_lists, [to(Some(0), 5, 2)]);
    assert_eq!(links.prop_points, [vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]]);
    let grass = &links.grass_lists[0];
    assert!(grass.archetypes.is_empty());
    let lerp = |lo: f32, hi: f32, q: f32| lo + (hi - lo) * q / 65535.0;
    let expected = [
        [
            lerp(10.0, 110.0, 0.0),
            lerp(20.0, 220.0, 65535.0),
            lerp(30.0, 330.0, 13107.0),
        ],
        [
            lerp(10.0, 110.0, 65535.0),
            lerp(20.0, 220.0, 0.0),
            lerp(30.0, 330.0, 0.0),
        ],
    ];
    assert_eq!(grass.points.len(), 2);
    for (got, want) in grass.points.iter().zip(expected) {
        for axis in 0..3 {
            assert!(
                (got[axis] - want[axis]).abs() < 1e-3,
                "{got:?} against {want:?}"
            );
        }
    }
    assert_eq!(expected[0][2], 90.0, "a fifth of the way up its box");
}

#[test]
fn a_parent_index_counts_the_parent_s_entities_then_its_prop_lists_then_its_grass_lists() {
    let at = |parent_index, instance| {
        LodLink {
            map: Some(0),
            parent_index,
            instance,
        }
        .target(2, 1, 1)
    };
    assert_eq!(at(1, -1), Some(LodTarget::Entity(1)));
    assert_eq!(
        at(2, 4),
        Some(LodTarget::PropList {
            list: 0,
            instance: Some(4)
        })
    );
    assert_eq!(
        at(2, -1),
        Some(LodTarget::PropList {
            list: 0,
            instance: None
        })
    );
    assert_eq!(at(3, -1), Some(LodTarget::GrassList(0)));
    assert_eq!(at(4, -1), None);
    assert_eq!(at(-1, -1), None);
}

/// A base archetype, then an interior of two entities, two rooms, one portal between them and
/// one entity set whose two entities stand in room 1 and in portal 0.
fn types_model() -> RbmModel {
    let mut root = vec![0u8; MAP_TYPES_SIZE];
    array(&mut root, 0x18, ptr(2, 0), 2);
    let mut mlo = vec![0u8; MLO_SIZE];
    put(&mut mlo, 0x08, &0x5555_0000_0000_0005u64.to_le_bytes());
    put(&mut mlo, 0x78, &0x6666_0006u32.to_le_bytes());
    array(&mut mlo, 0xC0, ptr(5, 0), 2);
    array(&mut mlo, 0xD0, ptr(8, 0), 2);
    array(&mut mlo, 0xE0, ptr(9, 0), 1);
    array(&mut mlo, 0xF0, ptr(14, 0), 1);
    let entity = |rotation: [f32; 4], word: u64, at: [f32; 3], name: u32| {
        let mut e = vec![0u8; 0x90];
        floats(&mut e, 0x10, &rotation);
        put(&mut e, 0x20, &word.to_le_bytes());
        floats(&mut e, 0x38, &at);
        put(&mut e, 0x44, &name.to_le_bytes());
        e
    };
    let mut rooms = vec![0u8; 2 * ROOM_SIZE];
    for (j, (name, floor, attached)) in [(0xA0u32, -1i32, None), (0xA1, 2, Some(ptr(10, 0)))]
        .into_iter()
        .enumerate()
    {
        let r = &mut rooms[ROOM_SIZE * j..];
        put(r, 0x00, &name.to_le_bytes());
        floats(
            r,
            0x10,
            &[-1.0 - j as f32, -2.0, -3.0, 0.0, 4.0, 5.0 + j as f32, 6.0],
        );
        put(r, 0x3C, &(7 + j as u32).to_le_bytes());
        put(r, 0x40, &1u32.to_le_bytes());
        put(r, 0x44, &floor.to_le_bytes());
        if let Some(p) = attached {
            array(r, 0x50, p, 1);
        }
    }
    let mut portal = vec![0u8; PORTAL_SIZE];
    put(&mut portal, 0x08, &u32s(&[0, 1, 9]));
    array(&mut portal, 0x38, ptr(11, 0), 4);
    array(&mut portal, 0x48, ptr(12, 0), 1);
    let mut corners = vec![0u8; 4 * 0x10];
    for c in 0..4 {
        floats(
            &mut corners,
            0x10 * c,
            &[c as f32, 10.0 + c as f32, 20.0 + c as f32],
        );
    }
    let mut set = vec![0u8; ENTITY_SET_SIZE];
    put(&mut set, 0x00, &0x05E7u32.to_le_bytes());
    array(&mut set, 0x08, ptr(13, 0), 2);
    array(&mut set, 0x18, ptr(15, 0), 2);
    model(vec![
        block(C_MAP_TYPES, root),
        block(7, u64s(&[ptr(3, 0), ptr(4, 0)])),
        block(C_BASE_ARCHETYPE_DEF, vec![0u8; 0xB8]),
        block(C_MLO_ARCHETYPE_DEF, mlo),
        block(7, u64s(&[ptr(6, 0), ptr(7, 0)])),
        block(
            C_ENTITY_DEF,
            entity([0.0, 0.0, 0.0, 1.0], 0x0E01, [1.0, 2.0, 3.0], 0xE1),
        ),
        block(
            C_ENTITY_DEF,
            entity([0.0, 0.0, 1.0, 0.0], 0x0E02, [4.0, 5.0, 6.0], 0xE2),
        ),
        block(C_MLO_ROOM_DEF, rooms),
        block(C_MLO_PORTAL_DEF, portal),
        block(0x15, u32s(&[0])),
        block(0x33, corners),
        block(0x15, u32s(&[1])),
        block(0x15, u32s(&[1, 0x8000_0000])),
        block(C_MLO_ENTITY_SET, set),
        block(7, u64s(&[ptr(6, 0), ptr(7, 0)])),
    ])
}

#[test]
fn an_interior_reads_its_rooms_portals_and_entity_sets_and_each_entity_is_held_once() {
    let Links::Types(interiors) = read(&types_model()).expect("it reads") else {
        panic!("a map types body's interiors");
    };
    let first = InteriorEntity {
        archetype_name: 0xE1,
        archetype_word: 0x0E01,
        position: [1.0, 2.0, 3.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
    };
    let second = InteriorEntity {
        archetype_name: 0xE2,
        archetype_word: 0x0E02,
        position: [4.0, 5.0, 6.0],
        rotation: [0.0, 0.0, 1.0, 0.0],
    };
    let room = |j: u32, name, floor_id, attached_objects| Room {
        name,
        bb_min: [-1.0 - j as f32, -2.0, -3.0],
        bb_max: [4.0, 5.0 + j as f32, 6.0],
        flags: 7 + j,
        portal_count: 1,
        floor_id,
        attached_objects,
    };
    let expected = Interior {
        archetype: 1,
        name: 0x6666_0006,
        name_word: 0x5555_0000_0000_0005,
        entities: vec![first, second],
        rooms: vec![room(0, 0xA0, -1, vec![]), room(1, 0xA1, 2, vec![0])],
        portals: vec![Portal {
            room_from: 0,
            room_to: 1,
            flags: 9,
            corners: [
                [0.0, 10.0, 20.0],
                [1.0, 11.0, 21.0],
                [2.0, 12.0, 22.0],
                [3.0, 13.0, 23.0],
            ],
            attached_objects: vec![1],
        }],
        entity_sets: vec![EntitySet {
            name: 0x05E7,
            entities: vec![
                (EntitySetLocation::Room(1), first),
                (EntitySetLocation::Portal(0), second),
            ],
        }],
        time_cycle_modifiers: vec![],
    };
    assert_eq!(interiors, [expected]);
}

type Patch = Box<dyn Fn(&mut RbmModel)>;
type Built = fn() -> RbmModel;

fn set(block: usize, at: usize, bytes: Vec<u8>) -> Patch {
    Box::new(move |m: &mut RbmModel| put(&mut m.data_blocks[block - 1].data, at, &bytes))
}

#[test]
fn a_link_that_breaks_a_measured_rule_is_refused_with_the_rule() {
    let cases: Vec<(Built, Patch, &str)> = vec![
        (map_model, set(1, 0x1C, u32s(&[1])), "its two counts differ"),
        (
            map_model,
            set(3, 4, u32s(&[0x1111_0001])),
            "names 0 or the map itself",
        ),
        (
            map_model,
            set(5, 0x7F, vec![2]),
            "selector 2 is past the 2 maps",
        ),
        (
            map_model,
            set(5, 0x7F, vec![0xFE]),
            "selector -2 is below -1",
        ),
        (
            map_model,
            set(5, 0x5C, (-1i16).to_le_bytes().to_vec()),
            "comes with parentIndex -1",
        ),
        (
            map_model,
            set(5, 0x7E, vec![5]),
            "an ORPHANHD element names a parent map",
        ),
        (
            map_model,
            set(9, 0x5C, 0i16.to_le_bytes().to_vec()),
            "a grass list names a parent",
        ),
        (
            map_model,
            set(10, 0x0E, 1u16.to_le_bytes().to_vec()),
            "word 0x0E disagrees",
        ),
        (
            types_model,
            set(13, 4, u32s(&[0x8000_0001])),
            "location 0x80000001 is past",
        ),
        (
            types_model,
            set(8, 0x40, u32s(&[2])),
            "portalCount 2 is not the 1 portals",
        ),
        (
            types_model,
            set(12, 0, u32s(&[0])),
            "attach entity 0 2 times",
        ),
        (
            types_model,
            set(12, 0, u32s(&[5])),
            "it attaches entity 5 of 2",
        ),
        (
            types_model,
            set(9, 0x0C, u32s(&[2])),
            "it joins room 0 and 2 of 2",
        ),
        (
            types_model,
            set(9, 0x40, 3u16.to_le_bytes().to_vec()),
            "3 corners, not 4",
        ),
        (
            types_model,
            set(4, 0xD8, vec![0, 0]),
            "an interior has no room",
        ),
        (
            types_model,
            set(4, 0x84, u32s(&[2])),
            "sets an assetType or a model word",
        ),
        (
            types_model,
            set(4, 0x1B0, vec![1, 0]),
            "its array at 0x1A8 is not empty",
        ),
        (
            types_model,
            Box::new(|m: &mut RbmModel| m.data_blocks[6].data_type = C_MLO_INSTANCE_DEF),
            "no CEntityDef",
        ),
        (
            map_model,
            Box::new(|m: &mut RbmModel| {
                let c = m.classes.get_mut(&C_MAP_DATA).expect("declared");
                c.entries[1].offset = 0x60;
            }),
            "CMapData's member",
        ),
    ];
    for (built, patch, want) in cases {
        let mut m = built();
        patch(&mut m);
        let err = read(&m).expect_err(want).to_string();
        assert!(err.contains(want), "{want}: {err}");
    }
}

#[test]
fn every_truncated_block_is_an_error_and_never_a_panic() {
    for built in [map_model as Built, types_model] {
        let whole = built();
        for b in 0..whole.data_blocks.len() {
            for len in 0..whole.data_blocks[b].data.len() {
                let mut m = whole.clone();
                m.data_blocks[b].data.truncate(len);
                let _ = read(&m);
            }
        }
    }
}

#[test]
fn prop_lists_that_alias_one_instance_block_are_refused_before_they_multiply() {
    let mut m = map_model();
    let lists = 64;
    let mut root = m.data_blocks[0].data.clone();
    array(&mut root, 0x148, ptr(7, 0), lists);
    let mut props = Vec::new();
    for _ in 0..lists {
        let mut list = linked(PROP_LIST_SIZE as usize, 0, 5, 2, 2);
        array(&mut list, 0x90, ptr(8, 0), 1024);
        props.extend(list);
    }
    m.data_blocks[0].data = root;
    m.data_blocks[6].data = props;
    m.data_blocks[7].data = vec![0u8; 1024 * 0x40];
    let refused = map_links_of(&m).expect_err("aliased arrays are refused");
    assert!(
        refused
            .to_string()
            .contains("more elements than its blocks hold"),
        "{refused}"
    );
}

#[test]
fn every_changed_byte_is_read_or_refused_and_never_a_panic() {
    for whole in [map_model(), types_model()] {
        crate::gta6::mutated_tests::every_block_change(&whole, |m| {
            let _ = read(m);
        });
    }
}
