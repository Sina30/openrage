use super::super::lights::tests::{array, block, model_of, ptr, put, put_floats};
use super::super::{C_BASE_ARCHETYPE_DEF, C_MAP_DATA, C_MAP_TYPES, MAP_DATA_SIZE, MAP_TYPES_SIZE};
use super::*;

const MLO_SIZE: usize = 0x220;
const PORTAL_SIZE: usize = 0x58;
const C_MLO_PORTAL_DEF: u32 = lit(b"CMloPortalDef");

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u64s(values: &[u64]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn with_tables(blocks: Vec<crate::meta::binary::rbm::schema::RbmDataBlock>) -> RbmModel {
    model_of(
        (super::super::LAYOUTS.iter())
            .chain(links::LAYOUTS)
            .chain(LAYOUTS),
        1,
        blocks,
    )
}

/// An entity record told apart by `seed`, turned by the conjugate of `stored`.
fn entity(seed: u32, stored: [f32; 4]) -> Vec<u8> {
    let mut e = vec![0u8; 0x90];
    put_floats(&mut e, 0x10, &stored);
    put(&mut e, 0x20, &(u64::from(seed) << 32 | 0xB1).to_le_bytes());
    put_floats(&mut e, 0x38, &[seed as f32, 2.0 * seed as f32, -3.0]);
    put(&mut e, 0x44, &(seed | 0xC1).to_le_bytes());
    put(&mut e, 0x48, &(0x18_0000 | seed).to_le_bytes());
    put_floats(&mut e, 0x54, &[1.5, 2.5]);
    put(&mut e, 0x5C, &(-1i16).to_le_bytes());
    put_floats(&mut e, 0x60, &[10.0 * seed as f32, 0.0]);
    e[0x7E] = 5;
    // No parent map picked.
    e[0x7F] = 0xFF;
    e
}

fn child(seed: u32, rotation: [f32; 4]) -> InteriorChild {
    InteriorChild {
        archetype_word: u64::from(seed) << 32 | 0xB1,
        archetype_name: seed | 0xC1,
        position: [seed as f32, 2.0 * seed as f32, -3.0],
        rotation,
        scale_xy: 1.5,
        scale_z: 2.5,
        lod_dist: 10.0 * seed as f32,
        flags: 0x18_0000 | seed,
    }
}

/// A base archetype, then an interior of two entities (blocks 6 and 7), two rooms, one portal
/// between them and one entity set whose two entities stand in room 1 and in portal 0.
fn types_model() -> RbmModel {
    let mut root = vec![0u8; MAP_TYPES_SIZE];
    array(&mut root, 0x18, ptr(2, 0), 2);
    let mut mlo = vec![0u8; MLO_SIZE];
    put(&mut mlo, 0x08, &0x5555_0000_0000_0005u64.to_le_bytes());
    put_floats(&mut mlo, 0x38, &[45.0]);
    put_floats(&mut mlo, 0x40, &[-10.0, -20.0, -3.0, 10.0, 20.0, 3.0]);
    put(&mut mlo, 0x78, &0x6666_0006u32.to_le_bytes());
    put(&mut mlo, 0xB8, &0x400u32.to_le_bytes());
    array(&mut mlo, 0xC0, ptr(5, 0), 2);
    array(&mut mlo, 0xD0, ptr(8, 0), 2);
    array(&mut mlo, 0xE0, ptr(9, 0), 1);
    array(&mut mlo, 0xF0, ptr(14, 0), 1);
    let mut rooms = vec![0u8; 2 * ROOM_SIZE];
    for (j, attached) in [None, Some(ptr(10, 0))].into_iter().enumerate() {
        let r = &mut rooms[ROOM_SIZE * j..];
        put(r, 0x00, &(0xA0 + j as u32).to_le_bytes());
        put_floats(r, 0x10, &[-1.0, -2.0, -3.0, 0.0, 4.0, 5.0, 6.0]);
        put_floats(r, 0x30, &[0.25 + j as f32]);
        put(r, 0x34, &u32s(&[0x7C00 + j as u32, 0x7C10 + j as u32, 7]));
        put(r, 0x40, &1u32.to_le_bytes());
        if let Some(p) = attached {
            array(r, 0x50, p, 1);
        }
    }
    let mut portal = vec![0u8; PORTAL_SIZE];
    put(&mut portal, 0x08, &u32s(&[0, 1, 9]));
    array(&mut portal, 0x38, ptr(11, 0), 4);
    array(&mut portal, 0x48, ptr(12, 0), 1);
    let mut set = vec![0u8; ENTITY_SET_SIZE];
    put(&mut set, 0x00, &0x05E7u32.to_le_bytes());
    array(&mut set, 0x08, ptr(13, 0), 2);
    array(&mut set, 0x18, ptr(15, 0), 2);
    let mut base = vec![0u8; 0xB8];
    put(&mut base, 0x78, &0xBA5Eu32.to_le_bytes());
    with_tables(vec![
        block(C_MAP_TYPES, root),
        block(7, u64s(&[ptr(3, 0), ptr(4, 0)])),
        block(C_BASE_ARCHETYPE_DEF, base),
        block(C_MLO_ARCHETYPE_DEF, mlo),
        block(7, u64s(&[ptr(6, 0), ptr(7, 0)])),
        block(C_ENTITY_DEF, entity(1, [0.0, 0.0, 0.0, 1.0])),
        block(C_ENTITY_DEF, entity(2, [0.0, 0.0, 0.6, 0.8])),
        block(C_MLO_ROOM_DEF, rooms),
        block(C_MLO_PORTAL_DEF, portal),
        block(0x15, u32s(&[0])),
        block(0x33, vec![0u8; 4 * 0x10]),
        block(0x15, u32s(&[1])),
        block(0x15, u32s(&[1, 0x8000_0000])),
        block(C_MLO_ENTITY_SET, set),
        block(7, u64s(&[ptr(7, 0), ptr(6, 0)])),
    ])
}

#[test]
fn an_interior_reads_what_places_its_entities_its_rooms_time_cycles_and_its_distance() {
    let interiors = read(&types_model()).expect("it reads");
    let (first, second) = (
        child(1, [0.0, 0.0, 0.0, 1.0]),
        child(2, [0.0, 0.0, -0.6, 0.8]),
    );
    let room = |j: u32, attached_objects| InteriorRoom {
        room: Room {
            name: 0xA0 + j,
            bb_min: [-1.0, -2.0, -3.0],
            bb_max: [4.0, 5.0, 6.0],
            flags: 7,
            portal_count: 1,
            floor_id: 0,
            attached_objects,
        },
        blend: 0.25 + j as f32,
        timecycle_name: 0x7C00 + j,
        secondary_timecycle_name: 0x7C10 + j,
    };
    let expected = Interior {
        archetype: 1,
        name: 0x6666_0006,
        name_word: 0x5555_0000_0000_0005,
        lod_dist: 45.0,
        bb_min: [-10.0, -20.0, -3.0],
        bb_max: [10.0, 20.0, 3.0],
        mlo_flags: 0x400,
        entities: vec![first, second],
        rooms: vec![room(0, vec![]), room(1, vec![0])],
        portals: vec![Portal {
            room_from: 0,
            room_to: 1,
            flags: 9,
            corners: [[0.0; 3]; 4],
            attached_objects: vec![1],
        }],
        entity_sets: vec![InteriorSet {
            name: 0x05E7,
            locations: vec![EntitySetLocation::Room(1), EntitySetLocation::Portal(0)],
            entities: vec![second, first],
        }],
        time_cycle_modifiers: vec![],
    };
    assert_eq!(interiors, [expected]);
}

#[test]
fn an_instance_draws_the_interior_s_own_entities_and_those_of_the_sets_it_names() {
    let interior = read(&types_model()).expect("it reads").remove(0);
    let words = |on: &[u32]| -> Vec<u64> { interior.drawn(on).map(|e| e.archetype_word).collect() };
    let (one, two) = (0x1_0000_00B1, 0x2_0000_00B1);
    assert_eq!(words(&[]), [one, two]);
    assert_eq!(words(&[0xBAD]), [one, two]);
    assert_eq!(words(&[0xBAD, 0x05E7]), [one, two, two, one]);
}

#[test]
fn an_entity_that_breaks_a_map_s_level_rules_still_reads_inside_an_interior() {
    let mut m = types_model();
    // An ORPHANHD entity with children: refused among a map's entities, and measured in an interior.
    put(&mut m.data_blocks[5].data, 0x68, &3u32.to_le_bytes());
    assert_eq!(read(&m).expect("it reads")[0].entities.len(), 2);
}

/// A `CEntityDef` (block 3), then a `CMloInstanceDef` (block 4) that turns two entity sets on.
fn map_model() -> RbmModel {
    let mut root = vec![0u8; MAP_DATA_SIZE];
    put(&mut root, 0x08, &0x1111_0001u32.to_le_bytes());
    array(&mut root, 0xD0, ptr(2, 0), 2);
    let mut plain = entity(1, [0.0, 0.0, 0.0, 1.0]);
    plain[0x7E] = 0;
    let mut instance = vec![0u8; INSTANCE_SIZE];
    put(&mut instance, 0, &entity(3, [0.0, 0.0, 0.6, 0.8]));
    put(&mut instance, 0x90, &u32s(&[4, 5]));
    array(&mut instance, 0xA0, ptr(5, 0), 2);
    put(&mut instance, 0xB0, &u32s(&[6, 2]));
    with_tables(vec![
        block(C_MAP_DATA, root),
        block(7, u64s(&[ptr(3, 0), ptr(4, 0)])),
        block(C_ENTITY_DEF, plain),
        block(C_MLO_INSTANCE_DEF, instance),
        block(0x4A, u32s(&[0x05E7, 0x05E8])),
    ])
}

#[test]
fn an_instance_reads_where_it_places_its_interior_and_the_sets_it_turns_on() {
    let expected = InteriorInstance {
        entity: 1,
        archetype_word: 0x3_0000_00B1,
        archetype_name: 0xC3,
        position: [3.0, 6.0, -3.0],
        rotation: [0.0, 0.0, 0.6, 0.8],
        scale_xy: 1.5,
        scale_z: 2.5,
        lod_dist: 30.0,
        group_id: 4,
        floor_id: 5,
        default_entity_sets: vec![0x05E7, 0x05E8],
        num_exit_portals: 6,
        flags: 2,
    };
    assert_eq!(read_instances(&map_model()).expect("it reads"), [expected]);
}

#[test]
fn a_map_that_places_no_interior_has_no_instance() {
    let mut m = map_model();
    put(&mut m.data_blocks[0].data, 0xD8, &[1, 0, 1, 0]);
    assert!(read_instances(&m).expect("it reads").is_empty());
}

type Patch = Box<dyn Fn(&mut RbmModel)>;
type Read = fn(&RbmModel) -> Option<String>;

fn set(block: usize, at: usize, bytes: Vec<u8>) -> Patch {
    Box::new(move |m: &mut RbmModel| put(&mut m.data_blocks[block - 1].data, at, &bytes))
}

fn interiors_error(m: &RbmModel) -> Option<String> {
    read(m).err().map(|e| e.to_string())
}

fn instances_error(m: &RbmModel) -> Option<String> {
    read_instances(m).err().map(|e| e.to_string())
}

#[test]
fn what_breaks_a_measured_rule_is_refused_with_the_rule() {
    let cases: Vec<(fn() -> RbmModel, Read, Patch, &str)> = vec![
        (
            types_model,
            interiors_error,
            set(6, 0x10, 2.0f32.to_le_bytes().to_vec()),
            "rotation's norm",
        ),
        (
            types_model,
            interiors_error,
            set(7, 0x44, vec![0; 4]),
            "archetypeName is 0",
        ),
        (
            types_model,
            interiors_error,
            Box::new(|m: &mut RbmModel| {
                let c = m.classes.get_mut(&C_MLO_ROOM_DEF).expect("declared");
                let named = lit(b"timecycleName");
                let at = c.entries.iter().position(|e| e.name_hash == named);
                c.entries[at.expect("declared")].offset = 0x3C;
            }),
            "CMloRoomDef's member",
        ),
        (
            map_model,
            interiors_error,
            Box::new(|_: &mut RbmModel| ()),
            "it is a CMapData",
        ),
        (
            types_model,
            instances_error,
            Box::new(|_: &mut RbmModel| ()),
            "it is a CMapTypes",
        ),
        (
            map_model,
            instances_error,
            Box::new(|m: &mut RbmModel| m.data_blocks[3].data.truncate(0xB8)),
            "runs past its block",
        ),
        (
            map_model,
            instances_error,
            set(4, 0xA8, vec![3, 0, 3, 0]),
            "defaultEntitySets run past their block",
        ),
        (
            map_model,
            instances_error,
            Box::new(|m: &mut RbmModel| {
                m.classes
                    .get_mut(&C_MLO_INSTANCE_DEF)
                    .expect("declared")
                    .data_size = 0x240;
            }),
            "CMloInstanceDef is 0x240 bytes",
        ),
    ];
    for (built, error_of, patch, want) in cases {
        let mut m = built();
        patch(&mut m);
        let err = error_of(&m).unwrap_or_else(|| panic!("{want}: it read"));
        assert!(err.contains(want), "{want}: {err}");
    }
}

#[test]
fn every_truncated_block_is_an_error_and_never_a_panic() {
    for built in [types_model as fn() -> RbmModel, map_model] {
        let whole = built();
        for b in 0..whole.data_blocks.len() {
            for len in 0..whole.data_blocks[b].data.len() {
                let mut m = whole.clone();
                m.data_blocks[b].data.truncate(len);
                let _ = (read(&m), read_instances(&m));
            }
        }
    }
}
