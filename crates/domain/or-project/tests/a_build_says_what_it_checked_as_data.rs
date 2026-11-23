//! What a build checked of what it wrote is data, not a sentence on a screen: every file reloaded
//! through a reader and how many, the metadata read back, whether any name is held twice, the
//! memory the files take against the ceiling the build stores, and on request whether a second
//! build writes the same bytes.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{declare, legacy_ytd, planned, write};
use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::{
    build, build_all_with, check_reproducible, files_contributions_with, plan_with, plans_with,
    verify, BuildError, Claim, Container, Contribution, Cook, Cooks, MetadataCount, Module,
    ModuleKind, Origin, Output, Plan, PlannedFile, Project, Reproducible, SourceFolder,
};
use serde_json::{json, Value};

fn fivem(limits: Value) -> Value {
    json!({
        "id": "fivem", "kind": "fivem-resource", "name": "pack", "take": ["files"],
        "limits": limits
    })
}

#[test]
fn every_file_a_reader_opened_is_listed_and_counted_past_twenty() {
    let dir = tempfile::tempdir().unwrap();
    let mut files: Vec<(String, Vec<u8>)> = (0..25)
        .map(|n| {
            (
                format!("stream/t{n:02}.ytd"),
                legacy_ytd(&format!("t{n:02}")),
            )
        })
        .collect();
    files.push(("client.lua".into(), b"print('hi')\n".to_vec()));
    let files: Vec<(&str, Vec<u8>)> = files.iter().map(|(p, b)| (p.as_str(), b.clone())).collect();
    let project = declare(
        &dir.path().join("pack"),
        &files,
        Value::Null,
        fivem(json!({})),
    );
    let plan = &planned(&project)[0];

    let built = build(plan, &dir.path().join("out")).expect("builds");
    let found = &built.verification;

    assert_eq!(
        found.reloaded.len(),
        25,
        "every .ytd reloaded, none dropped"
    );
    assert_eq!(
        found.checked,
        25 + 2,
        "the files, the script and the manifest"
    );
    assert_eq!(found.without_reader(), 2);
    assert_eq!(
        found.reload_line(),
        "27 file(s) compared with the plan, 25 reloaded through a reader, 2 with no reader"
    );
    assert_eq!(verify(plan, &built.folder).reloaded.len(), 25);
}

/// A metadata document of root `root` whose one array holds `entries` items.
fn metadata(root: &str, entries: usize) -> Vec<u8> {
    let items = "<Item><Flag>1</Flag></Item>".repeat(entries);
    format!("<?xml version=\"1.0\"?>\n<{root}><Entries>{items}</Entries></{root}>\n").into_bytes()
}

#[test]
fn metadata_documents_are_counted_by_what_they_are_and_how_many_entries_they_hold() {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        ("stream/a.ymt", metadata("PackInfo", 2)),
        ("stream/b.ymt", metadata("PackInfo", 3)),
        ("stream/c.ymt", metadata("OtherInfo", 1)),
        ("client.lua", b"x".to_vec()),
    ];
    let project = declare(
        &dir.path().join("pack"),
        &files,
        Value::Null,
        fivem(json!({})),
    );
    let built = build(&planned(&project)[0], &dir.path().join("out")).expect("builds");

    let counts = built.verification.metadata();

    assert_eq!(
        counts,
        vec![
            MetadataCount {
                reader: "OtherInfo (XML)".into(),
                documents: 1,
                entries: 1
            },
            MetadataCount {
                reader: "PackInfo (XML)".into(),
                documents: 2,
                entries: 5
            },
        ]
    );
}

fn unknown_content(id: &str) -> Module {
    Module {
        id: id.to_string(),
        kind: ModuleKind::Unknown("widgets".into()),
        extra: Default::default(),
    }
}

fn claiming(module: &str, kind: &str, name: &str) -> Contribution {
    let mut contribution = Contribution::of(module);
    contribution.claims.push(Claim {
        kind: kind.into(),
        name: name.into(),
    });
    contribution.files.push(PlannedFile {
        path: format!("stream/{module}_{name}.txt"),
        origin: Origin::Document(format!("{module} {name}")),
    });
    contribution
}

fn two_contents(dir: &Path, contributions: &[Contribution]) -> Plan {
    let container = Container {
        modules: vec![unknown_content("uniforms"), unknown_content("station")],
        outputs: vec![Output::fivem_resource(
            "fivem",
            "pack",
            &["uniforms", "station"],
        )],
    };
    plan_with(&container, "fivem", &dir.join("project"), contributions).expect("declared")
}

#[test]
fn a_build_says_which_names_it_checked_and_that_none_is_held_twice() {
    let dir = tempfile::tempdir().unwrap();
    let plan = two_contents(
        dir.path(),
        &[
            claiming("uniforms", "collection", "police"),
            claiming("station", "collection", "fire"),
            claiming("station", "archetype", "police"),
        ],
    );
    assert!(plan.is_buildable(), "{:?}", plan.problems);

    let built = build(&plan, &dir.path().join("out")).expect("builds");

    let names = &built.verification.names;
    assert!(names.clashes.is_empty());
    assert_eq!(
        names.line(),
        "3 name(s) the game resolves once, and no two contents hold one (1 archetype, 2 collection)"
    );
    assert_eq!(verify(&plan, &built.folder).names, plan.names);
}

#[test]
fn a_clash_is_counted_and_the_line_says_it_instead_of_the_good_news() {
    let dir = tempfile::tempdir().unwrap();
    let plan = two_contents(
        dir.path(),
        &[
            claiming("uniforms", "collection", "Police"),
            claiming("station", "collection", "police"),
        ],
    );

    assert!(!plan.is_buildable());
    assert_eq!(plan.names.clashes.len(), 1, "{:?}", plan.names);
    assert_eq!(
        plan.names.line(),
        "1 name(s) the game resolves once are held twice"
    );
    assert!(
        plan.problems.contains(&plan.names.clashes[0]),
        "a clash is a problem too"
    );
}

#[test]
fn a_build_with_nothing_to_name_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let files = [("client.lua", b"x".to_vec())];
    let project = declare(
        &dir.path().join("pack"),
        &files,
        Value::Null,
        fivem(json!({})),
    );

    let built = build(&planned(&project)[0], &dir.path().join("out")).expect("builds");

    assert_eq!(
        built.verification.names.line(),
        "No content holds a name the game resolves once"
    );
}

/// What the game allocates for the RSC7 files `bytes` are, from their headers.
fn streaming(files: &[&[u8]]) -> u64 {
    files
        .iter()
        .map(|bytes| u64::from(Rsc7::peek(bytes).expect("an RSC7 file").total_size()))
        .sum()
}

#[test]
fn memory_is_the_pages_the_headers_name_held_to_the_ceiling_the_build_stores() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (legacy_ytd("a"), legacy_ytd("b"));
    let expected = streaming(&[&a, &b]);
    assert!(expected > 0);
    let files = [
        ("stream/a.ytd", a),
        ("stream/b.ytd", b),
        ("client.lua", b"x".to_vec()),
    ];
    let project = declare(
        &dir.path().join("pack"),
        &files,
        Value::Null,
        fivem(json!({ "memory": expected + 1 })),
    );
    let plan = &planned(&project)[0];

    let built = build(plan, &dir.path().join("out")).expect("it is under its ceiling");

    let memory = &built.verification.memory;
    assert_eq!((memory.files, memory.bytes), (2, expected));
    assert_eq!(memory.ceiling, Some(expected + 1));
    assert_eq!(verify(plan, &built.folder).memory, *memory);
}

#[test]
fn a_build_over_its_memory_ceiling_is_not_swapped_in_and_the_last_one_stays() {
    let dir = tempfile::tempdir().unwrap();
    let a = legacy_ytd("a");
    let expected = streaming(&[&a]);
    let root = dir.path().join("out");
    let files = [("stream/a.ytd", a.clone())];

    let project = declare(
        &dir.path().join("pack"),
        &files,
        Value::Null,
        fivem(json!({ "memory": expected })),
    );
    build(&planned(&project)[0], &root).expect("at its ceiling is under it");
    let before = std::fs::read(root.join("pack/stream/a.ytd")).unwrap();

    let tight = declare(
        &dir.path().join("tight"),
        &files,
        Value::Null,
        fivem(json!({ "memory": expected - 1 })),
    );
    match build(&planned(&tight)[0], &root) {
        Err(BuildError::Unverified(found)) => {
            let said = found.problems.join(" ");
            assert!(
                said.contains("memory") && said.contains(&expected.to_string()),
                "{said}"
            );
            assert!(said.contains("build `fivem`"), "{said}");
        }
        other => panic!("expected the build to stop on its memory, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(root.join("pack/stream/a.ytd")).unwrap(),
        before
    );
}

fn source_project(dir: &Path) -> (Project, Vec<SourceFolder>) {
    let project = declare(
        &dir.join("pack"),
        &[
            ("client.lua", b"print('hi')\n".to_vec()),
            ("sources/bark_set.fake.src/payload.bin", b"bark".to_vec()),
        ],
        Value::Null,
        json!({ "id": "fivem", "kind": "fivem-resource", "name": "pack", "take": ["files"] }),
    );
    let folder = SourceFolder {
        folder: project.folder().join("sources/bark_set.fake.src"),
        cooks: Ok(Cooks {
            kind: "fake".into(),
            name: "bark_set".into(),
            extension: "bin".into(),
            bytes: 4,
            game: vec![],
        }),
    };
    (project, vec![folder])
}

fn plans_of(project: &Project, folders: &[SourceFolder]) -> Vec<Plan> {
    let handed = files_contributions_with(
        &project.container,
        "fivem",
        &project.folder(),
        &project.sources(),
        folders,
    )
    .unwrap();
    plans_with(&project.container, "fivem", &project.folder(), &handed).unwrap()
}

/// A cook that writes `payload` followed by its own run number when `stamp`, so no two runs agree.
fn cook_into(scratch: &Path, source: &Path, stamp: Option<usize>) -> Result<PathBuf, Vec<String>> {
    let mut bytes = std::fs::read(source.join("payload.bin")).map_err(|e| vec![e.to_string()])?;
    if let Some(run) = stamp {
        bytes.extend(format!(" run {run}").bytes());
    }
    let at = scratch.join("bark_set.bin");
    write(&at, &bytes);
    Ok(at)
}

#[test]
fn a_second_build_in_a_scratch_folder_says_whether_the_cook_writes_the_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let (project, folders) = source_project(dir.path());
    let plans = plans_of(&project, &folders);
    let root = dir.path().join("out");
    let remap = ParamRemap::default();
    let runs = AtomicUsize::new(0);

    let steady = |source: &Path, scratch: &Path| cook_into(scratch, source, None);
    let (mut built, stopped) = build_all_with(&plans, &root, &remap, Some(&steady as &Cook));
    assert!(stopped.is_none(), "{stopped:?}");
    assert_eq!(
        built[0].verification.reproducible,
        Reproducible::NotRun,
        "a second build is made on request"
    );
    assert_eq!(built[0].verification.reproducible.label(), "not run");

    check_reproducible(
        &plans[0],
        &mut built[0],
        &root,
        &remap,
        Some(&steady as &Cook),
    );
    assert_eq!(
        built[0].verification.reproducible,
        Reproducible::Yes { files: 3 },
        "the script, the cooked file and the manifest"
    );

    let restless = |source: &Path, scratch: &Path| {
        cook_into(scratch, source, Some(runs.fetch_add(1, Ordering::SeqCst)))
    };
    let (mut built, stopped) = build_all_with(&plans, &root, &remap, Some(&restless as &Cook));
    assert!(stopped.is_none(), "{stopped:?}");
    let live = std::fs::read(root.join("pack/stream/bark_set.bin")).unwrap();

    check_reproducible(
        &plans[0],
        &mut built[0],
        &root,
        &remap,
        Some(&restless as &Cook),
    );

    match &built[0].verification.reproducible {
        Reproducible::No { differences } => {
            assert_eq!(differences.len(), 1, "{differences:?}");
            assert!(
                differences[0].contains("stream/bark_set.bin"),
                "{differences:?}"
            );
        }
        other => panic!("a cook that stamps its run reproduced: {other:?}"),
    }
    assert!(built[0].verification.reproducible.label().starts_with("no"));
    assert_eq!(
        std::fs::read(root.join("pack/stream/bark_set.bin")).unwrap(),
        live,
        "the second build touched the live one"
    );
    let left: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".again") || name.ends_with(".cook"))
        .collect();
    assert!(left.is_empty(), "scratch folders were left: {left:?}");
}

#[test]
fn nineteen_files_that_settle_no_type_are_one_problem_with_a_count_and_three_names() {
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<(String, Vec<u8>)> = (0..19)
        .map(|n| (format!("data/f{n:02}.zzz"), vec![n as u8]))
        .collect();
    let files: Vec<(&str, Vec<u8>)> = files.iter().map(|(p, b)| (p.as_str(), b.clone())).collect();
    let project = declare(
        &dir.path().join("pack"),
        &files,
        Value::Null,
        json!({ "id": "dlc", "kind": "singleplayer-dlc", "name": "mypack", "take": ["files"] }),
    );

    let plan = &planned(&project)[0];

    let rows: Vec<&String> = plan
        .problems
        .iter()
        .filter(|p| p.contains("under File types"))
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", plan.problems);
    let row = rows[0];
    assert!(
        row.starts_with("Build a FiveM resource to ship these 19 files as they are ("),
        "{row}"
    );
    for named in ["data/f00.zzz", "data/f01.zzz", "data/f02.zzz"] {
        assert!(row.contains(named), "{row}");
    }
    assert!(!row.contains("data/f03.zzz"), "only three are named: {row}");
    assert!(row.contains("and 16 more"), "{row}");
    assert!(
        row.contains("File types"),
        "it still says how to state one: {row}"
    );
}

#[test]
fn one_file_that_settles_no_type_is_named_in_the_singular() {
    let dir = tempfile::tempdir().unwrap();
    let files = [("data/only.zzz", vec![1u8])];
    let project = declare(
        &dir.path().join("pack"),
        &files,
        Value::Null,
        json!({ "id": "dlc", "kind": "singleplayer-dlc", "name": "mypack", "take": ["files"] }),
    );

    let plan = &planned(&project)[0];

    let row = plan
        .problems
        .iter()
        .find(|p| p.contains("under File types"))
        .expect("a problem");
    assert!(
        row.starts_with("Build a FiveM resource to ship data/only.zzz as it is, or give it a type"),
        "{row}"
    );
}

#[test]
fn two_and_three_files_are_all_named_in_a_list_that_reads_as_one() {
    let dir = tempfile::tempdir().unwrap();
    for (count, expected) in [
        (2, "data/f00.zzz and data/f01.zzz"),
        (3, "data/f00.zzz, data/f01.zzz and data/f02.zzz"),
    ] {
        let files: Vec<(String, Vec<u8>)> = (0..count)
            .map(|n| (format!("data/f{n:02}.zzz"), vec![n as u8]))
            .collect();
        let files: Vec<(&str, Vec<u8>)> =
            files.iter().map(|(p, b)| (p.as_str(), b.clone())).collect();
        let project = declare(
            &dir.path().join(format!("pack{count}")),
            &files,
            Value::Null,
            json!({ "id": "dlc", "kind": "singleplayer-dlc", "name": "mypack", "take": ["files"] }),
        );

        let plan = &planned(&project)[0];

        let row = plan
            .problems
            .iter()
            .find(|p| p.contains("under File types"))
            .expect("a problem");
        assert!(
            row.starts_with(&format!(
                "Build a FiveM resource to ship these {count} files as they are ({expected}), or give each a type"
            )),
            "{row}"
        );
    }
}
