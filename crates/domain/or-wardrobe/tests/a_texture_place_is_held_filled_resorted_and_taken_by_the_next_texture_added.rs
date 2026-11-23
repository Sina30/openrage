//! A texture place: a texture of a garment that holds its letter and ships no file, as the dummies of the drawables hold a number.
//!
//! One is made, filled with a file (the place keeps its letter and what it states), put after the textures that have a file, and
//! taken by the next texture added when the pack replaces a dummy on add. A build writes the place as a texture with no file.

mod common;

use std::path::Path;

use common::{garment, put_bytes, JBIB};
use or_project::Origin;
use or_wardrobe::edit::undo_revision;
use or_wardrobe::{
    allocate, inspect, metadata, plan, replace, variations, Depth, Ped, Variation, Wardrobe,
};

/// A male jacket with the textures a and b, saved in `folder`.
fn jacket(folder: &Path) -> (Wardrobe, String) {
    put_bytes(folder, "models/jbib.ydd", &common::ydd("model"));
    put_bytes(folder, "textures/a.ytd", &common::ytd("texture a"));
    put_bytes(folder, "textures/b.ytd", &common::ytd("texture b"));
    let g = garment(
        Ped::Male,
        JBIB,
        Some(3),
        "models/jbib.ydd",
        &["textures/a.ytd", "textures/b.ytd"],
    );
    let id = g.id.clone();
    let mut w = Wardrobe::new("demo");
    w.garments.push(g);
    w.write(folder).unwrap();
    (w, id)
}

/// The file of each texture, none where the place holds none.
fn files(w: &Wardrobe) -> Vec<Option<&str>> {
    w.garments[0]
        .variations
        .iter()
        .map(|v| v.texture.as_deref())
        .collect()
}

/// The textures a build writes for the garment, by the names the game gives their files.
fn shipped(folder: &Path, w: &Wardrobe) -> Vec<String> {
    let facts = inspect(folder, w, Depth::Stat);
    let mut found: Vec<String> = plan(w, &facts, "pack", folder)
        .contribution
        .files
        .into_iter()
        .filter(|f| f.path.ends_with(".ytd") && matches!(f.origin, Origin::Copy { .. }))
        .map(|f| f.path.rsplit('/').next().unwrap().to_string())
        .collect();
    found.sort();
    found
}

#[test]
fn a_place_is_held_after_the_textures_ships_no_file_and_is_written_as_a_texture_of_the_drawable() {
    let dir = tempfile::tempdir().unwrap();
    let (mut w, id) = jacket(dir.path());
    let before = shipped(dir.path(), &w);

    assert_eq!(variations::reserve(&mut w, &id, 2).unwrap(), 2);
    assert_eq!(
        files(&w),
        [Some("textures/a.ytd"), Some("textures/b.ytd"), None, None]
    );
    assert_eq!(
        shipped(dir.path(), &w),
        before,
        "a place ships no file, and the textures that have one keep their letters"
    );

    // The drawable states four textures, because the game counts a texture for every letter.
    let info = metadata::variation_info(&w, &allocate(&w), Ped::Male);
    let counts: Vec<usize> = info
        .a_component_data3
        .iter()
        .flat_map(|c| &c.a_drawbl_data3)
        .map(|d| d.a_tex_data.len())
        .collect();
    assert_eq!(counts.iter().max(), Some(&4), "{counts:?}");

    // The letters a to z are all a garment can name.
    assert!(variations::reserve(&mut w, &id, 0).is_err());
    let refused = variations::reserve(&mut w, &id, 23)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("26"), "{refused}");
    assert_eq!(variations::reserve(&mut w, &id, 22).unwrap(), 22);
    assert_eq!(w.garments[0].variations.len(), 26);
    assert!(variations::reserve(&mut w, "nobody", 1).is_err());
}

#[test]
fn a_place_filled_keeps_its_letter_and_what_it_states_and_undo_holds_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let (mut w, id) = jacket(at);
    variations::reserve(&mut w, &id, 2).unwrap();
    // The place states a tint before it has a file.
    variations::tint(&mut w, &id, 2, None, Some("bla")).unwrap();
    w.write(at).unwrap();
    let held = Wardrobe::read(at).unwrap();

    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("c.ytd"), common::ytd("texture c")).unwrap();
    let change = replace::texture(at, &id, 2, &outside.path().join("c.ytd")).unwrap();

    let filled = Wardrobe::read(at).unwrap();
    let v = &filled.garments[0].variations;
    assert_eq!(v.len(), 4, "filling a place adds none");
    assert!(
        v[2].texture
            .as_deref()
            .is_some_and(|t| t.ends_with("c.textures.src")),
        "{:?}",
        v[2].texture
    );
    assert_eq!(
        v[2].tint.as_deref(),
        Some("bla"),
        "what the place stated stays"
    );
    assert_eq!(
        v[3],
        Variation::default(),
        "the place after it is still held"
    );
    assert_eq!(v[..2], held.garments[0].variations[..2]);

    undo_revision(at, &change).unwrap();
    assert_eq!(
        Wardrobe::read(at).unwrap(),
        held,
        "one Undo holds the place again"
    );
}

#[test]
fn the_places_are_put_after_the_textures_that_have_a_file_and_a_filled_place_keeps_its_number() {
    let dir = tempfile::tempdir().unwrap();
    let (mut w, id) = jacket(dir.path());
    // Place, a, place, b: the textures are 1 and 3 and the places 0 and 2.
    let g = &mut w.garments[0];
    let (a, b) = (g.variations[0].clone(), g.variations[1].clone());
    g.variations = vec![
        Variation::default(),
        a.clone(),
        Variation::default(),
        b.clone(),
    ];

    assert_eq!(variations::resort(&mut w, &id).unwrap(), 3);
    assert_eq!(
        files(&w),
        [Some("textures/a.ytd"), Some("textures/b.ytd"), None, None]
    );
    assert_eq!(w.garments[0].variations[..2], [a, b]);
    assert_eq!(
        variations::resort(&mut w, &id).unwrap(),
        0,
        "the places are last already"
    );

    // A place filled while the places are last stays where it is when they are put last again.
    w.garments[0].variations[2].texture = Some("textures/a.ytd".into());
    assert_eq!(variations::resort(&mut w, &id).unwrap(), 0);
    assert_eq!(
        files(&w),
        [
            Some("textures/a.ytd"),
            Some("textures/b.ytd"),
            Some("textures/a.ytd"),
            None
        ]
    );
    assert!(variations::resort(&mut w, "nobody").is_err());
}

#[test]
fn a_texture_added_takes_the_first_place_when_the_pack_replaces_a_dummy_on_add_and_follows_the_textures_otherwise(
) {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let (mut w, id) = jacket(at);
    // a, place, b: the place sits between two textures.
    let g = &mut w.garments[0];
    let (a, b) = (g.variations[0].clone(), g.variations[1].clone());
    g.variations = vec![a.clone(), Variation::default(), b.clone()];
    w.write(at).unwrap();
    let held = Wardrobe::read(at).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("c.ytd"), common::ytd("texture c")).unwrap();
    let source = outside.path().join("c.ytd");

    // Off, as a pack begins: the texture is the next letter.
    let change = variations::add(at, &id, &source).unwrap();
    let after = Wardrobe::read(at).unwrap();
    assert_eq!(after.garments[0].variations.len(), 4);
    assert_eq!(after.garments[0].variations[1], Variation::default());
    undo_revision(at, &change).unwrap();
    assert_eq!(Wardrobe::read(at).unwrap(), held);

    // On: it takes the place, and the textures after it keep their letters.
    let mut on = held.clone();
    on.rules.dummies.replace_on_add = true;
    on.write(at).unwrap();
    let held_on = Wardrobe::read(at).unwrap();
    let change = variations::add(at, &id, &source).unwrap();
    let after = Wardrobe::read(at).unwrap();
    let v = &after.garments[0].variations;
    assert_eq!(v.len(), 3, "no letter was added");
    assert!(v[1].texture.is_some(), "the place was filled: {v:?}");
    assert_eq!((&v[0], &v[2]), (&a, &b));
    undo_revision(at, &change).unwrap();
    assert_eq!(
        Wardrobe::read(at).unwrap(),
        held_on,
        "one Undo holds the place again"
    );

    // With no place held, the texture follows the others even when the rule is on.
    let mut full = held_on.clone();
    full.garments[0].variations = vec![a, b];
    full.write(at).unwrap();
    variations::add(at, &id, &source).unwrap();
    assert_eq!(Wardrobe::read(at).unwrap().garments[0].variations.len(), 3);
}
