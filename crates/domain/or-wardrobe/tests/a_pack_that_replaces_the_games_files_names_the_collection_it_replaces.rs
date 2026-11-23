//! A ped's files may replace the game's own instead of adding a collection: a collection of a DLC, or the ped's base
//! drawables. The build writes the files under the game's names and none of the metadata that states them, and it
//! says which collection it replaces and that nobody has seen the game load the result.

mod common;

use std::path::Path;

use common::{garment, put_bytes, ydd, ytd, JBIB, LOWR};
use or_project::{Container, Module, Output, OutputKind};
use or_wardrobe::plan::Build;
use or_wardrobe::verify::verify;
use or_wardrobe::{inspect, plan, plan_for, plans, Depth, Ped, Planned, Replace, Wardrobe};

/// The male ped replaces the heist DLC's jackets and the female ped the base game's trousers.
fn pack(folder: &Path) -> Wardrobe {
    put_bytes(folder, "textures/t.ytd", &ytd("t"));
    put_bytes(folder, "models/jacket.ydd", &ydd("jacket"));
    put_bytes(folder, "models/legs.ydd", &ydd("legs"));
    let mut w = Wardrobe::new("demo");
    w.rules.replace.insert(
        Ped::Male,
        Replace {
            dlc: Some("mp_m_heist".into()),
        },
    );
    w.rules.replace.insert(Ped::Female, Replace::default());
    let mut jacket = garment(
        Ped::Male,
        JBIB,
        Some(5),
        "models/jacket.ydd",
        &["textures/t.ytd"],
    );
    jacket.label = Some("jacket".into());
    let mut legs = garment(
        Ped::Female,
        LOWR,
        Some(3),
        "models/legs.ydd",
        &["textures/t.ytd"],
    );
    legs.label = Some("legs".into());
    w.garments = vec![jacket, legs];
    w.write(folder).unwrap();
    w
}

fn planned(folder: &Path, w: &Wardrobe) -> Planned {
    plan(w, &inspect(folder, w, Depth::Open), "pack", folder)
}

fn paths(p: &Planned) -> Vec<String> {
    p.contribution
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect()
}

#[test]
fn the_files_carry_the_names_of_the_collection_they_replace_or_of_the_ped_alone() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let files = paths(&planned(dir.path(), &w));
    assert!(
        files.contains(&"stream/demo/mp_m_freemode_01_mp_m_heist/jbib/mp_m_freemode_01_mp_m_heist^jbib_005_u.ydd".to_string()),
        "{files:?}"
    );
    assert!(files.contains(&"stream/demo/mp_m_freemode_01_mp_m_heist/jbib/mp_m_freemode_01_mp_m_heist^jbib_diff_005_a_uni.ytd".to_string()));
    assert!(
        files.contains(
            &"stream/demo/mp_f_freemode_01/lowr/mp_f_freemode_01^lowr_003_u.ydd".to_string()
        ),
        "the base game's drawables carry no collection: {files:?}"
    );
}

#[test]
fn a_replacement_writes_no_metadata_and_claims_no_collection() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let p = planned(dir.path(), &w);
    let files = paths(&p);
    assert!(
        files
            .iter()
            .all(|f| !f.ends_with(".ymt") && !f.ends_with(".meta")),
        "the game's own metadata states these drawables: {files:?}"
    );
    assert!(
        p.contribution.claims.is_empty(),
        "{:?}",
        p.contribution.claims
    );
    assert!(p.contribution.manifest.is_empty());
    assert!(p.documents.variations.is_empty() && p.documents.shops.is_empty());
    assert!(
        p.contribution.problems.is_empty(),
        "{:?}",
        p.contribution.problems
    );
}

#[test]
fn the_plan_and_the_problems_name_the_collection_and_say_nobody_has_seen_the_game_load_it() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let p = planned(dir.path(), &w);
    let notes = p.contribution.notes.join("\n");
    assert!(
        notes.contains("replace the collection mp_m_heist of the game")
            && notes.contains("base game's mp_f_freemode_01")
            && notes.contains("not verified in game"),
        "{notes}"
    );
    let info: Vec<_> = p
        .findings
        .iter()
        .filter(|f| f.code == "replace-mode")
        .collect();
    assert_eq!(info.len(), 2, "one for each ped: {:?}", p.findings);
    assert!(info.iter().all(|f| !f.blocks()));
    assert!(
        info[0].message.contains("mp_m_heist") && info[0].message.contains("not verified in game")
    );
}

#[test]
fn a_garment_pinned_at_no_number_of_the_game_is_warned_about() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    w.garments[0].position = None;
    let p = planned(dir.path(), &w);
    let warned: Vec<_> = p
        .findings
        .iter()
        .filter(|f| f.code == "replace-unpinned")
        .collect();
    assert_eq!(warned.len(), 1);
    assert_eq!(
        warned[0].garment.as_deref(),
        Some(w.garments[0].id.as_str())
    );
    assert!(!warned[0].blocks());
}

#[test]
fn a_replacement_is_a_pack_for_a_resource_and_a_singleplayer_dlc_says_why_it_cannot_ship_it() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let facts = inspect(dir.path(), &w, Depth::Open);
    let output = Output::of(OutputKind::SingleplayerDlc, "dlc", "demo", &["wardrobe"]);
    let (build, problems) = Build::of(&w, &output);
    assert!(problems.is_empty());
    let p = plan_for(&w, &facts, "pack", dir.path(), &build);
    assert!(
        p.contribution
            .problems
            .iter()
            .any(|why| why.contains("replace the collection mp_m_heist")
                && why.contains("a replacement ships in a FiveM resource")),
        "{:?}",
        p.contribution.problems
    );
}

#[test]
fn one_ped_may_replace_while_the_other_adds_a_collection_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    w.rules.replace.remove(&Ped::Female);
    w.garments[1].position = None;
    let p = planned(dir.path(), &w);
    let files = paths(&p);
    assert!(files
        .iter()
        .any(|f| f.ends_with("mp_f_freemode_01_mp_f_demo.ymt")));
    assert!(files.contains(&"data/mp_f_freemode_01_mp_f_demo.meta".to_string()));
    assert!(files
        .iter()
        .all(|f| !f.contains("mp_m_freemode_01_mp_m_heist.")));
    let claims: Vec<&str> = p
        .contribution
        .claims
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(claims, ["mp_f_demo"]);
}

#[test]
fn a_replacement_builds_every_file_reloads_and_the_check_reads_back_what_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let container = Container {
        modules: vec![Module::clothing("wardrobe", ".")],
        outputs: vec![Output::fivem_resource(
            "fivem",
            "demo_resource",
            &["wardrobe"],
        )],
    };
    let path = dir.path().join("pack.openrage-proj");
    or_project::Project::create(&path, &container, &[]).unwrap();
    let project = or_project::Project::load(&path).unwrap();
    let plan = plans(&project, "fivem").unwrap().remove(0);
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let built =
        or_project::build(&plan, &or_project::build_root(&project.path)).expect("it builds");
    assert!(built.verification.reloaded.iter().any(|r| r
        .path
        .ends_with("mp_m_freemode_01_mp_m_heist^jbib_005_u.ydd")));
    let planned = or_wardrobe::planned(
        &project.folder(),
        project.container.module("wardrobe").unwrap(),
        Depth::Digest,
    )
    .unwrap();
    let found = verify(&w, &planned, &built.folder);
    assert!(found.is_clean(), "{:?}", found.problems());
}

#[test]
fn a_replacement_is_kept_in_wardrobe_json_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let text = std::fs::read_to_string(dir.path().join("wardrobe.json")).unwrap();
    assert!(
        text.contains("\"replace\"") && text.contains("\"dlc\": \"mp_m_heist\""),
        "{text}"
    );
    assert_eq!(Wardrobe::read(dir.path()).unwrap(), w);
}
