use super::*;

/// One collapsible table of `len` rows, drawn only where it is scrolled to.
fn table(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    headings: &[&str],
    len: usize,
    mut cells: impl FnMut(&mut egui::Ui, usize),
) {
    egui::CollapsingHeader::new(format!("{title} ({len})"))
        .id_salt(id)
        .show(ui, |ui| {
            let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
            egui::ScrollArea::both()
                .id_salt(id)
                .max_height(320.0)
                .show_rows(ui, row_height, len, |ui, range| {
                    egui::Grid::new(id).striped(true).show(ui, |ui| {
                        for heading in headings {
                            ui.strong(*heading);
                        }
                        ui.end_row();
                        for i in range {
                            ui.monospace(i.to_string());
                            cells(ui, i);
                            ui.end_row();
                        }
                    });
                });
        });
}

fn vec3(v: [f32; 3]) -> String {
    format!("{:.1}, {:.1}, {:.1}", v[0], v[1], v[2])
}

fn hash(h: u32) -> String {
    hash_text(h, None)
}

pub(super) fn draw_dat_table(ui: &mut egui::Ui, state: &AppState, doc_id: DocumentId) {
    use crate::state::DatTable;
    match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::DatTable(DatTable::CacheFile(cache))) => draw_cache_file(ui, cache),
        Some(DocPayload::DatTable(DatTable::SocialClubMetadata(metadata))) => {
            draw_social_club_metadata(ui, metadata)
        }
        Some(DocPayload::DatTable(DatTable::AudioWorldSectors(grid))) => {
            draw_audio_world_sectors(ui, grid)
        }
        _ => {}
    }
}

fn draw_cache_file(ui: &mut egui::Ui, cache: &or_io::cache_loader::CacheFile) {
    ui.heading("Cache File");
    ui.separator();
    grid(ui, "cache_file_summary", |ui| {
        row(ui, "Version", &cache.version.to_string());
        row(ui, "Archive dates", &cache.file_dates.len().to_string());
    });
    ui.add_space(crate::ui::theme::SPACING_SNUG);

    let dates = &cache.file_dates;
    table(
        ui,
        "cache_file_dates",
        "fileDates",
        &["#", "Archive", "File time", "Third column"],
        dates.len(),
        |ui, i| {
            let d = &dates[i];
            ui.monospace(hash(d.name_hash));
            ui.monospace(d.file_time.to_string());
            ui.monospace(d.third_column.map_or_else(String::new, |t| t.to_string()));
        },
    );
    let maps = &cache.map_data_store;
    table(
        ui,
        "cache_file_map_data",
        "fwMapDataStore",
        &[
            "#",
            "Name",
            "Parent",
            "Content flags",
            "Streaming min",
            "Streaming max",
            "Physical min",
            "Physical max",
            "Dynamic",
            "Block info",
            "Parent",
        ],
        maps.len(),
        |ui, i| {
            let m = &maps[i];
            ui.monospace(hash(m.name_hash));
            ui.monospace(hash(m.parent_name_hash));
            ui.monospace(format!("{:#x}", m.content_flags));
            ui.monospace(vec3(m.str_min));
            ui.monospace(vec3(m.str_max));
            ui.monospace(vec3(m.phy_min));
            ui.monospace(vec3(m.phy_max));
            ui.label(m.dynamic_streaming.to_string());
            ui.label(m.contains_block_info.to_string());
            ui.label(m.is_parent.to_string());
        },
    );
    let proxies = &cache.interior_proxies;
    table(
        ui,
        "cache_file_interior_proxies",
        "CInteriorProxy",
        &[
            "#",
            "Proxy",
            "Map data",
            "Group",
            "Floor",
            "Exit portals",
            "Position",
            "Bounds min",
            "Bounds max",
        ],
        proxies.len(),
        |ui, i| {
            let p = &proxies[i];
            ui.monospace(hash(p.proxy_name_hash));
            ui.monospace(hash(p.map_data_store_name_hash));
            ui.label(p.group_id.to_string());
            ui.label(p.floor_id.to_string());
            ui.label(p.num_exit_portals.to_string());
            ui.monospace(vec3(p.pos));
            ui.monospace(vec3(p.bbox_min));
            ui.monospace(vec3(p.bbox_max));
        },
    );
    let bounds = &cache.bounds_store;
    table(
        ui,
        "cache_file_bounds",
        "BoundsStore",
        &["#", "Bounds", "Min", "Max", "Asset type"],
        bounds.len(),
        |ui, i| {
            let b = &bounds[i];
            ui.monospace(hash(b.hash_key));
            ui.monospace(vec3(b.min));
            ui.monospace(vec3(b.max));
            ui.label(b.asset_type.to_string());
        },
    );
}

fn draw_social_club_metadata(
    ui: &mut egui::Ui,
    metadata: &or_io::social_club_metadata::SocialClubMetadata,
) {
    ui.heading("Social Club Metadata");
    ui.separator();
    grid(ui, "social_club_metadata_summary", |ui| {
        row(ui, "Format word", &metadata.format_word.to_string());
        row(ui, "Built", &metadata.built);
        row(ui, "Body", &format!("{} bytes", metadata.body.len()));
    });
    ui.add_space(crate::ui::theme::SPACING_SNUG);
    ui.label(
        egui::RichText::new(
            "The game passes this file to the Social Club library. Only its header is \
             measured; the body is kept and saved as it was read.",
        )
        .small()
        .color(theme::TEXT_SECONDARY),
    );
}

fn draw_audio_world_sectors(
    ui: &mut egui::Ui,
    sector_grid: &or_io::audio_world_sectors::AudioWorldSectors,
) {
    let sectors = &sector_grid.sectors;
    ui.heading("Audio World Sectors");
    ui.separator();
    let across = or_io::audio_world_sectors::SECTORS_ACROSS;
    grid(ui, "audio_world_sectors_summary", |ui| {
        row(ui, "Grid", &format!("{across} x {across} sectors"));
        row(
            ui,
            "Over water",
            &sectors
                .iter()
                .filter(|s| s.is_water_sector)
                .count()
                .to_string(),
        );
    });
    ui.add_space(crate::ui::theme::SPACING_SNUG);
    table(
        ui,
        "audio_world_sectors",
        "Sectors",
        &[
            "#",
            "X",
            "Y",
            "Highway nodes",
            "Tallest building",
            "Buildings",
            "Trees",
            "Water",
        ],
        sectors.len(),
        |ui, i| {
            let s = &sectors[i];
            ui.label((i % across).to_string());
            ui.label((i / across).to_string());
            ui.label(s.num_highway_nodes.to_string());
            ui.label(s.tallest_building.to_string());
            ui.label(s.num_buildings.to_string());
            ui.label(s.num_trees.to_string());
            ui.label(s.is_water_sector.to_string());
        },
    );
}
