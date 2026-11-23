use super::*;

/// One row per clip of a GTA VI animation resource, or per animation where it has no clip, drawing
/// only the rows in view: a dictionary can hold hundreds. A row's selection is what plays.
pub(super) fn draw_gta6_anim(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    use crate::ui::panels::inspector::{gta6_choice_count, gta6_choice_label};
    let Some(DocPayload::Gta6Anim(resource)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    let resource = std::sync::Arc::clone(resource);
    let count = gta6_choice_count(&resource);
    let text = if resource.clips.is_empty() {
        tr_n!("{count} animation", "{count} animations", count)
    } else {
        tr_n!("{count} clip", "{count} clips", count)
    };
    ui.label(egui::RichText::new(text).color(theme::TEXT_SECONDARY));
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    let row_height = ui.spacing().interact_size.y;
    let mut clicked = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let top = ui.cursor().top();
        let clip = ui.clip_rect();
        let at = |y: f32| (((y - top) / row_height).max(0.0) as usize).min(count);
        let (first, end) = (at(clip.top()), (at(clip.bottom()) + 1).min(count));
        let first = first.min(end);
        ui.add_space(first as f32 * row_height);
        for index in first..end {
            let Some((label, seconds)) = gta6_choice_label(&resource, index) else {
                continue;
            };
            let selected = state.selection.is_selected(doc_id, index as u64);
            let response = ui
                .horizontal(|ui| {
                    ui.set_height(row_height);
                    let response = ui.selectable_label(selected, label);
                    ui.add(detail_label(format!("{seconds:.2}s")));
                    response
                })
                .inner;
            if response.clicked() {
                clicked = Some(index);
            }
        }
        ui.add_space((count - end) as f32 * row_height);
    });
    if let Some(index) = clicked {
        state.selection.select(doc_id, index as u64);
    }
}
