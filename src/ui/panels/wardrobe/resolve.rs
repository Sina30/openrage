//! The dialog that says what the garments with an unreadable name are: their ped and their slot, for all of the selection in one step.
//!
//! ⚠ **Never in the way of a drop.** A garment whose name states no ped or slot is added with a `?` and is fixed here or in
//! Garment; the dialog opens when somebody asks, and answers for the garments picked, never for the ones whose names were read.

use or_wardrobe::rules::name_of;
use or_wardrobe::{Ped, Slot};

use super::words::{ped_words, slot_order, slot_words};
use super::WardrobeUi;
use crate::state::front_door::counted;
use crate::state::workbench::PackRead;
use crate::state::AppState;

/// The most names the dialog lists before it says how many more there are.
const LISTED: usize = 6;

/// What the dialog has, between frames.
pub(super) struct Dialog {
    /// The garments it answers for, in the pack's order.
    ids: Vec<String>,
    /// What was chosen; none keeps what each garment's name stated.
    ped: Option<Ped>,
    slot: Option<Slot>,
}

/// The garments whose names left a ped or a slot unsaid: those of `picked` where any is, else every one in the pack.
pub(super) fn unread(read: &PackRead, picked: &[String]) -> Vec<String> {
    let unsaid = |g: &&or_wardrobe::Garment| g.ped.is_none() || g.slot.is_none();
    let mut among: Vec<String> = (read.wardrobe.garments.iter())
        .filter(unsaid)
        .filter(|g| picked.contains(&g.id))
        .map(|g| g.id.clone())
        .collect();
    if among.is_empty() {
        among = (read.wardrobe.garments.iter())
            .filter(unsaid)
            .map(|g| g.id.clone())
            .collect();
    }
    among
}

/// Open the dialog for the garments `ids`, or leave it as it is where there are none.
pub(super) fn open(shown: &mut WardrobeUi, ids: Vec<String>) {
    if !ids.is_empty() {
        shown.resolving = Some(Dialog {
            ids,
            ped: None,
            slot: None,
        });
    }
}

pub(super) fn draw(ctx: &egui::Context, state: &mut AppState, shown: &mut WardrobeUi) {
    let Some(mut dialog) = shown.resolving.take() else {
        return;
    };
    let Some(read) = state.wardrobe_pack().and_then(|p| p.read.as_ref().ok()) else {
        return;
    };
    // What is gone from the pack since is not asked about; with none left there is nothing to say.
    dialog
        .ids
        .retain(|id| read.wardrobe.garments.iter().any(|g| g.id == *id));
    if dialog.ids.is_empty() {
        return;
    }
    let names: Vec<String> = (dialog.ids.iter())
        .filter_map(|id| read.wardrobe.garment(id))
        .map(name_of)
        .collect();
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    egui::Window::new(tr!("Say what these are"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.label(tr!("The names of {count} do not say everything. What you choose here is set on all of them as one step, and Undo takes it back.", count = counted(dialog.ids.len(), "garment")));
            ui.add_space(crate::ui::theme::SPACING_TIGHT);
            for name in names.iter().take(LISTED) {
                ui.label(egui::RichText::new(name).small().monospace());
            }
            if names.len() > LISTED {
                ui.label(egui::RichText::new(tr!("and {count} more", count = names.len() - LISTED)).small());
            }
            ui.add_space(crate::ui::theme::SPACING_SNUG);
            ui.horizontal(|ui| {
                ui.label(tr!("Ped"));
                ui.selectable_value(&mut dialog.ped, None, tr!("As the name says"));
                for ped in Ped::ALL {
                    ui.selectable_value(
                        &mut dialog.ped,
                        Some(ped),
                        tr!("{ped} ped", ped = ped_words(Some(ped))),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr!("Slot"));
                let chosen = match dialog.slot {
                    Some(slot) => format!("{} ({})", slot_words(slot), slot.name()),
                    None => tr!("As the name says").to_string(),
                };
                let combo = egui::ComboBox::from_id_salt("wardrobe_resolve_slot")
                    .selected_text(chosen)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut dialog.slot, None, tr!("As the name says"));
                        // Tops first, as the list has them, which is what most packs are for.
                        let mut slots: Vec<Slot> = Slot::all().collect();
                        slots.sort_by_key(|slot| slot_order(Some(*slot)));
                        for slot in slots {
                            let words = format!("{} ({})", slot_words(slot), slot.name());
                            ui.selectable_value(&mut dialog.slot, Some(slot), words);
                        }
                    });
                combo.response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, tr!("Slot to set"))
                });
            });
            ui.add_space(crate::ui::theme::SPACING_SNUG);
            ui.horizontal(|ui| {
                let chosen = dialog.ped.is_some() || dialog.slot.is_some();
                let button = ui
                    .add_enabled(chosen, egui::Button::new(tr!("Apply")))
                    .on_disabled_hover_text(tr!("Choose a ped or a slot first"));
                apply = button.clicked();
                cancel = ui.button(tr!("Cancel")).clicked();
            });
        });
    if apply {
        state.resolve_garments(dialog.ids, dialog.ped, dialog.slot);
    } else if open && !cancel {
        shown.resolving = Some(dialog);
    }
}
