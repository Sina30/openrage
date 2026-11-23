use super::*;

pub(super) fn draw_height_map(ui: &mut egui::Ui, state: &AppState, doc_id: DocumentId) {
    let Some(DocPayload::HeightMap(map)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    ui.heading("Terrain Height Map");
    ui.separator();
    let [cell_x, cell_y] = map.cell_size();
    let [min_x, min_y, min_z] = map.bounds_min;
    let [max_x, max_y, max_z] = map.bounds_max;
    grid(ui, "height_map_summary", |ui| {
        row(
            ui,
            "Grid",
            &format!("{} x {} cells", map.num_cols, map.num_rows),
        );
        row(ui, "Cell", &format!("{cell_x} x {cell_y}"));
        row(ui, "X", &format!("{min_x} .. {max_x}"));
        row(ui, "Y", &format!("{min_y} .. {max_y}"));
        row(ui, "Z", &format!("{min_z} .. {max_z}"));
        row(
            ui,
            "Cells with a height",
            &map.max.iter().filter(|&&h| h != 0).count().to_string(),
        );
        row(
            ui,
            "Water mask",
            if map.water_mask.is_empty() {
                "no"
            } else {
                "yes"
            },
        );
    });
}

pub(super) fn draw_distant_lights(ui: &mut egui::Ui, state: &AppState, doc_id: DocumentId) {
    let Some(DocPayload::DistantLights(lights)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    ui.heading("Distant Lights");
    ui.separator();
    grid(ui, "distant_lights_summary", |ui| {
        row(ui, "Blocks", &format!("{0} x {0}", lights.blocks()));
        row(ui, "Points", &lights.points.len().to_string());
        row(ui, "Groups", &lights.groups.len().to_string());
    });
    ui.add_space(crate::ui::theme::SPACING_SNUG);
    ui.label(
        egui::RichText::new(
            "The file stores no colour: the game draws street lights in one colour and \
             traffic as white headlights and red tail lights, from its visual settings.",
        )
        .small()
        .color(theme::TEXT_SECONDARY),
    );
    ui.add_space(crate::ui::theme::SPACING_SNUG);

    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    egui::ScrollArea::vertical().show_rows(ui, row_height, lights.groups.len(), |ui, range| {
        egui::Grid::new("distant_lights_groups")
            .striped(true)
            .show(ui, |ui| {
                for heading in ["#", "Centre", "Radius", "Points", "Lanes", "Lights"] {
                    ui.strong(heading);
                }
                ui.end_row();
                for i in range {
                    let g = &lights.groups[i];
                    ui.monospace(i.to_string());
                    ui.monospace(format!("{}, {}, {}", g.centre_x, g.centre_y, g.centre_z));
                    ui.label(g.radius.to_string());
                    ui.label(g.point_count.to_string());
                    ui.label(g.num_lanes().to_string());
                    let (colour, kind) = match (g.use_streetlights(), g.two_way()) {
                        (true, _) => (theme::WARNING.get(), "street lights"),
                        (false, true) => (theme::TEXT_PRIMARY.get(), "traffic, white and red"),
                        (false, false) => (theme::TEXT_PRIMARY.get(), "traffic, one way"),
                    };
                    ui.colored_label(colour, kind);
                    ui.end_row();
                }
            });
    });
}

pub(super) fn draw_water_map(ui: &mut egui::Ui, state: &AppState, doc_id: DocumentId) {
    let Some(DocPayload::WaterMap(map)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    ui.heading("Water Height Map");
    ui.separator();
    grid(ui, "water_map_summary", |ui| {
        row(
            ui,
            "Grid",
            &format!("{} x {} tiles", map.num_tile_cols, map.num_tile_rows()),
        );
        row(
            ui,
            "Tile",
            &format!("{} x {}", map.tile_size_x, map.tile_size_y),
        );
        row(
            ui,
            "North-west corner",
            &format!("{}, {}", map.offset_x, map.offset_y),
        );
        row(ui, "References", &map.ref_ids.len().to_string());
        row(
            ui,
            "Rivers",
            &format!("{} ({} points)", map.rivers.len(), map.river_points.len()),
        );
        row(
            ui,
            "Lakes",
            &format!("{} ({} boxes)", map.lakes.len(), map.lake_boxes.len()),
        );
        row(ui, "Pools", &map.pools.len().to_string());
    });
}

pub(super) fn draw_world_blanket(ui: &mut egui::Ui, state: &AppState, doc_id: DocumentId) {
    let Some(DocPayload::WorldBlanket(blanket)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    ui.heading("World Blanket");
    ui.separator();
    grid(ui, "world_blanket_summary", |ui| {
        row(ui, "Grid", &format!("{0} x {0} cells", blanket.side()));
        row(
            ui,
            "Non-zero cells",
            &blanket
                .cells
                .iter()
                .filter(|&&c| c != 0)
                .count()
                .to_string(),
        );
        let max = blanket.cells.iter().max().copied().unwrap_or(0);
        row(ui, "Largest value", &max.to_string());
    });
    unmeasured_note(
        ui,
        "The file states no side, orientation or meaning for a cell: the grid is \
         the square its length makes, and each cell its stated 16-bit value.",
    );
}

pub(super) fn draw_nav_precalc(ui: &mut egui::Ui, state: &AppState, doc_id: DocumentId) {
    let Some(DocPayload::NavPrecalc(precalc)) = state.document(doc_id).map(|d| &d.payload) else {
        return;
    };
    ui.heading("Nav Precalc");
    ui.separator();
    grid(ui, "nav_precalc_summary", |ui| {
        row(ui, "Header values", &format!("{:?}", precalc.header));
        row(ui, "Cells", &precalc.cells.len().to_string());
        row(
            ui,
            "Cells with entries",
            &precalc
                .cells
                .iter()
                .filter(|c| c.entries() > 0)
                .count()
                .to_string(),
        );
        row(
            ui,
            "Entries",
            &precalc
                .cells
                .iter()
                .map(|c| c.entries())
                .sum::<usize>()
                .to_string(),
        );
        row(ui, "BITS block", &format!("{} bytes", precalc.bits.len()));
    });
    unmeasured_note(
        ui,
        "No reader in the game names this file, so each cell's two values, its four \
         groups of four-byte entries and the BITS block are kept as stated.",
    );
}

fn unmeasured_note(ui: &mut egui::Ui, text: &str) {
    ui.add_space(crate::ui::theme::SPACING_SNUG);
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}
