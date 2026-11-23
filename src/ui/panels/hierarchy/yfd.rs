use super::*;

pub(super) fn draw_yfd(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let dict: or_io::gta5::CrFrameFilterDictionary =
        match state.document(doc_id).map(|d| &d.payload) {
            Some(DocPayload::Resource(or_io::Asset::FrameFilterDict(d))) => d.clone(),
            _ => return,
        };

    if dict.entries.is_empty() {
        empty_centered(ui, "No filters in this dictionary");
        return;
    }

    let id_root = ui.make_persistent_id("hierarchy_yfd");
    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();

    ui.label(
        egui::RichText::new(format!("{} filters", dict.entries.len())).color(theme::TEXT_SECONDARY),
    );
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    for (idx, (hash, filter)) in dict.codes.iter().zip(dict.entries.iter()).enumerate() {
        let label = format!("#{hash:08X}");
        if filter_active && !label.to_ascii_lowercase().contains(&filter_lc) {
            continue;
        }
        let sel = dict_entry_id(*hash);
        let row_id = id_root.with(("yfd_entry", idx, *hash));
        consume_scroll_to(state, doc_id, sel, ui.ctx(), row_id);
        let selected = state.selection.is_selected(doc_id, sel);
        let label_color = if selected {
            theme::ACCENT_TEXT
        } else {
            theme::TEXT_PRIMARY
        };

        let multi = filter.as_deref().and_then(|f| f.as_multi_weight());
        let bones = multi
            .map(|f| {
                let mut ids: Vec<u16> = f.track_id_indices.iter().map(|e| e.id).collect();
                ids.sort_unstable();
                ids.dedup();
                ids.len()
            })
            .unwrap_or(0);
        // The weight count is separate from the entry count because pairs
        // share weights, so both are shown rather than one standing in.
        let detail = format!(
            "{bones}b {}e {}w",
            multi.map_or(0, |f| f.track_id_indices.len()),
            multi.map_or(0, |f| f.weights.len())
        );

        let resp = leaf_row(ui, |ui| {
            let job = icons::icon_label(
                &icons::doc_kind_icon(DocumentKind::Yfd, false),
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
