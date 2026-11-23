use super::*;

pub(super) fn draw_ycd(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let dict: or_io::gta5::CrClipDictionary = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::Clips(d))) => d.clone(),
        _ => return,
    };

    if or_anim::clip::clips(&dict).next().is_none() {
        empty_centered(ui, "No clips in this dictionary");
        return;
    }

    let id_root = ui.make_persistent_id("hierarchy_ycd");
    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();

    let rows: Vec<(u32, String, f32, u16, usize)> = or_anim::clip::clips(&dict)
        .map(|(key, clip)| {
            let name = &or_anim::clip::header(clip).name.data;
            let label = if name.is_empty() {
                format!("#{key:08X}")
            } else {
                name.clone()
            };
            let animation = or_anim::clip::primary(&dict, clip);
            let frames = animation.map(|a| a.num_frames).unwrap_or(0);
            let bones = animation
                .map(|a| or_anim::clip::bone_ids(a).len())
                .unwrap_or(0);
            (key, label, or_anim::clip::duration(clip), frames, bones)
        })
        .collect();

    let visible = rows
        .iter()
        .filter(|(_, label, _, _, _)| {
            !filter_active || label.to_ascii_lowercase().contains(&filter_lc)
        })
        .count();
    let count_text = if filter_active {
        format!("{} of {} clips match", visible, rows.len())
    } else {
        format!(
            "{} clips, {} animations",
            rows.len(),
            dict.anim_dictionary
                .as_deref()
                .map_or(0, |a| a.animations.entries().count())
        )
    };
    ui.label(egui::RichText::new(count_text).color(theme::TEXT_SECONDARY));
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    let playing = state
        .preview_scene
        .clip
        .filter(|c| c.document == doc_id)
        .map(|c| c.clip_key);

    for (key, label, duration, frames, bones) in &rows {
        if filter_active && !label.to_ascii_lowercase().contains(&filter_lc) {
            continue;
        }
        let sel = dict_entry_id(*key);
        let row_id = id_root.with(("ycd_clip", *key));
        consume_scroll_to(state, doc_id, sel, ui.ctx(), row_id);
        let selected = state.selection.is_selected(doc_id, sel);
        let is_playing = playing == Some(*key);
        let label_color = if selected {
            theme::ACCENT_TEXT
        } else {
            theme::TEXT_PRIMARY
        };

        let resp = leaf_row(ui, |ui| {
            let job = icons::icon_label(
                &icons::doc_kind_icon(DocumentKind::Ycd, false),
                label.clone(),
                label_color.into(),
            );
            ui.horizontal(|ui| {
                let r = ui.selectable_label(selected, job);
                if is_playing {
                    ui.label(egui::RichText::new("▶").small().color(theme::SUCCESS));
                }
                ui.add(detail_label(format!("{duration:.2}s {frames}f {bones}b")));
                r
            })
            .inner
        });
        pulse_scroll_into_view(state, doc_id, sel, &resp);
        if resp.clicked() {
            state.selection.select(doc_id, sel);
            state.preview_scene.set_clip(doc_id, *key);
            state.anim.invalidate();
        }
    }
}
