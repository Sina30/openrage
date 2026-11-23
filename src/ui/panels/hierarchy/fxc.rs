use super::*;

pub(super) fn draw_fxc(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let doc: crate::state::FxcDocument = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Fxc(d)) => d.clone(),
        _ => return,
    };
    let effect = &doc.effect;

    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();
    let matches = |name: &str| !filter_active || name.to_ascii_lowercase().contains(&filter_lc);

    // ⚠ The blob count comes from scanning the bytes, not from the effect: a
    // program stores its bytecode as an offset and a length into the file,
    // so the archive is the only thing that can say how many blobs are
    // really there and how much of the file they are.
    let blobs = or_io::gta5::shaders::fxc::FxcArchive::parse(&doc.bytes).ok();
    ui.label(
        egui::RichText::new(format!(
            "{} variables · {} programs · {} techniques · {} DXBC blobs",
            effect.variables.len(),
            effect.programs.len(),
            effect.techniques.len(),
            blobs.as_ref().map_or(0, |a| a.len())
        ))
        .color(theme::TEXT_SECONDARY),
    );
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    let row = |ui: &mut egui::Ui, name: String, detail: String| {
        leaf_row(ui, |ui| {
            let job = icons::icon_label(
                &icons::doc_kind_icon(DocumentKind::Fxc, false),
                name,
                theme::TEXT_PRIMARY.get(),
            );
            ui.horizontal(|ui| {
                let r = ui.label(job);
                ui.add(detail_label(detail));
                r
            })
            .inner
        });
    };

    egui::CollapsingHeader::new(format!("Variables ({})", effect.variables.len()))
        .default_open(true)
        .show(ui, |ui| {
            for var in effect.variables.iter().filter(|v| matches(&v.name)) {
                // The slot is what a material's parameter binds to, so it is
                // the one number worth putting beside the name.
                row(
                    ui,
                    var.name.clone(),
                    format!("{:?} s{}", var.var_type, var.slot),
                );
            }
        });

    egui::CollapsingHeader::new(format!("Techniques ({})", effect.techniques.len())).show(
        ui,
        |ui| {
            for tech in effect.techniques.iter().filter(|t| matches(&t.name)) {
                row(
                    ui,
                    tech.name.clone(),
                    format!("{} passes", tech.passes.len()),
                );
            }
        },
    );

    egui::CollapsingHeader::new(format!("Programs ({})", effect.programs.len())).show(ui, |ui| {
        for program in effect.programs.iter().filter(|p| matches(&p.name)) {
            row(
                ui,
                program.name.clone(),
                format!("{} {}b", program.stage.as_str(), program.bytecode_size),
            );
        }
    });

    egui::CollapsingHeader::new(format!("Constant buffers ({})", effect.cbuffers.len())).show(
        ui,
        |ui| {
            for cbuffer in effect.cbuffers.iter().filter(|c| matches(&c.name)) {
                row(ui, cbuffer.name.clone(), format!("{} bytes", cbuffer.size));
            }
        },
    );
}
