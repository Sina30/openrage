use super::*;

pub(super) fn draw_ytd(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) {
    if state
        .document(doc_id)
        .and_then(|d| crate::state::textures::legacy_dictionary(&d.payload))
        .is_none()
    {
        return;
    }
    let Some(sel) = selection.filter(|&s| s != DOC_ROOT_SEL) else {
        draw_root_summary(ui, state, doc_id, "ytd_root", Vec::new());
        ui.add_space(crate::ui::theme::SPACING_DEFAULT);
        select_in(ui, state, doc_id, tr_key!("a texture"));
        return;
    };

    let internals = shows_internals(state);
    let Some(payload) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    let Some(dict) = crate::state::textures::legacy_dictionary(payload) else {
        return;
    };
    let name = match texture_name_for_selection(payload, sel) {
        Some(n) => n,
        None => {
            empty(ui, "Selection is no longer valid");
            return;
        }
    };

    let Some(tex) = dict.get(&name) else {
        return;
    };

    ui.heading("Texture");
    ui.separator();
    texture_grid(ui, "ytd_tex", tex, internals);
}

/// A Legacy texture in a modder's words, and its pipeline's flags while internals show.
pub(super) fn texture_grid(
    ui: &mut egui::Ui,
    id: &str,
    tex: &or_io::gta5::gen8::GrcTexture,
    internals: bool,
) {
    use or_io::codec::texture::TextureSource;
    grid(ui, id, |ui| {
        row(ui, "Name", &tex.name);
        row(
            ui,
            "Dimensions",
            &format!("{} \u{00d7} {} px", tex.width, tex.height),
        );
        row(ui, "Format", &format!("{:?}", tex.pixel_format()));
        row(ui, "sRGB", if tex.is_srgb() { "yes" } else { "no" });
        row(ui, "Mip levels", &tex.mip_count.to_string());
        row(
            ui,
            "Memory",
            &or_job::human_bytes(tex.backing_store.len() as u64),
        );
        if internals {
            usage_rows(
                ui,
                tex.resource_type_and_conversion_flags,
                tex.cut_mip_levels,
            );
        }
    });
}

pub(super) fn draw_dds(ui: &mut egui::Ui, state: &AppState, doc_id: crate::state::DocumentId) {
    use or_io::codec::texture::TextureSource;

    let Some(DocPayload::Dds(tex)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    ui.heading("DDS Texture");
    ui.separator();
    let format = tex.pixel_format();
    let (width, height) = tex.stored_size();
    grid(ui, "dds_tex", |ui| {
        row(ui, "Dimensions", &format!("{width} \u{00d7} {height} px"));
        row(ui, "Format", &format!("{format:?}"));
        row(ui, "sRGB", if tex.is_srgb() { "yes" } else { "no" });
        row(
            ui,
            "Memory",
            &or_job::human_bytes(tex.backing_store.len() as u64),
        );
    });
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Mip chain")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "dds_mips", |ui| {
        for level in 0..tex.stored_levels() {
            let (w, h) = format.mip_dimensions(width, height, level);
            row(
                ui,
                &format!("Mip {level}"),
                &format!(
                    "{w}\u{00d7}{h} px, {}",
                    or_job::human_bytes(format.mip_size(w, h) as u64)
                ),
            );
        }
    });
}
