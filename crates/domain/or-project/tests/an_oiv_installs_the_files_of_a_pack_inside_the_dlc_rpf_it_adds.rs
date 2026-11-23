//! A content whose files are those of a singleplayer DLC pack is installed by an `.oiv` inside the
//! `dlc.rpf` the package adds, with the two documents that make it a pack, and the files of the
//! contents that are not a pack land where they always did.

use std::io::BufReader;

use or_project::{
    build, plan_with, verify, Container, Contribution, ManifestEntry, Module, Origin, Output,
    OutputKind, Plan, PlannedFile,
};
use or_title::oiv::PackageReader;

fn document(path: &str, text: &str) -> PlannedFile {
    PlannedFile {
        path: path.to_string(),
        origin: Origin::Document(text.to_string()),
    }
}

fn container() -> Container {
    let mut output = Output::of(
        OutputKind::OivPackage,
        "oiv",
        "uniforms",
        &["files", "wardrobe"],
    );
    output
        .extra
        .insert("author".into(), serde_json::json!("Ann"));
    Container {
        modules: vec![
            Module::files("files"),
            Module::clothing("wardrobe", "wardrobe"),
        ],
        outputs: vec![output],
    }
}

fn contributions() -> Vec<Contribution> {
    let mut files = Contribution::of("files");
    files.files = vec![document("readme.txt", "read me")];
    let mut pack = Contribution::of("wardrobe");
    pack.pack = true;
    pack.files = vec![
        document("x64/models/cdimages/u_female.rpf/stem/a.txt", "a"),
        document("x64/models/cdimages/u_female.rpf/stem.txt", "stem"),
        document("common/data/u_shop.meta", "<shop/>"),
    ];
    pack.manifest = vec![ManifestEntry {
        key: "data_file".into(),
        values: vec![
            "SHOP_PED_APPAREL_META_FILE".into(),
            "common/data/u_shop.meta".into(),
        ],
    }];
    vec![files, pack]
}

fn planned() -> Plan {
    plan_with(
        &container(),
        "oiv",
        std::path::Path::new("/projects/uniforms"),
        &contributions(),
    )
    .expect("the output is declared")
}

fn paths(plan: &Plan) -> Vec<&str> {
    plan.files.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn the_files_of_a_pack_are_installed_inside_the_dlc_rpf_with_the_documents_that_register_them() {
    let plan = planned();
    assert!(plan.is_buildable(), "{:?}", plan.problems);
    let root = "update/x64/dlcpacks/uniforms/dlc.rpf";
    let got = paths(&plan);
    for want in [
        "assembly.xml".to_string(),
        "readme.txt".to_string(),
        format!("{root}/content.xml"),
        format!("{root}/setup2.xml"),
        format!("{root}/common/data/u_shop.meta"),
        format!("{root}/x64/models/cdimages/u_female.rpf/stem/a.txt"),
    ] {
        assert!(got.contains(&want.as_str()), "{want} in {got:?}");
    }

    let content = plan
        .files
        .iter()
        .find(|f| f.path.ends_with("/content.xml"))
        .expect("the pack's content.xml");
    let Origin::Document(text) = &content.origin else {
        panic!("a document")
    };
    assert!(
        text.contains("SHOP_PED_APPAREL_META_FILE") && text.contains("u_shop.meta"),
        "{text}"
    );
    assert!(
        text.contains("RPF_FILE") && text.contains("u_female.rpf"),
        "{text}"
    );
    assert_eq!(plan.dlclist_entry.as_deref(), Some("dlcpacks:/uniforms/"));
    assert!(
        plan.notes.iter().any(|n| n.contains("dlclist.xml")),
        "{:?}",
        plan.notes
    );
}

#[test]
fn a_package_of_a_pack_is_written_and_its_manifest_adds_each_file_inside_the_archives_it_runs_through(
) {
    let plan = planned();
    let dir = tempfile::tempdir().unwrap();
    let built = build(&plan, dir.path()).expect("it builds");
    let found = verify(&plan, &built.folder);
    assert!(found.problems.is_empty(), "{:?}", found.problems);

    let file = std::fs::File::open(built.folder.join("uniforms.oiv")).unwrap();
    let package = PackageReader::new(BufReader::new(file)).unwrap();
    let adds = &package.package.adds;
    assert_eq!(
        adds.len(),
        plan.files.len() - 1,
        "every file but the manifest"
    );
    let nested = adds
        .iter()
        .find(|a| a.destination == "stem/a.txt")
        .expect("the file inside the pack's archive");
    assert_eq!(
        nested.archives,
        [
            "update/x64/dlcpacks/uniforms/dlc.rpf",
            "x64/models/cdimages/u_female.rpf"
        ]
    );
    let plain = adds.iter().find(|a| a.destination == "readme.txt").unwrap();
    assert!(plain.archives.is_empty());
}

#[test]
fn a_build_with_no_pack_in_it_installs_every_path_as_it_always_did() {
    let mut made = contributions();
    made[1].pack = false;
    made[1].manifest.clear();
    let plan = plan_with(
        &container(),
        "oiv",
        std::path::Path::new("/projects/uniforms"),
        &made,
    )
    .unwrap();
    assert!(
        paths(&plan).contains(&"common/data/u_shop.meta"),
        "{:?}",
        paths(&plan)
    );
    assert!(plan.dlclist_entry.is_none());
}
