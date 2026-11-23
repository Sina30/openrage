//! Undo, Redo and the History panel act on the document the focused panel shows, a pinned
//! inspector's included; a particle file's Live Evolutions sliders are named for the effect the
//! preview plays; and a clip dictionary's inspector shows its declared structure under its summary.

use or_io::gta5::particle::{PtxEvolution, PtxEvolutionList};
use or_io::Asset;
use or_particle::list;
use or_test_support::{skip_corpus, Corpus, TARGET_GEN8};
use winit::keyboard::{Key, KeyCode, PhysicalKey};

use super::an_edit_stays_where_it_was_made::{open, texts, Inspector};
use super::{AppState, DocPayload, DocumentId, DocumentKind};
use crate::ui::commands;
use crate::ui::panels::inspector::{self, ypt_steps, InspectorUiState};
use crate::ui::panels::registry::ids;
use crate::ui::panels::selection_ids::ypt_effect_id;
use crate::ui::shortcuts;

/// A new particle file holding an effect for each of `names`.
fn particles(state: &mut AppState, names: &[&str]) -> DocumentId {
    let doc = state.new_empty_document(DocumentKind::Ypt);
    let payload = &mut state.document_mut(doc).expect("open").payload;
    let fx =
        ypt_steps::particles::<or_io::decl::Gen8>(payload).expect("a particle file holds a list");
    for name in names {
        assert!(list::add_effect(fx, name));
    }
    doc
}

/// Two particle files of one effect each, the inspector pinned to the first's effect and focused while the second is active.
fn pinned_and_focused() -> (AppState, DocumentId, DocumentId) {
    let mut state = AppState::default();
    state.documents.clear();
    let pinned = particles(&mut state, &["pinned"]);
    let active = particles(&mut state, &["active"]);
    state.set_active_document(active);
    state.panels.get::<InspectorUiState>().pinned = Some((pinned, ypt_effect_id(0)));
    let ws = state.active_workspace;
    crate::ui::dock_commands::reveal(state.workspace_docks.get_mut(ws), ids::DETAILS);
    (state, pinned, active)
}

/// The steps `doc` holds, and its effect's `duration_min`.
fn held(state: &AppState, doc: DocumentId) -> (usize, f32) {
    let document = state.document(doc).expect("open");
    let DocPayload::Resource(Asset::Particles(fx)) = &document.payload else {
        panic!("not a particle file");
    };
    let effect = list::effect_rules(fx).next().expect("an effect");
    (document.history.done().len(), effect.duration_min)
}

/// `Ctrl+Z` as winit hands it over, run through the registry as a press is.
fn ctrl_z(state: &mut AppState) {
    let z = Key::Character("z".into());
    let mods = crate::ui::shortcuts::PRIMARY_STATE;
    let code = PhysicalKey::Code(KeyCode::KeyZ);
    let undo =
        shortcuts::action_for_press(&z, code, mods, false, &state.prefs).expect("Ctrl+Z is bound");
    commands::run_action(state, &egui::Context::default(), undo);
}

/// A drag across the pinned effect's `duration_min`, ended as the shell ends one.
fn edit_the_pinned_effect(state: &mut AppState) {
    Inspector::default().drag(state, "duration_min", 40.0);
    state.seal_edits();
}

#[test]
fn ctrl_z_undoes_an_edit_in_a_pinned_inspector_while_the_inspector_has_focus() {
    let (mut state, pinned, active) = pinned_and_focused();
    let (opened, bystander) = (held(&state, pinned), held(&state, active));
    edit_the_pinned_effect(&mut state);
    assert_eq!(
        held(&state, pinned).0,
        1,
        "the drag made no step in the pinned file"
    );

    ctrl_z(&mut state);
    assert_eq!(
        held(&state, pinned),
        opened,
        "Ctrl+Z left the pinned file's edit"
    );
    assert_eq!(
        held(&state, active),
        bystander,
        "Ctrl+Z reached the active file"
    );
}

/// Every text a frame of the workspace on screen paints.
fn shell(ctx: &egui::Context, state: &mut AppState) -> Vec<String> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| crate::ui::dock::draw(ui, state, None));
    (texts(&output.shapes).into_iter())
        .map(|(text, _)| text)
        .collect()
}

/// ⚠ History opens in the focused panel's place and takes its focus, so it follows the panel it replaced.
#[test]
fn the_history_panel_opened_from_a_focused_pinned_inspector_lists_and_undoes_its_files_edits() {
    let (mut state, pinned, _) = pinned_and_focused();
    edit_the_pinned_effect(&mut state);
    let ctx = egui::Context::default();
    shell(&ctx, &mut state);

    let history = (commands::all(&state).into_iter())
        .find(|command| command.id == "panel: history")
        .expect("the palette offers the History panel");
    commands::run(&mut state, &ctx, &history);
    let ws = state.active_workspace;
    assert_eq!(state.workspace_docks.focused(ws), Some(ids::HISTORY));
    let shown = shell(&ctx, &mut state);
    assert!(
        shown.iter().any(|text| text == "Edit pinned"),
        "the History panel lists another file's edits: {shown:?}"
    );

    ctrl_z(&mut state);
    assert_eq!(
        held(&state, pinned).0,
        0,
        "Ctrl+Z in the History panel left the pinned file's edit"
    );
}

#[test]
fn the_live_evolution_sliders_are_named_for_the_effect_the_preview_plays() {
    let mut state = AppState::default();
    state.documents.clear();
    let doc = particles(&mut state, &["pinned", "playing"]);
    let payload = &mut state.document_mut(doc).expect("open").payload;
    let fx =
        ypt_steps::particles::<or_io::decl::Gen8>(payload).expect("a particle file holds a list");
    for rule in list::effect_rules_mut(fx) {
        let evolution = PtxEvolution {
            name: format!("{} evolution", rule.name),
            ..Default::default()
        };
        rule.evolution_list = Some(Box::new(PtxEvolutionList {
            evolutions: vec![evolution],
            ..Default::default()
        }));
    }
    let at = |name: &str| list::effect_rules(fx).position(|rule| rule.name == name);
    let (pinned, playing) = (at("pinned").expect("added"), at("playing").expect("added"));

    state.set_active_document(doc);
    // The preview plays what the Outline selects.
    state.selection.select(doc, ypt_effect_id(playing));
    state.tick_sim();
    state.panels.get::<InspectorUiState>().pinned = Some((doc, ypt_effect_id(pinned)));
    let painted: Vec<String> = (texts(&Inspector::default().frame(&mut state, Vec::new())))
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    let named = |name: &str| painted.iter().any(|text| text == name);
    assert!(
        named("playing evolution") && !named("pinned evolution"),
        "the sliders are not named for the effect the preview plays: {painted:?}"
    );
}

#[test]
fn a_clip_dictionary_shows_its_declared_structure_under_its_summary() {
    let smallest = (Corpus::new(env!("CARGO_MANIFEST_DIR"), "ycd").target(TARGET_GEN8))
        .find()
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    let Some(path) = smallest else {
        skip_corpus(
            "a_clip_dictionary_shows_its_declared_structure_under_its_summary",
            "no Legacy .ycd is staged",
        );
        return;
    };
    let mut state = AppState::default();
    state.documents.clear();
    let doc = open(&mut state, &path);
    state.set_active_document(doc);
    state.selection.clear_all();
    let structure = {
        let document = state.document(doc).expect("open");
        let (_, root) = super::described_root(document).expect("a clip dictionary is declared");
        inspector::summary_at(document, &root, 0).expect("its root is described")
    };

    let painted = texts(&Inspector::default().frame(&mut state, Vec::new()));
    let (_, summary) = (painted.iter())
        .find(|(text, _)| text == "Animations")
        .expect("the summary counts the animations");
    let shown = (structure.rows.iter())
        .filter(|row| !row.internal)
        .map(|row| row.label.as_str());
    let missing: Vec<&str> = std::iter::once(structure.title.as_str())
        .chain(shown)
        .filter(|label| {
            !(painted.iter()).any(|(text, at)| text == label && at.min.y >= summary.max.y)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "{} draws none of {missing:?} under its summary",
        path.display()
    );
}
