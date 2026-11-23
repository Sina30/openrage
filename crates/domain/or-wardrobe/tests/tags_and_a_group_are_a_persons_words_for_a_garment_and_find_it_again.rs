//! Tags and a group are a person's words for a garment: set one by one or over a list as one edit, written to
//! `wardrobe.json` only where somebody wrote them, never part of a build, and what a search reads along with the
//! garment's place, slot, name and the name the game gives its model.

use or_wardrobe::edit::{apply, apply_all, Edit};
use or_wardrobe::{
    allocate, facts::named_paths, layout, plan, search, Component, Facts, Garment, Ped, Skin, Slot,
    Variation, Wardrobe,
};

const JBIB: Slot = Slot::Component(Component::Jbib);

fn garment(model: &str, label: Option<&str>) -> Garment {
    let mut g = Garment::new();
    g.ped = Some(Ped::Male);
    g.slot = Some(JBIB);
    g.model = Some(model.to_string());
    g.label = label.map(str::to_string);
    g.variations = vec![Variation {
        texture: Some(format!("{model}.ytd")),
        ..Default::default()
    }];
    g
}

fn pack() -> Wardrobe {
    let mut w = Wardrobe::new("demo");
    w.garments = vec![
        garment("models/jacket_red.ydd", Some("Red jacket")),
        garment("models/jacket_blue.ydd", None),
        garment("models/vest.ydd", None),
    ];
    w
}

fn ids(w: &Wardrobe) -> Vec<String> {
    w.garments.iter().map(|g| g.id.clone()).collect()
}

/// `edit` to every garment of the pack.
fn all(w: &mut Wardrobe, edit: Edit) {
    let ids = ids(w);
    apply_all(w, &ids, edit).unwrap();
}

/// `edit` to the garment at `index`.
fn one(w: &mut Wardrobe, index: usize, edit: Edit) -> Result<(), or_wardrobe::edit::Error> {
    let id = w.garments[index].id.clone();
    apply(w, &id, edit)
}

#[test]
fn tags_and_a_group_are_set_over_a_list_all_or_none_and_a_pack_without_them_writes_none() {
    let mut w = pack();
    let json = w.to_json();
    assert!(
        !json.contains("\"tags\"") && !json.contains("\"group\""),
        "a garment with neither writes neither key: {json}"
    );

    all(&mut w, Edit::AddTag("  Summer ".into()));
    assert!(w.garments.iter().all(|g| g.tags == ["Summer"]));
    // The same word in other case is the same tag.
    one(&mut w, 0, Edit::AddTag("SUMMER".into())).unwrap();
    assert_eq!(w.garments[0].tags, ["Summer"]);
    one(&mut w, 0, Edit::AddTag("Leather".into())).unwrap();
    assert_eq!(w.garments[0].tags, ["Summer", "Leather"]);
    assert!(
        one(&mut w, 0, Edit::AddTag("   ".into())).is_err(),
        "a tag of nothing is refused"
    );

    all(&mut w, Edit::RemoveTag("summer".into()));
    assert_eq!(w.garments[0].tags, ["Leather"]);
    assert!(w.garments[1].tags.is_empty());

    all(&mut w, Edit::Group(Some("  Winter set ".into())));
    assert!(w
        .garments
        .iter()
        .all(|g| g.group.as_deref() == Some("Winter set")));
    one(&mut w, 1, Edit::Group(Some("  ".into()))).unwrap();
    assert_eq!(w.garments[1].group, None, "a group of nothing is no group");

    // One id the pack lacks leaves every garment as it was.
    let before = w.clone();
    let mut asked = ids(&w);
    asked.push("not-in-the-pack".into());
    assert!(apply_all(&mut w, &asked, Edit::AddTag("lost".into())).is_err());
    assert_eq!(w, before);

    let again = Wardrobe::from_json(w.to_json().as_bytes(), "wardrobe.json").unwrap();
    assert_eq!(again, w, "the tags and the group come back from the file");
}

#[test]
fn a_build_is_the_same_with_tags_and_a_group_as_without() {
    let plain = pack();
    let mut worded = plain.clone();
    all(&mut worded, Edit::AddTag("Summer".into()));
    all(&mut worded, Edit::Group(Some("Set".into())));

    let planned = |w: &Wardrobe| {
        let facts = Facts::from_files(named_paths(w).map(|p| (p, 10, p)));
        let dir = std::path::Path::new(".");
        format!("{:?}", plan(w, &facts, "wardrobe", dir).contribution)
    };
    assert_eq!(planned(&plain), planned(&worded));
}

#[test]
fn a_garment_is_found_by_its_place_slot_name_game_name_group_and_tags() {
    let mut w = pack();
    let first = w.garments[0].id.clone();
    apply(&mut w, &first, Edit::AddTag("Summer".into())).unwrap();
    apply(&mut w, &first, Edit::Group(Some("Leather set".into()))).unwrap();
    let allocation = allocate(&w);

    let found = search::terms(&w, &allocation, 0);
    let game = layout::model_name(
        Ped::Male,
        &w.collection(Ped::Male),
        JBIB,
        0,
        Skin::Universal,
    );
    for part in [
        "000",
        JBIB.name(),
        "Red jacket",
        game.as_str(),
        "Summer",
        "Leather set",
    ] {
        assert!(
            found.iter().any(|t| t == part),
            "`{part}` is not one of {found:?}"
        );
    }
    assert_eq!(
        search::game_name(&w, &allocation, 0).as_deref(),
        Some(game.as_str())
    );

    // The second garment is at the next place, and its name is its model's file.
    let second = search::terms(&w, &allocation, 1);
    let game = layout::model_name(
        Ped::Male,
        &w.collection(Ped::Male),
        JBIB,
        1,
        Skin::Universal,
    );
    for part in ["001", "jacket_blue.ydd", game.as_str()] {
        assert!(
            second.iter().any(|t| t == part),
            "`{part}` is not one of {second:?}"
        );
    }
    assert!(!second.iter().any(|t| t == "Summer"), "{second:?}");

    // A garment whose ped nobody said has no place and no name from the game, and is still found by the rest.
    let mut loose = pack();
    loose.garments[2].ped = None;
    let allocation = allocate(&loose);
    assert_eq!(search::game_name(&loose, &allocation, 2), None);
    assert!(search::terms(&loose, &allocation, 2)
        .iter()
        .any(|t| t == "vest.ydd"));

    // A place held ships no model, so the game names no file for it.
    let mut held = pack();
    held.garments[2].model = None;
    assert_eq!(search::game_name(&held, &allocate(&held), 2), None);
}
