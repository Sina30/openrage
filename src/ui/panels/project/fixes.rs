//! The buttons of a finding, which Problems draws on each row and Properties under the asset it is about, and what each one does.

use crate::state::workbench::{Finding, Fix, Missing};
use crate::state::AppState;

/// The buttons of `finding`: what can be done about it, each with a sentence; the one a garment cannot have is drawn greyed, with why.
///
/// ⚠ `from_the_right` is for a layout that fills from the right, where the list is walked backwards so the buttons still read in order.
pub(super) fn buttons(ui: &mut egui::Ui, finding: &Finding, from_the_right: bool) -> Option<Fix> {
    let mut chosen = None;
    // ⚠ A garment's model cannot be taken off it, which the button that is not there would not have said.
    let model_lost = finding.fixes.iter().any(|what| {
        matches!(
            what,
            Fix::FindFile {
                what: Missing::Model,
                ..
            }
        )
    });
    let greyed = |ui: &mut egui::Ui| {
        if model_lost {
            ui.add_enabled(
                false,
                egui::Button::new(Missing::Model.remove_label()).small(),
            )
            .on_disabled_hover_text(
                tr!("A garment needs a model: Locate… points it at one, or remove the garment in the Wardrobe"),
            );
        }
    };
    if from_the_right {
        greyed(ui);
    }
    let fixes: Vec<&Fix> = match from_the_right {
        true => finding.fixes.iter().rev().collect(),
        false => finding.fixes.iter().collect(),
    };
    for what in fixes {
        if ui
            .small_button(what.label())
            .on_hover_text(what.hover())
            .clicked()
        {
            chosen = Some(what.clone());
        }
    }
    if !from_the_right {
        greyed(ui);
    }
    chosen
}

/// Do what the button `fix` says: a file is taken out or pointed elsewhere at once, and the rest ask what they change first.
pub(super) fn apply(state: &mut AppState, fix: Fix) {
    match fix {
        Fix::Remove(path) => {
            state.remove_from_project(&path);
            state.workbench.want_scan();
        }
        Fix::Locate(old) => state.locate_member_asking(&old),
        Fix::Reid(folder) => state.reid_source(&folder),
        ask @ (Fix::Remedy { .. }
        | Fix::Waive { .. }
        | Fix::FindFile { .. }
        | Fix::DropFile { .. }
        | Fix::Repair { .. }
        | Fix::WaivePlugin { .. }
        | Fix::Plugin { .. }
        | Fix::Materials { .. }) => state.ask_about_fix(&ask),
    }
}
