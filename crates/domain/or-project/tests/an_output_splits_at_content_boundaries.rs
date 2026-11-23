//! An output over a ceiling that is allowed to split divides into several resources, and the cut
//! falls between contents and never inside one.

use std::path::Path;

use or_project::{
    build, build_all, plans_with, BuildError, Claim, Container, Contribution, Module, ModuleKind,
    Origin, Output, Plan, PlannedFile,
};

const FOLDER: &str = "/projects/pack";
const SIZE: usize = 100;
const ALL: [&str; 4] = ["a", "b", "c", "d"];

fn container(modules: &[&str]) -> Container {
    Container {
        modules: modules
            .iter()
            .map(|id| Module {
                id: id.to_string(),
                kind: ModuleKind::Unknown("widgets".into()),
                extra: Default::default(),
            })
            .collect(),
        outputs: vec![Output::fivem_resource("fivem", "pack", modules)],
    }
}

/// A content of two files of `SIZE` bytes each, streamed so the manifest does not list them.
fn content(id: &str) -> Contribution {
    let mut contribution = Contribution::of(id);
    for n in 1..=2 {
        contribution.files.push(PlannedFile {
            path: format!("stream/{id}/{n}.dat"),
            origin: Origin::Document("x".repeat(SIZE)),
        });
    }
    contribution
}

fn contents(ids: &[&str]) -> Vec<Contribution> {
    ids.iter().map(|id| content(id)).collect()
}

fn planned(container: &Container, contributions: &[Contribution]) -> Vec<Plan> {
    plans_with(container, "fivem", Path::new(FOLDER), contributions)
        .expect("the output is declared")
}

/// The bytes of the manifest alone, which every resource of this test writes the same.
fn manifest_bytes() -> u64 {
    planned(&container(&[]), &[])[0].totals.bytes
}

/// A container of `modules` whose output may hold two contents of two files and not three, and may split.
fn splitting(modules: &[&str]) -> Container {
    let mut container = container(modules);
    // A cut by file, not by content, would put a fifth file into the first resource.
    container.outputs[0].max_bytes = Some(manifest_bytes() + 5 * SIZE as u64);
    container.outputs[0].split = true;
    container
}

#[test]
fn a_split_lands_on_content_boundaries() {
    let parts = planned(&splitting(&ALL), &contents(&ALL));

    assert_eq!(
        parts.len(),
        2,
        "{:?}",
        parts.iter().map(|p| &p.resource).collect::<Vec<_>>()
    );
    let names: Vec<&str> = parts.iter().map(|p| p.resource.as_str()).collect();
    assert_eq!(names, ["pack_1", "pack_2"]);
    for part in &parts {
        assert_eq!(part.output, "fivem");
        assert!(
            part.is_buildable(),
            "{}: {:?}",
            part.resource,
            part.problems
        );
        assert!(part.totals.bytes <= part.limits.bytes.unwrap());
    }
    // Every content is whole in exactly one resource, and the order of `take` holds.
    for id in ["a", "b", "c", "d"] {
        let holders: Vec<usize> = (0..parts.len())
            .filter(|&n| {
                parts[n]
                    .files
                    .iter()
                    .any(|f| f.path.starts_with(&format!("stream/{id}/")))
            })
            .collect();
        assert_eq!(holders.len(), 1, "content {id} is in resources {holders:?}");
        let held = parts[holders[0]]
            .files
            .iter()
            .filter(|f| f.path.starts_with(&format!("stream/{id}/")))
            .count();
        assert_eq!(held, 2, "content {id} was cut");
    }
    let in_first: Vec<&str> = parts[0].files.iter().map(|f| f.path.as_str()).collect();
    assert!(in_first.iter().any(|p| p.starts_with("stream/a/")));
    assert!(in_first.iter().any(|p| p.starts_with("stream/b/")));
    assert!(parts[1]
        .files
        .iter()
        .any(|f| f.path.starts_with("stream/c/")));
    assert!(parts[1]
        .files
        .iter()
        .any(|f| f.path.starts_with("stream/d/")));
}

#[test]
fn an_output_that_does_not_split_is_refused_over_its_ceiling() {
    let mut combined = splitting(&ALL);
    combined.outputs[0].split = false;

    let parts = planned(&combined, &contents(&["a", "b", "c", "d"]));

    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].resource, "pack");
    assert!(!parts[0].is_buildable());
}

#[test]
fn an_output_inside_its_ceilings_is_one_resource_with_its_own_name() {
    let parts = planned(&splitting(&["a", "b"]), &contents(&["a", "b"]));

    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].resource, "pack");
    assert!(parts[0].is_buildable(), "{:?}", parts[0].problems);
}

#[test]
fn a_content_over_the_ceiling_by_itself_is_reported_and_never_cut() {
    let mut oversize = content("a");
    oversize.files.push(PlannedFile {
        path: "stream/a/3.dat".into(),
        origin: Origin::Document("x".repeat(6 * SIZE)),
    });

    let parts = planned(
        &splitting(&ALL),
        &[oversize, content("b"), content("c"), content("d")],
    );

    let holder = parts
        .iter()
        .find(|p| p.files.iter().any(|f| f.path.starts_with("stream/a/")))
        .expect("content a is somewhere");
    assert_eq!(
        holder
            .files
            .iter()
            .filter(|f| f.path.starts_with("stream/a/"))
            .count(),
        3
    );
    assert!(!holder.is_buildable());
    assert!(
        holder
            .problems
            .iter()
            .any(|p| p.contains("`a`") && p.contains("inside")),
        "{:?}",
        holder.problems
    );
    // The others are not held up by it.
    assert!(parts.iter().any(Plan::is_buildable));
}

#[test]
fn every_resource_of_a_split_answers_for_the_names_of_the_whole_project() {
    let claim = |mut contribution: Contribution| {
        contribution.claims.push(Claim {
            kind: "collection".into(),
            name: "police".into(),
        });
        contribution
    };
    // `a` and `c` land in different resources and claim one name.
    let contributions = [
        claim(content("a")),
        content("b"),
        claim(content("c")),
        content("d"),
    ];
    let mut tight = splitting(&ALL);
    tight.outputs[0].max_bytes = Some(manifest_bytes() + 3 * SIZE as u64);

    let parts = planned(&tight, &contributions);

    assert!(parts.len() >= 2);
    for part in &parts {
        let holds = |id: &str| {
            part.files
                .iter()
                .any(|f| f.path.starts_with(&format!("stream/{id}/")))
        };
        if holds("a") || holds("c") {
            assert!(!part.is_buildable(), "{} ignored the clash", part.resource);
        }
    }
}

#[test]
fn the_resources_of_a_split_build_and_read_back_each_on_its_own() {
    // Plain files that no reader opens, so what is checked is the split and not a format.
    let contents: Vec<Contribution> = ["a", "b", "c", "d"]
        .iter()
        .map(|id| {
            let mut contribution = Contribution::of(id);
            for n in 1..=2 {
                contribution.files.push(PlannedFile {
                    path: format!("data/{id}{n}.txt"),
                    origin: Origin::Document(format!("{id} {n}")),
                });
            }
            contribution
        })
        .collect();
    let mut container = container(&["a", "b", "c", "d"]);
    container.outputs[0].max_files = Some(6);
    container.outputs[0].split = true;
    let root = tempfile::tempdir().unwrap();

    let parts = planned(&container, &contents);
    assert!(parts.len() > 1, "the files fit one resource");
    for part in &parts {
        let built = build(part, root.path()).expect("each part builds");
        assert!(built.verification.problems.is_empty());
        assert!(root
            .path()
            .join(&part.resource)
            .join("fxmanifest.lua")
            .is_file());
    }

    let mut written: Vec<String> = parts
        .iter()
        .flat_map(|part| {
            part.files
                .iter()
                .filter(|f| f.path.starts_with("data/"))
                .map(|f| f.path.clone())
        })
        .collect();
    written.sort();
    assert_eq!(
        written,
        [
            "data/a1.txt",
            "data/a2.txt",
            "data/b1.txt",
            "data/b2.txt",
            "data/c1.txt",
            "data/c2.txt",
            "data/d1.txt",
            "data/d2.txt"
        ],
        "a file is in no resource or in two"
    );
}

/// A content of `files` files of `SIZE` bytes each, streamed so the manifest does not list them.
fn content_of(id: &str, files: usize) -> Contribution {
    let mut contribution = Contribution::of(id);
    for n in 1..=files {
        contribution.files.push(PlannedFile {
            path: format!("stream/{id}/{n}.dat"),
            origin: Origin::Document("x".repeat(SIZE)),
        });
    }
    contribution
}

/// The ids of the contents of `plan`, in the order of its files.
fn holds(plan: &Plan) -> Vec<&str> {
    let mut ids: Vec<&str> = plan
        .files
        .iter()
        .filter_map(|f| f.path.strip_prefix("stream/")?.split('/').next())
        .collect();
    ids.dedup();
    ids
}

#[test]
fn a_split_starts_a_new_resource_when_the_next_content_does_not_fit_and_never_goes_back() {
    // `c` would fit beside `a`, but a resource is closed once a content has been refused by it.
    let contributions = [
        content_of("a", 4),
        content_of("b", 4),
        content_of("c", 2),
        content_of("d", 2),
    ];
    let mut container = container(&ALL);
    container.outputs[0].max_bytes = Some(manifest_bytes() + 6 * SIZE as u64);
    container.outputs[0].split = true;

    let parts = planned(&container, &contributions);

    let held: Vec<Vec<&str>> = parts.iter().map(holds).collect();
    assert_eq!(held, [vec!["a"], vec!["b", "c"], vec!["d"]]);
    let names: Vec<&str> = parts.iter().map(|p| p.resource.as_str()).collect();
    assert_eq!(names, ["pack_1", "pack_2", "pack_3"]);
}

/// A split of four contents into at least two resources, each of which builds, and the parts built into `root`.
fn four_contents_in_several_resources() -> (Vec<Plan>, tempfile::TempDir) {
    let contents: Vec<Contribution> = ALL
        .iter()
        .map(|id| {
            let mut contribution = Contribution::of(id);
            for n in 1..=2 {
                contribution.files.push(PlannedFile {
                    path: format!("data/{id}{n}.txt"),
                    origin: Origin::Document(format!("{id} {n}")),
                });
            }
            contribution
        })
        .collect();
    let mut container = container(&ALL);
    container.outputs[0].max_files = Some(6);
    container.outputs[0].split = true;
    let parts = planned(&container, &contents);
    assert!(parts.len() > 1, "the files fit one resource");
    assert!(parts.iter().all(Plan::is_buildable));
    (parts, tempfile::tempdir().unwrap())
}

#[test]
fn a_folder_one_resource_would_replace_and_did_not_build_stops_the_whole_split() {
    let (parts, root) = four_contents_in_several_resources();
    let (first, last) = (&parts[0].resource, &parts[parts.len() - 1].resource);
    let theirs = root.path().join(last);
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::write(theirs.join("keep.txt"), "not built by OpenRage").unwrap();

    let (built, stopped) = build_all(&parts, root.path());

    assert!(built.is_empty(), "built {} before it stopped", built.len());
    let Some(BuildError::Refused(why)) = stopped else {
        panic!("not refused: {stopped:?}")
    };
    assert!(why.iter().any(|p| p.contains(last.as_str())), "{why:?}");
    assert!(
        !root.path().join(first).exists(),
        "`{first}` was swapped in beside a refusal"
    );
    assert_eq!(
        std::fs::read_to_string(theirs.join("keep.txt")).unwrap(),
        "not built by OpenRage"
    );
}

#[test]
fn what_goes_wrong_while_a_resource_is_written_leaves_the_ones_before_it_built() {
    let (mut parts, root) = four_contents_in_several_resources();
    // A copy whose source has changed since the plan was made is found only when it is copied.
    let source = root.path().join("source.txt");
    std::fs::write(&source, "longer than the plan saw").unwrap();
    let last = parts.len() - 1;
    parts[last].files.push(PlannedFile {
        path: "data/copied.txt".into(),
        origin: Origin::Copy {
            from: source,
            bytes: 1,
        },
    });
    let theirs = root.path().join("out");

    let (built, stopped) = build_all(&parts, &theirs);

    assert_eq!(built.len(), last, "the resources before the failing one");
    assert!(matches!(stopped, Some(BuildError::SourceChanged(_))));
    for part in &parts[..last] {
        assert!(theirs.join(&part.resource).join("fxmanifest.lua").is_file());
    }
    assert!(!theirs.join(&parts[last].resource).exists());
}
