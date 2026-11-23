//! A clothing module is a kind of content this build knows and keeps its folder, a plan can hold a
//! file whose bytes are not text, and a build reloads a written `.ymt` as the metadata it is.

use or_io::decl::File;
use or_io::meta::CPedVariationInfo;
use or_project::{
    build, plan_with, BuildError, Container, Contribution, ModuleKind, Origin, Output, Plan,
    PlannedFile,
};
use serde_json::{json, Map, Value};

fn read(top: Value) -> Container {
    let Value::Object(top) = top else {
        panic!("a project file is an object")
    };
    let mut findings = Vec::new();
    let container = Container::read(&top, &mut findings);
    assert!(findings.is_empty(), "{findings:?}");
    container
}

#[test]
fn a_clothing_module_is_known_and_keeps_its_folder_through_a_save() {
    let declared = json!({
        "modules": [{ "id": "uniforms", "type": "clothing", "path": "modules/uniforms", "link": true }],
        "outputs": [],
    });
    let container = read(declared.clone());
    let module = &container.modules[0];
    assert_eq!(module.kind, ModuleKind::Clothing);
    assert_eq!(module.path(), Some("modules/uniforms"));

    let mut written = Map::new();
    container.write(&mut written);
    assert_eq!(
        written["modules"][0], declared["modules"][0],
        "the link flag rides along"
    );

    let made = or_project::Module::clothing("uniforms", "modules/uniforms");
    assert_eq!(made.kind, ModuleKind::Clothing);
    assert_eq!(made.path(), Some("modules/uniforms"));
}

fn planned(files: Vec<PlannedFile>, folder: &std::path::Path) -> Plan {
    let mut container = Container::default();
    container
        .modules
        .push(or_project::Module::clothing("wardrobe", "wardrobe"));
    container
        .outputs
        .push(Output::fivem_resource("fivem", "pack", &["wardrobe"]));
    let mut contribution = Contribution::of("wardrobe");
    contribution.files = files;
    let plan = plan_with(&container, "fivem", folder, &[contribution]).expect("declared");
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    plan
}

fn generated(path: &str, bytes: &[u8]) -> PlannedFile {
    PlannedFile {
        path: path.to_string(),
        origin: Origin::Generated(bytes.to_vec()),
    }
}

#[test]
fn a_file_whose_bytes_are_not_text_is_written_exactly_and_counted_in_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let bytes: Vec<u8> = (0..=255u8).chain([0xFF, 0xFE, 0x00, 0x80]).collect();
    let plan = planned(
        vec![generated("stream/blob.bin", &bytes)],
        &dir.path().join("project"),
    );
    assert!(plan.totals.bytes >= bytes.len() as u64);

    let built = build(&plan, &dir.path().join("out")).expect("it builds");
    let written = std::fs::read(built.folder.join("stream/blob.bin")).unwrap();
    assert_eq!(written, bytes, "not one byte went through a text encoding");
}

#[test]
fn a_ymt_that_reads_back_is_named_by_its_root_and_a_broken_one_stops_the_build() {
    let dir = tempfile::tempdir().unwrap();
    let ymt = CPedVariationInfo::default()
        .to_bytes()
        .expect("a variation built from nothing saves");
    let plan = planned(
        vec![generated("stream/a.ymt", &ymt)],
        &dir.path().join("project"),
    );
    let built = build(&plan, &dir.path().join("out")).expect("a metadata file builds");
    let reloaded = built
        .verification
        .reloaded
        .iter()
        .find(|r| r.path == "stream/a.ymt")
        .expect("the metadata was reloaded through a reader");
    assert!(
        reloaded.reader.contains("CPedVariationInfo"),
        "{}",
        reloaded.reader
    );

    let broken = planned(
        vec![generated("stream/b.ymt", b"this is no metadata")],
        &dir.path().join("project"),
    );
    match build(&broken, &dir.path().join("out2")) {
        Err(BuildError::Unverified(found)) => {
            assert!(
                found
                    .problems
                    .iter()
                    .any(|p| p.contains("b.ymt") && p.contains("does not reload")),
                "{:?}",
                found.problems
            );
        }
        other => panic!("a .ymt that does not read back must stop the build: {other:?}"),
    }
}
