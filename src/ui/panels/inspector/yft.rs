use super::*;

pub(super) fn draw_yft(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) {
    if !matches!(
        state.document(doc_id).map(|d| &d.payload),
        Some(DocPayload::Resource(or_io::Asset::Fragment(_)))
    ) {
        return;
    }
    // A bone's fields lead, as a drawable's part does, and not wait below the whole structure.
    if draw_selected_bone(ui, state, doc_id, selection) {
        return;
    }
    draw_root_summary(ui, state, doc_id, "yft_root", Vec::new());
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    if selection.filter(|&s| s != DOC_ROOT_SEL).is_none() {
        select_in(ui, state, doc_id, tr_key!("an item"));
        return;
    }
    empty(
        ui,
        "Selection details reuse the drawable section views above.",
    );
}
