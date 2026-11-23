//! A FiveM build states the author, the version and the description its entry holds in the
//! manifest, in a fixed order after the two lines every resource has, and says why it cannot when
//! the resource holds a manifest of its own.

mod common;

use common::{declare, planned};
use or_project::Origin;
use serde_json::json;

fn manifest(plan: &or_project::Plan) -> &str {
    let file = plan
        .files
        .iter()
        .find(|f| f.path == "fxmanifest.lua")
        .expect("a plan holds a manifest");
    match &file.origin {
        Origin::Document(text) => text,
        other => panic!("the manifest is a document, not {other:?}"),
    }
}

fn fivem(extra: serde_json::Value) -> serde_json::Value {
    let mut output =
        json!({ "id": "fivem", "kind": "fivem-resource", "name": "pack", "take": ["files"] });
    for (key, value) in extra.as_object().expect("an object") {
        output[key] = value.clone();
    }
    output
}

#[test]
fn the_manifest_states_the_author_the_description_and_the_version_the_build_holds() {
    let dir = tempfile::tempdir().unwrap();
    let files = vec![("client.lua", b"print('x')".to_vec())];
    let project = declare(
        &dir.path().join("p"),
        &files,
        serde_json::Value::Null,
        fivem(json!({ "author": "Ann O'Neil", "description": "Uniforms", "version": "1.2.0" })),
    );
    let plans = planned(&project);
    assert!(plans[0].is_buildable(), "{:?}", plans[0].problems);
    let text = manifest(&plans[0]);
    assert!(
        text.starts_with(
            "fx_version 'cerulean'\ngame 'gta5'\nauthor 'Ann O\\'Neil'\ndescription 'Uniforms'\nversion '1.2.0'\n"
        ),
        "{text}"
    );
}

#[test]
fn a_build_that_states_none_writes_the_manifest_it_always_did() {
    let dir = tempfile::tempdir().unwrap();
    let files = vec![("client.lua", b"print('x')".to_vec())];
    let project = declare(
        &dir.path().join("p"),
        &files,
        serde_json::Value::Null,
        fivem(json!({})),
    );
    let text = manifest(&planned(&project)[0]).to_string();
    assert!(
        !text.contains("\nauthor ") && !text.contains("\nversion "),
        "{text}"
    );
}

#[test]
fn a_word_that_is_not_text_and_a_manifest_the_resource_holds_are_problems_that_say_so() {
    let dir = tempfile::tempdir().unwrap();
    let files = vec![("client.lua", b"print('x')".to_vec())];
    let project = declare(
        &dir.path().join("p"),
        &files,
        serde_json::Value::Null,
        fivem(json!({ "author": 7 })),
    );
    let plans = planned(&project);
    assert!(
        plans[0]
            .problems
            .iter()
            .any(|p| p.contains("author") && p.contains("not text")),
        "{:?}",
        plans[0].problems
    );

    let own = vec![("fxmanifest.lua", b"fx_version 'cerulean'".to_vec())];
    let project = declare(
        &dir.path().join("q"),
        &own,
        serde_json::Value::Null,
        fivem(json!({ "version": "2.0" })),
    );
    let plans = planned(&project);
    assert!(
        plans[0]
            .problems
            .iter()
            .any(|p| p.contains("version") && p.contains("fxmanifest.lua")),
        "{:?}",
        plans[0].problems
    );
}
