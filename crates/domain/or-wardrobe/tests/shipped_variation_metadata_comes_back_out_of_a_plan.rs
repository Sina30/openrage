//! Every staged ped variation document, imported as places held and planned again, comes back with
//! the same drawables, textures, component infos, props and anchors.
//!
//! This holds the metadata builder to 1,800 documents the game shipped, not to documents it wrote
//! itself: a rule that only the planner's own output satisfies fails here.

mod common;

use std::collections::BTreeMap;

use common::compare::same_meaning;
use or_io::decl::File;
use or_io::meta::CPedVariationInfo;
use or_wardrobe::import::{import, Options};
use or_wardrobe::{inspect, plan, Depth, Ped};

const WHAT: &str = "shipped_variation_metadata_comes_back_out_of_a_plan";

#[test]
fn a_staged_variation_document_survives_being_held_as_places_and_planned_again() {
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
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(origin.path())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    names.sort();
    for path in names {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(shipped) = CPedVariationInfo::from_bytes(&bytes) else {
            continue;
        };
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        *tally.entry("variation documents").or_default() += 1;
        // Anchors past the ninth have no slot of a freemode pack, and a selection set is no garment.
        if shipped
            .prop_info
            .a_prop_meta_data
            .iter()
            .any(|m| m.anchor_id > 8)
        {
            *tally.entry("left out: an anchor past hip").or_default() += 1;
            continue;
        }
        // ⚠ A pack has one data entry for each slot that has drawables; these documents keep entries no slot names, or name one twice.
        let named: std::collections::BTreeSet<u8> = shipped
            .avail_comp
            .iter()
            .copied()
            .filter(|i| *i != 255)
            .collect();
        if named.len() != shipped.a_component_data3.len()
            || named.len() != shipped.avail_comp.iter().filter(|i| **i != 255).count()
        {
            *tally
                .entry("left out: component data and slots do not pair one to one")
                .or_default() += 1;
            continue;
        }
        if shipped.b_is_super_lod {
            *tally
                .entry("left out: a super LOD, whose drawables wear no texture")
                .or_default() += 1;
            continue;
        }
        if shipped.avail_comp.len() != 12 {
            *tally.entry("left out: not twelve slots").or_default() += 1;
            continue;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mp_m_freemode_01_mp_m_shipped.ymt"), &bytes).unwrap();
        let imported =
            import(dir.path(), &Options::default()).expect("a variation document imports");
        let facts = inspect(dir.path(), &imported.wardrobe, Depth::Stat);
        let planned = plan(&imported.wardrobe, &facts, "wardrobe", dir.path());
        let Some(made) = planned.documents.variations.get(&Ped::Male) else {
            if shipped.a_component_data3.is_empty() && shipped.prop_info.a_prop_meta_data.is_empty()
            {
                *tally.entry("left out: it dresses nothing").or_default() += 1;
            } else {
                failures.push(format!("{file}: nothing was planned"));
            }
            continue;
        };
        let wrong = same_meaning(&shipped, made);
        if shipped.a_selection_sets.is_empty() {
            *tally.entry("compared").or_default() += 1;
        } else {
            *tally.entry("compared, selection sets aside").or_default() += 1;
        }
        let have: usize = made.comp_infos.len();
        if have > shipped.comp_infos.len() {
            *tally
                .entry("infos the planner adds for drawables the document gives none")
                .or_default() += 1;
        }
        if !wrong.is_empty() {
            failures.push(format!(
                "{file}: {}",
                wrong[..wrong.len().min(2)].join("; ")
            ));
        }
    }
    eprintln!("{tally:#?}");
    assert!(
        tally.get("variation documents").copied().unwrap_or(0) > 0,
        "no variation document is staged"
    );
    assert!(
        failures.is_empty(),
        "{} documents differ ({tally:?}):\n{}",
        failures.len(),
        failures
            .iter()
            .map(|f| f.chars().take(110).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
