use super::*;
use crate::gta6::map::lights::tests::{array, block, model, ptr, put, put_floats};
use crate::gta6::map::lights::{C_EXTENSION_DEF_LIGHT_EFFECT, C_LIGHT_ATTR_DEF};
use crate::gta6::map::{C_MAP_TYPES, MapError};

const C_BASE_ARCHETYPE_DEF: u32 = crate::hash::literal_string_hash(b"CBaseArchetypeDef");
const ATTR_SIZE: usize = 0x2A0;
const EFFECT_SIZE: usize = 0x50;
const ARCHETYPE_SIZE: usize = 0xB8;
const MAP_TYPES_SIZE: usize = 0x50;

/// A light of type `light_type` and flags `flags`, whose root offset and `posn` differ.
fn light_bytes(light_type: u8, flags: u32) -> Vec<u8> {
    let mut l = vec![0u8; ATTR_SIZE];
    put_floats(&mut l, 0x120, &[1.0, 2.0, 3.0]);
    put_floats(&mut l, 0x12C, &[50.0, 60.0, 70.0]);
    put_floats(&mut l, 0x138, &[25.0]);
    put(&mut l, 0x140, &flags.to_le_bytes());
    put(&mut l, 0x144, &0x00E0_007Fu32.to_le_bytes());
    put_floats(&mut l, 0x148, &[12.0, 8.0]);
    put_floats(&mut l, 0x1A0, &[1.0, 0.0, 0.0]);
    put_floats(&mut l, 0x1B8, &[20.0, 40.0]);
    put_floats(&mut l, 0x1DC, &[3.0, 0.5, 0.5]);
    put(&mut l, 0x288, &[255, 0, 51]);
    l[0x291] = light_type;
    l
}

/// A `CMapTypes` of two archetypes: the first, of word 0xA1, holds one effect of a spot and a
/// corona-only light; the second, of word 0xA2, holds none.
fn lit_types() -> RbmModel {
    let mut root = vec![0u8; MAP_TYPES_SIZE];
    array(&mut root, 0x18, ptr(2, 0), 2);
    let mut pointers = vec![0u8; 16];
    put(&mut pointers, 0, &ptr(3, 0).to_le_bytes());
    put(&mut pointers, 8, &ptr(4, 0).to_le_bytes());
    let archetype = |word: u64, extensions: u16| {
        let mut a = vec![0u8; ARCHETYPE_SIZE];
        put(&mut a, 0x08, &word.to_le_bytes());
        put(&mut a, 0x78, &(word as u32 | 0x100).to_le_bytes());
        array(&mut a, 0x90, ptr(5, 0), extensions);
        a
    };
    let mut effect = vec![0u8; EFFECT_SIZE];
    put_floats(&mut effect, 0x10, &[0.5, 0.25, 0.125]);
    array(&mut effect, 0x20, ptr(7, 0), 2);
    let lights = [light_bytes(2, 0), light_bytes(1, CORONA_ONLY)].concat();
    model(
        1,
        vec![
            block(C_MAP_TYPES, root),
            block(7, pointers),
            block(C_BASE_ARCHETYPE_DEF, archetype(0xA1, 1)),
            block(C_BASE_ARCHETYPE_DEF, archetype(0xA2, 0)),
            block(7, ptr(6, 0).to_le_bytes().to_vec()),
            block(C_EXTENSION_DEF_LIGHT_EFFECT, effect),
            block(C_LIGHT_ATTR_DEF, lights),
        ],
    )
}

#[test]
fn an_archetype_s_lights_stand_at_the_effect_s_offset_plus_their_root_offset_by_its_word() {
    let lights = archetype_lights(&lit_types()).expect("it reads");
    assert_eq!(lights.len(), 1, "the archetype with no light, and the corona, give none");
    let (word, held) = &lights[0];
    assert_eq!(*word, 0xA1);
    assert_eq!(
        held.as_slice(),
        [Light {
            kind: LightKind::Spot,
            position: [1.5, 2.25, 3.125],
            direction: [1.0, 0.0, 0.0],
            colour: [1.0, 0.0, 0.2],
            intensity: 25.0,
            radius: 12.0,
            falloff_exponent: 8.0,
            hours: 0x00E0_007F,
            flags: 0,
            cone_inner: 20.0,
            cone_outer: 40.0,
            length: 3.0,
        }]
    );
    assert!(held[0].lit_at(0) && held[0].lit_at(6) && !held[0].lit_at(7) && held[0].lit_at(21));
    assert!(!held[0].lit_at(24));
}

#[test]
fn a_body_whose_lights_do_not_read_is_refused() {
    let mut broken = lit_types();
    put_floats(&mut broken.data_blocks[6].data, 0x1A0, &[0.0, 0.0, 2.0]);
    let refused = archetype_lights(&broken);
    assert!(matches!(refused, Err(MapError::Broken { .. })), "{refused:?}");
}

#[test]
fn a_light_is_placed_by_scale_then_turn_then_position_and_its_direction_turns() {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let pose = Pose {
        position: [10.0, 20.0, 30.0],
        // A quarter turn about +z: x onto y.
        rotation: [0.0, 0.0, s, s],
        scale_xy: 2.0,
        scale_z: 3.0,
    };
    let effect = LightEffect {
        name: 0,
        offset_position: [0.5, 0.0, 0.0],
        instances: Vec::new(),
        hash_c4e96a00: Vec::new(),
    };
    let mut attr = lights::read(&lit_types()).expect("it reads").effects[0].1.instances[0].clone();
    attr.hash_30eca388 = [0.5, 0.0, 1.0];
    let light = Light::of(&effect, &attr).expect("a spot");
    let placed = light.placed(&pose);
    let off = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f32::max);
    assert!(off(placed.position, [10.0, 22.0, 33.0]) < 1e-5, "{placed:?}");
    assert!(off(placed.direction, [0.0, 1.0, 0.0]) < 1e-6, "{placed:?}");
    assert_eq!((placed.radius, placed.intensity), (light.radius, light.intensity));
}

#[test]
fn a_type_no_frame_draws_is_no_light_and_the_four_kinds_are_named() {
    assert_eq!(LightKind::of(1), Some(LightKind::Point));
    assert_eq!(LightKind::of(2), Some(LightKind::Spot));
    assert_eq!(LightKind::of(4), Some(LightKind::Capsule));
    assert_eq!(LightKind::of(32), Some(LightKind::Area));
    assert_eq!(LightKind::of(3), None);
    let read = lights::read(&lit_types()).expect("it reads");
    let (_, effect) = &read.effects[0];
    let mut odd = effect.instances[0].clone();
    odd.light_type = 8;
    assert_eq!(Light::of(effect, &odd), None);
}
