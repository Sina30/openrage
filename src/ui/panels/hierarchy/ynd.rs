use super::*;

pub(super) fn draw_ynd(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let Some(DocPayload::Resource(or_io::Asset::PathRegion(region))) =
        state.document(doc_id).map(|d| &d.payload)
    else {
        return;
    };
    let id_root = ui.make_persistent_id(("hierarchy_ynd", doc_id));
    // Vehicle nodes come first; nothing in a node says which it is.
    let vehicle = usize::try_from(region.num_nodes_car_nodes).unwrap_or(0);

    collapsing_row(
        ui,
        id_root.with("nodes"),
        false,
        |ui, _| ui.label(format!("Nodes ({})", region.nodes.len())),
        |ui| {
            for (i, node) in region.nodes.iter().enumerate() {
                let side = if i < vehicle { "vehicle" } else { "pedestrian" };
                let detail = format!(
                    "{side} · ({}, {}, {}) · {} links",
                    node.coors_x,
                    node.coors_y,
                    node.coors_z(),
                    node.num_links()
                );
                leaf_row(ui, |ui| {
                    ui.label(format!("Node {i}"));
                    ui.add(detail_label(detail));
                });
            }
        },
    );
    collapsing_row(
        ui,
        id_root.with("links"),
        false,
        |ui, _| ui.label(format!("Links ({})", region.links.len())),
        |ui| {
            for (i, link) in region.links.iter().enumerate() {
                let to = &link.other_node;
                leaf_row(ui, |ui| {
                    ui.label(format!("Link {i}"));
                    ui.add(detail_label(format!("to {}:{}", to.region, to.index)));
                });
            }
        },
    );
    collapsing_row(
        ui,
        id_root.with("junctions"),
        false,
        |ui, _| ui.label(format!("Junctions ({})", region.virtual_junctions.len())),
        |ui| {
            for (i, junction) in region.virtual_junctions.iter().enumerate() {
                let detail = format!(
                    "{}×{} samples at ({}, {})",
                    junction.x_samples, junction.y_samples, junction.min_x, junction.min_y
                );
                leaf_row(ui, |ui| {
                    ui.label(format!("Junction {i}"));
                    ui.add(detail_label(detail));
                });
            }
        },
    );
}
