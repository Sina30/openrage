//! A particle list as the editor edits it: rules found by name in their
//! dictionaries, added, renamed and removed there, and laid out so each rule
//! is held once.

use or_io::decl::{Bool8, Hash32, Owned, OwnedOrKeyed, Raw, Record, Ref};
use or_io::gta5::drawable::{DrawableTarget, RmcDrawable};
use or_io::gta5::particle::{
    PtxDrawable, PtxEffectRule, PtxEmitterRule, PtxEventEmitter, PtxFxList, PtxParticleRule,
    PtxRenderState, PtxShaderInst, PtxShaderVarAny, PtxTechniqueDesc, ShaderVar,
};
use or_io::gta5::PgDictionary;

use crate::behaviour::Behaviour;
use crate::evolution::EvolutionList;
use crate::keyframe::raw_entry;

/// The code a particle list's dictionaries key a rule named `name` by.
pub fn code(name: &str) -> u32 {
    Hash32::of(name).get()
}

fn entries<R: Owned>(dict: &Option<Box<PgDictionary<R>>>) -> impl Iterator<Item = &R>
where
    Raw<R>: Record,
{
    dict.iter()
        .flat_map(|d| d.entries.iter())
        .filter_map(|e| e.as_deref())
}

fn entries_mut<R: Owned>(dict: &mut Option<Box<PgDictionary<R>>>) -> impl Iterator<Item = &mut R>
where
    Raw<R>: Record,
{
    dict.iter_mut()
        .flat_map(|d| d.entries.iter_mut())
        .filter_map(|e| e.as_deref_mut())
}

pub fn effect_rules<T: DrawableTarget>(
    list: &PtxFxList<T>,
) -> impl Iterator<Item = &PtxEffectRule<T>> {
    entries(&list.effect_rule_dict)
}

pub fn effect_rules_mut<T: DrawableTarget>(
    list: &mut PtxFxList<T>,
) -> impl Iterator<Item = &mut PtxEffectRule<T>> {
    entries_mut(&mut list.effect_rule_dict)
}

pub fn emitter_rules<T: DrawableTarget>(
    list: &PtxFxList<T>,
) -> impl Iterator<Item = &PtxEmitterRule> {
    entries(&list.emitter_rule_dict)
}

pub fn emitter_rules_mut<T: DrawableTarget>(
    list: &mut PtxFxList<T>,
) -> impl Iterator<Item = &mut PtxEmitterRule> {
    entries_mut(&mut list.emitter_rule_dict)
}

pub fn particle_rules<T: DrawableTarget>(
    list: &PtxFxList<T>,
) -> impl Iterator<Item = &PtxParticleRule<T>> {
    entries(&list.particle_rule_dict)
}

pub fn particle_rules_mut<T: DrawableTarget>(
    list: &mut PtxFxList<T>,
) -> impl Iterator<Item = &mut PtxParticleRule<T>> {
    entries_mut(&mut list.particle_rule_dict)
}

/// The emitter rule named `name`.
pub fn emitter_rule<'a, T: DrawableTarget>(
    list: &'a PtxFxList<T>,
    name: &str,
) -> Option<&'a PtxEmitterRule> {
    emitter_rules(list).find(|r| r.name == name)
}

/// The particle rule named `name`.
pub fn particle_rule<'a, T: DrawableTarget>(
    list: &'a PtxFxList<T>,
    name: &str,
) -> Option<&'a PtxParticleRule<T>> {
    particle_rules(list).find(|r| r.name == name)
}

/// The effect rule named `name`.
pub fn effect_rule<'a, T: DrawableTarget>(
    list: &'a PtxFxList<T>,
    name: &str,
) -> Option<&'a PtxEffectRule<T>> {
    effect_rules(list).find(|r| r.name == name)
}

/// Each model the list holds, with the code it is keyed by.
pub fn models<T: DrawableTarget>(
    list: &PtxFxList<T>,
) -> impl Iterator<Item = (u32, &RmcDrawable<T>)> {
    list.model_dict.iter().flat_map(|d| {
        d.codes
            .iter()
            .zip(&d.entries)
            .filter_map(|(code, e)| Some((*code, e.as_deref()?)))
    })
}

/// The model the list keys by `code`.
pub fn model<T: DrawableTarget>(list: &PtxFxList<T>, code: u32) -> Option<&RmcDrawable<T>> {
    models(list).find(|(c, _)| *c == code).map(|(_, m)| m)
}

/// What a particle rule states of the model keyed by `code`: its name and bounds.
pub fn model_info<T: DrawableTarget>(list: &PtxFxList<T>, code: u32) -> Option<&PtxDrawable<T>> {
    particle_rules(list)
        .flat_map(|r| &r.drawables)
        .find(|d| d.hash_name.get() == code)
}

/// An effect's events, in the order its timeline fires them.
pub fn events<T: DrawableTarget>(
    rule: &PtxEffectRule<T>,
) -> impl Iterator<Item = &PtxEventEmitter<T>> {
    rule.timeline.events.iter().filter_map(|e| e.as_deref())
}

pub fn events_mut<T: DrawableTarget>(
    rule: &mut PtxEffectRule<T>,
) -> impl Iterator<Item = &mut PtxEventEmitter<T>> {
    rule.timeline
        .events
        .iter_mut()
        .filter_map(|e| e.as_deref_mut())
}

/// A particle rule's behaviours, in `all_behaviours`' order.
pub fn behaviours<T: DrawableTarget>(rule: &PtxParticleRule<T>) -> Vec<Behaviour> {
    rule.all_behaviours
        .iter()
        .filter_map(|b| match b {
            Ref::Own(b) => Some(Behaviour::of(b)),
            Ref::To(_) => None,
        })
        .collect()
}

/// A particle rule's behaviours replaced by `behaviours`, each kept where it was of the same class.
///
/// ⚠ The init, update, finalize, draw and release lists name each behaviour
/// again by its place in `all_behaviours`, so they are rebuilt here, and
/// [`canonical`] names them by the place the rule then has.
pub fn set_behaviours<T: DrawableTarget>(rule: &mut PtxParticleRule<T>, behaviours: &[Behaviour]) {
    let old: Vec<Option<Box<_>>> = std::mem::take(&mut rule.all_behaviours)
        .into_iter()
        .map(|b| match b {
            Ref::Own(b) => Some(b),
            Ref::To(_) => None,
        })
        .collect();
    rule.all_behaviours = behaviours
        .iter()
        .enumerate()
        .map(|(i, b)| Ref::Own(Box::new(b.record(old.get(i).and_then(Option::as_deref)))))
        .collect();
    for list in [
        &mut rule.init_behaviours,
        &mut rule.update_behaviours,
        &mut rule.update_finalize_behaviours,
        &mut rule.draw_behaviours,
        &mut rule.release_behaviours,
    ] {
        list.clear();
    }
}

/// The lists naming a rule's behaviours by their place under `path`, whatever they named before.
fn name_behaviours<T: DrawableTarget>(rule: &mut PtxParticleRule<T>, path: &str) {
    let classes: Vec<[bool; 5]> = behaviours(rule).iter().map(Behaviour::lists).collect();
    let mut lists: [Vec<Ref<Box<_>>>; 5] = Default::default();
    for (i, member) in classes.iter().enumerate() {
        for (list, is) in lists.iter_mut().zip(member) {
            if *is {
                list.push(Ref::To(format!("{path}.all_behaviours[{i}]").into()));
            }
        }
    }
    let same = |a: &[Ref<Box<_>>], b: &[Ref<Box<_>>]| a == b;
    let [init, update, finalize, draw, release] = lists;
    if !same(&rule.init_behaviours, &init) {
        rule.init_behaviours = init;
    }
    if !same(&rule.update_behaviours, &update) {
        rule.update_behaviours = update;
    }
    if !same(&rule.update_finalize_behaviours, &finalize) {
        rule.update_finalize_behaviours = finalize;
    }
    if !same(&rule.draw_behaviours, &draw) {
        rule.draw_behaviours = draw;
    }
    if !same(&rule.release_behaviours, &release) {
        rule.release_behaviours = release;
    }
}

/// The shader variables a particle rule's shader sets, as the editor holds them.
pub fn shader_vars<T: DrawableTarget>(inst: &PtxShaderInst<T>) -> Vec<ShaderVar> {
    inst.inst_vars
        .iter()
        .filter_map(|v| {
            Some(match v.as_deref()? {
                PtxShaderVarAny::PtxShaderVarVector(v) => ShaderVar::Vector {
                    name_hash: v.base.hash_name.get(),
                    var_type: v.base.r#type,
                    id: v.base.id,
                    is_keyframeable: v.base.is_keyframeable.get(),
                    owns_info: v.base.owns_info.get(),
                    value: [v.vector.x, v.vector.y, v.vector.z, v.vector.w],
                },
                PtxShaderVarAny::PtxShaderVarTexture(v) => ShaderVar::Texture {
                    name_hash: v.base.hash_name.get(),
                    id: v.base.id,
                    is_keyframeable: v.base.is_keyframeable.get(),
                    owns_info: v.base.owns_info.get(),
                    texture_name: v.texture_name.clone(),
                    texture_hash_name: v.texture_hash_name.get(),
                    external_reference: v.external_reference.get(),
                },
                PtxShaderVarAny::PtxShaderVarKeyframe(v) => ShaderVar::Keyframe {
                    name_hash: v.base.hash_name.get(),
                    id: v.base.id,
                    is_keyframeable: v.base.is_keyframeable.get(),
                    owns_info: v.base.owns_info.get(),
                    entries: v.keyframe.entries.iter().map(raw_entry).collect(),
                },
                PtxShaderVarAny::Base(_) => return None,
            })
        })
        .collect()
}

/// The evolution lists of an effect and of each of its events.
pub fn evolution_lists<T: DrawableTarget>(
    rule: &PtxEffectRule<T>,
) -> (Option<EvolutionList>, Vec<Option<EvolutionList>>) {
    (
        rule.evolution_list.as_deref().map(EvolutionList::of),
        events(rule)
            .map(|e| e.base.evolution_list.as_deref().map(EvolutionList::of))
            .collect(),
    )
}

/// `rule` keyed in `dict` by the code of its name, in the ascending order a lookup assumes; `false` where a rule of that name is already there.
fn insert<R: Owned>(dict: &mut Option<Box<PgDictionary<R>>>, rule: R, name: &str) -> bool
where
    Raw<R>: Record,
    PgDictionary<R>: Default,
{
    let d = dict.get_or_insert_with(Default::default);
    let code = code(name);
    if d.codes.contains(&code) {
        return false;
    }
    let at = d.codes.partition_point(|c| *c < code);
    d.codes.insert(at, code);
    d.entries
        .insert(at.min(d.entries.len()), Some(Box::new(rule)));
    true
}

/// The entry `dict` keys by the code of `name`, taken out with its code.
fn remove<R: Owned>(dict: &mut Option<Box<PgDictionary<R>>>, name: &str) -> Option<R>
where
    Raw<R>: Record,
{
    let d = dict.as_deref_mut()?;
    let at = d.codes.iter().position(|c| *c == code(name))?;
    d.codes.remove(at);
    d.entries.remove(at).map(|e| *e)
}

/// A new effect rule named `name`; `false` where one of that name exists.
pub fn add_effect<T: DrawableTarget>(list: &mut PtxFxList<T>, name: &str) -> bool {
    let rule = PtxEffectRule {
        name: name.to_string(),
        file_version: 4.2,
        duration_min: 1.0,
        duration_max: 1.0,
        playback_rate_scalar_min: 1.0,
        playback_rate_scalar_max: 1.0,
        zoom_level: 100.0,
        ..Default::default()
    };
    insert(&mut list.effect_rule_dict, rule, name)
}

/// A copy of the effect rule named `from`, named `name`.
pub fn duplicate_effect<T: DrawableTarget + Clone>(
    list: &mut PtxFxList<T>,
    from: &str,
    name: &str,
) -> bool {
    let Some(mut rule) = effect_rule(list, from).cloned() else {
        return false;
    };
    rule.name = name.to_string();
    insert(&mut list.effect_rule_dict, rule, name)
}

/// A new emitter rule named `name`; `false` where one of that name exists.
pub fn add_emitter_rule<T: DrawableTarget>(list: &mut PtxFxList<T>, name: &str) -> bool {
    let rule = PtxEmitterRule {
        name: name.to_string(),
        file_version: 4.1,
        ..Default::default()
    };
    insert(&mut list.emitter_rule_dict, rule, name)
}

/// A new particle rule named `name`; `false` where one of that name exists.
pub fn add_particle_rule<T: DrawableTarget>(list: &mut PtxFxList<T>, name: &str) -> bool {
    let rule = PtxParticleRule {
        name: name.to_string(),
        file_version: 4.0,
        render_state: PtxRenderState {
            blend_set: 1,
            depth_test: Bool8::new(true),
            alpha_blend: Bool8::new(true),
            ..Default::default()
        },
        shader_inst: PtxShaderInst {
            is_data_in_sync: Bool8::new(true),
            technique_desc: PtxTechniqueDesc {
                is_soft: Bool8::new(true),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    insert(&mut list.particle_rule_dict, rule, name)
}

pub fn remove_effect<T: DrawableTarget>(list: &mut PtxFxList<T>, name: &str) -> bool {
    remove(&mut list.effect_rule_dict, name).is_some()
}

pub fn remove_emitter_rule<T: DrawableTarget>(list: &mut PtxFxList<T>, name: &str) -> bool {
    remove(&mut list.emitter_rule_dict, name).is_some()
}

pub fn remove_particle_rule<T: DrawableTarget>(list: &mut PtxFxList<T>, name: &str) -> bool {
    remove(&mut list.particle_rule_dict, name).is_some()
}

/// The effect rule named `from` renamed `to`, and every spawner naming it; `false` where `to` is taken or `from` is missing.
pub fn rename_effect<T: DrawableTarget>(list: &mut PtxFxList<T>, from: &str, to: &str) -> bool {
    if from == to {
        return effect_rule(list, from).is_some();
    }
    if effect_rule(list, to).is_some() {
        return false;
    }
    let Some(mut rule) = remove(&mut list.effect_rule_dict, from) else {
        return false;
    };
    rule.name = to.to_string();
    insert(&mut list.effect_rule_dict, rule, to);
    for rule in particle_rules_mut(list) {
        for spawner in [
            &mut rule.effect_spawner_at_ratio,
            &mut rule.effect_spawner_on_coln,
        ] {
            if spawner.name == from {
                spawner.name = to.to_string();
            }
        }
    }
    true
}

/// The emitter rule named `from` renamed `to`, and every event naming it.
pub fn rename_emitter_rule<T: DrawableTarget>(
    list: &mut PtxFxList<T>,
    from: &str,
    to: &str,
) -> bool {
    if from == to {
        return emitter_rule(list, from).is_some();
    }
    if emitter_rule(list, to).is_some() {
        return false;
    }
    let Some(mut rule) = remove(&mut list.emitter_rule_dict, from) else {
        return false;
    };
    rule.name = to.to_string();
    insert(&mut list.emitter_rule_dict, rule, to);
    for event in effect_rules_mut(list).flat_map(|r| events_mut(r)) {
        if event.emitter_rule_name == from {
            event.emitter_rule_name = to.to_string();
        }
    }
    true
}

/// The particle rule named `from` renamed `to`, and every event naming it.
pub fn rename_particle_rule<T: DrawableTarget>(
    list: &mut PtxFxList<T>,
    from: &str,
    to: &str,
) -> bool {
    if from == to {
        return particle_rule(list, from).is_some();
    }
    if particle_rule(list, to).is_some() {
        return false;
    }
    let Some(mut rule) = remove(&mut list.particle_rule_dict, from) else {
        return false;
    };
    rule.name = to.to_string();
    insert(&mut list.particle_rule_dict, rule, to);
    for event in effect_rules_mut(list).flat_map(|r| events_mut(r)) {
        if event.particle_rule_name == from {
            event.particle_rule_name = to.to_string();
        }
    }
    true
}

/// How many events name the emitter rule `name`.
pub fn emitter_references<T: DrawableTarget>(list: &PtxFxList<T>, name: &str) -> usize {
    if name.is_empty() {
        return 0;
    }
    effect_rules(list)
        .flat_map(|r| events(r))
        .filter(|e| e.emitter_rule_name == name)
        .count()
}

/// How many events name the particle rule `name`.
pub fn particle_references<T: DrawableTarget>(list: &PtxFxList<T>, name: &str) -> usize {
    if name.is_empty() {
        return 0;
    }
    effect_rules(list)
        .flat_map(|r| events(r))
        .filter(|e| e.particle_rule_name == name)
        .count()
}

/// A pointer at the rule `name` names, by key where `codes` holds it, else null.
fn by_key<R>(codes: &[u32], name: &str) -> OwnedOrKeyed<R> {
    let code = code(name);
    if !name.is_empty() && codes.contains(&code) {
        OwnedOrKeyed::Key(code)
    } else {
        OwnedOrKeyed::Own(None)
    }
}

/// Each rule held once, by its dictionary, and every event and spawner naming it by key.
///
/// ⚠ A file holds a rule where the first pointer to reach it points, and
/// every other holder reads a copy of it named by that pointer's place. Those
/// places move as soon as a dictionary gains, loses or reorders an entry, so
/// the editor edits this form instead: a dictionary entry is then the only
/// record of its rule, and the lists naming a rule's behaviours and an
/// evolution list's values are named by the place their rule has here.
pub fn canonical<T: DrawableTarget>(list: &mut PtxFxList<T>) {
    // A rule an event or a spawner owns that no dictionary holds joins its dictionary.
    let mut emitters: Vec<PtxEmitterRule> = Vec::new();
    let mut particles: Vec<PtxParticleRule<T>> = Vec::new();
    let mut effects: Vec<PtxEffectRule<T>> = Vec::new();
    fn take<R>(slot: &mut OwnedOrKeyed<R>, into: &mut Vec<R>) {
        if let OwnedOrKeyed::Own(rule) = slot {
            if let Some(rule) = rule.take() {
                into.push(*rule);
            }
        }
    }
    for rule in effect_rules_mut(list) {
        for event in events_mut(rule) {
            take(&mut event.emitter_rule, &mut emitters);
            take(&mut event.particle_rule, &mut particles);
        }
    }
    for rule in particle_rules_mut(list) {
        take(&mut rule.effect_spawner_at_ratio.effect_rule, &mut effects);
        take(&mut rule.effect_spawner_on_coln.effect_rule, &mut effects);
    }
    let orphans_of = |effects: &mut Vec<PtxEffectRule<T>>,
                      emitters: &mut Vec<PtxEmitterRule>,
                      particles: &mut Vec<PtxParticleRule<T>>| {
        for rule in effects.iter_mut() {
            for event in events_mut(rule) {
                take(&mut event.emitter_rule, emitters);
                take(&mut event.particle_rule, particles);
            }
        }
    };
    orphans_of(&mut effects, &mut emitters, &mut particles);
    for rule in emitters {
        let name = rule.name.clone();
        insert(&mut list.emitter_rule_dict, rule, &name);
    }
    for rule in particles {
        let name = rule.name.clone();
        insert(&mut list.particle_rule_dict, rule, &name);
    }
    for rule in effects {
        let name = rule.name.clone();
        insert(&mut list.effect_rule_dict, rule, &name);
    }
    fresh(&mut list.emitter_rule_dict);
    fresh(&mut list.particle_rule_dict);
    fresh(&mut list.effect_rule_dict);

    let emitter_codes = codes_of(&list.emitter_rule_dict);
    let particle_codes = codes_of(&list.particle_rule_dict);
    let effect_codes = codes_of(&list.effect_rule_dict);
    for (k, rule) in effect_rules_mut(list).enumerate() {
        let path = format!("effect_rule_dict.entries[{k}]");
        if let Some(l) = rule.evolution_list.as_deref_mut() {
            EvolutionList::of(l).write_to(l, &format!("{path}.evolution_list"));
        }
        for (i, event) in events_mut(rule).enumerate() {
            event.emitter_rule = by_key(&emitter_codes, &event.emitter_rule_name);
            event.particle_rule = by_key(&particle_codes, &event.particle_rule_name);
            if let Some(l) = event.base.evolution_list.as_deref_mut() {
                EvolutionList::of(l).write_to(
                    l,
                    &format!("{path}.timeline.events[{i}].base.evolution_list"),
                );
            }
        }
    }
    for (k, rule) in particle_rules_mut(list).enumerate() {
        for spawner in [
            &mut rule.effect_spawner_at_ratio,
            &mut rule.effect_spawner_on_coln,
        ] {
            spawner.effect_rule = by_key(&effect_codes, &spawner.name);
        }
        name_behaviours(rule, &format!("particle_rule_dict.entries[{k}]"));
    }
}

fn codes_of<R: Owned>(dict: &Option<Box<PgDictionary<R>>>) -> Vec<u32>
where
    Raw<R>: Record,
{
    dict.as_deref().map_or_else(Vec::new, |d| d.codes.clone())
}

/// A dictionary laid out anew: one entry per code, ascending, and nothing its old record said of copies.
fn fresh<R: Owned>(dict: &mut Option<Box<PgDictionary<R>>>)
where
    Raw<R>: Record,
    PgDictionary<R>: Default,
{
    let Some(old) = dict.as_deref_mut() else {
        return;
    };
    let mut held: Vec<(u32, Box<R>)> = std::mem::take(&mut old.codes)
        .into_iter()
        .zip(std::mem::take(&mut old.entries))
        .filter_map(|(c, e)| Some((c, e?)))
        .collect();
    held.sort_by_key(|(c, _)| *c);
    held.dedup_by_key(|(c, _)| *c);
    let (codes, entries) = held.into_iter().map(|(c, e)| (c, Some(e))).unzip();
    *dict = Some(Box::new(PgDictionary {
        codes,
        entries,
        ..Default::default()
    }));
}
