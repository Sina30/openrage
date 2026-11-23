//! A module of addon peds goes through the one plan and the one build: its files land where the layout
//! puts them, the data file is registered, and what was written reloads through the readers.

mod common;

use common::{container, ped_module, plan, staged_files};
use or_addonped::model::AddonPeds;
use or_addonped::verify::verify;
use or_project::{build, Origin};

#[test]
fn a_ped_made_of_staged_files_builds_to_the_places_its_layout_names_and_reads_back() {
    let Some(files) = staged_files() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let module = ped_module(dir.path(), "my_ped", &files);
    let container = container(&["peds"]);

    let planned = plan(&container, dir.path());
    assert_eq!(planned.problems, Vec::<String>::new());
    let paths: Vec<&str> = planned.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "fxmanifest.lua",
            "peds.meta",
            "stream/my_ped.yed",
            "stream/my_ped.yft",
            "stream/my_ped.ytd",
            "stream/my_ped/head_000_r.ydd",
        ],
        "the plan lists the manifest, the data file and each streamed file, by path"
    );
    let manifest = planned
        .files
        .iter()
        .find(|f| f.path == "fxmanifest.lua")
        .and_then(|f| match &f.origin {
            Origin::Document(text) => Some(text.clone()),
            _ => None,
        })
        .expect("the manifest is a document");
    assert!(
        manifest.contains("data_file 'PED_METADATA_FILE' 'peds.meta'"),
        "{manifest}"
    );
    assert!(
        manifest.contains("'peds.meta',"),
        "the data file is listed: {manifest}"
    );
    assert!(
        !manifest.contains("stream/"),
        "FiveM finds the stream folder by itself: {manifest}"
    );
    assert_eq!(planned.items.len(), 1);
    assert_eq!(planned.items[0].name, "my_ped");

    let root = dir.path().join("build");
    let built = build(&planned, &root).expect("the build writes and reads back");
    assert_eq!(built.verification.problems, Vec::<String>::new());
    let readers: Vec<&str> = built
        .verification
        .reloaded
        .iter()
        .map(|r| r.path.as_str())
        .collect();
    for streamed in [
        "stream/my_ped.yft",
        "stream/my_ped.ytd",
        "stream/my_ped/head_000_r.ydd",
        "stream/my_ped.yed",
    ] {
        assert!(
            readers.contains(&streamed),
            "`{streamed}` was reopened through a reader: {readers:?}"
        );
    }

    let model = AddonPeds::read(&module).unwrap();
    let found = verify(&model, &built.folder);
    assert!(found.is_clean(), "{:?}", found.problems());
    assert!(
        found.checks.iter().any(
            |c| c.name == "data file" && c.detail.contains("registers it as PED_METADATA_FILE")
        ),
        "{:?}",
        found.checks
    );
}

#[test]
fn two_builds_of_one_module_are_byte_identical_and_an_edited_file_is_reported_by_the_peds_id() {
    let Some(files) = staged_files() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let module = ped_module(dir.path(), "my_ped", &files);
    let container = container(&["peds"]);
    let root = dir.path().join("build");

    let first = build(&plan(&container, dir.path()), &root).expect("the first build");
    let bytes_of = |folder: &std::path::Path| -> Vec<(String, Vec<u8>)> {
        let mut found = Vec::new();
        fn walk(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
            entries.sort_by_key(|e| e.path());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    walk(base, &path, out);
                } else {
                    let name = path
                        .strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((name, std::fs::read(&path).unwrap()));
                }
            }
        }
        walk(folder, folder, &mut found);
        found
    };
    let before = bytes_of(&first.folder);
    let second = build(&plan(&container, dir.path()), &root).expect("the second build");
    assert_eq!(
        before,
        bytes_of(&second.folder),
        "the same module builds the same bytes"
    );
    let since = second.since.expect("the first build recorded its items");
    assert!(
        since.added.is_empty()
            && since.changed.is_empty()
            && since.removed.is_empty()
            && since.moved.is_empty()
    );

    // The data file changes: the ped's content, as the diff reads it, does not.
    std::fs::write(
        module.join("peds.meta"),
        common::peds_meta(&["my_ped", "another_ped"]),
    )
    .unwrap();
    // A file of the ped changes: its item is changed, by the id the model gave it.
    let mut model = AddonPeds::read(&module).unwrap();
    let id = model.peds[0].id.clone();
    std::fs::write(module.join("my_ped.yed"), b"changed").unwrap();
    model.peds[0].files.retain(|f| f.path != "my_ped.yed");
    model.write(&module).unwrap();

    let third = plan(&container, dir.path());
    let since =
        or_project::since_last_build(&third, &root).expect("an earlier build recorded its items");
    assert_eq!(
        since
            .changed
            .iter()
            .map(|i| i.id.as_str())
            .collect::<Vec<_>>(),
        [id.as_str()],
        "{since:?}"
    );
}
