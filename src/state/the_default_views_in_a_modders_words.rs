//! The default views in a modder's words: a hash reads as one whatever its field is called, no
//! format code, schema path or vertex stride shows until Show internals is on, which a restart
//! keeps, an archive on disk shows its size, a file row says what the file is, and a version a
//! save changes says so.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use egui::accesskit::Role;
use or_io::desc::{FieldRepr, ScalarValue};
use or_io::gta5::{DrawableTarget, RmcDrawable};
use or_test_support::{skip_corpus, Corpus};

use super::an_edit_stays_where_it_was_made::Inspector;
use super::every_edit_undoes::drawable;
use super::nested_archives_survive_a_save::archive;
use super::opening_a_ydd::{finish_load, node_at};
use super::prefs::{keep_prefs_in, load_prefs, Game, Prefs};
use super::the_inspector_leads_with_a_summary::{drawn_by, leaf, other_spellings};
use super::{AppState, CloseAnswer, DocumentId};
use crate::ui::panels::inspector::summary_at;
use crate::ui::panels::selection_ids::{
    dict_entry_part_id, pack_lod_sub_index, pack_selection, ypt_model_part_id, DOC_ROOT_SEL,
    YPT_MODEL_KIND_LOD,
};

const WHAT: &str = "the default view of every corpus kind";

/// How many outline nodes of one file a described body is drawn with, besides its root.
const NODES: usize = 48;

/// Every text the inspector lays out for the active document, with its role.
fn inspected(state: &mut AppState) -> Vec<(Role, String)> {
    drawn_by(state, |ui, state| {
        crate::ui::panels::inspector::draw(ui, state)
    })
}

/// The words of `texts`, each once, which a text run only repeats.
fn words(texts: &[(Role, String)]) -> Vec<String> {
    (texts.iter())
        .filter(|(role, _)| *role != Role::TextRun)
        .map(|(_, text)| text.clone())
        .collect()
}

fn inspector_words(state: &mut AppState) -> Vec<String> {
    words(&inspected(state))
}

/// What a walk of the default view selects: the root, a node of the declared outline, or a legacy id.
enum Selection {
    Root,
    Described(or_editor::PackPath),
    Legacy(u64),
}

/// Each declared node of `id`'s outline, depth first.
fn described_nodes(state: &AppState, id: DocumentId) -> Vec<or_editor::PackPath> {
    fn walk(node: &or_editor::Node, at: &or_editor::PackPath, out: &mut Vec<or_editor::PackPath>) {
        for child in &node.children {
            let path = match &child.hop {
                Some(hop) => at.then(hop.clone()),
                None => at.clone(),
            };
            out.push(path.clone());
            walk(child, &path, out);
        }
    }
    let mut out = Vec::new();
    let outline = (state.document(id)).and_then(crate::ui::panels::hierarchy::described_outline);
    if let Some(outline) = outline {
        walk(&outline.root, &Default::default(), &mut out);
    }
    out
}

/// The first geometry of a drawable: its level of detail, model and geometry.
fn first_geometry<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Option<(u8, u8, u16)> {
    (drawable.lod_group.lods.iter().enumerate()).find_map(|(lod, held)| {
        let model = held.models.iter().position(|m| !m.geometries.is_empty())?;
        Some((lod as u8, model as u8, 0))
    })
}

/// The selections a legacy body draws a geometry for: a drawable's, a dictionary entry's and a particle model's.
fn geometry_selections(state: &AppState, id: DocumentId) -> Vec<u64> {
    use or_io::Asset;
    let Some(doc) = state.document(id) else {
        return Vec::new();
    };
    let super::DocPayload::Resource(asset) = &doc.payload else {
        return Vec::new();
    };
    let lod = |(lod, model, geom)| pack_lod_sub_index(lod, model, Some(geom));
    match asset {
        Asset::Drawable(d) => (first_geometry(&d.base))
            .map(|(lod, model, geom)| pack_selection(lod, u16::from(model), Some(u32::from(geom))))
            .into_iter()
            .collect(),
        Asset::DrawableDict(dict) => (dict.codes.iter().zip(&dict.entries))
            .find_map(|(code, entry)| {
                let at = first_geometry(&entry.as_deref()?.base)?;
                Some(dict_entry_part_id(*code, YPT_MODEL_KIND_LOD, lod(at)))
            })
            .into_iter()
            .collect(),
        Asset::Particles(list) => or_particle::list::models(list)
            .find_map(|(code, model)| {
                let at = first_geometry(model)?;
                Some(ypt_model_part_id(code, YPT_MODEL_KIND_LOD, lod(at)))
            })
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// What a document's format declares of the fields a grid draws, by type and the field's name as the grid spells it.
#[derive(Default)]
struct Declared {
    fields: HashSet<(String, String)>,
    /// The fields declared a `Hash32`, whatever they are called.
    hashes: HashSet<(String, String)>,
    /// The type an inline field holds.
    inline: HashMap<(String, String), String>,
}

impl Declared {
    fn of(state: &AppState, id: DocumentId) -> Self {
        let mut out = Self::default();
        let Some(doc) = state.document(id) else {
            return out;
        };
        let Some(image) = doc.resource.as_ref() else {
            return out;
        };
        let Some(format) = or_io::gta5::formats::format(doc.kind.extension()) else {
            return out;
        };
        let Ok(reach) = format
            .target_of_version(image.seal.version())
            .and_then(|target| format.reach(target))
        else {
            return out;
        };
        for ty in reach.types() {
            let hashed = reach.hash_fields(&ty.name);
            for field in &ty.fields {
                let key = (ty.name.clone(), field.display_name());
                out.fields.insert(key.clone());
                if hashed.contains(&field.name.as_str()) {
                    out.hashes.insert(key.clone());
                }
                if let FieldRepr::Inline { type_name } = &field.repr {
                    out.inline.insert(key, type_name.clone());
                }
            }
        }
        out
    }

    /// Whether the row `label` of a grid headed `title` is a field declared a hash, where the declarations say.
    fn is_a_hash(&self, title: &str, label: &str) -> Option<bool> {
        let (mut ty, segments) = (title.to_string(), label.split(" > ").collect::<Vec<_>>());
        for (n, segment) in segments.iter().enumerate() {
            let name = match segment.rsplit_once(" [") {
                Some((name, index)) if index.ends_with(']') => name,
                _ => segment,
            };
            let key = (ty, name.to_string());
            if !self.fields.contains(&key) {
                return None;
            }
            if n + 1 == segments.len() {
                return Some(self.hashes.contains(&key));
            }
            ty = self.inline.get(&key)?.clone();
        }
        None
    }
}

/// The grid `state` draws for `selection` of `id`.
fn grid_of(state: &AppState, id: DocumentId, selection: &Selection) -> Option<or_editor::Summary> {
    let doc = state.document(id)?;
    match selection {
        Selection::Root => {
            let (_, root) = super::described_root(doc)?;
            summary_at(doc, &root, 0)
        }
        Selection::Described(path) => {
            let outline = crate::ui::panels::hierarchy::described_outline(doc)?;
            let node = outline.find(path)?;
            summary_at(doc, node.type_name.as_deref()?, node.at)
        }
        Selection::Legacy(_) => None,
    }
}

/// What `texts` say that a default view leaves to Show internals: a binary format's code, a vertex stride, the container's name or a schema's path.
fn says_an_internal(texts: &[String], schema_path: &str) -> Vec<String> {
    let says = |text: &str, word: &str, same_case: bool| {
        text.split(|c: char| !c.is_alphanumeric()).any(|w| {
            if same_case {
                w == word
            } else {
                w.eq_ignore_ascii_case(word)
            }
        })
    };
    texts
        .iter()
        .filter(|text| {
            ["PSO", "RBF", "RBM", "RSC7"]
                .iter()
                .any(|code| says(text, code, true))
                || says(text, "stride", false)
                || (!schema_path.is_empty() && text.contains(schema_path))
        })
        .cloned()
        .collect()
}

/// What the default view of the file at `path` draws wrongly, at its root, its outline's nodes and its geometries.
fn found_in(state: &mut AppState, path: &Path) -> Result<Vec<String>, String> {
    state.clear_error();
    state.load_file(path.to_path_buf());
    let id = finish_load(state)?;
    let kind = state.document(id).map(|d| d.kind).ok_or("no document")?;
    let declared = Declared::of(state, id);
    let schema_path = match state.document(id).map(|d| &d.payload) {
        Some(super::DocPayload::Meta(meta)) => crate::state::meta_schema()
            .struct_any(meta.type_name())
            .map(|s| s.source_path.clone())
            .unwrap_or_default(),
        _ => String::new(),
    };

    let mut selections = vec![Selection::Root];
    selections.extend(
        (described_nodes(state, id).into_iter())
            .take(NODES)
            .map(Selection::Described),
    );
    selections.extend((geometry_selections(state, id).into_iter()).map(Selection::Legacy));
    let mut found = Vec::new();
    for selection in &selections {
        let what = match selection {
            Selection::Root => "root".to_string(),
            Selection::Described(path) => path.to_string(),
            Selection::Legacy(id) => format!("{id:#x}"),
        };
        match selection {
            Selection::Root => {}
            Selection::Described(path) => state.selection.select_described(id, path.clone()),
            Selection::Legacy(sel) => state.selection.select(id, *sel),
        }
        let mut shown = inspected(state);
        if kind == super::DocumentKind::Meta {
            shown.extend(drawn_by(state, |ui, state| {
                crate::ui::panels::meta_editor::draw(ui, state)
            }));
        }
        for text in says_an_internal(&words(&shown), &schema_path) {
            found.push(format!("{} ({what}) says {text:?}", path.display()));
        }
        let Some(grid) = grid_of(state, id, selection) else {
            continue;
        };
        for row in &grid.rows {
            if declared.is_a_hash(&grid.title, &row.label) == Some(true)
                && !(row.hash && row.value.starts_with("0x"))
            {
                found.push(format!(
                    "{} ({what}) draws the hash {} as {:?}",
                    path.display(),
                    row.label,
                    row.value
                ));
            }
            if let (true, false, Some(ScalarValue::Int(held))) = (row.hash, row.internal, &row.held)
            {
                // A small number is a word of other texts too.
                let held = *held as u32;
                if held >= 0x1_0000 {
                    for other in other_spellings(&shown, held) {
                        found.push(format!(
                            "{} ({what}) spells {} another way: {other}",
                            path.display(),
                            row.label
                        ));
                    }
                }
            }
        }
    }
    state.close_document(id);
    state.answer_close(CloseAnswer::DontSave);
    Ok(found)
}

fn default_views_read_in_words(shard: usize) {
    let all = super::viewing_never_edits::one_file_of_every_kind();
    if all.is_empty() {
        skip_corpus(WHAT, "no file of a kind the app opens is staged");
        return;
    }
    let files = super::shards::take(all, shard);
    if files.is_empty() {
        return;
    }
    // ⚠ Not `AppState::default()`, which reads the preferences every test shares.
    let mut state = AppState::from_prefs(Prefs::default());
    // ⚠ An Enhanced file otherwise waits on the question whether to switch editions.
    state.prefs.active_game = Some(Game::GtaVEnhanced);

    let (mut opened, mut found) = (0, Vec::new());
    for path in &files {
        match found_in(&mut state, path) {
            Ok(wrong) => {
                opened += 1;
                found.extend(wrong);
            }
            Err(why) => eprintln!("{} did not open: {why}", path.display()),
        }
    }
    assert!(opened > 0, "none of {} staged files opened", files.len());
    found.sort();
    found.dedup();
    assert!(
        found.is_empty(),
        "a default view is in a modder's words:\n  {}",
        found.join("\n  ")
    );
}

mod every_corpus_kind_draws_each_hash_as_a_hash_and_no_format_code_schema_path_or_stride_in_a_default_view {
    super::super::shards::shard_tests!(super::default_views_read_in_words);
}

/// Whether `token` is a path in this repository: a source file, a document or a folder of the tree.
fn is_a_repository_path(token: &str) -> bool {
    let token = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.');
    token.ends_with(".rs")
        || token.ends_with(".md")
        || ["crates/", "docs/", "src/"]
            .iter()
            .any(|folder| token.starts_with(folder))
}

/// What `texts` word wrongly in a default view: a row its section states twice, "1 models" or "1
/// textures", and a path in the repository.
fn worded_wrongly(texts: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    // A particle model's geometry listed its index count as "Index count" and as "Indices in buffer".
    let said = |label: &str| texts.iter().any(|text| text == label);
    if said("Index count") && said("Indices in buffer") {
        found.push("states its index count twice".to_string());
    }
    for text in texts {
        let words: Vec<&str> = text.split_whitespace().collect();
        for pair in words.windows(2) {
            let counted = pair[1].trim_end_matches(')');
            if pair[0].trim_start_matches('(') == "1" && matches!(counted, "models" | "textures") {
                found.push(format!("counts one in the plural in {text:?}"));
            }
        }
        if words.iter().any(|word| is_a_repository_path(word)) {
            found.push(format!("names a path in the repository in {text:?}"));
        }
    }
    found
}

/// What the default views of the file at `path` word wrongly: its inspector at its root, its outline's
/// nodes and its geometries, and its Outline and Gallery as they open.
fn worded_wrongly_in(state: &mut AppState, path: &Path) -> Result<Vec<String>, String> {
    state.clear_error();
    state.load_file(path.to_path_buf());
    let id = finish_load(state)?;
    let mut selections = vec![Selection::Root];
    selections.extend(
        (described_nodes(state, id).into_iter())
            .take(NODES)
            .map(Selection::Described),
    );
    selections.extend((geometry_selections(state, id).into_iter()).map(Selection::Legacy));
    let mut found = Vec::new();
    let mut note = |what: &str, shown: &[(Role, String)]| {
        for fault in worded_wrongly(&words(shown)) {
            found.push(format!("{} ({what}) {fault}", path.display()));
        }
    };
    note(
        "the Outline",
        &drawn_by(state, |ui, state| {
            crate::ui::panels::hierarchy::draw_outline(ui, state)
        }),
    );
    note(
        "the Gallery",
        &drawn_by(state, |ui, state| {
            crate::ui::panels::gallery::draw(ui, state, None)
        }),
    );
    for selection in &selections {
        let what = match selection {
            Selection::Root => "root".to_string(),
            Selection::Described(path) => path.to_string(),
            Selection::Legacy(id) => format!("{id:#x}"),
        };
        match selection {
            Selection::Root => {}
            Selection::Described(path) => state.selection.select_described(id, path.clone()),
            Selection::Legacy(sel) => state.selection.select(id, *sel),
        }
        note(&format!("the inspector at {what}"), &inspected(state));
    }
    state.close_document(id);
    state.answer_close(CloseAnswer::DontSave);
    Ok(found)
}

fn default_views_state_nothing_twice(shard: usize) {
    let all = super::viewing_never_edits::one_file_of_every_kind();
    if all.is_empty() {
        skip_corpus(WHAT, "no file of a kind the app opens is staged");
        return;
    }
    let files = super::shards::take(all, shard);
    if files.is_empty() {
        return;
    }
    // ⚠ Not `AppState::default()`, which reads the preferences every test shares.
    let mut state = AppState::from_prefs(Prefs::default());
    // ⚠ An Enhanced file otherwise waits on the question whether to switch editions.
    state.prefs.active_game = Some(Game::GtaVEnhanced);

    let (mut opened, mut found) = (0, Vec::new());
    for path in &files {
        match worded_wrongly_in(&mut state, path) {
            Ok(wrong) => {
                opened += 1;
                found.extend(wrong);
            }
            Err(why) => eprintln!("{} did not open: {why}", path.display()),
        }
    }
    assert!(opened > 0, "none of {} staged files opened", files.len());
    found.sort();
    found.dedup();
    assert!(
        found.is_empty(),
        "a default view words something wrongly:\n  {}",
        found.join("\n  ")
    );
}

mod every_corpus_kind_states_no_row_twice_counts_one_model_in_the_singular_and_names_no_repository_path_in_a_default_view {
    super::super::shards::shard_tests!(super::default_views_state_nothing_twice);
}

/// The label of a row the declared grid keeps behind Show internals, in a described drawable.
fn an_internal_label(state: &AppState, doc: DocumentId) -> String {
    let doc = state.document(doc).expect("open");
    let (_, root) = super::described_root(doc).expect("a drawable is described");
    let rows = summary_at(doc, &root, 0).expect("a summary").rows;
    (rows.into_iter())
        .find(|r| r.internal && !r.label.is_empty())
        .expect("the grid keeps a row behind Show internals")
        .label
}

/// A drawable of no corpus, active with its root selected.
fn a_drawable_selected(state: &mut AppState) -> DocumentId {
    let doc = drawable(state, "probe.ydr");
    state.active_workspace = state.document(doc).expect("open").kind.info().workspace;
    state.selection.select(doc, DOC_ROOT_SEL);
    doc
}

#[test]
fn show_internals_stays_as_the_user_left_it_across_a_restart() {
    let primary = egui::PointerButton::Primary;
    let dir = tempfile::tempdir().expect("a scratch folder");
    // ⚠ The click saves the preferences, which no other test may read.
    keep_prefs_in(dir.path());
    // A run of the editor: the preferences it finds, and a drawable open and selected.
    let run = || {
        let mut state = AppState::from_prefs(load_prefs());
        state.documents.clear();
        let doc = a_drawable_selected(&mut state);
        (state, doc)
    };

    let (mut state, doc) = run();
    let internal = an_internal_label(&state, doc);
    assert!(
        !inspector_words(&mut state).contains(&internal),
        "`{internal}` shows before Show internals is on"
    );
    Inspector::default().click(&mut state, "Show internals", primary);
    assert!(
        inspector_words(&mut state).contains(&internal),
        "the click did not turn Show internals on"
    );

    let (mut restarted, _) = run();
    assert!(
        inspector_words(&mut restarted).contains(&internal),
        "a restart turned Show internals off"
    );
    Inspector::default().click(&mut restarted, "Show internals", primary);

    let (mut again, _) = run();
    assert!(
        !inspector_words(&mut again).contains(&internal),
        "a restart turned Show internals back on after it was turned off"
    );
}

/// The inspector's words for the row `name` of the folder holding `files`, which the user selected.
fn inspected_row(files: &[(&str, &[u8])], name: &str) -> (AppState, Vec<String>) {
    let dir = tempfile::tempdir().expect("a scratch folder");
    for (file, bytes) in files {
        std::fs::write(dir.path().join(file), bytes).expect("a file");
    }
    let mut state = AppState::new();
    state.documents.clear();
    state.open_folder(dir.path().to_path_buf());
    let folder = state.active_document.expect("the folder opens");
    let row = node_at(&mut state, folder, name);
    state.selection.select(folder, row);
    let shown = inspector_words(&mut state);
    (state, shown)
}

#[test]
fn an_archive_on_disk_not_yet_opened_shows_its_size_in_its_inspector() {
    let bytes = archive(&[("a.txt", b"hello")]);
    let (state, shown) = inspected_row(&[("outer.rpf", &bytes)], "outer.rpf");

    let folder = state.active_document.expect("the folder");
    let Some(super::DocPayload::Rpf(rpf)) = state.document(folder).map(|d| &d.payload) else {
        panic!("the folder is a tree");
    };
    let row = rpf.session.children_of(rpf.session.root_id())[0];
    assert!(
        matches!(
            rpf.session.node(row).map(|n| &n.backing),
            Some(super::RpfNodeBacking::Archive { scanned: false, .. })
        ),
        "the archive was opened by selecting it"
    );
    let size = or_job::human_bytes(bytes.len() as u64);
    assert!(
        shown.contains(&size),
        "the inspector of an unopened archive does not say it is {size}: {shown:?}"
    );
}

#[test]
fn a_text_files_inspector_is_headed_as_text_and_a_file_nothing_names_as_a_file() {
    let (_, shown) = inspected_row(&[("notes.txt", b"words")], "notes.txt");
    let said = |shown: &[String], text: &str| shown.iter().filter(|t| *t == text).count();
    assert_eq!(
        said(&shown, "Text Document"),
        1,
        "a text file is not headed as text, once: {shown:?}"
    );
    for stored in ["Binary File", "Resource File"] {
        assert_eq!(
            said(&shown, stored),
            0,
            "{stored} heads a text file: {shown:?}"
        );
    }

    let (_, shown) = inspected_row(&[("blob.zzz", b"words")], "blob.zzz");
    assert_eq!(
        said(&shown, "File"),
        1,
        "a file nothing names is not headed as a file: {shown:?}"
    );
}

/// A Legacy clip dictionary of the smallest size staged, opened and active.
fn a_clip_dictionary() -> Option<(AppState, DocumentId)> {
    let smallest = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ycd")
        .target(or_test_support::TARGET_GEN8)
        .find()
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()))?;
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.load_file(smallest);
    let id = finish_load(&mut state).ok()?;
    Some((state, id))
}

#[test]
fn a_clip_dictionarys_facts_under_show_internals_do_not_repeat_rows_of_its_grid() {
    let Some((mut state, id)) = a_clip_dictionary() else {
        skip_corpus(
            "a_clip_dictionarys_facts_under_show_internals_do_not_repeat_rows_of_its_grid",
            "no Legacy .ycd is staged",
        );
        return;
    };
    state.prefs.show_internals = true;
    let shown = inspector_words(&mut state);

    let doc = state.document(id).expect("open");
    let (_, root) = super::described_root(doc).expect("a clip dictionary is described");
    let rows = summary_at(doc, &root, 0).expect("a summary").rows;
    let gridded = |name: &str| {
        let squashed = |text: &str| text.to_ascii_lowercase().replace('_', "");
        let row = rows.iter().find(|r| squashed(leaf(&r.label)) == name);
        row.map(|r| r.label.clone())
            .unwrap_or_else(|| panic!("the grid has no `{name}`: {rows:?}"))
    };
    for (fact, name) in [
        ("Base-name keys", "basenamekeys"),
        ("Owns anim dict", "animdictionaryowner"),
        ("Hash buckets", "slots"),
    ] {
        let label = gridded(name);
        assert!(
            shown.contains(&label),
            "Show internals does not list `{label}` in the grid: {shown:?}"
        );
        assert!(
            !shown.iter().any(|t| t == fact),
            "`{fact}` repeats the grid's `{label}`: {shown:?}"
        );
    }
}

/// A Legacy drawable of no corpus, opened at the version `version`, active with its root selected.
fn drawable_at(state: &mut AppState, version: u32) -> DocumentId {
    use or_io::container::resource::Rsc7;
    let format = or_io::gta5::formats::format("ydr").expect(".ydr is declared");
    assert_eq!(
        format.target_of_version(version),
        Ok(or_io::Generation::Gen8)
    );
    let seal = Rsc7::invent_single_chunk_seal(version, 0x4000).expect("a seal");
    let data = vec![0; seal.total_size() as usize];
    let image = super::ResourceImage {
        virtual_size: data.len(),
        data,
        seal,
    };
    let id = state.alloc_doc_id();
    let payload = super::DocPayload::Resource(or_io::Asset::Drawable(Default::default()));
    let doc = super::Document::new(id, Some("probe.ydr".into()), payload)
        .with_resource(Some(std::sync::Arc::new(image)));
    state.documents.push(doc);
    state.active_document = Some(id);
    state.active_workspace = state.document(id).expect("open").kind.info().workspace;
    state.selection.select(id, DOC_ROOT_SEL);
    id
}

/// The version of `doc`'s image now.
fn version_of(state: &AppState, doc: DocumentId) -> Option<u32> {
    let image = state.document(doc)?.resource.as_ref()?;
    Some(image.seal.version())
}

/// The Edition row the inspector shows now.
fn edition(state: &mut AppState) -> String {
    let shown = inspector_words(state);
    let row = shown.iter().find(|t| t.starts_with("Legacy, version"));
    row.cloned()
        .unwrap_or_else(|| panic!("no Edition row: {shown:?}"))
}

/// An edit of `doc`'s drawable contents, as an editor of them makes one, sealed as the shell does.
fn edit_the_contents(state: &mut AppState, doc: DocumentId, distance: f32) {
    let document = state.document_mut(doc).expect("open");
    let drawable = super::drawable_mut(&mut document.payload, 0).expect("a Legacy drawable");
    let was = std::mem::replace(&mut drawable.lod_group.lod_distances[1], distance);
    let change = super::Change::part("distance 1", was, |p| {
        super::drawable_mut(p, 0)?
            .lod_group
            .lod_distances
            .get_mut(1)
    });
    state.record(doc, "Set a distance", change);
    // A frame asks for the image the edit took, and sealing builds it.
    inspector_words(state);
    state.seal_edits();
}

/// A drawable opened at a version a save does not write, then edited: the state, the document, the version the file had and the one a save writes.
fn an_edited_drawable() -> (AppState, DocumentId, u32, u32) {
    use or_io::container::resource::ResourceType;
    let opened = ResourceType::Ydr.gen8_alt_versions()[0];
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    let doc = drawable_at(&mut state, opened);
    assert_eq!(edition(&mut state), format!("Legacy, version {opened}"));
    edit_the_contents(&mut state, doc, 5.0);
    let written = version_of(&state, doc).expect("the image is built again");
    assert_ne!(written, opened, "a save writes the version the file had");
    (state, doc, opened, written)
}

#[test]
fn after_an_edit_of_the_contents_the_edition_row_says_the_version_a_save_writes_is_not_the_files() {
    let (mut state, _, opened, written) = an_edited_drawable();
    let row = edition(&mut state);
    assert!(
        row.contains(&written.to_string()) && row.contains(&opened.to_string()),
        "the Edition row `{row}` names only one of the versions {opened} and {written}"
    );
}

/// Saves the active document into `scratch`, with `edit` made while the file is written if given, and polls the save to its end.
fn saved_into(state: &mut AppState, scratch: &Path, edit: Option<DocumentId>) {
    // ⚠ A finished save writes the preferences, which no other test may read.
    keep_prefs_in(scratch);
    assert!(
        state.save_active_document_as(scratch.join("probe.ydr")),
        "the save did not start"
    );
    if let Some(doc) = edit {
        edit_the_contents(state, doc, 6.0);
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while state.save_receiver.is_some() && std::time::Instant::now() < until {
        state.poll_save_result();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(state.save_receiver.is_none(), "the save never finished");
}

#[test]
fn after_a_save_the_edition_row_says_the_version_the_file_now_has_and_no_other() {
    let (mut state, doc, _, written) = an_edited_drawable();
    let scratch = tempfile::tempdir().expect("a scratch folder");
    saved_into(&mut state, scratch.path(), None);
    assert!(
        !state.document(doc).expect("open").dirty,
        "the save left the document unsaved: {}",
        state.error_dialog_message
    );
    assert_eq!(edition(&mut state), format!("Legacy, version {written}"));
}

#[test]
fn an_edit_made_while_a_save_is_written_leaves_the_edition_row_no_old_version_of_the_file() {
    let (mut state, doc, opened, _) = an_edited_drawable();
    let scratch = tempfile::tempdir().expect("a scratch folder");
    saved_into(&mut state, scratch.path(), Some(doc));
    assert!(
        state.document(doc).expect("open").dirty,
        "the edit made during the save was lost: {}",
        state.error_dialog_message
    );
    let row = edition(&mut state);
    assert!(
        !row.contains(&opened.to_string()),
        "the Edition row `{row}` still names version {opened}, which the save replaced"
    );
}

#[test]
fn a_metadata_files_format_code_shows_under_show_internals_and_not_before() {
    let mut state = AppState::from_prefs(Prefs::default());
    // ⚠ An Enhanced file otherwise waits on the question whether to switch editions.
    state.prefs.active_game = Some(Game::GtaVEnhanced);
    let header = |state: &mut AppState| {
        words(&drawn_by(state, |ui, state| {
            crate::ui::panels::meta_editor::draw(ui, state)
        }))
    };
    for path in super::viewing_never_edits::one_file_of_every_kind() {
        state.clear_error();
        state.load_file(path.clone());
        let Ok(id) = finish_load(&mut state) else {
            continue;
        };
        let code = match state.document(id).map(|d| &d.payload) {
            Some(super::DocPayload::Meta(meta)) if meta.source_format().is_binary() => {
                meta.source_format().label()
            }
            _ => {
                state.close_document(id);
                state.answer_close(CloseAnswer::DontSave);
                continue;
            }
        };
        assert!(
            !header(&mut state).iter().any(|t| t == code),
            "{} shows its format code, {code}, before Show internals is on",
            path.display()
        );
        state.prefs.show_internals = true;
        assert!(
            header(&mut state).iter().any(|t| t == code),
            "{} does not show its format code, {code}, under Show internals",
            path.display()
        );
        return;
    }
    skip_corpus(
        "a_metadata_files_format_code_shows_under_show_internals_and_not_before",
        "no binary metadata file is staged",
    );
}

#[test]
fn a_geometrys_vertex_stride_shows_under_show_internals_and_not_before() {
    const WHAT: &str = "a_geometrys_vertex_stride_shows_under_show_internals_and_not_before";
    let mut state = AppState::from_prefs(Prefs::default());
    for ext in ["ydr", "ydd"] {
        let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), ext)
            .target(or_test_support::TARGET_GEN8)
            .find();
        files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
        for path in files {
            state.clear_error();
            state.load_file(path.clone());
            let Ok(id) = finish_load(&mut state) else {
                continue;
            };
            let Some(selected) = geometry_selections(&state, id).first().copied() else {
                state.close_document(id);
                state.answer_close(CloseAnswer::DontSave);
                continue;
            };
            state.selection.select(id, selected);
            let stride =
                |state: &mut AppState| inspector_words(state).contains(&"Vertex stride".into());
            assert!(
                !stride(&mut state),
                "{} shows a vertex stride before Show internals is on",
                path.display()
            );
            state.prefs.show_internals = true;
            assert!(
                stride(&mut state),
                "{} does not show its vertex stride under Show internals",
                path.display()
            );
            return;
        }
    }
    skip_corpus(WHAT, "no Legacy .ydr or .ydd with a geometry is staged");
}
