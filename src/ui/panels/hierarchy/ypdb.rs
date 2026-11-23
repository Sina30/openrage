use super::*;

pub(super) fn draw_ypdb(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let db: or_io::gta5::pose_db::PoseMatcherDatabase =
        match state.document(doc_id).map(|d| &d.payload) {
            Some(DocPayload::Ypdb(d)) => d.clone(),
            _ => return,
        };

    if db.samples.is_empty() {
        empty_centered(ui, "No samples in this database");
        return;
    }

    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();

    ui.label(
        egui::RichText::new(format!(
            "{} samples · {} bone tags",
            db.samples.len(),
            db.bone_tags.len()
        ))
        .color(theme::TEXT_SECONDARY),
    );
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    for sample in &db.samples {
        // ⚠ A sample names its clip set and clip by hash and nothing else,
        // so a name here would be one this document cannot supply.
        let label = format!("{:08X} / {:08X}", sample.clip_set, sample.clip);
        if filter_active && !label.to_ascii_lowercase().contains(&filter_lc) {
            continue;
        }
        let detail = format!("{:.3}s {}pt", sample.offset, sample.cloud.points.len());
        leaf_row(ui, |ui| {
            let job = icons::icon_label(
                &icons::doc_kind_icon(DocumentKind::Ypdb, false),
                label.clone(),
                theme::TEXT_PRIMARY.get(),
            );
            ui.horizontal(|ui| {
                let r = ui.label(job);
                ui.add(detail_label(detail.clone()));
                r
            })
            .inner
        });
    }
}
