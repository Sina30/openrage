use super::*;
use crate::state::particle_target::ParticleTarget;
use or_io::gta5::drawable::RmcDrawable;
use or_io::gta5::particle::{PtxDrawable, PtxFxList};
use or_particle::list;

pub(super) fn ypt_section_models<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    asset: &PtxFxList<T>,
    id_root: egui::Id,
) {
    let filter_active = !super::super::ui_state(state).filter.is_empty();
    let filter_lc = super::super::ui_state(state).filter.to_ascii_lowercase();

    let header_selected = state.selection.is_selected(doc_id, YPT_MODEL_DICT);
    let header_color = if header_selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let header_resp = collapsing_row(
        ui,
        id_root.with("models_section"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::ModelDict),
                format!("Models ({})", list::models(asset).count()),
                header_color.into(),
            );
            ui.selectable_label(header_selected, job)
        },
        |ui| {
            for (idx, (hash, mesh)) in list::models(asset).enumerate() {
                let info = list::model_info(asset, hash);
                let display_name = info
                    .map(|d| d.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| format!("model #{:08X}", hash));
                if filter_active && !display_name.to_ascii_lowercase().contains(&filter_lc) {
                    continue;
                }
                ypt_draw_model_row(
                    ui,
                    state,
                    doc_id,
                    asset,
                    id_root,
                    idx,
                    hash,
                    (mesh, info),
                    &display_name,
                );
            }
        },
    );
    if header_resp.clicked() {
        state.selection.select(doc_id, YPT_MODEL_DICT);
    }
}

#[allow(clippy::too_many_arguments)]
fn ypt_draw_model_row<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    asset: &PtxFxList<T>,
    id_root: egui::Id,
    idx: usize,
    hash: u32,
    (mesh, info): (&RmcDrawable<T>, Option<&PtxDrawable<T>>),
    display_name: &str,
) {
    let sel = ypt_model_id(hash);
    let row_id = id_root.with(("model", idx, hash));
    consume_scroll_to(state, doc_id, sel, ui.ctx(), row_id);
    let selected = state.selection.is_selected(doc_id, sel);
    let label_color = if selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let radius = mesh.lod_group.bounds_sphere.center_and_radius.w;

    let header_resp = collapsing_row(
        ui,
        row_id,
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::ModelDict),
                display_name.to_string(),
                label_color.into(),
            );
            ui.horizontal(|ui| {
                let r = ui.selectable_label(selected, job);
                ui.add(detail_label(format!("r {:.2}", radius)));
                r
            })
            .inner
        },
        |ui| {
            if let Some(info) = info {
                ypt_model_bound_row(ui, info);
            }
            ypt_model_section_lods(ui, state, doc_id, hash, &owned_lod_counts(mesh), row_id);
            if let Some(group) = &mesh.shader_group {
                let effects: Vec<u32> = (group.shaders.iter())
                    .map(|s| T::shader_name_hash(s))
                    .collect();
                ypt_model_section_shaders(ui, state, doc_id, hash, &effects, row_id);
            }
            ypt_model_section_textures(ui, state, doc_id, asset, hash, row_id);
            if let Some(skel) = &mesh.skeleton {
                let bones: Vec<&str> = skel.bones.iter().map(|b| b.name.as_str()).collect();
                ypt_model_section_skeleton(ui, state, doc_id, hash, &bones, row_id);
            }
        },
    );
    pulse_scroll_into_view(state, doc_id, sel, &header_resp);
    if header_resp.clicked() {
        state.selection.select(doc_id, sel);
    }
}

fn ypt_model_bound_row<T: ParticleTarget>(ui: &mut egui::Ui, info: &PtxDrawable<T>) {
    let b = &info.bound_info;
    leaf_row(ui, |ui| {
        ui.horizontal(|ui| {
            let job = icons::icon_label(
                &icons::ydr_section_icon(YdrSection::Bound),
                "Bound",
                theme::TEXT_SECONDARY.get(),
            );
            ui.add(egui::Label::new(job).selectable(false));
            ui.add(detail_label(format!(
                "{:.2} \u{00d7} {:.2} \u{00d7} {:.2}  \u{2022}  r {:.2}",
                b.x, b.y, b.z, b.w,
            )));
        })
        .inner
    });
}

fn ypt_model_section_textures<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    asset: &PtxFxList<T>,
    model_hash: u32,
    id_root: egui::Id,
) {
    use crate::ui::dock::Workspace;
    use crate::ui::panels::selection_ids::{
        ypt_model_texture_lookup, ypt_model_texture_names, ypt_model_textures_section_id,
    };

    let names = ypt_model_texture_names(asset, model_hash);
    if names.is_empty() {
        return;
    }
    let filter_active = !super::super::ui_state(state).filter.is_empty();
    let filter_lc = super::super::ui_state(state).filter.to_ascii_lowercase();
    let header_sel = ypt_model_textures_section_id(model_hash);
    let header_selected = state.selection.is_selected(doc_id, header_sel);
    let header_color = if header_selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let header_resp = collapsing_row(
        ui,
        id_root.with("ypt_model_textures"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::TextureDict),
                format!("Textures ({})", names.len()),
                header_color.into(),
            );
            ui.selectable_label(header_selected, job)
        },
        |ui| {
            for (idx, name) in names.iter().enumerate() {
                if filter_active && !name.to_ascii_lowercase().contains(&filter_lc) {
                    continue;
                }
                let sel = ypt_model_part_id(model_hash, YPT_MODEL_KIND_TEXTURE, idx as u32);
                let selected = state.selection.is_selected(doc_id, sel);
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(&icons::ytd_texture_icon(), name, label_color.into());
                let detail = match ypt_model_texture_lookup(asset, model_hash, name).map(T::wrap) {
                    Some(tex) => {
                        format!(
                            "{}\u{00d7}{}  {:?}",
                            tex.width(),
                            tex.height(),
                            tex.format()
                        )
                    }
                    None => "(unresolved)".to_string(),
                };
                let clicked = ui
                    .horizontal(|ui| {
                        let r = ui.selectable_label(selected, job);
                        ui.add(detail_label(detail));
                        r.clicked()
                    })
                    .inner;
                if clicked {
                    state.selection.select(doc_id, sel);
                    state.active_workspace = Workspace::Texture;
                }
            }
        },
    );
    if header_resp.clicked() {
        state.selection.select(doc_id, header_sel);
        state.active_workspace = Workspace::Texture;
    }
}
