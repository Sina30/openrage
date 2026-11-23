//! Heels: what a feet drawable states, what a build writes for it, and how high the preview raises the ped.
//!
//! The staged creature metadata gives every heel drawable the one expression 4 shape, and the
//! staged variation metadata states the same drawables' value in the fifth expression modifier;
//! the nearby tests hold the engine's write to both.

mod common;

use common::{garment, FEET, JBIB};
use or_io::decl::File;
use or_io::meta::{CCreatureMetaData, CPedVariationInfo};
use or_wardrobe::dress::{dress, BaseOutfit, Choice, Wear, METRES_PER_HEEL_UNIT};
use or_wardrobe::edit::{apply, Edit};
use or_wardrobe::{heels, inspect, plan, Component, Depth, Ped, Wardrobe};

const WHAT: &str = "a_feet_drawable_states_heels_in_expression_4_and_the_preview_is_raised_by_them";

fn pack() -> Wardrobe {
    let mut w = Wardrobe::new("demo");
    w.garments.push(garment(
        Ped::Female,
        FEET,
        Some(0),
        "models/feet_000_u.ydd",
        &["textures/feet_a.ytd"],
    ));
    w.garments.push(garment(
        Ped::Female,
        JBIB,
        Some(0),
        "models/jbib_000_u.ydd",
        &["textures/jbib_a.ytd"],
    ));
    w
}

#[test]
fn the_heel_flag_raises_the_ped_by_its_stated_height() {
    let mut w = pack();
    let feet = w.garments[0].id.clone();
    apply(&mut w, &feet, Edit::Heels(Some(1.5))).expect("a feet drawable takes heels");
    assert_eq!(heels::of(&w.garments[0]), Some(1.5));

    let mut choice = Choice::new(Ped::Female);
    choice.wear = vec![Wear {
        garment: feet,
        variation: 0,
    }];
    let dressing = dress(&w, &BaseOutfit::default(), &choice);
    assert_eq!(dressing.lift, 1.5 * METRES_PER_HEEL_UNIT);

    choice.heels = false;
    assert_eq!(
        dress(&w, &BaseOutfit::default(), &choice).lift,
        0.0,
        "the Heels button off leaves the ped on the floor"
    );
    choice.heels = true;
    choice.original = true;
    assert_eq!(
        dress(&w, &BaseOutfit::default(), &choice).lift,
        0.0,
        "the original has the ped's own shoes"
    );
}

#[test]
fn a_garment_that_is_not_shoes_takes_no_heels_and_a_value_that_is_no_number_is_refused() {
    let mut w = pack();
    let (feet, jacket) = (w.garments[0].id.clone(), w.garments[1].id.clone());
    assert!(apply(&mut w, &jacket, Edit::Heels(Some(1.0))).is_err());
    assert_eq!(heels::of(&w.garments[1]), None);
    for bad in [f32::NAN, f32::INFINITY, 40.0, -40.0] {
        assert!(
            apply(&mut w, &feet, Edit::Heels(Some(bad))).is_err(),
            "{bad}"
        );
    }
    assert_eq!(heels::of(&w.garments[0]), None, "a refusal changes nothing");
}

#[test]
fn heels_are_written_as_the_fifth_modifier_and_expression_4_and_taking_them_off_writes_neither() {
    let mut w = pack();
    let feet = w.garments[0].id.clone();
    let dir = tempfile::tempdir().unwrap();
    let written = |w: &Wardrobe| {
        let facts = inspect(dir.path(), w, Depth::Stat);
        plan(w, &facts, "wardrobe", dir.path())
    };

    apply(&mut w, &feet, Edit::Heels(Some(1.0))).unwrap();
    let planned = written(&w);
    let variation = &planned.documents.variations[&Ped::Female];
    let info = variation
        .comp_infos
        .iter()
        .find(|i| i.ped_xml_comp_idx == Component::Feet.id())
        .expect("the feet drawable has its component info");
    assert_eq!(info.ped_xml_expression_mods[4], 1.0);
    assert!(info.ped_xml_expression_mods[..4].iter().all(|m| *m == 0.0));
    let creature = planned
        .documents
        .creature
        .as_ref()
        .expect("an expression is stated");
    let expressions: Vec<_> = creature
        .ped_comp_expressions
        .iter()
        .map(|e| (e.ped_comp_id, e.ped_comp_expression_index))
        .collect();
    assert_eq!(
        expressions,
        [(u32::from(Component::Feet.id()), heels::EXPRESSION)]
    );

    apply(&mut w, &feet, Edit::Heels(None)).unwrap();
    assert_eq!(heels::of(&w.garments[0]), None);
    let planned = written(&w);
    let info = planned.documents.variations[&Ped::Female]
        .comp_infos
        .iter()
        .find(|i| i.ped_xml_comp_idx == Component::Feet.id());
    assert!(info.is_none_or(|i| i.ped_xml_expression_mods.iter().all(|m| *m == 0.0)));
    assert!(
        planned.documents.creature.is_none(),
        "no expression is left"
    );
}

/// The staged documents of one DLC: every feet drawable whose creature metadata states expression 4.
#[test]
fn the_expression_a_heel_is_written_with_is_the_one_staged_feet_drawables_state() {
    let origin = or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        "OPENRAGE_META_CORPUS",
        "binary",
    );
    if !origin.found() {
        or_test_support::skip_corpus(WHAT, "no metadata corpus is staged");
        return;
    }
    let mut shapes: std::collections::BTreeMap<String, usize> = Default::default();
    let mut names: Vec<_> = std::fs::read_dir(origin.path())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    names.sort();
    for path in names {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        if !file.starts_with("rbf__") {
            continue;
        }
        let Ok(creature) = CCreatureMetaData::from_bytes(&std::fs::read(&path).unwrap()) else {
            continue;
        };
        for e in &creature.ped_comp_expressions {
            if e.ped_comp_id == u32::from(Component::Feet.id())
                && e.ped_comp_expression_index == heels::EXPRESSION
            {
                let shape = format!("{:?}{:?}{:?}", e.tracks, e.ids, e.types);
                *shapes
                    .entry(format!("{shape}{:?}", e.components))
                    .or_default() += 1;
            }
        }
    }
    let (most, count) = shapes
        .iter()
        .max_by_key(|(_, n)| **n)
        .expect("heel expressions are staged");
    let ours = heels::expression();
    let written = format!(
        "{:?}{:?}{:?}{:?}",
        ours.tracks, ours.ids, ours.types, ours.components
    );
    assert_eq!(
        *most,
        written,
        "the shape most staged heels state ({count} of {}) is the one written",
        shapes.values().sum::<usize>()
    );
}

/// A feet drawable that states expression 4 states a value for it in the same document set.
#[test]
fn a_staged_heel_states_its_value_in_the_fifth_modifier() {
    let origin = or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        "OPENRAGE_META_CORPUS",
        "binary",
    );
    if !origin.found() {
        or_test_support::skip_corpus(WHAT, "no metadata corpus is staged");
        return;
    }
    let mut stated = 0;
    let mut feet = 0;
    for entry in std::fs::read_dir(origin.path()).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("rbm__") {
            continue;
        }
        let Ok(info) = CPedVariationInfo::from_bytes(&std::fs::read(entry.path()).unwrap()) else {
            continue;
        };
        for i in info
            .comp_infos
            .iter()
            .filter(|i| i.ped_xml_comp_idx == Component::Feet.id())
        {
            feet += 1;
            if i.ped_xml_expression_mods.get(4).is_some_and(|m| *m != 0.0) {
                stated += 1;
                assert!(
                    i.ped_xml_expression_mods[..4].iter().all(|m| *m == 0.0),
                    "{name}: a heel states the fifth modifier alone"
                );
            }
        }
    }
    assert!(
        stated > 100,
        "{stated} of {feet} staged feet drawables state heels"
    );
}
