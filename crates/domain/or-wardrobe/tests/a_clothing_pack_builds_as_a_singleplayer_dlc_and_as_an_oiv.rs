//! A clothing pack in a singleplayer DLC and in an `.oiv`: the files are laid out as the game's own packs lay them,
//! the pack registers the shop metadata with its type and each archive as a file, the build reads every file back through
//! the archives it is in, and the clothing engine's own check of the metadata passes on what was read.
//!
//! ⚠ Nobody here can run GTA V: this holds that what was written reopens, not that the game mounts it.

mod common;

use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use common::{garment, put_bytes, ydd, ytd, FEET, JBIB, LOWR, P_HEAD};
use or_io::container::rpf::{NoOpCryptoProvider, RpfArchive};
use or_project::{build_all, built_files, Container, Module, Output, OutputKind, Plan, Project};
use or_title::dlc::ContentXml;
use or_title::oiv::PackageReader;
use or_wardrobe::verify::verify_files;
use or_wardrobe::{planned_for, plans, Depth, Expression, Ped, Wardrobe};

const MALE: &str = "mp_m_freemode_01_mp_m_demo";
const MALE_PROPS: &str = "mp_m_freemode_01_p_mp_m_demo";

/// A pack of a male jacket with an alternate model, a male hat, a male pair of heels with an expression and a female pair of trousers.
fn pack(folder: &Path) -> Wardrobe {
    put_bytes(folder, "models/jacket.ydd", &ydd("jacket"));
    put_bytes(folder, "models/jacket_1.ydd", &ydd("jacket-alternate"));
    put_bytes(folder, "models/hat.ydd", &ydd("hat"));
    put_bytes(folder, "models/heels.ydd", &ydd("heels"));
    put_bytes(folder, "models/trousers.ydd", &ydd("trousers"));
    for name in ["a", "b", "hat", "heels", "t"] {
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
    let trousers = garment(
        Ped::Female,
        LOWR,
        Some(0),
        "models/trousers.ydd",
        &["textures/t.ytd"],
    );
    w.garments.extend([jacket, hat, heels, trousers]);
    w.write(folder).unwrap();
    w
}

/// A project in `dir` whose clothing content is `dir` and which declares `output`.
fn project(dir: &Path, output: Output) -> Project {
    let container = Container {
        modules: vec![Module::clothing("wardrobe", ".")],
        outputs: vec![output],
    };
    let path = dir.join("pack.openrage-proj");
    Project::create(&path, &container, &[]).unwrap();
    Project::load(&path).unwrap()
}

fn open(bytes: Vec<u8>, name: &str) -> RpfArchive {
    RpfArchive::from_bytes_with(Some(name), bytes, Arc::new(NoOpCryptoProvider::new()))
        .unwrap_or_else(|e| panic!("{name} opens: {e}"))
}

fn names(archive: &RpfArchive) -> Vec<String> {
    archive
        .nodes
        .iter()
        .filter(|n| !n.is_dir())
        .map(|n| {
            n.path()
                .replace('\\', "/")
                .trim_start_matches('/')
                .to_string()
        })
        .collect()
}

fn read(archive: &RpfArchive, path: &str) -> Vec<u8> {
    let node = archive
        .nodes
        .iter()
        .find(|n| n.path().replace('\\', "/").trim_start_matches('/') == path)
        .unwrap_or_else(|| panic!("{path} in {:?}", names(archive)));
    archive.read_entry(node.entry_index()).unwrap()
}

fn built(plan: &Plan, out: &Path) -> or_project::Built {
    let (mut built, stopped) = build_all(std::slice::from_ref(plan), out);
    assert!(stopped.is_none(), "{stopped:?}");
    built.remove(0)
}

#[test]
fn a_pack_is_laid_out_as_the_games_packs_are_and_registers_what_the_game_is_told_of() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    let project = project(
        dir.path(),
        Output::of(OutputKind::SingleplayerDlc, "dlc", "demo", &["wardrobe"]),
    );
    let plan = plans(&project, "dlc").unwrap().remove(0);
    assert!(plan.is_buildable(), "{:?}", plan.problems);

    let registered = |ty: &str| -> Vec<&str> {
        plan.registrations
            .iter()
            .filter(|r| r.file_type == ty)
            .map(|r| r.path.as_str())
            .collect()
    };
    assert_eq!(
        registered("RPF_FILE"),
        [
            "x64/anim/creaturemetadata.rpf",
            "x64/models/cdimages/demo_female.rpf",
            "x64/models/cdimages/demo_male.rpf",
            "x64/models/cdimages/demo_male_p.rpf",
        ]
    );
    assert_eq!(
        registered("SHOP_PED_APPAREL_META_FILE"),
        [
            "common/data/mp_f_freemode_01_mp_f_demo_shop.meta",
            "common/data/mp_m_freemode_01_mp_m_demo_shop.meta",
        ]
    );
    assert_eq!(plan.registrations.len(), 6, "{:?}", plan.registrations);
    assert_eq!(plan.dlclist_entry.as_deref(), Some("dlcpacks:/demo/"));

    // Inside the archives: a folder named for the collection, its files named without it, the variation metadata beside the folder.
    let paths: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
    for want in [
        format!("x64/models/cdimages/demo_male.rpf/{MALE}.ymt"),
        format!("x64/models/cdimages/demo_male.rpf/{MALE}/jbib_000_u.ydd"),
        format!("x64/models/cdimages/demo_male.rpf/{MALE}/jbib_000_u_1.ydd"),
        format!("x64/models/cdimages/demo_male.rpf/{MALE}/jbib_diff_000_b_uni.ytd"),
        format!("x64/models/cdimages/demo_male_p.rpf/{MALE_PROPS}/p_head_000.ydd"),
        format!("x64/models/cdimages/demo_male_p.rpf/{MALE_PROPS}/p_head_diff_000_a.ytd"),
        "x64/anim/creaturemetadata.rpf/mp_creaturemetadata_demo.ymt".to_string(),
    ] {
        assert!(paths.contains(&want.as_str()), "{want} in {paths:?}");
    }
    assert!(
        !paths
            .iter()
            .any(|p| p.starts_with("stream/") || p.contains('^')),
        "a pack has no stream folder and no collection in a file's name: {paths:?}"
    );
}

#[test]
fn a_dlc_rpf_reopens_through_the_readers_and_the_clothing_engine_checks_what_was_read() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    let project = project(
        dir.path(),
        Output::of(OutputKind::SingleplayerDlc, "dlc", "demo", &["wardrobe"]),
    );
    let plan = plans(&project, "dlc").unwrap().remove(0);
    let out = tempfile::tempdir().unwrap();
    let one = built(&plan, out.path());

    assert!(
        one.verification.problems.is_empty(),
        "{:?}",
        one.verification.problems
    );
    assert!(one
        .verification
        .reloaded
        .iter()
        .any(|r| r.path.ends_with(".ymt")));
    assert!(one
        .verification
        .reloaded
        .iter()
        .any(|r| r.path.ends_with("jbib_000_u.ydd")));

    // The archive is the file, and the archives in it hold the files.
    let pack_file = one.folder.join("dlc.rpf");
    let root = RpfArchive::open(&pack_file).expect("the pack opens");
    let male = open(
        read(&root, "x64/models/cdimages/demo_male.rpf"),
        "demo_male.rpf",
    );
    assert!(
        names(&male).contains(&format!("{MALE}/jbib_000_u.ydd")),
        "{:?}",
        names(&male)
    );
    assert!(names(&male).contains(&format!("{MALE}.ymt")));
    let props = open(
        read(&root, "x64/models/cdimages/demo_male_p.rpf"),
        "demo_male_p.rpf",
    );
    assert_eq!(
        names(&props).iter().filter(|n| n.ends_with(".ydd")).count(),
        1,
        "{:?}",
        names(&props)
    );
    // content.xml registers the shop metadata with its type.
    let content = ContentXml::parse(&read(&root, "content.xml")).unwrap();
    let shop = content
        .data_files
        .iter()
        .filter(|d| d.file_type == "SHOP_PED_APPAREL_META_FILE")
        .count();
    assert_eq!(shop, 2);

    // The engine's own check, through the archives.
    let module = project.container.module("wardrobe").unwrap();
    let output = project.container.output("dlc").unwrap();
    let (wardrobe, planned) =
        planned_for(&project.folder(), module, Depth::Digest, output).unwrap();
    let files = built_files(&plan, &one.folder).expect("the pack opens");
    let checked = verify_files(&wardrobe, &planned, files.as_ref());
    let failed: Vec<_> = checked.checks.iter().filter(|c| !c.passed).collect();
    assert!(failed.is_empty(), "{failed:?}");
    assert!(
        checked.checks.iter().any(|c| c.name.contains("variation")),
        "{:?}",
        checked.checks.iter().map(|c| &c.name).collect::<Vec<_>>()
    );

    // The same pack twice is the same bytes.
    let out_again = tempfile::tempdir().unwrap();
    let again = built(&plan, out_again.path());
    assert_eq!(
        std::fs::read(&pack_file).unwrap(),
        std::fs::read(again.folder.join("dlc.rpf")).unwrap()
    );
}

#[test]
fn an_oiv_lists_the_files_of_the_pack_it_installs_and_reads_back_through_the_clothing_check() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    let project = project(
        dir.path(),
        Output::of(OutputKind::OivPackage, "oiv", "demo", &["wardrobe"]),
    );
    let plan = plans(&project, "oiv").unwrap().remove(0);
    assert!(plan.is_buildable(), "{:?}", plan.problems);
    let root = "update/x64/dlcpacks/demo/dlc.rpf";
    for want in [
        format!("{root}/content.xml"),
        format!("{root}/setup2.xml"),
        format!("{root}/x64/models/cdimages/demo_male.rpf/{MALE}/jbib_000_u.ydd"),
        format!("{root}/common/data/mp_m_freemode_01_mp_m_demo_shop.meta"),
    ] {
        assert!(plan.files.iter().any(|f| f.path == want), "{want}");
    }
    assert!(
        plan.notes.iter().any(|n| n.contains("dlclist.xml")),
        "{:?}",
        plan.notes
    );

    let out = tempfile::tempdir().unwrap();
    let one = built(&plan, out.path());
    assert!(
        one.verification.problems.is_empty(),
        "{:?}",
        one.verification.problems
    );
    let file = std::fs::File::open(one.folder.join("demo.oiv")).unwrap();
    let package = PackageReader::new(BufReader::new(file)).unwrap();
    let adds = &package.package.adds;
    assert_eq!(
        adds.len(),
        plan.files.len() - 1,
        "an add for each file but the manifest"
    );
    let jacket = adds
        .iter()
        .find(|a| a.destination.ends_with("jbib_000_u.ydd"))
        .expect("the model is listed");
    assert_eq!(
        jacket.archives,
        [root, "x64/models/cdimages/demo_male.rpf"],
        "inside the pack, inside the archive of the male components"
    );

    let module = project.container.module("wardrobe").unwrap();
    let output = project.container.output("oiv").unwrap();
    let (wardrobe, planned) =
        planned_for(&project.folder(), module, Depth::Digest, output).unwrap();
    let files = built_files(&plan, &one.folder).unwrap();
    let checked = verify_files(&wardrobe, &planned, files.as_ref());
    let failed: Vec<_> = checked.checks.iter().filter(|c| !c.passed).collect();
    assert!(failed.is_empty(), "{failed:?}");
}

/// The pack of [`pack`] with the jacket changing the game's hair while it is worn.
fn pack_with_a_set(folder: &Path) -> Wardrobe {
    use or_wardrobe::alternate::{self, Asset};
    let mut w = pack(folder);
    let jacket = w.garments[0].id.clone();
    let set = alternate::new_set(&mut w, Ped::Male, "hair under a jacket").unwrap();
    let hair = or_wardrobe::Slot::Component(or_wardrobe::Component::Hair);
    alternate::add_switch(
        &mut w,
        &set,
        Asset::outside(Some("mp_m_example_hair"), hair, 4),
        1,
    )
    .unwrap();
    alternate::join(&mut w, &set, Asset::garment(&jacket)).unwrap();
    w.write(folder).unwrap();
    w
}

#[test]
fn a_set_is_registered_with_the_type_the_game_reads_alternate_variations_by_in_a_resource_and_in_a_pack(
) {
    // A pack of a singleplayer DLC: a file of its common data, registered under its type.
    let dir = tempfile::tempdir().unwrap();
    pack_with_a_set(dir.path());
    let pack_project = project(
        dir.path(),
        Output::of(OutputKind::SingleplayerDlc, "dlc", "demo", &["wardrobe"]),
    );
    let plan = plans(&pack_project, "dlc").unwrap().remove(0);
    assert!(plan.is_buildable(), "{:?}", plan.problems);
    let registered: Vec<&str> = plan
        .registrations
        .iter()
        .filter(|r| r.file_type == "ALTERNATE_VARIATIONS_FILE")
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(registered, ["common/data/demo_pedalternatevariations.meta"]);
    assert!(plan.files.iter().any(|f| f.path == registered[0]));

    // A FiveM resource: a line of the manifest and a file of the resource's data.
    let dir = tempfile::tempdir().unwrap();
    pack_with_a_set(dir.path());
    let resource_project = project(
        dir.path(),
        Output::fivem_resource("fivem", "demo_resource", &["wardrobe"]),
    );
    let plan = plans(&resource_project, "fivem").unwrap().remove(0);
    let manifest = plan
        .files
        .iter()
        .find(|f| f.path == "fxmanifest.lua")
        .expect("a manifest");
    let or_project::Origin::Document(text) = &manifest.origin else {
        panic!("the manifest is a document");
    };
    assert!(
        text.contains("data_file 'ALTERNATE_VARIATIONS_FILE' 'data/pedalternatevariations.meta'"),
        "{text}"
    );
    assert!(
        text.contains("  'data/pedalternatevariations.meta',"),
        "the file is one of those the resource lists: {text}"
    );
}
