//! A headless build over staged files: a pack of real drawables and textures planned into the
//! structure the staged packs have, built, read back, and the same twice.
//!
//! ⚠ The structure below is written out, not computed from the layout functions: it is what the
//! staged resource has (`stream/<pack>/<ped>_<collection>/<slot>/<ped>_<collection>^<slot>_<number>_<skin>.ydd`
//! and the rest), so the planner is held to the packs that exist and not to itself.

mod common;

use common::shop;
use common::{garment, project, put, snapshot, staged, FEET, JBIB, LOWR, P_HEAD};
use or_io::decl::File;
use or_io::meta::{CCreatureMetaData, CPedVariationInfo};
use or_project::{build, Origin};
use or_wardrobe::{plans, verify::verify, Depth, Expression, Garment, Ped, Skin, Wardrobe};

const MALE: &str = "mp_m_freemode_01_mp_m_police";
const FEMALE: &str = "mp_f_freemode_01_mp_f_police";

/// The pack, its files laid out in `dir`, and the staged files it was made of.
fn laid_out(dir: &std::path::Path) -> Option<Wardrobe> {
    let ydds = staged("ydd", 5, "a_pack_of_staged_files_builds_and_reads_back")?;
    let ytds = staged("ytd", 6, "a_pack_of_staged_files_builds_and_reads_back")?;
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
    late.label = Some("late".into());
    let mut early = garment(Ped::Male, JBIB, None, "models/m1.ydd", &["textures/t2.ytd"]);
    early.alternates = vec!["models/m3.ydd".into()];
    let mut trousers = garment(
        Ped::Male,
        LOWR,
        None,
        "models/m2.ydd",
        &["textures/t3.ytd", "textures/t4.ytd", "textures/t5.ytd"],
    );
    trousers.skin = Skin::Race;
    let hat = garment(
        Ped::Male,
        P_HEAD,
        None,
        "models/m3.ydd",
        &["textures/t0.ytd", "textures/t1.ytd"],
    );
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
    held.variations = vec![Default::default(), Default::default()];
    w.garments = vec![late, early, trousers, hat, shoes, held];
    Some(w)
}

fn path_set(plan: &or_project::Plan) -> Vec<String> {
    plan.files.iter().map(|f| f.path.clone()).collect()
}

#[test]
fn the_plan_has_the_structure_of_the_staged_packs() {
    let dir = tempfile::tempdir().unwrap();
    let Some(w) = laid_out(dir.path()) else {
        return;
    };
    let project = project(dir.path(), &w, "police_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);

    let mut expected: Vec<String> = [
        "fxmanifest.lua",
        "data/mp_f_freemode_01_mp_f_police.meta",
        "data/mp_m_freemode_01_mp_m_police.meta",
        "stream/police/creaturemetadata/mp_creaturemetadata_police.ymt",
        "stream/police/mp_f_freemode_01_mp_f_police.ymt",
        "stream/police/mp_m_freemode_01_mp_m_police.ymt",
        // The garment pinned at 3, and the one with no pin, which takes 0 (the held place is 1).
        "stream/police/mp_m_freemode_01_mp_m_police/jbib/mp_m_freemode_01_mp_m_police^jbib_000_u.ydd",
        // An alternate is the model's name with `_1` before the extension.
        "stream/police/mp_m_freemode_01_mp_m_police/jbib/mp_m_freemode_01_mp_m_police^jbib_000_u_1.ydd",
        "stream/police/mp_m_freemode_01_mp_m_police/jbib/mp_m_freemode_01_mp_m_police^jbib_diff_000_a_uni.ytd",
        "stream/police/mp_m_freemode_01_mp_m_police/jbib/mp_m_freemode_01_mp_m_police^jbib_003_u.ydd",
        "stream/police/mp_m_freemode_01_mp_m_police/jbib/mp_m_freemode_01_mp_m_police^jbib_diff_003_a_uni.ytd",
        "stream/police/mp_m_freemode_01_mp_m_police/jbib/mp_m_freemode_01_mp_m_police^jbib_diff_003_b_uni.ytd",
        // A race garment is `_r`, and its textures end in `whi`.
        "stream/police/mp_m_freemode_01_mp_m_police/lowr/mp_m_freemode_01_mp_m_police^lowr_000_r.ydd",
        "stream/police/mp_m_freemode_01_mp_m_police/lowr/mp_m_freemode_01_mp_m_police^lowr_diff_000_a_whi.ytd",
        "stream/police/mp_m_freemode_01_mp_m_police/lowr/mp_m_freemode_01_mp_m_police^lowr_diff_000_b_whi.ytd",
        "stream/police/mp_m_freemode_01_mp_m_police/lowr/mp_m_freemode_01_mp_m_police^lowr_diff_000_c_whi.ytd",
        // A prop has `_p_` in its folder and no skin or tint.
        "stream/police/mp_m_freemode_01_p_mp_m_police/p_head/mp_m_freemode_01_p_mp_m_police^p_head_000.ydd",
        "stream/police/mp_m_freemode_01_p_mp_m_police/p_head/mp_m_freemode_01_p_mp_m_police^p_head_diff_000_a.ytd",
        "stream/police/mp_m_freemode_01_p_mp_m_police/p_head/mp_m_freemode_01_p_mp_m_police^p_head_diff_000_b.ytd",
        "stream/police/mp_f_freemode_01_mp_f_police/feet/mp_f_freemode_01_mp_f_police^feet_001_u.ydd",
        "stream/police/mp_f_freemode_01_mp_f_police/feet/mp_f_freemode_01_mp_f_police^feet_diff_001_a_uni.ytd",
    ]
    .map(String::from)
    .to_vec();
    expected.sort();
    assert_eq!(path_set(&plan), expected);

    // The manifest registers each ped's collection with its shop metadata, as the staged one does.
    let manifest = plan
        .files
        .iter()
        .find(|f| f.path == "fxmanifest.lua")
        .unwrap();
    let Origin::Document(text) = &manifest.origin else {
        panic!("the manifest is a document");
    };
    for ped in [MALE, FEMALE] {
        assert!(
            text.contains(&format!(
                "data_file 'SHOP_PED_APPAREL_META_FILE' 'data/{ped}.meta'"
            )),
            "{text}"
        );
    }
    // The shop metadata is data a resource lists in its files; the metadata and the models are streamed.
    assert!(
        text.contains("'data/mp_m_freemode_01_mp_m_police.meta'"),
        "{text}"
    );
    assert!(!text.contains("stream/"), "{text}");
}

#[test]
fn the_variation_metadata_states_the_places_the_garments_have() {
    let dir = tempfile::tempdir().unwrap();
    let Some(w) = laid_out(dir.path()) else {
        return;
    };
    let project = project(dir.path(), &w, "police_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    let ymt = |name: &str| -> CPedVariationInfo {
        let file = plan.files.iter().find(|f| f.path.ends_with(name)).unwrap();
        let Origin::Generated(bytes) = &file.origin else {
            panic!("metadata is generated");
        };
        CPedVariationInfo::from_bytes(bytes).unwrap()
    };

    let male = ymt(&format!("{MALE}.ymt"));
    // jbib (slot 11) and lowr (slot 4): the components take data in the order of their numbers.
    assert_eq!(male.avail_comp[4], 0);
    assert_eq!(male.avail_comp[11], 1);
    assert_eq!(male.avail_comp.iter().filter(|i| **i != 255).count(), 2);
    // Four drawables in jbib (0, the held 1, the gap 2 and the pinned 3), and the textures each has.
    let jbib = &male.a_component_data3[1];
    let textures: Vec<usize> = jbib
        .a_drawbl_data3
        .iter()
        .map(|d| d.a_tex_data.len())
        .collect();
    assert_eq!(
        textures,
        [1, 2, 1, 2],
        "garment, held place, the gap, garment"
    );
    assert_eq!(
        jbib.a_drawbl_data3
            .iter()
            .map(|d| d.num_alternatives)
            .collect::<Vec<_>>(),
        [1, 0, 0, 0],
        "the alternates the first garment has"
    );
    assert_eq!(
        usize::from(jbib.num_avail_tex),
        textures.iter().sum::<usize>()
    );
    // A race drawable states texId 1 in each of its textures.
    assert!(male.a_component_data3[0].a_drawbl_data3[0]
        .a_tex_data
        .iter()
        .all(|t| t.tex_id == 1));
    // A drawable has a component info and only one, sorted by slot and position.
    let infos: Vec<(u8, u8)> = male
        .comp_infos
        .iter()
        .map(|i| (i.ped_xml_comp_idx, i.ped_xml_drawbl_idx))
        .collect();
    assert_eq!(infos, [(4, 0), (11, 0), (11, 1), (11, 2), (11, 3)]);
    // The prop is the only one, at the head anchor, with two textures.
    assert_eq!(male.prop_info.num_avail_props, 1);
    assert_eq!(male.prop_info.a_anchors[0].props, [2]);
    assert_eq!(male.dlc_name.hash, or_io::string_hash(b"mp_m_police"));

    let female = ymt(&format!("{FEMALE}.ymt"));
    let feet = &female.a_component_data3[usize::from(female.avail_comp[6])];
    assert_eq!(
        feet.a_drawbl_data3.len(),
        2,
        "the pin at 1 leaves a hole at 0"
    );
}

#[test]
fn the_creature_metadata_is_one_rbf_file_with_the_expressions_of_the_pack() {
    let dir = tempfile::tempdir().unwrap();
    let Some(w) = laid_out(dir.path()) else {
        return;
    };
    let project = project(dir.path(), &w, "police_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    let file = plan
        .files
        .iter()
        .find(|f| f.path.ends_with("mp_creaturemetadata_police.ymt"))
        .unwrap();
    let Origin::Generated(bytes) = &file.origin else {
        panic!("generated");
    };
    assert_eq!(&bytes[..4], b"RBF0");
    let creature = CCreatureMetaData::from_bytes(bytes).unwrap();
    assert_eq!(creature.ped_comp_expressions.len(), 1);
    let e = &creature.ped_comp_expressions[0];
    assert_eq!(
        (
            e.ped_comp_id,
            e.ped_comp_var_index,
            e.ped_comp_expression_index
        ),
        (6, 1, 4)
    );
    assert_eq!((&e.tracks[..], &e.ids[..]), (&[33u8][..], &[28462u16][..]));
}

#[test]
fn the_pack_builds_reads_back_and_builds_the_same_twice() {
    let dir = tempfile::tempdir().unwrap();
    let Some(w) = laid_out(dir.path()) else {
        return;
    };
    let project = project(dir.path(), &w, "police_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    let out = or_project::build_root(&project.path);

    let built = build(&plan, &out).expect("the pack builds");
    // Every file reloaded through a reader: the drawables, the textures and the metadata.
    let reloaded: Vec<&str> = built
        .verification
        .reloaded
        .iter()
        .map(|r| r.path.as_str())
        .collect();
    assert!(
        reloaded.iter().any(|p| p.ends_with(".ydd"))
            && reloaded.iter().any(|p| p.ends_with(".ytd"))
    );
    assert!(
        reloaded.iter().filter(|p| p.ends_with(".ymt")).count() == 3,
        "{reloaded:?}"
    );

    let planned = or_wardrobe::planned(
        &project.folder(),
        project.container.module("wardrobe").unwrap(),
        Depth::Digest,
    )
    .unwrap();
    let found = verify(&w, &planned, &built.folder);
    assert!(found.is_clean(), "{:?}", found.problems());
    assert_eq!(
        found.checks.len(),
        6,
        "two variation files, two shop files, one creature file, and what the garments do"
    );

    let first = snapshot(&built.folder);
    let again = build(&plans(&project, "fivem").unwrap().remove(0), &out).expect("it builds again");
    assert_eq!(
        snapshot(&again.folder),
        first,
        "two builds of one pack are byte-identical"
    );
}

#[test]
fn a_file_that_is_not_there_stops_the_plan_and_names_the_garment() {
    let dir = tempfile::tempdir().unwrap();
    let Some(mut w) = laid_out(dir.path()) else {
        return;
    };
    std::fs::remove_file(dir.path().join("models/m1.ydd")).unwrap();
    w.garments[1].label = Some("the early one".into());
    let project = project(dir.path(), &w, "police_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    assert!(!plan.is_buildable());
    assert!(
        plan.problems
            .iter()
            .any(|p| p.starts_with("the early one names a model that is not there")),
        "{:?}",
        plan.problems
    );
}

/// An entry of the shop metadata as its elements, without the name the game hashes for it.
fn entry(fields: &[(&str, &str)]) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect()
}

#[test]
fn the_shop_metadata_registers_each_collection_and_has_an_entry_for_each_texture() {
    let dir = tempfile::tempdir().unwrap();
    let Some(w) = laid_out(dir.path()) else {
        return;
    };
    let project = project(dir.path(), &w, "police_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    let read = |name: &str| -> shop::Shop {
        let path = format!("data/{name}.meta");
        let file = plan.files.iter().find(|f| f.path == path).unwrap();
        let Origin::Document(text) = &file.origin else {
            panic!("shop metadata is a document");
        };
        shop::read(text)
    };

    // The header is the one the staged packs have: the ped, the collection as the game resolves it,
    // the character it is for and the creature metadata, in that order.
    let (male, female) = (read(MALE), read(FEMALE));
    assert_eq!(
        male.header,
        entry(&[
            ("pedName", "mp_m_freemode_01"),
            ("dlcName", "mp_m_police"),
            ("fullDlcName", MALE),
            ("eCharacter", "SCR_CHAR_MULTIPLAYER"),
            ("creatureMetaData", "MP_CreatureMetadata_police"),
        ])
    );
    assert_eq!(
        female.header,
        entry(&[
            ("pedName", "mp_f_freemode_01"),
            ("dlcName", "mp_f_police"),
            ("fullDlcName", FEMALE),
            ("eCharacter", "SCR_CHAR_MULTIPLAYER_F"),
            ("creatureMetaData", "MP_CreatureMetadata_police"),
        ])
    );

    // An entry per texture of each garment that ships a model, and none for the place held at jbib 1.
    let component = |drawable: &str, comp: &str, texture: &str| {
        entry(&[
            ("cost", "0"),
            ("localDrawableIndex", drawable),
            ("eCompType", comp),
            ("textureIndex", texture),
        ])
    };
    let mut expected = vec![
        component("0", "PV_COMP_JBIB", "0"),
        component("3", "PV_COMP_JBIB", "0"),
        component("3", "PV_COMP_JBIB", "1"),
        component("0", "PV_COMP_LOWR", "0"),
        component("0", "PV_COMP_LOWR", "1"),
        component("0", "PV_COMP_LOWR", "2"),
    ];
    expected.sort();
    let keep = ["cost", "localDrawableIndex", "eCompType", "textureIndex"].into();
    assert_eq!(shop::only(&male.components, &keep), expected);
    // Every entry names its hash first after the cost, which the staged packs do too.
    assert!(male.components.iter().all(|e| {
        e.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
            == [
                "cost",
                "uniqueNameHash",
                "localDrawableIndex",
                "eCompType",
                "textureIndex",
            ]
    }));
    assert_eq!(
        shop::only(&female.components, &keep),
        [component("1", "PV_COMP_FEET", "0")]
    );
    assert!(female.props.is_empty());

    let prop = |texture: &str| {
        entry(&[
            ("cost", "0"),
            ("restrictionTags", ""),
            ("localPropIndex", "0"),
            ("eAnchorPoint", "ANCHOR_HEAD"),
            ("textureIndex", texture),
        ])
    };
    let keep = [
        "cost",
        "restrictionTags",
        "localPropIndex",
        "eAnchorPoint",
        "textureIndex",
    ]
    .into();
    assert_eq!(shop::only(&male.props, &keep), [prop("0"), prop("1")]);
    assert!(male.props.iter().all(|e| {
        e.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
            == [
                "cost",
                "uniqueNameHash",
                "restrictionTags",
                "localPropIndex",
                "eAnchorPoint",
                "textureIndex",
            ]
    }));
}
