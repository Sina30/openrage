//! The model, geometry or bone the Outline selects in a scanned GTA VI drawable, as GTA V's
//! inspector shows a selected part: a heading and a grid of what the reader read of it.

use std::sync::Arc;

use super::*;
use crate::state::gta6_selected::{selected_rows, Rows};

pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &AppState,
    doc_id: DocumentId,
    selection: Option<u64>,
) {
    let (Some(doc), Some(sel)) = (state.document(doc_id), selection) else {
        return;
    };
    // Built again only when the selection or the body changes: a bone's rows read the skeleton.
    let id = egui::Id::new(("gta6_selected_part", doc_id));
    let stamp = (doc.payload_epoch(), sel);
    type Cached = ((u64, u64), Option<Arc<Rows>>);
    let shown = match ui.data(|d| d.get_temp::<Cached>(id)) {
        Some((s, shown)) if s == stamp => shown,
        _ => {
            let shown = selected_rows(doc, sel).map(Arc::new);
            ui.data_mut(|d| d.insert_temp(id, (stamp, shown.clone())));
            shown
        }
    };
    let Some(shown) = shown else {
        return;
    };
    let (heading, rows) = &*shown;
    ui.add_space(crate::ui::theme::SPACING_TIGHT);
    ui.label(
        egui::RichText::new(heading)
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "gta6_selected_part", |ui| {
        for (label, value) in rows {
            row(ui, label, value);
        }
    });
    ui.separator();
}
