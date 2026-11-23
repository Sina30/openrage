//! The ped's own parts: the ped without its hair, and every drawable of its own taken off, left as the game dresses it, or swapped for another.
//!
//! ⚠ **Thin by design.** What a slot wears is `Dressed::base_edits`, which the scene reads; this lays the choices out.

use or_wardrobe::dress::BaseEdit;
use or_wardrobe::{Component, Slot};

use super::kit::{framed, menu, tip};
use super::words::{slot_order, slot_words};
use crate::state::AppState;
use crate::ui::theme;

/// The one switch for a ped with no hair: the ped's own hair taken off, for this view only.
pub(super) fn hide_hair(ui: &mut egui::Ui, state: &mut AppState) {
    let ped = state.dressed_ped();
    let dressed = &mut state.workbench.wardrobe.dressed;
    let has = dressed.base_slots(ped).contains(&Component::Hair);
    let mut off = dressed.base_edits.get(&Component::Hair) == Some(&BaseEdit::Off);
    let response = ui
        .add_enabled(has, egui::Checkbox::new(&mut off, tr!("Ped without hair")))
        .on_disabled_hover_text(
            tr!("The ped's own hair is read from the game install, which needs to be chosen in Settings"),
        );
    let response = tip(
        response,
        tr!("Take the ped's own hair off, to see a hat or a head garment on a bare head; for this view only, nothing is written to the pack. What a hat does to the hair in the game is Hide hair under Behaviour in Details"),
    );
    if response.changed() {
        match off {
            true => dressed.base_edits.insert(Component::Hair, BaseEdit::Off),
            false => dressed.base_edits.remove(&Component::Hair),
        };
    }
}

/// How a slot's choice reads: the game's own, or what was asked for in its place.
fn choice(now: Option<BaseEdit>) -> String {
    match now {
        None => tr!("As the game").to_string(),
        Some(BaseEdit::Off) => tr!("Off").to_string(),
        Some(BaseEdit::Drawable(n)) => tr!("Drawable {n}", n = n),
    }
}

/// The ped's own drawables, one slot at a time: left as the game dresses them, taken off, or another drawable of the ped.
pub(super) fn own_parts(ui: &mut egui::Ui, state: &mut AppState) {
    let ped = state.dressed_ped();
    let dressed = &mut state.workbench.wardrobe.dressed;
    let mut slots = dressed.base_slots(ped);
    slots.sort_by_key(|slot| slot_order(Some(Slot::Component(*slot))));
    let opened = menu(ui, tr!("Ped's own parts"), None, |ui| {
        framed(ui);
        if slots.is_empty() {
            ui.label(tr!("The ped's own parts are read from the game install, which needs to be chosen in Settings"));
        }
        egui::Grid::new("dressed_own_parts")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for slot in slots {
                    let count = dressed.base_drawables(ped, slot);
                    let now = dressed.base_edits.get(&slot).copied();
                    let words = slot_words(Slot::Component(slot));
                    ui.label(words);
                    let colour = match now {
                        None => theme::TEXT_SECONDARY.get(),
                        Some(_) => theme::ACCENT_TEXT.get(),
                    };
                    let chooser = egui::ComboBox::from_id_salt(("dressed own", slot.name()))
                        .width(110.0)
                        .selected_text(egui::RichText::new(choice(now)).color(colour))
                        .show_ui(ui, |ui| own_choices(ui, dressed, slot, count));
                    let said = tr!("Ped's own {words}", words = words.to_lowercase());
                    chooser.response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, said.clone())
                    });
                    ui.end_row();
                }
            });
        let edited = !dressed.base_edits.is_empty();
        if ui
            .add_enabled(
                edited,
                egui::Button::new(tr!("Put every part back as the game has it")),
            )
            .on_disabled_hover_text(tr!("Nothing is changed"))
            .clicked()
        {
            dressed.base_edits.clear();
        }
    });
    tip(
        opened.response,
        tr!("Take off a slot of the ped's own, or wear another of its drawables there; a garment worn in a slot comes first"),
    );
}

fn own_choices(
    ui: &mut egui::Ui,
    dressed: &mut crate::state::workbench::Dressed,
    slot: Component,
    count: usize,
) {
    if ui
        .selectable_label(!dressed.base_edits.contains_key(&slot), tr!("As the game"))
        .clicked()
    {
        dressed.base_edits.remove(&slot);
    }
    if ui
        .selectable_label(
            dressed.base_edits.get(&slot) == Some(&BaseEdit::Off),
            tr!("Off"),
        )
        .clicked()
    {
        dressed.base_edits.insert(slot, BaseEdit::Off);
    }
    for n in 0..count.min(u32::MAX as usize) as u32 {
        let now = dressed.base_edits.get(&slot) == Some(&BaseEdit::Drawable(n));
        if ui
            .selectable_label(now, tr!("Drawable {n}", n = n))
            .clicked()
        {
            dressed.base_edits.insert(slot, BaseEdit::Drawable(n));
        }
    }
}
