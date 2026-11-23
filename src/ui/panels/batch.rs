//! Batch: one command over the rows selected in Files or the results of a search, as one job, and the command line that does the same over a whole install.

use crate::state::batch::Batch;
use crate::state::navigation::Arrival;
use crate::state::AppState;
use crate::ui::commands::{self, Action, Command, Hits, Target};
use crate::ui::panel_state::PanelState;
use crate::ui::theme;

/// Whether the window is up.
#[derive(Default)]
pub struct BatchWindow {
    open: bool,
}

impl PanelState for BatchWindow {
    const KEY: &'static str = "batch-window";
}

pub fn open(state: &mut AppState) {
    state.panels.get::<BatchWindow>().open = true;
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    if !state.panels.get::<BatchWindow>().open {
        return;
    }
    let mut keep_open = true;
    egui::Window::new(tr!("Batch"))
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        // ⚠ In the middle of the window: where a window is put with no place given is its corner, over the rail and the tabs.
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut keep_open)
        .show(ctx, |ui| body(ui, state));
    if !keep_open {
        state.panels.get::<BatchWindow>().open = false;
    }
}

/// The two exports a batch adds to what the table already runs over a selection, each with what its button says and why it waits.
fn exports() -> [(Batch, &'static str, &'static str); 2] {
    [
        (
            Batch::Gltf,
            tr!("Export models to glTF…"),
            tr!("No model is among the rows selected"),
        ),
        (
            Batch::Pictures,
            tr!("Export textures to PNG…"),
            tr!("No texture dictionary is among the rows selected"),
        ),
    ]
}

fn body(ui: &mut egui::Ui, state: &mut AppState) {
    let over = match commands::selected(state) {
        Ok(Target::Rows { rows, .. }) if rows.is_empty() => None,
        Ok(over @ (Target::Rows { .. } | Target::Hits { .. })) => Some(over),
        _ => None,
    };
    match &over {
        Some(Target::Hits { rows, .. }) => {
            ui.label(tr!("{count} result(s) selected", count = rows.len()))
        }
        Some(Target::Rows { rows, .. }) => {
            ui.label(tr!("{count} row(s) selected in Files", count = rows.len()))
        }
        _ => ui.label(
            egui::RichText::new(tr!(
                "Select files, folders or results first: a batch runs over what is selected"
            ))
            .color(theme::TEXT_SECONDARY),
        ),
    };
    ui.add_space(theme::SPACING_TIGHT);

    // The two that are commands of the table run over the selection as they do from its menu.
    let mut asked: Option<Command> = None;
    for action in [Action::Extract, Action::Convert] {
        if let Some(command) = commands::of(state, action) {
            let button = egui::Button::new(&command.label);
            let response = ui.add_enabled(command.enabled.is_ok(), button);
            if commands::hints(response, &command).clicked() {
                asked = Some(command);
            }
        }
    }
    let mut export = None;
    for (kind, label, none) in exports() {
        // ⚠ `None` for results: what each is shows only once its archive is read, which the export does.
        let takes = match &over {
            Some(Target::Rows { doc, rows, .. }) => {
                Some(state.batch_inputs(*doc, rows, kind).len())
            }
            Some(_) => None,
            None => Some(0),
        };
        let text = match takes {
            Some(0) | None => label.to_string(),
            Some(takes) => format!("{label} ({takes})"),
        };
        let button = ui
            .add_enabled(takes != Some(0), egui::Button::new(text))
            .on_disabled_hover_text(none);
        if button.clicked() {
            export = Some(kind);
        }
    }

    // What does the first of them over a whole install, for a terminal or a script.
    if let Some(line) = commands::of(state, Action::Extract).and_then(|command| command.cli) {
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(tr!("Over a whole install, from a terminal:"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&line).monospace().small());
            if ui.small_button(tr!("Copy as command")).clicked() {
                ui.ctx().copy_text(line.clone());
            }
        });
    }

    if let Some(command) = asked {
        commands::run(state, ui.ctx(), &command);
    }
    let (Some(kind), Some(over)) = (export, over) else {
        return;
    };
    let Some(folder) = crate::platform::file_dialogs::pick_folder(tr!("Export into a folder"))
    else {
        return;
    };
    match over {
        Target::Rows { doc, rows, .. } => state.batch_export(doc, &rows, kind, folder),
        Target::Hits { list, rows } => {
            let then = Arrival::Batch { kind, folder };
            match list {
                Hits::Search => state.go_to_search_hits(&rows, then),
                Hits::Inside(doc) => state.go_to_deep_find_matches(doc, &rows, then),
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_opens_from_its_command_and_names_both_exports() {
        let mut state = AppState::from_prefs(crate::state::Prefs::default());
        let ctx = egui::Context::default();
        commands::run_action(&mut state, &ctx, Action::Batch);
        assert!(state.panels.get::<BatchWindow>().open);
        let kinds: Vec<Batch> = exports().iter().map(|(kind, _, _)| *kind).collect();
        assert_eq!(kinds, [Batch::Gltf, Batch::Pictures]);
    }
}
