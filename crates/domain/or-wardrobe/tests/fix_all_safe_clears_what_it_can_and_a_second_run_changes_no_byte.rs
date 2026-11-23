//! Fix all safe: a pack with a level of detail missing, a texture that is not a power of two and two garments pinned at one place
//! is mended as far as the pipeline goes, says what it left, is taken back whole by one Undo, and changes nothing when run again.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, put_bytes, snapshot, ytd_of, JBIB};
use or_io::codec::texture::GrcTextureFormat;
use or_source::game_import::{decompile, Facts as GameFacts};
use or_source::sample;
use or_wardrobe::edit::{undo_revision, Watch};
use or_wardrobe::fix::{all_safe, repair_garment, Step};
use or_wardrobe::{allocate, check, inspect, Depth, Finding, Ped, Wardrobe};

const DXT1: GrcTextureFormat = GrcTextureFormat::DXT1;

fn source(parent: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    std::fs::create_dir_all(parent).unwrap();
    decompile(bytes, name, parent, &GameFacts::default()).expect("a source")
}

/// A pack of two jackets pinned at one place, one with a texture of 100 by 60 that stores one mip and one with a texture that needs nothing, the second in a compiled model that has no medium level.
fn a_pack(dir: &Path) -> PathBuf {
    let pack = dir.join("clothing");
    source(
        &pack.join("models"),
        "jbib_000_u.ydd",
        &sample::drawable_dictionary("jbib_000_u", 6, 0.0),
    );
    // A model of a pack adopted from disk stays the compiled file it is, and its levels of detail are counted.
    put_bytes(
        &pack,
        "models/jbib_001_u.ydd",
        &sample::drawable_dictionary("jbib_001_u", 6, 0.0),
    );
    source(
        &pack.join("textures"),
        "odd.ytd",
        &ytd_of(&[("odd", DXT1, 100, 60, 1)]),
    );
    source(
        &pack.join("textures"),
        "fine.ytd",
        &ytd_of(&[("fine", DXT1, 64, 64, 5)]),
    );
    let mut w = Wardrobe::new("demo");
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(3),
        "models/jbib_000_u.drawables.src",
        &["textures/odd.textures.src"],
    ));
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(3),
        "models/jbib_001_u.ydd",
        &["textures/fine.textures.src"],
    ));
    w.write(&pack).unwrap();
    pack
}

fn findings(pack: &Path) -> Vec<Finding> {
    let w = Wardrobe::read(pack).unwrap();
    check(&w, &allocate(&w), &inspect(pack, &w, Depth::Measure))
}

fn codes(found: &[Finding]) -> Vec<&'static str> {
    let mut codes: Vec<_> = found.iter().map(|f| f.code).collect();
    codes.sort_unstable();
    codes.dedup();
    codes
}

/// The files under `pack` that a person's folder holds, and not the private folder that keeps what a session needs.
fn tree(pack: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut all = snapshot(pack);
    all.retain(|rel, _| !rel.starts_with(".openrage/"));
    all
}

#[test]
fn it_clears_what_it_can_says_what_stands_and_a_second_run_changes_no_byte() {
    let dir = tempfile::tempdir().unwrap();
    let pack = a_pack(dir.path());
    let before = findings(&pack);
    for code in [
        "position-clash",
        "texture-not-power-of-two",
        "texture-mips",
        "lod-missing",
    ] {
        assert!(
            codes(&before).contains(&code),
            "{code} in {:?}",
            codes(&before)
        );
    }
    let untouched = snapshot(&pack.join("textures/odd.textures.src"));
    let kept = snapshot(&pack.join("textures/fine.textures.src"));

    let (report, revision) = all_safe(&pack, &Watch::default()).expect("the pipeline runs");

    let after = findings(&pack);
    for cleared in ["position-clash", "texture-not-power-of-two", "texture-mips"] {
        assert!(
            !codes(&after).contains(&cleared),
            "{cleared} stands: {after:?}"
        );
    }
    assert!(
        codes(&after).contains(&"lod-missing"),
        "{:?}",
        codes(&after)
    );
    let stands = report
        .left
        .iter()
        .find(|s| s.code == "lod-missing")
        .expect("what stands is listed");
    assert!(stands.why.contains("mesh doctor"), "{stands:?}");
    assert_eq!(
        report.steps.iter().map(|s| s.step).collect::<Vec<_>>(),
        [Step::Repair, Step::Textures, Step::Order, Step::Mask],
        "a report for each step, in the order of the pipeline"
    );
    let said = |step: Step| {
        &report
            .steps
            .iter()
            .find(|s| s.step == step)
            .unwrap()
            .changed
    };
    assert_eq!(said(Step::Textures).len(), 1, "{:?}", said(Step::Textures));
    assert_eq!(said(Step::Order).len(), 1, "{:?}", said(Step::Order));
    assert!(said(Step::Repair).is_empty());

    // The source the texture was optimised from is as it was, and the one that needed nothing is not copied.
    assert_eq!(snapshot(&pack.join("textures/odd.textures.src")), untouched);
    assert_eq!(snapshot(&pack.join("textures/fine.textures.src")), kept);
    let w = Wardrobe::read(&pack).unwrap();
    assert_eq!(
        w.garments[0].variations[0].texture.as_deref(),
        Some("textures/odd-2.textures.src")
    );
    assert_eq!(
        w.garments[1].variations[0].texture.as_deref(),
        Some("textures/fine.textures.src")
    );
    assert_eq!(
        w.garments[1].position, None,
        "the second pin is let go, the first stays"
    );
    assert_eq!(w.garments[0].position, Some(3));

    // Run again: nothing is written, nothing is made.
    let (again, none) = all_safe(&pack, &Watch::default()).unwrap();
    assert!(none.is_none() && !again.changed(), "{again:?}");
    let settled = snapshot(&pack);
    all_safe(&pack, &Watch::default()).unwrap();
    assert_eq!(snapshot(&pack), settled, "a second run changes no byte");

    // One step of Undo takes all of it back.
    let revision = revision.expect("one step for the whole pipeline");
    let one_step_back = tree(&pack);
    assert!(one_step_back
        .keys()
        .any(|k| k.starts_with("textures/odd-2.textures.src/")));
    undo_revision(&pack, &revision).expect("Undo takes the pipeline back");
    let undone = tree(&pack);
    assert!(
        !undone.keys().any(|k| k.contains("odd-2")),
        "{:?}",
        undone.keys()
    );
    assert_eq!(Wardrobe::read(&pack).unwrap(), revision.before);
}

#[test]
fn a_protected_model_is_refused_by_name_and_the_repair_of_its_garment_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    put_bytes(&pack, "models/sealed.ydd", b"FXAP\0\0\0\0the author's");
    let mut w = Wardrobe::new("demo");
    let mut g = garment(Ped::Male, JBIB, None, "models/sealed.ydd", &[]);
    g.label = Some("sealed".into());
    let id = g.id.clone();
    w.garments.push(g);
    w.write(&pack).unwrap();
    let before = snapshot(&pack);

    let (report, revision) = all_safe(&pack, &Watch::default()).unwrap();

    let repair = report
        .steps
        .iter()
        .find(|s| s.step == Step::Repair)
        .unwrap();
    assert_eq!(repair.refused.len(), 1, "{repair:?}");
    assert_eq!(repair.refused[0].file, "models/sealed.ydd");
    assert!(
        repair.refused[0].why.contains("FXAP") && repair.refused[0].why.contains("protected"),
        "{:?}",
        repair.refused[0]
    );
    assert!(revision.is_none());
    assert_eq!(snapshot(&pack), before);

    let why = repair_garment(&pack, &id).unwrap_err().to_string();
    assert!(
        why.contains("models/sealed.ydd") && why.contains("FXAP"),
        "{why}"
    );
    assert_eq!(snapshot(&pack), before, "a refusal changes nothing");
}

#[test]
fn a_stop_leaves_the_pack_and_its_folder_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    let pack = a_pack(dir.path());
    let before = snapshot(&pack);
    let watch = Watch::default();
    watch.cancel.set();

    let stopped = all_safe(&pack, &watch);

    assert!(matches!(stopped, Err(or_wardrobe::edit::Error::Stopped)));
    assert_eq!(snapshot(&pack), before);
}

/// Over two models of the staged game files, whatever levels of detail they lack, the pipeline clears the clash and the odd texture and
/// makes no level of detail, and run again it changes no byte. Skipped, and said so, where no corpus is staged.
#[test]
fn over_staged_game_models_it_clears_what_it_can_and_leaves_their_levels_of_detail_alone() {
    let Some(models) = common::staged("ydd", 2, "two small staged .ydd files") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    source(
        &pack.join("textures"),
        "odd.ytd",
        &ytd_of(&[("odd", DXT1, 100, 60, 1)]),
    );
    let mut w = Wardrobe::new("demo");
    for (n, model) in models.iter().enumerate() {
        common::put(&pack, &format!("models/m{n}.ydd"), model);
        w.garments.push(garment(
            Ped::Male,
            JBIB,
            Some(3),
            &format!("models/m{n}.ydd"),
            &["textures/odd.textures.src"],
        ));
    }
    w.write(&pack).unwrap();
    let before = codes(&findings(&pack));
    assert!(before.contains(&"position-clash"), "{before:?}");

    let (report, _) = all_safe(&pack, &Watch::default()).expect("the pipeline runs");

    let after = codes(&findings(&pack));
    for cleared in ["position-clash", "texture-not-power-of-two", "texture-mips"] {
        assert!(!after.contains(&cleared), "{cleared} stands: {after:?}");
    }
    for lod in ["lod-missing", "lod-medium-heavy", "lod-low-heavy"] {
        assert_eq!(
            before.contains(&lod),
            after.contains(&lod),
            "{lod}: a level of detail is the mesh doctor's"
        );
    }
    assert!(report
        .steps
        .iter()
        .any(|s| s.step == Step::Order && !s.changed.is_empty()));
    let settled = snapshot(&pack);
    all_safe(&pack, &Watch::default()).unwrap();
    assert_eq!(snapshot(&pack), settled);
}
