//! A garment is copied to another ped whole: its model, cloth, alternates and textures are copied to files of their own,
//! its flags and tints are the same, and what names one texture of the first (a shop id) is not carried over.

mod common;

use std::path::Path;

use common::{drawables_source, garment, snapshot, JBIB};
use or_wardrobe::duplicate;
use or_wardrobe::edit::undo_revision;
use or_wardrobe::{allocate, check, facts, redo, Depth, Ped, ShopEntry, Wardrobe};

fn read(folder: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(folder.join(rel)).unwrap()
}

/// The files of the pack, not the private folder beside them.
fn files(folder: &Path) -> Vec<(String, Vec<u8>)> {
    snapshot(folder)
        .into_iter()
        .filter(|(rel, _)| !rel.starts_with(".openrage"))
        .collect()
}

/// A male jacket pinned at 3, with an alternate, a cloth, two tinted textures with flags, and a shop id on each.
fn jacket(folder: &Path) -> String {
    for (rel, body) in [
        ("models/jbib.ydd", &b"model"[..]),
        ("models/jbib_1.ydd", b"alternate"),
        ("cloth/jbib.yld", b"cloth"),
        ("textures/jbib_a.ytd", b"texture a"),
        ("textures/jbib_b.ytd", b"texture b"),
    ] {
        std::fs::create_dir_all(folder.join(rel).parent().unwrap()).unwrap();
        std::fs::write(folder.join(rel), body).unwrap();
    }
    let mut g = garment(
        Ped::Male,
        JBIB,
        Some(3),
        "models/jbib.ydd",
        &["textures/jbib_a.ytd", "textures/jbib_b.ytd"],
    );
    g.alternates = vec!["models/jbib_1.ydd".into()];
    g.cloth = Some("cloth/jbib.yld".into());
    g.variations[1].tint = Some("bla".into());
    g.variations[1].tex_id = Some(1);
    g.variations[0].shop_id = Some("MY_JACKET".into());
    g.variations[1].shop = Some(ShopEntry {
        id: "SHOP_ID_B".into(),
        from: "shop.meta".into(),
        entry: 4,
        cost: Some("500".into()),
        fields: Vec::new(),
    });
    g.meta.prop_mask = Some(17);
    g.label = Some("jacket".into());
    let id = g.id.clone();
    let mut w = Wardrobe::new("demo");
    w.garments.push(g);
    w.write(folder).unwrap();
    id
}

#[test]
fn a_jacket_is_copied_to_the_female_ped_with_its_files_and_flags_and_undo_takes_the_copy_out() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    let before = Wardrobe::read(at).unwrap();
    let files_before = files(at);

    let done = duplicate::to(at, std::slice::from_ref(&id), Ped::Female).expect("a copy is made");

    let w = Wardrobe::read(at).unwrap();
    assert_eq!(w.garments.len(), 2);
    let (first, copy) = (&w.garments[0], &w.garments[1]);
    assert_eq!(done.made, [(id.clone(), copy.id.clone())]);
    assert_ne!(copy.id, first.id);
    assert_eq!(
        first, &before.garments[0],
        "the garment copied is as it was"
    );
    assert_eq!(copy.ped, Some(Ped::Female));
    assert_eq!(copy.slot, first.slot);
    assert_eq!(
        copy.position, None,
        "it takes the lowest place the female slot has free"
    );
    assert_eq!(copy.skin, first.skin);
    assert_eq!(copy.label, first.label);
    assert_eq!(copy.meta, first.meta, "the flags are copied");
    assert_eq!(copy.variations.len(), 2);
    assert_eq!(copy.variations[1].tint.as_deref(), Some("bla"));
    assert_eq!(copy.variations[1].tex_id, Some(1));

    // Every file is a file of its own with the bytes of the one it was copied from.
    let pairs = [
        (first.model.as_ref(), copy.model.as_ref()),
        (first.alternates.first(), copy.alternates.first()),
        (first.cloth.as_ref(), copy.cloth.as_ref()),
        (
            first.variations[0].texture.as_ref(),
            copy.variations[0].texture.as_ref(),
        ),
        (
            first.variations[1].texture.as_ref(),
            copy.variations[1].texture.as_ref(),
        ),
    ];
    for (was, now) in pairs {
        let (was, now) = (
            was.expect("the garment names it"),
            now.expect("and so does the copy"),
        );
        assert_ne!(was, now, "{was} is not shared");
        assert_eq!(read(at, was), read(at, now), "{now} holds what {was} holds");
    }

    // What named one texture of the first names none of the copy.
    assert!(copy
        .variations
        .iter()
        .all(|v| v.shop.is_none() && v.shop_id.is_none()));
    assert!(first.variations[0].shop_id.is_some() && first.variations[1].shop.is_some());

    let redo_over = undo_revision(at, &done.revision).expect("taken back");
    assert_eq!(Wardrobe::read(at).unwrap(), before);
    assert_eq!(
        files(at),
        files_before,
        "the copies are gone and nothing else moved"
    );

    redo(at, &redo_over).expect("and made again");
    assert_eq!(Wardrobe::read(at).unwrap(), w);
    assert_eq!(read(at, copy.model.as_ref().unwrap()), b"model");
}

#[test]
fn a_copy_for_the_other_ped_ships_the_same_model_and_no_rule_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    duplicate::to(at, std::slice::from_ref(&id), Ped::Female).expect("a copy is made");

    let w = Wardrobe::read(at).unwrap();
    assert_eq!(read(at, w.garments[0].model.as_ref().unwrap()), b"model");
    assert_eq!(read(at, w.garments[1].model.as_ref().unwrap()), b"model");
    let found = check(&w, &allocate(&w), &facts::inspect(at, &w, Depth::Digest));
    assert!(
        found.iter().all(|f| f.code != "duplicate-content"),
        "the rule compares garments of one ped: {found:?}"
    );
}

#[test]
fn several_garments_are_copied_in_the_order_asked_or_none_is() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    let before = files(at);

    let refused = duplicate::to(at, &[id.clone(), "nobody".into()], Ped::Female).unwrap_err();
    assert!(refused.to_string().contains("nobody"), "{refused}");
    assert_eq!(
        files(at),
        before,
        "the first copy was made and taken out again"
    );

    assert!(duplicate::to(at, &[], Ped::Female).is_err());

    let two = duplicate::to(at, &[id.clone(), id.clone()], Ped::Male).unwrap();
    assert_eq!(two.made.len(), 2);
    assert_ne!(two.made[0].1, two.made[1].1);
    let w = Wardrobe::read(at).unwrap();
    assert_eq!(w.garments.len(), 3);
    let models: Vec<_> = w
        .garments
        .iter()
        .map(|g| g.model.clone().unwrap())
        .collect();
    assert_eq!(
        models
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3,
        "three garments, three files: {models:?}"
    );
}

#[test]
fn a_garment_that_names_a_source_folder_is_copied_to_a_source_folder_with_an_id_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    drawables_source(&at.join("models/jacket.drawables.src"), "jacket");
    std::fs::write(
        at.join("models/jacket.drawables.src/model.glb"),
        b"geometry",
    )
    .unwrap();
    let mut w = Wardrobe::read(at).unwrap();
    w.garments[0].model = Some("models/jacket.drawables.src".into());
    w.write(at).unwrap();

    let done = duplicate::to(at, &[id], Ped::Female).unwrap();

    let w = Wardrobe::read(at).unwrap();
    let copy = w.garments[1].model.clone().unwrap();
    assert_eq!(copy, "models/jacket-2.drawables.src");
    assert_eq!(read(at, &format!("{copy}/model.glb")), b"geometry");
    let id_of = |rel: &str| {
        or_source::asset::Source::open(&at.join(rel))
            .unwrap()
            .manifest
            .id
    };
    assert_ne!(
        id_of(&copy),
        id_of("models/jacket.drawables.src"),
        "two sources with one id cook into one file"
    );

    undo_revision(at, &done.revision).unwrap();
    assert!(
        !at.join(&copy).exists(),
        "the folder and the files in it are gone"
    );
}

/// A pack at `<tmp>/pack` holding `models/a.ydd`, with `<tmp>/outside.ydd` beside it, and a garment whose model, alternate, cloth and texture all name `named`.
fn pack_naming(named: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path().join("pack");
    std::fs::create_dir_all(at.join("models")).unwrap();
    std::fs::write(at.join("models/a.ydd"), b"inside").unwrap();
    std::fs::write(dir.path().join("outside.ydd"), b"outside").unwrap();
    let mut g = garment(Ped::Male, JBIB, Some(3), named, &[named]);
    g.alternates = vec![named.into()];
    g.cloth = Some(named.into());
    let id = g.id.clone();
    let mut w = Wardrobe::new("demo");
    w.garments.push(g);
    w.write(&at).unwrap();
    (dir, at, id)
}

#[test]
fn a_path_that_leaves_the_pack_or_is_not_a_file_is_named_by_the_copy_and_nothing_is_copied_for_it()
{
    for named in ["../outside.ydd", "..", ".", "", "models"] {
        let (dir, at, id) = pack_naming(named);
        let outside = std::fs::read_dir(dir.path()).unwrap().count();
        let kept: Vec<_> = files(&at)
            .into_iter()
            .filter(|(rel, _)| rel != "wardrobe.json")
            .collect();

        let done = duplicate::to(&at, &[id], Ped::Female)
            .unwrap_or_else(|e| panic!("{named:?} is the same problem for the copy: {e}"));

        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            outside,
            "{named:?}: nothing is written beside the pack"
        );
        let after: Vec<_> = files(&at)
            .into_iter()
            .filter(|(rel, _)| rel != "wardrobe.json")
            .collect();
        assert_eq!(after, kept, "{named:?}: no file or folder was copied");
        let copy = &Wardrobe::read(&at).unwrap().garments[1];
        assert_eq!(copy.model.as_deref(), Some(named));
        assert_eq!(copy.variations[0].texture.as_deref(), Some(named));
        assert_eq!(copy.cloth.as_deref(), Some(named));
        assert!(done.revision.copied.is_empty(), "{named:?}");
    }
}
