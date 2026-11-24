use super::super::super::lights::tests::{array, block, ptr, put, put_floats};
use super::super::read;
use super::super::tests::stream_model;
use super::*;

const LIST: usize = GRASS_LIST_SIZE as usize;
const FERN: (u32, u64) = (0xFE01, 0xFE01_0000_0000_0001);
const PALM: (u32, u64) = (0xFA02, 0xFA02_0000_0000_0002);

fn instance(position: [u16; 3], scale: u8, scale_z: u8, rotation: u8, guid: u16) -> Vec<u8> {
    let mut i = vec![0u8; INSTANCE_SIZE];
    for (axis, q) in position.iter().enumerate() {
        put(&mut i, 2 * axis, &q.to_le_bytes());
    }
    put(
        &mut i,
        0x06,
        &[127, 255, 60, 115, 109, scale, scale_z, rotation],
    );
    put(&mut i, 0x0E, &guid.to_le_bytes());
    i
}

/// The stream model with its two grass lists filled: one of two instances in a box of 100 by 200
/// by 50 that fades from 40 m to 50 m, and one of far stand-ins for a fern and a palm with one instance.
fn grass_model() -> RbmModel {
    let mut m = stream_model();
    let mut lists = vec![0u8; 2 * LIST];
    for (list, level) in [(0, 5u8), (1, 2)] {
        let at = list * LIST;
        put(&mut lists, at + 0x5C, &(-1i16).to_le_bytes());
        lists[at + 0x7E] = level;
        put_floats(&mut lists, at + 0x90, &[100.0, 200.0, 50.0]);
    }
    put(&mut lists, 0x20, &0x6A55_0000_0000_0001u64.to_le_bytes());
    put_floats(&mut lists, 0x38, &[50.0, 100.0, 10.0]);
    put(&mut lists, 0x44, &0x6A55u32.to_le_bytes());
    put_floats(&mut lists, 0x60, &[130.0]);
    put_floats(&mut lists, 0xA0, &[0.5, 1.5]);
    put_floats(&mut lists, 0xB8, &[40.0, 50.0, 1.0]);
    array(&mut lists, 0x178, ptr(12, 0), 2);
    put_floats(&mut lists, LIST + 0xA0, &[1.0, 2.0]);
    put_floats(&mut lists, LIST + 0xB8, &[900.0, 900.0, 1.0]);
    lists[LIST + 0x150] = 1;
    array(&mut lists, LIST + 0x158, ptr(13, 0), 2);
    array(&mut lists, LIST + 0x168, ptr(14, 0), 2);
    array(&mut lists, LIST + 0x178, ptr(15, 0), 1);
    m.data_blocks[9].data = lists;
    let two = [
        instance([0, 65535, 13107], 255, 0, 18, 0),
        instance([65535, 0, 0], 0, 0, 72, 0),
    ];
    m.data_blocks.extend([
        block(GRASS_INSTANCE, two.concat()),
        block(0x4A, [FERN.0, PALM.0].map(u32::to_le_bytes).concat()),
        block(0x4F, [FERN.1, PALM.1].map(u64::to_le_bytes).concat()),
        block(GRASS_INSTANCE, instance([1, 2, 3], 255, 0, 0, 0x277)),
    ]);
    m
}

#[test]
fn a_grass_list_reads_its_distances_and_each_instance_as_the_body_states_it() {
    let map = read(&grass_model()).expect("it reads");
    let [plain, far] = &map.grass[..] else {
        panic!("two grass lists");
    };
    assert_eq!(
        (plain.archetype_word, plain.archetype_name),
        (0x6A55_0000_0000_0001, 0x6A55)
    );
    assert_eq!(plain.position, [50.0, 100.0, 10.0]);
    assert_eq!((plain.lod_dist, plain.lod_level), (130.0, 5));
    assert_eq!(plain.scale_range, [0.5, 1.5]);
    assert_eq!(
        (plain.lod_fade_start_dist, plain.instance_lod_dist),
        (40.0, 50.0)
    );
    assert_eq!(plain.orient_to_terrain, 1.0);
    assert!(!plain.slod && plain.archetypes.is_empty());
    assert_eq!(
        plain.instances[0],
        GrassInstance {
            position: [0, 65535, 13107],
            normal: [127, 255],
            colour: [60, 115, 109],
            scale: 255,
            scale_z: 0,
            rotation: 18,
            guid: 0,
        }
    );
    assert!(far.slod);
    assert_eq!(far.archetypes, vec![FERN, PALM]);
    assert_eq!((far.lod_level, far.instances[0].guid), (2, 0x277));
}

#[test]
fn an_instance_stands_at_its_fractions_of_the_box_scaled_between_the_least_and_the_most() {
    let map = read(&grass_model()).expect("it reads");
    let (plain, far) = (&map.grass[0], &map.grass[1]);
    let [first, second] = plain.instances[..] else {
        panic!("two instances");
    };
    // A fifth of the box's height is 13107 of 65535.
    assert_eq!(plain.position_of(&first), [0.0, 200.0, 10.0]);
    assert_eq!(plain.position_of(&second), [100.0, 0.0, 0.0]);
    // A list that is no far stand-in scales alike across and up, whatever its `ScaleZ`.
    assert_eq!(plain.scale_of(&first), [1.5, 1.5]);
    assert_eq!(plain.scale_of(&second), [0.5, 0.5]);
    // A far stand-in takes its height from `ScaleZ`.
    assert_eq!(far.scale_of(&far.instances[0]), [2.0, 1.0]);
}

#[test]
fn a_turn_counts_seventy_seconds_and_a_normal_is_of_length_one() {
    let at = |normal, rotation| GrassInstance {
        position: [0; 3],
        normal,
        colour: [0; 3],
        scale: 0,
        scale_z: 0,
        rotation,
        guid: 0,
    };
    assert!((at([0, 0], 18).yaw() - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    assert!((at([0, 0], 72).yaw() - std::f32::consts::TAU).abs() < 1e-5);
    assert_eq!(at([0, 0], 0).yaw(), 0.0);
    // The middle of both bytes is straight up, within a step of the byte.
    let up = at([128, 128], 0).normal();
    assert!(up[2] > 0.9999 && up[0].abs() < 0.01 && up[1].abs() < 0.01);
    // 255 is a whole 1: the normal lies along x, on the ground.
    assert_eq!(at([255, 128], 0).normal()[2], 0.0);
    for normal in [[0, 0], [255, 255], [40, 200], [128, 0], [127, 255]] {
        let n = at(normal, 0).normal();
        let length = n.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-5, "{normal:?}: {length}");
        assert!(n[2] >= 0.0, "{normal:?} points down");
    }
}

type Patch = Box<dyn Fn(&mut RbmModel)>;

#[test]
fn a_grass_list_that_breaks_a_measured_rule_is_refused() {
    let cases: Vec<(Patch, &str)> = vec![
        (
            Box::new(|m: &mut RbmModel| m.data_blocks[9].data[0x7E] = 7),
            "GrassInstanceList[0]: its lodLevel 7 is no LODTYPES_DEPTH value",
        ),
        (
            Box::new(|m: &mut RbmModel| m.data_blocks[9].data[0x150] = 2),
            "GrassInstanceList[0]: its IsSLODList 2 is neither 0 nor 1",
        ),
        (
            Box::new(|m: &mut RbmModel| m.data_blocks[9].data[0x150] = 1),
            "GrassInstanceList[0]: its IsSLODList disagrees with its archetype arrays",
        ),
        (
            Box::new(|m: &mut RbmModel| m.data_blocks[9].data[LIST + 0x150] = 0),
            "GrassInstanceList[1]: its IsSLODList disagrees with its archetype arrays",
        ),
        (
            Box::new(|m: &mut RbmModel| put_floats(&mut m.data_blocks[9].data, 0xA0, &[0.0, 1.0])),
            "GrassInstanceList[0]: its ScaleRange is no least and most scale above 0",
        ),
        (
            Box::new(|m: &mut RbmModel| put_floats(&mut m.data_blocks[9].data, 0xA0, &[2.0, 1.0])),
            "GrassInstanceList[0]: its ScaleRange is no least and most scale above 0",
        ),
        (
            Box::new(|m: &mut RbmModel| {
                put_floats(&mut m.data_blocks[9].data, 0xBC, &[f32::NAN]);
            }),
            "GrassInstanceList[0]: the number at 0xBC is not finite",
        ),
        (
            Box::new(|m: &mut RbmModel| {
                let class = m.classes.get_mut(&GRASS_LIST).expect("declared");
                class.entries.retain(|member| member.offset != 0xBC);
            }),
            "rage__fwGrassInstanceListDef's member",
        ),
        (
            Box::new(|m: &mut RbmModel| {
                let class = m.classes.get_mut(&GRASS_INSTANCE).expect("declared");
                class.entries.retain(|member| member.offset != 0x0D);
            }),
            "rage__fwGrassInstanceListDef__InstanceData's member",
        ),
    ];
    for (patch, want) in cases {
        let mut m = grass_model();
        patch(&mut m);
        let err = read(&m).expect_err(want).to_string();
        assert!(err.contains(want), "{want}: {err}");
    }
}

#[test]
fn every_truncated_grass_block_is_an_error_and_never_a_panic() {
    let whole = grass_model();
    for b in 9..whole.data_blocks.len() {
        for len in 0..whole.data_blocks[b].data.len() {
            let mut m = whole.clone();
            m.data_blocks[b].data.truncate(len);
            let _ = read(&m);
        }
    }
}
