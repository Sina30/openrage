use super::*;

use or_io::gta5::{CharacterCloth, CharacterClothController, PhVerletCloth};

/// A cloth's controller's name, empty where it has none or it is not UTF-8.
pub(crate) fn cloth_name(cloth: &CharacterCloth) -> &str {
    cloth
        .cloth_controller
        .as_ref()
        .and_then(|c| std::str::from_utf8(c.base.name()).ok())
        .unwrap_or("")
}

/// The first simulated level a controller holds.
pub(crate) fn primary_cloth(controller: &CharacterClothController) -> Option<&PhVerletCloth> {
    controller
        .base
        .cloth()
        .into_iter()
        .flatten()
        .next()
        .map(|c| &**c)
}

/// Every simulated level a cloth holds.
pub(crate) fn cloth_levels(cloth: &CharacterCloth) -> Vec<(usize, &PhVerletCloth)> {
    cloth.cloth_controller.as_ref().map_or_else(Vec::new, |c| {
        c.base
            .cloth()
            .into_iter()
            .enumerate()
            .filter_map(|(lod, level)| level.as_deref().map(|l| (lod, l)))
            .collect()
    })
}

pub(super) fn draw_yld(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let dict = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::ClothDict(d))) => d.clone(),
        _ => return,
    };

    if dict.entries.is_empty() {
        empty_centered(ui, "No cloth entries in this dictionary");
        return;
    }

    let id_root = ui.make_persistent_id("hierarchy_yld");
    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();

    let rows: Vec<(usize, u32, String)> = dict
        .codes
        .iter()
        .zip(dict.entries.iter())
        .enumerate()
        .map(|(i, (hash, cloth))| {
            let name = cloth.as_deref().map_or("", cloth_name);
            let label = if name.is_empty() {
                format!("#{hash:08X}")
            } else {
                name.to_string()
            };
            (i, *hash, label)
        })
        .collect();

    let visible = rows
        .iter()
        .filter(|(_, _, label)| !filter_active || label.to_ascii_lowercase().contains(&filter_lc))
        .count();
    let count_text = if filter_active {
        format!("{} of {} cloth entries match", visible, rows.len())
    } else {
        format!("{} cloth entries", rows.len())
    };
    ui.label(egui::RichText::new(count_text).color(theme::TEXT_SECONDARY));
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    for (idx, hash, label) in &rows {
        if filter_active && !label.to_ascii_lowercase().contains(&filter_lc) {
            continue;
        }
        let Some(cloth) = dict.entries.get(*idx).and_then(|c| c.as_deref()) else {
            continue;
        };
        let sel = dict_entry_id(*hash);
        let row_id = id_root.with(("yld_entry", *idx, *hash));
        consume_scroll_to(state, doc_id, sel, ui.ctx(), row_id);
        let selected = state.selection.is_selected(doc_id, sel);
        let label_color = if selected {
            theme::ACCENT_TEXT
        } else {
            theme::TEXT_PRIMARY
        };
        let levels = cloth_levels(cloth);
        let verts: usize = levels
            .iter()
            .map(|(_, l)| l.cloth_data.vertex_positions.len())
            .sum();
        let edges: usize = levels.iter().map(|(_, l)| l.edge_data.len()).sum();
        let lods = levels.len();

        let resp = leaf_row(ui, |ui| {
            let job = icons::icon_label(
                &icons::doc_kind_icon(DocumentKind::Yld, false),
                label.clone(),
                label_color.into(),
            );
            ui.horizontal(|ui| {
                let r = ui.selectable_label(selected, job);
                ui.add(detail_label(format!("{verts}v {edges}e L{lods}")));
                r
            })
            .inner
        });
        pulse_scroll_into_view(state, doc_id, sel, &resp);
        if resp.clicked() {
            state.selection.select(doc_id, sel);
        }
    }
}
