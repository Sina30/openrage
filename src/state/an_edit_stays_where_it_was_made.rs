//! An edit stays where it was made: what the grid writes is held by the document's contents, so
//! every later save writes it too, and the grid writes into the document it describes.

use std::path::{Path, PathBuf};

use egui::epaint::ClippedShape;
use or_io::codec::texture::{ChannelView, RgbaImage};
use or_io::decl::Gen8;
use or_io::desc::ScalarValue;
use or_io::gta5::gen8::EntryData;
use or_io::gta5::{File as _, GtaDrawable, RmcDrawable};
use or_test_support::{skip_corpus, Corpus};

use super::nested_archives_survive_a_save::save;
use super::opening_a_ydd::finish_load;
use super::{drawable_mut, AppState, Change, DocPayload, Document, DocumentId, Game};
use crate::ui::panels::inspector::InspectorUiState;
use crate::ui::panels::selection_ids::DOC_ROOT_SEL;

/// `path` opened as Open File opens it.
pub(super) fn open(state: &mut AppState, path: &Path) -> DocumentId {
    state.load_file(path.to_path_buf());
    finish_load(state).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// What `path` holds now, read as a Legacy drawable.
fn on_disk(path: &Path) -> GtaDrawable {
    let bytes = std::fs::read(path).expect("saved");
    <GtaDrawable>::from_bytes(&bytes).expect("what was saved reads back")
}

/// Where the first float a material carries is: its shader and its entry.
fn material_value_at(drawable: &RmcDrawable<Gen8>) -> Option<(usize, usize)> {
    let shaders = &drawable.shader_group.as_deref()?.shaders;
    shaders.iter().enumerate().find_map(|(shader, data)| {
        let entry = (data.entries.iter())
            .position(|e| matches!(&e.data, EntryData::Float(rows) if !rows.is_empty()))?;
        Some((shader, entry))
    })
}

fn material_value(drawable: &RmcDrawable<Gen8>, (shader, entry): (usize, usize)) -> f32 {
    let shaders = &drawable.shader_group.as_deref().expect("materials").shaders;
    match &shaders[shader].entries[entry].data {
        EntryData::Float(rows) => rows[0].x,
        EntryData::Texture(_) => panic!("the entry holds a texture"),
    }
}

/// The smallest staged Legacy drawable whose materials carry a float, copied where a save may write it.
pub(super) fn drawable_with_a_material(dir: &Path) -> Option<(PathBuf, (usize, usize))> {
    let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr").find();
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    files.into_iter().find_map(|source| {
        let drawable = <GtaDrawable>::from_bytes(&std::fs::read(&source).ok()?).ok()?;
        let at = material_value_at(&drawable.base)?;
        let copy = dir.join(source.file_name()?);
        std::fs::copy(&source, &copy).ok()?;
        Some((copy, at))
    })
}

/// The row of the drawable `doc`'s grid whose label ends with `ending`: its label and its edit.
pub(super) fn grid_row(
    state: &AppState,
    doc: DocumentId,
    ending: &str,
) -> (String, or_editor::Edit) {
    let doc = state.document(doc).expect("open");
    let (pack, root) = super::described_root(doc).expect("a drawable is described");
    let image = doc.resource.as_ref().expect("the image it was read from");
    let summary = or_editor::Summary::of(&pack, &root, image.view(), 0).expect("a summary");
    let row = (summary.rows.iter())
        .find(|r| r.label.ends_with(ending))
        .unwrap_or_else(|| panic!("the grid shows no `…{ending}`"));
    (row.label.clone(), row.edit.expect("an editable row"))
}

fn image(state: &AppState, doc: DocumentId) -> Vec<u8> {
    let doc = state.document(doc).expect("open");
    doc.resource.as_ref().expect("an image").data.clone()
}

/// Add 1.5 to the material value at `at`, as the material editor's Set does, and return what it holds now.
pub(super) fn edit_a_material(
    state: &mut AppState,
    doc: DocumentId,
    (shader, entry): (usize, usize),
) -> f32 {
    let document = state.document_mut(doc).expect("open");
    let shaders = &mut drawable_mut(&mut document.payload, 0)
        .and_then(|d| d.shader_group.as_deref_mut())
        .expect("materials")
        .shaders;
    let before = shaders[shader].clone();
    let EntryData::Float(rows) = &mut shaders[shader].entries[entry].data else {
        panic!("the entry holds a texture");
    };
    rows[0].x += 1.5;
    let now = rows[0].x;
    let change = Change::part(format!("material 0 {shader}"), before, move |p| {
        (drawable_mut(p, 0)?.shader_group.as_deref_mut()?.shaders).get_mut(shader)
    });
    state.record(doc, "Set a material value", change);
    now
}

/// A staged drawable opened from a scratch copy, its first switch distance set in the grid.
struct Edited {
    _dir: tempfile::TempDir,
    path: PathBuf,
    state: AppState,
    doc: DocumentId,
    material: (usize, usize),
    opened: f32,
    distance: f32,
}

fn edited(what: &str) -> Option<Edited> {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let Some((path, material)) = drawable_with_a_material(dir.path()) else {
        skip_corpus(what, "no staged Legacy .ydr carries a material value");
        return None;
    };
    let mut state = AppState::new();
    state.documents.clear();
    let doc = open(&mut state, &path);
    let opened = on_disk(&path).base.lod_group.lod_distances[0];
    let distance = opened + 137.5;
    let (label, edit) = grid_row(&state, doc, "Distances [0]");
    assert!(state.patch_scalar(doc, &label, edit, ScalarValue::F32(distance)));
    Some(Edited {
        _dir: dir,
        path,
        state,
        doc,
        material,
        opened,
        distance,
    })
}

#[test]
fn a_grid_edit_survives_a_save_a_material_edit_and_a_second_save() {
    let Some(mut e) = edited("a_grid_edit_survives_a_save_a_material_edit_and_a_second_save")
    else {
        return;
    };
    save(&mut e.state, e.doc);
    let value = edit_a_material(&mut e.state, e.doc, e.material);
    save(&mut e.state, e.doc);

    let saved = on_disk(&e.path);
    assert_eq!(
        saved.base.lod_group.lod_distances[0], e.distance,
        "the second save dropped the grid's edit"
    );
    assert_eq!(material_value(&saved.base, e.material), value);
}

#[test]
fn a_material_edit_before_any_save_keeps_the_grid_edit_made_before_it() {
    let Some(mut e) = edited("a_material_edit_before_any_save_keeps_the_grid_edit_made_before_it")
    else {
        return;
    };
    let value = edit_a_material(&mut e.state, e.doc, e.material);
    save(&mut e.state, e.doc);

    let saved = on_disk(&e.path);
    assert_eq!(
        saved.base.lod_group.lod_distances[0], e.distance,
        "the material edit dropped the grid's edit"
    );
    assert_eq!(material_value(&saved.base, e.material), value);
}

#[test]
fn an_undone_grid_edit_is_not_written_by_a_later_save() {
    let Some(mut e) = edited("an_undone_grid_edit_is_not_written_by_a_later_save") else {
        return;
    };
    // The gesture ends before the undo, so nothing but the undo reads the image back.
    e.state.seal_edits();
    assert!(e.state.undo().is_some());
    let value = edit_a_material(&mut e.state, e.doc, e.material);
    save(&mut e.state, e.doc);

    let saved = on_disk(&e.path);
    assert_eq!(
        saved.base.lod_group.lod_distances[0], e.opened,
        "the save wrote the grid's undone edit"
    );
    assert_eq!(material_value(&saved.base, e.material), value);
}

#[test]
fn a_second_save_with_nothing_edited_between_keeps_the_grid_edit() {
    let Some(mut e) = edited("a_second_save_with_nothing_edited_between_keeps_the_grid_edit")
    else {
        return;
    };
    save(&mut e.state, e.doc);
    save(&mut e.state, e.doc);
    assert_eq!(
        on_disk(&e.path).base.lod_group.lod_distances[0],
        e.distance,
        "the second save dropped the grid's edit"
    );
}

#[test]
fn an_edit_no_step_holds_keeps_the_grid_edit_made_before_it() {
    let Some(mut e) = edited("an_edit_no_step_holds_keeps_the_grid_edit_made_before_it") else {
        return;
    };
    e.state.document_mut(e.doc).expect("open").mark_edited();
    save(&mut e.state, e.doc);
    assert_eq!(
        on_disk(&e.path).base.lod_group.lod_distances[0],
        e.distance,
        "the edit no step holds dropped the grid's edit"
    );
}

/// Two staged Enhanced drawables open, the inspector pinned to the first while the second is active.
struct Pinned {
    state: AppState,
    pinned: DocumentId,
    active: DocumentId,
    before: (Vec<u8>, Vec<u8>),
}

impl Pinned {
    fn open(what: &str) -> Option<Self> {
        let files = super::payload_handlers::enhanced_corpus("ydr");
        let [pinned_path, active_path, ..] = files.as_slice() else {
            skip_corpus(what, "fewer than two staged Enhanced .ydr");
            return None;
        };
        let mut state = AppState::new();
        state.documents.clear();
        // ⚠ Under Legacy, opening an Enhanced file asks to switch first.
        state.prefs.active_game = Some(Game::GtaVEnhanced);
        let pinned = open(&mut state, pinned_path);
        let active = open(&mut state, active_path);
        state.set_active_document(active);
        state.panels.get::<InspectorUiState>().pinned = Some((pinned, DOC_ROOT_SEL));
        let before = (image(&state, pinned), image(&state, active));
        Some(Self {
            state,
            pinned,
            active,
            before,
        })
    }

    fn only_the_pinned_document_changed(&self) {
        let active = self.state.document(self.active).expect("open");
        assert!(
            image(&self.state, self.active) == self.before.1 && !active.dirty,
            "the edit landed in the active document"
        );
        assert!(
            image(&self.state, self.pinned) != self.before.0,
            "the edit did not reach the pinned document"
        );
    }
}

#[test]
fn the_grid_writes_into_the_document_the_inspector_is_pinned_to() {
    let Some(mut p) = Pinned::open("the_grid_writes_into_the_document_the_inspector_is_pinned_to")
    else {
        return;
    };
    let (label, _) = grid_row(&p.state, p.pinned, "Distances [0]");
    Inspector::default().drag(&mut p.state, &label, 40.0);
    p.only_the_pinned_document_changed();
}

#[test]
fn a_field_pasted_in_a_pinned_inspector_lands_in_the_pinned_document() {
    let Some(mut p) =
        Pinned::open("a_field_pasted_in_a_pinned_inspector_lands_in_the_pinned_document")
    else {
        return;
    };
    let (min, _) = grid_row(&p.state, p.pinned, "BoundsBox > Min > X");
    let (max, _) = grid_row(&p.state, p.pinned, "BoundsBox > Max > Y");
    let (right, left) = (egui::PointerButton::Secondary, egui::PointerButton::Primary);
    let mut inspector = Inspector::default();
    inspector.click(&mut p.state, &min, right);
    inspector.click(&mut p.state, "Copy Min", left);
    inspector.click(&mut p.state, &max, right);
    inspector.click(&mut p.state, "Paste into Max", left);
    p.only_the_pinned_document_changed();
}

/// The first switch distance of the Enhanced drawable `doc`: what its payload holds, and what its image holds at `edit`.
fn distances(state: &AppState, doc: DocumentId, edit: or_editor::Edit) -> (f32, f32) {
    let doc = state.document(doc).expect("open");
    let DocPayload::Resource(or_io::Asset::DrawableGen9(drawable)) = &doc.payload else {
        panic!("not an Enhanced drawable");
    };
    let image = &doc.resource.as_ref().expect("an image").data;
    let bytes = image[edit.offset..edit.offset + 4].try_into();
    let held = drawable.base.lod_group.lod_distances[0];
    (held, f32::from_le_bytes(bytes.expect("four bytes")))
}

#[test]
fn a_drag_of_many_frames_reads_the_contents_back_once() {
    let Some(mut p) = Pinned::open("a_drag_of_many_frames_reads_the_contents_back_once") else {
        return;
    };
    let (label, edit) = grid_row(&p.state, p.pinned, "Distances [0]");
    let preview = (p.pinned, "probe".to_string(), ChannelView::Rgba);
    let pixel = RgbaImage {
        width: 1,
        height: 1,
        data: vec![0; 4],
        is_srgb: false,
    };
    let (ctx, linear) = (egui::Context::default(), egui::TextureOptions::LINEAR);
    (p.state.texture_cache).get_or_insert_image(&ctx, preview.clone(), linear, || Some(pixel));
    let reads = Document::reads_on_this_thread();
    let mut inspector = Inspector::default();
    let mut at = inspector.press(&mut p.state, &label);
    let mut written = Vec::new();
    for _ in 0..12 {
        at += egui::vec2(10.0, 0.0);
        inspector.move_to(&mut p.state, at);
        written.push(distances(&p.state, p.pinned, edit).1);
    }
    inspector.release(&mut p.state, at);
    written.dedup();
    assert!(
        written.len() >= 10,
        "the drag wrote the image on too few of its frames: {written:?}"
    );
    assert_eq!(
        Document::reads_on_this_thread(),
        reads,
        "a frame of the drag read the payload back"
    );

    // As the shell does once the pointer is up and nothing holds the keyboard, and on the next frame.
    p.state.seal_edits();
    p.state.seal_edits();
    assert_eq!(Document::reads_on_this_thread(), reads + 1);
    let (held, written) = distances(&p.state, p.pinned, edit);
    assert!(
        held == written,
        "the payload holds {held}, the image {written}"
    );
    assert!(
        p.state.texture_cache.get(&preview).is_none(),
        "a preview drawn from the payload before the read outlived it"
    );
}

/// The inspector drawn frame by frame, and what a user clicks and drags in it.
#[derive(Default)]
pub(super) struct Inspector {
    ctx: egui::Context,
    time: f64,
}

impl Inspector {
    pub(super) fn frame(
        &mut self,
        state: &mut AppState,
        events: Vec<egui::Event>,
    ) -> Vec<ClippedShape> {
        // ⚠ A press held past egui's click duration is a drag, not a click.
        self.time += 0.1;
        let input = egui::RawInput {
            events,
            time: Some(self.time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 4000.0),
            )),
            ..Default::default()
        };
        let draw = |ui: &mut egui::Ui| crate::ui::panels::inspector::draw(ui, state);
        self.ctx.run_ui(input, draw).shapes
    }

    /// A drag of `by` points to the right across the value on the row labelled `label`.
    pub(super) fn drag(&mut self, state: &mut AppState, label: &str, by: f32) {
        let to = self.press(state, label) + egui::vec2(by, 0.0);
        self.move_to(state, to);
        self.release(state, to);
    }

    /// A press on the value on the row labelled `label`, and where.
    fn press(&mut self, state: &mut AppState, label: &str) -> egui::Pos2 {
        let shapes = self.frame(state, Vec::new());
        let at = value_beside(&shapes, label)
            .unwrap_or_else(|| panic!("the inspector shows no value beside `{label}`"));
        let primary = egui::PointerButton::Primary;
        let moved = egui::Event::PointerMoved(at);
        self.frame(state, vec![moved, pointer(at, primary, true)]);
        at
    }

    /// The pressed pointer moved to `at`, a frame.
    fn move_to(&mut self, state: &mut AppState, at: egui::Pos2) {
        self.frame(state, vec![egui::Event::PointerMoved(at)]);
    }

    fn release(&mut self, state: &mut AppState, at: egui::Pos2) {
        let primary = egui::PointerButton::Primary;
        self.frame(state, vec![pointer(at, primary, false)]);
    }

    /// A click of `button` where the inspector paints `text`.
    pub(super) fn click(&mut self, state: &mut AppState, text: &str, button: egui::PointerButton) {
        let shapes = self.frame(state, Vec::new());
        let at = (texts(&shapes).into_iter())
            .find(|(painted, _)| painted == text)
            .map(|(_, rect)| rect.center())
            .unwrap_or_else(|| panic!("the inspector paints no `{text}`"));
        let moved = egui::Event::PointerMoved(at);
        self.frame(state, vec![moved, pointer(at, button, true)]);
        self.frame(state, vec![pointer(at, button, false)]);
    }
}

fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Default::default(),
    }
}

/// Where egui painted the first text to the right of `label`, on its row.
fn value_beside(shapes: &[ClippedShape], label: &str) -> Option<egui::Pos2> {
    let texts = texts(shapes);
    let (_, at) = texts.iter().find(|(text, _)| text == label)?;
    (texts.iter())
        .filter(|(_, r)| r.min.x >= at.max.x && (r.center().y - at.center().y).abs() < 8.0)
        .min_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x))
        .map(|(_, r)| r.center())
}

/// Every text egui painted, and the rectangle it covers.
pub(super) fn texts(shapes: &[ClippedShape]) -> Vec<(String, egui::Rect)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => {
                let rect = t.galley.rect.translate(t.pos.to_vec2());
                out.push((t.galley.text().to_string(), rect));
            }
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}
