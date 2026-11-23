//! What a scanned GTA VI drawable's physical section holds beside its geometries, each tint palette
//! drawn as a strip of its colours.

use std::sync::Arc;

use super::*;
use crate::state::gta6_head::{head_lines, head_of};

/// The colours a palette's strip shows at most, and the side of each, in points.
const SWATCHES: usize = 256;
const SWATCH: f32 = 6.0;

pub(super) fn draw(ui: &mut egui::Ui, state: &AppState, doc_id: DocumentId) {
    let Some(head) = (state.document(doc_id))
        .and_then(|d| d.path.as_deref())
        .and_then(head_of)
    else {
        return;
    };
    // Built again only when the body is read again.
    let id = egui::Id::new(("gta6_drawable_head", doc_id));
    let stamp = Arc::as_ptr(&head) as usize;
    type Cached = (usize, Arc<Vec<String>>);
    let lines = match ui.data(|d| d.get_temp::<Cached>(id)) {
        Some((s, lines)) if s == stamp => lines,
        _ => {
            let lines = Arc::new(head_lines(&head));
            ui.data_mut(|d| d.insert_temp(id, (stamp, lines.clone())));
            lines
        }
    };
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.strong(tr!("Physical section"));
    for line in lines.iter() {
        ui.label(line.as_str());
    }
    for palette in head.iter().flat_map(|h| h.palettes.iter().flatten()) {
        swatches(ui, &palette.colours);
    }
}

/// A palette's first colours as squares, wrapped to the panel's width.
fn swatches(ui: &mut egui::Ui, colours: &[[u8; 4]]) {
    let shown = &colours[..colours.len().min(SWATCHES)];
    let fit = (ui.available_width() / SWATCH) as usize;
    let across = fit.clamp(1, shown.len().max(1));
    let rows = shown.len().div_ceil(across);
    let size = egui::vec2(across as f32 * SWATCH, rows as f32 * SWATCH);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    for (k, [r, g, b, _]) in shown.iter().enumerate() {
        let at = egui::vec2((k % across) as f32, (k / across) as f32) * SWATCH;
        let square = egui::Rect::from_min_size(rect.min + at, egui::Vec2::splat(SWATCH));
        painter.rect_filled(square, 0.0, egui::Color32::from_rgb(*r, *g, *b));
    }
}
