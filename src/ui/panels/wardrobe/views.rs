//! Which sets of alternate variations the dressed ped wears, as the tools offer them.
//!
//! ⚠ Thin by design. The sets in force are `Dressed`'s, and what each does to what is drawn is `or_wardrobe::dress`'s.

use or_wardrobe::dress::Sets;

use super::kit::combo;
use crate::state::AppState;
use crate::ui::guide_links::{self, CHECK_ON_THE_PED};

/// The pack's sets of alternate variations for the ped worn, as their ids and names.
pub(super) fn sets_of_the_ped(state: &AppState) -> Vec<(String, String)> {
    let ped = state.dressed_ped();
    state
        .wardrobe_pack()
        .and_then(|pack| pack.read.as_ref().ok())
        .map(|read| {
            (read.wardrobe.alternates.iter())
                .filter(|set| set.ped == ped)
                .map(|set| (set.id.clone(), set.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// Which sets of alternate variations the ped wears: those a garment worn causes, none, or one of the pack's sets as if its garment were worn.
///
/// Not drawn where the pack has no set for the ped.
pub(super) fn alternate_sets(ui: &mut egui::Ui, state: &mut AppState) {
    let sets = sets_of_the_ped(state);
    if sets.is_empty() {
        return;
    }
    let dressed = &mut state.workbench.wardrobe.dressed;
    let shown = match &dressed.sets {
        Sets::Worn => tr!("As worn").to_string(),
        Sets::Off => tr!("None").to_string(),
        Sets::Only(id) => (sets.iter())
            .find(|(of, _)| of == id)
            .map_or_else(|| tr!("A set").to_string(), |(_, name)| name.clone()),
    };
    let chooser = combo(ui, tr!("Alternates"), &shown, |ui| {
        ui.selectable_value(&mut dressed.sets, Sets::Worn, tr!("As worn"))
            .on_hover_text(tr!("The sets a garment worn causes, as the game applies them: a hat that changes the hair changes it here"));
        ui.selectable_value(&mut dressed.sets, Sets::Off, tr!("None"))
            .on_hover_text(tr!("No set: every garment is worn with its own model"));
        for (id, name) in &sets {
            ui.selectable_value(&mut dressed.sets, Sets::Only(id.clone()), name)
                .on_hover_text(tr!(
                    "Only this set, as if a garment that causes it were worn"
                ));
        }
    });
    guide_links::tip(
        chooser,
        tr!("Which sets of alternate variations change the models the ped wears"),
        CHECK_ON_THE_PED,
    );
}
