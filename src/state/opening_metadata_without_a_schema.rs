//! The owner's `images.meta` and `weather.xml` open as metadata no schema
//! declares, a binary `.srl` as its frames, and `preload.list` as text, each
//! from the install's tree in the workspace that fits it.
//!
//! ⚠ Install-gated: the paths are the ones the owner opened, so only the
//! install's own archives are honest input.

use std::time::{Duration, Instant};

use or_io::meta::Value;
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

use super::opening_a_ydd::{finish_load, node_at, with_keys};
use super::{AppState, DocPayload, DocumentId, DocumentKind};
use crate::ui::dock::Workspace;

const IMAGES_META: &str = "common.rpf/data/levels/gta5/images.meta";
const WEATHER_XML: &str = "common.rpf/data/levels/gta5/weather.xml";
const FAMILY_4_SRL: &str = "common.rpf/data/streaming/family_4_int.srl";
const PRELOAD_LIST: &str = "common.rpf/shaders/preload.list";

fn open_from_tree(state: &mut AppState, game: DocumentId, path: &str) -> DocumentId {
    let node = node_at(state, game, path);
    state.clear_error();
    state.open_rpf_entry(game, node);
    finish_load(state).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The bytes `path` saves as, written through the app's own save.
fn saved(state: &mut AppState, doc: DocumentId, path: &str) -> Vec<u8> {
    state.active_document = Some(doc);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let out = dir
        .path()
        .join(path.rsplit('/').next().expect("a file name"));
    assert!(
        state.save_active_document_as(out.clone()),
        "{path}: no save"
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::read(&out).unwrap_or_else(|e| panic!("{path}: the save wrote nothing: {e}"))
}

fn parsed_meta(state: &AppState, doc: DocumentId, path: &str) -> or_io::meta::MetaInstance {
    let doc = state.document(doc).expect("the opened document");
    assert_eq!(doc.kind.info().workspace, Workspace::Data, "{path}");
    let DocPayload::Meta(meta) = &doc.payload else {
        panic!("{path}: opened as {:?}", doc.kind);
    };
    let parsed = meta.parsed.as_ref().unwrap_or_else(|| {
        panic!("{path}: no tree: {:?}", meta.parse_error);
    });
    parsed.instance.clone()
}

#[test]
fn the_files_the_owner_could_not_open_open_from_the_tree() {
    const WHAT: &str = "images.meta, weather.xml, a binary .srl and preload.list open";
    let Some(root) = game_roots().into_iter().find(|root| {
        root.join("GTA5.exe").is_file()
            && !root.join("GTA5_Enhanced.exe").is_file()
            && root.join("common.rpf").is_file()
    }) else {
        skip_install(
            WHAT,
            format_args!("no Legacy install root; set {GAME_ROOT_ENV}"),
        );
        return;
    };

    let mut state = with_keys(&root);
    state.open_game_root(root.clone());
    let game = state.documents[0].id;

    let images = open_from_tree(&mut state, game, IMAGES_META);
    let instance = parsed_meta(&state, images, IMAGES_META);
    assert_eq!(instance.type_name, "dataFiles");
    assert!(
        instance.fields.iter().all(|(name, _)| name == "Item"),
        "{IMAGES_META}: a root field that is not an item"
    );

    // An edit in the visual editor reaches the saved XML.
    let doc = state.document_mut(images).expect("the document");
    let DocPayload::Meta(meta) = &mut doc.payload else {
        unreachable!("checked above");
    };
    let Some((_, Value::Struct(first))) = meta
        .parsed
        .as_mut()
        .and_then(|p| p.instance.fields.first_mut())
    else {
        panic!("{IMAGES_META}: the first item is not a struct");
    };
    first.set("filename", Value::String("platform:/edited.rpf".into()));
    meta.after_visual_edit_in_place();
    meta.ensure_text_current();
    let xml = saved(&mut state, images, IMAGES_META);
    let xml = String::from_utf8(xml).expect("XML");
    assert!(
        xml.contains("<filename>platform:/edited.rpf</filename>"),
        "{IMAGES_META}: the edit is not in the save"
    );

    let weather = open_from_tree(&mut state, game, WEATHER_XML);
    let instance = parsed_meta(&state, weather, WEATHER_XML);
    assert_eq!(instance.type_name, "CContentsOfWeatherXmlFile");

    let srl = open_from_tree(&mut state, game, FAMILY_4_SRL);
    let instance = parsed_meta(&state, srl, FAMILY_4_SRL);
    let Some(Value::Array(frames)) = instance.get("Frames") else {
        panic!("{FAMILY_4_SRL}: no frames");
    };
    assert_eq!(frames.len(), 127, "{FAMILY_4_SRL}");
    let node = node_at(&mut state, game, FAMILY_4_SRL);
    let DocPayload::Rpf(rpf) = &state.document(game).expect("the game root").payload else {
        unreachable!("the game root is an archive document");
    };
    let shipped = rpf.session.bytes_for(node).expect("the entry's bytes");
    assert!(
        saved(&mut state, srl, FAMILY_4_SRL) == *shipped,
        "{FAMILY_4_SRL}: the save is not the shipped bytes"
    );

    let list = open_from_tree(&mut state, game, PRELOAD_LIST);
    let doc = state.document(list).expect("the opened document");
    assert_eq!(doc.kind, DocumentKind::Text, "{PRELOAD_LIST}");
    let DocPayload::Text(text) = &doc.payload else {
        panic!("{PRELOAD_LIST}: opened as {:?}", doc.kind);
    };
    assert!(
        text.text.to_string().contains("default.fx"),
        "{PRELOAD_LIST}"
    );
}
