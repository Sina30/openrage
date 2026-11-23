use super::*;

use or_io::gta5::PhBoundAny;

pub(super) fn draw_ybn(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) {
    let child_sel = selection.filter(|&s| s != DOC_ROOT_SEL && s != root_selection_id());

    if !matches!(
        state.document(doc_id).map(|d| &d.payload),
        Some(DocPayload::Resource(or_io::Asset::Bounds(_)))
    ) {
        return;
    }
    if child_sel.is_none() {
        draw_root_summary(ui, state, doc_id, "ybn_root", Vec::new());
        return;
    }

    let Some(doc) = state.document(doc_id) else {
        return;
    };
    let DocPayload::Resource(or_io::Asset::Bounds(bound)) = &doc.payload else {
        return;
    };
    let target = if let Some(sel) = child_sel {
        let mut idx: u64 = 0;
        find_child_bound(bound, sel, &mut idx).unwrap_or(bound)
    } else {
        bound
    };

    ui.heading("Physics Bound");
    ui.separator();
    draw_bound_grid(ui, "ybn_basic", target);
}

pub(super) fn draw_bound_grid(ui: &mut egui::Ui, id: &str, target: &PhBoundAny) {
    let header = crate::render::bound_meshes::header(target);
    grid(ui, id, |ui| {
        row(
            ui,
            "Type",
            &crate::ui::panels::hierarchy::bound_type_name(target),
        );
        row(
            ui,
            "Radius",
            &format!("{:.4}", header.radius_around_centroid),
        );
        let c = crate::render::bound_meshes::header_centroid(header);
        row(
            ui,
            "Centroid",
            &format!("({:.3}, {:.3}, {:.3})", c.x, c.y, c.z),
        );
        if let Some(g) = crate::ui::panels::hierarchy::bound_geometry(target) {
            row(ui, "Vertices", &g.compressed_vertices.len().to_string());
            row(ui, "Polygons", &g.polygons.len().to_string());
        }
        if let PhBoundAny::PhBoundComposite(c) = target {
            row(ui, "Children", &c.bounds.len().to_string());
        }
    });
}

/// The bound a hierarchy row numbered `target`, counting depth first from the root.
pub(super) fn find_child_bound<'a>(
    b: &'a PhBoundAny,
    target: u64,
    idx: &mut u64,
) -> Option<&'a PhBoundAny> {
    if *idx == target {
        return Some(b);
    }
    *idx += 1;
    if let PhBoundAny::PhBoundComposite(comp) = b {
        for child in comp.bounds.iter().flatten() {
            if let Some(found) = find_child_bound(child, target, idx) {
                return Some(found);
            }
        }
    }
    None
}
