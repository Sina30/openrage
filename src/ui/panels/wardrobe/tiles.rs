//! The strip under the dressed ped's view: what the preview could not do, and the tiles of the selected garment's textures.
//!
//! ⚠ **Thin by design.** The textures are read by the state; this draws their pictures, full size where there is room and one compact row where
//! there is not, so that the view above is never drawn over them.

use super::WardrobeUi;
use crate::state::workbench::{Thumb, NO_GAME};
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::theme;

/// A variation's tile: its picture, its letter and its file.
const TILE: egui::Vec2 = egui::vec2(96.0, 96.0);

/// A variation's tile in the compact strip: a small picture with its letter and file beside it.
const COMPACT_TILE: egui::Vec2 = egui::vec2(132.0, 40.0);

/// The most characters of a texture's file name a tile shows.
const CAPTION: usize = 15;

/// What a texture with no file is called: its letter is kept, and a texture added later can take it.
const EMPTY: &str = tr_key!("empty texture");

/// What the preview could not do, and the way to mend it where there is one.
pub(super) fn notes(ui: &mut egui::Ui, state: &mut AppState) {
    let dressed = &state.workbench.wardrobe.dressed;
    let retry = dressed.base_missing().is_some();
    // ⚠ The note that no game install is chosen stands over the view, with the button that chooses one.
    let mut lines: Vec<String> = (dressed.notes.iter())
        .filter(|note| note.as_str() != crate::i18n::tr(NO_GAME))
        .take(4)
        .cloned()
        .collect();
    let more = (dressed.notes.iter())
        .filter(|note| note.as_str() != crate::i18n::tr(NO_GAME))
        .count()
        .saturating_sub(4);
    lines.extend(dressed.clip_note.clone());
    let mut again = false;
    ui.horizontal_wrapped(|ui| {
        for line in &lines {
            ui.label(
                egui::RichText::new(line)
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
        if more > 0 {
            ui.label(
                egui::RichText::new(tr!("and {more} more", more = more))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
        if retry {
            again = ui.small_button(tr!("Try the install again")).clicked();
        }
    });
    if again {
        state.workbench.wardrobe.dressed.retry_base();
    }
}

/// The tiles of the selected garment's textures: click to wear one, drop an image on one to make its texture from it.
pub(super) fn variants(
    ui: &mut egui::Ui,
    state: &mut AppState,
    shown: &mut WardrobeUi,
    compact: bool,
) {
    shown.variants = (ui.ctx().cumulative_pass_nr(), Vec::new());
    let Some((pack, at)) = state.selected_garment() else {
        ui.label(
            egui::RichText::new(tr!("Select a garment to see its textures"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        return;
    };
    let Ok(read) = &pack.read else { return };
    let garment = &read.wardrobe.garments[at];
    let id = garment.id.clone();
    if garment.variations.is_empty() {
        return;
    }
    let wearing = state.dressed_variation(&id);
    let linked = pack.linked;
    let reading = state.workbench.wardrobe.dressed.is_reading();
    let mut chosen = None;
    egui::ScrollArea::horizontal()
        .id_salt("dressed_variants")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, variation) in garment.variations.iter().enumerate() {
                    let letter = char::from(b'a' + u8::try_from(i).unwrap_or(25).min(25));
                    let file = variation.texture.as_deref().map(|t| pack.folder.join(t));
                    let name = file.as_ref().and_then(|f| f.file_name()).map_or_else(
                        || crate::i18n::tr(EMPTY).to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    let thumb = file
                        .as_deref()
                        .and_then(|f| state.workbench.wardrobe.dressed.thumb(f))
                        .cloned();
                    let absent = match (&file, reading) {
                        (None, _) => crate::i18n::tr(EMPTY),
                        (Some(_), true) => tr!("reading…"),
                        (Some(_), false) => tr!("no picture"),
                    };
                    let size = if compact { COMPACT_TILE } else { TILE };
                    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                    let tile = Tile {
                        rect,
                        worn: i == wearing,
                        letter,
                        compact,
                    };
                    paint_tile(ui, shown, &tile, &name, thumb.as_deref(), absent);
                    let size = thumb.as_ref().map_or_else(String::new, |t| {
                        tr!(
                            ", {width} by {height}",
                            width = t.full[0],
                            height = t.full[1]
                        )
                    });
                    let drop = if linked {
                        tr!("It is linked from another project, which alone edits it").to_string()
                    } else {
                        tr!("Drop an image on it to make this texture from the image").to_string()
                    };
                    let tip = tr!(
                        "Texture {letter}: {name}{size}. Click to wear it. {drop}",
                        letter = letter,
                        name = name,
                        size = size,
                        drop = drop
                    );
                    let response = named(response, &tip).on_hover_text(&tip);
                    if response.clicked() {
                        chosen = Some(i);
                    }
                    shown.variants.1.push((rect, id.clone(), i));
                }
            });
        });
    if let Some(i) = chosen {
        state.workbench.wardrobe.dressed.variation.insert(id, i);
    }
}

/// Where a tile stands and what it says of its texture.
#[derive(Clone, Copy)]
struct Tile {
    rect: egui::Rect,
    worn: bool,
    letter: char,
    compact: bool,
}

/// The part of a texture's file name that tells it apart: after the garment's own name, the extension left off, and the front of a long one elided.
fn caption(name: &str) -> String {
    let stem = name.split('.').next().unwrap_or(name);
    let tail = stem.rsplit('^').next().unwrap_or(stem);
    let n = tail.chars().count();
    match n > CAPTION {
        true => format!(
            "…{}",
            tail.chars().skip(n - (CAPTION - 1)).collect::<String>()
        ),
        false => tail.to_string(),
    }
}

fn paint_tile(
    ui: &mut egui::Ui,
    shown: &mut WardrobeUi,
    tile: &Tile,
    name: &str,
    thumb: Option<&Thumb>,
    absent: &str,
) {
    let Tile {
        rect,
        worn,
        letter,
        compact,
    } = *tile;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, theme::RADIUS_SMALL, theme::BG_L2);
    let stroke = if worn {
        egui::Stroke::new(2.0_f32, theme::ACCENT.get())
    } else {
        egui::Stroke::new(1.0_f32, theme::BORDER)
    };
    painter.rect_stroke(rect, theme::RADIUS_SMALL, stroke, egui::StrokeKind::Inside);
    let (picture, side) = match compact {
        true => (
            egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 4.0), egui::vec2(32.0, 32.0)),
            32.0,
        ),
        false => (
            egui::Rect::from_min_size(rect.min + egui::vec2(16.0, 6.0), egui::vec2(64.0, 64.0)),
            64.0,
        ),
    };
    match thumb {
        Some(thumb) => {
            let texture = tile_texture(ui.ctx(), &mut shown.tiles, thumb);
            let [w, h] = thumb.size;
            let fit = egui::Vec2::new(w as f32, h as f32) * (side / w.max(h) as f32);
            let at = egui::Rect::from_center_size(picture.center(), fit);
            painter.image(
                texture,
                at,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(picture, 2.0, theme::BG_L1);
            if !compact {
                painter.text(
                    picture.center(),
                    egui::Align2::CENTER_CENTER,
                    absent,
                    egui::FontId::proportional(11.0),
                    theme::TEXT_PRIMARY.get(),
                );
            }
        }
    }
    let words = match thumb {
        None if name == crate::i18n::tr(EMPTY) => crate::i18n::tr(EMPTY).to_string(),
        _ => caption(name),
    };
    if compact {
        painter.text(
            rect.min + egui::vec2(42.0, 4.0),
            egui::Align2::LEFT_TOP,
            letter.to_string(),
            egui::FontId::monospace(13.0),
            theme::TEXT_PRIMARY.get(),
        );
        let said = match thumb {
            None => absent.to_string(),
            Some(_) => words,
        };
        painter.text(
            rect.min + egui::vec2(42.0, 21.0),
            egui::Align2::LEFT_TOP,
            said,
            egui::FontId::proportional(11.0),
            theme::TEXT_PRIMARY.get(),
        );
        return;
    }
    painter.text(
        picture.left_top() + egui::vec2(3.0, 1.0),
        egui::Align2::LEFT_TOP,
        letter.to_string(),
        egui::FontId::monospace(12.0),
        theme::TEXT_PRIMARY.get(),
    );
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 4.0),
        egui::Align2::CENTER_BOTTOM,
        words,
        egui::FontId::proportional(11.0),
        theme::TEXT_PRIMARY.get(),
    );
}

/// The picture of `thumb` as a texture of the screen, made once for the pixels it was decoded from.
pub(super) fn tile_texture(
    ctx: &egui::Context,
    tiles: &mut std::collections::HashMap<u64, egui::TextureHandle>,
    thumb: &Thumb,
) -> egui::TextureId {
    tiles
        .entry(thumb.id)
        .or_insert_with(|| {
            ctx.load_texture(
                format!("dressed-tile-{}", thumb.id),
                egui::ColorImage::from_rgba_unmultiplied(thumb.size, &thumb.rgba),
                egui::TextureOptions::LINEAR,
            )
        })
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caption_keeps_the_part_that_tells_the_textures_of_a_garment_apart() {
        let a = caption("mp_m_freemode_01_mp_m_unity_clothes^jbib_diff_007_a_uni.ytd");
        let b = caption("mp_m_freemode_01_mp_m_unity_clothes^jbib_diff_007_b_uni.textures.src");
        assert_eq!(a, "…diff_007_a_uni");
        assert_ne!(a, b, "two textures of one garment read apart");
        assert_eq!(caption(crate::i18n::tr(EMPTY)), crate::i18n::tr(EMPTY));
        assert_eq!(caption("short.ytd"), "short");
    }
}
