use super::*;

pub(super) fn add_menu_for_root(ui: &mut egui::Ui, ops: &mut Vec<YptDeferredOp>) {
    ui.menu_button("Add", |ui| {
        if ui
            .button(icons::glyph_with_label(
                &icons::ypt_icon(YptItem::Effect),
                "Effect",
            ))
            .clicked()
        {
            ops.push(YptDeferredOp::AddEffect);
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui
            .button(icons::glyph_with_label(
                &icons::ypt_icon(YptItem::Emitter),
                "Emitter Rule",
            ))
            .clicked()
        {
            ops.push(YptDeferredOp::AddEmitterRule);
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui
            .button(icons::glyph_with_label(
                &icons::ypt_icon(YptItem::Particle),
                "Particle Rule",
            ))
            .clicked()
        {
            ops.push(YptDeferredOp::AddParticleRule);
            ui.close_kind(egui::UiKind::Menu);
        }
    });
}

pub(super) fn add_menu_for_effects_section(ui: &mut egui::Ui, ops: &mut Vec<YptDeferredOp>) {
    if ui
        .button(icons::glyph_with_label(
            &icons::ypt_icon(YptItem::Effect),
            "Add Effect",
        ))
        .clicked()
    {
        ops.push(YptDeferredOp::AddEffect);
        ui.close_kind(egui::UiKind::Menu);
    }
}

pub(super) fn add_menu_for_emitter_lib(ui: &mut egui::Ui, ops: &mut Vec<YptDeferredOp>) {
    if ui
        .button(icons::glyph_with_label(
            &icons::ypt_icon(YptItem::Emitter),
            "Add Emitter Rule",
        ))
        .clicked()
    {
        ops.push(YptDeferredOp::AddEmitterRule);
        ui.close_kind(egui::UiKind::Menu);
    }
}

pub(super) fn add_menu_for_particle_lib(ui: &mut egui::Ui, ops: &mut Vec<YptDeferredOp>) {
    if ui
        .button(icons::glyph_with_label(
            &icons::ypt_icon(YptItem::Particle),
            "Add Particle Rule",
        ))
        .clicked()
    {
        ops.push(YptDeferredOp::AddParticleRule);
        ui.close_kind(egui::UiKind::Menu);
    }
}

pub(super) fn add_menu_for_effect(
    ui: &mut egui::Ui,
    state: &AppState,
    doc_id: DocumentId,
    effect_idx: usize,
    ops: &mut Vec<YptDeferredOp>,
) {
    if ui.button("Duplicate").clicked() {
        ops.push(YptDeferredOp::DuplicateEffect(effect_idx));
        ui.close_kind(egui::UiKind::Menu);
    }
    delete_item(ui, state, doc_id, ypt_effect_id(effect_idx), ops);
}

/// Whether the list `doc` holds saves in place, so no rule is made in it from nothing.
pub(super) fn in_place(state: &AppState, doc: DocumentId) -> bool {
    crate::state::gta6_particles::written_in_place(state, doc)
}

/// Edit's Delete, for the effect or rule `id` selects.
fn delete_item(
    ui: &mut egui::Ui,
    state: &AppState,
    doc: DocumentId,
    id: u64,
    ops: &mut Vec<YptDeferredOp>,
) {
    use crate::ui::commands::{self, Action, Target};
    let mut asked = None;
    commands::offer(
        ui,
        state,
        Action::Delete,
        &Target::Particle { doc, id },
        &mut asked,
    );
    ops.extend(asked.map(|command| YptDeferredOp::Run(Box::new(command))));
}

pub(super) fn add_menu_for_event_emitter<T: ParticleTarget>(
    ui: &mut egui::Ui,
    list: &or_io::gta5::PtxFxList<T>,
    effect_idx: usize,
    emitter_idx: usize,
    current_emitter_name: &str,
    current_particle_name: &str,
    ops: &mut Vec<YptDeferredOp>,
) {
    ui.menu_button("Bind emitter rule…", |ui| {
        for rule in or_particle::list::emitter_rules(list) {
            let active = rule.name == current_emitter_name;
            let label = if active {
                format!("{}  \u{2022} current", rule.name)
            } else {
                rule.name.clone()
            };
            if ui.button(label).clicked() {
                ops.push(YptDeferredOp::BindEmitterRule {
                    effect_idx,
                    emitter_idx,
                    rule_name: rule.name.clone(),
                });
                ui.close_kind(egui::UiKind::Menu);
            }
        }
        if or_particle::list::emitter_rules(list).next().is_none() {
            ui.add(detail_label("(no emitter rules in library)"));
        }
    });
    ui.menu_button("Bind particle rule…", |ui| {
        for rule in or_particle::list::particle_rules(list) {
            let active = rule.name == current_particle_name;
            let label = if active {
                format!("{}  \u{2022} current", rule.name)
            } else {
                rule.name.clone()
            };
            if ui.button(label).clicked() {
                ops.push(YptDeferredOp::BindParticleRule {
                    effect_idx,
                    emitter_idx,
                    rule_name: rule.name.clone(),
                });
                ui.close_kind(egui::UiKind::Menu);
            }
        }
        if or_particle::list::particle_rules(list).next().is_none() {
            ui.add(detail_label("(no particle rules in library)"));
        }
    });
}

pub(super) fn add_menu_for_lib_emitter_rule(
    ui: &mut egui::Ui,
    state: &AppState,
    doc_id: DocumentId,
    idx: usize,
    name: &str,
    refs: usize,
    ops: &mut Vec<YptDeferredOp>,
) {
    if ui.button("Find usages").clicked() {
        ops.push(YptDeferredOp::ToastUsages(format!(
            "Emitter rule '{}' used by {} event emitter slot{}",
            name,
            refs,
            if refs == 1 { "" } else { "s" }
        )));
        ui.close_kind(egui::UiKind::Menu);
    }
    delete_item(ui, state, doc_id, ypt_emitter_lib_id(idx), ops);
}

pub(super) fn add_menu_for_lib_particle_rule(
    ui: &mut egui::Ui,
    state: &AppState,
    doc_id: DocumentId,
    idx: usize,
    name: &str,
    refs: usize,
    ops: &mut Vec<YptDeferredOp>,
) {
    if ui.button("Find usages").clicked() {
        ops.push(YptDeferredOp::ToastUsages(format!(
            "Particle rule '{}' used by {} event emitter slot{}",
            name,
            refs,
            if refs == 1 { "" } else { "s" }
        )));
        ui.close_kind(egui::UiKind::Menu);
    }
    delete_item(ui, state, doc_id, ypt_particle_lib_id(idx), ops);
}
