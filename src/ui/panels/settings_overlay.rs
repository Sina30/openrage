//! Whether a mount reads the install's `mods` overlay at all, and the copies in it the game has outgrown.

use crate::state::AppState;
use crate::ui::theme;

/// ⚠ Drawn beside the edit mode deliberately. `EditMode::Overlay` says the
/// overlay is the writable half of an install; this says whether one is read.
/// A user with the first and not the second edits files the application will
/// not open.
pub fn draw_overlay_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading(tr!("Mods overlay"));
    ui.add_space(theme::SPACING_TIGHT);

    super::settings_overlay_loader::draw(ui, state);

    let mut read_first = state.prefs.read_overlay_first;
    if ui
        .checkbox(
            &mut read_first,
            tr!("Read the install's mods overlay first"),
        )
        .on_hover_text(tr!(
            "Resolved per archive: a mods copy is read where there is one, and \
             the install's own copy everywhere else. A partial overlay is the \
             normal state and is what this handles."
        ))
        .changed()
    {
        state.set_read_overlay_first(read_first);
    }

    ui.add_space(theme::SPACING_TIGHT);
    if read_first {
        ui.colored_label(
            theme::INFO,
            tr!("Archives resolve against the overlay first, one archive at a time."),
        );
    } else {
        ui.colored_label(
            theme::WARNING,
            tr!("The overlay is ignored. Every archive is read from the install as shipped."),
        );
    }
    draw_outgrown(ui, state);
}

/// The overlay copies the game was updated past, each with the refresh that replaces it.
fn draw_outgrown(ui: &mut egui::Ui, state: &mut AppState) {
    if state.overlay_copy.stale.is_empty() {
        return;
    }
    ui.add_space(theme::SPACING_DEFAULT);
    ui.colored_label(
        theme::WARNING,
        tr!("The game was updated since these copies were made, so they hide what the update changed:"),
    );
    let idle = state.prefs.edit_mode.permits_overlay() && state.overlay_copy.running().is_none();
    let mut asked = None;
    for stale in &state.overlay_copy.stale {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(state.overlay_name(&stale.copy)).monospace());
            let refresh = ui
                .add_enabled(idle, egui::Button::new(tr!("Refresh…")))
                .on_hover_text(tr!(
                    "Lists what of yours a refresh would discard, then asks."
                ))
                .on_disabled_hover_text(tr!(
                    "Needs the edit mode \"{mode}\", and no overlay copy running",
                    mode = crate::state::EditMode::Overlay.label()
                ));
            if refresh.clicked() {
                asked = Some(stale.copy.clone());
            }
        });
    }
    if let Some(copy) = asked {
        state.refresh_overlay_copy(&copy);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use or_title::StaleCopy;

    use super::*;
    use crate::state::EditMode;

    /// The "Refresh…" buttons the section draws, with whether each is disabled.
    fn refresh_buttons(state: &mut AppState) -> Vec<bool> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| draw_overlay_section(ui, state));
        let nodes = output
            .platform_output
            .accesskit_update
            .map(|update| update.nodes)
            .unwrap_or_default();
        (nodes.iter())
            .filter(|(_, node)| node.label() == Some("Refresh…"))
            .map(|(_, node)| node.is_disabled())
            .collect()
    }

    #[test]
    fn a_copy_the_game_outgrew_is_listed_with_a_refresh_the_edit_mode_gates() {
        let mut state = AppState::default();
        assert_eq!(refresh_buttons(&mut state), Vec::<bool>::new());

        state.overlay_copy.stale = vec![StaleCopy {
            copy: PathBuf::from("GTAV/mods/update/update.rpf"),
            base: PathBuf::from("GTAV/update/update.rpf"),
        }];
        state.prefs.edit_mode = EditMode::Overlay;
        assert_eq!(refresh_buttons(&mut state), vec![false]);

        state.prefs.edit_mode = EditMode::ReadOnly;
        assert_eq!(
            refresh_buttons(&mut state),
            vec![true],
            "the mode that refuses the overlay refuses the refresh, and the button says so"
        );
    }
}
