//! The options on a build change what the plan of a clothing pack writes, each as it says and nothing else, and a build that
//! states none writes what a build always wrote.

mod common;

use std::path::Path;

use common::{garment, put_bytes, ydd, ytd, FEET, JBIB, P_HEAD};
use or_io::decl::File;
use or_io::meta::CPedVariationInfo;
use or_project::{Origin, OutputKind};
use or_wardrobe::layout::Paths;
use or_wardrobe::options::{DumpKind, Options};
use or_wardrobe::plan::Build;
use or_wardrobe::{inspect, plan, plan_for, Depth, Expression, Ped, Planned, Wardrobe};

const M: &str = "mp_m_freemode_01_mp_m_demo";

/// A pack of a jacket with an alternate model and two textures, a hat, and a pair of heels that state an expression.
fn pack(folder: &Path) -> Wardrobe {
    put_bytes(folder, "models/jacket.ydd", &ydd("jacket"));
    put_bytes(folder, "models/jacket_1.ydd", &ydd("jacket-alternate"));
    put_bytes(folder, "models/hat.ydd", &ydd("hat"));
    put_bytes(folder, "models/heels.ydd", &ydd("heels"));
    for name in ["a", "b", "hat", "heels"] {
        put_bytes(folder, &format!("textures/{name}.ytd"), &ytd(name));
    }
    let mut w = Wardrobe::new("demo");
    let mut jacket = garment(
        Ped::Male,
        JBIB,
        Some(0),
        "models/jacket.ydd",
        &["textures/a.ytd", "textures/b.ytd"],
    );
    jacket.alternates = vec!["models/jacket_1.ydd".to_string()];
    let hat = garment(
        Ped::Male,
        P_HEAD,
        Some(0),
        "models/hat.ydd",
        &["textures/hat.ytd"],
    );
    let mut heels = garment(
        Ped::Male,
        FEET,
        Some(0),
        "models/heels.ydd",
        &["textures/heels.ytd"],
    );
    heels.expressions = vec![Expression {
        index: 4,
        tracks: vec![33],
        ids: vec![28462],
        types: vec![2],
        components: vec![1],
    }];
    w.garments.extend([jacket, hat, heels]);
    w.write(folder).unwrap();
    w
}

fn build(w: &Wardrobe, options: Options) -> Build {
    Build {
        paths: Paths::resource(&w.pack, options.subfolders),
        options,
        kind: OutputKind::FivemResource,
    }
}

fn planned(folder: &Path, w: &Wardrobe, options: Options) -> Planned {
    let facts = inspect(folder, w, Depth::Open);
    plan_for(w, &facts, "pack", folder, &build(w, options))
}

fn paths(p: &Planned) -> Vec<String> {
    p.contribution
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect()
}

fn text(p: &Planned, path: &str) -> String {
    let file = p
        .contribution
        .files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} in {:?}", paths(p)));
    match &file.origin {
        Origin::Document(text) => text.clone(),
        other => panic!("{path} is not a document: {other:?}"),
    }
}

fn variation(p: &Planned) -> CPedVariationInfo {
    let path = format!("stream/demo/{M}.ymt");
    let file = p
        .contribution
        .files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} in {:?}", paths(p)));
    let Origin::Generated(bytes) = &file.origin else {
        panic!("generated")
    };
    CPedVariationInfo::from_bytes(bytes).unwrap()
}

/// The alternates the metadata states for the first top: components are listed in the order of their slots, and a top is the twelfth.
fn jacket_alternatives(info: &CPedVariationInfo) -> u8 {
    let at = usize::from(info.avail_comp[11]);
    info.a_component_data3[at].a_drawbl_data3[0].num_alternatives
}

#[test]
fn a_build_that_states_none_writes_what_a_pack_always_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let facts = inspect(dir.path(), &w, Depth::Open);
    let before = plan(&w, &facts, "pack", dir.path());
    let after = planned(dir.path(), &w, Options::default());
    assert_eq!(paths(&before), paths(&after));
    assert!(paths(&after).contains(&format!("stream/demo/{M}/jbib/{M}^jbib_000_u.ydd")));
    assert!(paths(&after)
        .contains(&"stream/demo/creaturemetadata/mp_creaturemetadata_demo.ymt".to_string()));
    assert!(!paths(&after).iter().any(|p| p.contains("clothes_dump")));
}

#[test]
fn no_sub_folders_puts_the_files_of_a_ped_in_one_folder() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let flat = planned(
        dir.path(),
        &w,
        Options {
            subfolders: false,
            ..Options::default()
        },
    );
    let files = paths(&flat);
    assert!(
        files.contains(&format!("stream/demo/{M}/{M}^jbib_000_u.ydd")),
        "{files:?}"
    );
    assert!(
        files.contains(&format!("stream/demo/{M}/{M}^jbib_diff_000_b_uni.ytd")),
        "{files:?}"
    );
    assert!(
        !files
            .iter()
            .any(|p| p.contains("/jbib/") || p.contains("/feet/")),
        "{files:?}"
    );
    // Only where the files are changed: the number of files and the metadata are the default's.
    let default = planned(dir.path(), &w, Options::default());
    assert_eq!(files.len(), paths(&default).len());
}

#[test]
fn no_extra_ymt_leaves_the_creature_metadata_out_and_says_what_the_heels_lose() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let default = planned(dir.path(), &w, Options::default());
    assert!(text(&default, "data/mp_m_freemode_01_mp_m_demo.meta").contains("<creatureMetaData>"));
    assert!(default.documents.creature.is_some());

    let bare = planned(
        dir.path(),
        &w,
        Options {
            creature_metadata: false,
            ..Options::default()
        },
    );
    assert!(
        !paths(&bare).iter().any(|p| p.contains("creaturemetadata")),
        "{:?}",
        paths(&bare)
    );
    assert!(!text(&bare, "data/mp_m_freemode_01_mp_m_demo.meta").contains("creatureMetaData"));
    assert!(bare.documents.creature.is_none());
    assert!(
        bare.contribution
            .notes
            .iter()
            .any(|n| n.contains("creature metadata") && n.contains("1 garment")),
        "{:?}",
        bare.contribution.notes
    );
}

#[test]
fn no_alternate_variations_leaves_the_alternate_model_out_and_states_none() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let default = planned(dir.path(), &w, Options::default());
    assert!(paths(&default)
        .iter()
        .any(|p| p.ends_with("jbib_000_u_1.ydd")));
    assert_eq!(jacket_alternatives(&variation(&default)), 1);

    let without = planned(
        dir.path(),
        &w,
        Options {
            alternates: false,
            ..Options::default()
        },
    );
    assert!(
        !paths(&without).iter().any(|p| p.ends_with("_1.ydd")),
        "{:?}",
        paths(&without)
    );
    assert_eq!(jacket_alternatives(&variation(&without)), 0);
}

#[test]
fn a_collection_prefix_is_in_every_name_the_build_writes_and_in_the_claim() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let prefixed = planned(
        dir.path(),
        &w,
        Options {
            prefix: "srv_".into(),
            ..Options::default()
        },
    );
    let stem = "mp_m_freemode_01_srv_mp_m_demo";
    let files = paths(&prefixed);
    assert!(
        files.contains(&format!("stream/demo/{stem}/jbib/{stem}^jbib_000_u.ydd")),
        "{files:?}"
    );
    assert!(files.contains(&format!("data/{stem}.meta")), "{files:?}");
    assert!(
        text(&prefixed, &format!("data/{stem}.meta")).contains("<dlcName>srv_mp_m_demo</dlcName>")
    );
    assert!(
        prefixed
            .contribution
            .claims
            .iter()
            .any(|c| c.name == "srv_mp_m_demo"),
        "{:?}",
        prefixed.contribution.claims
    );
    assert!(
        !files.iter().any(|p| p.contains(&format!("{M}^"))),
        "no name keeps the old collection"
    );
}

#[test]
fn a_clothes_dump_is_carried_as_json_and_as_a_lua_table_only_as_asked() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let none = planned(dir.path(), &w, Options::default());
    assert!(!paths(&none).iter().any(|p| p.contains("clothes_dump")));

    let json = planned(
        dir.path(),
        &w,
        Options {
            dump: DumpKind::Json,
            ..Options::default()
        },
    );
    assert!(paths(&json).contains(&"data/clothes_dump.json".to_string()));
    assert!(!paths(&json).contains(&"data/clothes_dump.lua".to_string()));

    let both = planned(
        dir.path(),
        &w,
        Options {
            dump: DumpKind::Both,
            ..Options::default()
        },
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&text(&both, "data/clothes_dump.json")).expect("the dump is JSON");
    assert_eq!(
        parsed.as_array().map(Vec::len),
        Some(1),
        "one ped has garments"
    );
    assert_eq!(parsed[0]["DlcCollectionName"], "mp_m_demo");
    let lua = text(&both, "data/clothes_dump.lua");
    assert!(
        lua.starts_with("-- ")
            && lua.contains("return {")
            && lua.contains("DlcCollectionName = \"mp_m_demo\"")
    );
    assert!(
        both.contribution.problems.is_empty(),
        "{:?}",
        both.contribution.problems
    );
}
