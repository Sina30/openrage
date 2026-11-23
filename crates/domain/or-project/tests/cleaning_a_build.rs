//! Clean removes what a build left and nothing else: not the project, not a folder OpenRage did not build.

use std::path::{Path, PathBuf};

use or_project::{
    build, built_resources, clean, plan, since_last_build, BuildError, Container, Output, Project,
};

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A project of one script, and the plan of its one build.
fn built_twice(dir: &Path) -> (Project, PathBuf) {
    let folder = dir.join("pack");
    write(&folder.join("client.lua"), b"print('hello')");
    let mut container = Container::default();
    container
        .outputs
        .push(Output::fivem_resource("fivem", "pack", &["files"]));
    let file = folder.join("pack.openrage-proj");
    Project::create(&file, &container, &[folder.join("client.lua")]).expect("a new project");
    let project = Project::load(&file).expect("it reads back");
    let planned = plan(
        &project.container,
        "fivem",
        &project.folder(),
        &project.sources(),
    )
    .expect("declared");
    let root = project.folder().join("build");
    build(&planned, &root).expect("the first build");
    build(&planned, &root).expect("the second build, which keeps the first as .previous");
    (project, root)
}

#[test]
fn clean_removes_the_build_its_previous_one_and_its_marker_and_nothing_beside_them() {
    let dir = tempfile::tempdir().unwrap();
    let (project, root) = built_twice(dir.path());
    write(&root.join("another-resource/fxmanifest.lua"), b"theirs");
    write(&root.join("notes.txt"), b"mine");
    assert!(root.join("pack").is_dir());
    assert!(root.join(".pack.previous").is_dir());
    assert!(root.join(".pack.openrage-build").is_file());

    let cleaned = clean(&root, "pack").expect("a build OpenRage made is cleaned");

    assert!(!root.join("pack").exists(), "the build is still there");
    assert!(
        !root.join(".pack.previous").exists(),
        "the previous build is still there"
    );
    assert!(
        !root.join(".pack.openrage-build").exists(),
        "the marker is still there"
    );
    // The resource, the build it replaced, the record of its items and the marker.
    assert_eq!(cleaned.removed.len(), 4, "{:?}", cleaned.removed);
    assert!(root.join("another-resource/fxmanifest.lua").is_file());
    assert!(root.join("notes.txt").is_file());
    assert!(
        project.path.is_file(),
        "a clean never touches the project file"
    );
    assert!(project.folder().join("client.lua").is_file());
}

#[test]
fn clean_refuses_a_folder_openrage_did_not_build() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("build");
    write(&root.join("pack/their-file.lua"), b"theirs");

    let refused = clean(&root, "pack").expect_err("no marker says OpenRage built it");

    assert!(matches!(refused, BuildError::Refused(_)), "{refused:?}");
    assert!(
        root.join("pack/their-file.lua").is_file(),
        "the folder was touched"
    );
}

#[test]
fn clean_of_a_build_that_never_ran_removes_nothing_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let cleaned = clean(&dir.path().join("build"), "pack").expect("nothing to clean is no error");
    assert!(cleaned.removed.is_empty(), "{:?}", cleaned.removed);
}

#[test]
fn clean_refuses_a_name_that_is_not_a_folder_name() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("keep/file.txt"), b"keep");
    for name in ["..", "a/b", "", "."] {
        let root = dir.path().join("build");
        let refused = clean(&root, name).expect_err(name);
        assert!(
            matches!(refused, BuildError::Refused(_)),
            "{name}: {refused:?}"
        );
    }
    assert!(dir.path().join("keep/file.txt").is_file());
}

#[test]
fn clean_leaves_nothing_in_the_build_folder_and_the_next_build_has_no_build_to_differ_from() {
    let dir = tempfile::tempdir().unwrap();
    let (project, root) = built_twice(dir.path());
    let planned = plan(
        &project.container,
        "fivem",
        &project.folder(),
        &project.sources(),
    )
    .expect("declared");
    assert!(
        since_last_build(&planned, &root).is_some(),
        "a build records its items for the next one to differ from"
    );

    clean(&root, "pack").expect("a build OpenRage made is cleaned");

    let left: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "a clean left {left:?} in the build folder");
    assert!(
        since_last_build(&planned, &root).is_none(),
        "the record of a build that is gone still says what it shipped"
    );
    let rebuilt = build(&planned, &root).expect("a build after a clean");
    assert!(
        rebuilt.since.is_none(),
        "the first build after a clean differs from {:?}, which is not there",
        rebuilt.since
    );
}

#[test]
fn the_resources_a_build_left_are_found_by_their_markers_and_nothing_else_is() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("build");
    assert_eq!(
        built_resources(&root).expect("no build folder is no resource"),
        Vec::<String>::new()
    );

    // The two parts a split output writes, under names no output declares.
    let mut container = Container::default();
    for (id, name) in [("a", "pack_2"), ("b", "pack_1")] {
        container
            .outputs
            .push(Output::fivem_resource(id, name, &["files"]));
    }
    let folder = dir.path().join("proj");
    write(&folder.join("client.lua"), b"print('hello')");
    let file = folder.join("proj.openrage-proj");
    Project::create(&file, &container, &[folder.join("client.lua")]).expect("a new project");
    let project = Project::load(&file).expect("it reads back");
    for id in ["a", "b"] {
        let planned = plan(
            &project.container,
            id,
            &project.folder(),
            &project.sources(),
        )
        .expect("declared");
        build(&planned, &root).expect("a build");
    }
    write(&root.join("theirs/fxmanifest.lua"), b"theirs");
    write(&root.join(".notes.txt"), b"mine");
    write(&root.join("..openrage-build"), b"no name");
    write(&root.join(".a b.previous/x"), b"a build's own leftover");

    assert_eq!(
        built_resources(&root).expect("the folder reads"),
        ["pack_1", "pack_2"],
        "a folder with no marker is not one a clean may take"
    );
}
