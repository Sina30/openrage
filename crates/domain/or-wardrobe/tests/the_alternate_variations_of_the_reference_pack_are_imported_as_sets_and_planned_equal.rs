//! The alternate variations of the owner's server pack, imported as sets of the pack's garments and planned again.
//!
//! The pack is in no repository, so `OPENRAGE_REFERENCE_PACK` names its folder, the one that holds `stream/` and `data/`;
//! without it this suite skips, which is not a pass. The pack is only read, and its 3.8 GB of models and textures are not
//! (the plan is made at `Depth::Stat`).

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use or_io::decl::File;
use or_io::meta::CAlternateVariations;
use or_wardrobe::import::{import, Options};
use or_wardrobe::{inspect, plan, Depth, Ped};

const WHAT: &str =
    "the_alternate_variations_of_the_reference_pack_are_imported_as_sets_and_planned_equal";

fn reference() -> Option<PathBuf> {
    let path = std::env::var_os("OPENRAGE_REFERENCE_PACK").map(PathBuf::from);
    match path.filter(|p| p.join("stream").is_dir() && p.join("data").is_dir()) {
        Some(path) => Some(path),
        None => {
            or_test_support::skip_corpus(
                WHAT,
                "OPENRAGE_REFERENCE_PACK names no clothing resource; it is a pack of the owner's server, in no repository",
            );
            None
        }
    }
}

type Source = (u8, u8, u8, u32);
type Key = (u8, u8, u8, u32);

/// What a root means: for each ped, each (component, index, alternate, collection) and the set of assets that cause it.
fn meaning(root: &CAlternateVariations) -> BTreeMap<(u32, Key), BTreeSet<Source>> {
    let mut found: BTreeMap<(u32, Key), BTreeSet<Source>> = BTreeMap::new();
    for ped in &root.peds {
        for s in &ped.switches {
            let key = (s.component, s.index, s.alt, s.dlc_name_hash.hash);
            let sources = found.entry((ped.name.hash, key)).or_default();
            for a in &s.source_assets {
                sources.insert((a.component, a.anchor, a.index, a.dlc_name_hash.hash));
            }
        }
    }
    found
}

#[test]
fn the_pack_imports_as_a_few_sets_and_plans_the_switches_it_states() {
    let Some(root) = reference() else {
        return;
    };
    let imported = import(&root, &Options::default()).expect("the reference imports");
    let read = &imported.report.switches;
    eprintln!(
        "{} file(s), {} set(s), {} switch(es), {} source(s) of the pack, {} outside it, {} left out",
        read.files.len(),
        read.sets,
        read.switches,
        read.inside,
        read.outside,
        read.left_out.len()
    );
    assert_eq!(read.files, ["data/pedalternatevariations.meta"]);
    assert_eq!(read.switches, 372, "every switch of the file is in a set");
    assert!(
        imported.wardrobe.alternates.len() <= 8,
        "{} sets: the switches of a pack are a few groups of one set of garments each",
        imported.wardrobe.alternates.len()
    );
    assert!(read.inside > 0 && read.outside > 0);
    // Every garment named by a set is a garment of the pack, of the set's ped.
    for set in &imported.wardrobe.alternates {
        for asset in &set.sources {
            if let or_wardrobe::alternate::Asset::Garment { garment } = asset {
                let g = imported
                    .wardrobe
                    .garment(garment)
                    .expect("a garment of the pack");
                assert_eq!(g.ped, Some(set.ped));
            }
        }
    }

    // Planned at the depth that reads no model, the alternate variations mean what the file meant.
    let facts = inspect(&root, &imported.wardrobe, Depth::Stat);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", &root);
    let made = planned
        .documents
        .alternates
        .as_ref()
        .expect("the plan writes the alternate variations");
    let text = std::fs::read_to_string(root.join("data/pedalternatevariations.meta")).unwrap();
    let shipped = CAlternateVariations::from_bytes(text.as_bytes()).unwrap();
    assert!(
        meaning(&shipped) == meaning(made),
        "the plan's alternate variations are not the pack's"
    );
    // It is registered under the type the game reads it by.
    assert!(planned
        .contribution
        .manifest
        .iter()
        .any(|m| m.key == "data_file"
            && m.values.first().map(String::as_str) == Some("ALTERNATE_VARIATIONS_FILE")));
    // And the canonical form of what the plan wrote is a fixed point.
    let once = made.to_bytes().unwrap();
    let again = CAlternateVariations::from_bytes(&once).unwrap();
    assert!(again == *made);
    assert!(again.to_bytes().unwrap() == once);
    let _ = Ped::Male;
}

#[test]
fn a_garment_of_the_reference_states_the_skin_bit_exactly_when_its_skin_is_the_race_skin() {
    use or_wardrobe::mask::{self, Property};
    use or_wardrobe::Skin;
    let Some(root) = reference() else {
        return;
    };
    let imported = import(&root, &Options::default()).expect("the reference imports");
    let (mut race, mut universal) = (0usize, 0usize);
    for g in imported
        .wardrobe
        .garments
        .iter()
        .filter(|g| !g.is_reserved() && g.slot.is_some_and(|s| !s.is_prop()))
    {
        let bit = mask::has_property(g, Property::RaceTex);
        assert_eq!(bit, g.skin == Skin::Race, "{:?}", g.model);
        assert!(!mask::has_property(g, Property::MatchPrev), "{:?}", g.model);
        match g.skin {
            Skin::Race => race += 1,
            Skin::Universal => universal += 1,
        }
    }
    eprintln!("{race} garments of the race skin state the skin bit, {universal} of the universal skin do not");
    assert!(race > 0 && universal > 0);
    // So the rule that holds a mask to its skin finds nothing in a pack built by Durty Cloth Tool.
    let facts = inspect(&root, &imported.wardrobe, Depth::Stat);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", &root);
    assert!(planned.findings.iter().all(|f| f.code != "mask-skin"));
}
