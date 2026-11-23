//! The Inspector's body for a particle file: what the selection names, at either target.

use super::*;
use or_particle::list;

pub(super) fn draw_ypt(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) {
    let Some(doc) = state.document(doc_id) else {
        return;
    };
    on_particle_target!(&doc.payload, T => draw_selected::<T>(ui, state, doc_id, selection),
        else ())
}

fn draw_selected<T: ShaderRows>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: crate::state::DocumentId,
    selection: Option<u64>,
) {
    let sel = match selection {
        None | Some(DOC_ROOT_SEL) => {
            let facts: Facts = {
                let Some(asset) = state.document(doc_id).and_then(|d| T::list(&d.payload)) else {
                    return;
                };
                vec![
                    (
                        tr!("Effects").into(),
                        list::effect_rules(asset).count().to_string(),
                    ),
                    (
                        tr!("Emitter rules").into(),
                        list::emitter_rules(asset).count().to_string(),
                    ),
                    (
                        tr!("Particle rules").into(),
                        list::particle_rules(asset).count().to_string(),
                    ),
                    (
                        tr!("Embedded textures").into(),
                        asset
                            .texture_dict
                            .as_ref()
                            .map_or(0, |d| d.entries.len())
                            .to_string(),
                    ),
                    (
                        tr!("Embedded models").into(),
                        list::models(asset).count().to_string(),
                    ),
                ]
            };
            draw_root_summary(ui, state, doc_id, "ypt_root", facts);
            ui.add_space(crate::ui::theme::SPACING_DEFAULT);
            select_in(ui, state, doc_id, tr_key!("an element"));
            return;
        }
        Some(s) => {
            ui.heading(tr!("Particle Effect Bundle"));
            ui.separator();
            s
        }
    };

    let internals = shows_internals(state);
    match ypt_tag_of(sel) {
        YPT_EFFECT => draw_ypt_effect_mut::<T>(ui, state, doc_id, ypt_unpack_effect(sel)),
        YPT_EMITTER => {
            let (eff_idx, em_idx) = ypt_unpack_emitter(sel);
            draw_ypt_event_slot_mut::<T>(ui, state, doc_id, eff_idx, em_idx, false);
        }
        YPT_PARTICLE => {
            let (eff_idx, em_idx) = ypt_unpack_particle(sel);
            draw_ypt_event_slot_mut::<T>(ui, state, doc_id, eff_idx, em_idx, true);
        }
        YPT_EMITTER_LIB => {
            draw_ypt_emitter_lib_mut::<T>(ui, state, doc_id, ypt_unpack_emitter_lib(sel));
        }
        YPT_PARTICLE_LIB => {
            draw_ypt_particle_lib_mut::<T>(ui, state, doc_id, ypt_unpack_particle_lib(sel));
        }
        YPT_TEXTURE => {
            let Some(doc) = state.document(doc_id) else {
                return;
            };
            draw_ypt_texture::<T>(ui, &doc.payload, sel, internals);
        }
        YPT_MODEL => {
            // A bone of a model is edited as a drawable's is
            if draw_selected_bone(ui, state, doc_id, Some(sel)) {
                return;
            }
            let Some(doc) = state.document(doc_id) else {
                return;
            };
            let Some(asset) = T::list(&doc.payload) else {
                return;
            };
            let (model_hash, kind, sub) = ypt_unpack_model_part(sel);
            match kind {
                YPT_MODEL_KIND_MODEL => draw_ypt_model(ui, asset, model_hash),
                YPT_MODEL_KIND_LOD => draw_ypt_model_lod(ui, asset, model_hash, sub, internals),
                YPT_MODEL_KIND_SHADER => draw_ypt_model_shader(ui, asset, model_hash, sub),
                YPT_MODEL_KIND_BONE => draw_ypt_model_bone(ui, asset, model_hash, sub),
                YPT_MODEL_KIND_TEXTURE => {
                    draw_ypt_model_texture::<T>(ui, &doc.payload, model_hash, sel, internals)
                }
                _ => empty(ui, tr!("Selection is no longer valid")),
            }
        }
        YPT_MODEL_DICT => {
            let Some(asset) = state.document(doc_id).and_then(|d| T::list(&d.payload)) else {
                return;
            };
            draw_ypt_models_overview(ui, asset);
        }
        YPT_DOMAIN => {
            let (eff_idx, em_idx, kind) = ypt_unpack_domain(sel);
            draw_ypt_domain_mut::<T>(ui, state, doc_id, eff_idx, em_idx, kind);
        }
        YPT_BEHAVIOUR => {
            let (part_idx, beh_idx) = ypt_unpack_behaviour(sel);
            draw_ypt_behaviour_mut::<T>(ui, state, doc_id, part_idx, beh_idx);
        }
        _ => empty(ui, tr!("Selection is no longer valid")),
    }
}
