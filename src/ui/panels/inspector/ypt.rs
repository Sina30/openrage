use super::*;
use or_io::gta5::particle::PtxKeyframeProp;
use or_particle::list;

/// The live values of the effect the preview plays, a slider per evolution named from that effect, whichever one the inspector shows.
pub(super) fn draw_ypt_evolution_sliders(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(eff) = state.sim.manager.effect.as_mut() else {
        return;
    };
    let Some(evolutions) = (eff.evolution_list.as_ref()).filter(|l| !l.evolutions.is_empty())
    else {
        return;
    };
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.separator();
    ui.label(egui::RichText::new(tr!("Live Evolutions of {name}", name = eff.rule.name)).strong());
    for (evolution, value) in evolutions.evolutions.iter().zip(&mut eff.evo_values) {
        let name = match evolution.name.as_str() {
            "" => hash_text(evolution.name_hash, None),
            name => name.to_string(),
        };
        ui.add(egui::Slider::new(value, 0.0..=1.0).text(name));
    }
}

pub(super) fn draw_ypt_effect_mut<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    effect_idx: usize,
) {
    let mut dirty = false;
    let before;
    {
        let Some(doc) = state.document_mut(doc_id) else {
            return;
        };
        let Some(asset) = T::list_mut(&mut doc.payload) else {
            return;
        };
        let Some(effect) = list::effect_rules_mut(asset).nth(effect_idx) else {
            empty(ui, tr!("Selection is no longer valid"));
            return;
        };
        before = effect.clone();
        ui.label(
            egui::RichText::new(tr!("Effect: {name}", name = effect.name))
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(crate::ui::theme::SPACING_TIGHT);
        drag_f32(
            ui,
            "duration_min",
            &mut effect.duration_min,
            0.01,
            &mut dirty,
        );
        drag_f32(
            ui,
            "duration_max",
            &mut effect.duration_max,
            0.01,
            &mut dirty,
        );
        drag_i32(ui, "num_loops", &mut effect.num_loops, &mut dirty);
        drag_f32(ui, "zoom_level", &mut effect.zoom_level, 0.01, &mut dirty);
        drag_f32(
            ui,
            "playback_rate_min",
            &mut effect.playback_rate_scalar_min,
            0.01,
            &mut dirty,
        );
        drag_f32(
            ui,
            "playback_rate_max",
            &mut effect.playback_rate_scalar_max,
            0.01,
            &mut dirty,
        );
        drag_f32(
            ui,
            "viewport_culling_radius",
            &mut effect.viewport_culling_sphere_radius,
            0.1,
            &mut dirty,
        );
        drag_f32(
            ui,
            "distance_culling_cull_dist",
            &mut effect.distance_culling_cull_dist,
            0.1,
            &mut dirty,
        );
        drag_f32(
            ui,
            "lod_evo_dist_min",
            &mut effect.lod_evo_dist_min,
            0.1,
            &mut dirty,
        );
        drag_f32(
            ui,
            "lod_evo_dist_max",
            &mut effect.lod_evo_dist_max,
            0.1,
            &mut dirty,
        );
        ui.label(
            egui::RichText::new(tr!("Read-only"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        grid(ui, "ypt_effect_ro", |ui| {
            row(
                ui,
                tr!("Emitters"),
                &list::events(effect).count().to_string(),
            );
            row(
                ui,
                tr!("Evolutions"),
                &effect
                    .evolution_list
                    .as_ref()
                    .map(|e| e.evolutions.len().to_string())
                    .unwrap_or_else(|| "0".to_string()),
            );
        });
    }
    if dirty {
        super::ypt_steps::effect(state, doc_id, effect_idx, before);
    }
}

pub(super) fn draw_ypt_event_slot_mut<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    effect_idx: usize,
    emitter_idx: usize,
    show_particle: bool,
) {
    let mut dirty = false;
    let before;
    {
        let Some(doc) = state.document_mut(doc_id) else {
            return;
        };
        let Some(asset) = T::list_mut(&mut doc.payload) else {
            return;
        };
        let Some(effect) = list::effect_rules_mut(asset).nth(effect_idx) else {
            empty(ui, tr!("Selection is no longer valid"));
            return;
        };
        before = effect.clone();
        let Some(event_em) = list::events_mut(effect).nth(emitter_idx) else {
            empty(ui, tr!("Selection is no longer valid"));
            return;
        };
        ui.label(
            egui::RichText::new(tr!("Event Slot"))
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(crate::ui::theme::SPACING_TIGHT);
        drag_f32(
            ui,
            "start_ratio",
            &mut event_em.base.start_ratio,
            0.01,
            &mut dirty,
        );
        drag_f32(
            ui,
            "end_ratio",
            &mut event_em.base.end_ratio,
            0.01,
            &mut dirty,
        );
        drag_f32(
            ui,
            "playback_rate_min",
            &mut event_em.playback_rate_scalar_min,
            0.01,
            &mut dirty,
        );
        drag_f32(
            ui,
            "playback_rate_max",
            &mut event_em.playback_rate_scalar_max,
            0.01,
            &mut dirty,
        );
        drag_f32(
            ui,
            "zoom_min",
            &mut event_em.zoom_scalar_min,
            0.01,
            &mut dirty,
        );
        drag_f32(
            ui,
            "zoom_max",
            &mut event_em.zoom_scalar_max,
            0.01,
            &mut dirty,
        );
        ui.horizontal(|ui| {
            ui.label("emitter_rule_name");
            if ui
                .text_edit_singleline(&mut event_em.emitter_rule_name)
                .changed()
            {
                dirty = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("particle_rule_name");
            if ui
                .text_edit_singleline(&mut event_em.particle_rule_name)
                .changed()
            {
                dirty = true;
            }
        });
        let _ = show_particle;
    }
    if dirty {
        super::ypt_steps::effect(state, doc_id, effect_idx, before);
    }
}

pub(super) fn draw_ypt_emitter_lib_mut<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    idx: usize,
) {
    let mut dirty = false;
    let before;
    {
        let Some(doc) = state.document_mut(doc_id) else {
            return;
        };
        let Some(asset) = T::list_mut(&mut doc.payload) else {
            return;
        };
        let Some(rule) = list::emitter_rules_mut(asset).nth(idx) else {
            empty(ui, tr!("Selection is no longer valid"));
            return;
        };
        before = rule.clone();
        ui.label(
            egui::RichText::new(tr!("Emitter Rule: {name}", name = rule.name))
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(crate::ui::theme::SPACING_TIGHT);
        edit_kfp_scalar(
            ui,
            "spawn_rate (kfp)",
            &mut rule.spawn_rate_over_time_kfp,
            0.1,
            &mut dirty,
        );
        edit_kfp_scalar(
            ui,
            "spawn_rate_over_dist (kfp)",
            &mut rule.spawn_rate_over_dist_kfp,
            0.1,
            &mut dirty,
        );
        edit_kfp_scalar(
            ui,
            "particle_life (kfp)",
            &mut rule.particle_life_kfp,
            0.05,
            &mut dirty,
        );
        edit_kfp_scalar(
            ui,
            "playback_rate_scalar (kfp)",
            &mut rule.playback_rate_scalar_kfp,
            0.01,
            &mut dirty,
        );
        edit_kfp_scalar(
            ui,
            "speed_scalar (kfp)",
            &mut rule.speed_scalar_kfp,
            0.01,
            &mut dirty,
        );
        edit_kfp_scalar(
            ui,
            "size_scalar (kfp)",
            &mut rule.size_scalar_kfp,
            0.01,
            &mut dirty,
        );
        edit_kfp_scalar(
            ui,
            "inherit_velocity (kfp)",
            &mut rule.inherit_velocity_kfp,
            0.01,
            &mut dirty,
        );
        check_bool8(ui, "is_one_shot", &mut rule.is_one_shot, &mut dirty);
    }
    if dirty {
        super::ypt_steps::emitter::<T>(state, doc_id, idx, before);
    }
}

fn edit_kfp_scalar(
    ui: &mut egui::Ui,
    label: &str,
    kfp: &mut PtxKeyframeProp,
    speed: f32,
    dirty: &mut bool,
) {
    if kfp.keyframe.entries.is_empty() {
        add_first_keyframe(ui, label, kfp, dirty);
        return;
    }
    let Some(entry) = kfp.keyframe.entries.first_mut() else {
        return;
    };
    drag_f32(ui, label, &mut entry.value.x, speed, dirty);
}

fn edit_kfp_vec3(ui: &mut egui::Ui, label: &str, kfp: &mut PtxKeyframeProp, dirty: &mut bool) {
    if kfp.keyframe.entries.is_empty() {
        add_first_keyframe(ui, label, kfp, dirty);
        return;
    }
    let Some(entry) = kfp.keyframe.entries.first_mut() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(label);
        if drag_value(ui, &mut entry.value.x, |d| d.speed(0.01).prefix("x ")).inner {
            *dirty = true;
        }
        if drag_value(ui, &mut entry.value.y, |d| d.speed(0.01).prefix("y ")).inner {
            *dirty = true;
        }
        if drag_value(ui, &mut entry.value.z, |d| d.speed(0.01).prefix("z ")).inner {
            *dirty = true;
        }
    });
}

/// A property the file gives no keyframe, which gets one only from the user's Add.
fn add_first_keyframe(ui: &mut egui::Ui, label: &str, kfp: &mut PtxKeyframeProp, dirty: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.label(
            egui::RichText::new(tr!("no keyframe"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        if ui
            .small_button(tr!("Add"))
            .on_hover_text(tr!("Give the property a keyframe at time and value zero"))
            .clicked()
        {
            kfp.keyframe.entries.push(Default::default());
            *dirty = true;
        }
    });
}

pub(super) fn draw_ypt_particle_lib_mut<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    idx: usize,
) {
    // A scanned GTA VI list holds no shader template name to write.
    let named_template = !crate::state::gta6_particles::written_in_place(state, doc_id);
    let mut dirty = false;
    let before;
    {
        let Some(doc) = state.document_mut(doc_id) else {
            return;
        };
        let Some(asset) = T::list_mut(&mut doc.payload) else {
            return;
        };
        let Some(rule) = list::particle_rules_mut(asset).nth(idx) else {
            empty(ui, tr!("Selection is no longer valid"));
            return;
        };
        before = rule.clone();
        ui.label(
            egui::RichText::new(tr!("Particle Rule: {name}", name = rule.name))
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(crate::ui::theme::SPACING_TIGHT);
        drag_i32(
            ui,
            "tex_frame_id_min",
            &mut rule.tex_frame_id_min,
            &mut dirty,
        );
        drag_i32(
            ui,
            "tex_frame_id_max",
            &mut rule.tex_frame_id_max,
            &mut dirty,
        );
        drag_u8(ui, "sort_type", &mut rule.sort_type, 3, &mut dirty);
        drag_u8(ui, "draw_type", &mut rule.draw_type, 4, &mut dirty);
        let mut no_shadows = (rule.flags & 0x01) != 0;
        let mut screen_space = (rule.flags & 0x02) != 0;
        let mut no_fog = (rule.flags & 0x04) != 0;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut no_shadows, "no_shadows").changed() {
                rule.flags = (rule.flags & !0x01) | (no_shadows as u8);
                dirty = true;
            }
            if ui.checkbox(&mut screen_space, "screen_space").changed() {
                rule.flags = (rule.flags & !0x02) | ((screen_space as u8) << 1);
                dirty = true;
            }
            if ui.checkbox(&mut no_fog, "no_fog").changed() {
                rule.flags = (rule.flags & !0x04) | ((no_fog as u8) << 2);
                dirty = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("dict_hash");
            ui.label(
                egui::RichText::new(format!("{:#X}", list::code(&rule.name)))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        });
        ui.label(
            egui::RichText::new(tr!("Render state"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        let cull_options = ["None", "Front", "Back"];
        ui.horizontal(|ui| {
            ui.label("cull_mode");
            egui::ComboBox::from_id_salt("ypt_rule_cull")
                .selected_text(
                    cull_options
                        .get(rule.render_state.cull_mode.clamp(0, 2) as usize)
                        .copied()
                        .unwrap_or("None"),
                )
                .show_ui(ui, |ui| {
                    for (i, name) in cull_options.iter().enumerate() {
                        if ui
                            .selectable_label(rule.render_state.cull_mode == i as i32, *name)
                            .clicked()
                        {
                            rule.render_state.cull_mode = i as i32;
                            dirty = true;
                        }
                    }
                });
        });
        let blend_options = [
            "Normal",
            "Add",
            "Subtract",
            "Lightmap",
            "Matte",
            "Overwrite",
            "Dest",
            "AlphaAdd",
            "ReverseSubtract",
            "Min",
            "Max",
            "AlphaSubtract",
            "MultiplySrcDest",
            "CompositeAlpha",
            "CompositeAlphaSubtract",
        ];
        ui.horizontal(|ui| {
            ui.label("blend_set");
            egui::ComboBox::from_id_salt("ypt_rule_blend")
                .selected_text(
                    blend_options
                        .get(rule.render_state.blend_set as usize)
                        .copied()
                        .unwrap_or("?"),
                )
                .show_ui(ui, |ui| {
                    for (i, name) in blend_options.iter().enumerate() {
                        if ui
                            .selectable_label(rule.render_state.blend_set == i as i32, *name)
                            .clicked()
                        {
                            rule.render_state.blend_set = i as i32;
                            dirty = true;
                        }
                    }
                });
        });
        check_bool8(
            ui,
            "depth_write",
            &mut rule.render_state.depth_write,
            &mut dirty,
        );
        check_bool8(
            ui,
            "depth_test",
            &mut rule.render_state.depth_test,
            &mut dirty,
        );
        check_bool8(
            ui,
            "alpha_blend",
            &mut rule.render_state.alpha_blend,
            &mut dirty,
        );
        ui.label(
            egui::RichText::new(tr!("Shader instance"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        if named_template {
            ui.horizontal(|ui| {
                ui.label("shader_template_name");
                if ui
                    .text_edit_singleline(&mut rule.shader_inst.shader_template_name)
                    .changed()
                {
                    dirty = true;
                }
                const KNOWN_TEMPLATES: &[&str] = &[
                    "particle",
                    "particle_lit",
                    "particle_screen",
                    "particle_refract",
                    "particle_proj",
                    "ptxblood",
                    "ptxgeneric",
                ];
                egui::ComboBox::from_id_salt("ypt_rule_shader_template")
                    .selected_text("\u{2304}")
                    .show_ui(ui, |ui| {
                        for name in KNOWN_TEMPLATES {
                            if ui
                                .selectable_label(
                                    rule.shader_inst.shader_template_name == *name,
                                    *name,
                                )
                                .clicked()
                            {
                                rule.shader_inst.shader_template_name = (*name).to_string();
                                dirty = true;
                            }
                        }
                    });
            });
        }
        ui.horizontal(|ui| {
            ui.label("shader_technique_name");
            if ui
                .text_edit_singleline(&mut rule.shader_inst.shader_template_technique_name)
                .changed()
            {
                dirty = true;
            }
        });
        check_bool8(
            ui,
            "is_lit",
            &mut rule.shader_inst.technique_desc.is_lit,
            &mut dirty,
        );
        check_bool8(
            ui,
            "is_soft",
            &mut rule.shader_inst.technique_desc.is_soft,
            &mut dirty,
        );
        check_bool8(
            ui,
            "is_screen_space",
            &mut rule.shader_inst.technique_desc.is_screen_space,
            &mut dirty,
        );
        check_bool8(
            ui,
            "is_refract",
            &mut rule.shader_inst.technique_desc.is_refract,
            &mut dirty,
        );
        check_bool8(
            ui,
            "is_normal_spec",
            &mut rule.shader_inst.technique_desc.is_normal_spec,
            &mut dirty,
        );
        ui.label(
            egui::RichText::new(tr!("Read-only"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        let shader_vars = list::shader_vars(&rule.shader_inst);
        grid(ui, "ypt_part_lib_ro", |ui| {
            row(ui, "Behaviours", &rule.all_behaviours.len().to_string());
            row(ui, "Drawables", &rule.drawables.len().to_string());
            row(ui, "Shader vars", &shader_vars.len().to_string());
        });
        draw_shader_var_names(
            ui,
            rule.shader_inst.shader_template_hash_name.get(),
            &shader_vars,
        );
    }
    if dirty {
        super::ypt_steps::particle(state, doc_id, before);
    }
}

/// Name a rule's shader vars from the shader definition database.
///
/// ⚠ **The three answers are different and the panel says which.** A hash the
/// database cannot name is not the same as no database, and a user shown
/// `0xEA057402` has no way to tell "this variable is unknown" from "OpenRage
/// was never told what this shader declares".
fn draw_shader_var_names(
    ui: &mut egui::Ui,
    shader_hash: u32,
    shader_vars: &[or_io::gta5::particle::ShaderVar],
) {
    use or_io::gta5::shaders::shader_defs;

    if shader_vars.is_empty() {
        return;
    }
    if !shader_defs::is_installed() {
        ui.label(
            egui::RichText::new(
                "No shader definition database, so these are hashes. Build one with \
                 `or-shader-tool defs build`.",
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        return;
    }

    grid(ui, "ypt_part_shader_vars", |ui| {
        for var in shader_vars {
            let hash = match var {
                or_io::gta5::particle::ShaderVar::Vector { name_hash, .. }
                | or_io::gta5::particle::ShaderVar::Texture { name_hash, .. }
                | or_io::gta5::particle::ShaderVar::Keyframe { name_hash, .. } => *name_hash,
            };
            let label = format!("0x{hash:08X}");
            match shader_defs::describe(shader_hash, hash) {
                Some(def) => row(
                    ui,
                    def.ui_name.as_deref().unwrap_or(&def.name),
                    &format!("{} reg {}", def.kind.as_str(), def.register),
                ),
                None => row(ui, &label, "not declared by this shader"),
            }
        }
    });
}

pub(super) fn draw_ypt_domain_mut<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    eff_idx: usize,
    em_idx: usize,
    kind: u64,
) {
    let mut dirty = false;
    let before;
    {
        let Some(doc) = state.document_mut(doc_id) else {
            return;
        };
        let Some(asset) = T::list_mut(&mut doc.payload) else {
            return;
        };
        let Some(effect) = list::effect_rules(asset).nth(eff_idx) else {
            empty(ui, tr!("Selection is no longer valid"));
            return;
        };
        let Some(event_em) = list::events(effect).nth(em_idx) else {
            empty(ui, tr!("Selection is no longer valid"));
            return;
        };
        let em_name = event_em.emitter_rule_name.clone();
        let Some(rule) = list::emitter_rules_mut(asset).find(|r| r.name == em_name) else {
            empty(ui, tr!("Resolved emitter rule no longer exists"));
            return;
        };
        before = rule.clone();
        let domain_label = match kind {
            YPT_DOMAIN_CREATION => "Creation Domain",
            YPT_DOMAIN_TARGET => "Target / Velocity Domain",
            YPT_DOMAIN_ATTRACTOR => "Attractor Domain",
            _ => "Domain",
        };
        let obj = match kind {
            YPT_DOMAIN_CREATION => &mut rule.creation_domain_obj,
            YPT_DOMAIN_TARGET => &mut rule.target_domain_obj,
            YPT_DOMAIN_ATTRACTOR => &mut rule.attractor_domain_obj,
            _ => {
                empty(ui, tr!("Selection is no longer valid"));
                return;
            }
        };
        let Some(domain) = obj.domain.as_deref_mut() else {
            empty(ui, tr!("This emitter rule has no such domain"));
            return;
        };
        ui.label(
            egui::RichText::new(domain_label)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(crate::ui::theme::SPACING_TIGHT);
        let shape_options = ["Box", "Sphere", "Cylinder", "Attractor"];
        ui.horizontal(|ui| {
            ui.label("shape");
            for (i, name) in shape_options.iter().enumerate() {
                if ui.radio(domain.shape == i as i32, *name).clicked() {
                    domain.shape = i as i32;
                    dirty = true;
                }
            }
        });
        let _ = (eff_idx, em_idx, kind);
        edit_kfp_vec3(ui, "position_kfp", &mut domain.position_kfp, &mut dirty);
        edit_kfp_vec3(ui, "rotation_kfp", &mut domain.rotation_kfp, &mut dirty);
        edit_kfp_vec3(ui, "size_outer_kfp", &mut domain.size_outer_kfp, &mut dirty);
        edit_kfp_vec3(ui, "size_inner_kfp", &mut domain.size_inner_kfp, &mut dirty);
        check_bool8(ui, "is_world_space", &mut domain.is_world_space, &mut dirty);
        check_bool8(
            ui,
            "is_point_relative",
            &mut domain.is_point_relative,
            &mut dirty,
        );
        check_bool8(
            ui,
            "is_creation_relative",
            &mut domain.is_creation_relative,
            &mut dirty,
        );
        check_bool8(
            ui,
            "is_target_relative",
            &mut domain.is_target_relative,
            &mut dirty,
        );
    }
    if dirty {
        super::ypt_steps::named_emitter::<T>(state, doc_id, before);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawing_a_keyframe_list_the_file_leaves_empty_adds_no_keyframe() {
        let ctx = egui::Context::default();
        let (mut scalar, mut vector) = (PtxKeyframeProp::default(), PtxKeyframeProp::default());
        let mut dirty = false;
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                edit_kfp_scalar(ui, "spawn_rate (kfp)", &mut scalar, 0.1, &mut dirty);
                edit_kfp_vec3(ui, "position_kfp", &mut vector, &mut dirty);
            });
        }
        assert!(!dirty, "drawing reported an edit");
        assert!(
            scalar.keyframe.entries.is_empty() && vector.keyframe.entries.is_empty(),
            "drawing added a keyframe"
        );
    }
}
