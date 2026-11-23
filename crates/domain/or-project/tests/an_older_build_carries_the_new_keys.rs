//! A build that predates the container opens a project that declares one and writes it back
//! with the container intact, and this build opens what a newer one wrote.

use std::path::Path;

use or_project::{Container, Module, Output, Project, SCHEMA};
use serde_json::{json, Map, Value};

/// What schema 3 did with a file, as `src/state/project.rs` at `fc2b18b9` has it: read the five
/// keys it knows, keep every other top-level key and every unread key of a document, and write
/// schema 3.
///
/// ⚠ A copy of those rules and not that build: nothing in this tree can run the code a release
/// older than this one shipped, so what is checked is that the container's keys sit where those
/// rules carry them.
fn schema_three_round_trip(bytes: &[u8]) -> Vec<u8> {
    const KNOWN_TOP: [&str; 5] = ["schema", "workspace", "active", "open", "documents"];
    let Value::Object(mut top) = serde_json::from_slice(bytes).expect("a JSON file") else {
        panic!("a project file is an object")
    };
    let newer = top["schema"].as_u64().expect("a schema") > 3;
    let carried: Map<String, Value> = top
        .iter()
        .filter(|(key, _)| !KNOWN_TOP.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let mut written = carried;
    written.insert("schema".into(), 3.into());
    for key in ["workspace", "active", "open", "documents"] {
        if let Some(value) = top.remove(key) {
            written.insert(key.into(), value);
        }
    }
    assert!(newer, "the file under test is a newer one");
    serde_json::to_vec_pretty(&Value::Object(written)).expect("it serialises")
}

#[test]
fn an_older_build_carries_the_new_keys_and_a_newer_file_opens_in_this_one() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let file = dir.path().join("pack.openrage-proj");
    let mut container = Container::default();
    container.modules.push(Module::files("scripts"));
    let mut output = Output::fivem_resource("fivem", "pack", &["files", "scripts"]);
    output.max_bytes = Some(1 << 20);
    output.extra.insert("split_at".into(), json!(100));
    container.outputs.push(output);
    let a = dir.path().join("a.lua");
    std::fs::write(&a, b"a").expect("a member");
    Project::create(&file, &container, std::slice::from_ref(&a)).expect("a new project");

    let after = schema_three_round_trip(&std::fs::read(&file).expect("it was written"));
    let reopened = Project::decode(&after, &file, dir.path()).expect("this build opens it again");

    assert_eq!(
        reopened.container, container,
        "the older build lost what it did not read"
    );
    assert_eq!(reopened.members.len(), 1);
    assert_eq!(reopened.members[0].path, a);
    assert_eq!(
        reopened.schema, 3,
        "it saved under its own number, and this build still reads it"
    );
    assert!(reopened.findings.is_empty(), "{:?}", reopened.findings);
}

#[test]
fn a_file_of_a_later_schema_is_opened_and_what_it_adds_is_kept_in_the_container() {
    let file = Path::new("later.openrage-proj");
    let later = json!({
        "schema": SCHEMA + 1,
        "modules": [{ "id": "files", "type": "files", "colour": "red" }],
        "outputs": [{ "id": "o", "kind": "fivem-resource", "name": "o", "take": ["files"],
                       "split_at": 100 }],
        "documents": [],
    });

    let opened =
        Project::decode(later.to_string().as_bytes(), file, Path::new(".")).expect("opens");

    assert_eq!(opened.container.modules[0].extra["colour"], json!("red"));
    assert_eq!(opened.container.outputs[0].extra["split_at"], json!(100));
}
