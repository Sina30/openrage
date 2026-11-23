use super::*;

use crate::state::gta6_bound::{child_label, child_row};

/// A scanned GTA VI bound's own row, then one per child, drawing only the rows in view: a bound
/// can hold 65,535 children.
pub(super) fn draw_gta6_bound(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let Some(DocPayload::Gta6Drawable(resource)) = state.document(doc_id).map(|d| &d.payload)
    else {
        return;
    };
    let resource = std::sync::Arc::clone(resource);
    let or_io::gta6::headerless::Resource::Bound(bound) = &*resource else {
        return;
    };
    let count = bound.children.len();
    let mut clicked = None;
    let colour = |selected: bool| {
        if selected {
            theme::ACCENT_TEXT
        } else {
            theme::TEXT_PRIMARY
        }
    };
    let selected = state.selection.is_selected(doc_id, 0);
    let icon = icons::bound_icon(true, false);
    let root = icons::icon_label(
        &icon,
        tr!("Bound (composite)").to_string(),
        colour(selected).into(),
    );
    let response = leaf_row(ui, |ui| {
        let r = ui.selectable_label(selected, root);
        ui.add(detail_label(tr_n!(
            "{count} child bound",
            "{count} child bounds",
            count
        )));
        r
    });
    if response.clicked() {
        clicked = Some(0);
    }
    let row_height = ui.spacing().interact_size.y;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let top = ui.cursor().top();
        let clip = ui.clip_rect();
        let at = |y: f32| (((y - top) / row_height).max(0.0) as usize).min(count);
        let (first, end) = (at(clip.top()), (at(clip.bottom()) + 1).min(count));
        let first = first.min(end);
        ui.add_space(first as f32 * row_height);
        for (index, child) in bound.children[first..end].iter().enumerate() {
            let index = first + index;
            let (name, detail) = child_label(index, child);
            let row = child_row(index);
            let selected = state.selection.is_selected(doc_id, row);
            let geometry = matches!(
                child.bound,
                Some(Ok(or_io::gta6::headerless::ChildBound::Geometry(_)))
            );
            let icon = icons::bound_icon(false, geometry);
            let job = icons::icon_label(&icon, name, colour(selected).into());
            let response = ui
                .horizontal(|ui| {
                    ui.set_height(row_height);
                    ui.add_space(crate::ui::theme::SPACING_WIDE);
                    let r = ui.selectable_label(selected, job);
                    ui.add(detail_label(detail));
                    r
                })
                .inner;
            if response.clicked() {
                clicked = Some(row);
            }
        }
        ui.add_space((count - end) as f32 * row_height);
    });
    if let Some(row) = clicked {
        state.selection.select(doc_id, row);
    }
}
