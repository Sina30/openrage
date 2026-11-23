//! Every edit undoes in the document it was made in: undone, the document is what it opened as, and another is untouched.

use std::sync::Arc;

use egui::epaint::ClippedShape;
use or_io::container::resource::Rsc7;
use or_io::desc::ScalarValue;
use or_io::meta::{MetaInstance, Value};

use super::nested_archives_survive_a_save::save;
use super::presets::MetaStep;
use super::{
    AppState, Change, DocPayload, Document, DocumentId, EditHistory, MetaDocument, MetaEdit,
    ResourceImage,
};

/// A Legacy drawable whose resource image is all zeros, at `path`.
pub(super) fn drawable(state: &mut AppState, path: &str) -> DocumentId {
    let format = or_io::gta5::formats::format("ydr").expect(".ydr is declared");
    let version = or_io::container::resource::ResourceType::Ydr
        .read_versions()
        .into_iter()
        .find(|v| format.target_of_version(*v) == Ok(or_io::Generation::Gen8))
        .expect("a Legacy .ydr version");
    let seal = Rsc7::invent_single_chunk_seal(version, 0x4000).expect("a seal");
    let data = vec![0; seal.total_size() as usize];
    let image = ResourceImage {
        virtual_size: data.len(),
        data,
        seal,
    };
    let id = state.alloc_doc_id();
    let payload = DocPayload::Resource(or_io::Asset::Drawable(Default::default()));
    let doc = Document::new(id, Some(path.into()), payload).with_resource(Some(Arc::new(image)));
    state.documents.push(doc);
    state.active_document = Some(id);
    id
}

/// The `n`th field of `doc`'s described grid that takes a float, with its label.
pub(super) fn float_field(
    state: &AppState,
    doc: DocumentId,
    n: usize,
) -> (String, or_editor::Edit) {
    let doc = state.document(doc).expect("open");
    let (pack, root) = super::described_root(doc).expect("a drawable is described");
    let image = doc.resource.as_ref().expect("an image");
    let summary = or_editor::Summary::of(&pack, &root, image.view(), 0).expect("a summary");
    summary
        .rows
        .iter()
        .filter_map(|row| Some((row.label.clone(), row.edit?)))
        .filter(|(_, edit)| matches!(edit.kind, or_editor::EditKind::F32))
        .nth(n)
        .expect("the grid offers that many floats")
}

fn image(state: &AppState, doc: DocumentId) -> Vec<u8> {
    let doc = state.document(doc).expect("open");
    doc.resource.as_ref().expect("an image").data.clone()
}

fn history(state: &AppState, doc: DocumentId) -> &EditHistory {
    &state.document(doc).expect("open").history
}

fn dirty(state: &AppState, doc: DocumentId) -> bool {
    state.document(doc).expect("open").dirty
}

#[test]
fn a_grid_edit_undoes_to_the_bytes_the_file_opened_with() {
    let mut state = AppState::new();
    state.documents.clear();
    let id = drawable(&mut state, "probe.ydr");
    let opened = image(&state, id);
    let (label, edit) = float_field(&state, id, 0);

    assert!(state.patch_scalar(id, &label, edit, ScalarValue::F32(137.5)));
    assert_ne!(image(&state, id), opened, "the edit wrote nothing");
    assert!(dirty(&state, id));

    assert_eq!(state.undo(), Some(format!("Set {label}")));
    assert_eq!(
        image(&state, id),
        opened,
        "undone, the image is not as opened"
    );
    let doc = state.document(id).expect("open");
    assert!(
        !doc.dirty && !doc.patched(),
        "undone to as it opened, it has nothing to save"
    );

    state.redo();
    assert_ne!(image(&state, id), opened, "redone, the edit is gone");
    assert!(dirty(&state, id));
}

#[test]
fn undoing_in_one_document_leaves_another_untouched() {
    let mut state = AppState::new();
    state.documents.clear();
    let one = drawable(&mut state, "one.ydr");
    let other = drawable(&mut state, "other.ydr");
    let opened = image(&state, one);
    let (label, edit) = float_field(&state, one, 0);
    assert!(state.patch_scalar(one, &label, edit, ScalarValue::F32(1.0)));
    assert!(state.patch_scalar(other, &label, edit, ScalarValue::F32(2.0)));
    let edited = image(&state, other);

    state.set_active_document(one);
    assert!(state.undo().is_some());
    assert_eq!(image(&state, one), opened);
    assert_eq!(
        image(&state, other),
        edited,
        "the undo reached the other document"
    );
    assert!(dirty(&state, other) && history(&state, other).can_undo());

    assert!(
        state.undo().is_none(),
        "the active document has nothing left to undo"
    );
    assert_eq!(image(&state, other), edited);
}

#[test]
fn a_save_keeps_the_history_and_undoing_past_it_marks_the_document_unsaved() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("probe.ydr");
    let mut state = AppState::new();
    state.documents.clear();
    let id = drawable(&mut state, &path.to_string_lossy());
    let (label, edit) = float_field(&state, id, 0);
    assert!(state.patch_scalar(id, &label, edit, ScalarValue::F32(3.0)));

    save(&mut state, id);
    assert!(
        path.is_file() && !dirty(&state, id),
        "the save did not write it"
    );
    assert!(
        history(&state, id).can_undo(),
        "the save emptied the history"
    );

    state.undo();
    assert!(
        dirty(&state, id),
        "undone past the save, it no longer matches its file"
    );
    state.redo();
    assert!(
        !dirty(&state, id),
        "redone to the save, it matches its file again"
    );
}

#[test]
fn a_drag_is_one_step_until_the_pointer_lets_go() {
    let bytes = |at| Change::Bytes {
        at,
        other: vec![0; 4],
    };
    let mut history = EditHistory::default();
    for _ in 0..3 {
        history.push("Set radius".into(), bytes(8), None);
    }
    assert_eq!(history.done().len(), 1, "one drag made several steps");
    history.push("Set height".into(), bytes(12), None);
    assert_eq!(
        history.done().len(),
        2,
        "another field folded into the drag"
    );

    history.seal();
    history.push("Set height".into(), bytes(12), None);
    assert_eq!(
        history.done().len(),
        3,
        "a second drag folded into the first"
    );

    history.saved_at(history.at());
    history.push("Set height".into(), bytes(12), None);
    assert_eq!(
        history.done().len(),
        4,
        "an edit folded into the step the file holds"
    );
}

#[test]
fn a_click_in_the_history_panel_goes_back_or_forward_to_that_step() {
    let mut state = AppState::new();
    state.documents.clear();
    let id = drawable(&mut state, "probe.ydr");
    let opened = image(&state, id);
    for n in 0..3 {
        let (_, edit) = float_field(&state, id, n);
        let field = format!("field {n}");
        assert!(state.patch_scalar(id, &field, edit, ScalarValue::F32(1.0)));
    }
    let after = |state: &AppState| history(state, id).done().len();

    let mut screen = Screen::new(crate::ui::panels::history::draw);
    screen.click(&mut state, "Set field 0");
    assert_eq!(
        after(&state),
        1,
        "the click did not go back to the first step"
    );
    screen.click(&mut state, "Opened probe.ydr");
    assert_eq!(after(&state), 0);
    assert_eq!(image(&state, id), opened);
    screen.click(&mut state, "Set field 2");
    assert_eq!(
        after(&state),
        3,
        "the click did not go forward to the last step"
    );
}

/// Whether the text a frame painted as `text` is set in italics, when anything painted it.
fn italic(shapes: &[ClippedShape], text: &str) -> Option<bool> {
    fn walk(shape: &egui::Shape, text: &str) -> Option<bool> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(
                t.galley
                    .job
                    .sections
                    .iter()
                    .all(|section| section.format.italics),
            ),
            egui::Shape::Vec(inner) => inner.iter().find_map(|s| walk(s, text)),
            _ => None,
        }
    }
    shapes.iter().find_map(|c| walk(&c.shape, text))
}

#[test]
fn an_undone_step_in_the_history_panel_is_in_italics_as_well_as_dim() {
    let mut state = AppState::new();
    state.documents.clear();
    let id = drawable(&mut state, "probe.ydr");
    let (label, edit) = float_field(&state, id, 0);
    assert!(state.patch_scalar(id, &label, edit, ScalarValue::F32(1.0)));
    let step = format!("Set {label}");
    let mut screen = Screen::new(crate::ui::panels::history::draw);

    let shapes = screen.frame(&mut state, Vec::new());
    assert_eq!(
        italic(&shapes, &step),
        Some(false),
        "a step done is upright"
    );

    assert!(state.undo().is_some());
    let shapes = screen.frame(&mut state, Vec::new());
    assert_eq!(
        italic(&shapes, &step),
        Some(true),
        "a step undone differs from one done by its colour alone"
    );
    assert_eq!(italic(&shapes, "Opened probe.ydr"), Some(false));
}

/// One panel drawn frame by frame, and what a user clicks and types into it.
struct Screen {
    ctx: egui::Context,
    time: f64,
    draw: fn(&mut egui::Ui, &mut AppState),
}

impl Screen {
    fn new(draw: fn(&mut egui::Ui, &mut AppState)) -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
            draw,
        }
    }

    fn frame(&mut self, state: &mut AppState, events: Vec<egui::Event>) -> Vec<ClippedShape> {
        self.frame_with(state, events, egui::Modifiers::NONE)
    }

    fn frame_with(
        &mut self,
        state: &mut AppState,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> Vec<ClippedShape> {
        // ⚠ A press held past egui's click duration is a drag, not a click.
        self.time += 0.1;
        let input = egui::RawInput {
            events,
            modifiers,
            time: Some(self.time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            ..Default::default()
        };
        self.ctx.run_ui(input, |ui| (self.draw)(ui, state)).shapes
    }

    /// A click where the next frame paints `text`.
    fn click(&mut self, state: &mut AppState, text: &str) {
        let shapes = self.frame(state, Vec::new());
        let at = painted(&shapes, text).unwrap_or_else(|| panic!("nothing paints `{text}`"));
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.frame(state, vec![egui::Event::PointerMoved(at), press(true)]);
        self.frame(state, vec![press(false)]);
    }

    /// `key` pressed with Ctrl held.
    fn ctrl(&mut self, state: &mut AppState, key: egui::Key) {
        let ctrl = crate::ui::shortcuts::PRIMARY_EGUI;
        let press = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: ctrl,
        };
        self.frame_with(state, vec![press(true), press(false)], ctrl);
    }

    /// `text` typed a character a frame.
    fn type_text(&mut self, state: &mut AppState, text: &str) {
        for c in text.chars() {
            self.frame(state, vec![egui::Event::Text(c.to_string())]);
        }
    }
}

/// Where egui painted `text`.
fn painted(shapes: &[ClippedShape], text: &str) -> Option<egui::Pos2> {
    fn walk(shape: &egui::Shape, text: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => {
                Some(t.galley.rect.translate(t.pos.to_vec2()).center())
            }
            egui::Shape::Vec(inner) => inner.iter().find_map(|s| walk(s, text)),
            _ => None,
        }
    }
    shapes.iter().find_map(|c| walk(&c.shape, text))
}

/// A metadata document holding `inst`, active.
fn metadata(state: &mut AppState, inst: MetaInstance) -> DocumentId {
    let mut meta = MetaDocument::empty();
    if let Some(parsed) = meta.parsed.as_mut() {
        parsed.instance = inst;
    }
    let id = state.alloc_doc_id();
    let doc = Document::new(id, Some("probe.meta".into()), DocPayload::Meta(meta));
    state.documents.push(doc);
    state.active_document = Some(id);
    id
}

fn instance(state: &AppState, doc: DocumentId) -> MetaInstance {
    match &state.document(doc).expect("open").payload {
        DocPayload::Meta(meta) => meta.parsed.as_ref().expect("parsed").instance.clone(),
        _ => panic!("not metadata"),
    }
}

#[test]
fn metadata_fields_set_through_the_form_undo_to_the_instance_it_opened_as() {
    let mut inst = MetaInstance::new("OpenRageUndoProbe");
    inst.set("name", Value::String("hello".into()));
    inst.set("list", Value::Array(vec![Value::UInt(1)]));
    let mut state = AppState::new();
    state.documents.clear();
    let id = metadata(&mut state, inst.clone());

    let mut screen = Screen::new(crate::ui::panels::meta_editor::draw);
    screen.click(&mut state, &format!("{} Add", egui_phosphor::regular::PLUS));
    screen.click(&mut state, "hello");
    screen.type_text(&mut state, "ab");

    let edited = instance(&state, id);
    assert_ne!(edited, inst, "the form changed nothing");
    let steps: Vec<&str> = (history(&state, id).done().iter())
        .map(|step| step.label.as_str())
        .collect();
    assert_eq!(steps, ["Add to list", "Set name"], "typing is one step");

    while state.undo().is_some() {}
    assert_eq!(
        instance(&state, id),
        inst,
        "undone, it is not what it opened as"
    );
    assert!(!dirty(&state, id));
    while state.redo().is_some() {}
    assert_eq!(instance(&state, id), edited);
}

#[test]
fn every_kind_of_metadata_edit_undoes_and_redoes_exactly() {
    let mut item = MetaInstance::new("CItem");
    item.set("flag", Value::Bool(false));
    let mut inst = MetaInstance::new("CRoot");
    inst.set(
        "items",
        Value::Array(vec![Value::UInt(1), Value::UInt(2), Value::UInt(3)]),
    );
    inst.set("item", Value::Pointer(Some(Box::new(item))));
    // ⚠ A document read without a schema repeats a name; an edit of the second must not land in the first.
    inst.fields.push(("Item".into(), Value::Int(1)));
    inst.fields.push(("Item".into(), Value::Int(2)));
    let mut state = AppState::new();
    state.documents.clear();
    let id = metadata(&mut state, inst.clone());

    let items = || vec![MetaStep::Field("items".into())];
    let mut edits = Vec::new();
    let mut changed = inst.clone();
    if let Some(Value::Array(list)) = changed.get_mut("items") {
        let gone = list.remove(0);
        edits.push(MetaEdit::Element {
            path: items(),
            at: 0,
            other: Some(gone),
        });
        list.swap(0, 1);
        edits.push(MetaEdit::Swap {
            path: items(),
            a: 0,
            b: 1,
        });
    }
    let at = changed.fields.len();
    changed.fields.push(("added".into(), Value::Float(0.5)));
    edits.push(MetaEdit::Field {
        path: Vec::new(),
        name: "added".into(),
        at,
        other: None,
    });
    let before = std::mem::replace(
        changed.get_mut("item").expect("the pointer"),
        Value::Pointer(None),
    );
    edits.push(MetaEdit::Set {
        path: vec![MetaStep::Field("item".into())],
        other: before,
    });
    changed.fields[3].1 = Value::Int(20);
    edits.push(MetaEdit::Set {
        path: vec![MetaStep::Nth("Item".into(), 1)],
        other: Value::Int(2),
    });

    if let Some(DocPayload::Meta(meta)) = state.document_mut(id).map(|d| &mut d.payload) {
        if let Some(parsed) = meta.parsed.as_mut() {
            parsed.instance = changed.clone();
        }
    }
    state.record(id, "several", Change::Meta(edits));

    assert!(state.undo().is_some());
    assert_eq!(
        instance(&state, id),
        inst,
        "undone, it is not what it opened as"
    );
    assert!(state.redo().is_some());
    assert_eq!(instance(&state, id), changed, "redone, it is not as edited");
}

#[test]
fn a_text_table_entry_typed_into_undoes_to_the_text_it_opened_with() {
    let mut table = or_io::gta5::text::Gxt2::default();
    for (hash, value) in [(0x1234, "hello"), (0x5678, "world")] {
        let value = value.to_string();
        table
            .entries
            .push(or_io::gta5::text::Gxt2Entry { hash, value });
    }
    let mut state = AppState::new();
    state.documents.clear();
    let id = state.alloc_doc_id();
    let payload = DocPayload::Gxt2(table.clone());
    state
        .documents
        .push(Document::new(id, Some("probe.gxt2".into()), payload));
    state.active_document = Some(id);
    let entries = |state: &AppState| match &state.document(id).expect("open").payload {
        DocPayload::Gxt2(t) => t.entries.clone(),
        _ => panic!("not a text table"),
    };

    let mut screen = Screen::new(crate::ui::panels::gxt2_table::draw);
    screen.click(&mut state, "hello");
    screen.type_text(&mut state, "ab");
    let edited = entries(&state);
    assert_ne!(edited, table.entries, "typing changed nothing");
    assert_eq!(history(&state, id).done().len(), 1, "typing is one step");

    assert!(state.undo().is_some());
    assert_eq!(
        entries(&state),
        table.entries,
        "undone, it is not what it opened as"
    );
    assert!(!dirty(&state, id));
    state.redo();
    assert_eq!(entries(&state), edited);
}

#[test]
fn a_particle_rule_set_through_the_inspector_undoes_to_the_rule_it_opened_with() {
    let mut fx = or_io::gta5::PtxFxList::default();
    or_particle::list::add_emitter_rule(&mut fx, "em");
    let mut state = AppState::new();
    state.documents.clear();
    let id = state.alloc_doc_id();
    let payload = DocPayload::Resource(or_io::Asset::Particles(fx));
    state
        .documents
        .push(Document::new(id, Some("fx.ypt".into()), payload));
    state.set_active_document(id);
    let rule = crate::ui::panels::selection_ids::ypt_emitter_lib_id(0);
    state.selection.select(id, rule);
    let shown = |state: &AppState| match &state.document(id).expect("open").payload {
        DocPayload::Resource(or_io::Asset::Particles(fx)) => format!("{fx:?}"),
        _ => panic!("not a particle list"),
    };
    let opened = shown(&state);

    let mut screen = Screen::new(crate::ui::panels::inspector::draw);
    screen.click(&mut state, "is_one_shot");
    let edited = shown(&state);
    assert_ne!(edited, opened, "the click changed nothing");

    assert_eq!(state.undo().as_deref(), Some("Edit em"));
    assert_eq!(shown(&state), opened, "undone, it is not as it opened");
    assert!(!dirty(&state, id));
    state.redo();
    assert_eq!(shown(&state), edited);
}

#[test]
fn an_archives_adds_renames_moves_and_deletes_undo_to_the_archive_it_opened_as() {
    use super::nested_archives_survive_a_save::archive;
    use super::opening_a_ydd::node_at;

    let dir = tempfile::tempdir().expect("a scratch directory");
    let root = dir.path().join("mods");
    std::fs::create_dir_all(&root).expect("the folder");
    let x = archive(&[("a.txt", b"in x"), ("sub/b.txt", b"in sub")]);
    std::fs::write(root.join("x.rpf"), &x).expect("x.rpf");
    let imported = dir.path().join("c.txt");
    std::fs::write(&imported, b"imported").expect("a file to import");
    let mut state = AppState::new();
    state.open_folder(root.clone());
    let doc = state.active_document.expect("the folder's tree");
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    let sub = node_at(&mut state, doc, "x.rpf/sub");
    let x_row = node_at(&mut state, doc, "x.rpf");
    let b = node_at(&mut state, doc, "x.rpf/sub/b.txt");
    let id = match &state.document(doc).expect("open").payload {
        DocPayload::Rpf(rpf) => rpf.session.archive_on_disk_around(a).expect("x.rpf").0,
        _ => panic!("not an archive tree"),
    };
    let written = |state: &AppState| {
        state
            .archive_bytes(doc, id, None)
            .expect("x.rpf rebuilds")
            .0
    };
    let opened = written(&state);

    state.rpf_add_folder(doc, x_row, "added").expect("a folder");
    state
        .rpf_rename_node(doc, a, "renamed.txt")
        .expect("a rename");
    state.rpf_move_node(doc, a, sub).expect("a move");
    state
        .rpf_import_file_to_folder(doc, x_row, imported)
        .expect("an import");
    state.rpf_delete_node(doc, b).expect("the delete asks");
    state.answer_delete(true);
    let edited = written(&state);
    assert_ne!(edited, opened, "the edits changed nothing");
    assert!(dirty(&state, doc));

    state.set_active_document(doc);
    let mut undone = Vec::new();
    while let Some(label) = state.undo() {
        undone.push(label);
    }
    assert_eq!(undone.len(), 5, "not every edit is a step: {undone:?}");
    assert_eq!(written(&state), opened, "undone, it writes other bytes");
    let unwritten = |state: &AppState| match &state.document(doc).expect("open").payload {
        DocPayload::Rpf(rpf) => rpf.session.dirty_archive_ids(),
        _ => Vec::new(),
    };
    assert!(
        !dirty(&state, doc) && unwritten(&state).is_empty(),
        "undone to as it opened, it has nothing to save"
    );
    while state.redo().is_some() {}
    assert_eq!(written(&state), edited, "redone, an edit is lost");
    assert!(dirty(&state, doc) && unwritten(&state) == [id]);
}

/// ⚠ A row taken out reads its archive's entries by index, and the write renumbers them.
#[test]
fn a_saved_archive_keeps_its_renames_and_forgets_rows_the_write_would_leave_stale() {
    use super::nested_archives_survive_a_save::archive;
    use super::opening_a_ydd::node_at;

    let dir = tempfile::tempdir().expect("a scratch directory");
    let root = dir.path().join("mods");
    std::fs::create_dir_all(&root).expect("the folder");
    let x = archive(&[("a.txt", b"in x"), ("b.txt", b"also in x")]);
    std::fs::write(root.join("x.rpf"), &x).expect("x.rpf");
    let mut state = AppState::new();
    state.open_folder(root.clone());
    let doc = state.active_document.expect("the folder's tree");
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    let b = node_at(&mut state, doc, "x.rpf/b.txt");

    state
        .rpf_rename_node(doc, a, "renamed.txt")
        .expect("a rename");
    save(&mut state, doc);
    assert!(!dirty(&state, doc), "the save did not write x.rpf");
    assert!(state.undo().is_some(), "the save emptied the history");
    assert!(
        dirty(&state, doc),
        "undone past the save, it no longer matches its file"
    );
    assert!(state.redo().is_some());
    assert!(
        !dirty(&state, doc),
        "redone to the save, it matches its file again"
    );

    state.rpf_delete_node(doc, b).expect("the delete asks");
    state.answer_delete(true);
    save(&mut state, doc);
    assert!(
        state.undo().is_none(),
        "a row the write renumbered can still be put back"
    );
}

#[test]
fn a_drop_of_files_and_folders_into_an_archive_undoes_as_one_step() {
    use super::nested_archives_survive_a_save::archive;
    use super::opening_a_ydd::node_at;

    let dir = tempfile::tempdir().expect("a scratch directory");
    let root = dir.path().join("mods");
    std::fs::create_dir_all(&root).expect("the folder");
    std::fs::write(root.join("x.rpf"), archive(&[("a.txt", b"in x")])).expect("x.rpf");
    let dropped = dir.path().join("dropped");
    std::fs::create_dir_all(dropped.join("inner")).expect("a folder to drop");
    std::fs::write(dropped.join("one.txt"), b"one").expect("a file");
    std::fs::write(dropped.join("inner/two.txt"), b"two").expect("a file");
    let loose = dir.path().join("three.txt");
    std::fs::write(&loose, b"three").expect("a file");
    let mut state = AppState::new();
    state.open_folder(root.clone());
    let doc = state.active_document.expect("the folder's tree");
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    let x_row = node_at(&mut state, doc, "x.rpf");
    let id = match &state.document(doc).expect("open").payload {
        DocPayload::Rpf(rpf) => rpf.session.archive_on_disk_around(a).expect("x.rpf").0,
        _ => panic!("not an archive tree"),
    };
    let written = |state: &AppState| {
        state
            .archive_bytes(doc, id, None)
            .expect("x.rpf rebuilds")
            .0
    };
    let opened = written(&state);

    state.import_paths_into_folder(doc, x_row, vec![dropped, loose]);
    let labels: Vec<&str> = (history(&state, doc).done().iter())
        .map(|step| step.label.as_str())
        .collect();
    assert_eq!(labels, ["Add 3 file(s)"], "a drop is not one step");

    state.set_active_document(doc);
    assert!(state.undo().is_some());
    assert_eq!(written(&state), opened, "undone, it writes other bytes");
    assert!(!dirty(&state, doc));
}

/// ⚠ The text editor keeps its own undo, which `Ctrl+Z` reaches while it has the keyboard.
#[test]
fn typing_undoes_in_the_text_editor_and_the_documents_history_holds_none_of_it() {
    let mut state = AppState::new();
    state.documents.clear();
    let id = state.alloc_doc_id();
    let payload = DocPayload::Text(super::TextDocument::decode(b"hello world"));
    state
        .documents
        .push(Document::new(id, Some("probe.txt".into()), payload));
    state.set_active_document(id);
    let text = |state: &AppState| match &state.document(id).expect("open").payload {
        DocPayload::Text(t) => t.text.to_string(),
        _ => panic!("not text"),
    };

    let mut screen = Screen::new(crate::ui::panels::text_editor::draw);
    screen.click(&mut state, "hello world");
    screen.type_text(&mut state, "ab");
    assert_ne!(text(&state), "hello world", "typing changed nothing");
    assert!(
        !history(&state, id).can_undo(),
        "the history holds a copy of the typing"
    );

    screen.ctrl(&mut state, egui::Key::Z);
    assert_eq!(
        text(&state),
        "hello world",
        "Ctrl+Z did not undo the typing"
    );
}
