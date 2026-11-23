//! A slot holds as many drawables as the cap of a collection, and the garments past it are written as the next
//! collection of the pack (`<collection>_1`, `<collection>_2`), each with the files and the metadata of its own.

mod common;

use std::path::Path;

use common::{garment, put_bytes, ydd, ytd, JBIB, LOWR};
use or_wardrobe::{inspect, plan, Depth, Ped, Planned, Wardrobe};

const M: &str = "mp_m_freemode_01_mp_m_demo";

/// A pack of `count` jackets for the male ped, none pinned, each a model of its own.
fn pack(folder: &Path, count: usize) -> Wardrobe {
    put_bytes(folder, "textures/t.ytd", &ytd("t"));
    let mut w = Wardrobe::new("demo");
    for i in 0..count {
        put_bytes(folder, &format!("models/m{i}.ydd"), &ydd(&format!("m{i}")));
        w.garments.push(garment(
            Ped::Male,
            JBIB,
            None,
            &format!("models/m{i}.ydd"),
            &["textures/t.ytd"],
        ));
    }
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
fn a_pack_with_130_garments_in_a_slot_splits_at_the_default_cap_of_128() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path(), 130);
    let p = planned(dir.path(), &w);
    let files = paths(&p);

    let last_of_own = format!("stream/demo/{M}/jbib/{M}^jbib_127_u.ydd");
    assert!(
        files.contains(&last_of_own),
        "the last drawable of the pack's own collection is 127"
    );
    let first_of_next = format!("stream/demo/{M}_1/jbib/{M}_1^jbib_000_u.ydd");
    assert!(
        files.contains(&first_of_next),
        "garment 129 is the first drawable of the next collection"
    );
    assert!(
        files.contains(&"data/mp_m_freemode_01_mp_m_demo_1.meta".to_string())
            && files.contains(&"data/mp_m_freemode_01_mp_m_demo.meta".to_string()),
        "each collection registers itself with a shop file of its own"
    );
    assert!(
        files.contains(&"stream/demo/mp_m_freemode_01_mp_m_demo_1.ymt".to_string()),
        "and has a variation file of its own"
    );

    let own = &p.documents.variations[&Ped::Male];
    assert_eq!(own.a_component_data3[0].a_drawbl_data3.len(), 128);
    let figure = |name: &str| {
        (p.contribution.figures.iter())
            .find(|f| f.name == name)
            .map(|f| f.amount)
    };
    assert_eq!(
        figure("clothing_drawables"),
        Some(130),
        "the drawables of both collections are counted"
    );
    let claims: Vec<&str> = p
        .contribution
        .claims
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert!(
        claims.contains(&"mp_m_demo") && claims.contains(&"mp_m_demo_1"),
        "both collections are claimed: {claims:?}"
    );
}

#[test]
fn a_pack_past_the_cap_warns_and_is_not_refused_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path(), 130);
    let p = planned(dir.path(), &w);
    let cap: Vec<_> = p.findings.iter().filter(|f| f.code == "slot-cap").collect();
    assert_eq!(cap.len(), 1, "{:?}", p.findings);
    assert!(!cap[0].blocks(), "a warning, not an error");
    assert!(
        cap[0].message.contains("130") && cap[0].message.contains("128"),
        "{}",
        cap[0].message
    );
    assert!(
        p.contribution.problems.is_empty(),
        "{:?}",
        p.contribution.problems
    );
}

#[test]
fn a_pack_of_exactly_the_cap_stays_in_one_collection_and_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path(), 128);
    let p = planned(dir.path(), &w);
    assert!(p.findings.iter().all(|f| f.code != "slot-cap"));
    assert!(!paths(&p).iter().any(|f| f.contains("_demo_1")));
}

#[test]
fn the_cap_is_the_projects_up_to_255_and_a_slot_is_cut_where_it_says() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path(), 130);
    w.rules.set("slot-cap", |s| s.threshold = Some(255));
    let p = planned(dir.path(), &w);
    assert!(
        !paths(&p).iter().any(|f| f.contains("_demo_1")),
        "130 fit a cap of 255"
    );
    assert!(p.findings.iter().all(|f| f.code != "slot-cap"));

    w.rules.set("slot-cap", |s| s.threshold = Some(100));
    let p = planned(dir.path(), &w);
    assert_eq!(
        p.documents.variations[&Ped::Male].a_component_data3[0]
            .a_drawbl_data3
            .len(),
        100
    );

    // A cap past what a collection holds is held at what it holds, and the pack is told.
    let mut w = pack(dir.path(), 257);
    w.rules.set("slot-cap", |s| s.threshold = Some(300));
    let p = planned(dir.path(), &w);
    assert!(paths(&p).iter().any(|f| f.contains(&format!("{M}_1"))));
    assert_eq!(
        p.documents.variations[&Ped::Male].a_component_data3[0]
            .a_drawbl_data3
            .len(),
        255
    );
    assert!(
        p.findings.iter().any(|f| f.code == "setting-unheeded"),
        "{:?}",
        p.findings
    );
}

#[test]
fn a_slot_is_cut_by_itself_and_another_slot_of_the_ped_stays_in_the_first_collection() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path(), 130);
    put_bytes(dir.path(), "models/legs.ydd", &ydd("legs"));
    w.garments.push(garment(
        Ped::Male,
        LOWR,
        None,
        "models/legs.ydd",
        &["textures/t.ytd"],
    ));
    let p = planned(dir.path(), &w);
    let files = paths(&p);
    assert!(
        files.contains(&format!("stream/demo/{M}/lowr/{M}^lowr_000_u.ydd")),
        "the legs sit in the first collection"
    );
    assert!(
        !files.iter().any(|f| f.contains("_1/lowr")),
        "the legs are one garment and stay in the pack's own collection"
    );
}

#[test]
fn a_split_pack_builds_and_every_collection_reads_back_with_the_metadata_of_its_own() {
    use or_wardrobe::plans;
    use or_wardrobe::verify::verify;
    use or_wardrobe::Expression;

    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path(), 130);
    let expression = |index| Expression {
        index,
        tracks: vec![33],
        ids: vec![28462],
        types: vec![2],
        components: vec![1],
    };
    // One garment in each collection states an expression, and the two share a drawable number.
    w.garments[0].expressions = vec![expression(4)];
    w.garments[128].expressions = vec![expression(5)];
    let project = common::project(dir.path(), &w, "demo_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let built =
        or_project::build(&plan, &or_project::build_root(&project.path)).expect("it builds");

    let reloaded: Vec<&str> = (built.verification.reloaded.iter())
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(
        reloaded.iter().filter(|p| p.ends_with(".ymt")).count(),
        4,
        "a variation file and a creature file for each collection: {reloaded:?}"
    );
    let planned = or_wardrobe::planned(
        &project.folder(),
        project.container.module("wardrobe").unwrap(),
        Depth::Digest,
    )
    .unwrap();
    let found = verify(&w, &planned, &built.folder);
    assert!(found.is_clean(), "{:?}", found.problems());
    let names: Vec<&str> = found.checks.iter().map(|c| c.name.as_str()).collect();
    for wanted in [
        "mp_m_freemode_01 variation metadata",
        "mp_m_freemode_01 variation metadata of mp_m_demo_1",
        "mp_m_freemode_01 shop metadata of mp_m_demo_1",
        "creature metadata of collection 1",
    ] {
        assert!(names.contains(&wanted), "{wanted}: {names:?}");
    }
    // Each collection registers itself with the game, once.
    let manifest = std::fs::read_to_string(built.folder.join("fxmanifest.lua")).unwrap();
    assert!(
        manifest.contains("data/mp_m_freemode_01_mp_m_demo.meta")
            && manifest.contains("data/mp_m_freemode_01_mp_m_demo_1.meta"),
        "{manifest}"
    );
}

#[test]
fn an_import_of_a_collection_past_the_default_cap_raises_the_cap_so_that_it_is_written_as_it_was() {
    use or_wardrobe::import::{import, Options};

    let dir = tempfile::tempdir().unwrap();
    for n in [0, 5, 199] {
        put_bytes(
            dir.path(),
            &format!("stream/x/{M}/jbib/{M}^jbib_{n:03}_u.ydd"),
            &ydd(&format!("m{n}")),
        );
        put_bytes(
            dir.path(),
            &format!("stream/x/{M}/jbib/{M}^jbib_diff_{n:03}_a_uni.ytd"),
            &ytd("t"),
        );
    }
    let imported = import(dir.path(), &Options::default()).unwrap().wardrobe;
    assert_eq!(imported.rules.slot_cap(), 200, "the most it found");
    assert_eq!(
        or_wardrobe::allocate(&imported).parts_of(Ped::Male),
        [0],
        "one collection, as the server had it"
    );

    // A collection that fits the default says nothing of the cap.
    let small = tempfile::tempdir().unwrap();
    put_bytes(
        small.path(),
        &format!("stream/x/{M}/jbib/{M}^jbib_127_u.ydd"),
        &ydd("m"),
    );
    put_bytes(
        small.path(),
        &format!("stream/x/{M}/jbib/{M}^jbib_diff_127_a_uni.ytd"),
        &ytd("t"),
    );
    let imported = import(small.path(), &Options::default()).unwrap().wardrobe;
    assert!(imported.rules.is_default(), "{:?}", imported.rules);
}

#[test]
fn a_split_pack_ships_in_a_singleplayer_dlc_with_a_folder_and_a_shop_file_for_each_collection() {
    use or_project::{Container, Module, Output, OutputKind, Project};
    use or_wardrobe::plans;
    use or_wardrobe::verify::verify_files;

    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path(), 130);
    w.write(dir.path()).unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", ".")],
        outputs: vec![Output::of(
            OutputKind::SingleplayerDlc,
            "dlc",
            "demo",
            &["wardrobe"],
        )],
    };
    let file = dir.path().join("pack.openrage-proj");
    Project::create(&file, &container, &[]).unwrap();
    let project = Project::load(&file).unwrap();
    let plan = plans(&project, "dlc").unwrap().remove(0);
    assert!(plan.is_buildable(), "{:?}", plan.problems);

    let paths: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
    for want in [
        format!("x64/models/cdimages/demo_male.rpf/{M}.ymt"),
        format!("x64/models/cdimages/demo_male.rpf/{M}_1.ymt"),
        format!("x64/models/cdimages/demo_male.rpf/{M}/jbib_127_u.ydd"),
        format!("x64/models/cdimages/demo_male.rpf/{M}_1/jbib_000_u.ydd"),
    ] {
        assert!(paths.contains(&want.as_str()), "{want}");
    }
    let shops: Vec<&str> = plan
        .registrations
        .iter()
        .filter(|r| r.file_type == "SHOP_PED_APPAREL_META_FILE")
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(
        shops,
        [
            "common/data/mp_m_freemode_01_mp_m_demo_1_shop.meta",
            "common/data/mp_m_freemode_01_mp_m_demo_shop.meta"
        ]
    );

    let out = tempfile::tempdir().unwrap();
    let (mut built, stopped) = or_project::build_all(std::slice::from_ref(&plan), out.path());
    assert!(stopped.is_none(), "{stopped:?}");
    let one = built.remove(0);
    assert!(
        one.verification.problems.is_empty(),
        "{:?}",
        one.verification.problems
    );
    let (wardrobe, planned) = or_wardrobe::planned_for(
        &project.folder(),
        project.container.module("wardrobe").unwrap(),
        Depth::Digest,
        project.container.output("dlc").unwrap(),
    )
    .unwrap();
    let files = or_project::built_files(&plan, &one.folder).unwrap();
    let found = verify_files(&wardrobe, &planned, files.as_ref());
    assert!(found.is_clean(), "{:?}", found.problems());
    assert!(found
        .checks
        .iter()
        .any(|c| c.name.contains("variation metadata of mp_m_demo_1") && c.passed));
}
