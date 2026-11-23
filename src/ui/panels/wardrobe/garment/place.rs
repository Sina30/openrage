//! The place of a garment: its ped, its slot, its skin and the position it takes or is pinned at, as a form with each label beside its field.
//!
//! The ped and the slot are choices that a pick of several garments uses as well, so each is a function of its own.

use or_wardrobe::edit::Edit;
use or_wardrobe::{Garment, Ped, Skin, Slot};

use super::{heading, Asked};
use crate::state::workbench::PackRead;
use crate::ui::panels::wardrobe::words::{ped_words, slot_words};
use crate::ui::theme;

/// The widest a choice of the form is drawn, so that the labels beside it line up.
const CHOICE: f32 = 190.0;

/// The width of the column of labels of the form.
const LABEL: f32 = 60.0;

/// Give a combo box the name a screen reader speaks.
fn named(response: &egui::Response, name: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name));
}

/// The choice of ped: what the person clicked, none while nothing was.
///
/// `many` is none for one garment and, for several, whether they share no ped: a ped is set on all of them and never taken away from all of them, so the choice of several has no `? not said`.
pub(super) fn ped_choice(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    now: Option<Ped>,
    many: Option<bool>,
) -> Option<Option<Ped>> {
    let mixed = many == Some(true);
    let mut chosen = None;
    let combo = egui::ComboBox::from_id_salt(id)
        .width(ui.available_width().min(CHOICE))
        .selected_text(if mixed {
            tr!("Several")
        } else {
            ped_words(now)
        })
        .show_ui(ui, |ui| {
            if many.is_none()
                && ui
                    .selectable_label(!mixed && now.is_none(), tr!("? not said"))
                    .clicked()
            {
                chosen = Some(None);
            }
            for ped in Ped::ALL {
                if ui
                    .selectable_label(!mixed && now == Some(ped), ped_words(Some(ped)))
                    .clicked()
                {
                    chosen = Some(Some(ped));
                }
            }
        });
    named(&combo.response, tr!("Garment ped"));
    chosen
}

/// The choice of slot, as [`ped_choice`] is the choice of ped.
pub(super) fn slot_choice(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    now: Option<Slot>,
    many: Option<bool>,
) -> Option<Option<Slot>> {
    let mixed = many == Some(true);
    let mut chosen = None;
    let said = |slot: Slot| format!("{} ({})", slot_words(slot), slot.name());
    let combo = egui::ComboBox::from_id_salt(id)
        .width(ui.available_width().min(CHOICE))
        .selected_text(match (mixed, now) {
            (true, _) => tr!("Several").to_string(),
            (false, None) => tr!("? not said").to_string(),
            (false, Some(slot)) => said(slot),
        })
        .show_ui(ui, |ui| {
            if many.is_none()
                && ui
                    .selectable_label(!mixed && now.is_none(), tr!("? not said"))
                    .clicked()
            {
                chosen = Some(None);
            }
            for slot in Slot::all() {
                if ui
                    .selectable_label(!mixed && now == Some(slot), said(slot))
                    .clicked()
                {
                    chosen = Some(Some(slot));
                }
            }
        });
    named(&combo.response, tr!("Garment slot"));
    chosen
}

/// A row of the form: `label` in a column of one width, then what `add` draws, so that the fields line up.
///
/// ⚠ Not an `egui::Grid`: a grid new to a frame asks for the frame to be drawn again, and a popup open over the list loses its anchor in the frame the selection changes.
pub(super) fn field<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL, height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(LABEL, height));
                ui.label(label);
            },
        );
        add(ui)
    })
    .inner
}

/// The place of the garment at `at`: each field a row of a form, the label on the left.
pub(super) fn places(
    ui: &mut egui::Ui,
    read: &PackRead,
    at: usize,
    g: &Garment,
    asked: &mut Asked,
) {
    let id = &g.id;
    heading(ui, tr!("Place"));
    field(ui, tr!("Ped"), |ui| {
        if let Some(ped) = ped_choice(ui, "garment-ped", g.ped, None).filter(|p| *p != g.ped) {
            asked.edit = Some((id.clone(), Edit::Ped(ped)));
        }
    });
    field(ui, tr!("Slot"), |ui| {
        if let Some(slot) = slot_choice(ui, "garment-slot", g.slot, None).filter(|s| *s != g.slot) {
            asked.edit = Some((id.clone(), Edit::Slot(slot)));
        }
    });
    if !g.slot.is_some_and(Slot::is_prop) {
        field(ui, tr!("Skin"), |ui| {
            if let Some(skin) = skin_choice(ui, g.skin).filter(|s| *s != g.skin) {
                asked.edit = Some((id.clone(), Edit::Skin(skin)));
            }
        });
    }
    field(ui, tr!("Position"), |ui| position(ui, read, at, g, asked));
}

fn skin_choice(ui: &mut egui::Ui, now: Skin) -> Option<Skin> {
    let mut skin = now;
    let words = |skin: Skin| match skin {
        Skin::Universal => tr!("Universal (u)"),
        Skin::Race => tr!("A texture for each skin (r)"),
    };
    let combo = egui::ComboBox::from_id_salt("garment-skin")
        .width(ui.available_width().min(CHOICE))
        .selected_text(words(now))
        .show_ui(ui, |ui| {
            for choice in [Skin::Universal, Skin::Race] {
                ui.selectable_value(&mut skin, choice, words(choice));
            }
        });
    named(&combo.response, tr!("Garment skin"));
    Some(skin)
}

/// Where the garment stands in its slot: the lowest place no pin holds, or the one it is pinned at.
fn position(ui: &mut egui::Ui, read: &PackRead, at: usize, g: &Garment, asked: &mut Asked) {
    let id = &g.id;
    ui.horizontal(|ui| {
        let mut pinned = g.position.is_some();
        let mut position = g.position.unwrap_or_default();
        let taken = read.allocation.of(at);
        let last = (g.ped).map_or(or_wardrobe::MAX_POSITION, |ped| {
            or_wardrobe::places_of(&read.wardrobe, ped).saturating_sub(1)
        });
        let pin = ui.checkbox(&mut pinned, tr!("Pin at"));
        let value = ui.add_enabled(pinned, egui::DragValue::new(&mut position).range(0..=last));
        value.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::DragValue, pinned, tr!("Position"))
        });
        if !pinned {
            let words = taken.map_or(tr!("no place yet").to_string(), |p| match p.part {
                0 => tr!("automatic, takes {place}", place = format!("{:03}", p.place)),
                n => tr!(
                    "automatic, takes {place}: drawable {position} of {collection}",
                    place = format!("{:03}", p.place),
                    position = format!("{:03}", p.position),
                    collection = read.wardrobe.collection_of(p.ped, n)
                ),
            });
            ui.label(egui::RichText::new(words).small().color(theme::TEXT_SECONDARY));
        }
        if pin.changed() || value.changed() {
            let edit = Edit::Position(pinned.then_some(position));
            asked.edit = Some((id.clone(), edit));
        }
    })
    .response
    .on_hover_text(tr!("Automatic takes the lowest place no pin holds. A pinned garment keeps its place whatever happens around it, which is what a saved outfit points at"));
}
