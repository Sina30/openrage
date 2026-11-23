//! The Durty-compatible shop names are an option of the pack, off by default: with it an entry is
//! named as Durty Cloth Tool names it, and without it as OpenRage names it.
//!
//! ⚠ The names are the format of the owner's Vestium source, which says it matched them against
//! Durty-built packs; no Durty-built resource is staged here, so what these checks hold is that the
//! option writes that format and nothing else changes, not that Durty writes it.

use std::collections::BTreeSet;

use or_wardrobe::{
    facts::named_paths, metadata, plan, Component, Facts, Garment, Ped, Prop, Slot, Wardrobe,
};

fn placed(ped: Ped, slot: Slot, position: u32, textures: usize) -> Garment {
    let mut g = Garment::new();
    g.ped = Some(ped);
    g.slot = Some(slot);
    g.position = Some(position);
    g.model = Some(format!("{}_{position}.ydd", slot.name()));
    g.variations = (0..textures)
        .map(|t| or_wardrobe::Variation {
            texture: Some(format!("{}_{position}_{t}.ytd", slot.name())),
            ..Default::default()
        })
        .collect();
    g
}

fn pack(durty: bool) -> Wardrobe {
    let mut w = Wardrobe::new("police");
    w.shop.durty_hashes = durty;
    w.garments = vec![
        placed(Ped::Male, Slot::Component(Component::Jbib), 0, 2),
        placed(Ped::Male, Slot::Component(Component::Lowr), 3, 1),
        placed(Ped::Male, Slot::Prop(Prop::LeftWrist), 2, 2),
        placed(Ped::Female, Slot::Component(Component::Feet), 1, 1),
    ];
    w
}

fn shop(w: &Wardrobe, ped: Ped) -> String {
    let facts = Facts::from_files(named_paths(w).map(|p| (p, 10, p)));
    let planned = plan(w, &facts, "wardrobe", std::path::Path::new("."));
    assert!(
        planned.contribution.problems.is_empty(),
        "{:?}",
        planned.contribution.problems
    );
    planned.documents.shops[&ped].clone()
}

fn names(text: &str) -> Vec<String> {
    text.split("<uniqueNameHash>")
        .skip(1)
        .map(|rest| rest.split("</uniqueNameHash>").next().unwrap().to_string())
        .collect()
}

#[test]
fn the_option_is_off_unless_the_pack_asks_and_the_file_says_so_only_then() {
    let off = pack(false);
    assert!(!off.shop.durty_hashes);
    assert!(!off.to_json().contains("shop"), "{}", off.to_json());

    let on = pack(true);
    let text = on.to_json();
    assert!(text.contains("\"durty_hashes\": true"), "{text}");
    assert_eq!(Wardrobe::from_json(text.as_bytes(), "x").unwrap(), on);
}

#[test]
fn without_the_option_an_entry_is_named_for_its_collection_slot_position_and_texture() {
    let male = names(&shop(&pack(false), Ped::Male));
    assert_eq!(
        male,
        [
            "MP_M_POLICE_LOWR_003_0",
            "MP_M_POLICE_JBIB_000_0",
            "MP_M_POLICE_JBIB_000_1",
            "MP_M_POLICE_P_LWRIST_002_0",
            "MP_M_POLICE_P_LWRIST_002_1"
        ]
    );
}

#[test]
fn with_the_option_an_entry_is_named_as_durty_names_it_and_only_the_names_change() {
    let (off, on) = (pack(false), pack(true));
    let male = shop(&on, Ped::Male);
    assert_eq!(
        names(&male),
        [
            "CLO_DCT_LEGS_3_0_M",
            "CLO_DCT_TOP_0_0_M",
            "CLO_DCT_TOP_0_1_M",
            "CLO_DCT_PROPLWRIST_2_0_M",
            "CLO_DCT_PROPLWRIST_2_1_M"
        ]
    );
    assert_eq!(names(&shop(&on, Ped::Female)), ["CLO_DCT_SHOES_1_0_F"]);

    // The same entries in the order of the slots' numbers, and the same text around the names.
    let strip = |text: &str| {
        let mut out = text.to_string();
        for name in names(text) {
            out = out.replace(&name, "N");
        }
        out
    };
    assert_eq!(strip(&male), strip(&shop(&off, Ped::Male)));
}

#[test]
fn every_component_has_a_word_of_its_own_and_so_does_every_prop() {
    let words: BTreeSet<String> = Component::ALL
        .into_iter()
        .map(|c| metadata::durty_name(Ped::Male, Slot::Component(c), 0, 0))
        .collect();
    assert_eq!(words.len(), Component::ALL.len(), "{words:?}");
    let words: BTreeSet<String> = Prop::ALL
        .into_iter()
        .map(|p| metadata::durty_name(Ped::Male, Slot::Prop(p), 0, 0))
        .collect();
    assert_eq!(words.len(), Prop::ALL.len(), "{words:?}");
    // The names of a ped's entries never meet: each place and texture is one name.
    let male = names(&shop(&pack(true), Ped::Male));
    assert_eq!(male.iter().collect::<BTreeSet<_>>().len(), male.len());
}

#[test]
fn a_shop_file_with_the_option_reloads_through_the_metadata_reader() {
    let text = shop(&pack(true), Ped::Male);
    let doc = or_io::meta::MetaDocument::parse(or_io::meta::default_library(), text.into_bytes())
        .expect("the shop metadata reloads");
    assert!(!doc.instance.type_name.is_empty());
}
