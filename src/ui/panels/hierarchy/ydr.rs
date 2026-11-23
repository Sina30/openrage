use super::*;

pub(super) fn draw_ydr(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let drawable = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::Drawable(d))) => d.clone(),
        _ => return,
    };
    let outline = DrawableOutline::of(&drawable.base, &drawable.lights, drawable.bound.as_deref());

    let id_root = ui.make_persistent_id("hierarchy_ydr");

    leaf_row(ui, |ui| {
        let job = icons::icon_label(
            &icons::doc_kind_icon(DocumentKind::Ydr, false),
            outline.name,
            theme::ACCENT_TEXT.get(),
        );
        ui.add(egui::Label::new(job).selectable(false));
        ui.add(detail_label(format!(
            "Bounds radius: {:.2}",
            outline.radius
        )));
    });

    section_drawable_tree(ui, state, doc_id, &outline, id_root);
}
