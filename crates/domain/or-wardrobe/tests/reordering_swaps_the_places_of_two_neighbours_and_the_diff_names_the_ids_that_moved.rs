//! Moving a garment earlier or later in its slot swaps its place with its neighbour's, and the build's diff says which ids
//! moved: the name the game sees changes, the stable id does not.

mod common;

use std::path::Path;

use common::{garment, JBIB, LOWR};
use or_project::{diff, Item};
use or_wardrobe::edit::{reorder, Toward};
use or_wardrobe::{allocate, inspect, plan, Depth, Ped, Wardrobe};

fn name(n: u32) -> String {
    format!("mp_m_freemode_01_mp_m_demo^jbib_{n:03}")
}

/// Three male jackets with models of their own, at the pins given.
fn pack(folder: &Path, pins: [Option<u32>; 3]) -> Wardrobe {
    let mut w = Wardrobe::new("demo");
    for (i, pin) in pins.into_iter().enumerate() {
        std::fs::write(folder.join(format!("m{i}.ydd")), format!("model {i}")).unwrap();
        let mut g = garment(Ped::Male, JBIB, pin, &format!("m{i}.ydd"), &[]);
        g.label = Some(format!("g{i}"));
        w.garments.push(g);
    }
    w
}

fn items(folder: &Path, w: &Wardrobe) -> Vec<Item> {
    let facts = inspect(folder, w, Depth::Digest);
    plan(w, &facts, "pack", folder).contribution.items
}

fn places(w: &Wardrobe) -> Vec<(String, u32)> {
    let mut found: Vec<(String, u32)> = allocate(w)
        .placed
        .iter()
        .map(|p| (w.garments[p.garment].label.clone().unwrap(), p.position))
        .collect();
    found.sort_by_key(|(_, position)| *position);
    found
}

fn moved(before: &[Item], after: &[Item]) -> Vec<(String, String, String)> {
    let mut found: Vec<_> = diff(before, after)
        .moved
        .into_iter()
        .map(|m| (m.id, m.from, m.to))
        .collect();
    found.sort();
    found
}

fn ids(w: &Wardrobe) -> Vec<String> {
    w.garments.iter().map(|g| g.id.clone()).collect()
}

#[test]
fn two_garments_that_are_not_pinned_swap_their_places_and_stay_free() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path(), [None, None, None]);
    let (id, before) = (ids(&w), items(dir.path(), &w));
    assert_eq!(
        places(&w),
        [("g0".into(), 0), ("g1".into(), 1), ("g2".into(), 2)]
    );

    reorder(&mut w, &id[2], Toward::Earlier).unwrap();

    assert_eq!(
        places(&w),
        [("g0".into(), 0), ("g2".into(), 1), ("g1".into(), 2)]
    );
    assert!(
        w.garments.iter().all(|g| g.position.is_none()),
        "nobody was pinned by it"
    );
    let mut expected = vec![
        (id[1].clone(), name(1), name(2)),
        (id[2].clone(), name(2), name(1)),
    ];
    expected.sort();
    assert_eq!(
        moved(&before, &items(dir.path(), &w)),
        expected,
        "the diff reports the two ids that moved, by id"
    );
    assert_eq!(ids(&w).len(), 3, "and no garment got a new id");
}

#[test]
fn pinned_garments_swap_pins_so_each_is_where_it_was_asked_to_be() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path(), [Some(0), Some(1), Some(2)]);
    let (id, before) = (ids(&w), items(dir.path(), &w));

    reorder(&mut w, &id[0], Toward::Later).unwrap();

    assert_eq!(
        places(&w),
        [("g1".into(), 0), ("g0".into(), 1), ("g2".into(), 2)]
    );
    assert_eq!(
        (w.garments[0].position, w.garments[1].position),
        (Some(1), Some(0))
    );
    let mut expected = vec![
        (id[0].clone(), name(0), name(1)),
        (id[1].clone(), name(1), name(0)),
    ];
    expected.sort();
    assert_eq!(moved(&before, &items(dir.path(), &w)), expected);
}

#[test]
fn a_pin_among_free_garments_pins_both_of_the_pair_and_leaves_the_others_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path(), [Some(5), None, None]);
    let id = ids(&w);
    assert_eq!(
        places(&w),
        [("g1".into(), 0), ("g2".into(), 1), ("g0".into(), 5)]
    );

    // g0 is pinned at 5, and the one before it by place is g2.
    reorder(&mut w, &id[0], Toward::Earlier).unwrap();

    assert_eq!(
        places(&w),
        [("g1".into(), 0), ("g0".into(), 1), ("g2".into(), 5)]
    );
    assert_eq!(w.garments[1].position, None, "g1 was not part of the swap");
    assert!(allocate(&w).refused.is_empty());
}

#[test]
fn the_ends_of_a_slot_a_garment_with_no_place_and_a_missing_id_are_refused_and_change_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path(), [None, None, None]);
    let id = ids(&w);
    let mut other = garment(Ped::Male, LOWR, None, "m0.ydd", &[]);
    other.label = Some("trousers".into());
    let mut unplaced = garment(Ped::Male, JBIB, None, "m1.ydd", &[]);
    unplaced.slot = None;
    let (other_id, unplaced_id) = (other.id.clone(), unplaced.id.clone());
    w.garments.push(other);
    w.garments.push(unplaced);
    let held = w.clone();

    for (who, toward, why) in [
        (&id[0], Toward::Earlier, "first"),
        (&id[2], Toward::Later, "last"),
        // Trousers are alone in their slot: a jacket is no neighbour of theirs.
        (&other_id, Toward::Later, "last"),
        (&unplaced_id, Toward::Earlier, "no place"),
        (&"nobody".to_string(), Toward::Earlier, "nobody"),
    ] {
        let refused = reorder(&mut w, who, toward).unwrap_err().to_string();
        assert!(refused.contains(why), "{refused}");
    }
    assert_eq!(w, held);
}

#[test]
fn a_garment_of_another_ped_is_no_neighbour() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path(), [None, None, None]);
    w.garments[1].ped = Some(Ped::Female);
    let id = ids(&w);
    reorder(&mut w, &id[2], Toward::Earlier).unwrap();
    assert_eq!(
        w.garments
            .iter()
            .map(|g| g.label.clone().unwrap())
            .collect::<Vec<_>>(),
        ["g2", "g1", "g0"],
        "g2 swapped with g0, the male jacket before it, and g1 is the female ped's"
    );
}
