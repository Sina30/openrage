//! The inspector opens on a summary in a modder's words, and keeps the
//! engine's bookkeeping (vft words, counts, reference counts, padding and
//! addresses) behind "Show internals".
//!
//! ⚠ **Read from what egui lays out, not from the row model**, because a
//! per-format body draws rows of its own beside the declared grid.

use std::collections::BTreeSet;
use std::path::Path;

use egui::accesskit::Role;

use super::opening_a_ydd::finish_load;
use super::prefs::Game;
use super::{AppState, CloseAnswer, DocumentId};

const WHAT: &str = "the default inspector of every corpus kind";

/// How many outline nodes of one file a described body is drawn with, besides its root.
const NODES: usize = 48;

/// Every text the inspector lays out for the active document: its role and its words.
fn drawn(state: &mut AppState) -> Vec<(Role, String)> {
    drawn_by(state, |ui, state| {
        crate::ui::panels::inspector::draw(ui, state)
    })
}

/// Every text `draw` lays out over three frames: its role and its words.
pub(super) fn drawn_by(
    state: &mut AppState,
    draw: impl Fn(&mut egui::Ui, &mut AppState),
) -> Vec<(Role, String)> {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut texts = Vec::new();
    // egui lays some widgets out only on the second frame.
    for frame in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 8000.0),
            )),
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| draw(ui, state));
        if let Some(update) = output.platform_output.accesskit_update {
            texts = (update.nodes.iter())
                .filter_map(|(_, node)| {
                    let text = node.label().or_else(|| node.value())?;
                    Some((node.role(), text.to_string()))
                })
                .collect();
        }
    }
    texts
}

/// The names of the fields an owned value keeps to itself, in every type `doc`'s format reaches, that no type shows.
fn internal_names(state: &AppState, doc: DocumentId) -> BTreeSet<String> {
    let Some(doc) = state.document(doc) else {
        return BTreeSet::new();
    };
    let Some(image) = doc.resource.as_ref() else {
        return BTreeSet::new();
    };
    let Some(format) = or_io::gta5::formats::format(doc.kind.extension()) else {
        return BTreeSet::new();
    };
    let Ok(reach) = format
        .target_of_version(image.seal.version())
        .and_then(|target| format.reach(target))
    else {
        return BTreeSet::new();
    };
    let (mut kept, mut shown) = (BTreeSet::new(), BTreeSet::new());
    for ty in reach.types() {
        let Some(public) = reach.owned_public(&ty.name) else {
            continue;
        };
        for field in &ty.fields {
            if public.iter().any(|(name, _)| *name == field.name) {
                shown.insert(field.display_name());
            } else {
                kept.insert(field.display_name());
            }
        }
    }
    kept.difference(&shown).cloned().collect()
}

/// A grid label's own field: `Base > LodGroup > LodDistances [0]` is `LodDistances`.
pub(super) fn leaf(label: &str) -> &str {
    let last = label.rsplit(" > ").next().unwrap_or(label);
    match last.rsplit_once(" [") {
        Some((name, index)) if index.ends_with(']') => name,
        _ => last,
    }
}

/// What in `texts` is a vft word, a reference count, an address or a field the owned value keeps to itself.
fn internals_in(texts: &[(Role, String)], kept: &BTreeSet<String>) -> Vec<String> {
    let says = |text: &str, word: &str| {
        text.split(|c: char| !c.is_alphanumeric())
            .any(|w| w.eq_ignore_ascii_case(word))
    };
    texts
        .iter()
        // A text run repeats the label it lays out.
        .filter(|(role, _)| *role != Role::TextRun)
        .filter(|(_, text)| {
            ["vft", "vtable", "refcount"]
                .iter()
                .any(|word| says(text, word))
                || text.to_ascii_lowercase().contains("ref count")
                || text.trim_start().starts_with("->")
                || kept.contains(leaf(text))
        })
        .map(|(role, text)| format!("{role:?} {text:?}"))
        .collect()
}

/// What the default inspector of the file at `path` shows that it keeps to itself, at its root and its outline's nodes.
fn internals_shown(state: &mut AppState, path: &Path) -> Result<Vec<String>, String> {
    state.clear_error();
    state.load_file(path.to_path_buf());
    let id = finish_load(state)?;
    let kind = state.document(id).map(|d| d.kind).ok_or("no document")?;
    let kept = internal_names(state, id);

    // Each text once, with the first selection that showed it.
    let mut found = std::collections::BTreeMap::new();
    for text in internals_in(&drawn(state), &kept) {
        found.entry(text).or_insert_with(|| "root".to_string());
    }
    let nodes = state
        .document(id)
        .and_then(crate::ui::panels::hierarchy::described_outline)
        .map(|outline| node_paths(&outline.root, &Default::default()))
        .unwrap_or_default();
    for node in nodes.into_iter().take(NODES) {
        let what = node.to_string();
        state.selection.select_described(id, node);
        for text in internals_in(&drawn(state), &kept) {
            found.entry(text).or_insert_with(|| what.clone());
        }
    }
    state.close_document(id);
    state.answer_close(CloseAnswer::DontSave);
    Ok(found
        .into_iter()
        .map(|(text, what)| format!("{kind:?} {} ({what}): {text}", path.display()))
        .collect())
}

/// Every node under `node`, depth first.
fn node_paths(node: &or_editor::Node, at: &or_editor::PackPath) -> Vec<or_editor::PackPath> {
    let mut out = Vec::new();
    for child in &node.children {
        let path = match &child.hop {
            Some(hop) => at.then(hop.clone()),
            None => at.clone(),
        };
        out.push(path.clone());
        out.extend(node_paths(child, &path));
    }
    out
}

fn the_default_inspector_shows_no_internals(shard: usize) {
    let all = super::viewing_never_edits::one_file_of_every_kind();
    if all.is_empty() {
        or_test_support::skip_corpus(WHAT, "no file of a kind the app opens is staged");
        return;
    }
    let files = super::shards::take(all, shard);
    if files.is_empty() {
        return;
    }
    let mut state = AppState::default();
    // ⚠ An Enhanced file otherwise waits on the question whether to switch editions.
    state.prefs.active_game = Some(Game::GtaVEnhanced);

    let (mut opened, mut found) = (0, Vec::new());
    for path in &files {
        match internals_shown(&mut state, path) {
            Ok(shown) => {
                opened += 1;
                found.extend(shown);
            }
            Err(why) => eprintln!("{} did not open: {why}", path.display()),
        }
    }
    assert!(opened > 0, "none of {} staged files opened", files.len());
    assert!(
        found.is_empty(),
        "the default inspector shows what the owned value keeps to itself:\n  {}",
        found.join("\n  ")
    );
}

mod no_vtable_word_count_or_pointer_shows_in_the_default_inspector_of_any_corpus_kind {
    super::super::shards::shard_tests!(super::the_default_inspector_shows_no_internals);
}

#[test]
fn a_grid_label_is_read_as_its_own_field() {
    assert_eq!(leaf("Base > LodGroup > LodDistances [0]"), "LodDistances");
    assert_eq!(leaf("RefCount"), "RefCount");
    assert_eq!(leaf("Levels of detail"), "Levels of detail");
}

/// The key a dictionary's first entry is selected by, or a drawable's first shader's effect.
fn a_hash_in(payload: &super::DocPayload) -> Option<(Option<u64>, u32)> {
    use crate::ui::panels::selection_ids::dict_entry_id;
    use or_io::Asset;
    let entry = |key: u32| (Some(dict_entry_id(key)), key);
    match payload {
        super::DocPayload::Resource(Asset::DrawableDict(d)) => d.codes.first().copied().map(entry),
        super::DocPayload::Resource(Asset::ClothDict(d)) => d.codes.first().copied().map(entry),
        super::DocPayload::Resource(Asset::Clips(d)) => {
            or_anim::clip::clips(d).next().map(|(key, _)| entry(key))
        }
        super::DocPayload::Resource(Asset::Drawable(d)) => {
            let shader = d.shader_group.as_deref()?.shaders.first()?;
            Some((None, shader.basis_hash.get()))
        }
        _ => None,
    }
}

/// Each text that writes `hash` another way than `0x` and eight digits: with `#`, `hash_`, bare, unpadded, in lower case or in decimal.
pub(super) fn other_spellings(texts: &[(Role, String)], hash: u32) -> Vec<String> {
    let one = format!("0x{hash:08X}");
    let others = [
        format!("{hash:08X}"),
        format!("{hash:08x}"),
        format!("{hash:X}"),
        format!("{hash:x}"),
        hash.to_string(),
    ];
    (texts.iter())
        .filter(|(role, _)| *role != Role::TextRun)
        .filter(|(_, text)| {
            let rest = text.replace(&one, "");
            rest.split(|c: char| !c.is_ascii_alphanumeric())
                .map(|word| word.trim_start_matches("0x").trim_start_matches("0X"))
                .any(|word| others.iter().any(|other| word == other))
        })
        .map(|(_, text)| text.clone())
        .collect()
}

#[test]
fn a_hash_is_spelled_one_way_wherever_the_inspector_draws_one() {
    let mut state = AppState::default();
    let mut checked = Vec::new();
    for ext in ["ydd", "yld", "ycd", "ydr"] {
        let smallest = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), ext)
            .target(or_test_support::TARGET_GEN8)
            .find()
            .into_iter()
            .min_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
        let Some(path) = smallest else {
            continue;
        };
        state.load_file(path.clone());
        let id = finish_load(&mut state).expect("a staged file opens");
        let Some((selection, hash)) = state.document(id).and_then(|d| a_hash_in(&d.payload)) else {
            continue;
        };
        if let Some(selection) = selection {
            state.selection.select(id, selection);
        }
        let texts = drawn(&mut state);
        let one = format!("0x{hash:08X}");
        assert!(
            texts.iter().any(|(_, text)| text.contains(&one)),
            "{} draws its hash nowhere as {one}: {texts:?}",
            path.display()
        );
        let others = other_spellings(&texts, hash);
        assert!(
            others.is_empty(),
            "{} spells {one} another way too: {others:?}",
            path.display()
        );
        checked.push(ext);
        state.close_document(id);
        state.answer_close(CloseAnswer::DontSave);
    }
    if checked.is_empty() {
        or_test_support::skip_corpus(
            "a_hash_is_spelled_one_way_wherever_the_inspector_draws_one",
            "no Legacy .ydd, .yld, .ycd or .ydr is staged",
        );
    }
}

#[test]
fn every_other_spelling_of_a_hash_is_found() {
    let hash = 0xC677_BCAC_u32;
    for text in [
        format!("#{hash:08X}"),
        format!("hash_{hash:08X}"),
        format!("{hash:08X}"),
        format!("0x{hash:08x}"),
        format!("{hash}"),
        format!("shader_{hash:08X}"),
    ] {
        assert_eq!(
            other_spellings(&[(Role::Label, text.clone())], hash),
            std::slice::from_ref(&text),
            "{text}"
        );
    }
    let one = format!("0x{hash:08X} (prop)");
    assert!(other_spellings(&[(Role::Label, one)], hash).is_empty());
}
