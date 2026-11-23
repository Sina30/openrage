//! What sits above the garments: the drop area of an empty pack, and the counts with the chips that narrow the list.

use egui_phosphor::regular as icon;

use super::list::Asked;
use super::{resolve, Chip, WardrobeUi};
use crate::state::front_door::counted;
use crate::state::workbench::PackRead;
use crate::ui::theme;

/// The one sentence of the drop area: what goes on it.
const DROP_HERE: &str = tr_key!("Drop models, textures and cloth here, or folders of them");

const HELP: &str = tr_key!("A model becomes a garment, and the textures and cloth of its place go with it; an image named as a texture is made into one. \
                    The ped and the slot are read from the names, and what a name does not say is marked ?. \
                    Nothing you already have is overwritten, and Undo takes a drop back.");

/// The frame of a drop area: outlined at [`theme::outline`], which reaches 3 to 1, so that it is seen as a place to drop.
fn drop_frame(vertical: i8) -> egui::Frame {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.5_f32, theme::outline()))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_MEDIUM))
        .inner_margin(egui::Margin::symmetric(12, vertical))
}

/// The empty Wardrobe: one drop area with one sentence and the one primary button.
pub(super) fn empty_wardrobe(ui: &mut egui::Ui, asked: &mut Asked) {
    drop_frame(28).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} {}",
                    icon::DOWNLOAD_SIMPLE,
                    crate::i18n::tr(DROP_HERE)
                ))
                .strong(),
            )
            .on_hover_text(crate::i18n::tr(HELP));
            ui.add_space(theme::SPACING_DEFAULT);
            let button = egui::Button::new(
                egui::RichText::new(tr!("Add clothes…")).color(theme::ON_ACCENT.get()),
            )
            .fill(theme::ACCENT.get());
            if ui
                .add(button)
                .on_hover_text(crate::i18n::tr(HELP))
                .clicked()
            {
                asked.pick = true;
            }
        });
    });
}

/// The reason nothing can be dropped on linked clothing.
///
/// ⚠ A pack that has garments draws no drop area: the whole panel takes a drop, and the window draws its own cue while a file is held over it.
pub(super) fn linked_note(ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(tr!(
            "Read here, edited in the project that holds it: nothing can be dropped on it"
        ))
        .small()
        .color(theme::TEXT_SECONDARY),
    );
    ui.add_space(theme::SPACING_TIGHT);
}

/// The counts of the pack, and a chip for the garments that need attention and one for the duplicates; with a filter in force the counts say how many of the pack's garments are listed.
pub(super) fn draw(
    ui: &mut egui::Ui,
    shown: &mut WardrobeUi,
    read: &PackRead,
    picked: &[String],
    linked: bool,
) {
    let Some(cached) = shown.cache.as_ref() else {
        return;
    };
    let summary = cached.summary;
    let (listed, listed_textures) = (summary.listed, summary.listed_textures);
    let mut chip = shown.chip;
    ui.horizontal_wrapped(|ui| {
        let counts = match listed == summary.garments {
            true => format!(
                "{} · {}",
                counted(summary.garments, "garment"),
                counted(summary.textures, "texture")
            ),
            false => tr!("{listed} of {garments} · {listed_textures}", listed = listed, garments = counted(summary.garments, "garment"), listed_textures = counted(listed_textures, "texture")),
        };
        let counts = ui.label(
            egui::RichText::new(counts)
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        let named = tr!("The pack {pack}.", pack = read.wardrobe.pack);
        if !linked {
            counts.on_hover_text(tr!("{named} Drop models, textures and cloth anywhere on this panel to add them. {help} A drop of thousands of files shows how far it has got and can be stopped.", named = named, help = crate::i18n::tr(HELP)));
        } else {
            counts.on_hover_text(named);
        }
        if summary.attention > 0 {
            let words = match summary.attention {
                1 => tr!("1 needs attention").to_string(),
                n => tr!("{n} need attention", n = n),
            };
            let on = chip == Chip::Attention;
            let clicked = ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(format!("? {words}"))
                            .small()
                            .color(theme::WARNING),
                    )
                    .small()
                    .selected(on),
                )
                .on_hover_text(
                    tr!("A ped or a slot the names did not say, an error, or a duplicate. Click to list only these"),
                )
                .clicked();
            if clicked {
                chip = if on { Chip::All } else { Chip::Attention };
            }
        }
        if summary.unread > 0
            && ui
                .small_button(tr!("Say what they are…"))
                .on_hover_text(
                    tr!("Set the ped and the slot of the garments whose names did not say, all together: the ones picked, or all of them. One step of Undo"),
                )
                .clicked()
        {
            resolve::open(shown, resolve::unread(read, picked));
        }
        if summary.duplicates > 0 {
            let on = chip == Chip::Duplicates;
            let clicked = ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(counted(summary.duplicates, "duplicate"))
                            .small()
                            .color(theme::WARNING),
                    )
                    .small()
                    .selected(on),
                )
                .on_hover_text(
                    tr!("Garments of one ped and slot that ship the same model and the same textures as another, found by what the files hold and not by their names. Click to list only these"),
                )
                .clicked();
            if clicked {
                chip = if on { Chip::All } else { Chip::Duplicates };
            }
        }
        if summary.held > 0 {
            ui.label(
                egui::RichText::new(tr!("{places} held", places = counted(summary.held, "place")))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            )
            .on_hover_text(tr!("A drawable with no model keeps its number, as the game's own gaps do"));
        }
    });
    shown.chip = chip;
    ui.add_space(theme::SPACING_TIGHT);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ Falsified by a stroke at the border colour: the place to drop is as faint as a divider.
    #[test]
    fn the_outline_of_a_drop_area_reaches_3_to_1_on_every_panel_in_both_themes() {
        let _serial = theme::serial();
        for choice in [
            crate::state::ThemeChoice::Dark,
            crate::state::ThemeChoice::Light,
        ] {
            let ctx = egui::Context::default();
            theme::apply_theme(
                &ctx,
                &crate::state::AppearancePrefs {
                    theme: choice,
                    ..crate::state::AppearancePrefs::default()
                },
            );
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                drop_frame(8).show(ui, |ui| ui.label("Drop here"));
            });
            let stroke = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) if rect.stroke.width > 0.0 => Some(rect.stroke),
                    _ => None,
                })
                .expect("the frame is outlined");
            for panel in [theme::BG_L0, theme::BG_L1, theme::BG_L2, theme::BG_L3] {
                let ratio = theme::contrast(stroke.color, panel.get());
                assert!(
                    ratio >= theme::GRAPHIC,
                    "{choice:?}: the drop outline is {ratio:.2} to 1 on {panel:?}"
                );
            }
        }
    }
}
