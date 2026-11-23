//! A preset saved from one metadata record, applied to a selection of a hundred.

use super::presets::{MetaStep, PresetLibrary, PRESET_VERSION};
use super::*;
use or_io::meta::{MetaInstance, Value};

const RECORDS: usize = 100;

fn entity(lod_dist: f64) -> Value {
    let mut e = MetaInstance::new("CEntityDef");
    e.set("archetypeName", Value::String(format!("prop_{lod_dist}")));
    e.set("lodDist", Value::Float(lod_dist));
    e.set("flags", Value::UInt(0));
    Value::Struct(Box::new(e))
}

/// A map document of a hundred entities and one room, whose `entities` array is the selection.
fn state_with_map(presets: &std::path::Path) -> (AppState, DocumentId, Vec<MetaStep>) {
    let mut items: Vec<Value> = (0..RECORDS).map(|i| entity(i as f64)).collect();
    items.push(Value::Struct(Box::new(MetaInstance::new("CMloRoomDef"))));
    let mut map = MetaInstance::new("CMapData");
    map.set("entities", Value::Array(items));
    let mut meta = MetaDocument::empty();
    meta.parsed
        .as_mut()
        .expect("an empty document parses")
        .instance = map;

    let mut state = AppState::new();
    state.documents.clear();
    let id: DocumentId = 1;
    state.documents.push(Document::new(
        id,
        Some(PathBuf::from("map.ymap")),
        DocPayload::Meta(meta),
    ));
    state.active_document = Some(id);
    state.presets = PresetLibrary::at(presets.to_path_buf());
    (state, id, vec![MetaStep::Field("entities".into())])
}

fn entities(state: &AppState, id: DocumentId) -> Vec<Value> {
    let Some(DocPayload::Meta(meta)) = state.document(id).map(|d| &d.payload) else {
        panic!("the map is a metadata document");
    };
    match meta
        .parsed
        .as_ref()
        .and_then(|p| p.instance.get("entities"))
    {
        Some(Value::Array(items)) => items.clone(),
        other => panic!("entities is an array: {other:?}"),
    }
}

fn field(record: &Value, name: &str) -> Value {
    let Value::Struct(inst) = record else {
        panic!("a record is a struct: {record:?}");
    };
    inst.get(name).cloned().unwrap_or(Value::Null)
}

#[test]
fn a_preset_from_one_record_applies_to_a_hundred_in_one_undo_step() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, id, path) = state_with_map(dir.path());

    // The preset's values come from record 7 after the user edits it.
    if let Some(DocPayload::Meta(meta)) = state.document_mut(id).map(|d| &mut d.payload) {
        let instance = &mut meta.parsed.as_mut().unwrap().instance;
        let Some(Value::Array(items)) = instance
            .fields
            .iter_mut()
            .find(|(n, _)| n == "entities")
            .map(|(_, v)| v)
        else {
            unreachable!()
        };
        let Value::Struct(seventh) = &mut items[7] else {
            unreachable!()
        };
        seventh.set("lodDist", Value::Float(250.0));
        seventh.set("flags", Value::UInt(0x20));
    }
    state
        .save_preset(
            id,
            &path,
            7,
            "far props",
            &["lodDist".into(), "flags".into()],
        )
        .expect("the preset saves");

    let reloaded = PresetLibrary::load(dir.path().to_path_buf());
    assert!(reloaded.refused.is_empty(), "{:?}", reloaded.refused);
    assert_eq!(
        reloaded.list.len(),
        1,
        "the preset is a file under the directory"
    );
    state.presets = reloaded;

    let before = entities(&state, id);
    for i in 0..=RECORDS {
        state.records_selected.toggle(id, &path, i);
    }
    let undo_before = state.document(id).unwrap().history.done().len();
    let report = state.apply_preset(id, 0).expect("the preset applies");

    assert_eq!(report.applied, RECORDS);
    assert_eq!(report.refused.len(), 1, "{:?}", report.refused);
    assert_eq!(report.refused[0].0, RECORDS);
    assert!(
        report.refused[0].1.contains("CMloRoomDef"),
        "the refusal names what the record is: {}",
        report.refused[0].1
    );

    let after = entities(&state, id);
    for record in &after[..RECORDS] {
        assert_eq!(field(record, "lodDist"), Value::Float(250.0));
        assert_eq!(field(record, "flags"), Value::UInt(0x20));
    }
    assert_eq!(
        field(&after[3], "archetypeName"),
        field(&before[3], "archetypeName"),
        "a field the preset did not choose is left alone"
    );
    assert_eq!(
        state.document(id).unwrap().history.done().len(),
        undo_before + 1
    );
    assert!(state.document(id).unwrap().dirty);

    state.undo().expect("the apply undoes");
    let undone = entities(&state, id);
    assert_eq!(
        undone[..RECORDS],
        before[..RECORDS],
        "one undo restores all hundred"
    );
    state.redo().expect("the apply redoes");
    assert_eq!(entities(&state, id), after);
}

#[test]
fn a_preset_of_a_newer_version_is_refused_by_name() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let newer = serde_json::json!({
        "version": PRESET_VERSION + 1,
        "name": "from the future",
        "type_name": "CEntityDef",
        "fields": [],
    });
    std::fs::write(dir.path().join("future.json"), newer.to_string()).unwrap();

    let library = PresetLibrary::load(dir.path().to_path_buf());
    assert!(library.list.is_empty(), "a newer preset is not offered");
    assert_eq!(library.refused.len(), 1);
    let why = &library.refused[0];
    assert!(why.contains("from the future"), "{why}");
    assert!(why.contains(&(PRESET_VERSION + 1).to_string()), "{why}");
}
