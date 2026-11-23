//! Opening a file and drawing every panel over it changes no document.
//!
//! ⚠ **Two questions, because a panel can change a document without marking
//! it.** The ` *` of a modified document is one; what saving it writes, asked
//! before the first frame and after the last, is the other.

use std::path::{Path, PathBuf};

use super::opening_a_ydd::finish_load;
use super::prefs::Game;
use super::{AppState, CloseAnswer, DocumentId};
use crate::ui::dock::{Tab, Workspace};

const WHAT: &str = "opening a file of every kind and drawing every panel modifies no document";

/// Frames per arrangement: egui lays some widgets out only on the second.
const FRAMES: usize = 3;

/// The smallest staged file of each extension the app opens, per target, from the shared and the metadata corpus.
pub(super) fn one_file_of_every_kind() -> Vec<PathBuf> {
    let meta = or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        "OPENRAGE_META_CORPUS",
        "binary",
    );
    let size = |path: &PathBuf| std::fs::metadata(path).map_or(u64::MAX, |m| m.len());
    let mut files = Vec::new();
    for ext in super::kinds::openable_extensions() {
        for corpus in [
            or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), ext),
            or_test_support::Corpus::with_root(meta.path(), ext),
        ] {
            for target in or_test_support::KNOWN_TARGETS {
                files.extend(
                    corpus
                        .clone()
                        .target(target)
                        .find()
                        .into_iter()
                        .min_by_key(size),
                );
            }
        }
    }
    files
}

/// What saving `id` now would write, or why it would not.
fn saved(state: &mut AppState, id: DocumentId) -> Result<Vec<u8>, String> {
    let source = state.save_source_for(id).ok_or("the document is gone")?;
    source
        .into_bytes(
            state.prefs.rpf_output_mode,
            state.keys.provider(),
            or_io::decl::Repair::Refuse,
        )
        .map(|(bytes, _)| bytes)
}

/// `FRAMES` frames of the editor's shell, ticked and drawn as its redraw does, with no window.
fn draw(state: &mut AppState, arrange: impl FnOnce(&mut AppState)) {
    arrange(state);
    let ctx = crate::app::gpu::configured_egui_context(Some(2048), &state.prefs.appearance);
    for frame in 0..FRAMES {
        state.tick_previews();
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        });
        crate::ui::draw(&ctx, state, None);
        crate::app::modals::draw_editor_overlays(&ctx, state);
        let _ = ctx.end_pass();
    }
}

/// Every way `path` changed a document once opened and drawn, each naming its kind and the file.
fn changes_from_viewing(state: &mut AppState, path: &Path) -> Result<Vec<String>, String> {
    state.clear_error();
    state.load_file(path.to_path_buf());
    let id = finish_load(state)?;
    let kind = state.document(id).map(|d| d.kind).ok_or("no document")?;
    let at = format!("{kind:?} {}", path.display());
    let mut changes = Vec::new();
    if state.documents.iter().any(|d| d.dirty) {
        changes.push(format!("{at}: modified as it opened"));
    }
    let before = saved(state, id);

    let ws = Workspace::for_kind(kind);
    let mut arrangements: Vec<(String, Option<Tab>)> = vec![("its workspace".into(), None)];
    for panel in crate::ui::panel_registry::all() {
        arrangements.push((format!("the `{}` panel", panel.id()), Some(Tab(panel.id()))));
    }
    for (what, tab) in arrangements {
        draw(state, |state| {
            state.set_active_document(id);
            state.active_workspace = ws;
            match tab {
                Some(tab) => {
                    *state.workspace_docks.get_mut(ws) = egui_dock::DockState::new(vec![tab])
                }
                None => state.workspace_docks.reset(ws),
            }
        });
        if let Some(doc) = state.documents.iter().find(|d| d.dirty) {
            changes.push(format!("{at}: drawing {what} modified {}", doc.name));
            break;
        }
    }
    if saved(state, id) != before {
        changes.push(format!(
            "{at}: saving it writes other bytes once its panels drew"
        ));
    }
    state.workspace_docks.reset(ws);
    state.close_document(id);
    state.answer_close(CloseAnswer::DontSave);
    Ok(changes)
}

fn viewing_modifies_nothing(shard: usize) {
    let all = one_file_of_every_kind();
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

    let (mut opened, mut changes) = (0, Vec::new());
    for path in &files {
        match changes_from_viewing(&mut state, path) {
            Ok(found) => {
                opened += 1;
                changes.extend(found);
            }
            Err(why) => eprintln!("{} did not open: {why}", path.display()),
        }
    }
    assert!(opened > 0, "none of {} staged files opened", files.len());
    assert!(
        changes.is_empty(),
        "viewing changed a document:\n  {}",
        changes.join("\n  ")
    );
}

mod opening_a_file_of_every_kind_and_drawing_every_panel_modifies_no_document {
    super::super::shards::shard_tests!(super::viewing_modifies_nothing);
}
