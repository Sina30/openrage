//! A metadata record copied from one file and pasted after the selection in another.

use std::path::PathBuf;

use or_io::meta::{MetaInstance, Value};

use super::*;
use crate::state::presets::MetaStep;
use crate::state::{Document, MetaDocument, Prefs};

fn record(type_name: &str, name: &str) -> Value {
    let mut record = MetaInstance::new(type_name);
    record.set("archetypeName", Value::String(name.into()));
    Value::Struct(Box::new(record))
}

/// A metadata file of `root`, whose `array` holds `records`, open as `id`.
fn open(state: &mut AppState, id: DocumentId, root: &str, array: &str, records: Vec<Value>) {
    let mut instance = MetaInstance::new(root);
    instance.set(array, Value::Array(records));
    let mut meta = MetaDocument::empty();
    meta.parsed
        .as_mut()
        .expect("an empty document parses")
        .instance = instance;
    let path = PathBuf::from(format!("file{id}.ymap"));
    state
        .documents
        .push(Document::new(id, Some(path), DocPayload::Meta(meta)));
}

/// The names of what `array` of `doc` holds, in order.
fn names(state: &AppState, doc: DocumentId, array: &str) -> Vec<String> {
    let Some(DocPayload::Meta(meta)) = state.document(doc).map(|d| &d.payload) else {
        panic!("{doc} is a metadata document");
    };
    let Some(Value::Array(items)) = meta.parsed.as_ref().and_then(|p| p.instance.get(array)) else {
        panic!("{array} is an array");
    };
    (items.iter())
        .filter_map(|item| match item {
            Value::Struct(record) => match record.get("archetypeName") {
                Some(Value::String(name)) => Some(name.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn path(array: &str) -> Vec<MetaStep> {
    vec![MetaStep::Field(array.into())]
}

/// Two maps and an interior's rooms: two entities go from one map to the
/// other, after its first, and are refused by the rooms.
fn three_files() -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    let entities = |names: &[&str]| names.iter().map(|n| record("CEntityDef", n)).collect();
    open(
        &mut state,
        101,
        "CMapData",
        "entities",
        entities(&["a0", "a1", "a2"]),
    );
    open(
        &mut state,
        102,
        "CMapData",
        "entities",
        entities(&["b0", "b1"]),
    );
    let rooms = vec![record("CMloRoomDef", "hall")];
    open(&mut state, 103, "CMloArchetypeDef", "rooms", rooms);
    state
}

/// Click the widget labelled `label` in a frame of the metadata form.
fn click_in_form(ctx: &egui::Context, state: &mut AppState, label: &str) {
    let frame = |state: &mut AppState, events| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 1000.0),
            )),
            events,
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| crate::ui::panels::meta_editor::draw(ui, state));
        output
            .platform_output
            .accesskit_update
            .map_or(Vec::new(), |update| {
                (update.nodes.iter())
                    .filter_map(|(_, node)| {
                        let b = node.bounds()?;
                        let at =
                            egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
                        Some((node.label()?.to_string(), at))
                    })
                    .collect::<Vec<_>>()
            })
    };
    frame(state, Vec::new());
    let shown = frame(state, Vec::new());
    let at = (shown.iter().find(|(text, _)| text == label))
        .map(|(_, at)| *at)
        .unwrap_or_else(|| panic!("no `{label}` in {shown:?}"));
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(state, vec![egui::Event::PointerMoved(at), press(true)]);
    frame(state, vec![press(false)]);
}

/// ⚠ The form's own Copy, then Edit's Paste and Delete as their keys run them.
#[test]
fn the_forms_copy_and_edits_paste_and_delete_reach_the_ticked_records() {
    use crate::ui::commands::{self, Action};
    let mut state = three_files();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    state.set_active_document(101);
    let ws = state.active_workspace;
    let centre = crate::ui::panels::registry::ids::DOCUMENT;
    crate::ui::dock_commands::reveal(state.workspace_docks.get_mut(ws), centre);
    state.records_selected.set(101, &path("entities"), [0]);
    click_in_form(&ctx, &mut state, "Copy");
    assert!(state.records_to_paste(), "the form's Copy held the record");

    state.set_active_document(102);
    state.records_selected.set(102, &path("entities"), [1]);
    commands::run_action(&mut state, &ctx, Action::Paste);
    assert_eq!(names(&state, 102, "entities"), ["b0", "b1", "a0"]);
    commands::run_action(&mut state, &ctx, Action::Delete);
    assert_eq!(
        names(&state, 102, "entities"),
        ["b0", "b1"],
        "Delete takes out the records the paste left ticked"
    );
}

#[test]
fn a_metadata_record_copied_from_one_file_pastes_into_another() {
    let mut state = three_files();
    let (a, b, c) = (101, 102, 103);
    state.records_selected.set(a, &path("entities"), [1, 2]);
    state.copy_records(a).expect("the copy");
    state.records_selected.set(b, &path("entities"), [0]);
    state.paste_records(b).expect("the paste");
    assert_eq!(
        names(&state, b, "entities"),
        ["b0", "a1", "a2", "b1"],
        "inserted after the selection"
    );
    assert_eq!(names(&state, a, "entities"), ["a0", "a1", "a2"]);

    state.set_active_document(b);
    assert_eq!(state.undo().as_deref(), Some("Paste 2 record(s)"));
    assert_eq!(
        names(&state, b, "entities"),
        ["b0", "b1"],
        "one Undo takes both out"
    );

    state.records_selected.set(c, &path("rooms"), [0]);
    let refused = state
        .paste_records(c)
        .expect_err("an entity does not go among rooms");
    assert!(
        refused.contains("CMloRoomDef") && refused.contains("CEntityDef"),
        "the refusal names both types: {refused}"
    );
    assert_eq!(names(&state, c, "rooms"), ["hall"]);
}
