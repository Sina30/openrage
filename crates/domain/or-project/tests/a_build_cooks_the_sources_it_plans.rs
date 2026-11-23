//! A `.src` folder of the project is planned as a file the build cooks, never as files it copies:
//! the cook comes before any write, a source of the game's own that is still the game's is left out,
//! and a cook that fails leaves the last good build as it was. The cooker here is a stand-in, since
//! this crate knows no kind of source.

mod common;

use std::path::{Path, PathBuf};

use common::{declare, snapshot};
use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::{
    build, build_all_with, files_contributions_with, plans_with, BuildError, Cook, Cooks, Origin,
    Plan, Project, SourceFolder,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const SOURCE: &str = "sources/bark_set.fake.src";

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn output(ship_unchanged: bool) -> Value {
    json!({
        "id": "fivem", "kind": "fivem-resource", "name": "pack", "take": ["files"],
        "ship_unchanged": ship_unchanged
    })
}

/// A project holding a script and the file inside a source folder, as a project that lists its files does.
fn project(dir: &Path, payload: &[u8], ship_unchanged: bool) -> Project {
    declare(
        &dir.join("pack"),
        &[
            ("client.lua", b"print('hi')\n".to_vec()),
            (&format!("{SOURCE}/payload.bin"), payload.to_vec()),
        ],
        Value::Null,
        output(ship_unchanged),
    )
}

/// The folder the plan cooks, said to be the game's own when `game` holds the original's SHA-256.
fn folder(project: &Project, name: &str, game: Vec<String>) -> SourceFolder {
    let folder = project.folder().join(format!("sources/{name}.fake.src"));
    let bytes = std::fs::metadata(folder.join("payload.bin"))
        .map(|m| m.len())
        .unwrap_or(0);
    SourceFolder {
        folder,
        cooks: Ok(Cooks {
            kind: "fake".into(),
            name: name.into(),
            extension: "bin".into(),
            bytes,
            game,
        }),
    }
}

fn plans(project: &Project, folders: &[SourceFolder]) -> Vec<Plan> {
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

/// Cooks a fake source: its `payload.bin`, unless that says `broken`, into `<scratch>/<folder name>/`.
fn cook(source: &Path, scratch: &Path) -> Result<PathBuf, Vec<String>> {
    let payload = std::fs::read(source.join("payload.bin")).map_err(|e| vec![e.to_string()])?;
    if payload == b"broken" {
        return Err(vec!["payload.bin is not a payload".to_string()]);
    }
    let name = source.file_name().unwrap().to_string_lossy().into_owned();
    let at = scratch
        .join(&name)
        .join(format!("{}.bin", name.split('.').next().unwrap()));
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, payload).unwrap();
    Ok(at)
}

fn run(plans: &[Plan], root: &Path) -> (Vec<or_project::Built>, Option<BuildError>) {
    build_all_with(plans, root, &ParamRemap::default(), Some(&cook as &Cook))
}

#[test]
fn a_source_is_planned_as_a_cook_and_nothing_inside_its_folder_ships() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path(), b"bark", false);
    let plans = plans(&project, &[folder(&project, "bark_set", vec![])]);
    let plan = &plans[0];
    assert!(plan.is_buildable(), "{:?}", plan.problems);

    let cooked = plan
        .files
        .iter()
        .find(|f| f.path == "stream/bark_set.bin")
        .expect("the source is a file at stream/<name>.<ext>");
    assert!(matches!(&cooked.origin, Origin::Cook { name, .. } if name == "bark_set"));
    assert!(
        plan.files.iter().all(|f| !f.path.contains(".src")),
        "a file of the source folder is a file of the plan: {:?}",
        plan.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );

    let (built, stopped) = run(&plans, &dir.path().join("out"));
    assert!(stopped.is_none(), "{stopped:?}");
    let written = snapshot(&built[0].folder);
    assert_eq!(written["stream/bark_set.bin"], b"bark");
    assert!(written.keys().all(|p| !p.contains(".src")), "{written:?}");
    assert_eq!(built[0].cooked.len(), 1);
}

#[test]
fn a_plan_with_a_source_is_refused_by_a_build_that_was_handed_no_cooker() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path(), b"bark", false);
    let plans = plans(&project, &[folder(&project, "bark_set", vec![])]);
    let root = dir.path().join("out");
    match build(&plans[0], &root) {
        Err(BuildError::Cooking(why)) => assert!(why.join(" ").contains("cooker"), "{why:?}"),
        other => panic!("expected a refusal to cook, got {other:?}"),
    }
    assert!(!root.join("pack").exists(), "a refused build wrote");
}

#[test]
fn two_builds_of_one_project_are_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path(), b"bark", false);
    let plans = plans(&project, &[folder(&project, "bark_set", vec![])]);
    let (first, stopped) = run(&plans, &dir.path().join("one"));
    assert!(stopped.is_none(), "{stopped:?}");
    let (second, stopped) = run(&plans, &dir.path().join("two"));
    assert!(stopped.is_none(), "{stopped:?}");
    assert_eq!(snapshot(&first[0].folder), snapshot(&second[0].folder));
}

#[test]
fn a_source_that_is_still_the_games_file_is_left_out_and_listed_until_the_build_ships_it() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path(), b"original", false);
    let game = vec![sha(b"original")];
    let folders = [folder(&project, "bark_set", game.clone())];
    let plans_left_out = plans(&project, &folders);
    let root = dir.path().join("out");
    let (built, stopped) = run(&plans_left_out, &root);
    assert!(stopped.is_none(), "{stopped:?}");
    assert_eq!(
        built[0]
            .unchanged
            .iter()
            .map(|u| u.path.as_str())
            .collect::<Vec<_>>(),
        ["stream/bark_set.bin"]
    );
    assert!(built[0].cooked.is_empty());
    assert!(!snapshot(&built[0].folder).contains_key("stream/bark_set.bin"));
    assert_eq!(built[0].written.files, 2, "client.lua and the manifest");
    let verified = or_project::verify(&plans_left_out[0], &built[0].folder);
    assert!(verified.problems.is_empty(), "{:?}", verified.problems);

    // The person edits the payload: it is no longer the game's file, so it ships.
    std::fs::write(project.folder().join(SOURCE).join("payload.bin"), b"edited").unwrap();
    let (built, stopped) = run(&plans_left_out, &root);
    assert!(stopped.is_none(), "{stopped:?}");
    assert!(built[0].unchanged.is_empty());
    assert_eq!(snapshot(&built[0].folder)["stream/bark_set.bin"], b"edited");

    // The switch ships the unchanged original for a pack that replaces the game's file.
    std::fs::write(
        project.folder().join(SOURCE).join("payload.bin"),
        b"original",
    )
    .unwrap();
    let shipping = self::project(&dir.path().join("again"), b"original", true);
    let plans_shipped = plans(&shipping, &[folder(&shipping, "bark_set", game)]);
    let (built, stopped) = run(&plans_shipped, &dir.path().join("ships"));
    assert!(stopped.is_none(), "{stopped:?}");
    assert!(built[0].unchanged.is_empty());
    assert_eq!(
        snapshot(&built[0].folder)["stream/bark_set.bin"],
        b"original"
    );
}

#[test]
fn a_cook_that_fails_stops_the_build_names_the_source_and_leaves_the_last_build() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path(), b"bark", false);
    let folders = [folder(&project, "bark_set", vec![])];
    let plans = plans(&project, &folders);
    let root = dir.path().join("out");
    let (built, stopped) = run(&plans, &root);
    assert!(stopped.is_none(), "{stopped:?}");
    let good = snapshot(&built[0].folder);

    std::fs::write(project.folder().join(SOURCE).join("payload.bin"), b"broken").unwrap();
    let (built, stopped) = run(&plans, &root);
    assert!(built.is_empty());
    match stopped {
        Some(BuildError::Cooking(why)) => {
            let said = why.join(" ");
            assert!(said.contains("bark_set.fake.src"), "{said}");
            assert!(said.contains("not a payload"), "{said}");
        }
        other => panic!("expected the cook to fail, got {other:?}"),
    }
    assert_eq!(snapshot(&root.join("pack")), good, "the last build changed");
    let hidden: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".cook") || n.ends_with(".new"))
        .collect();
    assert!(hidden.is_empty(), "a failed build left {hidden:?}");
}

#[test]
fn two_sources_that_cook_one_file_are_refused_naming_both() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path(), b"bark", false);
    let first = folder(&project, "bark_set", vec![]);
    let mut second = folder(&project, "bark_set_2", vec![]);
    second.cooks.as_mut().unwrap().name = "bark_set".into();
    let plans = plans(&project, &[first, second]);
    assert!(!plans[0].is_buildable());
    let said = plans[0].problems.join(" ");
    assert!(
        said.contains("bark_set.fake.src") && said.contains("bark_set_2.fake.src"),
        "{said}"
    );
    assert!(
        said.contains("give one a name of its own"),
        "the problem does not say what to do: {said}"
    );
}

#[test]
fn a_source_this_build_cannot_cook_is_a_problem_of_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path(), b"bark", false);
    let mut refused = folder(&project, "bark_set", vec![]);
    refused.cooks = Err("its platform is xbox360, and this build cooks win64 only".into());
    let plans = plans(&project, &[refused]);
    assert!(!plans[0].is_buildable());
    assert!(
        plans[0].problems.join(" ").contains("xbox360"),
        "{:?}",
        plans[0].problems
    );
}

#[test]
fn shipping_unchanged_originals_is_a_key_of_the_build_that_is_kept_and_checked() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pack.openrage-proj");
    let mut container = or_project::Container::default();
    let mut output = or_project::Output::fivem_resource("fivem", "pack", &["files"]);
    output.ship_unchanged = true;
    container.outputs.push(output);
    Project::create(&file, &container, &[]).unwrap();

    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"ship_unchanged\": true"), "{text}");
    let reopened = Project::load(&file).unwrap();
    assert_eq!(reopened.container, container);
    assert!(reopened.findings.is_empty(), "{:?}", reopened.findings);

    std::fs::write(
        &file,
        text.replace("\"ship_unchanged\": true", "\"ship_unchanged\": \"yes\""),
    )
    .unwrap();
    let reopened = Project::load(&file).unwrap();
    assert!(!reopened.container.outputs[0].ship_unchanged);
    assert!(
        reopened
            .findings
            .iter()
            .any(|f| f.contains("ship_unchanged")),
        "{:?}",
        reopened.findings
    );
}
