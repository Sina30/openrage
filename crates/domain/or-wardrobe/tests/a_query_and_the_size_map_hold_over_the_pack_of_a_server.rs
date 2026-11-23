//! A query and the size map hold over a clothing resource somebody built and a server runs: a query selects what the files and the pack state, the
//! weights of the garments and of what none owns add up to the files of the plan, and the dependencies list each file the garments name once.
//!
//! The reference is in no repository, so `OPENRAGE_REFERENCE_PACK` names its folder, the one that holds `stream/`. Without it this suite skips,
//! which is not a pass. The pack is only read: nothing is written in it.

mod common;

use std::path::PathBuf;

use or_wardrobe::import::{import, Options};
use or_wardrobe::measure::{measure, Measure};
use or_wardrobe::query::{Query, Subject};
use or_wardrobe::sizes::{dependencies, sizes, Leads};
use or_wardrobe::{inspect, plan, Depth};

const WHAT: &str = "a_query_and_the_size_map_hold_over_the_pack_of_a_server";

fn reference() -> Option<PathBuf> {
    let path = std::env::var_os("OPENRAGE_REFERENCE_PACK").map(PathBuf::from);
    match path.filter(|p| p.join("stream").is_dir()) {
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

#[test]
fn a_query_and_the_size_map_hold_over_the_pack_of_a_server() {
    let Some(root) = reference() else {
        return;
    };
    let imported = import(&root, &Options::default()).expect("the reference imports");
    let w = &imported.wardrobe;
    let facts = inspect(&root, w, Depth::Measure);
    let planned = plan(w, &facts, "wardrobe", &root);
    let subject = Subject::new(w, &planned.allocation, &planned.findings, &planned.counts);
    let select = |text: &str| Query::parse(text).expect(text).select(&subject);

    // What the pack states: a slot and a ped.
    let first = w
        .garments
        .iter()
        .find_map(|g| g.ped.zip(g.slot))
        .expect("a placed garment");
    let (ped, slot) = first;
    let expected: Vec<usize> = (0..w.garments.len())
        .filter(|&n| (w.garments[n].ped, w.garments[n].slot) == (Some(ped), Some(slot)))
        .collect();
    let line = format!(
        "slot:{slot} ped:{}",
        if ped == or_wardrobe::Ped::Male {
            "male"
        } else {
            "female"
        }
    );
    assert_eq!(select(&line), expected, "{line}");

    // What the files state, counted by the reader that opens each compiled model.
    let triangles: Vec<Option<u64>> = w
        .garments
        .iter()
        .map(|g| {
            let model = g.model.as_deref()?;
            let bytes =
                std::fs::read(root.join(model.replace('/', std::path::MAIN_SEPARATOR_STR))).ok()?;
            let counted = Measure::of(&measure(model, &bytes)?)?;
            (counted
                .drawables
                .iter()
                .filter_map(|d| d.lods[0])
                .map(|l| l.triangles))
            .reduce(|a, b| a + b)
        })
        .collect();
    let mut counted: Vec<u64> = triangles.iter().flatten().copied().collect();
    counted.sort_unstable();
    assert!(!counted.is_empty(), "the pack's models are counted");
    let median = counted[counted.len() / 2];
    let heavy: Vec<usize> = (0..w.garments.len())
        .filter(|&n| triangles[n].is_some_and(|t| t > median))
        .collect();
    assert_eq!(select(&format!("polys>{median}")), heavy);
    eprintln!(
        "{} garments, {} over {median} triangles",
        w.garments.len(),
        heavy.len()
    );

    // The size map adds up to the files of the plan, and no garment weighs more than its own files do on the disk.
    let map = sizes(w, &planned, &[]);
    let files: u64 = planned.contribution.files.iter().map(|f| f.bytes()).sum();
    assert_eq!(map.bytes, files);
    assert_eq!(
        map.garments.iter().map(|g| g.bytes).sum::<u64>() + map.rest.bytes,
        files
    );
    assert_eq!(map.files, planned.contribution.files.len() as u64);

    // Each file the garments name once, all of them in the pack.
    let deps = dependencies(w, &root, &facts);
    let named: std::collections::BTreeSet<&str> = w
        .garments
        .iter()
        .flat_map(|g| {
            g.model
                .iter()
                .chain(&g.alternates)
                .chain(g.cloth.iter())
                .chain(g.variations.iter().filter_map(|v| v.texture.as_ref()))
                .map(String::as_str)
        })
        .collect();
    assert_eq!(deps.len(), named.len());
    assert!(
        deps.iter().all(|d| d.leads == Leads::Inside),
        "every file the pack names is in it"
    );
}
