//! The edit mode as a choice: the rows the menu bar, Settings and a refused write offer.

use super::question;
use crate::state::{AppState, EditMode};

/// The three modes with a sentence each, the current one selected; the one clicked.
pub fn mode_choices(ui: &mut egui::Ui, current: EditMode) -> Option<EditMode> {
    let mut chosen = None;
    for mode in EditMode::ALL {
        if choice(ui, mode.label(), mode.semantics(), mode == current).clicked() {
            chosen = Some(mode);
        }
    }
    chosen
}

/// A mode as a row: its name, and its sentence under it.
pub fn choice(ui: &mut egui::Ui, label: &str, sentence: &str, selected: bool) -> egui::Response {
    let mut text = egui::text::LayoutJob::default();
    egui::RichText::new(label).append_to(
        &mut text,
        ui.style(),
        egui::FontSelection::Default,
        egui::Align::Min,
    );
    egui::RichText::new(format!("\n{sentence}"))
        .small()
        .weak()
        .append_to(
            &mut text,
            ui.style(),
            egui::FontSelection::Default,
            egui::Align::Min,
        );
    let row = egui::Button::selectable(selected, text)
        .wrap()
        .min_size(egui::vec2(ui.available_width(), 0.0));
    ui.add(row)
}

/// The modes a refused write offers, under the menu bar's control for them.
///
/// ⚠ No "don't ask again": what it changes is the mode, which stays in view.
pub fn draw_refused_write(ctx: &egui::Context, state: &mut AppState) {
    let Some(path) = state.refused_write.clone() else {
        return;
    };
    let current = state.prefs.edit_mode;
    let mut chosen = None;
    let mut close = false;
    question(ctx, tr!("The edit mode refused a write"), |ui| {
        ui.set_width(380.0);
        ui.label(egui::RichText::new(path.display().to_string()).monospace());
        ui.add_space(crate::ui::theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(tr!(
                "Choose a mode that permits it, then do it again. The mode is also \
                 at the right of the menu bar and on the Editing page of Settings."
            ))
            .weak(),
        );
        ui.add_space(crate::ui::theme::SPACING_SNUG);
        chosen = mode_choices(ui, current);
        ui.add_space(crate::ui::theme::SPACING_SNUG);
        close = ui.button(tr!("Close")).clicked();
    });
    if let Some(mode) = chosen {
        state.set_edit_mode(mode);
    }
    if chosen.is_some() || close {
        state.refused_write = None;
    }
}
