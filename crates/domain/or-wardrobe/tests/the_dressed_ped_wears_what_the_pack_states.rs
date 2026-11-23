//! The pure query behind the dressed ped: which drawable and texture each slot wears.
//!
//! No renderer and no install: the pack is a list of states, the ped's own outfit comes from the
//! staged base ped's metadata, and what is held is a list of file names.

mod common;

use common::{garment, FEET, JBIB, LOWR, P_HEAD};
use or_io::decl::File;
use or_io::meta::CPedVariationInfo;
use or_wardrobe::dress::{dress, BaseOutfit, By, Choice, Dressing, Tone, Wear};
use or_wardrobe::{Component, Ped, Wardrobe};

const WHAT: &str = "the_dressed_ped_wears_what_the_pack_states";

fn pack() -> Wardrobe {
    let mut w = Wardrobe::new("demo");
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(0),
        "models/jbib_000_u.ydd",
        &[
            "textures/jbib_a.ytd",
            "textures/jbib_b.ytd",
            "textures/jbib_c.ytd",
        ],
    ));
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(1),
        "models/jbib_001_u.ydd",
        &["textures/jbib_1a.ytd"],
    ));
    w.garments.push(garment(
        Ped::Male,
        LOWR,
        Some(0),
        "models/lowr_000_r.ydd",
        &["textures/lowr_a.ytd"],
    ));
    w.garments.push(garment(
        Ped::Male,
        FEET,
        Some(0),
        "models/feet_000_u.ydd",
        &["textures/feet_a.ytd"],
    ));
    w.garments.push(garment(
        Ped::Female,
        JBIB,
        Some(0),
        "models/f_jbib_000_u.ydd",
        &["textures/f_jbib_a.ytd"],
    ));
    w.garments.push(garment(
        Ped::Male,
        P_HEAD,
        Some(0),
        "models/p_head_000.ydd",
        &["textures/p_head_a.ytd"],
    ));
    // A place held: it holds a number and ships nothing.
    let mut held = garment(Ped::Male, JBIB, Some(2), "x", &[]);
    held.model = None;
    w.garments.push(held);
    w
}

fn wear(w: &Wardrobe, at: usize, variation: usize) -> Wear {
    Wear {
        garment: w.garments[at].id.clone(),
        variation,
    }
}

/// The base ped's own variation metadata, as the corpus stages it.
fn base_of(ped: Ped) -> Option<BaseOutfit> {
    let origin = or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        "OPENRAGE_META_CORPUS",
        "binary",
    );
    if !origin.found() {
        or_test_support::skip_corpus(WHAT, "no metadata corpus is staged");
        return None;
    }
    let file = format!("rbm__platform__{}.ymt", ped.model());
    let bytes = std::fs::read(origin.path().join(&file)).ok().or_else(|| {
        or_test_support::skip_corpus(WHAT, format!("{file} is not staged"));
        None
    })?;
    let info = CPedVariationInfo::from_bytes(&bytes).expect("the base ped's metadata opens");
    Some(BaseOutfit::of(&info))
}

#[test]
fn a_garment_wears_the_model_and_the_texture_its_slot_states() {
    let w = pack();
    let mut choice = Choice::new(Ped::Male);
    choice.wear = vec![wear(&w, 0, 1)];
    let dressing = dress(&w, &BaseOutfit::default(), &choice);

    assert_eq!(dressing.worn.len(), 1, "{dressing:?}");
    let worn = &dressing.worn[0];
    assert_eq!(worn.slot, Component::Jbib);
    assert_eq!(
        worn.by,
        By::Garment {
            id: w.garments[0].id.clone(),
            model: w.garments[0].model.clone().unwrap(),
            texture: w.garments[0].variations[1].texture.clone(),
        },
        "the texture is the second variation's, not the first"
    );
}

#[test]
fn a_later_garment_takes_the_slot_of_an_earlier_one_and_the_earlier_is_listed() {
    let w = pack();
    let mut choice = Choice::new(Ped::Male);
    choice.wear = vec![wear(&w, 0, 0), wear(&w, 1, 0)];
    let dressing = dress(&w, &BaseOutfit::default(), &choice);

    let Some(By::Garment { id, .. }) = dressing.slot(Component::Jbib).map(|w| &w.by) else {
        panic!("jbib is worn by a garment: {dressing:?}");
    };
    assert_eq!(*id, w.garments[1].id);
    assert_eq!(dressing.left.len(), 1, "{dressing:?}");
    assert_eq!(dressing.left[0].garment, w.garments[0].id);
}

#[test]
fn a_garment_of_the_other_ped_and_a_place_held_are_listed_and_not_worn_and_a_prop_is_worn_at_its_anchor(
) {
    let w = pack();
    let mut choice = Choice::new(Ped::Male);
    choice.wear = vec![wear(&w, 4, 0), wear(&w, 5, 0), wear(&w, 6, 0)];
    let dressing = dress(&w, &BaseOutfit::default(), &choice);

    assert!(dressing.worn.is_empty(), "{dressing:?}");
    let listed: Vec<&str> = dressing.left.iter().map(|l| l.garment.as_str()).collect();
    for at in [4, 6] {
        assert!(
            listed.contains(&w.garments[at].id.as_str()),
            "{at}: {listed:?}"
        );
    }
    assert!(
        !listed.contains(&w.garments[5].id.as_str()),
        "the hat is a prop and a prop is worn: {listed:?}"
    );
    assert_eq!(dressing.props.len(), 1, "{dressing:?}");
    assert_eq!(dressing.props[0].prop, or_wardrobe::Prop::Head);
    assert!(
        dressing.left.iter().all(|l| !l.why.is_empty()),
        "each says why: {dressing:?}"
    );
}

#[test]
fn an_unknown_garment_and_a_variation_past_the_end_wear_what_they_can() {
    let w = pack();
    let mut choice = Choice::new(Ped::Male);
    choice.wear = vec![
        Wear {
            garment: "gone".into(),
            variation: 0,
        },
        wear(&w, 2, 9),
    ];
    let dressing = dress(&w, &BaseOutfit::default(), &choice);
    assert_eq!(dressing.left.len(), 1, "{dressing:?}");
    let Some(By::Garment { texture, .. }) = dressing.slot(Component::Lowr).map(|w| &w.by) else {
        panic!("lowr is worn: {dressing:?}");
    };
    assert_eq!(
        *texture, None,
        "a variation the garment lacks is no texture"
    );
}

#[test]
fn wearing_a_garment_changes_exactly_its_slot_and_the_original_is_the_ped_without_the_pack() {
    let Some(base) = base_of(Ped::Male) else {
        return;
    };
    assert!(!base.is_empty(), "the base ped dresses its slots");
    let w = pack();
    let bare = dress(&w, &base, &Choice::new(Ped::Male));
    assert!(
        bare.worn.iter().all(|w| matches!(w.by, By::Base { .. })),
        "nothing chosen wears only the ped: {bare:?}"
    );

    let mut choice = Choice::new(Ped::Male);
    choice.wear = vec![wear(&w, 2, 0)];
    let dressed = dress(&w, &base, &choice);
    let changed: Vec<Component> = Component::ALL
        .into_iter()
        .filter(|c| bare.slot(*c) != dressed.slot(*c))
        .collect();
    assert_eq!(changed, [Component::Lowr], "{dressed:?}");

    choice.original = true;
    assert_eq!(
        dress(&w, &base, &choice),
        bare,
        "A of A/B is the ped as the game has it"
    );
}

#[test]
fn the_base_outfit_is_drawable_0_of_each_slot_with_the_skin_its_first_texture_states() {
    for ped in Ped::ALL {
        let Some(base) = base_of(ped) else {
            return;
        };
        let origin = or_test_support::owned_corpus_origin(
            "or-meta",
            env!("CARGO_MANIFEST_DIR"),
            "OPENRAGE_META_CORPUS",
            "binary",
        );
        let bytes = std::fs::read(
            origin
                .path()
                .join(format!("rbm__platform__{}.ymt", ped.model())),
        )
        .unwrap();
        let info = CPedVariationInfo::from_bytes(&bytes).unwrap();
        let dressing = dress(&Wardrobe::new("x"), &base, &Choice::new(ped));
        for component in Component::ALL {
            let drawable = info
                .avail_comp
                .get(usize::from(component.id()))
                .and_then(|i| info.a_component_data3.get(usize::from(*i)))
                .and_then(|d| d.a_drawbl_data3.first());
            let Some(drawable) = drawable else {
                assert!(dressing.slot(component).is_none(), "{ped} {component:?}");
                continue;
            };
            let letter = if drawable.a_tex_data[0].tex_id == 0 {
                'u'
            } else {
                'r'
            };
            let Some(By::Base { model, texture }) = dressing.slot(component).map(|w| &w.by) else {
                panic!("{ped} {component:?} wears the ped's own drawable: {dressing:?}");
            };
            assert_eq!(*model, format!("{}_000_{letter}.ydd", component.name()));
            assert!(
                texture
                    .as_deref()
                    .is_some_and(|t| t.starts_with(&format!("{}_diff_000_a_", component.name()))),
                "{texture:?}"
            );
        }
    }
}

#[test]
fn a_tone_chooses_the_base_head_and_its_texture_and_leaves_the_body() {
    let Some(base) = base_of(Ped::Female) else {
        return;
    };
    let w = Wardrobe::new("x");
    let white = dress(&w, &base, &Choice::new(Ped::Female));
    for tone in Tone::ALL {
        let mut choice = Choice::new(Ped::Female);
        choice.tone = tone;
        let dressing: Dressing = dress(&w, &base, &choice);
        let Some(By::Base { model, texture }) = dressing.slot(Component::Head).map(|w| &w.by)
        else {
            panic!("the head is the ped's: {dressing:?}");
        };
        assert_eq!(*model, format!("head_{:03}_r.ydd", tone.head()));
        assert_eq!(
            *texture,
            Some(format!(
                "head_diff_{:03}_a_{}.ytd",
                tone.head(),
                tone.word()
            ))
        );
        for component in Component::ALL.into_iter().filter(|c| *c != Component::Head) {
            assert_eq!(
                dressing.slot(component),
                white.slot(component),
                "{tone:?} {component:?}"
            );
        }
    }
}

/// Every garment of a pack somebody built and a server runs, worn alone: its slot wears the model and the texture the pack states, and those files are in the pack.
///
/// The pack is named by `OPENRAGE_REFERENCE_PACK` and only read; without it this skips, which is not a pass.
#[test]
fn for_a_reference_pack_the_query_selects_the_drawables_and_textures_its_slots_state() {
    let Some(root) = std::env::var_os("OPENRAGE_REFERENCE_PACK")
        .map(std::path::PathBuf::from)
        .filter(|p| p.join("stream").is_dir())
    else {
        or_test_support::skip_corpus(
            WHAT,
            "OPENRAGE_REFERENCE_PACK names no clothing resource; it is a pack of the owner's server, in no repository",
        );
        return;
    };
    let imported = or_wardrobe::import::import(&root, &or_wardrobe::import::Options::default())
        .expect("the reference imports");
    let w = imported.wardrobe;
    let (mut checked, mut missing) = (0usize, Vec::new());
    for garment in &w.garments {
        let (Some(ped), Some(or_wardrobe::Slot::Component(component)), Some(model)) =
            (garment.ped, garment.slot, garment.model.as_ref())
        else {
            continue;
        };
        let mut choice = Choice::new(ped);
        choice.wear = vec![Wear {
            garment: garment.id.clone(),
            variation: 0,
        }];
        let dressing = dress(&w, &BaseOutfit::default(), &choice);
        let Some(By::Garment {
            model: worn,
            texture,
            ..
        }) = dressing.slot(component).map(|w| &w.by)
        else {
            panic!("{}: the slot is not worn by it: {dressing:?}", garment.id);
        };
        assert_eq!(worn, model);
        assert_eq!(
            *texture,
            garment.variations.first().and_then(|v| v.texture.clone())
        );
        checked += 1;
        if !root.join(model).is_file() {
            missing.push(model.clone());
        }
    }
    assert!(checked > 1000, "{checked} garments were worn alone");
    assert!(
        missing.is_empty(),
        "{} of {checked} worn models are not files of the pack, the first {:?}",
        missing.len(),
        &missing[..missing.len().min(3)]
    );
}

#[test]
fn a_garment_wears_the_alternate_model_chosen_for_it_with_its_own_texture_and_a_number_it_lacks_wears_its_model(
) {
    let mut w = pack();
    w.garments[0].alternates = vec![
        "models/jbib_000_u_1.ydd".into(),
        "models/jbib_000_u_2.ydd".into(),
    ];
    w.garments[5].alternates = vec!["models/p_head_000_1.ydd".into()];
    let jacket = w.garments[0].id.clone();
    let hat = w.garments[5].id.clone();
    let worn = |chosen: Option<(&str, usize)>| {
        let mut choice = Choice::new(Ped::Male);
        choice.wear = vec![wear(&w, 0, 1), wear(&w, 5, 0)];
        if let Some((id, n)) = chosen {
            choice.alternates.insert(id.to_string(), n);
        }
        let dressing = dress(&w, &BaseOutfit::default(), &choice);
        let Some(By::Garment { model, texture, .. }) =
            dressing.slot(Component::Jbib).map(|w| &w.by)
        else {
            panic!("the jacket is worn: {dressing:?}");
        };
        let Some(By::Garment {
            model: hat_model, ..
        }) = dressing.props.first().map(|p| &p.by)
        else {
            panic!("the hat is worn: {dressing:?}");
        };
        (model.clone(), texture.clone(), hat_model.clone())
    };

    let (main, texture, hat_main) = worn(None);
    assert_eq!(main, "models/jbib_000_u.ydd");
    assert_eq!(hat_main, "models/p_head_000.ydd");
    assert_eq!(worn(Some((&jacket, 0))).0, main, "0 is the model itself");
    assert_eq!(worn(Some((&jacket, 1))).0, "models/jbib_000_u_1.ydd");
    assert_eq!(worn(Some((&jacket, 2))).0, "models/jbib_000_u_2.ydd");
    assert_eq!(
        worn(Some((&jacket, 3))).0,
        main,
        "a number it has no model for"
    );
    assert_eq!(
        worn(Some((&jacket, 2))).1,
        texture,
        "the texture stays the variation's"
    );
    assert_eq!(
        worn(Some((&hat, 1))).2,
        "models/p_head_000_1.ydd",
        "a prop has alternates too"
    );
    assert_eq!(
        worn(Some((&hat, 1))).0,
        main,
        "the other garment is not changed"
    );
}
