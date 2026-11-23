use super::*;

use or_io::gta5::CNavMeshQuadTree;

use crate::ui::panels::selection_ids::{nav_polygon_id, nav_polygon_of};

/// What the polygon menu asked for, applied once the rows are drawn.
enum PolygonEdit {
    Disable(usize),
    Delete(usize),
}

pub(super) fn draw_ynv(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let Some(DocPayload::Resource(or_io::Asset::NavMesh(mesh))) =
        state.document(doc_id).map(|d| &d.payload)
    else {
        return;
    };
    let id_root = ui.make_persistent_id(("hierarchy_ynv", doc_id));
    let mut edit = None;
    let mut picked = None;

    collapsing_row(
        ui,
        id_root.with("sectors"),
        false,
        |ui, _| ui.label("Sectors"),
        |ui| match mesh.quad_tree.as_deref() {
            Some(root) => draw_sector(ui, id_root.with("sector"), root, "Sector".to_string()),
            None => {
                ui.label(egui::RichText::new("No quadtree").color(theme::TEXT_SECONDARY));
            }
        },
    );
    let polys: Vec<_> = mesh
        .polys_array
        .as_deref()
        .map(|p| p.iter().collect())
        .unwrap_or_default();
    let cell = (state.document(doc_id).and_then(|d| d.path.as_deref()))
        .and_then(crate::state::gta6_navmesh::scanned_cell);
    // A pick in the viewport opens the section its row is in
    if revealing(state, doc_id).and_then(nav_polygon_of).is_some() {
        force_open_collapsing(ui.ctx(), id_root.with("polygons"));
    }
    collapsing_row(
        ui,
        id_root.with("polygons"),
        false,
        |ui, _| ui.label(format!("Polygons ({})", polys.len())),
        |ui| {
            // Worked out only while the section is open. A scanned GTA VI cell names polygons past
            // the view's fifteen bits: it marks and counts them.
            let disabled = (cell.as_deref())
                .and_then(|m| crate::state::gta6_navmesh::disabled_polygons(mesh, m))
                .unwrap_or_else(|| mesh.disabled_polygons());
            let links = cell.as_deref().map(crate::state::gta6_navmesh::link_runs);
            for (i, poly) in polys.iter().enumerate() {
                let off = disabled.get(i).copied().unwrap_or(false);
                let special = (links.as_ref())
                    .and_then(|runs| runs.get(i))
                    .map_or(poly.num_special_links(), Vec::len);
                let detail = format!(
                    "{} corners · {} special links{}",
                    poly.num_vertices(),
                    special,
                    if off { " · disabled" } else { "" }
                );
                let row = nav_polygon_id(i);
                let selected = state.selection.is_selected(doc_id, row);
                leaf_row(ui, |ui| {
                    let label = ui.selectable_label(selected, format!("Polygon {i}"));
                    pulse_scroll_into_view(state, doc_id, row, &label);
                    if label.clicked() {
                        picked = Some(row);
                    }
                    label.context_menu(|ui| {
                        let disable = ui.add_enabled(!off, egui::Button::new("Disable polygon"));
                        if disable
                            .on_hover_text("Every edge into and out of it leads nowhere")
                            .on_disabled_hover_text("No edge leads into or out of it")
                            .clicked()
                        {
                            edit = Some(PolygonEdit::Disable(i));
                            ui.close();
                        }
                        let delete = ui.button("Delete polygon");
                        if delete
                            .on_hover_text("Refused, naming what it would break")
                            .clicked()
                        {
                            edit = Some(PolygonEdit::Delete(i));
                            ui.close();
                        }
                    });
                    ui.add(detail_label(detail));
                });
            }
        },
    );
    if let Some(row) = picked {
        state.selection.select(doc_id, row);
    }
    let outcome = match edit {
        Some(PolygonEdit::Disable(i)) => state.disable_navmesh_polygon(doc_id, i),
        Some(PolygonEdit::Delete(i)) => state.delete_navmesh_polygon(doc_id, i),
        None => return,
    };
    match outcome {
        Ok(said) => state.show_toast(said),
        Err(said) => state.show_error_toast(said),
    }
}

/// A quadtree node, named by its path of child indices from the root.
fn draw_sector(ui: &mut egui::Ui, id: egui::Id, node: &CNavMeshQuadTree, name: String) {
    if node.children.iter().all(Option::is_none) {
        let detail = node.leaf_data.as_deref().map_or_else(
            || "empty".to_string(),
            |leaf| {
                format!(
                    "{} polygons, {} cover points",
                    leaf.polys.len(),
                    leaf.cover_points.len()
                )
            },
        );
        leaf_row(ui, |ui| {
            ui.label(name);
            ui.add(detail_label(detail));
        });
        return;
    }
    collapsing_row(
        ui,
        id,
        false,
        |ui, _| ui.label(&name),
        |ui| {
            for (i, child) in node.children.iter().enumerate() {
                if let Some(child) = child.as_deref() {
                    draw_sector(ui, id.with(i), child, format!("{name}.{i}"));
                }
            }
        },
    );
}
