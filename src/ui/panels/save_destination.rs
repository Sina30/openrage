//! The prompt that asks where a save of a document opened from an archive goes.

use std::path::Path;

use super::question;
use crate::platform::file_dialogs;
use crate::state::save_destination::SaveDestination;
use crate::state::{AppState, EditMode};

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let Some(choice) = state.save_destination.clone() else {
        return;
    };
    let kind = state.document(choice.doc_id).map(|d| d.kind);
    // ⚠ An answer the edit mode would refuse is offered as the change of mode it needs, so the question raises no refusal and nobody saves twice.
    let needs = |path: &Option<std::path::PathBuf>| {
        (path.as_deref()).and_then(|path| state.mode_a_write_needs(path))
    };
    let (archive_needs, overlay_needs) = (needs(&choice.archive), needs(&choice.overlay));
    let mut answer: Option<Option<SaveDestination>> = None;
    let mut change: Option<EditMode> = None;
    question(ctx, &tr!("Save {name}", name = choice.name), |ui| {
        ui.set_min_width(420.0);
        ui.label(egui::RichText::new(tr!("Where does {name} go?", name = choice.name)).strong());
        ui.add_space(crate::ui::theme::SPACING_DEFAULT);

        if ui.button(tr!("Back into the archive")).clicked() {
            answer = Some(Some(SaveDestination::Archive));
            change = archive_needs;
        }
        mode_line(ui, archive_needs);
        match &choice.archive {
            Some(path) => path_line(ui, path),
            None => {
                ui.label(
                    egui::RichText::new(tr!(
                        "Its archive is in memory; save that archive to write it."
                    ))
                    .weak(),
                );
            }
        }
        ui.add_space(crate::ui::theme::SPACING_SNUG);

        if ui.button(tr!("As a loose file…")).clicked() {
            if let Some(path) =
                kind.and_then(|k| file_dialogs::save_any_file(k, Some(&choice.name)))
            {
                answer = Some(Some(SaveDestination::LooseFile(path)));
            }
        }
        ui.label(egui::RichText::new(tr!("The archive keeps its bytes.")).weak());
        ui.add_space(crate::ui::theme::SPACING_SNUG);

        let overlay = ui
            .add_enabled(
                choice.overlay.is_some(),
                egui::Button::new(tr!("Into the mods overlay")),
            )
            .on_disabled_hover_text(tr!(
                "Only an archive inside a game install has an overlay copy"
            ));
        if overlay.clicked() {
            answer = Some(Some(SaveDestination::Overlay));
            change = overlay_needs;
        }
        mode_line(ui, overlay_needs);
        if let Some(path) = &choice.overlay {
            path_line(ui, path);
        }

        ui.add_space(crate::ui::theme::SPACING_LOOSE);
        if ui.button(tr!("Cancel")).clicked() {
            answer = Some(None);
        }
    });
    match answer {
        Some(Some(dest)) => {
            if let Some(mode) = change {
                state.set_edit_mode(mode);
            }
            state.choose_save_destination(dest);
        }
        Some(None) => state.cancel_save_destination(),
        None => {}
    }
}

/// Under an answer the edit mode would refuse: the mode it changes to, which choosing the answer does.
fn mode_line(ui: &mut egui::Ui, needs: Option<EditMode>) {
    if let Some(mode) = needs {
        ui.label(
            egui::RichText::new(tr!(
                "This changes the edit mode to {mode}, then saves.",
                mode = mode.label()
            ))
            .small()
            .color(crate::ui::theme::WARNING),
        );
    }
}

fn path_line(ui: &mut egui::Ui, path: &Path) {
    ui.label(egui::RichText::new(path.display().to_string()).monospace());
}
