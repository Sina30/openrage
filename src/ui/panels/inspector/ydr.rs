use super::*;

pub(super) fn draw_ydr(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) {
    if !matches!(
        state.document(doc_id).map(|d| &d.payload),
        Some(DocPayload::Resource(or_io::Asset::Drawable(_)))
    ) {
        return;
    }
    let Some(sel) = selection.filter(|&s| s != DOC_ROOT_SEL) else {
        draw_root_summary(ui, state, doc_id, "ydr_root", Vec::new());
        ui.add_space(crate::ui::theme::SPACING_DEFAULT);
        select_in(ui, state, doc_id, tr_key!("an item"));
        return;
    };

    // ⚠ The material and bone editors need the payload mutably, so they are
    // dispatched before the shared immutable borrow below rather than inside it.
    if section_of(sel) == SECT_SHADER {
        super::material::draw_material(ui, state, doc_id, 0, payload_index(sel) as usize);
        return;
    }
    if draw_selected_bone(ui, state, doc_id, Some(sel)) {
        return;
    }

    let internals = shows_internals(state);
    let Some(doc) = state.document(doc_id) else {
        return;
    };
    let DocPayload::Resource(or_io::Asset::Drawable(drawable)) = &doc.payload else {
        return;
    };

    match section_of(sel) {
        SECT_SHADER => draw_ydr_shader(ui, drawable, payload_index(sel) as usize),
        SECT_LIGHT => draw_ydr_light(ui, &drawable.lights, payload_index(sel) as usize),
        SECT_BOUND => draw_ydr_bound(ui, drawable.bound.as_deref(), sel),
        SECT_LOD => draw_ydr_lod_geom(ui, drawable, sel, internals),
        SECT_TEXTURE => draw_ydr_drawable_texture(ui, drawable, sel, internals),
        _ => draw_ydr_lod_geom(ui, drawable, SECT_LOD | sel, internals),
    }
}
