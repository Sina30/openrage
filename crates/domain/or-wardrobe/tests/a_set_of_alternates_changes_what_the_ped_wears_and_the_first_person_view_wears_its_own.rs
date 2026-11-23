//! What the dressed ped wears when a set of alternate variations is in force, and what it wears in the first person view.
//!
//! No renderer, no install and no corpus: the ped's own outfit is built here from the metadata's records, as the hat suite does.

mod common;

use common::{garment, JBIB, P_HEAD};
use or_io::meta::{CPVComponentData, CPVDrawblData, CPVTextureData, CPedVariationInfo};
use or_wardrobe::alternate::{self, Asset};
use or_wardrobe::dress::{dress, BaseEdit, BaseOutfit, By, Choice, Dressing, Sets, Wear};
use or_wardrobe::flags::{self, Flag};
use or_wardrobe::{firstperson, Component, Ped, Skin, Slot, Wardrobe};

/// A ped with four heads, three hairs and two torsos, drawable 0 of each worn by default.
fn base() -> BaseOutfit {
    let mut info = CPedVariationInfo::default();
    info.avail_comp = vec![255; Component::ALL.len()];
    for (component, drawables) in [
        (Component::Head, 4),
        (Component::Hair, 3),
        (Component::Jbib, 2),
    ] {
        info.avail_comp[usize::from(component.id())] = info.a_component_data3.len() as u8;
        let mut data = CPVComponentData::default();
        data.a_drawbl_data3 = (0..drawables)
            .map(|_| {
                let mut drawable = CPVDrawblData::default();
                let mut texture = CPVTextureData::default();
                texture.tex_id = Skin::Universal.tex_id();
                drawable.a_tex_data = vec![texture];
                drawable
            })
            .collect();
        info.a_component_data3.push(data);
    }
    BaseOutfit::of(&info)
}

const MAIN: &str = "models/jbib_000_u.ydd";
const FIRST: &str = "models/jbib_000_u_1.ydd";
const SECOND: &str = "models/jbib_000_u_2.ydd";

/// A pack of a hat and a jacket that holds two alternate models, and the ids of both.
fn pack() -> (Wardrobe, String, String) {
    let mut w = Wardrobe::new("demo");
    let hat = garment(
        Ped::Male,
        P_HEAD,
        Some(0),
        "models/p_head_000.ydd",
        &["textures/p_head_a.ytd"],
    );
    let mut jacket = garment(Ped::Male, JBIB, Some(0), MAIN, &["textures/jbib_a.ytd"]);
    jacket.alternates = vec![FIRST.into(), SECOND.into()];
    let ids = (hat.id.clone(), jacket.id.clone());
    w.garments.push(hat);
    w.garments.push(jacket);
    (w, ids.0, ids.1)
}

fn wearing(ids: &[&str]) -> Choice {
    let mut choice = Choice::new(Ped::Male);
    choice.wear = ids
        .iter()
        .map(|id| Wear {
            garment: (*id).to_string(),
            variation: 0,
        })
        .collect();
    choice
}

fn model_of(dressing: &Dressing, component: Component) -> Option<&str> {
    match dressing.slot(component).map(|w| &w.by) {
        Some(By::Garment { model, .. } | By::Base { model, .. }) => Some(model),
        None => None,
    }
}

/// A set that makes the jacket wear alternate `alt` while the hat is worn.
fn hat_changes_the_jacket(w: &mut Wardrobe, hat: &str, jacket: &str, alt: u8) -> String {
    let set = alternate::new_set(w, Ped::Male, "Hat and jacket").unwrap();
    alternate::add_switch(w, &set, Asset::garment(jacket), alt).unwrap();
    alternate::join(w, &set, Asset::garment(hat)).unwrap();
    set
}

#[test]
fn a_set_changes_the_model_of_a_garment_while_its_source_is_worn_and_the_person_can_hold_it_off_or_on(
) {
    let (mut w, hat, jacket) = pack();
    let set = hat_changes_the_jacket(&mut w, &hat, &jacket, 1);

    // The jacket alone wears its own model; with the hat on, the set says alternate 1.
    let dressing = dress(&w, &base(), &wearing(&[&jacket]));
    assert_eq!(model_of(&dressing, Component::Jbib), Some(MAIN));
    assert!(dressing.driven.is_empty());
    let dressing = dress(&w, &base(), &wearing(&[&jacket, &hat]));
    assert_eq!(model_of(&dressing, Component::Jbib), Some(FIRST));
    assert_eq!(dressing.driven.len(), 1, "{:?}", dressing.driven);
    let driven = &dressing.driven[0];
    assert_eq!(
        (&driven.target, driven.alt, &driven.set, driven.overridden),
        (&Asset::garment(&jacket), 1, &set, false)
    );
    assert_eq!(driven.name, "Hat and jacket");

    // Off holds every set back whatever is worn.
    let mut off = wearing(&[&jacket, &hat]);
    off.sets = Sets::Off;
    let dressing = dress(&w, &base(), &off);
    assert_eq!(model_of(&dressing, Component::Jbib), Some(MAIN));
    assert!(dressing.driven.is_empty());

    // One set chosen is worn as if its source were, with no hat on.
    let mut only = wearing(&[&jacket]);
    only.sets = Sets::Only(set.clone());
    let dressing = dress(&w, &base(), &only);
    assert_eq!(model_of(&dressing, Component::Jbib), Some(FIRST));

    // A set of the other ped changes nothing of this one.
    let mut other = wearing(&[&jacket, &hat]);
    other.sets = Sets::Only("no such set".into());
    assert_eq!(
        model_of(&dress(&w, &base(), &other), Component::Jbib),
        Some(MAIN)
    );
    // A is the ped as the game has it, with no pack worn.
    let mut original = wearing(&[&jacket, &hat]);
    original.original = true;
    assert!(dress(&w, &base(), &original).driven.is_empty());
}

#[test]
fn the_model_a_person_chose_for_a_garment_wins_over_a_set_and_says_so() {
    let (mut w, hat, jacket) = pack();
    hat_changes_the_jacket(&mut w, &hat, &jacket, 1);
    let mut choice = wearing(&[&jacket, &hat]);
    choice.alternates.insert(jacket.clone(), 2);
    let dressing = dress(&w, &base(), &choice);
    assert_eq!(model_of(&dressing, Component::Jbib), Some(SECOND));
    assert!(dressing.driven[0].overridden, "{:?}", dressing.driven);

    // The main model is a choice too: it is held against the set.
    choice.alternates.insert(jacket.clone(), 0);
    let dressing = dress(&w, &base(), &choice);
    assert_eq!(model_of(&dressing, Component::Jbib), Some(MAIN));
    assert!(dressing.driven[0].overridden);
}

#[test]
fn a_switch_to_an_alternate_the_garment_holds_no_model_for_is_said_and_not_drawn() {
    let (mut w, hat, jacket) = pack();
    hat_changes_the_jacket(&mut w, &hat, &jacket, 3);
    let dressing = dress(&w, &base(), &wearing(&[&jacket, &hat]));
    assert_eq!(model_of(&dressing, Component::Jbib), Some(MAIN));
    assert!(dressing.driven.is_empty());
    assert_eq!(dressing.unmet.len(), 1, "{:?}", dressing.unmet);
    assert!(
        dressing.unmet[0].contains("alternate 3") && dressing.unmet[0].contains("2 alternate"),
        "{}",
        dressing.unmet[0]
    );
}

#[test]
fn a_set_changes_the_peds_own_drawable_by_the_number_after_its_models_name_when_the_ped_wears_that_one(
) {
    let (mut w, hat, _) = pack();
    // The game's own hair 0 wears alternate 1 while the hat is worn.
    let set = alternate::new_set(&mut w, Ped::Male, "Hat and hair").unwrap();
    alternate::add_switch(
        &mut w,
        &set,
        Asset::outside(None, Slot::Component(Component::Hair), 0),
        1,
    )
    .unwrap();
    alternate::join(&mut w, &set, Asset::garment(&hat)).unwrap();

    let dressing = dress(&w, &base(), &wearing(&[&hat]));
    assert_eq!(
        model_of(&dressing, Component::Hair),
        Some("hair_000_u_1.ydd")
    );
    assert_eq!(dressing.driven.len(), 1);

    // Another of the ped's hairs is not the drawable the switch names.
    let mut other = wearing(&[&hat]);
    other.base.insert(Component::Hair, BaseEdit::Drawable(1));
    let dressing = dress(&w, &base(), &other);
    assert_eq!(model_of(&dressing, Component::Hair), Some("hair_001_u.ydd"));
    assert!(dressing.driven.is_empty());
}

#[test]
fn the_first_person_view_leaves_out_what_is_flagged_hidden_there_and_wears_the_first_person_alternate(
) {
    let (mut w, hat, jacket) = pack();
    let mut first = wearing(&[&jacket, &hat]);
    first.first_person = true;

    // Nothing is flagged and no alternate stated: the same garments are worn.
    let plain = dress(&w, &base(), &wearing(&[&jacket, &hat]));
    let seen = dress(&w, &base(), &first);
    assert_eq!(seen.props, plain.props);
    assert_eq!(
        model_of(&seen, Component::Jbib),
        model_of(&plain, Component::Jbib)
    );
    assert!(seen.left.is_empty() && seen.unmet.is_empty());

    // The jacket states alternate 2 for the first person.
    let g = w.garments.iter_mut().find(|g| g.id == jacket).unwrap();
    firstperson::set(g, Some(2)).unwrap();
    let dressing = dress(&w, &base(), &first);
    assert_eq!(model_of(&dressing, Component::Jbib), Some(SECOND));
    let in_third = dress(&w, &base(), &wearing(&[&jacket, &hat]));
    assert_eq!(
        model_of(&in_third, Component::Jbib),
        Some(MAIN),
        "only the first person view wears it"
    );

    // An alternate the garment holds no model for is not worn.
    let g = w.garments.iter_mut().find(|g| g.id == jacket).unwrap();
    firstperson::set(g, Some(9)).unwrap();
    assert_eq!(
        model_of(&dress(&w, &base(), &first), Component::Jbib),
        Some(MAIN)
    );

    // The hat is flagged hidden in the first person view: it is not drawn there, and says why.
    let g = w.garments.iter_mut().find(|g| g.id == hat).unwrap();
    flags::set(g, Flag::HideInFirstPerson, true).unwrap();
    let dressing = dress(&w, &base(), &first);
    assert!(dressing.props.is_empty(), "{dressing:?}");
    assert!(
        dressing
            .left
            .iter()
            .any(|l| l.garment == hat && l.why.contains("first person")),
        "{:?}",
        dressing.left
    );
    assert_eq!(
        dress(&w, &base(), &wearing(&[&jacket, &hat])).props.len(),
        1
    );
}

#[test]
fn a_garment_hidden_in_the_first_person_view_is_still_worn_for_the_sets_it_causes() {
    let (mut w, hat, jacket) = pack();
    hat_changes_the_jacket(&mut w, &hat, &jacket, 1);
    let g = w.garments.iter_mut().find(|g| g.id == hat).unwrap();
    flags::set(g, Flag::HideInFirstPerson, true).unwrap();
    let mut first = wearing(&[&jacket, &hat]);
    first.first_person = true;
    let dressing = dress(&w, &base(), &first);
    assert!(dressing.props.is_empty());
    assert_eq!(
        model_of(&dressing, Component::Jbib),
        Some(FIRST),
        "the game collects the alternates of what is worn, drawn or not"
    );
}

#[test]
fn the_first_person_view_leaves_out_the_peds_own_head_and_hair_the_camera_stands_in() {
    let (w, _, jacket) = pack();
    let third = dress(&w, &base(), &wearing(&[&jacket]));
    assert!(third.slot(Component::Head).is_some() && third.slot(Component::Hair).is_some());

    let mut first = wearing(&[&jacket]);
    first.first_person = true;
    let dressing = dress(&w, &base(), &first);
    assert!(dressing.slot(Component::Head).is_none(), "{dressing:?}");
    assert!(dressing.slot(Component::Hair).is_none(), "{dressing:?}");
    assert!(
        dressing.slot(Component::Jbib).is_some(),
        "the body is seen from the head"
    );
}
