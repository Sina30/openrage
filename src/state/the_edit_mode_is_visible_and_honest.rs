//! The edit mode is a control in the menu bar, a refused write offers it, and
//! a base-game Yes lasts only as long as the mode it was given under.

use std::path::PathBuf;

use super::destructive_file_operations_ask::node_at;
use super::document::{DocumentId, RpfNodeId};
use super::prefs::EditMode;
use super::{AppState, GateOutcome, RetailEditDecision};

/// An install holding a shipped `update` folder, mounted as the app mounts one.
fn mounted_install() -> (tempfile::TempDir, PathBuf, AppState, DocumentId, RpfNodeId) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let install = tmp.path().join("GTAV");
    std::fs::create_dir_all(install.join("update")).expect("install dirs");
    std::fs::write(install.join("gta5.exe"), b"an install marker").expect("marker");
    let mut state = AppState::new();
    state.retail_edit_session_allow = false;
    state.prefs.edit_mode = EditMode::Everything;
    state.open_game_root(install.clone());
    let doc = state.documents[0].id;
    state.set_active_document(doc);
    let update = node_at(&mut state, doc, "update");
    (tmp, install, state, doc, update)
}

type Draw = fn(&mut egui::Ui, &mut AppState);

/// Each labelled widget `draw` paints, with its centre.
fn frame(
    ctx: &egui::Context,
    state: &mut AppState,
    draw: Draw,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 720.0),
        )),
        events,
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| draw(ui, state));
    output
        .platform_output
        .accesskit_update
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let b = node.bounds()?;
                    let text = node.label().or_else(|| node.value())?;
                    let centre =
                        egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
                    Some((text.to_string(), centre))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn click(ctx: &egui::Context, state: &mut AppState, draw: Draw, is: impl Fn(&str) -> bool) {
    let shown = frame(ctx, state, draw, Vec::new());
    let at = shown
        .iter()
        .find(|(label, _)| is(label))
        .unwrap_or_else(|| panic!("nothing drawn is the one asked for: {shown:?}"))
        .1;
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        ctx,
        state,
        draw,
        vec![egui::Event::PointerMoved(at), press(true)],
    );
    frame(ctx, state, draw, vec![press(false)]);
}

/// The row that offers `mode`: its label, then its sentence.
fn row(mode: EditMode) -> impl Fn(&str) -> bool {
    move |label| label.starts_with(&format!("{}\n", mode.label()))
}

fn menu_bar(ui: &mut egui::Ui, state: &mut AppState) {
    crate::ui::panels::menu::draw(ui, state);
}

/// Open the menu bar's edit mode control and choose `mode`, as a user does.
fn choose_in_menu_bar(ctx: &egui::Context, state: &mut AppState, mode: EditMode) {
    click(ctx, state, menu_bar, |label| {
        label.ends_with(egui_phosphor::regular::CARET_DOWN)
    });
    click(ctx, state, menu_bar, row(mode));
}

/// A granted Yes holds until Read only is chosen in the menu bar, and choosing
/// the base game again asks again.
#[test]
fn a_base_game_yes_ends_when_the_menu_bar_switches_to_read_only() {
    let (_tmp, install, mut state, doc, update) = mounted_install();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    assert!(state.rpf_add_folder(doc, update, "granted").is_err());
    assert!(
        state.pending_retail_edit.is_some(),
        "the first base-game write of a session asks"
    );
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    assert!(
        install.join("update/granted").is_dir(),
        "and lands once granted"
    );
    state
        .rpf_add_folder(doc, update, "unasked")
        .expect("the Yes holds for the writes after it");
    assert!(install.join("update/unasked").is_dir());

    choose_in_menu_bar(&ctx, &mut state, EditMode::ReadOnly);
    assert_eq!(
        state.prefs.edit_mode,
        EditMode::ReadOnly,
        "the menu bar's control changes the mode"
    );
    assert!(state.rpf_add_folder(doc, update, "refused").is_err());
    assert!(
        !install.join("update/refused").exists(),
        "Read only takes back the Yes given before it"
    );

    choose_in_menu_bar(&ctx, &mut state, EditMode::Everything);
    assert!(state.rpf_add_folder(doc, update, "asked_again").is_err());
    assert!(
        state.pending_retail_edit.is_some(),
        "switching back asks again rather than reviving the old Yes"
    );
    choose_in_menu_bar(&ctx, &mut state, EditMode::ReadOnly);
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    assert!(
        !install.join("update/asked_again").exists(),
        "a question asked under the old mode is not answered under the new one"
    );
}

/// The gate itself, whatever changed the mode: a Yes stands in for
/// `Everything`'s question and for nothing else.
#[test]
fn a_session_yes_never_outlives_the_mode_it_was_given_in() {
    let (_tmp, install, mut state, _doc, _update) = mounted_install();
    let shipped = install.join("update");
    state.retail_edit_session_allow = true;
    for mode in [EditMode::ReadOnly, EditMode::Overlay] {
        state.prefs.edit_mode = mode;
        assert_eq!(
            state.gate_permits(Some(&shipped)),
            GateOutcome::Refused,
            "{mode:?} refuses the game as shipped, Yes or not"
        );
    }
    state.prefs.edit_mode = EditMode::Everything;
    assert_eq!(state.gate_permits(Some(&shipped)), GateOutcome::Proceed);
}

#[test]
fn a_refused_write_offers_to_change_the_mode() {
    let (_tmp, install, mut state, doc, update) = mounted_install();
    state.prefs.edit_mode = EditMode::ReadOnly;
    assert!(state.rpf_add_folder(doc, update, "wanted").is_err());
    assert_eq!(
        state.refused_write.as_deref(),
        Some(install.join("update").as_path()),
        "the refusal offers the mode, naming what it refused"
    );

    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let prompt: Draw = |ui, state| {
        let ctx = ui.ctx().clone();
        crate::ui::panels::edit_mode::draw_refused_write(&ctx, state);
    };
    // ⚠ A window's first frame only sizes it, so its rows move once it shows.
    frame(&ctx, &mut state, prompt, Vec::new());
    click(&ctx, &mut state, prompt, row(EditMode::Everything));
    assert_eq!(state.prefs.edit_mode, EditMode::Everything);
    assert_eq!(
        state.refused_write, None,
        "choosing a mode answers the offer"
    );

    assert!(state.rpf_add_folder(doc, update, "wanted").is_err());
    assert!(
        state.pending_retail_edit.is_some(),
        "the base game still asks before its first write"
    );
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    assert!(install.join("update/wanted").is_dir());
}

/// ⚠ Settings once only saved the new mode, so a Yes outlived a switch made there.
#[test]
fn the_editing_page_holds_the_mode_and_takes_back_a_yes_as_the_menu_bar_does() {
    let mut state = AppState::new();
    state.prefs.edit_mode = EditMode::Everything;
    state.retail_edit_session_allow = true;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let page: Draw = crate::ui::panels::settings_editing::draw_editing_page;

    let shown: Vec<String> = frame(&ctx, &mut state, page, Vec::new())
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    for setting in [
        "Read the install's mods overlay first",
        "Encryption for archives OpenRage saves:",
        "When OpenRage's export fixes would alter a file's bytes:",
    ] {
        assert!(
            shown.iter().any(|label| label == setting),
            "the Editing page holds `{setting}`: {shown:?}"
        );
    }

    click(&ctx, &mut state, page, row(EditMode::ReadOnly));
    assert_eq!(state.prefs.edit_mode, EditMode::ReadOnly);
    assert!(
        !state.retail_edit_session_allow,
        "changing the mode in Settings takes the Yes back too"
    );
}
