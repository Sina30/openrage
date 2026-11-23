//! A hint that names a key names the one the user bound, not the default.

use crate::state::{AppState, PersistedBinding, Prefs};

/// Each text one frame of `draw` wrote: labels, and the values of plain text.
fn written(state: &mut AppState, draw: fn(&mut egui::Ui, &mut AppState)) -> Vec<String> {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut texts = Vec::new();
    for _ in 0..2 {
        let output = ctx.run_ui(egui::RawInput::default(), |ui| draw(ui, state));
        texts = output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| node.label().or_else(|| node.value()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
    }
    texts
}

fn bound_to(prefs: &mut Prefs, id: &str, ctrl: bool, shift: bool, key: &str) {
    prefs.shortcut_bindings.insert(
        id.to_string(),
        PersistedBinding {
            control: false,
            ctrl,
            shift,
            alt: false,
            key: key.to_string(),
        },
    );
}

#[test]
fn the_status_bar_names_the_key_open_is_bound_to() {
    let mut prefs = Prefs::default();
    bound_to(&mut prefs, "open", true, true, "O");
    let mut state = AppState::from_prefs(prefs);
    state.documents.clear();
    state.active_document = None;

    let shown = written(&mut state, crate::ui::panels::status_bar::draw);
    let hint = shown
        .iter()
        .find(|text| text.starts_with("No document open"))
        .unwrap_or_else(|| panic!("the status bar says nothing of opening a file: {shown:?}"));
    assert_eq!(
        hint,
        &format!(
            "No document open — File ▸ Open file… ({}+Shift+O)",
            crate::ui::shortcuts::primary_modifier_name()
        )
    );
}

#[test]
fn the_palette_names_the_key_the_text_editors_find_is_bound_to() {
    let mut prefs = Prefs::default();
    bound_to(&mut prefs, "find_in_document", true, false, "G");
    let mut state = AppState::from_prefs(prefs);
    crate::ui::panels::command_palette::open(&mut state);

    let shown = written(&mut state, |ui, state| {
        crate::ui::panels::command_palette::draw(ui.ctx(), state)
    });
    assert!(
        shown.iter().any(|text| *text
            == format!(
                "Text: Find in current document {}+G",
                crate::ui::shortcuts::primary_modifier_name()
            )),
        "the palette does not name Find's binding as bound: {shown:#?}"
    );
}
