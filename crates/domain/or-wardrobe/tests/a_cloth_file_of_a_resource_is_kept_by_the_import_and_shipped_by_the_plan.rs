//! The cloth of a drawable travels as its child: an import links the `.yld` beside the `.ydd` it
//! belongs to when the metadata states `ownsCloth`, the plan ships it under the model's own name and
//! writes the flag from the link, a drawable that states the flag with no cloth file is missing its
//! cloth (an error a build stops on), and a cloth the metadata does not state is not linked.
//!
//! The files here are laid out by hand, with bytes that no reader opens, because what is held to
//! account is where the import and the plan put a name: whether a reader opens a `.yld` is the
//! build's reload and the verifier's, in `a_built_pack_reopens_the_cloth_its_drawables_own`.

use std::path::Path;

use or_io::decl::File;
use or_wardrobe::import::{import, Options};
use or_wardrobe::{allocate, inspect, metadata, plan, Depth, Garment, Ped, Slot, Wardrobe};

const STEM: &str = "mp_m_freemode_01_mp_m_p";
const JBIB_DIR: &str = "stream/p/mp_m_freemode_01_mp_m_p/jbib";

fn write(dir: &Path, rel: &str, bytes: &[u8]) {
    let to = dir.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, bytes).unwrap();
}

/// A resource of one drawable, `jbib` 0, with a texture and, if `cloth`, a cloth file beside the model; `states_it` is what its metadata states of `ownsCloth`.
fn a_resource(dir: &Path, cloth: bool, states_it: bool) {
    write(dir, &format!("{JBIB_DIR}/{STEM}^jbib_000_u.ydd"), b"ydd");
    write(
        dir,
        &format!("{JBIB_DIR}/{STEM}^jbib_diff_000_a_uni.ytd"),
        b"ytd",
    );
    if cloth {
        write(dir, &format!("{JBIB_DIR}/{STEM}^jbib_000_u.yld"), b"yld");
    }
    // The variation metadata is the one the engine writes for such a drawable: the flag follows the link, or the claim of one.
    let mut w = Wardrobe::new("p");
    let mut g = Garment::new();
    g.ped = Some(Ped::Male);
    g.slot = Some("jbib".parse::<Slot>().unwrap());
    g.position = Some(0);
    g.model = Some("m.ydd".into());
    g.variations = vec![Default::default()];
    g.missing_cloth = states_it;
    w.garments.push(g);
    let info = metadata::variation_info(&w, &allocate(&w), Ped::Male);
    write(
        dir,
        &format!("stream/p/{STEM}.ymt"),
        &info.to_bytes().unwrap(),
    );
}

/// The `cloth` a garment of the imported pack names, as its file would spell it.
fn cloth_of(garment: &Garment) -> Option<String> {
    serde_json::to_value(garment).unwrap()["cloth"]
        .as_str()
        .map(String::from)
}

/// Whether the variation metadata a plan writes for the male ped states `ownsCloth` for the one drawable.
fn states_the_flag(planned: &or_wardrobe::Planned) -> bool {
    let info = planned
        .documents
        .variations
        .get(&Ped::Male)
        .expect("the plan writes the variation metadata of the male ped");
    let component = info
        .a_component_data3
        .first()
        .expect("a component is dressed");
    component.a_drawbl_data3[0].cloth_data.owns_cloth
}

#[test]
fn an_import_links_the_cloth_beside_its_model_when_the_metadata_states_it() {
    let dir = tempfile::tempdir().unwrap();
    a_resource(dir.path(), true, true);

    let imported = import(dir.path(), &Options::default()).expect("the resource imports");

    assert!(
        imported.report.left_out.is_empty(),
        "the cloth belongs to a drawable of the pack: {:?}",
        imported.report.left_out
    );
    let garment = &imported.wardrobe.garments[0];
    assert_eq!(
        cloth_of(garment).as_deref(),
        Some(format!("{JBIB_DIR}/{STEM}^jbib_000_u.yld").as_str())
    );
    assert!(!garment.missing_cloth);
    assert!(imported.report.cloth_missing.is_empty());
}

#[test]
fn the_plan_of_an_imported_pack_ships_the_cloth_under_the_models_name_and_states_the_flag_from_it()
{
    let dir = tempfile::tempdir().unwrap();
    a_resource(dir.path(), true, true);
    let imported = import(dir.path(), &Options::default()).unwrap();

    let facts = inspect(dir.path(), &imported.wardrobe, Depth::Digest);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", dir.path());

    let shipped = planned
        .contribution
        .files
        .iter()
        .find(|f| f.path.ends_with(".yld"))
        .unwrap_or_else(|| {
            panic!(
                "the plan ships no cloth: {:?}",
                planned
                    .contribution
                    .files
                    .iter()
                    .map(|f| &f.path)
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(shipped.path, format!("{JBIB_DIR}/{STEM}^jbib_000_u.yld"));
    assert!(matches!(
        &shipped.origin,
        or_project::Origin::Copy { bytes: 3, .. }
    ));
    assert!(
        planned.findings.iter().all(|f| f.code != "cloth-missing"),
        "{:?}",
        planned.findings
    );
    assert!(
        planned.contribution.problems.is_empty(),
        "{:?}",
        planned.contribution.problems
    );
    // The flag is written from the link: the pack holds no flag of its own to say so.
    assert!(states_the_flag(&planned));
}

#[test]
fn a_drawable_that_states_the_flag_with_no_cloth_file_is_missing_its_cloth_and_stops_the_build_until_a_reason_waives_it(
) {
    let dir = tempfile::tempdir().unwrap();
    // The metadata states the cloth and the resource has none.
    a_resource(dir.path(), false, true);
    let imported = import(dir.path(), &Options::default()).unwrap();
    let mut wardrobe = imported.wardrobe;
    assert!(wardrobe.garments[0].missing_cloth);
    assert_eq!(imported.report.cloth_missing.len(), 1);

    let facts = inspect(dir.path(), &wardrobe, Depth::Digest);
    let planned = plan(&wardrobe, &facts, "wardrobe", dir.path());
    let found: Vec<_> = planned
        .findings
        .iter()
        .filter(|f| f.code == "cloth-missing")
        .collect();
    assert_eq!(found.len(), 1, "{:?}", planned.findings);
    assert_eq!(found[0].severity, or_wardrobe::Severity::Error);
    assert!(
        planned
            .contribution
            .problems
            .iter()
            .any(|p| p.contains("is missing its cloth")),
        "the plan refuses it: {:?}",
        planned.contribution.problems
    );
    // The flag the pack was read with is the one it is written with, so nothing changes for the game.
    assert!(states_the_flag(&planned));

    wardrobe.waivers.push(or_wardrobe::Waiver {
        code: "cloth-missing".into(),
        garment: Some(wardrobe.garments[0].id.clone()),
        reason: "the cloth is added by hand afterwards".into(),
    });
    let planned = plan(&wardrobe, &facts, "wardrobe", dir.path());
    assert!(
        planned.contribution.problems.is_empty(),
        "a waiver with a reason lets it build: {:?}",
        planned.contribution.problems
    );
}

#[test]
fn a_cloth_the_metadata_does_not_state_is_not_linked_and_is_listed_as_ignored_by_the_game() {
    let dir = tempfile::tempdir().unwrap();
    a_resource(dir.path(), true, false);

    let imported = import(dir.path(), &Options::default()).unwrap();

    let garment = &imported.wardrobe.garments[0];
    assert_eq!(cloth_of(garment), None, "{garment:?}");
    assert!(!garment.missing_cloth);
    assert_eq!(imported.report.cloth, 0);
    let ignored: Vec<_> = imported
        .report
        .left_out
        .iter()
        .filter(|l| l.path.ends_with(".yld"))
        .collect();
    assert_eq!(ignored.len(), 1, "{:?}", imported.report.left_out);
    assert!(
        ignored[0].why.contains("does not state ownsCloth") && ignored[0].why.contains("Link"),
        "{}",
        ignored[0].why
    );
    // A build of that pack writes the metadata the original had: no flag, and no cloth shipped.
    let facts = inspect(dir.path(), &imported.wardrobe, Depth::Digest);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", dir.path());
    assert!(planned
        .contribution
        .files
        .iter()
        .all(|f| !f.path.ends_with(".yld")));
    assert!(!states_the_flag(&planned));
}
