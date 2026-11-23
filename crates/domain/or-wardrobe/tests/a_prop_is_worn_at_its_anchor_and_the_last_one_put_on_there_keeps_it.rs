//! What the dressed ped wears of the props: one at each anchor, the last put on keeping it, and the bone each sits at.
//!
//! No renderer, no install and no corpus: the ped's own outfit is empty here, because a ped wears no prop of its own.

mod common;

use common::{garment, JBIB, P_HEAD};
use or_wardrobe::dress::{dress, BaseOutfit, By, Choice, Wear};
use or_wardrobe::{hair, Ped, Prop, Slot, Wardrobe};

const EYES: Slot = Slot::Prop(Prop::Eyes);
const WRIST: Slot = Slot::Prop(Prop::LeftWrist);

fn pack() -> Wardrobe {
    let mut w = Wardrobe::new("demo");
    for (slot, model, texture) in [
        (P_HEAD, "models/p_head_000.ydd", "textures/p_head_a.ytd"),
        (EYES, "models/p_eyes_000.ydd", "textures/p_eyes_a.ytd"),
        (WRIST, "models/p_lwrist_000.ydd", "textures/p_lwrist_a.ytd"),
        (JBIB, "models/jbib_000_u.ydd", "textures/jbib_a.ytd"),
    ] {
        w.garments
            .push(garment(Ped::Male, slot, Some(0), model, &[texture]));
    }
    w
}

fn wearing(w: &Wardrobe, at: &[usize]) -> Choice {
    let mut choice = Choice::new(Ped::Male);
    choice.wear = at
        .iter()
        .map(|i| Wear {
            garment: w.garments[*i].id.clone(),
            variation: 0,
        })
        .collect();
    choice
}

fn model_of(by: &By) -> &str {
    match by {
        By::Garment { model, .. } => model,
        By::Base { model, .. } => model,
    }
}

#[test]
fn a_hat_glasses_and_a_watch_are_worn_each_at_its_anchor_beside_the_jacket() {
    let w = pack();
    let dressing = dress(&w, &BaseOutfit::default(), &wearing(&w, &[0, 1, 2, 3]));

    let at = |prop| dressing.prop(prop).map(|p| model_of(&p.by));
    assert_eq!(at(Prop::Head), Some("models/p_head_000.ydd"));
    assert_eq!(at(Prop::Eyes), Some("models/p_eyes_000.ydd"));
    assert_eq!(at(Prop::LeftWrist), Some("models/p_lwrist_000.ydd"));
    assert_eq!(dressing.props.len(), 3, "{dressing:?}");
    assert!(
        dressing.left.is_empty(),
        "nothing is left off now that a prop is worn: {:?}",
        dressing.left
    );
    assert_eq!(
        dressing
            .slot(or_wardrobe::Component::Jbib)
            .map(|j| model_of(&j.by)),
        Some("models/jbib_000_u.ydd"),
        "the jacket is still worn"
    );
    // A prop sits at the bone its anchor names, which the table of anchors gives and a scene reads.
    assert_eq!(Prop::Head.bone(), "SKEL_Head");
    assert_eq!(Prop::LeftWrist.bone(), "RB_L_ForeArmRoll");
}

#[test]
fn the_texture_of_the_variation_chosen_is_the_one_a_prop_wears() {
    let mut w = pack();
    w.garments[0]
        .variations
        .push(common::variation("textures/p_head_b.ytd"));
    let mut choice = wearing(&w, &[0]);
    choice.wear[0].variation = 1;
    let dressing = dress(&w, &BaseOutfit::default(), &choice);
    let Some(By::Garment { texture, .. }) = dressing.prop(Prop::Head).map(|p| &p.by) else {
        panic!("the hat is worn: {dressing:?}");
    };
    assert_eq!(texture.as_deref(), Some("textures/p_head_b.ytd"));
}

#[test]
fn a_second_hat_takes_the_place_of_the_first_and_says_so() {
    let mut w = pack();
    let mut second = garment(
        Ped::Male,
        P_HEAD,
        Some(1),
        "models/p_head_001.ydd",
        &["textures/p_head_c.ytd"],
    );
    second.label = Some("Second hat".into());
    w.garments.push(second);
    let dressing = dress(&w, &BaseOutfit::default(), &wearing(&w, &[0, 4]));

    assert_eq!(
        dressing.prop(Prop::Head).map(|p| model_of(&p.by)),
        Some("models/p_head_001.ydd"),
        "the hat put on last is the one worn"
    );
    assert_eq!(dressing.left.len(), 1, "{:?}", dressing.left);
    assert_eq!(dressing.left[0].garment, w.garments[0].id);
    assert!(dressing.left[0].why.contains("takes its slot"));
}

#[test]
fn a_prop_of_the_other_ped_a_place_held_and_the_ped_without_the_pack_wear_nothing() {
    let mut w = pack();
    let mut woman = garment(
        Ped::Female,
        EYES,
        Some(0),
        "models/p_eyes_f.ydd",
        &["textures/p_eyes_f.ytd"],
    );
    woman.label = Some("Her glasses".into());
    w.garments.push(woman);
    let mut held = garment(Ped::Male, WRIST, Some(1), "x.ydd", &[]);
    held.model = None;
    w.garments.push(held);

    let dressing = dress(&w, &BaseOutfit::default(), &wearing(&w, &[4, 5]));
    assert!(dressing.props.is_empty(), "{dressing:?}");
    let why: Vec<&str> = dressing.left.iter().map(|l| l.why.as_str()).collect();
    assert_eq!(
        why,
        ["dresses the other ped", "holds a place and has no model"]
    );

    let mut original = wearing(&w, &[0, 1]);
    original.original = true;
    assert!(
        dress(&w, &BaseOutfit::default(), &original)
            .props
            .is_empty(),
        "A is the ped as the game has it"
    );
}

#[test]
fn only_the_hat_that_is_worn_hides_the_hair_under_it() {
    let mut w = pack();
    hair::set_scale(&mut w.garments[0], Some(hair::HIDES)).unwrap();
    let mut second = garment(
        Ped::Male,
        P_HEAD,
        Some(1),
        "models/p_head_001.ydd",
        &["textures/p_head_c.ytd"],
    );
    hair::set_scale(&mut second, Some(-0.3)).unwrap();
    w.garments.push(second);

    let hides = dress(&w, &BaseOutfit::default(), &wearing(&w, &[0]));
    assert_eq!(hides.hidden.len(), 1, "{hides:?}");
    let taken = dress(&w, &BaseOutfit::default(), &wearing(&w, &[0, 4]));
    assert!(
        taken.hidden.is_empty(),
        "the hat that hid the hair was taken off by the one that does not: {taken:?}"
    );
}
