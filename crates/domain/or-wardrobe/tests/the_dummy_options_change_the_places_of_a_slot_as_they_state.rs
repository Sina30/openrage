//! A dummy is a garment that holds a place and ships no model. A garment added may take one's place, the holes of a
//! slot may be filled with them, a slot may be sorted so that they come last, and a garment taken out may leave one.

mod common;

use std::path::Path;

use common::{garment, ydd, ytd, JBIB};
use or_wardrobe::dummies;
use or_wardrobe::edit::{add_clothes, undo};
use or_wardrobe::{allocate, Garment, Ped, Wardrobe};

const C: &str = "mp_m_freemode_01_mp_m_x";

fn jacket(label: &str, pin: Option<u32>) -> Garment {
    let mut g = garment(Ped::Male, JBIB, pin, &format!("{label}.ydd"), &["t.ytd"]);
    g.label = Some(label.to_string());
    g
}

fn dummy_at(pin: u32) -> Garment {
    let mut g = Garment::new();
    g.ped = Some(Ped::Male);
    g.slot = Some(JBIB);
    g.position = Some(pin);
    g
}

/// What each garment of `w` is called and where the allocator puts it, by place.
fn places(w: &Wardrobe) -> Vec<(String, u32)> {
    allocate(w)
        .placed
        .iter()
        .map(|p| {
            let g = &w.garments[p.garment];
            let name = g.label.clone().unwrap_or_else(|| "-".to_string());
            (name, p.place)
        })
        .collect()
}

fn named(rows: &[(&str, u32)]) -> Vec<(String, u32)> {
    rows.iter().map(|(n, p)| (n.to_string(), *p)).collect()
}

#[test]
fn a_garment_taken_out_leaves_a_dummy_when_asked_and_the_garments_after_it_keep_their_places() {
    let mut w = Wardrobe::new("x");
    w.garments = vec![jacket("a", None), jacket("b", None), jacket("c", None)];
    let id = w.garments[1].id.clone();

    let mut plain = w.clone();
    assert_eq!(
        dummies::take_out(&mut plain, &id, false).unwrap().left,
        None
    );
    assert_eq!(places(&plain), named(&[("a", 0), ("c", 1)]), "c moves up");

    let held = dummies::take_out(&mut w, &id, true)
        .unwrap()
        .left
        .expect("a dummy was to be left");
    assert_eq!(w.garments.len(), 3);
    assert_eq!(
        w.garments[1].id, held,
        "the dummy stands where the garment stood in the list"
    );
    assert!(w.garments[1].is_reserved());
    assert_eq!(
        places(&w),
        named(&[("a", 0), ("Dummy", 1), ("c", 2)]),
        "c keeps its place"
    );

    // A dummy taken out leaves no dummy, and a garment with no place has none to hold.
    assert_eq!(dummies::take_out(&mut w, &held, true).unwrap().left, None);
    let mut nobody = Wardrobe::new("x");
    nobody.garments.push(Garment::new());
    let id = nobody.garments[0].id.clone();
    assert_eq!(
        dummies::take_out(&mut nobody, &id, true).unwrap().left,
        None
    );
    assert!(nobody.garments.is_empty());
}

#[test]
fn a_garment_added_takes_the_first_dummy_of_its_slot_or_the_place_a_dummy_holds() {
    let mut w = Wardrobe::new("x");
    w.garments = vec![
        jacket("a", None),
        dummy_at(1),
        jacket("c", None),
        dummy_at(3),
    ];
    // Two new garments, none pinned: they take the dummies in the order of their places.
    w.garments.extend([jacket("n1", None), jacket("n2", None)]);
    let ids: Vec<String> = w.garments[4..].iter().map(|g| g.id.clone()).collect();
    let taken = dummies::replace_on_add(&mut w, &ids);
    assert_eq!(taken.len(), 2);
    assert_eq!(
        places(&w),
        named(&[("a", 0), ("n1", 1), ("c", 2), ("n2", 3)])
    );
    assert!(w.garments.iter().all(|g| !g.is_reserved()));

    // A garment pinned where a dummy stands takes that place, which would be a clash otherwise.
    let mut w = Wardrobe::new("x");
    w.garments = vec![
        dummy_at(0),
        jacket("a", Some(1)),
        dummy_at(2),
        jacket("n", Some(2)),
    ];
    let n = w.garments[3].id.clone();
    dummies::replace_on_add(&mut w, &[n]);
    assert_eq!(places(&w), named(&[("-", 0), ("a", 1), ("n", 2)]));
    assert!(allocate(&w).refused.is_empty());

    // A slot with no dummy leaves the garment as it was.
    let mut w = Wardrobe::new("x");
    w.garments = vec![jacket("a", None), jacket("n", None)];
    let n = w.garments[1].id.clone();
    assert!(dummies::replace_on_add(&mut w, &[n]).is_empty());
    assert_eq!(w.garments[1].position, None);
}

#[test]
fn filling_a_slot_makes_a_dummy_of_each_hole_below_the_last_garment_of_each_collection() {
    let mut w = Wardrobe::new("x");
    w.garments = vec![
        jacket("a", Some(0)),
        jacket("b", Some(3)),
        jacket("c", Some(6)),
        // The second collection of the slot holds drawable 2 of its own and has two holes before it.
        jacket("far", Some(130)),
    ];
    let made = dummies::fill(&mut w, Ped::Male, JBIB);
    assert_eq!(
        made,
        4 + 2,
        "1, 2, 4, 5 of the first collection, 128 and 129 of the second"
    );
    let places: Vec<u32> = allocate(&w).placed.iter().map(|p| p.place).collect();
    assert_eq!(places, [0, 1, 2, 3, 4, 5, 6, 128, 129, 130]);
    assert_eq!(dummies::count(&w, Ped::Male, JBIB), 6);
    // Filling again finds nothing to fill.
    assert_eq!(dummies::fill(&mut w, Ped::Male, JBIB), 0);
}

#[test]
fn sorting_a_slot_puts_its_dummies_after_its_garments_and_pins_each_where_it_went() {
    let mut w = Wardrobe::new("x");
    w.garments = vec![
        dummy_at(0),
        jacket("a", Some(1)),
        dummy_at(2),
        jacket("b", Some(3)),
        jacket("other slot", None),
    ];
    w.garments[4].slot = Some(common::LOWR);
    let moved = dummies::resort(&mut w, Ped::Male, JBIB);
    assert_eq!(moved, 4);
    let in_slot: Vec<(String, u32)> = places(&w)
        .into_iter()
        .filter(|(n, _)| n != "other slot")
        .collect();
    assert_eq!(in_slot, named(&[("a", 0), ("b", 1), ("-", 2), ("-", 3)]));
    assert_eq!(
        w.garments[4].position, None,
        "another slot of the ped is left alone"
    );
    assert_eq!(
        dummies::resort(&mut w, Ped::Male, JBIB),
        0,
        "sorted already"
    );
}

fn drop_module(dir: &Path) -> std::path::PathBuf {
    let module = dir.join("clothing");
    std::fs::create_dir_all(&module).unwrap();
    let mut w = Wardrobe::new("x");
    w.rules.dummies.replace_on_add = true;
    w.garments = vec![dummy_at(0), jacket("a", Some(1))];
    w.write(&module).unwrap();
    module
}

fn drop_jacket(dir: &Path) -> std::path::PathBuf {
    let at = dir.join("drop").join(format!("{C}^jbib_000_u.ydd"));
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, ydd("new jacket")).unwrap();
    let texture = dir
        .join("drop")
        .join(format!("{C}^jbib_diff_000_a_uni.ytd"));
    std::fs::write(&texture, ytd("new texture")).unwrap();
    at.parent().unwrap().to_path_buf()
}

#[test]
fn a_drop_takes_a_dummys_place_when_the_pack_says_so_and_undo_puts_the_dummy_back() {
    let dir = tempfile::tempdir().unwrap();
    let module = drop_module(dir.path());
    let dropped = drop_jacket(dir.path());
    let before = Wardrobe::read(&module).unwrap();

    let added = add_clothes(&module, std::slice::from_ref(&dropped)).unwrap();
    assert_eq!(added.garments.len(), 1);
    let after = Wardrobe::read(&module).unwrap();
    let new = after.garment(&added.garments[0]).expect("the new garment");
    assert_eq!(new.position, Some(0), "the dummy's place");
    assert_eq!(after.garments.len(), 2, "the dummy went");
    assert!(after.garments.iter().all(|g| !g.is_reserved()));

    undo(&module, &added).unwrap();
    let back = Wardrobe::read(&module).unwrap();
    assert_eq!(
        back.garments, before.garments,
        "the dummy is back where it stood"
    );
}

#[test]
fn a_drop_that_replaces_a_dummy_keeps_the_ids_its_shop_metadata_states() {
    use common::demo::{self, M};
    use or_wardrobe::edit;

    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("clothing");
    edit::create(&module, "demo").unwrap();
    let mut w = Wardrobe::read(&module).unwrap();
    w.rules.dummies.replace_on_add = true;
    w.garments = vec![dummy_at(0)];
    w.write(&module).unwrap();

    // A coat whose name states drawable 5, which the server's shop entry also states.
    let high = dir.path().join("high");
    std::fs::create_dir_all(&high).unwrap();
    std::fs::write(high.join(format!("{M}^jbib_005_u.ydd")), ydd("coat")).unwrap();
    std::fs::write(
        high.join(format!("{M}^jbib_diff_005_a_uni.ytd")),
        ytd("coat texture"),
    )
    .unwrap();
    let component = demo::component("0", "HIGH_COAT_5", 5, "PV_COMP_JBIB", 0, "");
    let shop = demo::shop_file("mp_m_freemode_01", "mp_m_demo", &component, "");
    std::fs::write(high.join("shop.meta"), shop).unwrap();

    let added = add_clothes(&module, std::slice::from_ref(&high)).unwrap();
    assert_eq!(added.dummies.len(), 1, "the coat took the dummy's place");
    assert_eq!(added.shop.resolved, 1, "{:?}", added.shop);
    let after = Wardrobe::read(&module).unwrap();
    let coat = after.garment(&added.garments[0]).expect("the coat");
    assert_eq!(coat.position, Some(0), "the dummy's place");
    assert_eq!(
        coat.variations[0].shop.as_ref().map(|e| e.id.as_str()),
        Some("HIGH_COAT_5")
    );
    assert!(after.shop.unplaced.is_empty(), "{:?}", after.shop.unplaced);
}

#[test]
fn a_drop_to_a_pack_that_does_not_replace_dummies_takes_the_lowest_free_place_and_leaves_them() {
    let dir = tempfile::tempdir().unwrap();
    let module = drop_module(dir.path());
    let mut w = Wardrobe::read(&module).unwrap();
    w.rules.dummies.replace_on_add = false;
    w.write(&module).unwrap();
    let dropped = drop_jacket(dir.path());

    let added = add_clothes(&module, &[dropped]).unwrap();
    let after = Wardrobe::read(&module).unwrap();
    assert_eq!(after.garments.len(), 3);
    assert_eq!(after.garment(&added.garments[0]).unwrap().position, None);
    let rows = places(&after);
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter().any(|(_, place)| *place == 2),
        "the new garment is at 2: {rows:?}"
    );
}

#[test]
fn a_pack_that_leaves_dummies_does_it_for_every_removal_and_putting_the_garment_back_lets_the_dummy_go(
) {
    use or_wardrobe::edit::{apply, put_back, take_out, Edit};

    let mut w = Wardrobe::new("x");
    w.rules.dummies.leave_on_delete = true;
    w.garments = vec![jacket("a", None), jacket("b", None), jacket("c", None)];
    let before = w.clone();
    let id = w.garments[1].id.clone();

    let removed = take_out(&mut w, &id).unwrap();
    assert!(removed.left.is_some());
    assert_eq!(places(&w), named(&[("a", 0), ("Dummy", 1), ("c", 2)]));
    put_back(&mut w, &removed).unwrap();
    assert_eq!(w.garments, before.garments, "the dummy let go of the place");

    // The plain removal of an edit does the same, and a pack that does not ask leaves nothing.
    apply(&mut w, &id, Edit::Remove).unwrap();
    assert_eq!(places(&w), named(&[("a", 0), ("Dummy", 1), ("c", 2)]));
    let mut plain = before.clone();
    plain.rules.dummies.leave_on_delete = false;
    apply(&mut plain, &id, Edit::Remove).unwrap();
    assert_eq!(places(&plain), named(&[("a", 0), ("c", 1)]));
}
