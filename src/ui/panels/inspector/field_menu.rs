//! Copy and Paste on a row of the grid: the field the row belongs to, a vector or a colour whole.

use crate::state::{AppState, DocumentId};

/// The menu of row `at` of `summary`, read from `doc`, on its label.
pub(super) fn field_menu(
    label: &egui::Response,
    state: &mut AppState,
    doc: DocumentId,
    summary: &or_editor::Summary,
    at: usize,
) {
    label.context_menu(|ui| {
        let name = crate::state::clipboard::field_name(summary, at);
        let mut done = None;
        if ui.button(format!("Copy {name}")).clicked() {
            done = Some(state.copy_field(doc, summary, at));
        }
        if ui
            .add_enabled(
                state.field_to_paste(),
                egui::Button::new(format!("Paste into {name}")),
            )
            .on_disabled_hover_text("Copy a field first")
            .clicked()
        {
            done = Some(state.paste_field(doc, summary, at));
        }
        match done {
            Some(Ok(said)) => state.show_info_toast(said),
            Some(Err(why)) => state.show_error_toast(why),
            None => return,
        }
        ui.close_kind(egui::UiKind::Menu);
    });
}
