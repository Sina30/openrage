//! Settings ▸ Editing: how much of a game install OpenRage writes, and what saving and exporting write.

use super::settings_overlay;
use crate::state::{AppState, ExportFixes, RpfOutputMode};
use crate::ui::theme;

pub fn draw_editing_page(ui: &mut egui::Ui, state: &mut AppState) {
    draw_edit_mode_section(ui, state);
    section_break(ui);
    settings_overlay::draw_overlay_section(ui, state);
    section_break(ui);
    draw_saving_section(ui, state);
}

fn section_break(ui: &mut egui::Ui) {
    ui.add_space(theme::SPACING_RELAXED);
    ui.separator();
    ui.add_space(theme::SPACING_TIGHT);
}

/// The mode picker, and the sentence each mode owes the user.
///
/// ⚠ The mode is the *only* place a base-game write can be permitted
/// permanently; the confirmation dialog allows one session at most.
fn draw_edit_mode_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading(tr!("Edit mode"));
    ui.add_space(theme::SPACING_TIGHT);
    ui.label(tr!(
        "How much of a game install OpenRage is allowed to write:"
    ));
    ui.add_space(theme::SPACING_HAIR);
    if let Some(mode) = super::edit_mode::mode_choices(ui, state.prefs.edit_mode) {
        state.set_edit_mode(mode);
    }

    if state.retail_edit_session_allow {
        ui.add_space(theme::SPACING_TIGHT);
        if ui
            .button(tr!("Revoke this session's base-game permission"))
            .on_hover_text(tr!(
                "Confirming a base-game write allows further ones until the edit \
                 mode changes or OpenRage closes. This puts that back."
            ))
            .clicked()
        {
            state.retail_edit_session_allow = false;
            state.show_toast(tr!("Base-game writes will be confirmed again"));
        }
    }

    ui.add_space(theme::SPACING_TIGHT);
    ui.label(
        egui::RichText::new(tr!(
            "Writing outside a game install is always allowed, in every mode."
        ))
        .weak(),
    );
}

/// Every setting that decides what a save or an export writes, in one group.
fn draw_saving_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading(tr!("When saving and exporting"));
    ui.add_space(theme::SPACING_TIGHT);

    ui.label(tr!("Encryption for archives OpenRage saves:"));
    let mut mode = state.prefs.rpf_output_mode;
    let needs_keys = !matches!(mode, RpfOutputMode::Open);
    let combo = egui::ComboBox::from_id_salt("rpf_output_mode")
        .selected_text(mode.label())
        .show_ui(ui, |ui| {
            for option in RpfOutputMode::ALL {
                ui.selectable_value(&mut mode, option, option.label());
            }
        });
    crate::ui::accessible_name::named(
        combo.response,
        tr!("Encryption for archives OpenRage saves"),
    );
    if mode != state.prefs.rpf_output_mode {
        state.prefs.rpf_output_mode = mode;
        crate::state::save_prefs(&state.prefs);
    }
    if needs_keys && !state.keys.has_keys() {
        ui.colored_label(
            theme::WARNING,
            tr!("Warning: this mode requires extracted keys; saves will fall back to OPEN until they are loaded."),
        );
    }
    if matches!(mode, RpfOutputMode::Ng) && state.keys.has_keys() {
        if state.keys.has_encrypt_state() {
            ui.colored_label(
                theme::SUCCESS,
                tr!("NG encrypt state ready — saves will not block on derivation."),
            );
        } else {
            ui.colored_label(
                theme::INFO,
                tr!(
                    "Note: the first NG save derives ~25 MiB of inverse tables (~10 s). \
                 Use 'Pre-derive NG encrypt state' under Advanced on the Keys & \
                 Encryption page to warm it up now if you'd rather not block on the \
                 first save."
                ),
            );
        }
    }

    ui.add_space(theme::SPACING_DEFAULT);
    ui.label(tr!(
        "When OpenRage's export fixes would alter a file's bytes:"
    ));
    let mut choice = state.prefs.export_fixes;
    let combo = egui::ComboBox::from_id_salt("export_fixes")
        .selected_text(choice.label())
        .show_ui(ui, |ui| {
            for option in ExportFixes::ALL {
                ui.selectable_value(&mut choice, option, option.label());
            }
        });
    crate::ui::accessible_name::named(
        combo.response,
        tr!("When export fixes would alter a file's bytes"),
    );
    if choice != state.prefs.export_fixes {
        state.prefs.export_fixes = choice;
        crate::state::save_prefs(&state.prefs);
    }
}
