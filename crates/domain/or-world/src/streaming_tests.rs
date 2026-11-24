use std::collections::HashMap;

use glam::Vec3;

use super::*;

/// An entity: of the high level where it has a parent, a level-of-detail model where it has none.
fn entity(at: [f32; 3], lod_dist: f32, child_lod_dist: f32, parent: Option<(u64, u32)>) -> Node {
    Node {
        position: Vec3::from(at),
        extent: Extent::Point,
        lod_dist,
        child_lod_dist,
        parent: parent.map(|(map, node)| ParentRef { map, node }),
        instance: None,
        level: match parent {
            Some(_) => Level::Hd,
            None => Level::Lod,
        },
    }
}

fn list(min: [f32; 3], max: [f32; 3], lod_dist: f32, parent: Option<(u64, u32)>) -> Node {
    let (min, max) = (Vec3::from(min), Vec3::from(max));
    Node {
        position: (min + max) / 2.0,
        extent: Extent::Box(min, max),
        lod_dist,
        child_lod_dist: 0.0,
        parent: parent.map(|(map, node)| ParentRef { map, node }),
        instance: None,
        level: Level::Lod,
    }
}

/// Every node ready but those named, which are loading.
fn loading(nodes: &[NodeId]) -> impl Fn(NodeId) -> NodeState + '_ {
    move |id| match nodes.contains(&id) {
        true => NodeState::Loading,
        false => NodeState::Ready,
    }
}

fn all_ready(_: NodeId) -> NodeState {
    NodeState::Ready
}

/// A camera at rest `x` metres along the first axis.
fn along(x: f32) -> View {
    View::at(Vec3::new(x, 0.0, 0.0), 1.0)
}

/// The entities a selection draws, in order.
fn ids(selection: &Selection) -> Vec<NodeId> {
    let mut ids: Vec<NodeId> = selection.drawn.iter().map(|d| d.node).collect();
    ids.sort_unstable();
    ids
}

/// How opaque a selection draws the entity or the list `id`: nought where it does not.
fn fade(selection: &Selection, id: NodeId) -> f32 {
    let entity = selection.drawn.iter().find(|d| d.node == id);
    let list = selection.lists.iter().find(|l| l.node == id);
    entity.map(|d| d.fade).or(list.map(|l| l.fade)).unwrap_or(0.0)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// A parent at the origin drawn to 1,000 m whose two children, 300 m apart, draw within 100 m of it.
fn parent_and_two_children() -> Forest {
    let mut forest = Forest::new();
    forest.insert(1, vec![entity([0.0; 3], 1000.0, 100.0, None)]);
    forest.insert(
        2,
        vec![
            entity([150.0, 0.0, 0.0], 40.0, 0.0, Some((1, 0))),
            entity([-150.0, 0.0, 0.0], 40.0, 0.0, Some((1, 0))),
        ],
    );
    forest
}

#[test]
fn the_children_of_one_parent_swap_together_by_the_distance_to_the_parent() {
    let mut forest = parent_and_two_children();
    // Beside the first child and 240 m from the parent: neither child draws, the parent does.
    let far = forest.select(&along(240.0), &all_ready);
    assert_eq!(ids(&far), vec![(1, 0)]);
    // 70 m from the parent and 220 m from the second child: both children draw, the parent does not.
    let near = forest.select(&along(70.0), &all_ready);
    assert_eq!(ids(&near), vec![(2, 0), (2, 1)]);
}

#[test]
fn a_parent_and_its_children_cross_fade_over_a_band_either_side_of_their_distance() {
    let mut forest = parent_and_two_children();
    // What draws at each distance from the parent: the parent, then each child.
    for (away, parent, child) in [
        (130.0, 1.0, 0.0),
        (110.0, 1.0, 0.5),
        (100.0, 1.0, 1.0),
        (90.0, 0.5, 1.0),
        (80.0, 0.0, 1.0),
    ] {
        let drawn = forest.select(&along(away), &all_ready);
        assert!(close(fade(&drawn, (1, 0)), parent), "the parent at {away}");
        assert!(close(fade(&drawn, (2, 0)), child), "a child at {away}");
        assert!(close(fade(&drawn, (2, 1)), child), "its sibling at {away}");
        // What has faded out is not listed.
        assert!(drawn.drawn.iter().all(|d| d.fade > 0.0));
        assert!(!drawn.fading);
    }
}

#[test]
fn a_parent_draws_until_every_child_is_ready_and_the_child_is_waited_for() {
    let mut forest = parent_and_two_children();
    let waiting = [(2, 1)];
    let held = forest.select(&along(90.0), &loading(&waiting));
    assert_eq!(ids(&held), vec![(1, 0)]);
    assert!(close(fade(&held, (1, 0)), 1.0));
    assert_eq!(held.wanted, vec![(2, 1)]);
    // In the band before their distance the children are waited for already.
    let band = forest.select(&along(115.0), &loading(&waiting));
    assert_eq!(band.wanted, vec![(2, 1)]);
    // Out of it nothing of theirs is waited for.
    let far = forest.select(&along(500.0), &loading(&waiting));
    assert_eq!((ids(&far), far.wanted), (vec![(1, 0)], vec![]));
}

#[test]
fn a_parent_draws_while_a_child_map_of_its_map_is_not_loaded() {
    let mut forest = parent_and_two_children();
    forest.set_complete(1, false);
    let held = forest.select(&along(70.0), &all_ready);
    assert_eq!(ids(&held), vec![(1, 0)]);
    forest.set_complete(1, true);
    let swapped = forest.select(&along(70.0), &all_ready);
    assert_eq!(ids(&swapped), vec![(2, 0), (2, 1)]);
}

#[test]
fn a_child_takes_the_distance_its_parent_states_for_its_children() {
    // The child states 40 m and its parent 100 m for its children: at 90 m from the parent it draws.
    let mut forest = parent_and_two_children();
    let at = |d: f32| View::at(Vec3::new(0.0, d, 0.0), 1.0);
    let drawn = forest.select(&at(90.0), &all_ready);
    assert!(close(fade(&drawn, (2, 0)), 1.0) && close(fade(&drawn, (2, 1)), 1.0));
    // A parent that states none takes the farthest of its children's.
    let mut unstated = Forest::new();
    unstated.insert(1, vec![entity([0.0; 3], 1000.0, 0.0, None)]);
    unstated.insert(
        2,
        vec![
            entity([1.0, 0.0, 0.0], 40.0, 0.0, Some((1, 0))),
            entity([2.0, 0.0, 0.0], 70.0, 0.0, Some((1, 0))),
        ],
    );
    assert_eq!(ids(&unstated.select(&at(95.0), &all_ready)), vec![(1, 0)]);
    // Within 70 m the parent gives way, and the child of 40 m is itself out of its distance.
    let part = unstated.select(&at(60.0), &all_ready);
    assert_eq!(ids(&part), vec![(1, 0), (2, 1)]);
    assert!(close(fade(&part, (1, 0)), 0.5) && close(fade(&part, (2, 1)), 1.0));
    assert_eq!(
        ids(&unstated.select(&at(30.0), &all_ready)),
        vec![(2, 0), (2, 1)]
    );
}

#[test]
fn a_node_with_no_parent_draws_by_its_own_place_and_fades_out_past_its_distance() {
    let mut forest = Forest::new();
    forest.insert(7, vec![entity([10.0, 0.0, 0.0], 50.0, 0.0, None)]);
    assert!(close(
        fade(&forest.select(&along(59.0), &all_ready), (7, 0)),
        1.0
    ));
    assert!(close(
        fade(&forest.select(&along(70.0), &all_ready), (7, 0)),
        0.5
    ));
    assert!(forest.select(&along(81.0), &all_ready).drawn.is_empty());
    // The scale stretches the distance and leaves the band as it is.
    let scaled = |x: f32, scale: f32| View::at(Vec3::new(x, 0.0, 0.0), scale);
    assert!(close(
        fade(&forest.select(&scaled(100.0, 2.0), &all_ready), (7, 0)),
        1.0
    ));
    assert!(forest
        .select(&scaled(59.0, 0.5), &all_ready)
        .drawn
        .is_empty());
}

#[test]
fn each_level_takes_its_own_scale() {
    let mut forest = parent_and_two_children();
    // At 150 m the children, which draw within 100 m, are out of their distance.
    assert_eq!(ids(&forest.select(&along(150.0), &all_ready)), vec![(1, 0)]);
    // Twice the distance for the high level alone brings them in and takes the parent out.
    let mut view = along(150.0);
    view.scales[Level::Hd as usize] = 2.0;
    assert_eq!(
        ids(&forest.select(&view, &all_ready)),
        vec![(2, 0), (2, 1)]
    );
    // A scale of another level leaves them where they were.
    let mut view = along(150.0);
    view.scales[Level::Slod1 as usize] = 2.0;
    assert_eq!(ids(&forest.select(&view, &all_ready)), vec![(1, 0)]);
    // And the parent's own level stretches the parent's own distance.
    let mut view = along(1500.0);
    assert!(forest.select(&view, &all_ready).drawn.is_empty());
    view.scales[Level::Lod as usize] = 2.0;
    assert_eq!(ids(&forest.select(&view, &all_ready)), vec![(1, 0)]);
}

#[test]
fn an_orphan_that_draws_no_farther_than_the_band_fades_over_a_short_one() {
    let orphan = |lod_dist: f32, level: Level| {
        let mut forest = Forest::new();
        let mut node = entity([0.0; 3], lod_dist, 0.0, None);
        node.level = level;
        forest.insert(1, vec![node]);
        forest
    };
    let mut small = orphan(15.0, Level::OrphanHd);
    assert!(close(
        fade(&small.select(&along(17.0), &all_ready), (1, 0)),
        0.6
    ));
    assert!(small.select(&along(21.0), &all_ready).drawn.is_empty());
    // One of another level with the same distance takes the whole band.
    let mut other = orphan(15.0, Level::Lod);
    assert!(close(
        fade(&other.select(&along(21.0), &all_ready), (1, 0)),
        0.7
    ));
    // And so does an orphan that draws farther than the band.
    let mut large = orphan(50.0, Level::OrphanHd);
    assert!(close(
        fade(&large.select(&along(60.0), &all_ready), (1, 0)),
        0.5
    ));
    // The level a map's number names: one past the last is an orphan.
    assert_eq!(Level::from_raw(5), Level::OrphanHd);
    assert_eq!(Level::from_raw(6), Level::Slod4);
    assert_eq!(Level::from_raw(9), Level::OrphanHd);
    assert_eq!(Level::from_raw(0), Level::Hd);
}

/// A forest whose parent drew alone a selection ago, its children loading, 70 m from it.
fn children_about_to_arrive() -> Forest {
    let mut forest = parent_and_two_children();
    let waiting = [(2, 0), (2, 1)];
    let held = forest.select(&along(70.0), &loading(&waiting));
    assert_eq!(ids(&held), vec![(1, 0)]);
    forest
}

#[test]
fn what_comes_ready_in_view_fades_in_by_time_and_holds_its_parent_whole() {
    let mut forest = children_about_to_arrive();
    let frame = View {
        dt: 0.033,
        ..along(70.0)
    };
    let step = 32.0 / 255.0;
    for k in 1..=7 {
        let drawn = forest.select(&frame, &all_ready);
        assert!(close(fade(&drawn, (2, 0)), step * k as f32), "step {k}");
        assert!(close(fade(&drawn, (2, 1)), step * k as f32), "step {k}");
        // 70 m from it the parent has given way, and is held whole until its children are.
        assert!(close(fade(&drawn, (1, 0)), 1.0), "step {k}");
        assert!(drawn.fading);
    }
    let whole = forest.select(&frame, &all_ready);
    assert_eq!(ids(&whole), vec![(2, 0), (2, 1)]);
    assert!(close(fade(&whole, (2, 0)), 1.0) && !whole.fading);
}

#[test]
fn a_fade_in_time_starts_by_one_step_however_long_ago_the_last_selection_was() {
    let mut forest = children_about_to_arrive();
    let late = View {
        dt: 10.0,
        ..along(70.0)
    };
    let drawn = forest.select(&late, &all_ready);
    assert!(close(fade(&drawn, (2, 0)), 32.0 / 255.0) && drawn.fading);
}

#[test]
fn nothing_fades_in_time_for_a_camera_at_rest_in_time_or_faster_than_the_limit() {
    // No time has passed: what is ready is there at once.
    let mut forest = children_about_to_arrive();
    let at_once = forest.select(&along(70.0), &all_ready);
    assert_eq!(ids(&at_once), vec![(2, 0), (2, 1)]);
    assert!(close(fade(&at_once, (2, 0)), 1.0) && !at_once.fading);
    // Faster than the limit a model pops in.
    let mut forest = children_about_to_arrive();
    let fast = View {
        dt: 0.033,
        velocity: Vec3::new(FADE_MAX_SPEED + 1.0, 0.0, 0.0),
        ..along(70.0)
    };
    let popped = forest.select(&fast, &all_ready);
    assert_eq!(ids(&popped), vec![(2, 0), (2, 1)]);
    assert!(close(fade(&popped, (2, 0)), 1.0) && !popped.fading);
}

#[test]
fn a_grandparent_draws_until_the_whole_of_what_replaces_it_is_ready() {
    let mut forest = Forest::new();
    forest.insert(1, vec![entity([0.0; 3], 5000.0, 1000.0, None)]);
    forest.insert(
        2,
        vec![
            entity([0.0; 3], 0.0, 100.0, Some((1, 0))),
            entity([400.0, 0.0, 0.0], 0.0, 100.0, Some((1, 0))),
        ],
    );
    forest.insert(3, vec![entity([0.0; 3], 0.0, 0.0, Some((2, 0)))]);
    let view = View::at(Vec3::new(0.0, 50.0, 0.0), 1.0);
    // The first parent gives way to its child, the second draws itself: three levels at once.
    assert_eq!(
        ids(&forest.select(&view, &all_ready)),
        vec![(2, 1), (3, 0)]
    );
    // With the grandchild loading its parent draws, which is ready, so the grandparent still gives way.
    let waiting = [(3, 0)];
    assert_eq!(
        ids(&forest.select(&view, &loading(&waiting))),
        vec![(2, 0), (2, 1)]
    );
    // With that parent loading as well nothing can take the grandparent's place.
    let waiting = [(3, 0), (2, 0)];
    let held = forest.select(&view, &loading(&waiting));
    assert_eq!(ids(&held), vec![(1, 0)]);
    assert!(held.wanted.contains(&(2, 0)) && held.wanted.contains(&(3, 0)));
}

#[test]
fn a_child_whose_parent_map_is_not_loaded_draws_alone_until_it_is() {
    let mut forest = Forest::new();
    forest.insert(2, vec![entity([0.0; 3], 40.0, 0.0, Some((1, 0)))]);
    let view = View::at(Vec3::new(0.0, 30.0, 0.0), 1.0);
    assert_eq!(ids(&forest.select(&view, &all_ready)), vec![(2, 0)]);
    forest.insert(1, vec![entity([0.0, 500.0, 0.0], 2000.0, 100.0, None)]);
    // The parent stands 470 m away: its child, now judged by that, does not draw.
    assert_eq!(ids(&forest.select(&view, &all_ready)), vec![(1, 0)]);
    forest.remove(1);
    assert_eq!(ids(&forest.select(&view, &all_ready)), vec![(2, 0)]);
    assert_eq!(forest.len(), (1, 1));
}

#[test]
fn a_list_draws_by_its_box_less_each_instance_whose_entity_draws() {
    let mut forest = Forest::new();
    forest.insert(1, vec![list([0.0; 3], [100.0, 100.0, 10.0], 300.0, None)]);
    let mut stand_in = entity([10.0, 10.0, 0.0], 50.0, 0.0, Some((1, 0)));
    stand_in.instance = Some(4);
    let mut other = entity([90.0, 90.0, 0.0], 50.0, 0.0, Some((1, 0)));
    other.instance = Some(9);
    forest.insert(2, vec![stand_in, other]);
    let at = |x: f32, y: f32| View::at(Vec3::new(x, y, 0.0), 1.0);
    // Inside the box, beside the first entity: it draws and its instance is left out.
    let near = forest.select(&at(12.0, 12.0), &all_ready);
    assert_eq!(ids(&near), vec![(2, 0)]);
    assert_eq!(near.lists.len(), 1);
    assert_eq!(&near.skipped[near.lists[0].skipped.clone()], &[4]);
    // 250 m off the box's side the list draws whole, 310 m off it by half, 350 m off it not at all.
    let off = forest.select(&at(350.0, 50.0), &all_ready);
    assert_eq!(
        (off.drawn.len(), off.lists.len(), off.skipped.len()),
        (0, 1, 0)
    );
    assert!(close(fade(&off, (1, 0)), 1.0));
    let fading = forest.select(&at(410.0, 50.0), &all_ready);
    assert!(close(fade(&fading, (1, 0)), 0.5));
    let gone = forest.select(&at(450.0, 50.0), &all_ready);
    assert!(gone.lists.is_empty());
    // An entity still loading leaves its instance drawn and is waited for.
    let waiting = [(2, 0)];
    let held = forest.select(&at(12.0, 12.0), &loading(&waiting));
    assert_eq!((held.drawn.len(), held.skipped.len()), (0, 0));
    assert_eq!(held.wanted, vec![(2, 0)]);
}

#[test]
fn a_list_gives_way_to_the_lists_under_it() {
    let mut forest = Forest::new();
    let mut far = list([0.0; 3], [100.0, 100.0, 10.0], 2000.0, None);
    far.child_lod_dist = 200.0;
    forest.insert(1, vec![far]);
    forest.insert(
        2,
        vec![list([0.0; 3], [50.0, 50.0, 10.0], 0.0, Some((1, 0)))],
    );
    let at = |x: f32| View::at(Vec3::new(x, 50.0, 0.0), 1.0);
    let lists = |s: &Selection| s.lists.iter().map(|l| l.node).collect::<Vec<_>>();
    assert_eq!(lists(&forest.select(&at(150.0), &all_ready)), vec![(2, 0)]);
    assert_eq!(lists(&forest.select(&at(900.0), &all_ready)), vec![(1, 0)]);
    // 210 m off the upper list's box both draw: the lower by half, the upper whole.
    let both = forest.select(&at(310.0), &all_ready);
    assert!(close(fade(&both, (2, 0)), 0.5) && close(fade(&both, (1, 0)), 1.0));
}

#[test]
fn a_node_with_no_model_draws_nothing_and_holds_nothing_back() {
    let mut forest = parent_and_two_children();
    let absent: HashMap<NodeId, NodeState> = HashMap::from([((2, 1), NodeState::Absent)]);
    let state = |id: NodeId| *absent.get(&id).unwrap_or(&NodeState::Ready);
    let near = forest.select(&along(70.0), &state);
    assert_eq!((ids(&near), near.wanted), (vec![(2, 0)], vec![]));
}

#[test]
fn a_map_streams_in_within_fifty_metres_of_the_box_it_streams_by() {
    let streamed = MapBox {
        streaming: (Vec3::splat(-100.0), Vec3::splat(100.0)),
        entities: (Vec3::splat(-10.0), Vec3::splat(10.0)),
    };
    assert!(streamed.wants(Vec3::new(149.0, 0.0, 0.0)));
    assert!(!streamed.wants(Vec3::new(151.0, 0.0, 0.0)));
    assert!(!streamed.wants(Vec3::new(0.0, 0.0, 151.0)));
    // A camera at rest wants a map of any tier by the same test.
    for flags in [0x1, 0x401, 0x2, 0x4] {
        assert!(streamed.wanted(&along(149.0), flags));
        assert!(!streamed.wanted(&along(151.0), flags));
    }
    // A streaming box that does not hold the entities' box is not the one streamed by.
    let narrow = MapBox {
        streaming: (Vec3::splat(-1.0), Vec3::splat(1.0)),
        entities: (Vec3::splat(-200.0), Vec3::splat(200.0)),
    };
    assert!(narrow.wants(Vec3::new(240.0, 0.0, 0.0)));
    assert!(!narrow.wants(Vec3::new(260.0, 0.0, 0.0)));
}

#[test]
fn a_map_of_the_high_tier_is_not_asked_for_at_speed() {
    assert_eq!(Tier::of(0x1), Tier::High);
    assert_eq!(Tier::of(0x401), Tier::HighCritical);
    assert_eq!(Tier::of(0x201), Tier::HighCritical);
    assert_eq!(Tier::of(0x403), Tier::Medium);
    assert_eq!(Tier::of(0x4), Tier::Low);
    assert_eq!(Tier::of(0x80), Tier::Low);
    let map = MapBox {
        streaming: (Vec3::splat(-100.0), Vec3::splat(100.0)),
        entities: (Vec3::splat(-10.0), Vec3::splat(10.0)),
    };
    let moving = |speed: f32| View {
        velocity: Vec3::new(0.0, speed, 0.0),
        ..along(0.0)
    };
    assert!(map.wanted(&moving(HD_MAX_SPEED - 1.0), 0x1));
    assert!(!map.wanted(&moving(HD_MAX_SPEED), 0x1));
    // The other tiers stream in at any speed.
    for flags in [0x401, 0x2, 0x4] {
        assert!(map.wanted(&moving(300.0), flags));
    }
}
