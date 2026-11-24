use super::super::lights::tests::{
    array, block, entity_at, lit_map, model_of, ptr, put, put_floats,
};
use super::*;

/// A link list of `count` links to a node at (0, 0), as the lane reader reads one.
fn links_model(count: u32) -> RbmModel {
    let mut root = vec![0u8; 0x10];
    put(&mut root, 0, &count.to_le_bytes());
    put(&mut root, 4, &count.to_le_bytes());
    put(&mut root, 8, &ptr(2, 0).to_le_bytes());
    let links = vec![0u8; 0x14 * count as usize];
    model_of(
        std::iter::empty(),
        1,
        vec![block(lanes::LINK_LIST, root), block(0x0F2D_A4C2, links)],
    )
}

fn group_model(entities: u16) -> RbmModel {
    let mut root = vec![0u8; GROUP_SIZE];
    root[0x08] = 5;
    put(&mut root, 0x0C, &0xABCDu32.to_le_bytes());
    let pointer = if entities == 0 { 0 } else { ptr(2, 0) };
    array(&mut root, 0x10, pointer, entities);
    let records = vec![0u8; ENTITY_SIZE as usize * usize::from(entities)];
    model_of(
        LAYOUTS.iter(),
        1,
        vec![block(ENTITY_GROUP, root), block(C_ENTITY_DEF, records)],
    )
}

#[test]
fn a_census_counts_a_body_at_a_time_and_two_censuses_merge() {
    let mut census = Census::default();
    let read = census.add_model(WorldKind::MapData, &lit_map());
    assert!(
        matches!(read, Ok(Counted::Map(MapBody::Data(_)))),
        "{read:?}"
    );
    let read = census.add_model(WorldKind::EntityGroup, &group_model(3));
    let group = EntityGroup {
        name: 0xABCD,
        flags: 5,
        entities: 3,
    };
    assert_eq!(read, Ok(Counted::Group(group)));
    let read = census.add_model(WorldKind::LaneLinks, &links_model(4));
    assert!(
        matches!(read, Ok(Counted::Lanes(LaneBody::Links(_)))),
        "{read:?}"
    );
    let expected = Census {
        maps: 1,
        placement_maps: 1,
        entities: 1,
        light_effects: 1,
        entity_lights: 3,
        entity_groups: 1,
        group_entities: 3,
        lane_links: 4,
        ..Census::default()
    };
    assert_eq!(census, expected);
    assert_eq!(census.lights(), 3);

    let mut whole = Census {
        maps: 10,
        unread: 2,
        ..Census::default()
    };
    whole.merge(&census);
    whole.merge(&census);
    let merged = Census {
        maps: 12,
        placement_maps: 2,
        entities: 2,
        light_effects: 2,
        entity_lights: 6,
        entity_groups: 2,
        group_entities: 6,
        lane_links: 8,
        unread: 2,
        ..Census::default()
    };
    assert_eq!(whole, merged);
}

#[test]
fn a_map_whose_lights_do_not_read_is_still_counted_and_its_lights_are_said_unread() {
    let mut m = lit_map();
    // The first light's direction, no longer a unit vector.
    put_floats(&mut m.data_blocks[6].data, 0x1A0, &[0.0, 0.0, -2.0]);
    let mut census = Census::default();
    assert!(census.add_model(WorldKind::MapData, &m).is_ok());
    let expected = Census {
        maps: 1,
        placement_maps: 1,
        entities: 1,
        lights_unread: 1,
        ..Census::default()
    };
    assert_eq!(census, expected);
}

/// The head of a virtual section whose one block is of type `root`, and nothing a reader reads.
fn head(root: u32) -> Vec<u8> {
    let mut v = vec![0u8; 0x80];
    put(&mut v, 0x20, &1u32.to_le_bytes());
    put(&mut v, 0x38, &0x5000_0060u64.to_le_bytes());
    put(&mut v, 0x4C, &1u16.to_le_bytes());
    put(&mut v, 0x60, &root.to_le_bytes());
    v
}

#[test]
fn a_body_that_is_no_world_data_is_passed_over_and_one_that_does_not_read_is_counted_unread() {
    let mut census = Census::default();
    assert_eq!(census.add_body(&head(0x1234_5678), &[]), None);
    assert_eq!(census, Census::default());
    for root in [C_MAP_DATA, ENTITY_GROUP, lanes::JUNCTION_LIST] {
        assert!(matches!(census.add_body(&head(root), &[]), Some(Err(_))));
    }
    let expected = Census {
        unread: 3,
        ..Census::default()
    };
    assert_eq!(census, expected);
    let kinds = [
        (C_MAP_DATA, WorldKind::MapData),
        (C_MAP_TYPES, WorldKind::MapTypes),
        (ENTITY_GROUP, WorldKind::EntityGroup),
        (lanes::NODE_LIST, WorldKind::LaneNodes),
        (lanes::LINK_LIST, WorldKind::LaneLinks),
        (lanes::JUNCTION_LIST, WorldKind::LaneJunctions),
    ];
    for (root, kind) in kinds {
        assert_eq!(WorldKind::of_root(root), Some(kind));
    }
}

#[test]
fn an_entity_group_whose_entities_are_not_in_place_is_refused() {
    let mut m = group_model(3);
    m.data_blocks[1].data_type = 7;
    let e = read_group(&m).expect_err("pointers, not entities");
    assert!(e.to_string().contains("do not fit a block"), "{e}");
    let mut m = group_model(3);
    m.data_blocks[1].data.truncate(2 * ENTITY_SIZE as usize);
    assert!(read_group(&m).is_err());
    let mut m = group_model(3);
    m.data_blocks[0].data_type = C_MAP_TYPES;
    assert!(matches!(read_group(&m), Err(MapError::NotAMap(_))));
    assert_eq!(
        read_group(&group_model(0))
            .expect("an empty group")
            .entities,
        0
    );
}

fn map_of(name: u32, parent: u32, extents: Aabb, entities: Vec<super::super::Entity>) -> MapData {
    MapData {
        name,
        name_word: u64::from(name),
        parent,
        hash_6c0f9c8c: 0,
        hash_9e8d32a0: 0,
        hash_ec16451d: 0,
        hash_36c898aa: 0,
        flags: 0,
        content_flags: 0,
        streaming_extents: extents,
        entities_extents: extents,
        entities,
        prop_lists: Vec::new(),
        grass_lists: Vec::new(),
    }
}

fn square(lo: f32, hi: f32) -> Aabb {
    Aabb {
        min: [lo, lo, -10.0],
        max: [hi, hi, 10.0],
    }
}

#[test]
fn an_index_holds_the_rows_of_many_maps_and_finds_those_of_an_area() {
    let mut index = PlacementIndex::default();
    let mut child = entity_at([5.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    (child.parent_index, child.lod_dist, child.lod_level) = (4, 120.0, 0);
    let near = vec![
        child.clone(),
        entity_at([90.0, 90.0, 0.0], [0.0, 0.0, 0.0, 1.0]),
    ];
    index.add(0x100, &map_of(0xA, 0xB, square(0.0, 100.0), near));
    index.add(0x200, &map_of(0xC, 0, square(0.0, 100.0), Vec::new()));
    let far = vec![entity_at([1000.0, 1000.0, 0.0], [0.0, 0.0, 0.0, 1.0])];
    index.add(0x300, &map_of(0xD, 0, square(900.0, 1100.0), far));

    assert_eq!(
        index.maps.len(),
        2,
        "a map that places nothing adds nothing"
    );
    assert_eq!(
        (index.maps[0].offset, index.maps[0].rows.clone()),
        (0x100, 0..2)
    );
    assert_eq!(
        (index.maps[1].name, index.maps[1].rows.clone()),
        (0xD, 2..3)
    );
    assert_eq!(index.maps[0].parent, 0xB);
    assert_eq!(index.rows_of(0).len(), 2);
    assert!(index.rows_of(2).is_empty());
    let row = index.rows[0];
    assert_eq!(
        (
            row.archetype_word,
            row.lod_parent,
            row.lod_dist,
            row.lod_level
        ),
        (child.archetype_word, 4, 120.0, 0)
    );

    let found: Vec<usize> = (index.maps_within([-50.0, -50.0], [50.0, 50.0]))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(found, [0]);
    let rows: Vec<[f32; 3]> = (index.rows_within([-50.0, -50.0], [50.0, 50.0]))
        .map(|r| r.position)
        .collect();
    assert_eq!(
        rows,
        [[5.0, 5.0, 0.0]],
        "the row at 90 is in the map and outside the area"
    );
    assert_eq!(index.rows_within([0.0, 0.0], [2000.0, 2000.0]).count(), 3);
}

#[test]
fn a_grid_finds_the_points_of_a_rectangle_by_the_cells_it_meets() {
    let points = [
        [5.0, 5.0],
        [15.0, 5.0],
        [-5.0, -5.0],
        [f32::NAN, 0.0],
        [995.0, 995.0],
        [6.0, 7.0],
    ];
    let grid = Grid::new(10.0, points);
    assert_eq!((grid.cell(), grid.cells()), (10.0, 4));
    let found = |min: [f32; 2], max: [f32; 2]| {
        let mut held: Vec<u32> = (grid.cells_within(min, max).into_iter())
            .flat_map(|(_, held)| held.to_vec())
            .collect();
        held.sort_unstable();
        held
    };
    assert_eq!(found([0.0, 0.0], [9.0, 9.0]), [0, 5]);
    assert_eq!(found([-1.0, -1.0], [11.0, 1.0]), [0, 1, 2, 5]);
    assert_eq!(
        found([-1e9, -1e9], [1e9, 1e9]),
        [0, 1, 2, 4, 5],
        "a rectangle of more cells than are held walks the held ones"
    );
    assert!(found([20.0, 20.0], [30.0, 30.0]).is_empty());
    assert!(found([15.0, 15.0], [0.0, 0.0]).is_empty(), "no rectangle");
    assert!(found([f32::NAN, 0.0], [1.0, 1.0]).is_empty());
    let corners: Vec<[f32; 2]> = (grid.cells_within([-10.0, -10.0], [-1.0, -1.0]).into_iter())
        .map(|(corner, _)| corner)
        .collect();
    assert_eq!(corners, [[-10.0, -10.0]]);
    assert_eq!(
        Grid::new(0.0, [[0.5, 0.5]]).cell(),
        1.0,
        "no cell of no width"
    );
}

#[test]
fn a_body_gives_an_area_its_map_with_its_lights_or_a_lane_list_and_a_group_nothing() {
    let read = area_body(WorldKind::MapData, &lit_map());
    let Some(AreaBody::Map(MapBody::Data(map), Some(lights))) = read else {
        panic!("a map with its lights");
    };
    assert_eq!((map.entities.len(), lights.lights()), (1, 3));
    assert_eq!(index_row_pose(&map), [0.0, 0.0, 0.0]);

    // A map whose lights do not read is still a map.
    let mut m = lit_map();
    put_floats(&mut m.data_blocks[6].data, 0x1A0, &[0.0, 0.0, -2.0]);
    assert!(matches!(
        area_body(WorldKind::MapData, &m),
        Some(AreaBody::Map(MapBody::Data(_), None))
    ));
    assert!(matches!(
        area_body(WorldKind::LaneLinks, &links_model(2)),
        Some(AreaBody::Lanes(LaneBody::Links(l))) if l.len() == 2
    ));
    assert_eq!(area_body(WorldKind::EntityGroup, &group_model(1)), None);
    assert_eq!(area_body(WorldKind::MapTypes, &group_model(1)), None);
    assert_eq!(read_area_body(&head(ENTITY_GROUP), &[]), None);
    assert_eq!(
        read_area_body(&head(C_MAP_DATA), &[]),
        None,
        "it does not read"
    );
    assert_eq!(read_area_body(&head(0x1234_5678), &[]), None);
}

/// Where the row of the one entity of `map` stands, through its pose.
fn index_row_pose(map: &MapData) -> [f32; 3] {
    let mut index = PlacementIndex::default();
    index.add(0, map);
    index.rows[0].pose().apply([0.0; 3])
}

#[test]
fn a_row_places_a_model_s_box_by_its_scale_then_turn_then_position() {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    // Stored as the conjugate: the model turns a quarter about +z, x onto y.
    let mut e = entity_at([10.0, 0.0, 0.0], [0.0, 0.0, -s, s]);
    (e.scale_xy, e.scale_z) = (2.0, 3.0);
    let mut index = PlacementIndex::default();
    index.add(0, &map_of(1, 0, square(0.0, 1.0), vec![e]));
    let corners = index.rows[0].corners([0.0; 3], [1.0; 3]);
    let lo = [0, 1, 2].map(|i| corners.iter().map(|c| c[i]).fold(f32::MAX, f32::min));
    let hi = [0, 1, 2].map(|i| corners.iter().map(|c| c[i]).fold(f32::MIN, f32::max));
    let off = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f32::max);
    assert!(off(lo, [8.0, 0.0, 0.0]) < 1e-5, "{lo:?}");
    assert!(off(hi, [10.0, 2.0, 3.0]) < 1e-5, "{hi:?}");
}

#[test]
fn every_changed_byte_is_read_or_refused_and_never_a_panic() {
    for whole in [links_model(3), group_model(3)] {
        crate::gta6::mutated_tests::every_block_change(&whole, |m| {
            let _ = (
                read_group(m),
                area_body(WorldKind::MapData, m),
                area_body(WorldKind::LaneLinks, m),
            );
        });
    }
}
