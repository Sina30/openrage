//! The app embeds the plugin host: a plugin's declared capabilities are shown
//! before it first runs, a grant is recorded and revoked, and a refused plugin
//! shows the refusal's key and sentence rather than appearing to do nothing.
//!
//! ⚠ The sample guest is built by a nested `cargo build` for
//! wasm32-unknown-unknown into the test binary's target directory, so that target must be installed, as
//! `crates/domain/or-plugin/tests/the_sample_guest_runs_under_the_host.rs`
//! also requires.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use or_plugin::{Capability, Manifest, ABI_VERSION};

use super::plugins::{Consent, InstalledPlugin};
use super::{AppState, Document, DocumentKind, EditMode};

const WASM32: &str = "wasm32-unknown-unknown";

/// The sample guest built with `features`, into its own directory under the test binary's target directory.
pub(super) fn built_sample(features: &str, directory: &str) -> PathBuf {
    built_plugin("or-plugin-sample", features, directory)
}

/// The plugin `package` of `plugins/` built with `features`, into its own directory under the test binary's target directory.
pub(super) fn built_plugin(package: &str, features: &str, directory: &str) -> PathBuf {
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
    // ⚠ Under the test binary's own target directory (`target/debug/deps/<binary>`), never `$TEMP`:
    // it is kept between runs, so a folder there would outlive the lane that built it.
    let binary = std::env::current_exe().expect("the test binary's path");
    let target_dir = (binary.ancestors().nth(3))
        .expect("a target directory")
        .join(directory);
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build
        .current_dir(&plugins)
        .args(["build", "--release", "-p", package, "--target", WASM32]);
    if !features.is_empty() {
        build.args(["--features", features]);
    }
    let status = build
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("cargo runs");
    assert!(
        status.success(),
        "the plugin `{package}` did not build for {WASM32}; if the target is missing, \
         `rustup target add {WASM32}`"
    );
    target_dir
        .join(WASM32)
        .join(format!("release/{}.wasm", package.replace('-', "_")))
}

fn sample_guest() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| built_sample("", "openrage-plugin-sample"))
        .clone()
}

/// The sample guest that parses a `.ydd` with `or-io` linked into it.
fn bytes_guest() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| built_sample("bytes", "openrage-plugin-sample-bytes"))
        .clone()
}

/// The sample guest under `id`, declaring what it calls: reading and patching the open document.
fn sample(id: &str) -> InstalledPlugin {
    InstalledPlugin {
        manifest: Manifest {
            id: id.into(),
            name: "Sample".into(),
            abi: ABI_VERSION,
            wants: vec![Capability::ReadOpenDocument, Capability::EditOpenDocument],
        },
        wasm: sample_guest(),
        refused: None,
    }
}

const KIND: DocumentKind = DocumentKind::Ybn;

/// A state with `plugin` installed and a blank declared resource open at `path`.
fn state_with(plugin: &InstalledPlugin, path: PathBuf) -> AppState {
    let mut state = AppState::default();
    state.plugins.scanned = Some(vec![plugin.clone()]);

    let resource = KIND.resource_type().expect("a paged format");
    let handler = super::payload_handlers::handler_for(resource, or_io::Generation::Gen8)
        .expect("a payload handler");
    let payload = (handler.empty)();
    let bytes = handler.save(&payload).expect("a blank serializes");
    let mut doc = Document::new(1, Some(path), payload);
    doc.set_resource(super::document_io::resource_image_for(KIND, &bytes));
    assert!(
        super::described_root(&doc).is_some(),
        "the sample reads a described root"
    );
    state.documents.push(doc);
    state.active_document = Some(1);
    state
}

fn loose() -> PathBuf {
    std::env::temp_dir().join(format!("loose.{}", KIND.extension()))
}

fn keys(state: &AppState, id: &str) -> Vec<(String, Option<String>)> {
    state
        .plugin_report(id)
        .unwrap_or_default()
        .iter()
        .map(|d| {
            (
                d.key.to_string(),
                d.arg("capability").map(ToString::to_string),
            )
        })
        .collect()
}

#[test]
fn the_sample_guest_shows_what_it_declared_before_it_runs() {
    let plugin = sample("asks-first");
    let mut state = state_with(&plugin, loose());

    state.request_plugin_run("asks-first");

    let pending = state
        .plugins
        .pending_consent
        .as_ref()
        .expect("a plugin nobody has answered about is asked about first");
    assert_eq!(
        pending.manifest.wants,
        vec![Capability::ReadOpenDocument, Capability::EditOpenDocument],
        "the prompt shows what the manifest declared"
    );
    let doc = state.active_document().expect("open");
    assert!(
        !doc.dirty && state.plugin_report("asks-first").is_none(),
        "and nothing ran before the answer"
    );
}

#[test]
fn a_grant_is_recorded_and_revoking_it_asks_again() {
    let plugin = sample("granted");
    let mut state = state_with(&plugin, loose());

    state.request_plugin_run("granted");
    state.answer_plugin_consent(true);

    assert_eq!(
        state.prefs.plugin_consent.get("granted"),
        Some(&Consent::Granted(
            ["cap.edit_open_document", "cap.read_open_document"]
                .map(String::from)
                .into()
        )),
        "the grant is recorded in the preferences by capability key"
    );
    assert!(state.plugins.pending_consent.is_none());
    assert!(
        state.active_document().expect("open").dirty,
        "the sample ran and patched the document: {:?}",
        state.plugin_report("granted")
    );

    state.request_plugin_run("granted");
    assert!(
        state.plugins.pending_consent.is_none(),
        "a recorded grant is not asked again"
    );

    state.revoke_plugin_consent("granted");
    assert!(!state.prefs.plugin_consent.contains_key("granted"));
    state.request_plugin_run("granted");
    assert!(
        state.plugins.pending_consent.is_some(),
        "a revoked grant is a question again"
    );
}

#[test]
fn a_declined_plugin_shows_the_refusal_key_and_sentence() {
    let plugin = sample("declined");
    let mut state = state_with(&plugin, loose());

    state.request_plugin_run("declined");
    state.answer_plugin_consent(false);

    assert_eq!(
        state.prefs.plugin_consent.get("declined"),
        Some(&Consent::Declined)
    );
    assert!(!state.active_document().expect("open").dirty);
    let report = state
        .plugin_report("declined")
        .expect("a declined run says why");
    let edit = report
        .iter()
        .find(|d| {
            d.arg("capability").map(ToString::to_string).as_deref()
                == Some("cap.edit_open_document")
        })
        .unwrap_or_else(|| panic!("the refusal names the capability: {report:?}"));
    assert_eq!(edit.key, "plugin.refused");
    assert!(
        edit.message
            .contains(Capability::EditOpenDocument.asks_for()),
        "the sentence a prompt shows: {}",
        edit.message
    );

    state.request_plugin_run("declined");
    assert!(
        state.plugins.pending_consent.is_none(),
        "a declined plugin is not asked again until the answer is revoked"
    );
}

/// A user who set the editor read-only does not become writable by installing a plugin.
#[test]
fn a_granted_edit_is_refused_by_the_edit_mode_and_says_so() {
    let root = std::env::temp_dir().join("openrage-plugin-install");
    let plugin = sample("read-only");
    let mut state = state_with(
        &plugin,
        root.join(format!("x64a/thing.{}", KIND.extension())),
    );
    state
        .prefs
        .game_paths
        .set(or_title::Game::GtaVLegacy, Some(root.clone()));
    state.prefs.edit_mode = EditMode::ReadOnly;
    state.retail_edit_session_allow = false;

    state.request_plugin_run("read-only");
    state.answer_plugin_consent(true);

    assert!(
        !state.active_document().expect("open").dirty,
        "the edit mode refused the patch"
    );
    assert!(
        keys(&state, "read-only").contains(&(
            "plugin.refused".to_string(),
            Some("cap.edit_open_document".to_string())
        )),
        "{:?}",
        state.plugin_report("read-only")
    );
    let report = state.plugin_report("read-only").expect("a report");
    assert!(
        report
            .iter()
            .any(|d| d.message.contains(EditMode::ReadOnly.semantics())),
        "the refusal carries the editor's own sentence: {report:?}"
    );
}

/// A Legacy `.ydd` of one drawable whose first level-of-detail distance is `first`.
fn authored_ydd(first: f32) -> Vec<u8> {
    use or_io::decl::{File as _, Gen8};
    use or_io::gta5::{GtaDrawable, PgDictionary};

    let mut drawable = GtaDrawable::<Gen8>::default();
    drawable.base.lod_group.lod_distances = [first, 200.0, 400.0, 800.0];
    PgDictionary::<GtaDrawable<Gen8>> {
        codes: vec![0x1234],
        entries: vec![Some(Box::new(drawable))],
        ..Default::default()
    }
    .to_bytes()
    .expect("a dictionary of one drawable saves")
}

pub(super) fn first_distance(doc: &Document) -> f32 {
    let super::document::DocPayload::Resource(or_io::Asset::DrawableDict(dictionary)) =
        &doc.payload
    else {
        panic!(
            "a .ydd holds a drawable dictionary, and this holds {:?}",
            doc.kind
        );
    };
    dictionary.entries[0]
        .as_ref()
        .expect("the dictionary's one drawable")
        .base
        .lod_group
        .lod_distances[0]
}

/// The sample guest that parses with `or-io`, declaring what it calls: a file's bytes, read and replaced.
fn bytes_plugin(id: &str) -> InstalledPlugin {
    InstalledPlugin {
        manifest: Manifest {
            id: id.into(),
            name: "Bytes".into(),
            abi: ABI_VERSION,
            wants: vec![Capability::ReadFileBytes, Capability::WriteFileBytes],
        },
        wasm: bytes_guest(),
        refused: None,
    }
}

/// A state with `plugin` installed and `authored_ydd(100.0)` open at `path`, on the editor's own fuel.
pub(super) fn state_with_ydd(plugin: &InstalledPlugin, path: PathBuf) -> AppState {
    let mut state = AppState::default();
    state.plugins.scanned = Some(vec![plugin.clone()]);

    let bytes = authored_ydd(100.0);
    let resource = DocumentKind::Ydd.resource_type().expect("a paged format");
    let handler = super::payload_handlers::handler_for(resource, or_io::Generation::Gen8)
        .expect("a payload handler");
    let payload = (handler.parse)(&path, &bytes).expect("the authored .ydd reads");
    let mut doc = Document::new(1, Some(path), payload);
    doc.set_resource(super::document_io::resource_image_for(
        DocumentKind::Ydd,
        &bytes,
    ));
    state.documents.push(doc);
    state.active_document = Some(1);
    state
}

#[test]
fn a_plugin_that_reads_and_replaces_a_file_shows_both_before_it_runs() {
    let plugin = bytes_plugin("bytes-asks");
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = state_with_ydd(&plugin, dir.path().join("thing.ydd"));

    state.request_plugin_run("bytes-asks");

    let pending = state
        .plugins
        .pending_consent
        .as_ref()
        .expect("a plugin nobody has answered about is asked about first");
    assert_eq!(
        pending.manifest.wants,
        vec![Capability::ReadFileBytes, Capability::WriteFileBytes]
    );
    assert_eq!(
        first_distance(state.active_document().expect("open")),
        100.0,
        "and nothing ran before the answer"
    );
}

#[test]
fn a_granted_plugin_reads_the_file_parses_it_in_the_guest_and_the_document_becomes_what_it_wrote() {
    let plugin = bytes_plugin("bytes-run");
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("thing.ydd");
    let mut state = state_with_ydd(&plugin, path.clone());

    state.request_plugin_run("bytes-run");
    state.answer_plugin_consent(true);

    let doc = state.active_document().expect("open");
    assert_eq!(
        first_distance(doc),
        200.0,
        "the guest parsed the file with or-io, doubled the distance and wrote it back: {:?}",
        state.plugin_report("bytes-run")
    );
    assert!(doc.dirty, "the document is changed and unsaved");
    assert!(
        keys(&state, "bytes-run").contains(&("plugin.changed".to_string(), None)),
        "{:?}",
        state.plugin_report("bytes-run")
    );
    assert!(
        !path.exists(),
        "a plugin changes the open document, and saving it is the user's, through the gate"
    );
}

/// A plugin's write is a step of the document's history, so Undo takes it back and Redo makes it again, and the history the user had is kept.
#[test]
fn a_plugin_that_replaces_a_file_is_one_step_of_undo_and_the_history_before_it_is_kept() {
    let plugin = bytes_plugin("bytes-undo");
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("thing.ydd");
    let mut state = state_with_ydd(&plugin, path);
    let doc_id = state.active_document().expect("open").id;
    let own = crate::state::edit_history::Change::Bytes {
        at: 0,
        other: vec![0; 1],
    };
    state
        .document_mut(doc_id)
        .unwrap()
        .history
        .push("An edit of the user's".into(), own, None);
    let before = first_distance(state.active_document().unwrap());

    state.request_plugin_run("bytes-undo");
    state.answer_plugin_consent(true);

    assert_eq!(
        first_distance(state.active_document().unwrap()),
        before * 2.0
    );
    let labels: Vec<String> = (state.active_document().unwrap().history.done().iter())
        .map(|s| s.label.clone())
        .collect();
    assert_eq!(
        labels,
        ["An edit of the user's", "Bytes changed the document"],
        "the user's step is kept and the run is one step after it"
    );

    assert_eq!(state.undo().as_deref(), Some("Bytes changed the document"));
    assert_eq!(first_distance(state.active_document().unwrap()), before);
    assert_eq!(state.redo().as_deref(), Some("Bytes changed the document"));
    assert_eq!(
        first_distance(state.active_document().unwrap()),
        before * 2.0
    );
}

/// The write goes through the edit mode as a patch does: the same grant, refused where the editor refuses.
#[test]
fn a_plugin_replacing_a_file_is_refused_in_read_only_mode_and_accepted_in_overlay_mode() {
    let root = tempfile::tempdir().expect("a scratch directory");
    let plugin = bytes_plugin("bytes-gated");
    let mut state = state_with_ydd(&plugin, root.path().join("mods/thing.ydd"));
    state
        .prefs
        .game_paths
        .set(or_title::Game::GtaVLegacy, Some(root.path().to_path_buf()));
    state.prefs.edit_mode = EditMode::ReadOnly;
    state.retail_edit_session_allow = false;

    state.request_plugin_run("bytes-gated");
    state.answer_plugin_consent(true);

    assert_eq!(
        first_distance(state.active_document().expect("open")),
        100.0,
        "the edit mode refused the file"
    );
    assert!(!state.active_document().expect("open").dirty);
    assert!(
        keys(&state, "bytes-gated").contains(&(
            "plugin.refused".to_string(),
            Some("cap.write_file_bytes".to_string())
        )),
        "{:?}",
        state.plugin_report("bytes-gated")
    );
    let report = state.plugin_report("bytes-gated").expect("a report");
    assert!(
        report
            .iter()
            .any(|d| d.message.contains(EditMode::ReadOnly.semantics())),
        "the refusal carries the editor's own sentence: {report:?}"
    );

    state.prefs.edit_mode = EditMode::Overlay;
    state.request_plugin_run("bytes-gated");

    assert_eq!(
        first_distance(state.active_document().expect("open")),
        200.0,
        "the same grant, once the edit mode permits the overlay: {:?}",
        state.plugin_report("bytes-gated")
    );
    assert!(state.active_document().expect("open").dirty);
}

/// The file a plugin writes must be one the document's format reads back and saves, at the document's own target.
#[test]
fn a_written_file_must_read_back_and_save_at_the_documents_target() {
    use or_io::decl::{File as _, Gen9};
    use or_io::gta5::gen8::GrcTexture;
    use or_io::gta5::{GtaDrawable, PgDictionary};
    use or_plugin::SaveCheck as _;

    let plugin = bytes_plugin("bytes-check");
    let dir = tempfile::tempdir().expect("a scratch directory");
    let state = state_with_ydd(&plugin, dir.path().join("thing.ydd"));
    let check = super::plugins::save_check(state.active_document().expect("open"))
        .expect("a .ydd has a save check");

    assert_eq!(
        check.why_the_file_would_be_invalid(&authored_ydd(300.0)),
        None,
        "another .ydd of the same target"
    );

    let not_rsc7 = check
        .why_the_file_would_be_invalid(b"not a resource at all")
        .expect("bytes that are no file");
    assert!(not_rsc7.contains("not a game resource file"), "{not_rsc7}");

    let enhanced = PgDictionary::<GtaDrawable<Gen9>>::default()
        .to_bytes()
        .expect("an Enhanced dictionary saves");
    let other_target = check
        .why_the_file_would_be_invalid(&enhanced)
        .expect("the other target's layout would read as nonsense");
    assert!(
        other_target.contains("Enhanced file") && other_target.contains("Legacy one"),
        "the sentence names the targets as a user reads them: {other_target}"
    );

    let textures = PgDictionary::<GrcTexture>::default()
        .to_bytes()
        .expect("a texture dictionary saves");
    let other_format = check
        .why_the_file_would_be_invalid(&textures)
        .expect("a .ytd's version is not a .ydd's");
    assert!(
        other_format.contains("not a file of this format"),
        "{other_format}"
    );
}

#[test]
fn a_plugin_directory_is_read_and_a_broken_one_is_kept_with_its_reason() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let good = dir.path().join("sample");
    std::fs::create_dir_all(&good).expect("mkdir");
    std::fs::write(
        good.join("plugin.json"),
        r#"{"id":"sample","name":"Sample","abi":0,"wants":["cap.read_open_document","cap.edit_open_document"]}"#,
    )
    .expect("write");
    std::fs::copy(sample_guest(), good.join("plugin.wasm")).expect("copy");

    let bad = dir.path().join("greedy");
    std::fs::create_dir_all(&bad).expect("mkdir");
    std::fs::write(
        bad.join("plugin.json"),
        r#"{"id":"greedy","name":"Greedy","abi":0,"wants":["cap.read_the_users_email"]}"#,
    )
    .expect("write");

    std::fs::create_dir_all(dir.path().join("not-a-plugin")).expect("mkdir");

    let found = super::plugins::installed_in(dir.path());
    assert_eq!(
        found
            .iter()
            .map(|p| p.manifest.id.as_str())
            .collect::<Vec<_>>(),
        vec!["greedy", "sample"],
        "a directory with no plugin.json is not a plugin"
    );
    assert!(found[1].refused.is_none(), "{:?}", found[1].refused);
    assert_eq!(found[1].manifest.wants, sample("sample").manifest.wants);
    let why = found[0]
        .refused
        .as_deref()
        .expect("an unknown capability is refused");
    assert!(why.contains("cap.read_the_users_email"), "{why}");
}

/// A manifest names the file-bytes capabilities by key.
#[test]
fn a_manifest_wanting_a_files_bytes_is_read_with_both_capabilities() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let tool = dir.path().join("bytes");
    std::fs::create_dir_all(&tool).expect("mkdir");
    std::fs::write(
        tool.join("plugin.json"),
        r#"{"id":"bytes","name":"Bytes","abi":0,"wants":["cap.read_file_bytes","cap.write_file_bytes"]}"#,
    )
    .expect("write");
    std::fs::write(tool.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("write");

    let found = super::plugins::installed_in(dir.path());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].refused, None, "both keys are defined");
    assert_eq!(
        found[0]
            .manifest
            .wants
            .iter()
            .map(|c| c.key())
            .collect::<Vec<_>>(),
        ["cap.read_file_bytes", "cap.write_file_bytes"]
    );
}

/// An unreadable answer is asked again; it does not cost the rest of the preferences.
#[test]
fn an_unreadable_answer_costs_only_itself() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("prefs.json");
    std::fs::write(
        &path,
        r#"{"read_overlay_first":false,"plugin_consent":{"kept":"Declined","garbled":{"Granted":7}}}"#,
    )
    .expect("write");

    let prefs = super::prefs::load_prefs_from(&path);
    assert!(!prefs.read_overlay_first, "the rest of the file was read");
    assert_eq!(
        prefs.plugin_consent.into_iter().collect::<Vec<_>>(),
        vec![("kept".to_string(), Consent::Declined)]
    );
}
