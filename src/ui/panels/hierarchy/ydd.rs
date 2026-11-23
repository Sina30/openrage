use super::*;

pub(super) fn draw_ydd(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let dict = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::DrawableDict(d))) => d.clone(),
        _ => return,
    };
    let entries: Vec<(usize, u32, DrawableOutline<'_>)> = (dict.codes.iter().zip(&dict.entries))
        .enumerate()
        .filter_map(|(i, (hash, entry))| {
            let d = entry.as_deref()?;
            Some((
                i,
                *hash,
                DrawableOutline::of(&d.base, &d.lights, d.bound.as_deref()),
            ))
        })
        .collect();
    draw_ydd_dictionary(ui, state, doc_id, &entries, or_io::Generation::Gen8);
}

/// A drawable dictionary's entries, each by its index in the file, its hash and its outline.
pub(super) fn draw_ydd_dictionary(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    entries: &[(usize, u32, DrawableOutline<'_>)],
    target: or_io::Generation,
) {
    if entries.is_empty() {
        empty_centered(ui, "No drawables in this dictionary");
        return;
    }

    let id_root = ui.make_persistent_id("hierarchy_ydd");
    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();

    let rows: Vec<(usize, u32, String, &DrawableOutline<'_>)> = entries
        .iter()
        .map(|(i, hash, outline)| {
            let label = if outline.name.is_empty() {
                format!("#{hash:08X}")
            } else {
                outline.name.to_string()
            };
            (*i, *hash, label, outline)
        })
        .collect();

    let visible = rows
        .iter()
        .filter(|(_, _, label, _)| {
            !filter_active || label.to_ascii_lowercase().contains(&filter_lc)
        })
        .count();
    let count_text = if filter_active {
        format!("{} of {} drawables match", visible, rows.len())
    } else {
        format!("{} drawables · {}", rows.len(), target.edition_label())
    };
    ui.label(egui::RichText::new(count_text).color(theme::TEXT_SECONDARY));
    ui.add_space(crate::ui::theme::SPACING_HAIR);

    for (idx, hash, label, outline) in &rows {
        if filter_active && !label.to_ascii_lowercase().contains(&filter_lc) {
            continue;
        }
        ydd_draw_entry_row(ui, state, doc_id, outline, id_root, *idx, *hash, label);
    }
}

#[allow(clippy::too_many_arguments)]
fn ydd_draw_entry_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    outline: &DrawableOutline<'_>,
    id_root: egui::Id,
    idx: usize,
    hash: u32,
    label: &str,
) {
    let sel = dict_entry_id(hash);
    let row_id = id_root.with(("ydd_entry", idx, hash));
    consume_scroll_to(state, doc_id, sel, ui.ctx(), row_id);
    let selected = state.selection.is_selected(doc_id, sel);
    let label_color = if selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let radius = outline.radius;

    let header_resp = collapsing_row(
        ui,
        row_id,
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::doc_kind_icon(DocumentKind::Ydr, false),
                label.to_string(),
                label_color.into(),
            );
            ui.horizontal(|ui| {
                let r = ui.selectable_label(selected, job);
                ui.add(detail_label(format!("r {radius:.2}")));
                r
            })
            .inner
        },
        |ui| draw_entry_sections(ui, state, doc_id, hash, outline, row_id),
    );
    pulse_scroll_into_view(state, doc_id, sel, &header_resp);
    if header_resp.clicked() {
        state.selection.select(doc_id, sel);
    }
}

/// A container entry's sections, keyed by the entry's hash.
pub(super) fn draw_entry_sections(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    hash: u32,
    outline: &DrawableOutline<'_>,
    id_root: egui::Id,
) {
    ypt_model_section_lods(ui, state, doc_id, hash, &outline.lods, id_root);
    if let Some(effects) = &outline.effects {
        ypt_model_section_shaders(ui, state, doc_id, hash, effects, id_root);
        if let Some(rows) = &outline.textures {
            ydd_entry_section_textures(ui, state, doc_id, hash, rows, id_root);
        }
    }
    if let Some(bones) = &outline.bones {
        ypt_model_section_skeleton(ui, state, doc_id, hash, bones, id_root);
    }
    if !outline.lights.is_empty() {
        ydd_entry_section_lights(ui, state, doc_id, hash, &outline.lights, id_root);
    }
    if let Some(bound) = &outline.bound {
        ydd_entry_bound_row(ui, state, doc_id, hash, &bound.label, id_root);
    }
}

pub(super) fn ydd_entry_section_textures(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    entry_hash: u32,
    rows: &[TextureRow],
    id_root: egui::Id,
) {
    if rows.is_empty() {
        return;
    }
    collapsing_row(
        ui,
        id_root.with("ydd_textures"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::doc_kind_icon(DocumentKind::Ytd, false),
                format!("Textures ({})", rows.len()),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(egui::Label::new(job).selectable(false))
        },
        |ui| {
            for (i, (name, width, height, _)) in rows.iter().enumerate() {
                let sel = dict_entry_part_id(entry_hash, YPT_MODEL_KIND_TEXTURE, i as u32);
                let selected = state.selection.is_selected(doc_id, sel);
                let color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(&icons::ytd_texture_icon(), name, color.into());
                let clicked = leaf_row(ui, |ui| {
                    ui.horizontal(|ui| {
                        let r = ui.selectable_label(selected, job);
                        ui.add(detail_label(format!("{}\u{00d7}{}", width, height)));
                        r.clicked()
                    })
                    .inner
                });
                if clicked {
                    state.selection.select(doc_id, sel);
                    state.active_workspace = crate::ui::dock::Workspace::Texture;
                }
            }
        },
    );
}

fn ydd_entry_section_lights(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    entry_hash: u32,
    lights: &[u8],
    id_root: egui::Id,
) {
    collapsing_row(
        ui,
        id_root.with("ydd_lights"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Lights),
                format!("Lights ({})", lights.len()),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(egui::Label::new(job).selectable(false))
        },
        |ui| {
            for (i, light_type) in lights.iter().enumerate() {
                let sel = dict_entry_part_id(entry_hash, YPT_MODEL_KIND_LIGHT, i as u32);
                let selected = state.selection.is_selected(doc_id, sel);
                let color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(
                    &icons::ydr_section_icon(YdrSection::Lights),
                    format!("Light {i}"),
                    color.into(),
                );
                let clicked = leaf_row(ui, |ui| {
                    ui.horizontal(|ui| {
                        let r = ui.selectable_label(selected, job);
                        ui.add(detail_label(light_type_name(*light_type)));
                        r.clicked()
                    })
                    .inner
                });
                if clicked {
                    state.selection.select(doc_id, sel);
                }
            }
        },
    );
}

pub(super) fn ydd_entry_bound_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    entry_hash: u32,
    bound_type: &str,
    _id_root: egui::Id,
) {
    let sel = dict_entry_part_id(entry_hash, YPT_MODEL_KIND_BOUND, 0);
    let selected = state.selection.is_selected(doc_id, sel);
    let color = if selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let job = icons::icon_label(
        &icons::ydr_section_icon(YdrSection::Bound),
        "Bound",
        color.into(),
    );
    let resp = leaf_row(ui, |ui| {
        ui.horizontal(|ui| {
            let r = ui.selectable_label(selected, job);
            ui.add(detail_label(bound_type.to_string()));
            r
        })
        .inner
    });
    pulse_scroll_into_view(state, doc_id, sel, &resp);
    if resp.clicked() {
        state.selection.select(doc_id, sel);
    }
}
