//! The folders an external editor saves into, bound to a slot of a texture of the garment: what is saved there becomes the picture of the slot and the preview follows.
//!
//! ⚠ Thin by design: `or-wardrobe::slots` binds and delivers, `state.workbench.wardrobe.textures` watches the folders, and this asks.

use or_wardrobe::slots::{Binding, MapSlot};
use or_wardrobe::Garment;

use crate::state::AppState;
use crate::ui::theme;

/// What the controls asked for, carried out once the panel is drawn.
#[derive(Default)]
pub(super) struct Asked {
    /// Pick a folder for this garment's texture and slot.
    pub bind: Option<(String, usize, MapSlot)>,
    /// Let go of this binding.
    pub unbind: Option<Binding>,
}

/// The letter a texture is told apart by, as the rest of the panel spells it.
fn letter(i: usize) -> char {
    char::from(b'a' + (i % 26) as u8)
}

/// The watched-folders section for garment `g`.
pub(super) fn draw(ui: &mut egui::Ui, state: &AppState, g: &Garment, asked: &mut Asked) {
    if g.variations.is_empty() {
        return;
    }
    let mine: Vec<&Binding> = state
        .workbench
        .wardrobe
        .textures
        .bindings()
        .iter()
        .map(|(_, _, b)| b)
        .filter(|b| b.garment == g.id)
        .collect();
    egui::CollapsingHeader::new(tr!("Watched folders"))
        .id_salt(("watched folders", &g.id, mine.is_empty()))
        .default_open(!mine.is_empty())
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    tr!("A picture an editor saves in a watched folder becomes the diffuse, specular or normal picture of the texture's source, which is recooked at once. The texture must be a source of the pack."),
                )
                .small()
                .color(theme::TEXT_SECONDARY),
            );
            for binding in &mine {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(tr!("{slot} of texture {texture}", slot = binding.slot.label(), texture = letter(binding.texture)))
                        .small(),
                    );
                    if ui
                        .small_button(tr!("Unbind"))
                        .on_hover_text(tr!("Stop watching the folder for this slot; its files are not touched"))
                        .clicked()
                    {
                        asked.unbind = Some((*binding).clone());
                    }
                });
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&binding.folder)
                            .small()
                            .color(theme::TEXT_SECONDARY),
                    )
                    .truncate(),
                )
                .on_hover_text(&binding.folder);
            }
            let id = ui.id().with(("watch_new", &g.id));
            let (mut texture, mut slot): (usize, MapSlot) = ui
                .data(|d| d.get_temp(id))
                .unwrap_or((0, MapSlot::Diffuse));
            texture = texture.min(g.variations.len() - 1);
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt(("watch texture", &g.id))
                    .selected_text(tr!("Texture {texture}", texture = letter(texture)))
                    .show_ui(ui, |ui| {
                        for i in 0..g.variations.len() {
                            ui.selectable_value(&mut texture, i, tr!("Texture {i}", i = letter(i)));
                        }
                    });
                egui::ComboBox::from_id_salt(("watch slot", &g.id))
                    .selected_text(slot.label())
                    .show_ui(ui, |ui| {
                        for each in MapSlot::ALL {
                            ui.selectable_value(&mut slot, each, each.label());
                        }
                    });
                if ui
                    .button(tr!("Watch a folder…"))
                    .on_hover_text(tr!("Pick the folder the editor saves into"))
                    .clicked()
                {
                    asked.bind = Some((g.id.clone(), texture, slot));
                }
            });
            ui.data_mut(|d| d.insert_temp(id, (texture, slot)));
        });
}
