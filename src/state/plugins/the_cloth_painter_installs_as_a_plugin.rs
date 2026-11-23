//! The cloth painter of R46 installs as a plugin and paints a `.yld` open in the editor: its panel
//! draws, a click reaches it, its overlay is the vertices of the level, and the file it writes is the
//! one the format's own save check keeps. Nothing in the editor is its own.
//!
//! ⚠ The guest is built by a nested `cargo build` for wasm32-unknown-unknown, as the other plugin
//! suites build theirs, and the cloth is a staged `.yld`, which this skips without.

use std::path::{Path, PathBuf};

use or_io::decl::File as _;
use or_io::gta5::{CharacterCloth, PgDictionary};
use or_plugin::{Event, Widget};

use super::{Consent, InstalledPlugin};
use crate::state::a_plugin_asks_before_it_runs::built_plugin;
use crate::state::{AppState, Document, DocumentKind, EditMode};

const ID: &str = "cloth-painter";

fn guest() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            built_plugin(
                "or-plugin-cloth-painter",
                "",
                "openrage-plugin-cloth-painter",
            )
        })
        .clone()
}

/// The plugin as a user installs it: the manifest it ships and the module built from it.
fn installed(plugins: &Path) -> InstalledPlugin {
    let dir = plugins.join(ID);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(guest(), dir.join("plugin.wasm")).unwrap();
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/cloth-painter/plugin.json");
    std::fs::copy(shipped, dir.join("plugin.json")).unwrap();
    let plugin = InstalledPlugin::read(&dir);
    assert_eq!(plugin.refused, None, "what the plugin ships is a plugin");
    // It is built for interface 0, which the editor still runs as it was built, and the manager says so.
    assert!(
        plugin
            .notes()
            .iter()
            .any(|note| note.contains("still loads")),
        "{:?}",
        plugin.notes()
    );
    plugin
}

type Cloth = PgDictionary<CharacterCloth>;

/// A state with the painter installed and answered yes for, and `bytes` open as `thing.yld` in `folder`.
fn state_over(plugin: &InstalledPlugin, bytes: &[u8], folder: &Path) -> AppState {
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
    state.prefs.edit_mode = EditMode::Overlay;

    let path = folder.join("thing.yld");
    let kind = DocumentKind::Yld;
    let handler = crate::state::payload_handlers::handler_for(
        kind.resource_type().expect("a paged format"),
        or_io::Generation::Gen8,
    )
    .expect("a payload handler");
    let payload = (handler.parse)(&path, bytes).expect("the staged cloth reads");
    let mut doc = Document::new(1, Some(path), payload);
    doc.set_resource(crate::state::document_io::resource_image_for(kind, bytes));
    state.documents.push(doc);
    state.active_document = Some(1);
    state
}

fn labels(widgets: &[Widget]) -> Vec<&str> {
    widgets
        .iter()
        .filter_map(|w| match w {
            Widget::Label { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn clicked(id: &str) -> Event {
    Event::Clicked { id: id.into() }
}

fn typed(id: &str, text: &str) -> Event {
    Event::Typed {
        id: id.into(),
        text: text.into(),
    }
}

/// The weights of the first level of the first cloth of the open document.
fn weights(state: &AppState) -> Vec<f32> {
    let doc = state.active_document().expect("a document is open");
    let crate::state::DocPayload::Resource(or_io::Asset::ClothDict(dict)) = &doc.payload else {
        panic!("the document is a cloth dictionary");
    };
    let bridge = dict.entries[0]
        .as_deref()
        .and_then(|c| c.cloth_controller.as_deref())
        .and_then(|c| c.base.bridge.as_deref())
        .expect("the staged cloth has a bridge");
    bridge.vertex_weight_0.clone()
}

#[test]
fn the_painter_installs_draws_in_a_panel_paints_the_open_cloth_and_the_editor_keeps_what_it_writes()
{
    let Some(file) = or_test_support::Corpus::with_root(
        or_test_support::shared_corpus_root(env!("CARGO_MANIFEST_DIR")),
        "yld",
    )
    .target(or_test_support::TARGET_GEN8)
    .limit(1)
    .find_or_skip("a staged .yld through the cloth painter in the app")
    .and_then(|files| files.into_iter().next()) else {
        return;
    };
    let bytes = std::fs::read(&file).unwrap();
    let before = Cloth::from_bytes(&bytes).expect("the staged cloth opens");
    let wanted: usize = before.entries[0]
        .as_deref()
        .map_or(0, |c| c.lod_vertex_count(0));
    assert!(wanted > 0, "a staged cloth has vertices at its first level");

    let dir = tempfile::tempdir().unwrap();
    let plugin = installed(&dir.path().join("plugins"));
    let mut state = state_over(&plugin, &bytes, dir.path());
    state.load_plugin_surfaces();

    let registered = state
        .plugin_registrations(ID)
        .expect("the painter is loaded");
    assert_eq!(
        registered
            .panels
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["painter"]
    );
    assert_eq!(
        state.plugin_overlays().len(),
        0,
        "nothing is drawn before its first frame"
    );

    let widgets = state.plugin_panel(ID, "painter").expect("the panel draws");
    let said = labels(&widgets).join("\n");
    assert!(
        said.contains(&format!("{wanted} vertices, 0 selected")),
        "the panel states the level the document holds: {said}"
    );
    let overlay = state.plugin_overlays();
    assert_eq!(overlay.len(), 1, "the painter draws over the open document");
    assert_eq!(
        overlay[0].len(),
        wanted,
        "one mark for each vertex of the level"
    );

    state.deliver_plugin_events(
        ID,
        "painter",
        vec![
            clicked("all"),
            typed("weight", "0.003"),
            clicked("paint_weight"),
        ],
    );
    let widgets = state.plugin_panel(ID, "painter").expect("the panel draws");
    assert!(
        labels(&widgets).iter().any(|l| l.starts_with("Painted ")),
        "{:?}",
        labels(&widgets)
    );
    assert_eq!(
        weights(&state)[0],
        before.entries[0]
            .as_deref()
            .unwrap()
            .cloth_controller
            .as_deref()
            .unwrap()
            .base
            .bridge
            .as_deref()
            .unwrap()
            .vertex_weight_0[0],
        "painting changes the guest's model and not yet the document"
    );
    assert!(!state.active_document().unwrap().dirty);

    state.deliver_plugin_events(ID, "painter", vec![clicked("write")]);
    let widgets = state.plugin_panel(ID, "painter").expect("the panel draws");
    assert!(
        labels(&widgets).iter().any(|l| l.starts_with("Wrote ")),
        "the editor's own save check kept the file: {:?}",
        labels(&widgets)
    );
    let doc = state.active_document().expect("open");
    assert!(doc.dirty, "the written file is the open document, unsaved");
    let painted = weights(&state);
    assert!(
        painted.iter().take(wanted).all(|w| *w == 0.003),
        "every vertex of the level holds the weight that was typed: {painted:?}"
    );
}

#[test]
fn the_painter_is_refused_its_write_while_the_editor_is_read_only() {
    let Some(file) = or_test_support::Corpus::with_root(
        or_test_support::shared_corpus_root(env!("CARGO_MANIFEST_DIR")),
        "yld",
    )
    .target(or_test_support::TARGET_GEN8)
    .limit(1)
    .find_or_skip("a staged .yld through the cloth painter in the app")
    .and_then(|files| files.into_iter().next()) else {
        return;
    };
    let bytes = std::fs::read(&file).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let plugin = installed(&dir.path().join("plugins"));
    // The document is in the game's install, where a person is asked and a plugin is refused.
    let mut state = state_over(&plugin, &bytes, &dir.path().join("mods"));
    state
        .prefs
        .game_paths
        .set(or_title::Game::GtaVLegacy, Some(dir.path().to_path_buf()));
    state.retail_edit_session_allow = false;
    state.prefs.edit_mode = EditMode::ReadOnly;
    state.load_plugin_surfaces();

    state.plugin_panel(ID, "painter").expect("the panel draws");
    state.deliver_plugin_events(
        ID,
        "painter",
        vec![
            clicked("all"),
            typed("weight", "0.003"),
            clicked("paint_weight"),
        ],
    );
    state.plugin_panel(ID, "painter").expect("the panel draws");
    state.deliver_plugin_events(ID, "painter", vec![clicked("write")]);
    let widgets = state.plugin_panel(ID, "painter").expect("the panel draws");

    assert!(
        !state.active_document().unwrap().dirty,
        "nothing was written"
    );
    assert!(
        labels(&widgets).iter().any(|l| l.contains("host refused")),
        "the painter shows the host's refusal: {:?}",
        labels(&widgets)
    );
}
