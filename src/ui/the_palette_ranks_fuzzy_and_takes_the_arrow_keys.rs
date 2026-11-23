//! The command palette matches letters in order, puts what was run from it
//! first, says why a command cannot run, and runs the row the arrow keys
//! highlight.

use egui::accesskit::Role;

use crate::state::{AppState, Prefs};
use crate::ui::panels::command_palette;

/// A widget one palette frame drew, top to bottom: its label or text, and whether it was disabled.
fn frame(
    ctx: &egui::Context,
    state: &mut AppState,
    events: Vec<egui::Event>,
) -> Vec<(Role, String, bool)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| command_palette::draw(ui.ctx(), state));
    let mut drawn: Vec<(f64, Role, String, bool)> = output
        .platform_output
        .accesskit_update
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let text = node.label().or_else(|| node.value())?.to_string();
                    let top = node.bounds()?.y0;
                    Some((top, node.role(), text, node.is_disabled()))
                })
                .collect()
        })
        .unwrap_or_default();
    drawn.sort_by(|a, b| a.0.total_cmp(&b.0));
    drawn
        .into_iter()
        .map(|(_, role, text, disabled)| (role, text, disabled))
        .collect()
}

/// The palette's rows, top to bottom: the buttons below its query field.
fn rows(drawn: &[(Role, String, bool)]) -> Vec<String> {
    drawn
        .iter()
        .skip_while(|(role, ..)| *role != Role::TextInput)
        .filter(|(role, ..)| *role == Role::Button)
        .map(|(_, label, _)| label.clone())
        .collect()
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// The palette opened again with `query` typed into it.
fn typed(ctx: &egui::Context, state: &mut AppState, query: &str) -> Vec<(Role, String, bool)> {
    // A frame with the palette closed forgets the last query, as one does in the app.
    frame(ctx, state, Vec::new());
    command_palette::open(state);
    frame(ctx, state, Vec::new());
    frame(ctx, state, vec![egui::Event::Text(query.to_string())]);
    frame(ctx, state, Vec::new())
}

fn a_fresh_palette() -> (egui::Context, AppState) {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    (ctx, AppState::from_prefs(Prefs::default()))
}

#[test]
fn svas_finds_save_as_first() {
    let (ctx, mut state) = a_fresh_palette();
    let shown = rows(&typed(&ctx, &mut state, "svas"));
    assert_eq!(
        shown.first().map(String::as_str),
        Some(format!(
            "File: Save as… {}+Shift+S",
            crate::ui::shortcuts::primary_modifier_name()
        ))
        .as_deref(),
        "{shown:#?}"
    );
}

#[test]
fn the_arrow_keys_move_the_highlight_and_enter_runs_the_highlighted_row() {
    let (ctx, mut state) = a_fresh_palette();
    let grid = state.prefs.viewport.show_grid;
    let axis = state.prefs.viewport.show_axis;
    let shown = rows(&typed(&ctx, &mut state, "show"));
    let grid_row = shown
        .iter()
        .position(|row| row == "View: Show grid")
        .unwrap_or_else(|| panic!("no Show grid row: {shown:#?}"));
    assert_eq!(
        shown.get(grid_row + 1).map(String::as_str),
        Some("View: Show axis"),
        "{shown:#?}"
    );

    // Down from the first row to the one under Show grid, which is Show axis.
    for _ in 0..=grid_row {
        frame(&ctx, &mut state, vec![key(egui::Key::ArrowDown)]);
    }
    frame(&ctx, &mut state, vec![key(egui::Key::Enter)]);
    assert_eq!(
        (
            state.prefs.viewport.show_grid,
            state.prefs.viewport.show_axis
        ),
        (grid, !axis),
        "Enter ran another row than the one Down highlighted"
    );
}

#[test]
fn the_palette_says_why_the_highlighted_command_cannot_run() {
    let (ctx, mut state) = a_fresh_palette();
    let drawn = typed(&ctx, &mut state, "undo");
    let undo = drawn
        .iter()
        .find(|(role, label, _)| *role == Role::Button && label.starts_with("Edit: Undo"))
        .unwrap_or_else(|| panic!("no Undo row: {drawn:#?}"));
    assert!(undo.2, "Undo runs with nothing to undo: {undo:?}");
    assert!(
        drawn
            .iter()
            .any(|(_, text, _)| text == "Nothing to undo yet"),
        "the palette does not say why Undo cannot run: {drawn:#?}"
    );
}

#[test]
fn a_command_run_from_the_palette_comes_first_next_time() {
    let (ctx, mut state) = a_fresh_palette();
    let before = rows(&typed(&ctx, &mut state, ""));
    let axis = before
        .iter()
        .position(|label| label == "View: Show axis")
        .unwrap_or_else(|| panic!("no Show axis row: {before:#?}"));
    assert!(axis > 0, "Show axis was first before it ran");

    frame(
        &ctx,
        &mut state,
        vec![egui::Event::Text("show axis".into())],
    );
    frame(&ctx, &mut state, vec![key(egui::Key::Enter)]);
    let after = rows(&typed(&ctx, &mut state, ""));
    assert_eq!(
        after.first().map(String::as_str),
        Some("View: Show axis"),
        "{after:#?}"
    );
}

#[test]
fn letters_scattered_over_a_whole_name_match_nothing_and_enter_then_runs_nothing() {
    let (ctx, mut state) = a_fresh_palette();
    // w, a, i, v and e are all in View: Reset active workspace layout, in that order.
    let drawn = typed(&ctx, &mut state, "waive");
    assert!(
        rows(&drawn).is_empty(),
        "a word that names no command lists one: {:?}",
        rows(&drawn)
    );
    assert!(
        drawn
            .iter()
            .any(|(_, text, _)| text.starts_with("No commands match")),
        "nothing says that no command matches: {drawn:#?}"
    );
    // Running a command closes the palette, so a palette still there after Enter ran nothing.
    let after = frame(&ctx, &mut state, vec![key(egui::Key::Enter)]);
    assert!(
        after
            .iter()
            .any(|(_, text, _)| text.starts_with("No commands match")),
        "Enter on a word that names nothing ran a command: {after:#?}"
    );
    // What a person abbreviates still finds its command: the letters of a name that stay close.
    let found = rows(&typed(&ctx, &mut state, "np"));
    assert!(
        found.iter().any(|row| row.contains("New project")),
        "np no longer finds New project…: {found:#?}"
    );
    let words = rows(&typed(&ctx, &mut state, "open project"));
    assert!(
        words
            .iter()
            .any(|row| row.contains("Open project in new window")),
        "two words of a name no longer find it: {words:#?}"
    );
}
