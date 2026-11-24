use or_io::gta6::map::interior::{InteriorChild, InteriorSet};
use or_io::gta6::map::links::Room;

use super::super::index::tests_support::index_of;
use super::*;

/// A chair that draws to 20 m and a lamp that draws to 50, each with a model, and a ghost with none.
const CHAIR: u64 = 100;
const LAMP: u64 = 200;
const GHOST: u64 = 300;
const SHELL: u64 = 900;
const KITCHEN: EntitySetLocation = EntitySetLocation::Room(1);

fn index() -> WorldIndex {
    index_of(
        Vec::new(),
        &[
            (CHAIR, 20.0, Some(0x1000)),
            (LAMP, 50.0, Some(0x2000)),
            (GHOST, 20.0, None),
        ],
    )
}

fn child(word: u64, at: [f32; 3], lod_dist: f32) -> InteriorChild {
    InteriorChild {
        archetype_word: word,
        archetype_name: word as u32,
        position: at,
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale_xy: 1.0,
        scale_z: 1.0,
        lod_dist,
        flags: 0,
    }
}

fn room(attached_objects: Vec<u32>) -> InteriorRoom {
    InteriorRoom {
        room: Room {
            name: 1,
            bb_min: [-4.0, -3.0, 0.0],
            bb_max: [4.0, 3.0, 0.0],
            flags: 0,
            portal_count: 0,
            floor_id: 0,
            attached_objects,
        },
        blend: 0.0,
        timecycle_name: 0,
        secondary_timecycle_name: 0,
    }
}

/// An interior 8 m by 6 that draws to 30 m: a chair, a ghost and an entity of no archetype in its
/// second room, and a lamp in each of two entity sets.
fn interior() -> Interior {
    let lamp = |name, at| InteriorSet {
        name,
        locations: vec![KITCHEN],
        entities: vec![child(LAMP, at, 5.0)],
    };
    Interior {
        archetype: 3,
        name: 9,
        name_word: SHELL,
        lod_dist: 30.0,
        bb_min: [-4.0, -3.0, 0.0],
        bb_max: [4.0, 3.0, 0.0],
        mlo_flags: 0,
        entities: vec![
            child(CHAIR, [1.0, 0.0, 0.0], -1.0),
            child(GHOST, [0.0; 3], -1.0),
            child(0xDEAD, [0.0; 3], -1.0),
        ],
        rooms: vec![room(vec![]), room(vec![0, 1, 2])],
        portals: vec![],
        entity_sets: vec![lamp(0xA, [0.0, 2.0, 0.0]), lamp(0xB, [0.0, -2.0, 0.0])],
        time_cycle_modifiers: vec![],
    }
}

/// An instance at (10, 20, 30), turned a quarter about z, that turns the entity set 0xA on.
fn instance() -> InteriorInstance {
    let half = std::f32::consts::FRAC_1_SQRT_2;
    InteriorInstance {
        entity: 0,
        archetype_word: SHELL,
        archetype_name: 9,
        position: [10.0, 20.0, 30.0],
        rotation: [0.0, 0.0, half, half],
        scale_xy: 1.0,
        scale_z: 1.0,
        lod_dist: -1.0,
        group_id: 0,
        floor_id: 0,
        default_entity_sets: vec![0xA],
        num_exit_portals: 0,
        flags: 2,
    }
}

fn origin(rows: &Rows) -> Vec3 {
    Vec3::new(rows[0][3], rows[1][3], rows[2][3])
}

fn near(found: Vec3, wanted: [f32; 3]) -> bool {
    found.distance(Vec3::from(wanted)) < 1e-4
}

fn model(offset: u64) -> ModelRef {
    ModelRef {
        archive: 0,
        offset,
        entry: None,
    }
}

#[test]
fn an_interior_s_entities_stand_in_the_world_under_the_instance_that_places_it() {
    let placed = place(&index(), &instance(), &interior());
    // The ghost has no model, 0xDEAD no archetype, and the set 0xB is off.
    let [chair, lamp] = placed.children.as_slice() else {
        panic!("{} children", placed.children.len());
    };
    assert_eq!((chair.model, lamp.model), (model(0x1000), model(0x2000)));
    // The quarter turn takes the interior's x to the world's y, and its y to the world's −x.
    assert!(near(origin(&chair.rows), [10.0, 21.0, 30.0]));
    assert!(near(origin(&lamp.rows), [8.0, 20.0, 30.0]));
    assert!(near(
        Vec3::new(chair.rows[0][0], chair.rows[1][0], chair.rows[2][0]),
        [0.0, 1.0, 0.0],
    ));
    assert!(near(chair.centre, [10.0, 21.0, 30.0]) && (chair.radius - 1.0).abs() < 1e-4);
    // The chair states no distance and takes its archetype's; the lamp states its own.
    assert_eq!((chair.lod_dist, lamp.lod_dist), (20.0, 5.0));
    assert_eq!((chair.location, lamp.location), (KITCHEN, KITCHEN));
    assert!(near(placed.centre, [10.0, 20.0, 30.0]));
    assert!((placed.radius - 5.0).abs() < 1e-4);
    assert_eq!(placed.lod_dist, 30.0);
    assert!(near(origin(&placed.rows), [10.0, 20.0, 30.0]));
    assert_eq!((placed.rooms.len(), placed.portals.len()), (2, 0));
}

#[test]
fn an_instance_that_states_a_distance_draws_its_interior_to_it() {
    let far = InteriorInstance {
        lod_dist: 80.0,
        ..instance()
    };
    assert_eq!(place(&index(), &far, &interior()).lod_dist, 80.0);
}

/// The models `placed` draws for a camera `north` metres along y from its middle.
fn drawn(placed: &Placed, north: f32, scale: f32) -> Vec<ModelRef> {
    let mut models = Vec::new();
    let eye = placed.centre + Vec3::new(0.0, north, 0.0);
    placed.each(eye, scale, |child, away| {
        assert!((away - eye.distance(origin(&child.rows))).abs() < 1e-4);
        models.push(child.model);
    });
    models
}

#[test]
fn the_contents_draw_within_the_interior_s_distance_and_each_within_its_own() {
    let placed = place(&index(), &instance(), &interior());
    let (chair, lamp) = (model(0x1000), model(0x2000));
    assert_eq!(drawn(&placed, 0.0, 1.0), [chair, lamp]);
    // 10 m out the lamp is past its 5 m.
    assert_eq!(drawn(&placed, 10.0, 1.0), [chair]);
    // 22 m out the chair, a metre north of the middle, is past its 20 m.
    assert!(drawn(&placed, 22.0, 1.0).is_empty());
    // Past the interior's 30 m nothing of it draws, whatever a child's own distance.
    let mut long = placed.clone();
    long.children[0].lod_dist = 500.0;
    assert_eq!(drawn(&long, 29.0, 1.0), [chair]);
    assert!(drawn(&long, 31.0, 1.0).is_empty());
    // Every distance scales.
    assert_eq!(drawn(&placed, 31.0, 2.0), [chair]);
}

#[test]
fn a_camera_inside_the_interior_s_sphere_is_in_range_of_a_shorter_distance() {
    let mut placed = place(&index(), &instance(), &interior());
    placed.lod_dist = 2.0;
    assert_eq!(drawn(&placed, 4.0, 1.0), [model(0x1000), model(0x2000)]);
    assert!(drawn(&placed, 6.0, 1.0).is_empty());
}
