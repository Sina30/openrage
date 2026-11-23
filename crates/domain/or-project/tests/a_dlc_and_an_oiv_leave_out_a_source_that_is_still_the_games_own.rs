//! A singleplayer DLC and an `.oiv` leave out a source of the game's own content that the person did
//! not change, as a FiveM resource does, and ship it when the build says so. What they leave out is
//! also left out of the documents that name their files, so a pack never registers a file it does not
//! hold. The cooker here is a stand-in, since this crate knows no kind of source.

use std::path::{Path, PathBuf};

use or_io::container::rpf::RpfArchive;
use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::{
    build_all_with, plan_with, verify, BuildError, Built, Container, Contribution, Cook,
    ManifestEntry, Module, Origin, Output, OutputKind, Plan, PlannedFile,
};
use or_title::oiv::PackageReader;
use sha2::{Digest, Sha256};

const HANDLING: &str = "<?xml version=\"1.0\"?>\n<CHandlingDataMgr/>\n";
const ORIGINAL: &[u8] = b"the game's own bytes";
const COOKED: &str = "stream/bark_set.bin";

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn document(path: &str, text: &str) -> PlannedFile {
    PlannedFile {
        path: path.to_string(),
        origin: Origin::Document(text.to_string()),
    }
}

/// A source folder that cooks to `path`, said to be the game's own when `ORIGINAL` is its payload.
fn source(project: &Path, path: &str) -> PlannedFile {
    PlannedFile {
        path: path.to_string(),
        origin: Origin::Cook {
            source: project.join("sources/bark_set.fake.src"),
            kind: "fake".into(),
            name: "bark_set".into(),
            bytes: ORIGINAL.len() as u64,
            game: vec![sha(ORIGINAL)],
        },
    }
}

/// Cooks a fake source: its `payload.bin`, into `<scratch>/<folder name>/bark_set.bin`.
fn cook(source: &Path, scratch: &Path) -> Result<PathBuf, Vec<String>> {
    let payload = std::fs::read(source.join("payload.bin")).map_err(|e| vec![e.to_string()])?;
    let at = scratch.join("bark_set.fake.src").join("bark_set.bin");
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, payload).unwrap();
    Ok(at)
}

fn write_payload(project: &Path, payload: &[u8]) {
    let at = project.join("sources/bark_set.fake.src/payload.bin");
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, payload).unwrap();
}

fn container(kind: OutputKind, ship_unchanged: bool, takes: &[&str]) -> Container {
    let mut output = Output::of(kind, "out", "mypack", takes);
    output.ship_unchanged = ship_unchanged;
    Container {
        modules: takes
            .iter()
            .map(|id| match *id {
                "wardrobe" => Module::clothing("wardrobe", "wardrobe"),
                other => Module::files(other),
            })
            .collect(),
        outputs: vec![output],
    }
}

/// The plan of a project holding the handling file and, with `cooked`, the source that cooks `COOKED`.
fn plan_of(kind: OutputKind, ship_unchanged: bool, cooked: bool, project: &Path) -> Plan {
    let mut files = Contribution::of("files");
    files.files = vec![document("data/handling.meta", HANDLING)];
    files.manifest = vec![ManifestEntry {
        key: "data_file".into(),
        values: vec!["HANDLING_FILE".into(), "data/handling.meta".into()],
    }];
    if cooked {
        files.files.push(source(project, COOKED));
        files.manifest.push(ManifestEntry {
            key: "data_file".into(),
            values: vec!["HANDLING_FILE".into(), COOKED.into()],
        });
    }
    let plan = plan_with(
        &container(kind, ship_unchanged, &["files"]),
        "out",
        project,
        &[files],
    )
    .expect("the output is declared");
    assert!(plan.is_buildable(), "{:?}", plan.problems);
    plan
}

fn run(plan: &Plan, root: &Path) -> Built {
    let (built, stopped) = build_all_with(
        std::slice::from_ref(plan),
        root,
        &ParamRemap::default(),
        Some(&cook as &Cook),
    );
    assert!(stopped.is_none(), "{stopped:?}");
    built.into_iter().next().expect("one resource")
}

fn pack_files(built: &Built) -> Vec<String> {
    let archive = RpfArchive::open(built.folder.join("dlc.rpf")).expect("the pack opens");
    let mut files: Vec<String> = archive
        .nodes
        .iter()
        .filter(|node| !node.is_dir())
        .map(|node| {
            node.path()
                .replace('\\', "/")
                .trim_start_matches('/')
                .to_string()
        })
        .collect();
    files.sort();
    files
}

fn package_of(built: &Built) -> PackageReader<std::io::BufReader<std::fs::File>> {
    let file = std::fs::File::open(built.folder.join("mypack.oiv")).expect("the package is there");
    PackageReader::new(std::io::BufReader::new(file)).expect("the package reads")
}

fn bytes_of(built: &Built, name: &str) -> Vec<u8> {
    std::fs::read(built.folder.join(name)).expect("the build wrote it")
}

#[test]
fn a_dlc_leaves_out_a_source_that_is_still_the_games_file_and_does_not_register_it() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);
    let plan = plan_of(OutputKind::SingleplayerDlc, false, true, &project);

    let built = run(&plan, &dir.path().join("out"));

    assert_eq!(
        built
            .unchanged
            .iter()
            .map(|u| u.path.as_str())
            .collect::<Vec<_>>(),
        [COOKED]
    );
    assert!(built.cooked.is_empty());
    assert_eq!(
        pack_files(&built),
        ["content.xml", "data/handling.meta", "setup2.xml"]
    );
    let archive = RpfArchive::open(built.folder.join("dlc.rpf")).unwrap();
    let content = archive
        .nodes
        .iter()
        .find(|n| n.path().ends_with("content.xml"))
        .map(|n| archive.read_entry(n.entry_index()).unwrap())
        .map(|bytes| String::from_utf8(bytes).unwrap())
        .expect("content.xml");
    assert!(
        content.contains("handling.meta") && !content.contains("bark_set"),
        "the pack registers a file it does not hold: {content}"
    );
    assert_eq!(built.verification.problems, Vec::<String>::new());
    assert_eq!(
        verify(&plan, &built.folder),
        built.verification,
        "the plan as made reads the pack back the same"
    );
}

#[test]
fn a_dlc_that_leaves_a_source_out_is_the_pack_of_a_project_that_never_held_it() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);

    let left_out = run(
        &plan_of(OutputKind::SingleplayerDlc, false, true, &project),
        &dir.path().join("one"),
    );
    let never_held = run(
        &plan_of(OutputKind::SingleplayerDlc, false, false, &project),
        &dir.path().join("two"),
    );

    assert_eq!(
        bytes_of(&left_out, "dlc.rpf"),
        bytes_of(&never_held, "dlc.rpf"),
        "what is left out leaves no trace in the pack"
    );
}

#[test]
fn a_dlc_ships_the_unchanged_source_when_the_build_says_so_and_when_the_person_changed_it() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);

    let shipped = run(
        &plan_of(OutputKind::SingleplayerDlc, true, true, &project),
        &dir.path().join("ships"),
    );
    assert!(shipped.unchanged.is_empty());
    assert!(pack_files(&shipped).contains(&COOKED.to_string()));
    assert_eq!(shipped.verification.problems, Vec::<String>::new());

    write_payload(&project, b"edited by the person");
    let edited = run(
        &plan_of(OutputKind::SingleplayerDlc, false, true, &project),
        &dir.path().join("edited"),
    );
    assert!(edited.unchanged.is_empty());
    assert!(pack_files(&edited).contains(&COOKED.to_string()));
}

#[test]
fn a_dlc_with_nothing_but_unchanged_sources_is_refused_with_a_sentence_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);
    let mut only = Contribution::of("files");
    only.files = vec![source(&project, COOKED)];
    only.manifest = vec![ManifestEntry {
        key: "data_file".into(),
        values: vec!["HANDLING_FILE".into(), COOKED.into()],
    }];
    let plan = plan_with(
        &container(OutputKind::SingleplayerDlc, false, &["files"]),
        "out",
        &project,
        &[only],
    )
    .unwrap();
    let root = dir.path().join("out");

    let (built, stopped) = build_all_with(
        std::slice::from_ref(&plan),
        &root,
        &ParamRemap::default(),
        Some(&cook as &Cook),
    );

    assert!(built.is_empty());
    match stopped {
        Some(BuildError::Refused(why)) => {
            let said = why.join(" ");
            assert!(
                said.contains("still the game's") && said.contains("Ship a source"),
                "the refusal does not say what to do: {said}"
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!root.join("mypack").exists(), "a refused build wrote");
}

#[test]
fn an_oiv_leaves_out_a_source_that_is_still_the_games_file_and_does_not_add_it() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);
    let plan = plan_of(OutputKind::OivPackage, false, true, &project);

    let built = run(&plan, &dir.path().join("out"));

    assert_eq!(
        built
            .unchanged
            .iter()
            .map(|u| u.path.as_str())
            .collect::<Vec<_>>(),
        [COOKED]
    );
    let mut package = package_of(&built);
    let adds: Vec<&str> = package
        .package
        .adds
        .iter()
        .map(|a| a.source.as_str())
        .collect();
    assert_eq!(
        adds,
        ["data/handling.meta"],
        "the manifest adds a file the package lacks"
    );
    assert!(package.read(COOKED).is_err());
    assert_eq!(built.verification.problems, Vec::<String>::new());
    assert_eq!(verify(&plan, &built.folder), built.verification);
}

#[test]
fn an_oiv_that_leaves_a_source_out_is_the_package_of_a_project_that_never_held_it() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);

    let left_out = run(
        &plan_of(OutputKind::OivPackage, false, true, &project),
        &dir.path().join("one"),
    );
    let never_held = run(
        &plan_of(OutputKind::OivPackage, false, false, &project),
        &dir.path().join("two"),
    );

    assert_eq!(
        bytes_of(&left_out, "mypack.oiv"),
        bytes_of(&never_held, "mypack.oiv")
    );
}

#[test]
fn an_oiv_ships_the_unchanged_source_when_the_build_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);

    let built = run(
        &plan_of(OutputKind::OivPackage, true, true, &project),
        &dir.path().join("ships"),
    );

    assert!(built.unchanged.is_empty());
    let mut package = package_of(&built);
    assert_eq!(package.read(COOKED).unwrap(), ORIGINAL);
    assert!(package.package.adds.iter().any(|a| a.source == COOKED));
}

#[test]
fn an_oiv_pack_registers_only_the_files_it_holds_and_installs_the_documents_written_again() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);
    let shop = "common/data/u_shop.meta";
    let contributions = |cooked: bool| {
        let mut files = Contribution::of("files");
        files.files = vec![document("readme.txt", "read me")];
        let mut pack = Contribution::of("wardrobe");
        pack.pack = true;
        pack.files = vec![document("common/data/u_other.meta", "<other/>")];
        pack.manifest = vec![ManifestEntry {
            key: "data_file".into(),
            values: vec![
                "SHOP_PED_APPAREL_META_FILE".into(),
                "common/data/u_other.meta".into(),
            ],
        }];
        if cooked {
            pack.files.push(source(&project, shop));
            pack.manifest.push(ManifestEntry {
                key: "data_file".into(),
                values: vec!["SHOP_PED_APPAREL_META_FILE".into(), shop.into()],
            });
        }
        vec![files, pack]
    };
    let plan_for = |cooked: bool| {
        let plan = plan_with(
            &container(OutputKind::OivPackage, false, &["files", "wardrobe"]),
            "out",
            &project,
            &contributions(cooked),
        )
        .unwrap();
        assert!(plan.is_buildable(), "{:?}", plan.problems);
        plan
    };

    let left_out = run(&plan_for(true), &dir.path().join("one"));
    let never_held = run(&plan_for(false), &dir.path().join("two"));

    assert_eq!(left_out.unchanged.len(), 1);
    assert_eq!(
        bytes_of(&left_out, "mypack.oiv"),
        bytes_of(&never_held, "mypack.oiv"),
        "the pack's documents and the manifest name only what the package holds"
    );
    assert_eq!(left_out.verification.problems, Vec::<String>::new());
}

#[test]
fn a_rebuild_of_the_same_project_is_byte_identical_whatever_is_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);
    for (kind, file) in [
        (OutputKind::SingleplayerDlc, "dlc.rpf"),
        (OutputKind::OivPackage, "mypack.oiv"),
    ] {
        let plan = plan_of(kind, false, true, &project);
        let first = run(&plan, &dir.path().join(format!("first-{file}")));
        let second = run(&plan, &dir.path().join(format!("second-{file}")));
        assert_eq!(bytes_of(&first, file), bytes_of(&second, file), "{file}");
    }
}

#[test]
fn the_plan_says_in_a_sentence_which_sources_it_leaves_out_unless_the_build_ships_them() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    for kind in [OutputKind::SingleplayerDlc, OutputKind::OivPackage] {
        let left_out = plan_of(kind.clone(), false, true, &project);
        let said = left_out.notes.join(" ");
        assert!(
            said.contains("1 source") && said.contains("still the game's own file"),
            "{kind:?}: {said}"
        );
        assert!(
            said.contains("Ship a source that is still the game's own file"),
            "{kind:?} does not say how to ship it: {said}"
        );
        let shipped = plan_of(kind.clone(), true, true, &project);
        assert!(
            shipped.notes.iter().all(|n| !n.contains("left out")),
            "{:?}",
            shipped.notes
        );
        let none = plan_of(kind, false, false, &project);
        assert!(
            none.notes.iter().all(|n| !n.contains("left out")),
            "{:?}",
            none.notes
        );
    }
}

const SHOP: &str = "common/data/u_shop.meta";

/// A content that is a pack whose only file is the source `SHOP`.
fn pack_of_the_shop(project: &Path) -> Contribution {
    let mut pack = Contribution::of("wardrobe");
    pack.pack = true;
    pack.files = vec![source(project, SHOP)];
    pack.manifest = vec![ManifestEntry {
        key: "data_file".into(),
        values: vec!["SHOP_PED_APPAREL_META_FILE".into(), SHOP.into()],
    }];
    pack
}

/// A package that adds a readme and, with `pack`, installs a pack whose only file is the source `SHOP`.
fn package_of_a_readme(project: &Path, pack: bool) -> Plan {
    let mut files = Contribution::of("files");
    files.files = vec![document("readme.txt", "read me")];
    let (takes, contributions) = if pack {
        (
            vec!["files", "wardrobe"],
            vec![files, pack_of_the_shop(project)],
        )
    } else {
        (vec!["files"], vec![files])
    };
    let plan = plan_with(
        &container(OutputKind::OivPackage, false, &takes),
        "out",
        project,
        &contributions,
    )
    .unwrap();
    assert!(plan.is_buildable(), "{:?}", plan.problems);
    plan
}

#[test]
fn an_oiv_whose_pack_holds_only_unchanged_sources_ships_the_rest_and_adds_no_pack() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);
    let plan = package_of_a_readme(&project, true);

    let built = run(&plan, &dir.path().join("one"));
    let never_held = run(
        &package_of_a_readme(&project, false),
        &dir.path().join("two"),
    );

    assert_eq!(built.unchanged.len(), 1);
    assert_eq!(
        bytes_of(&built, "mypack.oiv"),
        bytes_of(&never_held, "mypack.oiv"),
        "the package holds the readme and no pack, as a project that never held the source builds"
    );
    let mut package = package_of(&built);
    let adds: Vec<&str> = (package.package.adds.iter())
        .map(|a| a.source.as_str())
        .collect();
    assert_eq!(adds, ["readme.txt"]);
    assert_eq!(package.read("readme.txt").unwrap(), b"read me");
    assert_eq!(built.verification.problems, Vec::<String>::new());
    assert_eq!(verify(&plan, &built.folder), built.verification);
}

#[test]
fn an_oiv_that_would_add_nothing_once_the_unchanged_sources_are_out_is_refused_with_a_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("pack");
    write_payload(&project, ORIGINAL);
    let plan = plan_with(
        &container(OutputKind::OivPackage, false, &["wardrobe"]),
        "out",
        &project,
        &[pack_of_the_shop(&project)],
    )
    .unwrap();
    let root = dir.path().join("out");

    let (built, stopped) = build_all_with(
        std::slice::from_ref(&plan),
        &root,
        &ParamRemap::default(),
        Some(&cook as &Cook),
    );

    assert!(built.is_empty());
    match stopped {
        Some(BuildError::Refused(why)) => {
            let said = why.join(" ");
            assert!(
                said.contains("still the game's") && said.contains("Ship a source"),
                "{said}"
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!root.join("mypack").exists(), "a refused build wrote");
}
