//! A pack whose drawable owns cloth builds with the cloth beside the model, the build reloads it
//! through the cloth reader, the verifier holds the files to the metadata the build wrote, and a
//! pack imported from that build rebuilds with the cloth.
//!
//! The files are staged ones the readers open: a drawable, a texture dictionary and a cloth dictionary.

mod common;

use common::{garment, project, put, snapshot, staged, JBIB};
use or_project::{build, build_root};
use or_wardrobe::import::{import, Options};
use or_wardrobe::{plans, verify::verify, Depth, Ped, Wardrobe};

const WHAT: &str = "a_built_pack_reopens_the_cloth_its_drawables_own";
const CLOTH: &str =
    "stream/police/mp_m_freemode_01_mp_m_police/jbib/mp_m_freemode_01_mp_m_police^jbib_000_u.yld";

/// A pack of one jacket that owns the staged cloth, laid out in `dir`.
fn a_jacket_with_cloth(dir: &std::path::Path) -> Option<Wardrobe> {
    let ydd = staged("ydd", 1, WHAT)?;
    let ytd = staged("ytd", 1, WHAT)?;
    let yld = staged("yld", 1, WHAT)?;
    put(dir, "models/jacket.ydd", &ydd[0]);
    put(dir, "textures/jacket.ytd", &ytd[0]);
    put(dir, "cloth/jacket.yld", &yld[0]);
    let mut jacket = garment(
        Ped::Male,
        JBIB,
        Some(0),
        "models/jacket.ydd",
        &["textures/jacket.ytd"],
    );
    jacket.cloth = Some("cloth/jacket.yld".into());
    let mut w = Wardrobe::new("police");
    w.garments = vec![jacket];
    Some(w)
}

fn verified(
    project: &or_project::Project,
    w: &Wardrobe,
    built: &std::path::Path,
) -> or_wardrobe::verify::Verification {
    let planned = or_wardrobe::planned(
        &project.folder(),
        project.container.module("wardrobe").unwrap(),
        Depth::Digest,
    )
    .unwrap();
    verify(w, &planned, built)
}

#[test]
fn the_cloth_is_built_under_the_models_name_reloaded_and_held_to_the_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let Some(w) = a_jacket_with_cloth(dir.path()) else {
        return;
    };
    let project = project(dir.path(), &w, "police_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert!(
        plan.files.iter().any(|f| f.path == CLOTH),
        "{:?}",
        plan.files
    );

    let built = build(&plan, &build_root(&project.path)).expect("it builds");
    assert!(
        built.verification.reloaded.iter().any(|r| r.path == CLOTH),
        "the build reloads the cloth through a reader: {:?}",
        built.verification.reloaded
    );

    let found = verified(&project, &w, &built.folder);
    assert!(found.is_clean(), "{:?}", found.problems());
    let check = found.checks.iter().find(|c| c.name == "cloth").unwrap();
    assert!(
        check
            .detail
            .starts_with("1 drawable(s) own cloth and 1 cloth file(s)"),
        "{}",
        check.detail
    );
}

#[test]
fn a_cloth_that_is_gone_from_the_build_or_does_not_reopen_fails_the_check_that_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let Some(w) = a_jacket_with_cloth(dir.path()) else {
        return;
    };
    let project = project(dir.path(), &w, "police_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    let built = build(&plan, &build_root(&project.path)).expect("it builds");
    let file = built.folder.join(CLOTH);

    let original = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    let found = verified(&project, &w, &built.folder);
    let problems = found.problems();
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("cloth: ") && p.contains("is not in the build")),
        "{problems:?}"
    );

    std::fs::write(&file, b"not a cloth dictionary").unwrap();
    let found = verified(&project, &w, &built.folder);
    assert!(
        found
            .problems()
            .iter()
            .any(|p| p.starts_with("cloth: ") && p.contains("does not reopen")),
        "{:?}",
        found.problems()
    );

    std::fs::write(&file, original).unwrap();
    assert!(verified(&project, &w, &built.folder).is_clean());
}

#[test]
fn a_pack_imported_from_a_build_rebuilds_with_its_cloth() {
    let dir = tempfile::tempdir().unwrap();
    let Some(w) = a_jacket_with_cloth(dir.path()) else {
        return;
    };
    let source = project(dir.path(), &w, "police_pack");
    let first_plan = plans(&source, "fivem").unwrap().remove(0);
    let built = build(&first_plan, &build_root(&source.path)).expect("it builds");

    let imported = import(&built.folder, &Options::default()).expect("the build imports");
    assert_eq!(imported.report.cloth, 1);
    assert!(
        imported.report.left_out.is_empty(),
        "{:?}",
        imported.report.left_out
    );

    // The imported pack is a project of its own, whose files are the built resource's.
    let again_dir = tempfile::tempdir().unwrap();
    copy_tree(&built.folder, again_dir.path());
    let again = project(again_dir.path(), &imported.wardrobe, "police_again");
    let plan = plans(&again, "fivem").unwrap().remove(0);
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let rebuilt = build(&plan, &build_root(&again.path)).expect("it builds again");
    let (first, second) = (snapshot(&built.folder), snapshot(&rebuilt.folder));
    assert_eq!(
        first.get(CLOTH),
        second.get(CLOTH),
        "the cloth, byte for byte"
    );
    assert!(second.contains_key(CLOTH));
    assert_eq!(
        first.keys().collect::<Vec<_>>(),
        second.keys().collect::<Vec<_>>(),
        "the same files"
    );
}

#[test]
fn a_drawable_that_owns_cloth_with_none_to_ship_builds_only_with_a_reason_and_the_verifier_says_so()
{
    let dir = tempfile::tempdir().unwrap();
    let Some(mut w) = a_jacket_with_cloth(dir.path()) else {
        return;
    };
    // What an import of a pack whose metadata states the flag and that has no cloth file makes of the garment.
    w.garments[0].cloth = None;
    w.garments[0].missing_cloth = true;
    let project = project(dir.path(), &w, "police_pack");
    let refused = plans(&project, "fivem").unwrap().remove(0);
    assert!(
        refused
            .problems
            .iter()
            .any(|p| p.contains("is missing its cloth")),
        "{:?}",
        refused.problems
    );
    assert!(build(&refused, &build_root(&project.path)).is_err());

    w.waivers.push(or_wardrobe::Waiver {
        code: "cloth-missing".into(),
        garment: Some(w.garments[0].id.clone()),
        reason: "the cloth is added by hand".into(),
    });
    let project = common::project(dir.path(), &w, "police_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let built = build(&plan, &build_root(&project.path)).expect("a waived cloth builds");
    let found = verified(&project, &w, &built.folder);
    assert!(found.is_clean(), "{:?}", found.problems());
    let check = found.checks.iter().find(|c| c.name == "cloth").unwrap();
    assert!(
        check.detail.contains("waived: the cloth is added by hand"),
        "{}",
        check.detail
    );
}

/// `from` copied to `to`, every file under it.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_tree(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}
