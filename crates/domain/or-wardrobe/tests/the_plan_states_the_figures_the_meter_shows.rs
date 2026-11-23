//! The plan states, besides its drawables and textures, what the meter of the Wardrobe shows against a build's ceilings:
//! the size of the metadata it writes, the shop entries, the entries of an archive and the memory the game streams for it.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::{garment, JBIB, LOWR, P_HEAD};
use or_io::decl::File;
use or_project::{Origin, PlannedFile};
use or_wardrobe::measure::Measure;
use or_wardrobe::{plan, Fact, Facts, Ped, Wardrobe};

fn figure(planned: &or_wardrobe::Planned, name: &str) -> u64 {
    planned
        .contribution
        .figures
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| {
            panic!(
                "no figure called {name}: {:?}",
                planned.contribution.figures
            )
        })
        .amount
}

/// Every folder a file of the plan sits in, however deep, and the files themselves: the entries of an archive that holds them.
fn entries_of(files: &[PlannedFile]) -> usize {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for file in files {
        let mut parts: Vec<&str> = file.path.split('/').collect();
        while !parts.is_empty() {
            names.insert(parts.join("/"));
            parts.pop();
        }
    }
    names.len()
}

fn counted(streaming: u64) -> Fact {
    let measure = Measure {
        streaming,
        detail: true,
        enhanced: false,
        textures: Vec::new(),
        drawables: Vec::new(),
    };
    Fact::File {
        bytes: 100,
        digest: Some("same".into()),
        read: None,
        measured: serde_json::to_value(measure).ok(),
    }
}

#[test]
fn the_plan_counts_the_metadata_the_shop_the_archive_and_the_memory_of_what_it_ships() {
    let mut w = Wardrobe::new("pack");
    let mut jacket = garment(Ped::Male, JBIB, Some(0), "a.ydd", &["a1.ytd", "a2.ytd"]);
    jacket.alternates = vec!["a_alt.ydd".into()];
    let trousers = garment(Ped::Female, LOWR, None, "b.ydd", &["b1.ytd"]);
    let cap = garment(Ped::Male, P_HEAD, None, "c.ydd", &["c1.ytd"]);
    let mut held = garment(Ped::Male, JBIB, Some(5), "x.ydd", &[]);
    held.model = None;
    w.garments.extend([jacket, trousers, cap, held]);

    // Every file the pack names is counted at a size of its own; the place held names none.
    let sizes = [
        ("a.ydd", 1_000),
        ("a_alt.ydd", 2_000),
        ("a1.ytd", 30_000),
        ("a2.ytd", 40_000),
        ("b.ydd", 500_000),
        ("b1.ytd", 600_000),
        ("c.ydd", 7_000_000),
        ("c1.ytd", 8_000_000),
    ];
    let mut facts = Facts::default();
    for (path, streaming) in sizes {
        facts.insert(path, counted(streaming));
    }
    let planned = plan(&w, &facts, "wardrobe", Path::new("."));

    // The memory is what the files of the garments that ship a model stream for, the file by file.
    let memory: u64 = sizes.iter().map(|(_, streaming)| streaming).sum();
    assert_eq!(figure(&planned, "clothing_memory"), memory);

    // The drawables are counted as the component slots' and the props' apart, which together are the total the ceilings are set on.
    let (slots, props) = (
        figure(&planned, "clothing_slots"),
        figure(&planned, "clothing_props"),
    );
    assert_eq!(props, 1, "the cap is the one prop");
    assert!(
        slots > 1,
        "the jacket, the trousers and the place held are components"
    );
    assert_eq!(slots + props, figure(&planned, "clothing_drawables"));

    // A ped wears one garment of a slot and one texture of it: the heaviest outfit is the male one, the jacket with its heavier texture and the cap with its one.
    let size = |name: &str| sizes.iter().find(|(n, _)| *n == name).unwrap().1;
    let male = size("a.ydd") + size("a1.ytd").max(size("a2.ytd")) + size("c.ydd") + size("c1.ytd");
    let female = size("b.ydd") + size("b1.ytd");
    assert!(male > female);
    assert_eq!(figure(&planned, "clothing_outfit_memory"), male);
    assert!(
        figure(&planned, "clothing_outfit_memory") < memory,
        "the whole pack streams more than any one outfit of it"
    );

    // The metadata is what the plan writes of its own: each ped's variation file, the creature file and each shop file.
    let documents = &planned.documents;
    let written: u64 = documents
        .variations
        .values()
        .map(|v| v.to_bytes().unwrap().len() as u64)
        .chain(
            documents
                .creature
                .iter()
                .map(|c| c.to_bytes().unwrap().len() as u64),
        )
        .chain(documents.shops.values().map(|s| s.len() as u64))
        .sum();
    assert!(written > 0);
    assert_eq!(figure(&planned, "clothing_metadata_bytes"), written);

    // A shop entry is a `uniqueNameHash`: one for each texture of a garment that ships a model.
    let entries: usize = documents
        .shops
        .values()
        .map(|text| text.matches("<uniqueNameHash>").count())
        .sum();
    assert_eq!(
        entries, 4,
        "the jacket's two, the trousers' one and the cap's one"
    );
    assert_eq!(figure(&planned, "clothing_shop_entries"), entries as u64);

    assert_eq!(
        figure(&planned, "clothing_archive_entries"),
        entries_of(&planned.contribution.files) as u64
    );
    assert!(
        planned
            .contribution
            .files
            .iter()
            .any(|f| matches!(f.origin, Origin::Generated(_))),
        "the files of the plan include the metadata the entries are counted over"
    );
}

#[test]
fn a_file_that_nobody_counted_adds_nothing_to_the_memory_and_a_pack_of_none_says_zero() {
    let mut w = Wardrobe::new("pack");
    w.garments
        .push(garment(Ped::Male, JBIB, Some(0), "a.ydd", &["a1.ytd"]));
    let mut facts = Facts::default();
    facts.insert("a.ydd", counted(1234));
    facts.insert("a1.ytd", Fact::file(100, Some("same".into())));
    let planned = plan(&w, &facts, "wardrobe", Path::new("."));
    assert_eq!(figure(&planned, "clothing_memory"), 1234);

    let empty = plan(
        &Wardrobe::new("pack"),
        &Facts::default(),
        "wardrobe",
        Path::new("."),
    );
    for name in [
        "clothing_memory",
        "clothing_metadata_bytes",
        "clothing_shop_entries",
        "clothing_archive_entries",
    ] {
        assert_eq!(figure(&empty, name), 0, "{name}");
    }
}

#[test]
fn the_heaviest_outfit_wears_the_heaviest_garment_of_each_slot_and_the_heaviest_ped_sets_the_figure(
) {
    let mut w = Wardrobe::new("pack");
    let light = garment(Ped::Male, JBIB, Some(0), "light.ydd", &["l1.ytd"]);
    let mut heavy = garment(Ped::Male, JBIB, Some(1), "heavy.ydd", &["h1.ytd", "h2.ytd"]);
    heavy.cloth = Some("heavy.yld".into());
    let trousers = garment(Ped::Male, LOWR, Some(0), "t.ydd", &["t1.ytd"]);
    let woman = garment(Ped::Female, JBIB, Some(0), "w.ydd", &["w1.ytd"]);
    w.garments.extend([light, heavy, trousers, woman]);
    let mut facts = Facts::default();
    let sizes = [
        ("light.ydd", 10),
        ("l1.ytd", 20),
        ("heavy.ydd", 1_000),
        ("h1.ytd", 2_000),
        ("h2.ytd", 3_000),
        ("heavy.yld", 400),
        ("t.ydd", 50_000),
        ("t1.ytd", 60_000),
        ("w.ydd", 9),
        ("w1.ytd", 8),
    ];
    for (path, streaming) in sizes {
        facts.insert(path, counted(streaming));
    }
    let size = |name: &str| sizes.iter().find(|(n, _)| *n == name).unwrap().1;

    let outfits = or_wardrobe::outfit::heaviest(&w, &or_wardrobe::allocate(&w), &facts);
    let male = outfits
        .iter()
        .find(|o| o.ped == Ped::Male)
        .expect("the male outfit");
    // The heavy jacket (its model, its cloth and its heavier texture) and the trousers: the light jacket is not worn with it.
    let wanted =
        size("heavy.ydd") + size("heavy.yld") + size("h2.ytd") + size("t.ydd") + size("t1.ytd");
    assert_eq!(male.bytes, wanted);
    assert_eq!(
        male.garments.len(),
        2,
        "one garment for each slot: {:?}",
        male.garments
    );
    assert!(male.garments.iter().any(|(_, id)| *id == w.garments[1].id));
    assert!(!male.garments.iter().any(|(_, id)| *id == w.garments[0].id));

    let female = outfits
        .iter()
        .find(|o| o.ped == Ped::Female)
        .expect("the female outfit");
    assert_eq!(female.bytes, size("w.ydd") + size("w1.ytd"));

    let planned = plan(&w, &facts, "wardrobe", Path::new("."));
    assert_eq!(
        figure(&planned, "clothing_outfit_memory"),
        wanted,
        "the heaviest ped sets it"
    );
}
