//! A pack keeps a rotating set of its own versions beside it, a `wardrobe.json` that does not read is replaced by the last
//! version that does (and the text that did not read is kept), and one garment or one texture goes back to what a version held
//! of it while every other garment stays as it is.

mod common;

use std::path::Path;

use common::{garment, JBIB, LOWR};
use or_wardrobe::backup::{
    last_good, previous, read_version, restore_last_good, roll_back_garment, roll_back_variation,
    versions, KEPT,
};
use or_wardrobe::edit::{apply, Edit};
use or_wardrobe::{Ped, Wardrobe, FILE};

/// A pack of two garments, the jacket with two textures, written into `dir`.
fn pack(dir: &Path) -> Wardrobe {
    let mut w = Wardrobe::new("pack");
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(0),
        "m0.ydd",
        &["t0.ytd", "t1.ytd"],
    ));
    w.garments
        .push(garment(Ped::Male, LOWR, Some(0), "m1.ydd", &["t2.ytd"]));
    w.write(dir).unwrap();
    w
}

#[test]
fn every_write_keeps_a_version_and_the_set_rotates_and_an_unchanged_write_adds_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    assert_eq!(
        versions(dir.path()).len(),
        0,
        "a pack that was just made has no earlier version"
    );
    assert!(
        !dir.path().join(".openrage").exists(),
        "and makes no private folder"
    );

    w.write(dir.path()).unwrap();
    assert_eq!(
        versions(dir.path()).len(),
        1,
        "the first write over a pack keeps it as it was found"
    );
    w.write(dir.path()).unwrap();
    assert_eq!(
        versions(dir.path()).len(),
        1,
        "the same bytes are not kept twice"
    );

    for i in 0..KEPT + 6 {
        w.garments[0].label = Some(format!("jacket {i}"));
        w.write(dir.path()).unwrap();
    }
    let kept = versions(dir.path());
    assert_eq!(kept.len(), KEPT, "the oldest go first");
    let newest = read_version(dir.path(), kept.last().unwrap().n).unwrap();
    assert_eq!(newest, Wardrobe::read(dir.path()).unwrap());
    assert_eq!(kept.last().unwrap().label, "Changed jacket 55");

    let ignore = std::fs::read(dir.path().join(".openrage").join(".gitignore")).unwrap();
    assert_eq!(ignore, b"*\n", "the private folder ignores itself");
}

#[test]
fn a_change_made_outside_openrage_is_kept_as_a_version_before_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    w.garments[0].label = Some("first".into());
    w.write(dir.path()).unwrap();

    // Somebody edits the file by hand, then OpenRage reads it, changes it and writes it.
    let mut by_hand = w.clone();
    by_hand.garments[1].label = Some("by hand".into());
    std::fs::write(dir.path().join(FILE), by_hand.to_json()).unwrap();
    let mut w = Wardrobe::read(dir.path()).unwrap();
    w.garments[0].label = Some("by OpenRage".into());
    w.write(dir.path()).unwrap();

    let labels: Vec<String> = versions(dir.path()).into_iter().map(|v| v.label).collect();
    assert_eq!(
        labels,
        [
            "The pack as it was found",
            "Changed first",
            "Changed outside OpenRage",
            "Changed by OpenRage"
        ],
        "{labels:?}"
    );
    let held = read_version(dir.path(), versions(dir.path())[2].n).unwrap();
    assert_eq!(held, by_hand, "the edit made by hand is still in a version");
}

#[test]
fn a_manifest_that_does_not_read_is_replaced_by_the_last_version_that_does_and_its_text_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    w.garments[0].label = Some("the good one".into());
    w.write(dir.path()).unwrap();

    std::fs::write(dir.path().join(FILE), b"{ \"garments\": [ not json").unwrap();
    assert!(
        Wardrobe::read(dir.path()).is_err(),
        "the pack no longer reads"
    );
    let (version, held) = last_good(dir.path()).expect("an earlier version reads");
    assert_eq!(held, w);

    let restored = restore_last_good(dir.path()).unwrap();
    assert_eq!(restored, version);
    assert_eq!(
        Wardrobe::read(dir.path()).unwrap(),
        w,
        "the last good pack is back"
    );

    let broken: Vec<_> = std::fs::read_dir(dir.path().join(".openrage").join("unreadable"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(broken.len(), 1);
    assert_eq!(
        std::fs::read(broken[0].path()).unwrap(),
        b"{ \"garments\": [ not json",
        "the text that did not read is kept so a typo can be mended by hand"
    );
}

#[test]
fn a_pack_with_no_version_that_reads_has_nothing_to_restore() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FILE), b"nonsense").unwrap();
    assert!(last_good(dir.path()).is_none());
    assert!(restore_last_good(dir.path()).is_err());
    assert_eq!(
        std::fs::read(dir.path().join(FILE)).unwrap(),
        b"nonsense",
        "nothing is touched when there is nothing to restore"
    );
}

#[test]
fn a_rollback_of_one_garment_leaves_the_others_and_a_rollback_of_one_texture_leaves_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    let (jacket, trousers) = (w.garments[0].id.clone(), w.garments[1].id.clone());
    let then = Wardrobe::read(dir.path()).unwrap();

    // Both garments change, and so does a texture of the jacket.
    apply(&mut w, &jacket, Edit::Skin(or_wardrobe::Skin::Universal)).unwrap();
    w.garments[0].label = Some("renamed jacket".into());
    w.garments[0].variations[0].tint = Some("red".into());
    w.garments[0].variations[1].tint = Some("blue".into());
    w.garments[1].label = Some("renamed trousers".into());
    w.write(dir.path()).unwrap();

    let mut now = Wardrobe::read(dir.path()).unwrap();
    roll_back_variation(&mut now, &then, &jacket, 0).unwrap();
    assert_eq!(
        now.garments[0].variations[0].tint, None,
        "texture a is back"
    );
    assert_eq!(
        now.garments[0].variations[1].tint.as_deref(),
        Some("blue"),
        "texture b keeps its change"
    );
    assert_eq!(now.garments[0].label.as_deref(), Some("renamed jacket"));
    assert_eq!(now.garments[1].label.as_deref(), Some("renamed trousers"));

    let mut now = Wardrobe::read(dir.path()).unwrap();
    roll_back_garment(&mut now, &then, &jacket).unwrap();
    assert_eq!(now.garments[0], then.garments[0], "the jacket is as it was");
    assert_eq!(
        now.garments[1].label.as_deref(),
        Some("renamed trousers"),
        "the trousers keep theirs"
    );
    assert_eq!(now.garments[1].id, trousers);

    assert!(roll_back_garment(&mut now, &then, "nobody").is_err());
    assert!(roll_back_variation(&mut now, &then, &jacket, 9).is_err());
}

#[test]
fn the_previous_version_is_the_pack_as_it_was_before_its_latest_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    assert!(
        previous(dir.path(), &w).is_none(),
        "a pack that was just made has no earlier version"
    );

    w.garments[0].label = Some("first".into());
    w.write(dir.path()).unwrap();
    let (version, before) = previous(dir.path(), &w).expect("the pack as it was found");
    assert_eq!(before.garments[0].label, None);
    assert_eq!(version.label, "The pack as it was found");

    w.garments.truncate(1);
    w.write(dir.path()).unwrap();
    let (_, before) = previous(dir.path(), &w).expect("the pack before the removal");
    assert_eq!(before.garments.len(), 2, "the removal is what changed");
    assert_eq!(before.garments[0].label.as_deref(), Some("first"));

    // The current pack is never its own previous version, and an unchanged write adds none.
    w.write(dir.path()).unwrap();
    assert_eq!(previous(dir.path(), &w).unwrap().1.garments.len(), 2);
}
