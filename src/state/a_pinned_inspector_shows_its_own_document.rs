//! An inspector pinned to a document draws and edits that document in every kind's body while
//! another document of its kind is active.

use std::collections::BTreeSet;
use std::path::PathBuf;

use or_editor::EditKind;
use or_io::desc::ScalarValue;
use or_io::gta5::gen8::GrcInstanceData;
use or_io::Asset;
use or_test_support::{skip_corpus, Corpus, TARGET_GEN8};

use super::an_edit_stays_where_it_was_made::{open, texts, Inspector};
use super::{AppState, DocPayload, Document, DocumentId};
use crate::ui::panels::inspector::{self, InspectorUiState};
use crate::ui::panels::selection_ids::{
    dict_entry_part_id, pack_index, ypt_effect_id, DOC_ROOT_SEL, SECT_SHADER, YPT_MODEL_KIND_SHADER,
};

/// How many staged files of a kind are opened, smallest first, to find a pair to test.
const TRIES: usize = 6;

/// An edit a kind's inspector body makes.
#[derive(Clone, Copy, Debug)]
enum Gesture {
    /// A drag across the first number of the declared structure at the file's root.
    Grid,
    /// A click on the first Remove of the first material that holds an entry.
    Material,
    /// A drag across the first effect's `duration_min`.
    Effect,
}

/// Each staged kind with an inspector body of its own, and the edits that body makes.
const KINDS: [(&str, &[Gesture]); 10] = [
    ("ybn", &[Gesture::Grid]),
    ("ycd", &[]),
    ("ydd", &[Gesture::Material]),
    ("ydr", &[Gesture::Grid, Gesture::Material]),
    ("yft", &[Gesture::Grid]),
    ("yld", &[]),
    ("ypt", &[Gesture::Effect]),
    ("ytd", &[]),
    ("fxc", &[]),
    ("ypdb", &[]),
];

impl Gesture {
    /// Where the inspector is pinned in `doc` to make this edit, and the text dragged or clicked; `None` where `doc` holds nothing to edit there.
    fn on(self, doc: &Document) -> Option<(u64, String)> {
        let first_material = |shaders: &[Box<GrcInstanceData>]| {
            (shaders.iter()).position(|shader| !shader.entries.is_empty())
        };
        match (self, &doc.payload) {
            (Gesture::Grid, _) => {
                let (_, root) = super::described_root(doc)?;
                let summary = inspector::summary_at(doc, &root, 0)?;
                let row = summary.rows.iter().find(|r| {
                    let number = r.edit.is_some_and(|e| e.kind == EditKind::F32);
                    let finite = matches!(r.held, Some(ScalarValue::F32(v)) if v.is_finite());
                    !r.internal && r.choices.is_none() && number && finite
                })?;
                Some((DOC_ROOT_SEL, row.label.clone()))
            }
            (Gesture::Material, DocPayload::Resource(Asset::Drawable(drawable))) => {
                let shader = first_material(&drawable.shader_group.as_deref()?.shaders)?;
                Some((pack_index(SECT_SHADER, shader as u32), "Remove".into()))
            }
            (Gesture::Material, DocPayload::Resource(Asset::DrawableDict(dict))) => {
                let mut entries = dict.entries.iter().zip(&dict.codes);
                entries.find_map(|(entry, &key)| {
                    let shaders = &entry.as_deref()?.shader_group.as_deref()?.shaders;
                    let shader = first_material(shaders)? as u32;
                    let at = dict_entry_part_id(key, YPT_MODEL_KIND_SHADER, shader);
                    Some((at, "Remove".into()))
                })
            }
            (Gesture::Effect, DocPayload::Resource(Asset::Particles(list))) => {
                or_particle::list::effect_rules(list).next()?;
                Some((ypt_effect_id(0), "duration_min".into()))
            }
            _ => None,
        }
    }
}

/// The staged Legacy files of `ext`, smallest first.
fn staged(ext: &str) -> Vec<PathBuf> {
    let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), ext)
        .target(TARGET_GEN8)
        .find();
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    files
}

/// Every text the last of three frames of `draw` paints.
fn painted(state: &mut AppState, draw: fn(&mut egui::Ui, &mut AppState)) -> BTreeSet<String> {
    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    // egui lays some widgets out only on the second frame.
    for frame in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 8000.0),
            )),
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        shapes = ctx.run_ui(input, |ui| draw(ui, state)).shapes;
    }
    texts(&shapes).into_iter().map(|(text, _)| text).collect()
}

/// What `doc`'s body draws while it is the active document with nothing selected, as the Document panel draws it.
fn body_of(state: &mut AppState, doc: DocumentId) -> BTreeSet<String> {
    state.set_active_document(doc);
    state.selection.clear_all();
    painted(state, inspector::draw_body)
}

/// Pins the inspector to the first of `files` while another is active, and says what its body draws wrongly.
fn drawn_while_pinned(files: &[PathBuf]) -> Option<Vec<String>> {
    let [first, others @ ..] = files else {
        return None;
    };
    let mut state = AppState::new();
    state.documents.clear();
    let pinned = open(&mut state, first);
    let own = body_of(&mut state, pinned);
    // The first other file whose body draws something the pinned one's does not.
    let (active, theirs) = others.iter().take(TRIES).find_map(|path| {
        let active = open(&mut state, path);
        let theirs: BTreeSet<String> = body_of(&mut state, active)
            .difference(&own)
            .cloned()
            .collect();
        (!theirs.is_empty()).then_some((active, theirs))
    })?;
    state.set_active_document(active);
    state.panels.get::<InspectorUiState>().pinned = Some((pinned, DOC_ROOT_SEL));
    let shown = painted(&mut state, inspector::draw);

    let what = |doc: DocumentId| {
        state
            .document(doc)
            .map(Document::file_name)
            .unwrap_or_default()
    };
    let (pinned, active) = (what(pinned), what(active));
    let mut wrong = Vec::new();
    if own.is_empty() {
        wrong.push(format!("{pinned}'s body draws nothing"));
    }
    let missing: Vec<&String> = own.difference(&shown).collect();
    if !missing.is_empty() {
        wrong.push(format!(
            "pinned to {pinned} beside {active}, it leaves out {missing:?}"
        ));
    }
    let foreign: Vec<&String> = theirs.intersection(&shown).collect();
    if !foreign.is_empty() {
        wrong.push(format!(
            "pinned to {pinned} beside {active}, it draws {active}'s {foreign:?}"
        ));
    }
    Some(wrong)
}

#[test]
fn a_pinned_inspector_draws_its_own_document_in_every_kinds_body() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let text: Vec<PathBuf> = [("pinned.txt", "one line"), ("active.txt", "two\nlines\n")]
        .into_iter()
        .map(|(name, body)| {
            let path = dir.path().join(name);
            std::fs::write(&path, body).expect("a scratch text file");
            path
        })
        .collect();

    let (mut unchecked, mut wrong) = (Vec::new(), Vec::new());
    let kinds = (KINDS.iter()).map(|(ext, _)| (*ext, staged(ext)));
    for (ext, files) in kinds.chain([("txt", text)]) {
        match drawn_while_pinned(&files) {
            Some(found) => wrong.extend(found.into_iter().map(|w| format!(".{ext}: {w}"))),
            None => unchecked.push(ext),
        }
    }
    if !unchecked.is_empty() {
        skip_corpus(
            "a_pinned_inspector_draws_its_own_document_in_every_kinds_body",
            format_args!("no two staged files whose bodies differ of {unchecked:?}"),
        );
    }
    assert!(
        wrong.is_empty(),
        "a pinned inspector draws another document's body:\n  {}",
        wrong.join("\n  ")
    );
}

/// The steps `doc` holds, and its image.
fn held(state: &AppState, doc: DocumentId) -> (usize, Option<Vec<u8>>) {
    let doc = state.document(doc).expect("open");
    let image = doc.resource.as_ref().map(|r| r.data.clone());
    (doc.history.done().len(), image)
}

/// Makes each of `gestures` in an inspector pinned to a staged file of `ext` while another is active, and says where an edit went wrong.
fn edited_while_pinned(ext: &str, gestures: &[Gesture]) -> Option<Vec<String>> {
    let mut state = AppState::new();
    state.documents.clear();
    let (mut pinned, mut active) = (None, None);
    for path in staged(ext).iter().take(TRIES) {
        let id = open(&mut state, path);
        let doc = state.document(id).expect("open");
        if pinned.is_none() && gestures.iter().all(|g| g.on(doc).is_some()) {
            pinned = Some(id);
        } else if active.is_none() {
            active = Some(id);
        }
        if pinned.is_some() && active.is_some() {
            break;
        }
    }
    let (pinned, active) = (pinned?, active?);
    state.set_active_document(active);

    let mut wrong = Vec::new();
    for &gesture in gestures {
        let doc = state.document(pinned).expect("open");
        let (selection, text) = gesture.on(doc).expect("chosen for it");
        state.panels.get::<InspectorUiState>().pinned = Some((pinned, selection));
        let before = (held(&state, pinned).0, held(&state, active));
        let mut inspector = Inspector::default();
        let shapes = inspector.frame(&mut state, Vec::new());
        if !texts(&shapes).iter().any(|(painted, _)| *painted == text) {
            wrong.push(format!("{gesture:?}: the pinned body draws no `{text}`"));
            continue;
        }
        match gesture {
            Gesture::Material => inspector.click(&mut state, &text, egui::PointerButton::Primary),
            Gesture::Grid | Gesture::Effect => inspector.drag(&mut state, &text, 40.0),
        }
        // As the shell does once the pointer is up.
        state.seal_edits();
        if held(&state, pinned).0 != before.0 + 1 {
            wrong.push(format!(
                "{gesture:?} on `{text}` made no step in the pinned document"
            ));
        }
        let untouched = state.document(active).is_some_and(|d| !d.dirty);
        if !untouched || held(&state, active) != before.1 {
            wrong.push(format!(
                "{gesture:?} on `{text}` landed in the active document"
            ));
        }
    }
    Some(wrong)
}

#[test]
fn an_edit_in_a_pinned_inspector_lands_in_its_document_and_leaves_the_active_one_untouched() {
    let (mut unchecked, mut wrong) = (Vec::new(), Vec::new());
    for (ext, gestures) in KINDS.iter().filter(|(_, gestures)| !gestures.is_empty()) {
        match edited_while_pinned(ext, gestures) {
            Some(found) => wrong.extend(found.into_iter().map(|w| format!(".{ext} {w}"))),
            None => unchecked.push(*ext),
        }
    }
    if !unchecked.is_empty() {
        skip_corpus(
            "an_edit_in_a_pinned_inspector_lands_in_its_document_and_leaves_the_active_one_untouched",
            format_args!("no two staged files of {unchecked:?}, one holding each edit its body makes"),
        );
    }
    assert!(
        wrong.is_empty(),
        "an edit in a pinned inspector went elsewhere:\n  {}",
        wrong.join("\n  ")
    );
}
