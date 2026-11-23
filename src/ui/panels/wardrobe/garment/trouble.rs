//! What is wrong with the garment selected, right under its name: the worst finding with the fix and the waiver it offers, and the rest behind Show all.

use egui_phosphor::regular as icon;

use super::Asked;
use crate::state::workbench::{Finding, Place, Severity};
use crate::state::AppState;
use crate::ui::panels::wardrobe::WardrobeUi;
use crate::ui::theme;

/// The findings of garment `id`, the worst first and the others in the order the scan listed them.
pub(super) fn of<'a>(state: &'a AppState, id: &str) -> Vec<&'a Finding> {
    let mut found: Vec<&Finding> = (state.workbench.findings().iter())
        .filter(|f| matches!(&f.place, Place::Garment { id: Some(who), .. } if who == id))
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.severity));
    found
}

/// What is wrong with this garment, with the fix and the waiver each offers.
pub(super) fn problems(
    ui: &mut egui::Ui,
    state: &AppState,
    shown: &mut WardrobeUi,
    id: &str,
    asked: &mut Asked,
) {
    let found = of(state, id);
    let Some(worst) = found.first() else {
        return;
    };
    ui.add_space(theme::SPACING_TIGHT);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(tr!("Problems ({count})", count = found.len()))
                .strong()
                .color(theme::TEXT_SECONDARY),
        );
        if found.len() > 1 {
            let (words, name) = match shown.problems_open {
                true => (
                    tr!("Show the worst only").to_string(),
                    tr!("Show only the worst problem"),
                ),
                false => (
                    tr!("Show all {count}", count = found.len()),
                    tr!("Show all the problems"),
                ),
            };
            let toggle = ui.small_button(words);
            let toggle = crate::ui::accessible_name::named(toggle, name);
            if toggle.clicked() {
                shown.problems_open = !shown.problems_open;
            }
        }
    });
    let listed = match shown.problems_open {
        true => &found[..],
        false => std::slice::from_ref(worst),
    };
    // ⚠ Each finding is its own block: the sentence, wrapped at the panel's width, then its buttons on a line of their own, so a long sentence never pushes a button off the panel.
    for finding in listed {
        ui.horizontal_top(|ui| {
            let (mark, colour) = match finding.severity {
                Severity::Error => (icon::X_CIRCLE, theme::ERROR),
                Severity::Warning => (icon::WARNING, theme::WARNING),
            };
            ui.label(egui::RichText::new(mark).color(colour));
            ui.add(egui::Label::new(&finding.text).wrap());
        });
        if !finding.fixes.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(theme::SPACING_RELAXED);
                for fix in &finding.fixes {
                    if ui.small_button(fix.label()).clicked() {
                        asked.fix = Some(fix.clone());
                    }
                }
            });
        }
        ui.add_space(theme::SPACING_TIGHT);
    }
}
