//! Importing the resource a build made gives a pack that plans the same resource again: the same
//! files at the same names, and metadata that is the same bytes.

mod common;

use common::{garment, project, put, snapshot, staged, FEET, JBIB, LOWR, P_HEAD};
use or_project::{build, build_root};
use or_wardrobe::import::{import, Options};
use or_wardrobe::{inspect, plan, plans, Depth, Expression, Garment, Ped, Skin, Wardrobe};

const WHAT: &str = "an_import_is_the_inverse_of_a_build";

fn pack(dir: &std::path::Path) -> Option<Wardrobe> {
    let ydds = staged("ydd", 5, WHAT)?;
    let ytds = staged("ytd", 6, WHAT)?;
    for (i, f) in ydds.iter().enumerate() {
        put(dir, &format!("models/m{i}.ydd"), f);
    }
    for (i, f) in ytds.iter().enumerate() {
        put(dir, &format!("textures/t{i}.ytd"), f);
    }
    let mut w = Wardrobe::new("police");
    let mut late = garment(
        Ped::Male,
        JBIB,
        Some(3),
        "models/m0.ydd",
        &["textures/t0.ytd", "textures/t1.ytd"],
    );
    late.meta.prop_mask = Some(3);
    late.meta.ped_xml_expression_mods = Some([0.0, 0.0, 0.185, 0.0, 0.0]);
    late.variations[1].tex_id = Some(0);
    late.variations[1].tint = Some("bla".into());
    let mut early = garment(Ped::Male, JBIB, None, "models/m1.ydd", &["textures/t2.ytd"]);
    early.alternates = vec!["models/m3.ydd".into(), "models/m4.ydd".into()];
    let mut trousers = garment(
        Ped::Male,
        LOWR,
        None,
        "models/m2.ydd",
        &["textures/t3.ytd", "textures/t4.ytd", "textures/t5.ytd"],
    );
    trousers.skin = Skin::Race;
    let mut hat = garment(
        Ped::Male,
        P_HEAD,
        None,
        "models/m3.ydd",
        &["textures/t0.ytd", "textures/t1.ytd"],
    );
    hat.meta.expression_mods = Some([-0.5, 0.0, 0.0, 0.0, 0.0]);
    let mut shoes = garment(
        Ped::Female,
        FEET,
        Some(1),
        "models/m4.ydd",
        &["textures/t2.ytd"],
    );
    shoes.expressions.push(Expression {
        index: 4,
        tracks: vec![33],
        ids: vec![28462],
        types: vec![2],
        components: vec![1],
    });
    let mut held = Garment::new();
    held.ped = Some(Ped::Male);
    held.slot = Some(JBIB);
    held.position = Some(1);
    held.meta.prop_mask = Some(5);
    held.variations = vec![Default::default(), Default::default()];
    w.garments = vec![late, early, trousers, hat, shoes, held];
    Some(w)
}

#[test]
fn the_pack_imported_from_its_own_build_plans_the_same_resource() {
    let dir = tempfile::tempdir().unwrap();
    let Some(w) = pack(dir.path()) else {
        return;
    };
    let source = project(dir.path(), &w, "police_pack");
    let first_plan = plans(&source, "fivem").unwrap().remove(0);
    assert!(first_plan.problems.is_empty(), "{:?}", first_plan.problems);
    let built = build(&first_plan, &build_root(&source.path)).expect("it builds");

    // The built resource, read in place: nothing is written into it.
    let before = snapshot(&built.folder);
    let imported = import(&built.folder, &Options::default()).expect("the build imports");
    assert_eq!(snapshot(&built.folder), before, "an import writes nothing");
    assert_eq!(imported.wardrobe.pack, "police");
    assert!(
        imported.wardrobe.collections.is_empty(),
        "the collections follow `mp_<m|f>_<pack>`"
    );
    assert_eq!(imported.report.models, 5);
    assert_eq!(imported.report.alternates, 2);
    assert_eq!(imported.report.textures, 9);
    // The held place at jbib 1, the gap at jbib 2 and the hole before the pinned female shoes.
    assert_eq!(imported.report.reserved, 3);
    assert!(
        imported.report.left_out.is_empty(),
        "{:?}",
        imported.report.left_out
    );
    assert!(
        imported
            .wardrobe
            .garments
            .iter()
            .all(|g| g.position.is_some()),
        "every garment is pinned at its number"
    );

    let again = plan(
        &imported.wardrobe,
        &inspect(&built.folder, &imported.wardrobe, Depth::Digest),
        "wardrobe",
        &built.folder,
    );
    assert!(
        again.contribution.problems.is_empty(),
        "{:?}",
        again.contribution.problems
    );

    // The same files at the same names, and the same bytes where the plan writes them.
    let by_path = |files: &[or_project::PlannedFile]| {
        files
            .iter()
            .filter_map(|f| match &f.origin {
                or_project::Origin::Generated(bytes) => Some((f.path.clone(), bytes.clone())),
                or_project::Origin::Document(text) => {
                    Some((f.path.clone(), text.clone().into_bytes()))
                }
                or_project::Origin::Copy { .. }
                | or_project::Origin::Retarget { .. }
                | or_project::Origin::Cook { .. } => None,
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let names = |files: &[or_project::PlannedFile]| {
        files.iter().map(|f| f.path.clone()).collect::<Vec<_>>()
    };
    let mut wanted: Vec<String> = names(&first_plan.files);
    wanted.retain(|p| p != "fxmanifest.lua");
    let mut got = names(&again.contribution.files);
    wanted.sort();
    got.sort();
    assert_eq!(got, wanted);
    // Two alternates make numAlternatives 2, which no `meta` states, and the import keeps them as files.
    let imported_early = imported
        .wardrobe
        .garments
        .iter()
        .find(|g| g.alternates.len() == 2)
        .expect("the garment with its alternates");
    assert!(imported_early.meta.num_alternatives.is_none());
    let first_generated = by_path(&first_plan.files);
    for (path, bytes) in by_path(&again.contribution.files) {
        assert_eq!(
            first_generated.get(&path),
            Some(&bytes),
            "{path} is not what the first build wrote"
        );
    }
}

#[test]
fn what_a_name_does_not_state_is_left_out_with_the_reason_and_nothing_is_guessed() {
    let dir = tempfile::tempdir().unwrap();
    let Some(ydds) = staged("ydd", 3, WHAT) else {
        return;
    };
    let Some(ytds) = staged("ytd", 1, WHAT) else {
        return;
    };
    put(
        dir.path(),
        "stream/mp_m_freemode_01_mp_m_x^jbib_000_u.ydd",
        &ydds[0],
    );
    put(
        dir.path(),
        "stream/mp_m_freemode_01_mp_m_x^jbib_diff_000_a_uni.ytd",
        &ytds[0],
    );
    put(dir.path(), "stream/shirt.ydd", &ydds[1]);
    put(
        dir.path(),
        "stream/mp_m_freemode_01_mp_m_x^jbib_diff_007_a_uni.ytd",
        &ytds[0],
    );
    put(
        dir.path(),
        "stream/mp_m_freemode_01_mp_m_x^jbib_001_u_1.ydd",
        &ydds[2],
    );
    // A source folder and the project's own are not looked into.
    put(
        dir.path(),
        "build/x/mp_m_freemode_01_mp_m_x^jbib_005_u.ydd",
        &ydds[0],
    );
    put(
        dir.path(),
        "a.textures.src/mp_m_freemode_01_mp_m_x^jbib_006_u.ydd",
        &ydds[0],
    );

    let imported = import(dir.path(), &Options::default()).expect("one garment imports");
    assert_eq!(imported.wardrobe.garments.len(), 1);
    assert_eq!(imported.wardrobe.garments[0].position, Some(0));
    let left: std::collections::BTreeMap<&str, &str> = imported
        .report
        .left_out
        .iter()
        .map(|l| (l.path.as_str(), l.why.as_str()))
        .collect();
    assert!(
        left["stream/shirt.ydd"].contains("the slot")
            && left["stream/shirt.ydd"].contains("the gender")
    );
    assert!(left["stream/mp_m_freemode_01_mp_m_x^jbib_diff_007_a_uni.ytd"].contains("no model"));
    assert!(left["stream/mp_m_freemode_01_mp_m_x^jbib_001_u_1.ydd"].contains("alternate"));
    assert_eq!(left.len(), 3, "{left:?}");
}

#[test]
fn an_import_never_replaces_a_wardrobe_and_a_folder_with_no_pack_has_nothing_to_import() {
    let dir = tempfile::tempdir().unwrap();
    assert!(import(dir.path(), &Options::default()).is_err());
    let Some(ydds) = staged("ydd", 1, WHAT) else {
        return;
    };
    put(
        dir.path(),
        "mp_f_freemode_01_mp_f_x^hair_000_u.ydd",
        &ydds[0],
    );
    let imported = import(dir.path(), &Options::default()).unwrap();
    or_wardrobe::import::write(dir.path(), &imported).expect("the first write");
    assert!(matches!(
        or_wardrobe::import::write(dir.path(), &imported),
        Err(or_wardrobe::import::Error::Exists(_))
    ));
    assert_eq!(Wardrobe::read(dir.path()).unwrap(), imported.wardrobe);
}

#[test]
fn a_file_of_a_pack_that_nothing_here_builds_is_left_out_with_the_reason_and_not_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let c = "mp_m_freemode_01_mp_m_x";
    let write = |rel: &str, bytes: &[u8]| {
        let at = dir.path().join(rel);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, bytes).unwrap();
    };
    write(&format!("stream/{c}^jbib_000_u.ydd"), b"model");
    write(&format!("stream/{c}^jbib_diff_000_a_uni.ytd"), b"texture");
    // A fragment, metadata that is neither variation metadata nor creature metadata, and a cloth with no model at its place.
    write("stream/frag/body.YFT", b"fragment");
    write("stream/other.ymt", b"not metadata");
    write(&format!("stream/{c}^lowr_000_u.yld"), b"cloth");
    // What a resource holds besides its streamed files is no file of a pack.
    write("fxmanifest.lua", b"fx_version 'cerulean'");
    write("data/readme.txt", b"hello");
    // The cloth of the garment is not left out: it travels with the model it is named after.
    write(&format!("stream/{c}^jbib_000_u.yld"), b"cloth");

    let imported = import(dir.path(), &Options::default()).expect("one garment imports");
    assert_eq!(
        (imported.report.models, imported.report.textures),
        (1, 1),
        "the cloth is not taken for a model"
    );
    assert_eq!(imported.report.cloth, 1);
    let left: std::collections::BTreeMap<&str, &str> = imported
        .report
        .left_out
        .iter()
        .map(|l| (l.path.as_str(), l.why.as_str()))
        .collect();
    assert_eq!(
        left.keys().copied().collect::<Vec<_>>(),
        [
            "stream/frag/body.YFT",
            format!("stream/{c}^lowr_000_u.yld").as_str(),
            "stream/other.ymt"
        ],
        "{left:?}"
    );
    assert!(left[format!("stream/{c}^lowr_000_u.yld").as_str()].contains("no model"));
    assert!(left["stream/frag/body.YFT"].contains(".yft"));
    assert!(left["stream/other.ymt"].contains("neither"));
}
