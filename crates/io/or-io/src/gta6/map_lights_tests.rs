use std::collections::HashMap;

use super::*;
use crate::meta::binary::rbm::schema::{PsoType, RbmClass, RbmClassEntry, RbmDataBlock};

/// A body whose class table declares every class the map reader and this one read, as they read it.
pub(in super::super) fn model(root_block_id: u32, blocks: Vec<RbmDataBlock>) -> RbmModel {
    model_of(
        super::super::LAYOUTS.iter().chain(LAYOUTS),
        root_block_id,
        blocks,
    )
}

/// A body whose class table declares the classes of `layouts`.
pub(in super::super) fn model_of<'a>(
    layouts: impl Iterator<Item = &'a (u32, &'static str, &'static [Member], Option<u32>)>,
    root_block_id: u32,
    blocks: Vec<RbmDataBlock>,
) -> RbmModel {
    let mut classes: HashMap<u32, RbmClass> = HashMap::new();
    for (hash, _, members, size) in layouts {
        let class = classes.entry(*hash).or_insert_with(|| RbmClass {
            name_hash: *hash,
            signature: 0,
            flags: 0,
            align_power: 4,
            data_size: 0x10,
            major_version: 0,
            entries: Vec::new(),
        });
        if let Some(size) = size {
            class.data_size = *size;
        }
        class
            .entries
            .extend(members.iter().map(|&(h, offset, code)| {
                RbmClassEntry::synthesised(h, offset, PsoType::from_byte(code), 0, 0)
            }));
    }
    RbmModel {
        class_order: classes.keys().copied().collect(),
        classes,
        enums: HashMap::new(),
        data_blocks: blocks,
        root_block_id,
        enum_order: Vec::new(),
    }
}

pub(in super::super) fn ptr(block: u64, offset: u64) -> u64 {
    block | offset << 20
}

pub(in super::super) fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
    buf[at..at + bytes.len()].copy_from_slice(bytes);
}

pub(in super::super) fn put_floats(buf: &mut [u8], at: usize, values: &[f32]) {
    for (i, v) in values.iter().enumerate() {
        put(buf, at + 4 * i, &v.to_le_bytes());
    }
}

pub(in super::super) fn array(buf: &mut [u8], at: usize, pointer: u64, count: u16) {
    put(buf, at, &pointer.to_le_bytes());
    put(buf, at + 8, &count.to_le_bytes());
    put(buf, at + 10, &count.to_le_bytes());
}

pub(in super::super) fn block(data_type: u32, data: Vec<u8>) -> RbmDataBlock {
    RbmDataBlock {
        data_type,
        data,
        data_addr: 0x5000_0000,
    }
}

/// A light whose fields are told apart by `seed`, pointing down.
fn light_bytes(seed: u8) -> Vec<u8> {
    let s = f32::from(seed);
    let mut l = vec![0u8; ATTR_SIZE];
    put_floats(&mut l, 0x120, &[s + 0.25, s + 0.5, s + 0.75]);
    put_floats(&mut l, 0x12C, &[s, 2.0 * s, 3.0 * s]);
    put_floats(&mut l, 0x138, &[10.0 + s]);
    put(&mut l, 0x140, &(0x100 | u32::from(seed)).to_le_bytes());
    put(&mut l, 0x144, &0x00F0_000Fu32.to_le_bytes());
    put_floats(&mut l, 0x148, &[20.0 + s, 8.0]);
    put_floats(&mut l, 0x154, &[0.5, 1.5, 2.5]);
    put_floats(&mut l, 0x168, &[0.1, 3.5, 0.2, 4.5, 0.05]);
    put_floats(&mut l, 0x1A0, &[0.0, 0.0, -1.0, 1.0, 0.0, 0.0, 30.0, 45.0]);
    put_floats(&mut l, 0x1DC, &[1.0, 2.0, 3.0]);
    put(&mut l, 0x282, &[1, 2, 3]);
    put(&mut l, 0x288, &[seed, 200, 100]);
    put(&mut l, 0x28B, &[9, 0, 11, 12, 13, 14, 2, seed, 16, 0, 18]);
    l
}

fn light_read(seed: u8) -> LightAttr {
    let s = f32::from(seed);
    LightAttr {
        hash_30eca388: [s + 0.25, s + 0.5, s + 0.75],
        posn: [s, 2.0 * s, 3.0 * s],
        colour: [seed, 200, 100],
        vol_outer_colour: [1, 2, 3],
        intensity: 10.0 + s,
        flags: 0x100 | u32::from(seed),
        time_flags: 0x00F0_000F,
        falloff: 20.0 + s,
        falloff_exponent: 8.0,
        vol_intensity: 0.5,
        vol_size_scale: 1.5,
        vol_outer_intensity: 2.5,
        vol_outer_exponent: 3.5,
        corona_size: 0.1,
        corona_intensity: 4.5,
        corona_z_bias: 0.05,
        shadow_near_clip: 0.2,
        direction: [0.0, 0.0, -1.0],
        tangent: [1.0, 0.0, 0.0],
        cone_inner_angle: 30.0,
        cone_outer_angle: 45.0,
        extents: [1.0, 2.0, 3.0],
        flashiness: 9,
        light_fade_distance: 11,
        shadow_fade_distance: 12,
        specular_fade_distance: 13,
        volumetric_fade_distance: 14,
        light_type: 2,
        group_id: seed,
        shadow_blur: 16,
        light_hash: 18,
    }
}

/// Blocks 4 to 7 of a body: an owner's two extensions, one of another class and one light effect
/// of two lights and one in its second list.
fn extension_blocks() -> Vec<RbmDataBlock> {
    let mut pointers = vec![0u8; 16];
    put(&mut pointers, 0, &ptr(5, 0).to_le_bytes());
    put(&mut pointers, 8, &ptr(6, 0x50).to_le_bytes());
    let mut effect = vec![0u8; 2 * EFFECT_SIZE];
    let e = &mut effect[0x50..];
    put(e, 0x08, &0xE1u32.to_le_bytes());
    put_floats(e, 0x10, &[0.5, 0.25, 0.125]);
    array(e, 0x20, ptr(7, 0), 2);
    array(e, 0x30, ptr(7, 2 * ATTR_SIZE as u64), 1);
    let lights = [light_bytes(1), light_bytes(2), light_bytes(3)].concat();
    vec![
        block(7, pointers),
        block(0x0BAD_C1A5, vec![0; 8]),
        block(C_EXTENSION_DEF_LIGHT_EFFECT, effect),
        block(C_LIGHT_ATTR_DEF, lights),
    ]
}

/// A `CMapData` of one entity, block 3, which holds the extensions of [`extension_blocks`].
pub(in super::super) fn lit_map() -> RbmModel {
    let mut root = vec![0u8; MAP_DATA_SIZE];
    array(&mut root, 0xD0, ptr(2, 0), 1);
    let mut entity = vec![0u8; 0x90];
    put_floats(&mut entity, 0x10, &[0.0, 0.0, 0.0, 1.0]);
    put(&mut entity, 0x44, &0xC1u32.to_le_bytes());
    put_floats(&mut entity, 0x54, &[1.0, 1.0]);
    put(&mut entity, 0x5C, &(-1i16).to_le_bytes());
    entity[0x7E] = 5;
    array(&mut entity, ENTITY_EXTENSIONS, ptr(4, 0), 2);
    let mut blocks = vec![
        block(C_MAP_DATA, root),
        block(7, ptr(3, 0).to_le_bytes().to_vec()),
        block(C_ENTITY_DEF, entity),
    ];
    blocks.extend(extension_blocks());
    model(1, blocks)
}

fn the_effect() -> LightEffect {
    LightEffect {
        name: 0xE1,
        offset_position: [0.5, 0.25, 0.125],
        instances: vec![light_read(1), light_read(2)],
        hash_c4e96a00: vec![light_read(3)],
    }
}

#[test]
fn an_entity_s_light_effect_reads_both_lists_and_other_extensions_are_counted() {
    let lights = read(&lit_map()).expect("it reads");
    assert_eq!(
        lights,
        MapLights {
            effects: vec![(LightOwner::Entity(0), the_effect())],
            other_extensions: 1,
        }
    );
    assert_eq!(lights.lights(), 3);
    let light = &lights.effects[0].1.instances[0];
    assert_eq!(light.light_type_name(), Some("spot"));
    assert!(light.lit_at(0) && light.lit_at(23) && !light.lit_at(4) && !light.lit_at(24));
}

#[test]
fn an_archetype_s_lights_and_an_interior_entity_s_are_told_apart() {
    let mut root = vec![0u8; MAP_TYPES_SIZE];
    array(&mut root, 0x18, ptr(2, 0), 2);
    let mut pointers = vec![0u8; 16];
    put(&mut pointers, 0, &ptr(3, 0).to_le_bytes());
    put(&mut pointers, 8, &ptr(8, 0).to_le_bytes());
    let mut base = vec![0u8; ARCHETYPE_SIZE];
    array(&mut base, ARCHETYPE_EXTENSIONS, ptr(4, 0), 2);
    let mut interior = vec![0u8; 0x220];
    array(&mut interior, INTERIOR_ENTITIES, ptr(9, 0), 1);
    let mut inside = vec![0u8; 0x90];
    array(&mut inside, ENTITY_EXTENSIONS, ptr(4, 8), 1);
    let mut blocks = vec![
        block(C_MAP_TYPES, root),
        block(7, pointers),
        block(C_BASE_ARCHETYPE_DEF, base),
    ];
    blocks.extend(extension_blocks());
    blocks.extend([
        block(C_MLO_ARCHETYPE_DEF, interior),
        block(7, ptr(10, 0).to_le_bytes().to_vec()),
        block(C_ENTITY_DEF, inside),
    ]);
    let lights = read(&model(1, blocks)).expect("it reads");
    let nowhere = Pose {
        position: [0.0; 3],
        rotation: [0.0; 4],
        scale_xy: 0.0,
        scale_z: 0.0,
    };
    let inside = LightOwner::InteriorEntity {
        archetype: 1,
        entity: 0,
        pose: nowhere,
    };
    assert_eq!(
        lights.effects,
        vec![
            (LightOwner::Archetype(0), the_effect()),
            (inside, the_effect())
        ]
    );
    assert_eq!(lights.other_extensions, 1);
}

/// A `CMapTypes` of one interior, block 3: one entity of its own, block 5, and one entity set,
/// block 6, of one entity, block 8; each entity holds the extensions of [`extension_blocks`],
/// blocks 9 to 12.
fn interior_with_a_set() -> RbmModel {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let mut root = vec![0u8; MAP_TYPES_SIZE];
    array(&mut root, 0x18, ptr(2, 0), 1);
    let mut interior = vec![0u8; 0x220];
    array(&mut interior, INTERIOR_ENTITIES, ptr(4, 0), 1);
    array(&mut interior, INTERIOR_ENTITY_SETS, ptr(6, 0), 1);
    let entity = |position: [f32; 3], rotation: [f32; 4], extensions: u16| {
        let mut e = vec![0u8; 0x90];
        put_floats(&mut e, 0x10, &rotation);
        put_floats(&mut e, 0x38, &position);
        put_floats(&mut e, 0x54, &[2.0, 3.0]);
        array(&mut e, ENTITY_EXTENSIONS, ptr(9, 0), extensions);
        e
    };
    let mut set = vec![0u8; ENTITY_SET_SIZE];
    array(&mut set, SET_ENTITIES, ptr(7, 0), 1);
    let mut blocks = vec![
        block(C_MAP_TYPES, root),
        block(7, ptr(3, 0).to_le_bytes().to_vec()),
        block(C_MLO_ARCHETYPE_DEF, interior),
        block(7, ptr(5, 0).to_le_bytes().to_vec()),
        // Stored as the conjugate of a quarter turn about +z.
        block(C_ENTITY_DEF, entity([1.0, 2.0, 3.0], [0.0, 0.0, -s, s], 2)),
        block(C_MLO_ENTITY_SET, set),
        block(7, ptr(8, 0).to_le_bytes().to_vec()),
        block(
            C_ENTITY_DEF,
            entity([4.0, 5.0, 6.0], [0.0, 0.0, 0.0, 1.0], 2),
        ),
    ];
    blocks.extend(
        extension_blocks()
            .into_iter()
            .enumerate()
            .map(|(k, mut b)| {
                // The pointers of [`extension_blocks`] name blocks 5 to 7; here they are 10 to 12.
                if k == 0 {
                    put(&mut b.data, 0, &ptr(10, 0).to_le_bytes());
                    put(&mut b.data, 8, &ptr(11, 0x50).to_le_bytes());
                }
                if k == 2 {
                    array(&mut b.data[0x50..], 0x20, ptr(12, 0), 2);
                    array(&mut b.data[0x50..], 0x30, ptr(12, 2 * ATTR_SIZE as u64), 1);
                }
                b
            }),
    );
    model(1, blocks)
}

#[test]
fn an_interior_s_entities_and_those_of_its_entity_sets_hold_lights_with_their_pose() {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let lights = read(&interior_with_a_set()).expect("it reads");
    let own = LightOwner::InteriorEntity {
        archetype: 0,
        entity: 0,
        pose: Pose {
            position: [1.0, 2.0, 3.0],
            rotation: [-0.0, -0.0, s, s],
            scale_xy: 2.0,
            scale_z: 3.0,
        },
    };
    let of_set = LightOwner::EntitySetEntity {
        archetype: 0,
        set: 0,
        entity: 0,
        pose: Pose {
            position: [4.0, 5.0, 6.0],
            rotation: [-0.0, -0.0, -0.0, 1.0],
            scale_xy: 2.0,
            scale_z: 3.0,
        },
    };
    assert_eq!(
        lights.effects,
        vec![(own, the_effect()), (of_set, the_effect())]
    );
    assert_eq!(lights.lights(), 6);

    // An entity set whose entity is no `CEntityDef`, and one whose records are of another class.
    let mut m = interior_with_a_set();
    m.data_blocks[7].data_type = C_MLO_INSTANCE_DEF;
    let e = read(&m).expect_err("no CEntityDef");
    assert!(e.to_string().contains("entitySets[0].entities[0]"), "{e}");
    let mut m = interior_with_a_set();
    m.data_blocks[5].data_type = 7;
    let e = read(&m).expect_err("no entity set");
    assert!(e.to_string().contains("do not fit a block"), "{e}");
    let mut m = interior_with_a_set();
    put_floats(&mut m.data_blocks[4].data, 0x38, &[f32::NAN]);
    assert!(read(&m).is_err(), "a pose that is not finite");
}

#[test]
fn an_interior_light_stands_in_the_world_through_its_entity_inside_the_instance() {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    // The entity inside: at (10, 0, 0) of the interior, its model turned a quarter about +z.
    let inside = Pose {
        position: [10.0, 0.0, 0.0],
        rotation: [0.0, 0.0, s, s],
        scale_xy: 1.0,
        scale_z: 1.0,
    };
    // The instance: at (100, 200, 5), turning the interior a quarter about +z too.
    let mut instance = entity_at([100.0, 200.0, 5.0], [0.0, 0.0, s, s]);
    instance.interior = true;
    let mut effect = the_effect();
    effect.offset_position = [0.0; 3];
    let mut light = light_read(1);
    light.posn = [1.0, 0.0, 2.0];
    light.direction = [1.0, 0.0, 0.0];
    let placed = inside.within(&Pose::of(&instance)).place(&effect, &light);
    let off = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f32::max);
    // In the interior the light is at (10, 1, 2); the instance turns that to (-1, 10, 2).
    assert!(
        off(placed.position, [99.0, 210.0, 7.0]) < 1e-4,
        "{placed:?}"
    );
    // Two quarter turns point +x at -x.
    assert!(off(placed.direction, [-1.0, 0.0, 0.0]) < 1e-5, "{placed:?}");
}

#[test]
fn a_light_that_does_not_read_as_measured_refuses_the_body() {
    let broken_at = |change: &dyn Fn(&mut RbmModel)| {
        let mut m = lit_map();
        change(&mut m);
        read(&m).expect_err("it is refused")
    };
    let e = broken_at(&|m: &mut RbmModel| {
        put_floats(&mut m.data_blocks[6].data, 0x1A0, &[0.0, 0.0, -2.0])
    });
    assert!(e.to_string().contains("direction's norm"), "{e}");
    let e = broken_at(&|m: &mut RbmModel| m.data_blocks[6].data[0x147] = 1);
    assert!(e.to_string().contains("past the 24 hours"), "{e}");
    let e = broken_at(&|m: &mut RbmModel| m.data_blocks[6].data_type = 0x1234);
    assert!(e.to_string().contains("do not fit a block"), "{e}");
    let e = broken_at(&|m: &mut RbmModel| m.data_blocks[6].data.truncate(2 * ATTR_SIZE));
    assert!(matches!(e, MapError::Broken { .. }), "{e}");
    let e = broken_at(&|m: &mut RbmModel| put(&mut m.data_blocks[3].data, 0, &0u64.to_le_bytes()));
    assert!(e.to_string().contains("null pointer"), "{e}");
    let e = broken_at(&|m: &mut RbmModel| {
        m.classes
            .get_mut(&C_LIGHT_ATTR_DEF)
            .expect("declared")
            .data_size = 0x2B0
    });
    assert!(
        matches!(
            e,
            MapError::Size {
                class: "CLightAttrDef",
                found: 0x2B0,
                expected: 0x2A0
            }
        ),
        "{e}"
    );
    let e = broken_at(&|m: &mut RbmModel| {
        let class = m.classes.get_mut(&C_LIGHT_ATTR_DEF).expect("declared");
        let posn = (class.entries.iter_mut())
            .find(|e| e.name_hash == lit(b"posn"))
            .expect("declared");
        posn.offset += 4;
    });
    assert!(
        matches!(
            e,
            MapError::Layout {
                class: "CLightAttrDef",
                offset: 0x12C,
                ..
            }
        ),
        "{e}"
    );
    let e = broken_at(&|m: &mut RbmModel| m.data_blocks[0].data_type = 0x1234);
    assert!(matches!(e, MapError::NotAMap(0x1234)), "{e}");
}

pub(in super::super) fn entity_at(position: [f32; 3], rotation: [f32; 4]) -> Entity {
    Entity {
        interior: false,
        id: 1,
        rotation,
        archetype_word: 2,
        position,
        archetype_name: 3,
        flags: 0,
        scale_xy: 1.0,
        scale_z: 1.0,
        parent_index: -1,
        hash_e74c3c23: 0,
        lod_dist: -1.0,
        child_lod_dist: 0.0,
        num_children: 0,
        tint_value: 0,
        priority_level: 0,
        lod_level: 5,
    }
}

#[test]
fn a_light_is_placed_by_its_entity_s_scale_then_turn_then_position() {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    // Stored as the conjugate: the model turns a quarter about +z, x onto y.
    let mut entity = entity_at([10.0, 20.0, 30.0], [0.0, 0.0, -s, s]);
    (entity.scale_xy, entity.scale_z) = (2.0, 3.0);
    let mut effect = the_effect();
    effect.offset_position = [0.5, 0.0, 0.25];
    let mut light = light_read(1);
    light.posn = [0.5, 0.0, 0.75];
    light.direction = [1.0, 0.0, 0.0];
    let placed = place(&effect, &light, &entity);
    let off = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f32::max);
    assert!(
        off(placed.position, [10.0, 22.0, 33.0]) < 1e-5,
        "{placed:?}"
    );
    assert!(off(placed.direction, [0.0, 1.0, 0.0]) < 1e-6, "{placed:?}");

    // An interior instance turns by the quaternion as it is stored.
    entity.interior = true;
    let placed = place(&effect, &light, &entity);
    assert!(
        off(placed.position, [10.0, 18.0, 33.0]) < 1e-5,
        "{placed:?}"
    );
}
