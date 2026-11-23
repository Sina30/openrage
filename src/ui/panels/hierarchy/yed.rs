use super::*;

/// The lowercased stem of an expression set's authoring path: the name its dictionary key hashes.
pub(crate) fn short_name(expr: &or_io::gta5::CrExpressions) -> String {
    let name = expr.name.data.as_str();
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = file.rfind('.').map_or(file, |dot| &file[..dot]);
    stem.to_ascii_lowercase()
}

pub(super) fn draw_yed(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let dict: or_io::gta5::CrExpressionsDictionary =
        match state.document(doc_id).map(|d| &d.payload) {
            Some(DocPayload::Resource(or_io::Asset::ExpressionDict(d))) => d.clone(),
            _ => return,
        };

    if dict.entries.is_empty() {
        empty_centered(ui, "No expressions in this dictionary");
        return;
    }

    let id_root = ui.make_persistent_id("hierarchy_yed");
    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();

    let rows: Vec<(usize, u32, String)> = dict
        .codes
        .iter()
        .zip(dict.entries.iter())
        .enumerate()
        .map(|(i, (hash, expr))| {
            let name = expr.as_deref().map(short_name).unwrap_or_default();
            let label = if name.is_empty() {
                format!("#{hash:08X}")
            } else {
                name
            };
            (i, *hash, label)
        })
        .collect();

    let visible = rows
        .iter()
        .filter(|(_, _, label)| !filter_active || label.to_ascii_lowercase().contains(&filter_lc))
        .count();
    let count_text = if filter_active {
        format!("{} of {} expressions match", visible, rows.len())
    } else {
        format!("{} expressions", rows.len())
    };
    ui.label(egui::RichText::new(count_text).color(theme::TEXT_SECONDARY));
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    for (idx, hash, label) in &rows {
        if filter_active && !label.to_ascii_lowercase().contains(&filter_lc) {
            continue;
        }
        let Some(expr) = dict.entries.get(*idx).and_then(|e| e.as_deref()) else {
            continue;
        };
        let sel = dict_entry_id(*hash);
        let row_id = id_root.with(("yed_entry", *idx, *hash));
        consume_scroll_to(state, doc_id, sel, ui.ctx(), row_id);
        let selected = state.selection.is_selected(doc_id, sel);
        let label_color = if selected {
            theme::ACCENT_TEXT
        } else {
            theme::TEXT_PRIMARY
        };

        // Bones and variables rather than instruction counts: they are what
        // the header states about the programs without decoding one.
        let bones = {
            let mut ids: Vec<u16> = expr.tracks.iter().map(|t| t.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len()
        };
        let detail = format!(
            "{bones}b {}v {}p",
            expr.variables.len(),
            expr.expression_streams.len()
        );

        let resp = leaf_row(ui, |ui| {
            let job = icons::icon_label(
                &icons::doc_kind_icon(DocumentKind::Yed, false),
                label.clone(),
                label_color.into(),
            );
            ui.horizontal(|ui| {
                let r = ui.selectable_label(selected, job);
                ui.add(detail_label(detail.clone()));
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
