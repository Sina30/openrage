//! The lossless repair of the owner's policy (2026-10-03) as the Wardrobe runs it: asked for, a model whose counts contradict their
//! runs becomes a source made of the repaired file, the report is kept with the source, and the file it was is untouched.
//!
//! ⚠ Reads the mod `.ydd` staged in `openrage-corpus/mods/`; a machine without it skips, and says so.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, put_bytes, snapshot, JBIB};
use or_wardrobe::edit::{undo_revision, Watch};
use or_wardrobe::fix::{all_safe, repair_garment, Step};
use or_wardrobe::{Ped, Wardrobe};

const INCOHERENT: [&str; 2] = ["Naruto4tails.ydd", "susanoo_2.ydd"];

fn staged(file: &str) -> Option<PathBuf> {
    let mods = or_test_support::mods_corpus_origin(env!("CARGO_MANIFEST_DIR"));
    let found = (or_test_support::Corpus::with_root(mods.path(), "ydd").all_targets())
        .find()
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == file));
    if found.is_none() {
        or_test_support::skip_corpus(
            &format!("the staged mod {file}"),
            format!("not staged in {}", mods.describe()),
        );
    }
    found
}

fn a_pack_with(dir: &Path, file: &Path) -> (PathBuf, String) {
    let pack = dir.join("clothing");
    put_bytes(&pack, "models/mod.ydd", &std::fs::read(file).unwrap());
    let mut w = Wardrobe::new("demo");
    let g = garment(Ped::Male, JBIB, None, "models/mod.ydd", &[]);
    let id = g.id.clone();
    w.garments.push(g);
    w.write(&pack).unwrap();
    (pack, id)
}

#[test]
fn asked_to_repair_it_the_model_becomes_a_source_with_the_report_and_the_file_stays() {
    for file in INCOHERENT {
        let Some(staged) = staged(file) else { continue };
        let dir = tempfile::tempdir().unwrap();
        let (pack, id) = a_pack_with(dir.path(), &staged);
        let compiled = std::fs::read(pack.join("models/mod.ydd")).unwrap();

        let (report, revision) = repair_garment(&pack, &id).expect("the repair is lossless");

        assert_eq!(report.step, Step::Repair);
        assert_eq!(report.changed.len(), 1, "{report:?}");
        assert!(
            report.changed[0].contains("primitive_count 0 rewritten as")
                && report.changed[0].contains("models/mod.ydd")
                && report.changed[0].contains("models/mod.drawables.src"),
            "{}",
            report.changed[0]
        );
        let w = Wardrobe::read(&pack).unwrap();
        assert_eq!(
            w.garments[0].model.as_deref(),
            Some("models/mod.drawables.src")
        );
        assert_eq!(
            std::fs::read(pack.join("models/mod.ydd")).unwrap(),
            compiled
        );
        // The report travels with the source it made.
        let manifest =
            std::fs::read_to_string(pack.join("models/mod.drawables.src/asset.json")).unwrap();
        assert!(
            manifest.contains("\"repaired\"") && manifest.contains("rewritten as"),
            "{manifest}"
        );

        // The pipeline finds nothing more to do for a garment that names a source, and Undo takes the repair back.
        let (again, none) = all_safe(&pack, &Watch::default()).unwrap();
        assert!(none.is_none(), "{again:?}");
        let revision = revision.expect("a step of Undo");
        undo_revision(&pack, &revision).unwrap();
        assert!(!pack.join("models/mod.drawables.src").exists());
        assert_eq!(Wardrobe::read(&pack).unwrap(), revision.before);
        assert_eq!(
            snapshot(&pack).get("models/mod.ydd"),
            Some(&compiled),
            "the file it was is the file it is"
        );
    }
}

#[test]
fn a_model_that_states_no_count_against_its_run_is_not_touched() {
    let Some(coherent) = staged("Madara_Susanoo_big.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack_with(dir.path(), &coherent);
    let before = snapshot(&pack);

    let (report, revision) = repair_garment(&pack, &id).unwrap();

    assert!(
        report.changed.is_empty() && revision.is_none(),
        "{report:?}"
    );
    assert_eq!(snapshot(&pack), before);
}
