use super::*;

pub(super) fn draw_fxc(
    ui: &mut egui::Ui,
    state: &AppState,
    doc_id: crate::state::DocumentId,
    _selection: Option<u64>,
) {
    let Some(DocPayload::Fxc(doc)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    let effect = &doc.effect;

    ui.heading("Shader Effect");
    ui.separator();
    grid(ui, "fxc_basic", |ui| {
        row(ui, "Vertex type", &format!("{:#010X}", effect.vertex_type));
        if let Some(bucket) = effect.draw_bucket() {
            row(ui, "Draw bucket", &bucket.to_string());
        }
        row(ui, "Presets", &effect.presets.len().to_string());
        row(ui, "Variables", &effect.variables.len().to_string());
        row(ui, "Constant buffers", &effect.cbuffers.len().to_string());
        row(ui, "Techniques", &effect.techniques.len().to_string());
        row(ui, "Programs", &effect.programs.len().to_string());
        // A `.fxc` has no table of contents, so consuming the file exactly is
        // the only thing that says the parse landed on the right fields.
        match or_io::gta5::shaders::fxc::GrcEffect::trailing_bytes(&doc.bytes) {
            Ok(0) => row(ui, "Consumed", "every byte"),
            Ok(n) => row(ui, "Consumed", &format!("all but {n} byte(s)")),
            Err(_) => {}
        }
    });

    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new(
            "Read-only. The programs are fxc.exe's output; OpenRage bakes \
             shaders rather than rewriting them, so a .fxc has no save path.",
        )
        .small()
        .color(theme::TEXT_SECONDARY),
    );
}
