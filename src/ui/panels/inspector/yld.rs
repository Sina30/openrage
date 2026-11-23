use super::*;

use crate::ui::panels::hierarchy::{cloth_levels, cloth_name};

pub(super) fn draw_yld(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) {
    let facts: Facts = {
        let Some(DocPayload::Resource(or_io::Asset::ClothDict(dict))) =
            state.document(doc_id).map(|d| &d.payload)
        else {
            return;
        };
        let cloths = || dict.entries.iter().flatten();
        let levels = || cloths().flat_map(|c| cloth_levels(c));
        vec![
            ("Entries".into(), dict.entries.len().to_string()),
            (
                "With collision".into(),
                cloths()
                    .filter(|c| c.bound_composite.is_some())
                    .count()
                    .to_string(),
            ),
            (
                "Vertices".into(),
                levels()
                    .map(|(_, l)| l.cloth_data.vertex_positions.len())
                    .sum::<usize>()
                    .to_string(),
            ),
            (
                "Edges".into(),
                levels()
                    .map(|(_, l)| l.edge_data.len())
                    .sum::<usize>()
                    .to_string(),
            ),
            ("Lookup order".into(), lookup_order(&dict.codes).into()),
        ]
    };

    let Some(sel) = selection.filter(|&s| s != DOC_ROOT_SEL) else {
        draw_root_summary(ui, state, doc_id, "yld_root", facts);
        ui.add_space(crate::ui::theme::SPACING_DEFAULT);
        select_in(ui, state, doc_id, tr_key!("a cloth entry"));
        return;
    };
    let Some(payload) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    let DocPayload::Resource(or_io::Asset::ClothDict(dict)) = payload else {
        return;
    };
    let Some(idx) = yld_selected_entry_index(payload, sel) else {
        ui.add_space(crate::ui::theme::SPACING_DEFAULT);
        select_in(ui, state, doc_id, tr_key!("a cloth entry"));
        return;
    };
    let Some(cloth) = dict.entries.get(idx).and_then(|c| c.as_deref()) else {
        return;
    };

    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Cloth")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    grid(ui, "yld_entry", |ui| {
        row(
            ui,
            "Index",
            &format!("{} of {}", idx + 1, dict.entries.len()),
        );
        row(
            ui,
            "Hash",
            &hash_text(dict.codes[idx], state.name_index.name_of(dict.codes[idx])),
        );
        row(ui, "Name", cloth_name(cloth));
        row(ui, "Active", &cloth.is_active.get().to_string());
        row(ui, "Negate force", &cloth.negate_force.get().to_string());
        row(ui, "Bone IDs", &format!("{}", cloth.bone_id.len()));
        row(ui, "Bone indices", &format!("{}", cloth.bone_index.len()));
        row(ui, "Poses", &format!("{}", cloth.poses.len()));
        row(
            ui,
            "Bound",
            if cloth.bound_composite.is_some() {
                "present"
            } else {
                "none"
            },
        );
    });

    if let Some(controller) = cloth.cloth_controller.as_ref() {
        let levels = cloth_levels(cloth);
        ui.add_space(crate::ui::theme::SPACING_DEFAULT);
        ui.label(
            egui::RichText::new("Controller")
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        grid(ui, "yld_controller", |ui| {
            row(ui, "Name", cloth_name(cloth));
            row(
                ui,
                "Populated LODs",
                &format!(
                    "{:?}",
                    levels.iter().map(|(lod, _)| *lod).collect::<Vec<_>>()
                ),
            );
            row(
                ui,
                "Tri indices",
                &format!("{}", controller.tri_indices.len()),
            );
            row(
                ui,
                "Original positions",
                &format!("{}", controller.original_pos.len()),
            );
            row(
                ui,
                "Pin radius scale",
                &format!("{:.4}", controller.pinning_radius_scale),
            );
            row(
                ui,
                "Pin radius threshold",
                &format!("{:.4}", controller.pin_radius_set_threshold),
            );
            row(ui, "Wind scale", &format!("{:.4}", controller.wind_scale));
            row(
                ui,
                "Binding info",
                &format!("{}", controller.binding_info.len()),
            );
        });

        for (lod, verlet) in levels {
            ui.add_space(crate::ui::theme::SPACING_SNUG);
            ui.label(
                egui::RichText::new(format!("Verlet cloth LOD {lod}"))
                    .strong()
                    .color(theme::TEXT_PRIMARY),
            );
            let grid_id = format!("yld_verlet_{lod}");
            grid(ui, &grid_id, |ui| {
                row(
                    ui,
                    "Vertices",
                    &verlet.cloth_data.vertex_positions.len().to_string(),
                );
                row(ui, "Pinned", &verlet.num_pinned_verts.to_string());
                row(ui, "Edges", &format!("{}", verlet.edge_data.len()));
                row(
                    ui,
                    "Custom edges",
                    &format!("{}", verlet.custom_edge_data.len()),
                );
                row(ui, "Cloth weight", &format!("{:.4}", verlet.cloth_weight));
                row(
                    ui,
                    "BB min",
                    &format!(
                        "({:.3}, {:.3}, {:.3})",
                        verlet.bb_min.x, verlet.bb_min.y, verlet.bb_min.z
                    ),
                );
                row(
                    ui,
                    "BB max",
                    &format!(
                        "({:.3}, {:.3}, {:.3})",
                        verlet.bb_max.x, verlet.bb_max.y, verlet.bb_max.z
                    ),
                );
            });
        }
    }
}
