use crate::effect_inst::EffectInst;
use crate::emitter_inst::{EmitterInst, EmitterRules};
use or_io::decl::{Gen8, OwnedOrKeyed};
use or_io::gta5::particle::{
    PtxEffectRule, PtxEmitterRule, PtxEvent, PtxEventEmitter, PtxFxList, PtxParticleRule,
};
use or_particle::keyframe::constant;
use or_particle::list;
use or_particle::rules::EvolutionList;
#[derive(Debug, Clone)]
pub enum PreviewTarget {
    Effect(usize),
    Emitter { effect: usize, event: usize },
    Particle(usize),
}

pub fn build(asset: &PtxFxList, target: PreviewTarget) -> Option<EffectInst> {
    match target {
        PreviewTarget::Effect(idx) => {
            let effect = list::effect_rules(asset).nth(idx)?;
            Some(build_effect(asset, effect, None))
        }
        PreviewTarget::Emitter { effect, event } => {
            let effect = list::effect_rules(asset).nth(effect)?;
            Some(build_effect(asset, effect, Some(event)))
        }
        PreviewTarget::Particle(idx) => {
            let particle = list::particle_rules(asset).nth(idx)?.clone();
            let emitter = PtxEmitterRule {
                spawn_rate_over_time_kfp: constant([50.0, 0.0, 0.0, 0.0]),
                particle_life_kfp: constant([2.0, 0.0, 0.0, 0.0]),
                ..Default::default()
            };
            let event = PtxEventEmitter::<Gen8> {
                base: PtxEvent {
                    end_ratio: 1.0,
                    ..Default::default()
                },
                playback_rate_scalar_min: 1.0,
                playback_rate_scalar_max: 1.0,
                zoom_scalar_min: 1.0,
                zoom_scalar_max: 1.0,
                colour_tint_min: 0xFFFF_FFFF,
                colour_tint_max: 0xFFFF_FFFF,
                ..Default::default()
            };
            let rules = EmitterRules::from_event(emitter, particle, &event);
            let inst = EmitterInst::new(rules);
            let rule = PtxEffectRule::<Gen8> {
                duration_min: 5.0,
                duration_max: 5.0,
                playback_rate_scalar_min: 1.0,
                playback_rate_scalar_max: 1.0,
                ..Default::default()
            };
            Some(EffectInst::new(rule, vec![inst]))
        }
    }
}

/// The rule an event plays: the one its list holds under the event's name, else the event's own.
fn rule_of<'a, R>(held: Option<&'a R>, own: &'a OwnedOrKeyed<R>) -> Option<&'a R> {
    held.or(match own {
        OwnedOrKeyed::Own(Some(rule)) => Some(&**rule),
        _ => None,
    })
}

/// `effect` as the simulation plays it; with `only`, that one event alone, over the whole effect.
fn build_effect(
    asset: &PtxFxList,
    effect: &PtxEffectRule<Gen8>,
    only: Option<usize>,
) -> EffectInst {
    let effect_list = effect.evolution_list.as_deref().map(EvolutionList::of);
    let mut emitters = Vec::new();
    for (i, evt) in list::events(effect).enumerate() {
        if only.is_some_and(|only| only != i) {
            continue;
        }
        let emitter = rule_of(
            list::emitter_rule(asset, &evt.emitter_rule_name),
            &evt.emitter_rule,
        )
        .cloned()
        .unwrap_or_default();
        let particle = rule_of(
            list::particle_rule(asset, &evt.particle_rule_name),
            &evt.particle_rule,
        )
        .cloned()
        .unwrap_or_else(PtxParticleRule::default);
        let rules = if only.is_some() {
            let mut whole = evt.clone();
            (whole.base.start_ratio, whole.base.end_ratio) = (0.0, 1.0);
            EmitterRules::from_event(emitter, particle, &whole)
        } else {
            EmitterRules::from_event(emitter, particle, evt)
        };
        let mut inst = EmitterInst::new(rules);
        let event_list = evt
            .base
            .evolution_list
            .as_deref()
            .map(EvolutionList::of)
            .or_else(|| effect_list.clone());
        if event_list.is_some() {
            inst.set_evolution_list(event_list);
        }
        emitters.push(inst);
    }
    let mut rule = effect.clone();
    rule.timeline.events.clear();
    let mut effect_inst = EffectInst::with_evolution(rule, emitters, effect_list);
    effect_inst.matrix = glam::Mat4::IDENTITY;
    effect_inst
}
