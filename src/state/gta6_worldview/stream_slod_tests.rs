use std::path::PathBuf;

use or_io::gta6::map::stream::GrassInstance;
use or_io::gta6::map::Aabb;

use super::super::super::index::tests_support::index_of;
use super::*;

const OAK: u64 = 100;
const PALM: u64 = 200;
/// A word no archetype of the index bears.
const LOST: u64 = 999;

/// An index whose oak has a model and whose palm has none, with the far stand-in ids 1 to 3 solved
/// to the oak, the palm and an archetype it does not hold.
fn index() -> WorldIndex {
    let mut index = index_of(
        vec![PathBuf::from("a.rpf")],
        &[(OAK, 500.0, Some(0x4000)), (PALM, 500.0, None)],
    );
    index.slod = vec![0, OAK, PALM, LOST];
    index
}

fn stand_in(position: [u16; 3], guid: u16) -> GrassInstance {
    GrassInstance {
        position,
        normal: [128, 128],
        colour: [10, 20, 30],
        scale: 255,
        scale_z: 128,
        rotation: 36,
        guid,
    }
}

fn far() -> StreamGrassList {
    StreamGrassList {
        archetype_word: 1,
        archetype_name: 1,
        position: [0.0; 3],
        lod_dist: 1000.0,
        lod_level: 5,
        batch_aabb: Aabb {
            min: [10.0, 20.0, 30.0],
            max: [110.0, 220.0, 80.0],
        },
        scale_range: [0.5, 2.0],
        lod_fade_start_dist: 899.0,
        instance_lod_dist: 900.0,
        orient_to_terrain: 1.0,
        slod: true,
        archetypes: vec![(1, OAK), (2, PALM)],
        instances: vec![
            stand_in([0, 0, 0], 1),
            stand_in([65535, 0, 7], 2),
            stand_in([5, 6, 7], 1),
        ],
    }
}

#[test]
fn each_id_names_its_archetypes_sphere_and_one_left_unsolved_names_nothing() {
    let index = index();
    let species = species(&index);
    let oak = Species {
        archetype: OAK as u32,
        centre: [0.0; 3],
        radius: 1.0,
    };
    let palm = Species {
        archetype: PALM as u32,
        ..oak
    };
    assert_eq!(
        species[..],
        [Species::default(), oak, palm, Species::default()]
    );
    assert_eq!(
        model_of(&index, 1),
        Some(ModelRef {
            archive: 0,
            offset: 0x4000,
            entry: None
        })
    );
    for id in [0, 2, 3, 4, 70_000] {
        assert_eq!(model_of(&index, id), None, "{id}");
    }
}

#[test]
fn a_card_list_keeps_the_box_the_scales_and_each_record_as_stored() {
    let list = far();
    let cards = CardList::of(&list, &index());
    assert_eq!(cards.min, [10.0, 20.0, 30.0]);
    assert_eq!(cards.size, [100.0, 200.0, 50.0]);
    assert_eq!(cards.centre, [60.0, 120.0, 55.0]);
    // The box's half diagonal, and the corner of a radius-1 model's card at the most scale past it.
    let half = Vec3::new(100.0, 200.0, 50.0).length() / 2.0;
    let corner = 2.0 * std::f32::consts::SQRT_2;
    assert!(
        (cards.radius - (half + corner)).abs() < 1e-4,
        "{}",
        cards.radius
    );
    assert_eq!(cards.scale_range, [0.5, 2.0]);
    assert_eq!((cards.distance, cards.fade), (900.0, 1.0));
    let stored: Vec<[u8; 16]> = list.instances.iter().map(|i| i.record()).collect();
    assert_eq!(cards.records[..], stored[..]);
    assert_eq!(
        u16::from_le_bytes([cards.records[1][14], cards.records[1][15]]),
        2
    );
    // Each species once, for the atlas to wait for.
    assert_eq!(cards.ids[..], [1, 2]);
    // Seen through a view, it scales its distance and takes the list's fade, sharing the records.
    let seen = cards.seen(1.5, 0.25);
    assert_eq!((seen.distance, seen.fade), (1350.0, 0.25));
    assert!(Arc::ptr_eq(&seen.records, &cards.records));
    assert_eq!((seen.centre, seen.radius), (cards.centre, cards.radius));
}

#[test]
fn a_card_list_of_ids_no_list_settles_holds_its_box_alone() {
    let mut list = far();
    for instance in &mut list.instances {
        instance.guid = 3;
    }
    let cards = CardList::of(&list, &index());
    let half = Vec3::new(100.0, 200.0, 50.0).length() / 2.0;
    assert!((cards.radius - half).abs() < 1e-4, "{}", cards.radius);
}
