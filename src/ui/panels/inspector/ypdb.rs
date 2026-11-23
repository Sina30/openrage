use super::*;

pub(super) fn draw_ypdb(
    ui: &mut egui::Ui,
    state: &AppState,
    doc_id: crate::state::DocumentId,
    _selection: Option<u64>,
) {
    let Some(DocPayload::Ypdb(db)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };

    ui.heading("Pose Matcher Database");
    ui.separator();
    grid(ui, "ypdb_basic", |ui| {
        row(ui, "Signature", &format!("{:#010X}", db.signature));
        row(ui, "Samples", &db.samples.len().to_string());
        row(ui, "Cloud points", &db.point_count().to_string());
        row(ui, "Bone tags", &db.bone_tags.len().to_string());
        row(ui, "Weights", &db.weight_set.len().to_string());
        row(ui, "Serializer", &db.serializer_version.to_string());
    });

    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new(
            "Every field it holds is above, and its samples are listed under it \
             in the tree.",
        )
        .small()
        .color(theme::TEXT_SECONDARY),
    );
}
