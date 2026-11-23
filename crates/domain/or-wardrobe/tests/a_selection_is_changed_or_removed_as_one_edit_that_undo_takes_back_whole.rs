//! An edit over a list of garments is made to all of them or to none, and `revise` keeps it as one revision, so one Undo
//! takes back the removal of three garments, or the change of their ped or slot.

mod common;

use common::{garment, JBIB};
use or_wardrobe::edit::{apply_all, revise, undo_revision, Edit};
use or_wardrobe::{redo, Component, Ped, Slot, Wardrobe};

/// Five male jackets, labelled g0 to g4, in a pack on disk.
fn pack(folder: &std::path::Path) -> Vec<String> {
    let mut w = Wardrobe::new("demo");
    for i in 0..5 {
        let mut g = garment(Ped::Male, JBIB, None, &format!("m{i}.ydd"), &[]);
        g.label = Some(format!("g{i}"));
        w.garments.push(g);
    }
    w.write(folder).unwrap();
    w.garments.iter().map(|g| g.id.clone()).collect()
}

fn labels(folder: &std::path::Path) -> Vec<String> {
    Wardrobe::read(folder)
        .unwrap()
        .garments
        .iter()
        .map(|g| g.label.clone().unwrap())
        .collect()
}

#[test]
fn three_garments_removed_together_come_back_together_with_one_undo() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let ids = pack(at);
    let before = Wardrobe::read(at).unwrap();

    let ((), change) = revise(at, |w| {
        apply_all(
            w,
            &[ids[0].clone(), ids[2].clone(), ids[4].clone()],
            Edit::Remove,
        )
    })
    .unwrap();

    assert_eq!(labels(at), ["g1", "g3"]);
    let change = change.expect("the pack changed, so there is something to take back");

    let redo_over = undo_revision(at, &change).unwrap();
    assert_eq!(
        Wardrobe::read(at).unwrap(),
        before,
        "all three, in their places"
    );
    redo(at, &redo_over).unwrap();
    assert_eq!(labels(at), ["g1", "g3"]);
}

#[test]
fn a_selection_changes_ped_and_slot_in_one_step_and_one_name_the_pack_lacks_changes_none() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let ids = pack(at);
    let before = Wardrobe::read(at).unwrap();
    let three = [ids[1].clone(), ids[2].clone(), ids[3].clone()];

    let ((), change) = revise(at, |w| apply_all(w, &three, Edit::Ped(Some(Ped::Female)))).unwrap();
    let ((), later) = revise(at, |w| {
        apply_all(
            w,
            &three,
            Edit::Slot(Some(Slot::Component(Component::Lowr))),
        )
    })
    .unwrap();

    let w = Wardrobe::read(at).unwrap();
    for (i, g) in w.garments.iter().enumerate() {
        let moved = (1..=3).contains(&i);
        assert_eq!(
            g.ped,
            Some(if moved { Ped::Female } else { Ped::Male }),
            "g{i}"
        );
        assert_eq!(
            g.slot,
            Some(if moved {
                Slot::Component(Component::Lowr)
            } else {
                JBIB
            }),
            "g{i}"
        );
    }

    // One Undo for each edit, the newer first: the older is refused until then.
    assert!(undo_revision(at, &change.clone().unwrap()).is_err());
    undo_revision(at, &later.unwrap()).unwrap();
    undo_revision(at, &change.unwrap()).unwrap();
    assert_eq!(Wardrobe::read(at).unwrap(), before);

    // A name the pack lacks stops the whole edit: no garment of the list is changed.
    let mut w = before.clone();
    let list = [ids[0].clone(), "nobody".to_string(), ids[1].clone()];
    let refused = apply_all(&mut w, &list, Edit::Ped(Some(Ped::Female))).unwrap_err();
    assert!(refused.to_string().contains("nobody"), "{refused}");
    assert_eq!(w, before);

    // A garment named twice is changed once.
    let mut w = before.clone();
    apply_all(&mut w, &[ids[0].clone(), ids[0].clone()], Edit::Remove).unwrap();
    assert_eq!(w.garments.len(), 4);
}
