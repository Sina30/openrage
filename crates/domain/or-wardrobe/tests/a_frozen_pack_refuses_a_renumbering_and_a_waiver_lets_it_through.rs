//! A pack that has been built keeps the drawables it shipped: a garment the allocator would put at another drawable
//! stops the build until it is pinned back or a reason says why, and a pack that does not freeze moves freely.

mod common;

use std::path::Path;

use common::{garment, put_bytes, ydd, ytd, JBIB};
use or_wardrobe::edit::{apply, remedy, waive, Edit};
use or_wardrobe::{freeze, plan, Depth, Ped, Planned, Wardrobe};

/// A pack of three unpinned jackets, written to `folder`.
fn pack(folder: &Path) -> Wardrobe {
    put_bytes(folder, "textures/t.ytd", &ytd("t"));
    let mut w = Wardrobe::new("demo");
    for i in 0..3 {
        put_bytes(folder, &format!("models/m{i}.ydd"), &ydd(&format!("m{i}")));
        let mut g = garment(
            Ped::Male,
            JBIB,
            None,
            &format!("models/m{i}.ydd"),
            &["textures/t.ytd"],
        );
        g.label = Some(format!("jacket {i}"));
        w.garments.push(g);
    }
    w.write(folder).unwrap();
    w
}

fn planned(folder: &Path) -> Planned {
    let w = Wardrobe::read(folder).unwrap();
    plan(
        &w,
        &or_wardrobe::inspect(folder, &w, Depth::Open),
        "pack",
        folder,
    )
}

fn renumbered(p: &Planned) -> Vec<&str> {
    p.findings
        .iter()
        .filter(|f| f.code == "renumbered")
        .map(|f| f.message.as_str())
        .collect()
}

/// Take the first jacket out of the pack on disk, as the Wardrobe does.
fn remove_first(folder: &Path) {
    let mut w = Wardrobe::read(folder).unwrap();
    let id = w.garments[0].id.clone();
    apply(&mut w, &id, Edit::Remove).unwrap();
    w.write(folder).unwrap();
}

#[test]
fn a_pack_that_has_not_been_built_renumbers_freely() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    remove_first(dir.path());
    assert!(renumbered(&planned(dir.path())).is_empty());
    assert!(!dir.path().join(freeze::FILE).exists());
}

#[test]
fn a_build_that_went_through_freezes_the_drawables_it_shipped() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    let kept = freeze::keep(dir.path()).unwrap();
    assert_eq!(
        kept,
        freeze::Kept::Wrote {
            first: true,
            places: 3
        }
    );
    let lock = freeze::read(dir.path())
        .unwrap()
        .expect("the lock is there");
    assert_eq!(
        lock.places.iter().map(|l| l.position).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(freeze::keep(dir.path()).unwrap(), freeze::Kept::Same);
    let text = std::fs::read_to_string(dir.path().join(freeze::FILE)).unwrap();
    assert!(text.ends_with("}\n") && !text.contains('\r'));
    assert!(
        !std::fs::read_to_string(dir.path().join("wardrobe.json"))
            .unwrap()
            .contains("places"),
        "the lock is a file of its own and wardrobe.json states none of it"
    );
}

#[test]
fn a_frozen_pack_refuses_a_renumbering_naming_each_garment_that_moved() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    freeze::keep(dir.path()).unwrap();
    remove_first(dir.path());

    let p = planned(dir.path());
    let said = renumbered(&p);
    assert_eq!(
        said.len(),
        2,
        "the two jackets after the one that left: {said:?}"
    );
    assert!(
        said[0].starts_with("jacket 1 was jbib 001 of mp_m_demo") && said[0].contains("jbib 000"),
        "{}",
        said[0]
    );
    assert!(
        (p.findings.iter().filter(|f| f.code == "renumbered")).all(|f| f.blocks()),
        "an error a build stops on"
    );
    assert_eq!(
        p.contribution.problems.len(),
        2,
        "{:?}",
        p.contribution.problems
    );
}

#[test]
fn a_waiver_with_a_reason_lets_a_renumbering_through_and_the_next_good_build_freezes_the_new_drawables(
) {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    freeze::keep(dir.path()).unwrap();
    remove_first(dir.path());

    assert_eq!(
        planned(dir.path()).contribution.problems.len(),
        2,
        "stopped before the waiver"
    );
    let mut w = Wardrobe::read(dir.path()).unwrap();
    assert!(
        waive(&mut w, "renumbered", None, "").is_err(),
        "a waiver needs a reason"
    );
    let reason = "the server wipes its saved outfits with this update";
    waive(&mut w, "renumbered", None, reason).unwrap();
    w.write(dir.path()).unwrap();
    let p = planned(dir.path());
    assert!(
        p.contribution.problems.is_empty(),
        "{:?}",
        p.contribution.problems
    );
    assert!((p.findings.iter().filter(|f| f.code == "renumbered"))
        .all(|f| f.waived.as_deref() == Some(reason)));

    assert!(matches!(
        freeze::keep(dir.path()).unwrap(),
        freeze::Kept::Wrote {
            first: false,
            places: 2
        }
    ));
    assert!(
        renumbered(&planned(dir.path())).is_empty(),
        "the lock holds the new drawables now"
    );
}

#[test]
fn pinning_a_garment_back_where_it_was_built_is_a_fix_that_needs_no_choice() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    freeze::keep(dir.path()).unwrap();
    remove_first(dir.path());

    // One at a time, as the Problems list offers them: the place of one is held by the other until that one is pinned back.
    let mut fixed = 0;
    loop {
        let w = Wardrobe::read(dir.path()).unwrap();
        let p = planned(dir.path());
        let Some(fix) = (p.findings.iter())
            .filter(|f| f.code == "renumbered")
            .find_map(|f| remedy(&w, f))
        else {
            break;
        };
        assert!(fix.what.starts_with("Pin jacket"), "{}", fix.what);
        let mut w = w;
        fix.apply(&mut w).unwrap();
        w.write(dir.path()).unwrap();
        fixed += 1;
    }
    assert_eq!(fixed, 2);
    let p = planned(dir.path());
    assert!(renumbered(&p).is_empty(), "{:?}", renumbered(&p));
    assert!(p.contribution.problems.is_empty());
    let places: Vec<u32> = p.allocation.placed.iter().map(|p| p.position).collect();
    assert_eq!(
        places,
        [1, 2],
        "the two that stayed keep their drawables and 0 is a hole"
    );
}

#[test]
fn a_pack_that_says_not_to_freeze_moves_freely_and_writes_no_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    w.rules.freeze = Some(false);
    w.write(dir.path()).unwrap();
    assert_eq!(freeze::keep(dir.path()).unwrap(), freeze::Kept::Off);
    assert!(!dir.path().join(freeze::FILE).exists());

    // A lock that is there is not held to once the pack says so.
    w.rules.freeze = None;
    w.write(dir.path()).unwrap();
    freeze::keep(dir.path()).unwrap();
    let mut w = Wardrobe::read(dir.path()).unwrap();
    w.rules.freeze = Some(false);
    w.write(dir.path()).unwrap();
    remove_first(dir.path());
    assert!(renumbered(&planned(dir.path())).is_empty());
    assert_eq!(freeze::keep(dir.path()).unwrap(), freeze::Kept::Off);
    assert!(
        !dir.path().join(freeze::FILE).exists(),
        "a lock the pack no longer wants goes"
    );
}

#[test]
fn a_lock_that_does_not_read_freezes_nothing_and_the_next_good_build_writes_it_again() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    std::fs::write(dir.path().join(freeze::FILE), b"not json").unwrap();
    assert!(freeze::read(dir.path()).is_err());
    assert!(Wardrobe::read(dir.path()).unwrap().lock.is_none());
    assert!(matches!(
        freeze::keep(dir.path()).unwrap(),
        freeze::Kept::Wrote { first: true, .. }
    ));
    assert!(freeze::read(dir.path()).unwrap().is_some());
    assert!(freeze::thaw(dir.path()).unwrap());
    assert!(!freeze::thaw(dir.path()).unwrap());
}

#[test]
fn a_changed_cap_moves_the_garments_past_the_old_cut_and_a_frozen_pack_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    for i in 3..130 {
        put_bytes(
            dir.path(),
            &format!("models/m{i}.ydd"),
            &ydd(&format!("m{i}")),
        );
        w.garments.push(garment(
            Ped::Male,
            JBIB,
            None,
            &format!("models/m{i}.ydd"),
            &["textures/t.ytd"],
        ));
    }
    w.write(dir.path()).unwrap();
    freeze::keep(dir.path()).unwrap();

    w.rules.set("slot-cap", |s| s.threshold = Some(100));
    w.write(dir.path()).unwrap();
    let p = planned(dir.path());
    // Drawable 100 of the first collection is drawable 0 of the second now.
    assert_eq!(renumbered(&p).len(), 30, "garments 100 to 129 move");
}

#[test]
fn an_undo_is_not_refused_because_a_build_froze_the_drawables_since_the_edit() {
    use or_wardrobe::edit::{revise, undo_revision};

    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    let (_, revision) = revise(dir.path(), |w| {
        w.garments[0].label = Some("renamed".into());
        Ok(())
    })
    .unwrap();
    let revision = revision.expect("the edit changed the pack");
    freeze::keep(dir.path()).unwrap();
    assert!(freeze::read(dir.path()).unwrap().is_some());

    undo_revision(dir.path(), &revision)
        .expect("only the lock differs, and it is a file of its own");
    assert_eq!(
        Wardrobe::read(dir.path()).unwrap().garments[0]
            .label
            .as_deref(),
        Some("jacket 0")
    );
}
