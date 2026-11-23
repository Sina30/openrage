//! Fix all safe adds the medium and low levels a model lacks only when it is asked to, in the same one step of Undo as its other steps,
//! and a second run changes nothing.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, put_bytes, snapshot, JBIB};
use or_io::decl::{File, Gen8};
use or_io::gta5::{GtaDrawable, PgDictionary};
use or_source::game_import::{decompile, Facts as GameFacts};
use or_source::sample;
use or_wardrobe::edit::{undo_revision, Watch};
use or_wardrobe::fix::{all_safe, all_safe_also, Also, Step};
use or_wardrobe::{allocate, check, inspect, Depth, Ped, Wardrobe};

/// A drawable dictionary like the sample's, whose levels switch at the 9998 of a ped component's, which is where the doctor makes levels.
fn ped_component(name: &str, vertices: usize, shift: f32) -> Vec<u8> {
    let mut dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(
        &sample::drawable_dictionary(name, vertices, shift),
    )
    .unwrap();
    for drawable in dictionary.entries.iter_mut().flatten() {
        drawable.base.lod_group.lod_distances = [9998.0; 4];
    }
    dictionary.to_bytes().unwrap()
}

/// A pack of two jackets, one a compiled model and one a drawables source, each of one level of detail only.
fn a_pack(dir: &Path) -> PathBuf {
    let pack = dir.join("clothing");
    put_bytes(
        &pack,
        "models/jbib_000_u.ydd",
        &ped_component("jbib_000_u", 90, 0.0),
    );
    std::fs::create_dir_all(pack.join("models")).unwrap();
    decompile(
        &ped_component("jbib_001_u", 90, 1.0),
        "jbib_001_u.ydd",
        &pack.join("models"),
        &GameFacts::default(),
    )
    .expect("a source");
    let mut w = Wardrobe::new("demo");
    for (n, model) in ["models/jbib_000_u.ydd", "models/jbib_001_u.drawables.src"]
        .iter()
        .enumerate()
    {
        w.garments
            .push(garment(Ped::Male, JBIB, Some(n as u32), model, &[]));
    }
    w.write(&pack).unwrap();
    pack
}

/// How many garments the rule `lod-missing` finds at fault in the pack as the disk says it.
fn missing(pack: &Path) -> usize {
    let w = Wardrobe::read(pack).unwrap();
    check(&w, &allocate(&w), &inspect(pack, &w, Depth::Measure))
        .iter()
        .filter(|f| f.code == "lod-missing")
        .count()
}

#[test]
fn levels_are_added_only_when_asked_in_the_one_undo_and_a_second_run_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let pack = a_pack(dir.path());
    let first = snapshot(&pack);
    assert_eq!(missing(&pack), 2, "both models lack a medium and a low");

    // Not asked: the four steps run, no model is touched, and the finding stands.
    let (plain, revision) = all_safe(&pack, &Watch::default()).unwrap();
    assert_eq!(
        plain.steps.iter().map(|s| s.step).collect::<Vec<_>>(),
        [Step::Repair, Step::Textures, Step::Order, Step::Mask]
    );
    assert!(revision.is_none(), "{plain:?}");
    assert_eq!(snapshot(&pack), first);
    assert_eq!(missing(&pack), 2);

    // Asked: a fifth step makes the levels of both models, and the finding is gone.
    let also = Also {
        levels: Some(vec![(1, 0.35), (2, 0.08)]),
    };
    let (report, revision) = all_safe_also(&pack, &Watch::default(), &also).unwrap();
    let revision =
        revision.unwrap_or_else(|| panic!("the levels are a change: {:?}", report.lines()));
    let levels = report
        .steps
        .iter()
        .find(|s| s.step == Step::Levels)
        .expect("a levels step");
    assert_eq!(levels.changed.len(), 2, "{levels:?}");
    assert!(
        levels.changed[0].contains("triangles by level"),
        "{:?}",
        levels.changed
    );
    assert_eq!(missing(&pack), 0, "{:?}", report.lines());
    let w = Wardrobe::read(&pack).unwrap();
    assert!(
        w.garments.iter().all(|g| g
            .model
            .as_deref()
            .is_some_and(|m| m.ends_with(".drawables.src"))),
        "{:?}",
        w.garments.iter().map(|g| &g.model).collect::<Vec<_>>()
    );
    // The compiled file the first garment named is where it was.
    assert!(pack.join("models/jbib_000_u.ydd").is_file());

    // A second run changes no byte.
    let after = snapshot(&pack);
    let (again, none) = all_safe_also(&pack, &Watch::default(), &also).unwrap();
    assert!(none.is_none() && !again.changed(), "{:?}", again.lines());
    assert_eq!(snapshot(&pack), after);

    // One Undo takes all of it back, the levels with the rest.
    undo_revision(&pack, &revision).expect("Undo takes the pipeline back");
    assert_eq!(missing(&pack), 2);
    assert_eq!(
        Wardrobe::read(&pack).unwrap().garments[0].model.as_deref(),
        Some("models/jbib_000_u.ydd")
    );
}
