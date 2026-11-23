//! A texture of the garment as two lines: its letter and file on one, and what can be done to it, with its tint, under it.

use or_wardrobe::Garment;

use super::{Asked, TextureAsk};
use crate::ui::panels::wardrobe::words::cut_in_the_middle;
use crate::ui::panels::wardrobe::WardrobeUi;
use crate::ui::theme;

/// The letter the game's files give texture `i` of a drawable.
pub(super) fn letter(i: usize) -> char {
    char::from(b'a' + u8::try_from(i).unwrap_or(25).min(25))
}

/// Texture `i` of the garment: its line, then its tools.
pub(super) fn row(
    ui: &mut egui::Ui,
    shown: &mut WardrobeUi,
    g: &Garment,
    i: usize,
    asked: &mut Asked,
) {
    let variation = &g.variations[i];
    let file = variation
        .texture
        .as_deref()
        .and_then(|t| t.rsplit('/').next())
        .unwrap_or(tr!("Empty texture"));
    let front = format!("{}  ", letter(i));
    let line = format!("{front}{file}");
    // ⚠ One line whatever the panel's width, cut in the middle: a texture's file ends in its letter and its tint, which is what tells it from the next, and the whole of it is in the tip.
    // ⚠ The text style that `.small().monospace()` ends in is the monospace one, whose size is not the small one's.
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let room = ui.available_width();
    let shown_file = cut_in_the_middle(file, |text| {
        let galley = ui.painter().layout_no_wrap(
            format!("{front}{text}"),
            font.clone(),
            egui::Color32::WHITE,
        );
        galley.size().x <= room
    });
    ui.add(
        egui::Label::new(
            egui::RichText::new(format!("{front}{shown_file}"))
                .small()
                .monospace(),
        )
        .wrap_mode(egui::TextWrapMode::Extend),
    )
    .on_hover_text(line);
    tools(ui, shown, g, i, asked);
}

/// What can be done to one texture of the garment: put another file in its place, move it to another letter, tint it, export its file or take it out.
fn tools(ui: &mut egui::Ui, shown: &mut WardrobeUi, g: &Garment, i: usize, asked: &mut Asked) {
    let letter = letter(i);
    let variation = &g.variations[i];
    let file = variation.texture.clone();
    ui.horizontal_wrapped(|ui| {
        // An empty texture is filled, and a texture is replaced.
        let (word, tip) = match variation.texture {
            None => (tr!("Fill"), tr!("Put an image or a .ytd in the empty texture: it keeps its letter and what it states, and Undo makes it empty again")),
            Some(_) => (tr!("Replace"), tr!("Put an image or a .ytd in its place: the tint and flags stay and the old file is left where it was, and Undo goes back to it")),
        };
        let replace = ui.small_button(format!("{word}…"));
        replace.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                tr!("{word} texture {letter}", word = word, letter = letter),
            )
        });
        if replace.on_hover_text(tip).clicked() {
            asked.texture = Some((g.id.clone(), TextureAsk::Replace(i)));
        }
        let more = ui.menu_button(tr!("More"), |ui| {
            crate::ui::popup_of_the_shell(ui.ctx());
            let earlier = ui.add_enabled(i > 0, egui::Button::new(tr!("Move earlier")));
            if earlier.clicked() {
                asked.texture = Some((g.id.clone(), TextureAsk::Move(i, file.clone(), i - 1)));
                ui.close_kind(egui::UiKind::Menu);
            }
            let later = ui.add_enabled(i + 1 < g.variations.len(), egui::Button::new(tr!("Move later")));
            if later.clicked() {
                asked.texture = Some((g.id.clone(), TextureAsk::Move(i, file.clone(), i + 1)));
                ui.close_kind(egui::UiKind::Menu);
            }
            if ui
                .add_enabled(file.is_some(), egui::Button::new(tr!("Export…")))
                .clicked()
            {
                asked.texture = Some((g.id.clone(), TextureAsk::Export(i)));
                ui.close_kind(egui::UiKind::Menu);
            }
            if ui.button(tr!("Remove from the garment")).clicked() {
                asked.texture = Some((g.id.clone(), TextureAsk::Remove(i, file.clone())));
                ui.close_kind(egui::UiKind::Menu);
            }
        });
        more.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                tr!("More for texture {letter}", letter = letter),
            )
        });
        more.response
            .on_hover_text(tr!("Move it to another letter, write its file out under the game's name, or take it out of the garment (its file stays)"));

        // The word its file name ends with; empty writes the skin's own.
        ui.label(egui::RichText::new(tr!("Tint")).small().color(theme::TEXT_SECONDARY));
        let mut typed = match &shown.tint {
            Some((who, texture, text)) if *who == g.id && *texture == i => text.clone(),
            _ => variation.tint.clone().unwrap_or_default(),
        };
        let field = ui.add(
            egui::TextEdit::singleline(&mut typed)
                .desired_width(48.0)
                .font(egui::TextStyle::Monospace)
                .hint_text(g.skin.tint()),
        );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                tr!("Tint of texture {letter}", letter = letter),
            )
        });
        field.clone().on_hover_text(tr!("The tint: the word the texture's file name ends with, such as uni or whi. Empty writes the skin's own"));
        if field.changed() {
            shown.tint = Some((g.id.clone(), i, typed.clone()));
        }
        if field.lost_focus() {
            shown.tint = None;
            let chosen = Some(typed.trim().to_ascii_lowercase()).filter(|t| !t.is_empty());
            if chosen != variation.tint {
                asked.tint = Some((g.id.clone(), i, file.clone(), chosen));
            }
        }
    });
}
