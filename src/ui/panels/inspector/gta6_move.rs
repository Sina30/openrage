//! A GTA VI MoVE network's nodes from the root, each opening to its fields and to the nodes it
//! leads to, as GTA V's network view opens its machines and states.

use or_io::gta6::move_network::MoveOutline;

use crate::state::gta6_other_bodies::nodes;

use super::*;

/// How deep a path of nodes opens: a node's inputs may lead back to a node above it.
const DEPTH: usize = 64;

/// The nodes of the GTA VI network the document at `path` opened from; `false`, drawing
/// nothing, for any other document.
pub(super) fn draw_scanned(ui: &mut egui::Ui, path: Option<&std::path::Path>) -> bool {
    let Some(outline) = path.and_then(nodes::outline) else {
        return false;
    };
    ui.heading(tr!("GTA VI MoVE network"));
    ui.separator();
    ui.label(tr!("Each node opens to its fields and to the nodes it leads to, from the root. A kind's name is GTA V's at the same place in GTA V's list, which is not confirmed."));
    ui.add_space(crate::ui::theme::SPACING_TIGHT);
    draw_node(ui, &outline, outline.root, 0);
    true
}

fn draw_node(ui: &mut egui::Ui, outline: &MoveOutline, at: usize, depth: usize) {
    let label = nodes::label(outline, at);
    if depth >= DEPTH {
        ui.label(label);
        return;
    }
    egui::CollapsingHeader::new(label)
        .id_salt(("gta6_move_node", at))
        .show(ui, |ui| {
            grid(ui, &format!("gta6_move_fields_{at:X}_{depth}"), |ui| {
                for (name, value) in nodes::rows(outline, at) {
                    row(ui, &name, &value);
                }
            });
            for child in nodes::children(outline, at) {
                draw_node(ui, outline, child, depth + 1);
            }
        });
}
