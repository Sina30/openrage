//! A particle list built in code: one effect whose events each fire the one
//! emitter rule with the one particle rule.

// Each suite that declares this module calls a part of it.
#![allow(dead_code)]

use or_io::decl::Gen8;
use or_io::gta5::particle::{
    PtxEffectRule, PtxEmitterRule, PtxEventEmitter, PtxFxList, PtxParticleRule,
};
use or_particle::behaviour::Behaviour;
use or_particle::keyframe::constant;
use or_particle::list;

pub const EFFECT: &str = "test_effect";
pub const EMITTER: &str = "test_emitter";
pub const PARTICLE: &str = "test_particle";

/// `events` events over an effect lasting `duration`, each spawning `rate`
/// particles a second that live `life` seconds and run `behaviours`.
pub fn list_of(
    events: usize,
    duration: f32,
    rate: f32,
    life: f32,
    behaviours: &[Behaviour],
) -> PtxFxList {
    let mut list = PtxFxList {
        name: "test".into(),
        ..Default::default()
    };
    list::add_effect(&mut list, EFFECT);
    list::add_emitter_rule(&mut list, EMITTER);
    list::add_particle_rule(&mut list, PARTICLE);
    let emitter = emitter(&mut list);
    emitter.spawn_rate_over_time_kfp = constant([rate, 0.0, 0.0, 0.0]);
    emitter.particle_life_kfp = constant([life, 0.0, 0.0, 0.0]);
    list::set_behaviours(particle(&mut list), behaviours);
    let effect = effect(&mut list, EFFECT);
    (effect.duration_min, effect.duration_max) = (duration, duration);
    for i in 0..events {
        let mut event = PtxEventEmitter::<Gen8>::default();
        event.base.index = i as i32;
        event.base.end_ratio = 1.0;
        event.emitter_rule_name = EMITTER.into();
        event.particle_rule_name = PARTICLE.into();
        (
            event.playback_rate_scalar_min,
            event.playback_rate_scalar_max,
        ) = (1.0, 1.0);
        (event.zoom_scalar_min, event.zoom_scalar_max) = (1.0, 1.0);
        (event.colour_tint_min, event.colour_tint_max) = (0xFFFF_FFFF, 0xFFFF_FFFF);
        effect.timeline.events.push(Some(Box::new(event)));
    }
    list
}

/// Another effect, with no event, lasting `duration`.
pub fn add_effect(list: &mut PtxFxList, name: &str, duration: f32) {
    list::add_effect(list, name);
    let effect = effect(list, name);
    (effect.duration_min, effect.duration_max) = (duration, duration);
}

/// Where the effect named `name` sits among the list's effects.
pub fn index_of(list: &PtxFxList, name: &str) -> usize {
    list::effect_rules(list)
        .position(|e| e.name == name)
        .expect("the list holds the effect")
}

pub fn effect<'a>(list: &'a mut PtxFxList, name: &str) -> &'a mut PtxEffectRule<Gen8> {
    list::effect_rules_mut(list)
        .find(|e| e.name == name)
        .expect("the list holds the effect")
}

pub fn emitter(list: &mut PtxFxList) -> &mut PtxEmitterRule {
    list::emitter_rules_mut(list)
        .next()
        .expect("the list holds its emitter rule")
}

pub fn particle(list: &mut PtxFxList) -> &mut PtxParticleRule<Gen8> {
    list::particle_rules_mut(list)
        .next()
        .expect("the list holds its particle rule")
}
