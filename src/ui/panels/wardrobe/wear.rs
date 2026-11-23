//! The first two rows of the dressed ped's tools: who wears what (the ped, its skin, the outfit, the heels) and which version of the pack is shown.
//!
//! ⚠ **Thin by design.** What is worn is `or_wardrobe::dress`'s and the choices are `Dressed`'s; this lays them out in rows that keep
//! their places, and hands what was asked for back to the state.

use or_wardrobe::dress::Tone;
use or_wardrobe::Ped;

use super::kit::{combo_with, framed, menu, segments, tip, Part};
use super::words::ped_words;
use crate::state::AppState;

/// Why a control of the ped itself is greyed while no game install is chosen.
const NEEDS_THE_GAME: &str = tr_key!("Needs a game install: the ped is drawn from it");

/// Who wears it: the ped, its skin, the outfit and the heels, with the model and the sets of alternates when the pack has them.
pub(super) fn wear(ui: &mut egui::Ui, state: &mut AppState) {
    let ped = state.dressed_ped();
    let selected = state
        .workbench
        .wardrobe
        .selected
        .as_ref()
        .map(|(_, id)| id.clone());
    let has_game = state.prefs.active_game_root().is_some();
    let mut asked_ped = None;
    ui.horizontal_wrapped(|ui| {
        let parts = Ped::ALL.map(|candidate| Part {
            text: ped_words(Some(candidate)),
            chosen: ped == candidate,
            tip: tr!("The ped the garments are worn on"),
            off: None,
        });
        if let Some(at) = segments(ui, &parts) {
            asked_ped = Some(Ped::ALL[at]);
        }
        let dressed = &mut state.workbench.wardrobe.dressed;
        let skin = combo_with(ui, tr!("Skin"), dressed.tone.name(), has_game, None, |ui| {
            for tone in Tone::ALL {
                ui.selectable_value(&mut dressed.tone, tone, tone.name());
            }
        });
        let skin = skin.on_disabled_hover_text(crate::i18n::tr(NEEDS_THE_GAME));
        tip(skin, tr!("The skin tone of the ped's own head"));
        outfit(ui, state, selected.as_ref());
        let dressed = &mut state.workbench.wardrobe.dressed;
        let heels = if dressed.lift != 0.0 {
            tr!("Heels ({lift} mm)", lift = format!("{:+.0}", dressed.lift * 1000.0))
        } else {
            tr!("Heels").to_string()
        };
        tip(
            ui.checkbox(&mut dressed.heels, heels),
            tr!("Raise the ped by a tenth of the value the worn shoes state for heels, as other tools draw it; what the game does with the value is not measured"),
        );
    });
    // ⚠ A row of their own, whole or not at all: the first row has no room for two more drop-downs at the smallest window.
    let has_choices = super::light::alternates_of_the_selected(state).is_some()
        || !super::views::sets_of_the_ped(state).is_empty();
    if has_choices {
        ui.horizontal_wrapped(|ui| {
            super::light::alternate_model(ui, state);
            super::views::alternate_sets(ui, state);
        });
    }
    if let Some(ped) = asked_ped {
        let dressed = &mut state.workbench.wardrobe.dressed;
        dressed.ped = Some(ped);
        dressed.frame_again();
    }
}

/// The outfit: the garments worn together, the garment selected put in or taken out, and the way to take everything off.
fn outfit(ui: &mut egui::Ui, state: &mut AppState, selected: Option<&String>) {
    let dressed = &mut state.workbench.wardrobe.dressed;
    let worn = dressed.outfit.len();
    let name = match worn {
        0 => tr!("Outfit").to_string(),
        n => tr!("Outfit ({n})", n = n),
    };
    let in_outfit = selected.is_some_and(|id| dressed.outfit.contains(id));
    let (mut toggle, mut clear) = (false, false);
    let opened = menu(ui, &name, None, |ui| {
        framed(ui);
        tip(
            ui.checkbox(&mut dressed.outfit_on, tr!("Wear the outfit together")),
            tr!("Wear the garments of the outfit together, with the garment selected on top; off wears the selected garment alone"),
        );
        let label = match in_outfit {
            true => tr!("Take the selected garment out of the outfit"),
            false => tr!("Add the selected garment to the outfit"),
        };
        let add = ui.add_enabled(selected.is_some(), egui::Button::new(label));
        if add
            .on_disabled_hover_text(tr!("Select a garment first"))
            .clicked()
        {
            toggle = true;
            ui.close();
        }
        let none = ui.add_enabled(worn > 0, egui::Button::new(tr!("Take everything off")));
        if none
            .on_disabled_hover_text(tr!("Nothing is in the outfit yet"))
            .clicked()
        {
            clear = true;
            ui.close();
        }
    });
    tip(
        opened.response,
        tr!("The garments worn together: put the garment selected in, take it out, or take everything off"),
    );
    if toggle {
        if let Some(id) = selected {
            dressed.toggle_in_outfit(id);
            dressed.outfit_on = true;
        }
    }
    if clear {
        dressed.clear_outfit();
    }
}

/// Which version of the pack the view shows: the ped without it, with it, or with it as it was before its last change.
pub(super) fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let dressed = &state.workbench.wardrobe.dressed;
    let (original, previous) = (dressed.original, dressed.previous);
    let has_game = state.prefs.active_game_root().is_some();
    let mut asked = None;
    ui.horizontal(|ui| {
        ui.label(tr!("Show"));
        let parts = [
            Part {
                text: tr!("Without the pack"),
                chosen: original,
                tip: tr!("The ped as the game has it, without this pack (A in other clothing tools)"),
                off: (!has_game).then_some(crate::i18n::tr(NEEDS_THE_GAME)),
            },
            Part {
                text: tr!("With the pack"),
                chosen: !original && !previous,
                tip: tr!("The ped wearing the pack's garments as they are now (B in other clothing tools)"),
                off: None,
            },
            Part {
                text: tr!("Before the last change"),
                chosen: previous,
                tip: tr!("The ped wearing the pack as it was before its last change, from the versions the Wardrobe keeps; a pack with no earlier version says so"),
                off: None,
            },
        ];
        asked = segments(ui, &parts);
    });
    match asked {
        Some(0) => {
            let dressed = &mut state.workbench.wardrobe.dressed;
            (dressed.original, dressed.previous) = (true, false);
        }
        Some(1) => {
            let dressed = &mut state.workbench.wardrobe.dressed;
            (dressed.original, dressed.previous) = (false, false);
        }
        Some(_) => state.show_previous_version(),
        None => {}
    }
}
