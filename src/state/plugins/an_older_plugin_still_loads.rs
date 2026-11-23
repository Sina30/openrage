//! A plugin built for an older interface than the editor's newest is installed, listed and run as it
//! was built, and the Plugins panel says it is older; one built for a newer interface is listed with
//! the sentence that says to update OpenRage, and does not run.
//!
//! ⚠ The sample guest is built by a nested `cargo build` for wasm32-unknown-unknown, as the other
//! plugin suites build theirs.

use std::path::{Path, PathBuf};

use or_plugin::ABI_VERSION;

use super::{Consent, InstalledPlugin};
use crate::state::a_plugin_asks_before_it_runs::built_sample;
use crate::state::{AppState, Document, DocumentKind};

fn guest() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| built_sample("", "openrage-plugin-sample"))
        .clone()
}

/// The sample as a user installs it: a folder holding `plugin.json` of interface `abi` and the module built from it.
fn installed(plugins: &Path, id: &str, abi: u32) -> InstalledPlugin {
    let dir = plugins.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(guest(), dir.join("plugin.wasm")).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"id":"{id}","name":"Plugin {id}","abi":{abi},"wants":["cap.read_open_document","cap.edit_open_document"]}}"#
        ),
    )
    .unwrap();
    InstalledPlugin::read(&dir)
}

fn state_with(plugin: &InstalledPlugin) -> AppState {
    let mut state = AppState::default();
    state.plugins.scanned = Some(vec![plugin.clone()]);
    state.prefs.plugin_consent.insert(
        plugin.manifest.id.clone(),
        Consent::Granted(
            plugin
                .manifest
                .wants
                .iter()
                .map(|c| c.key().to_string())
                .collect(),
        ),
    );
    let kind = DocumentKind::Ybn;
    let handler = crate::state::payload_handlers::handler_for(
        kind.resource_type().expect("a paged format"),
        or_io::Generation::Gen8,
    )
    .expect("a payload handler");
    let payload = (handler.empty)();
    let bytes = handler.save(&payload).expect("a blank serializes");
    let mut doc = Document::new(1, Some(PathBuf::from("older.ybn")), payload);
    doc.set_resource(crate::state::document_io::resource_image_for(kind, &bytes));
    state.documents.push(doc);
    state.active_document = Some(1);
    state
}

fn keys(state: &AppState, id: &str) -> Vec<String> {
    (state.plugin_report(id).unwrap_or_default().iter())
        .map(|d| d.key.to_string())
        .collect()
}

#[test]
fn a_plugin_of_interface_0_is_listed_runs_as_built_and_the_manager_says_it_is_older() {
    let dir = tempfile::tempdir().unwrap();
    let old = installed(dir.path(), "old", 0);
    let new = installed(dir.path(), "new", ABI_VERSION);

    assert_eq!(old.refused, None, "it loads");
    let notes = old.notes();
    assert!(
        notes.len() == 1 && notes[0].contains("interface 0") && notes[0].contains("still loads"),
        "{notes:?}"
    );
    assert!(
        new.notes().is_empty(),
        "a plugin of the newest interface says nothing"
    );

    // It runs exactly as the same module declared at the newest interface does.
    let mut state = state_with(&old);
    state.request_plugin_run("old");
    let mut again = state_with(&new);
    again.request_plugin_run("new");
    assert_eq!(keys(&state, "old"), keys(&again, "new"));
    assert!(
        !keys(&state, "old")
            .iter()
            .any(|k| k == "plugin.rejected" || k == "plugin.failed"),
        "{:?}",
        keys(&state, "old")
    );
}

#[test]
fn a_plugin_built_for_a_newer_interface_is_listed_with_the_way_out_and_does_not_run() {
    let dir = tempfile::tempdir().unwrap();
    let later = installed(dir.path(), "later", ABI_VERSION + 1);

    let why = later.refused.clone().expect("it is refused");
    assert!(why.contains("update OpenRage"), "{why}");
    assert!(
        later.notes().is_empty(),
        "a refused plugin has one sentence, the refusal"
    );

    let mut state = state_with(&later);
    state.request_plugin_run("later");
    let report = state.plugin_report("later").expect("a report");
    assert_eq!(report[0].key, "plugin.rejected");
    assert!(
        report[0].message.contains("update OpenRage"),
        "{:?}",
        report[0].message
    );
}

#[test]
fn a_plugin_of_interface_0_that_wants_what_only_interface_1_has_is_refused_with_a_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("liar");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::copy(guest(), folder.join("plugin.wasm")).unwrap();
    std::fs::write(
        folder.join("plugin.json"),
        r#"{"id":"liar","name":"Liar","abi":0,"wants":["cap.write_files"]}"#,
    )
    .unwrap();

    let plugin = InstalledPlugin::read(&folder);

    let why = plugin.refused.expect("it is refused");
    assert!(
        why.contains("rebuild the plugin") && why.contains("interface 1"),
        "{why}"
    );
}
