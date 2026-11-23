mod menus;
mod models;
mod ops;

pub(crate) use ops::{delete_row, deletes};

use menus::*;
use models::*;
use ops::*;

use super::*;
use crate::state::particle_target::{on_particle_target, ParticleTarget};
use or_io::gta5::particle::{PtxDomainObj, PtxEffectRule, PtxEventEmitter, PtxFxList};
use or_particle::list;

pub(super) fn draw_ypt(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let Some(doc) = state.document(doc_id) else {
        return;
    };
    on_particle_target!(&doc.payload, T => draw_list::<T>(ui, state, doc_id), else ())
}

fn draw_list<T: ParticleTarget>(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let Some(slot) = state
        .document_mut(doc_id)
        .and_then(|d| T::list_mut(&mut d.payload))
    else {
        return;
    };
    let asset = std::mem::take(slot);

    let id_root = ui.make_persistent_id("hierarchy_ypt");
    let mut ops: Vec<YptDeferredOp> = Vec::new();
    let ref_cache = YptRefCache::build(&asset);

    pulse_open_for_target(state, doc_id, ui.ctx(), id_root);

    ypt_section_effects(ui, state, doc_id, &asset, &ref_cache, id_root, &mut ops);

    ypt_section_emitter_rules(ui, state, doc_id, &asset, &ref_cache, id_root, &mut ops);
    ypt_section_particle_rules(ui, state, doc_id, &asset, &ref_cache, id_root, &mut ops);

    if let Some(td) = asset.texture_dict.as_deref() {
        if !td.entries.is_empty() {
            ypt_section_textures::<T>(ui, state, doc_id, td, id_root);
        }
    }

    if list::models(&asset).next().is_some() {
        ypt_section_models(ui, state, doc_id, &asset, id_root);
    }

    if list::effect_rules(&asset).any(|e| e.evolution_list.is_some()) {
        ypt_section_evolutions(ui, state, doc_id, &asset, id_root);
    }

    let avail = ui.available_size_before_wrap();
    if avail.x > 4.0 && avail.y > 4.0 {
        let rect = egui::Rect::from_min_size(ui.cursor().min, avail.min(egui::Vec2::splat(64.0)));
        let bg = ui.interact(
            rect,
            ui.id().with(("ypt_root_ctx", doc_id)),
            egui::Sense::click(),
        );
        if !in_place(state, doc_id) {
            bg.context_menu(|ui| add_menu_for_root(ui, &mut ops));
        }
    }

    if let Some(slot) = state
        .document_mut(doc_id)
        .and_then(|d| T::list_mut(&mut d.payload))
    {
        *slot = asset;
    }

    apply_ypt_ops::<T>(state, doc_id, ops, ui.ctx());
}

struct YptRefCache<'a> {
    emitter_count: Vec<usize>,
    particle_count: Vec<usize>,
    emitter_idx_by_name: std::collections::HashMap<&'a str, usize>,
    particle_idx_by_name: std::collections::HashMap<&'a str, usize>,
}

impl<'a> YptRefCache<'a> {
    fn build<T: ParticleTarget>(asset: &'a PtxFxList<T>) -> Self {
        let emitter_idx_by_name: std::collections::HashMap<&str, usize> =
            list::emitter_rules(asset)
                .enumerate()
                .map(|(i, r)| (r.name.as_str(), i))
                .collect();
        let particle_idx_by_name: std::collections::HashMap<&str, usize> =
            list::particle_rules(asset)
                .enumerate()
                .map(|(i, r)| (r.name.as_str(), i))
                .collect();

        let mut emitter_count = vec![0usize; list::emitter_rules(asset).count()];
        let mut particle_count = vec![0usize; list::particle_rules(asset).count()];
        for effect in list::effect_rules(asset) {
            for ev in list::events(effect) {
                if !ev.emitter_rule_name.is_empty() {
                    if let Some(&i) = emitter_idx_by_name.get(ev.emitter_rule_name.as_str()) {
                        emitter_count[i] += 1;
                    }
                }
                if !ev.particle_rule_name.is_empty() {
                    if let Some(&i) = particle_idx_by_name.get(ev.particle_rule_name.as_str()) {
                        particle_count[i] += 1;
                    }
                }
            }
        }

        Self {
            emitter_count,
            particle_count,
            emitter_idx_by_name,
            particle_idx_by_name,
        }
    }

    fn unused_emitter_count(&self) -> usize {
        self.emitter_count.iter().filter(|&&c| c == 0).count()
    }

    fn unused_particle_count(&self) -> usize {
        self.particle_count.iter().filter(|&&c| c == 0).count()
    }
}

fn ypt_section_effects<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    asset: &PtxFxList<T>,
    ref_cache: &YptRefCache<'_>,
    id_root: egui::Id,
    ops: &mut Vec<YptDeferredOp>,
) {
    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();
    let count = list::effect_rules(asset).count();

    let header_resp = collapsing_row(
        ui,
        id_root.with("effects_section"),
        true,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::EffectsSection),
                format!("Effects ({})", count),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            if count == 0 {
                ui.add(detail_label("(empty — right-click to add)"));
                return;
            }
            for (effect_idx, effect) in list::effect_rules(asset).enumerate() {
                if filter_active && !effect.name.to_ascii_lowercase().contains(&filter_lc) {
                    continue;
                }
                ypt_draw_effect(
                    ui, state, doc_id, asset, ref_cache, effect_idx, effect, id_root, ops,
                );
            }
        },
    );

    if !in_place(state, doc_id) {
        header_resp.context_menu(|ui| add_menu_for_effects_section(ui, ops));
    }
}

#[allow(clippy::too_many_arguments)]
fn ypt_draw_effect<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    list: &PtxFxList<T>,
    ref_cache: &YptRefCache<'_>,
    effect_idx: usize,
    effect: &PtxEffectRule<T>,
    id_root: egui::Id,
    ops: &mut Vec<YptDeferredOp>,
) {
    let effect_id = id_root.with(("effect", effect_idx));
    let effect_sel = ypt_effect_id(effect_idx);
    consume_scroll_to(state, doc_id, effect_sel, ui.ctx(), effect_id);
    let effect_selected = state.selection.is_selected(doc_id, effect_sel);

    let effect_header_resp = collapsing_row(
        ui,
        effect_id,
        effect_idx == 0,
        |ui, _open| {
            let label_color = if effect_selected {
                theme::ACCENT_TEXT
            } else {
                theme::TEXT_PRIMARY
            };
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::Effect),
                &effect.name,
                label_color.into(),
            );
            let resp = ui.selectable_label(effect_selected, job);
            let events = list::events(effect).count();
            ui.add(detail_label(format!(
                "{} emitter{} \u{2022} {:.1}s\u{2013}{:.1}s",
                events,
                if events == 1 { "" } else { "s" },
                effect.duration_min,
                effect.duration_max
            )));
            resp
        },
        |ui| {
            for (emitter_idx, event_em) in list::events(effect).enumerate() {
                ypt_draw_event_emitter(
                    ui,
                    state,
                    doc_id,
                    list,
                    ref_cache,
                    effect_idx,
                    emitter_idx,
                    event_em,
                    effect_id,
                    ops,
                );
            }
        },
    );

    pulse_scroll_into_view(state, doc_id, effect_sel, &effect_header_resp);
    handle_select_click(
        state,
        ui.ctx(),
        &effect_header_resp,
        doc_id,
        effect_sel,
        effect_id,
        effect_idx == 0,
    );
    effect_header_resp.context_menu(|ui| add_menu_for_effect(ui, state, doc_id, effect_idx, ops));
}

#[allow(clippy::too_many_arguments)]
fn ypt_draw_event_emitter<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    list: &PtxFxList<T>,
    ref_cache: &YptRefCache<'_>,
    effect_idx: usize,
    emitter_idx: usize,
    event_em: &PtxEventEmitter<T>,
    effect_id: egui::Id,
    ops: &mut Vec<YptDeferredOp>,
) {
    let emitter_id = effect_id.with(("emitter", emitter_idx));
    let emitter_sel = ypt_emitter_id(effect_idx, emitter_idx);
    consume_scroll_to(state, doc_id, emitter_sel, ui.ctx(), emitter_id);
    let emitter_selected = state.selection.is_selected(doc_id, emitter_sel);
    let emitter_target_idx = ref_cache
        .emitter_idx_by_name
        .get(event_em.emitter_rule_name.as_str())
        .copied();
    let emitter_rule = emitter_target_idx.and_then(|i| list::emitter_rules(list).nth(i));
    let spawn_rate = emitter_rule
        .and_then(|r| r.spawn_rate_over_time_kfp.keyframe.entries.first())
        .map_or(0.0, |e| e.value.x);

    let header_resp = collapsing_row(
        ui,
        emitter_id,
        emitter_idx == 0,
        |ui, _open| {
            let label_color = if emitter_selected {
                theme::ACCENT_TEXT
            } else {
                theme::TEXT_PRIMARY
            };
            let label = format!("Event #{}", emitter_idx);
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::Emitter),
                label,
                label_color.into(),
            );
            let resp = ui.selectable_label(emitter_selected, job);
            ui.add(detail_label(format!("{:.1}/s", spawn_rate)));
            resp
        },
        |ui| {
            link_chip(
                ui,
                state,
                doc_id,
                "Emitter rule",
                &event_em.emitter_rule_name,
                emitter_target_idx.map(ypt_emitter_lib_id),
                ops,
            );

            let particle_target_idx = ref_cache
                .particle_idx_by_name
                .get(event_em.particle_rule_name.as_str())
                .copied();
            link_chip(
                ui,
                state,
                doc_id,
                "Particle rule",
                &event_em.particle_rule_name,
                particle_target_idx.map(ypt_particle_lib_id),
                ops,
            );

            if let Some(em) = emitter_rule {
                ypt_domain_row(
                    ui,
                    state,
                    doc_id,
                    effect_idx,
                    emitter_idx,
                    YPT_DOMAIN_CREATION,
                    "Creation domain",
                    shape_of(&em.creation_domain_obj),
                );
                ypt_domain_row(
                    ui,
                    state,
                    doc_id,
                    effect_idx,
                    emitter_idx,
                    YPT_DOMAIN_TARGET,
                    "Target domain",
                    shape_of(&em.target_domain_obj),
                );
                if em.attractor_domain_obj.domain.is_some() {
                    ypt_domain_row(
                        ui,
                        state,
                        doc_id,
                        effect_idx,
                        emitter_idx,
                        YPT_DOMAIN_ATTRACTOR,
                        "Attractor domain",
                        shape_of(&em.attractor_domain_obj),
                    );
                }
            }
        },
    );

    pulse_scroll_into_view(state, doc_id, emitter_sel, &header_resp);
    handle_select_click(
        state,
        ui.ctx(),
        &header_resp,
        doc_id,
        emitter_sel,
        emitter_id,
        emitter_idx == 0,
    );
    let snapshot_emitter_name = event_em.emitter_rule_name.clone();
    let snapshot_particle_name = event_em.particle_rule_name.clone();
    if in_place(state, doc_id) {
        return;
    }
    header_resp.context_menu(|ui| {
        add_menu_for_event_emitter(
            ui,
            list,
            effect_idx,
            emitter_idx,
            &snapshot_emitter_name,
            &snapshot_particle_name,
            ops,
        )
    });
}

/// The shape a domain states; a missing domain samples as a box.
fn shape_of(obj: &PtxDomainObj) -> u32 {
    obj.domain.as_deref().map_or(0, |d| d.shape as u32)
}

#[allow(clippy::too_many_arguments)]
fn ypt_domain_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    effect_idx: usize,
    emitter_idx: usize,
    kind: u64,
    label: &str,
    shape: u32,
) {
    let sel = ypt_domain_id(effect_idx, emitter_idx, kind);
    let selected = state.selection.is_selected(doc_id, sel);
    let label_color = if selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let shape_name = match PtxDomainShape::from_u32(shape) {
        Some(PtxDomainShape::Box) => "Box",
        Some(PtxDomainShape::Sphere) => "Sphere",
        Some(PtxDomainShape::Cylinder) => "Cylinder",
        Some(PtxDomainShape::Attractor) => "Attractor",
        None => "?",
    };
    let job = icons::icon_label(&icons::ypt_icon(YptItem::Domain), label, label_color.into());
    let clicked = leaf_row(ui, |ui| {
        ui.horizontal(|ui| {
            let r = ui.selectable_label(selected, job);
            ui.add(detail_label(format!("({})", shape_name)));
            r.clicked()
        })
        .inner
    });
    if clicked {
        state.selection.select(doc_id, sel);
    }
}

fn ypt_section_emitter_rules<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    asset: &PtxFxList<T>,
    ref_cache: &YptRefCache<'_>,
    id_root: egui::Id,
    ops: &mut Vec<YptDeferredOp>,
) {
    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();
    let count = list::emitter_rules(asset).count();
    let unused_count = ref_cache.unused_emitter_count();

    let header_resp = collapsing_row(
        ui,
        id_root.with("emitter_rules_section"),
        false,
        |ui, _open| {
            let label = if unused_count == 0 {
                format!("Emitter Rules ({})", count)
            } else {
                format!("Emitter Rules ({} \u{2022} {} unused)", count, unused_count)
            };
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::EmitterRuleLibrary),
                label,
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            if count == 0 {
                ui.add(detail_label("(empty — right-click to add)"));
                return;
            }
            for (idx, rule) in list::emitter_rules(asset).enumerate() {
                if filter_active && !rule.name.to_ascii_lowercase().contains(&filter_lc) {
                    continue;
                }
                let sel = ypt_emitter_lib_id(idx);
                let row_id = id_root.with(("em_rule", idx));
                consume_scroll_to(state, doc_id, sel, ui.ctx(), row_id);
                let selected = state.selection.is_selected(doc_id, sel);
                let refs = ref_cache.emitter_count.get(idx).copied().unwrap_or(0);
                let is_unused = refs == 0;
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else if is_unused {
                    theme::TEXT_DISABLED
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(
                    &icons::ypt_icon(YptItem::Emitter),
                    &rule.name,
                    label_color.into(),
                );
                let resp = leaf_row(ui, |ui| {
                    ui.horizontal(|ui| {
                        let r = ui.selectable_label(selected, job);
                        let badge = if refs == 0 {
                            "unused".to_string()
                        } else {
                            format!("\u{00d7}{}", refs)
                        };
                        ui.add(detail_label(badge));
                        r
                    })
                    .inner
                });
                pulse_scroll_into_view(state, doc_id, sel, &resp);
                if resp.clicked() {
                    state.selection.select(doc_id, sel);
                }
                let snapshot_name = rule.name.clone();
                resp.context_menu(|ui| {
                    add_menu_for_lib_emitter_rule(
                        ui,
                        state,
                        doc_id,
                        idx,
                        &snapshot_name,
                        refs,
                        ops,
                    );
                });
            }
        },
    );
    if !in_place(state, doc_id) {
        header_resp.context_menu(|ui| add_menu_for_emitter_lib(ui, ops));
    }
}

fn ypt_section_particle_rules<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    asset: &PtxFxList<T>,
    ref_cache: &YptRefCache<'_>,
    id_root: egui::Id,
    ops: &mut Vec<YptDeferredOp>,
) {
    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();
    let count = list::particle_rules(asset).count();
    let unused_count = ref_cache.unused_particle_count();

    let header_resp = collapsing_row(
        ui,
        id_root.with("particle_rules_section"),
        false,
        |ui, _open| {
            let label = if unused_count == 0 {
                format!("Particle Rules ({})", count)
            } else {
                format!(
                    "Particle Rules ({} \u{2022} {} unused)",
                    count, unused_count
                )
            };
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::ParticleRuleLibrary),
                label,
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            if count == 0 {
                ui.add(detail_label("(empty — right-click to add)"));
                return;
            }
            for (idx, rule) in list::particle_rules(asset).enumerate() {
                if filter_active && !rule.name.to_ascii_lowercase().contains(&filter_lc) {
                    continue;
                }
                let sel = ypt_particle_lib_id(idx);
                let row_id = id_root.with(("part_rule", idx));
                consume_scroll_to(state, doc_id, sel, ui.ctx(), row_id);
                let selected = state.selection.is_selected(doc_id, sel);
                let refs = ref_cache.particle_count.get(idx).copied().unwrap_or(0);
                let is_unused = refs == 0;
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else if is_unused {
                    theme::TEXT_DISABLED
                } else {
                    theme::TEXT_PRIMARY
                };
                let job = icons::icon_label(
                    &icons::ypt_icon(YptItem::Particle),
                    &rule.name,
                    label_color.into(),
                );
                let header_resp = collapsing_row(
                    ui,
                    row_id,
                    false,
                    |ui, _open| {
                        ui.horizontal(|ui| {
                            let r = ui.selectable_label(selected, job);
                            let badge = if refs == 0 {
                                "unused".to_string()
                            } else {
                                format!("\u{00d7}{}", refs)
                            };
                            ui.add(detail_label(badge));
                            r
                        })
                        .inner
                    },
                    |ui| {
                        let behaviours = list::behaviours(rule);
                        if behaviours.is_empty() {
                            ui.add(detail_label("(no behaviours)"));
                            return;
                        }
                        for (b_idx, behaviour) in behaviours.iter().enumerate() {
                            let b_sel = ypt_behaviour_id(idx, b_idx);
                            let b_selected = state.selection.is_selected(doc_id, b_sel);
                            let b_color = if b_selected {
                                theme::ACCENT_TEXT
                            } else {
                                theme::TEXT_SECONDARY
                            };
                            let b_job = icons::icon_label_small(
                                &icons::ypt_icon(YptItem::Behaviour),
                                behaviour_name(behaviour),
                                b_color.into(),
                            );
                            let r = leaf_row(ui, |ui| ui.selectable_label(b_selected, b_job));
                            pulse_scroll_into_view(state, doc_id, b_sel, &r);
                            if r.clicked() {
                                state.selection.select(doc_id, b_sel);
                            }
                        }
                    },
                );
                pulse_scroll_into_view(state, doc_id, sel, &header_resp);
                if header_resp.clicked() {
                    state.selection.select(doc_id, sel);
                }
                let snapshot_name = rule.name.clone();
                header_resp.context_menu(|ui| {
                    add_menu_for_lib_particle_rule(
                        ui,
                        state,
                        doc_id,
                        idx,
                        &snapshot_name,
                        refs,
                        ops,
                    );
                });
            }
        },
    );
    if !in_place(state, doc_id) {
        header_resp.context_menu(|ui| add_menu_for_particle_lib(ui, ops));
    }
}

fn ypt_section_textures<T: ParticleTarget>(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    td: &or_io::gta5::PgDictionary<T::Texture>,
    id_root: egui::Id,
) {
    let filter_active = !super::ui_state(state).filter.is_empty();
    let filter_lc = super::ui_state(state).filter.to_ascii_lowercase();

    let header_selected = state.selection.is_selected(doc_id, YPT_TEXTURE_DICT);
    let header_color = if header_selected {
        theme::ACCENT_TEXT
    } else {
        theme::TEXT_PRIMARY
    };
    let header_resp = collapsing_row(
        ui,
        id_root.with("textures_section"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::TextureDict),
                format!("Textures ({})", td.entries.len()),
                header_color.into(),
            );
            ui.selectable_label(header_selected, job)
        },
        |ui| {
            for tex in td.entries.iter().flatten() {
                let tex = T::wrap(tex);
                if filter_active && !tex.name().to_ascii_lowercase().contains(&filter_lc) {
                    continue;
                }
                let sel = ypt_texture_id(tex.name());
                let row_id = id_root.with(("tex", tex.name()));
                consume_scroll_to(state, doc_id, sel, ui.ctx(), row_id);
                let selected = state.selection.is_selected(doc_id, sel);
                let label_color = if selected {
                    theme::ACCENT_TEXT
                } else {
                    theme::TEXT_PRIMARY
                };
                let job =
                    icons::icon_label(&icons::ytd_texture_icon(), tex.name(), label_color.into());
                let r = leaf_row(ui, |ui| {
                    ui.horizontal(|ui| {
                        let r = ui.selectable_label(selected, job);
                        ui.add(detail_label(format!(
                            "{}\u{00d7}{}  {:?}",
                            tex.width(),
                            tex.height(),
                            tex.format()
                        )));
                        r
                    })
                    .inner
                });
                pulse_scroll_into_view(state, doc_id, sel, &r);
                if r.clicked() {
                    state.selection.select(doc_id, sel);
                    state.active_workspace = Workspace::Texture;
                }
            }
        },
    );
    if header_resp.clicked() {
        state.selection.select(doc_id, YPT_TEXTURE_DICT);
        state.active_workspace = Workspace::Texture;
    }
}

fn ypt_section_evolutions<T: ParticleTarget>(
    ui: &mut egui::Ui,
    _state: &mut AppState,
    _doc_id: DocumentId,
    asset: &PtxFxList<T>,
    id_root: egui::Id,
) {
    let total: usize = list::effect_rules(asset)
        .filter_map(|e| e.evolution_list.as_deref())
        .map(|l| l.evolutions.len())
        .sum();

    collapsing_row(
        ui,
        id_root.with("evolutions_section"),
        false,
        |ui, _open| {
            let job = icons::icon_label(
                &icons::ypt_icon(YptItem::Evolution),
                format!("Evolutions ({})", total),
                theme::TEXT_PRIMARY.get(),
            );
            ui.add(
                egui::Label::new(job)
                    .selectable(false)
                    .sense(egui::Sense::click()),
            )
        },
        |ui| {
            for effect in list::effect_rules(asset) {
                let Some(evolutions) = effect.evolution_list.as_deref() else {
                    continue;
                };
                if evolutions.evolutions.is_empty() {
                    continue;
                }
                ui.add(detail_label(format!("{}: ", effect.name)));
                for evo in &evolutions.evolutions {
                    let job = icons::icon_label_small(
                        &icons::ypt_icon(YptItem::Evolution),
                        &evo.name,
                        theme::TEXT_SECONDARY.get(),
                    );
                    leaf_row(ui, |ui| ui.add(egui::Label::new(job).selectable(false)));
                }
            }
        },
    );
}

fn link_chip(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    label: &str,
    name: &str,
    target: Option<u64>,
    ops: &mut Vec<YptDeferredOp>,
) {
    leaf_row(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(detail_label(format!("{}:", label)));
            let display = if name.is_empty() {
                "(none)".to_string()
            } else {
                name.to_string()
            };
            let color = if target.is_some() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_DISABLED
            };
            let text = egui::RichText::new(display).italics().color(color);
            let resp = ui
                .add(egui::Label::new(text).selectable(false).sense(egui::Sense::click()))
                .on_hover_text(if target.is_some() {
                    "Click to select this reference. Shift-click to reveal the target rule in its library."
                } else {
                    "No matching library rule. Right-click the parent emitter to bind one."
                });
            if let Some(target_sel) = target {
                let glyph = egui::RichText::new(ph::ARROW_SQUARE_OUT)
                    .small()
                    .color(theme::TEXT_SECONDARY);
                ui.add(egui::Label::new(glyph).selectable(false));
                if resp.clicked() {
                    if ui.input(|i| i.modifiers.shift) {
                        ops.push(YptDeferredOp::SelectAndReveal(target_sel));
                    } else {
                        state.selection.select(doc_id, target_sel);
                    }
                }
            }
        })
        .inner
    });
}

fn behaviour_name(behaviour: &or_particle::Behaviour) -> &'static str {
    use or_particle::Behaviour;
    match behaviour {
        Behaviour::Acceleration(_) => "Acceleration",
        Behaviour::Velocity(_) => "Velocity",
        Behaviour::Rotation(_) => "Rotation",
        Behaviour::RotationSpeed(_) => "Rotation Speed",
        Behaviour::Colour(_) => "Colour",
        Behaviour::Size(_) => "Size",
        Behaviour::AnimateTexture(_) => "Animate Texture",
        Behaviour::FadeIn(_) => "Fade In",
        Behaviour::FadeOut(_) => "Fade Out",
        Behaviour::Noise(_) => "Noise",
        Behaviour::Wind(_) => "Wind",
        Behaviour::Dampening(_) => "Dampening",
        Behaviour::Gravity(_) => "Gravity",
        Behaviour::Collision(_) => "Collision",
        Behaviour::Attractor(_) => "Attractor",
        Behaviour::DrawSprite(_) => "Draw Sprite",
        Behaviour::DrawTrail(_) => "Draw Trail",
        Behaviour::DrawModel(_) => "Draw Model",
        Behaviour::Decal(_) => "Decal",
        Behaviour::FogVolume(_) => "Fog Volume",
        Behaviour::Light(_) => "Light",
        Behaviour::Liquid(_) => "Liquid",
        Behaviour::River(_) => "River",
        Behaviour::DecalPool(_) => "Decal Pool",
        Behaviour::Age(_) => "Age",
        Behaviour::Pause(_) => "Pause",
        Behaviour::ZCulling(_) => "Z Culling",
        Behaviour::MatrixWeight(_) => "Matrix Weight",
        Behaviour::Unknown { .. } => "Unknown",
    }
}
