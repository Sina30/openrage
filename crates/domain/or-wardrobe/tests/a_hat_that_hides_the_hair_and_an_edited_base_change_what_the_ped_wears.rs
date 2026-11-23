//! What the dressed ped wears when a hat states the hair scale that hides the hair and when the ped's own slots are edited.
//!
//! No renderer, no install and no corpus: the ped's own outfit is built here from the metadata's records.

mod common;

use common::{garment, JBIB, P_HEAD};
use or_io::meta::{CPVComponentData, CPVDrawblData, CPVTextureData, CPedVariationInfo};
use or_wardrobe::dress::{dress, BaseEdit, BaseOutfit, By, Choice, Dressing, Hidden, Wear};
use or_wardrobe::{hair, Component, Ped, Skin, Wardrobe};

/// A ped with four heads (race skin), three hairs and two torsos (universal skin), drawable 0 of each worn by default.
fn base() -> BaseOutfit {
    let mut info = CPedVariationInfo::default();
    info.avail_comp = vec![255; Component::ALL.len()];
    for (component, drawables, skin) in [
        (Component::Head, 4, Skin::Race),
        (Component::Hair, 3, Skin::Universal),
        (Component::Jbib, 2, Skin::Universal),
    ] {
        info.avail_comp[usize::from(component.id())] = info.a_component_data3.len() as u8;
        let mut data = CPVComponentData::default();
        data.a_drawbl_data3 = (0..drawables)
            .map(|_| {
                let mut drawable = CPVDrawblData::default();
                let mut texture = CPVTextureData::default();
                texture.tex_id = skin.tex_id();
                drawable.a_tex_data = vec![texture];
                drawable
            })
            .collect();
        info.a_component_data3.push(data);
    }
    BaseOutfit::of(&info)
}

/// A pack of one hat and one jacket.
fn pack(scale: Option<f32>) -> Wardrobe {
    let mut w = Wardrobe::new("demo");
    let mut hat = garment(
        Ped::Male,
        P_HEAD,
        Some(0),
        "models/p_head_000.ydd",
        &["textures/p_head_a.ytd"],
    );
    hair::set_scale(&mut hat, scale).unwrap();
    w.garments.push(hat);
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(0),
        "models/jbib_000_u.ydd",
        &["textures/jbib_a.ytd"],
    ));
    w
}

fn wearing(w: &Wardrobe, at: usize) -> Choice {
    let mut choice = Choice::new(Ped::Male);
    choice.wear = vec![Wear {
        garment: w.garments[at].id.clone(),
        variation: 0,
    }];
    choice
}

fn base_model(dressing: &Dressing, component: Component) -> Option<&str> {
    match dressing.slot(component).map(|w| &w.by) {
        Some(By::Base { model, .. }) => Some(model),
        _ => None,
    }
}

#[test]
fn a_hat_that_states_the_hide_scale_takes_the_peds_own_hair_off_and_a_hat_that_squashes_it_does_not(
) {
    let hides = pack(Some(hair::HIDES));
    let dressing = dress(&hides, &base(), &wearing(&hides, 0));
    assert!(dressing.slot(Component::Hair).is_none(), "{dressing:?}");
    assert_eq!(
        dressing.hidden,
        vec![Hidden {
            slot: Component::Hair,
            by: hides.garments[0].id.clone()
        }]
    );
    assert!(
        dressing.slot(Component::Head).is_some() && dressing.slot(Component::Jbib).is_some(),
        "only the hair goes: {dressing:?}"
    );

    let squashes = pack(Some(-0.5));
    let dressing = dress(&squashes, &base(), &wearing(&squashes, 0));
    assert!(dressing.slot(Component::Hair).is_some(), "{dressing:?}");
    assert!(dressing.hidden.is_empty());

    // A is the ped as the game has it: its hair, with the hat not worn at all.
    let mut original = wearing(&hides, 0);
    original.original = true;
    let dressing = dress(&hides, &base(), &original);
    assert!(dressing.slot(Component::Hair).is_some());
    assert!(dressing.hidden.is_empty());
}

#[test]
fn hiding_is_a_hair_scale_at_or_below_the_preset_and_only_a_hat_has_one() {
    let mut hat = pack(None).garments.remove(0);
    assert!(!hair::hides_hair(&hat));
    hair::set_scale(&mut hat, Some(hair::HIDES)).unwrap();
    assert!(hair::hides_hair(&hat));
    hair::set_scale(&mut hat, Some(-1.2)).unwrap();
    assert!(hair::hides_hair(&hat), "a stronger squash hides it too");
    hair::set_scale(&mut hat, Some(-0.99)).unwrap();
    assert!(!hair::hides_hair(&hat));
    let jacket = pack(None).garments.remove(1);
    assert!(!hair::hides_hair(&jacket));
}

#[test]
fn a_slot_of_the_peds_own_is_taken_off_or_wears_another_of_its_drawables() {
    let w = pack(None);
    let mut choice = Choice::new(Ped::Male);
    assert_eq!(
        base_model(&dress(&w, &base(), &choice), Component::Hair),
        Some("hair_000_u.ydd")
    );

    choice.base.insert(Component::Hair, BaseEdit::Off);
    choice.base.insert(Component::Jbib, BaseEdit::Drawable(1));
    let dressing = dress(&w, &base(), &choice);
    assert!(dressing.slot(Component::Hair).is_none(), "{dressing:?}");
    let Some(By::Base { model, texture }) = dressing.slot(Component::Jbib).map(|w| &w.by) else {
        panic!("the torso is worn: {dressing:?}");
    };
    assert_eq!(model, "jbib_001_u.ydd");
    assert!(
        texture
            .as_deref()
            .is_some_and(|t| t.starts_with("jbib_diff_001_a_")),
        "{texture:?}"
    );

    // A drawable the ped does not have is no edit.
    choice.base.insert(Component::Jbib, BaseEdit::Drawable(9));
    assert_eq!(
        base_model(&dress(&w, &base(), &choice), Component::Jbib),
        Some("jbib_000_u.ydd")
    );
}

#[test]
fn a_garment_beats_an_edit_of_its_slot_and_a_head_keeps_its_tone() {
    let w = pack(None);
    let mut choice = wearing(&w, 1);
    choice.base.insert(Component::Jbib, BaseEdit::Off);
    let dressing = dress(&w, &base(), &choice);
    assert!(
        matches!(
            dressing.slot(Component::Jbib).map(|w| &w.by),
            Some(By::Garment { .. })
        ),
        "{dressing:?}"
    );

    let mut choice = Choice::new(Ped::Male);
    choice.base.insert(Component::Head, BaseEdit::Drawable(3));
    let dressing = dress(&w, &base(), &choice);
    let Some(By::Base { model, texture }) = dressing.slot(Component::Head).map(|w| &w.by) else {
        panic!("the head is worn: {dressing:?}");
    };
    assert_eq!(model, "head_003_r.ydd");
    assert_eq!(
        texture.as_deref(),
        Some(format!("head_diff_003_a_{}.ytd", choice.tone.word()).as_str())
    );
}

#[test]
fn the_base_says_how_many_drawables_each_slot_has() {
    let base = base();
    assert_eq!(base.drawables(Component::Head), 4);
    assert_eq!(base.drawables(Component::Hair), 3);
    assert_eq!(base.drawables(Component::Feet), 0);
    assert_eq!(
        base.slots().collect::<Vec<_>>(),
        [Component::Head, Component::Hair, Component::Jbib]
    );
}
