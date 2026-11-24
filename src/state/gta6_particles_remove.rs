//! What a scanned GTA VI particle list's GTA V form no longer holds, as the removals its save
//! writes first: each rule gone from its dictionary, and each kept effect's events naming one.

use std::collections::{BTreeSet, HashSet};

use or_io::gta5::particle::PtxFxList;
use or_io::gta6::headerless::{ParticleEffects, ParticleRemovals};
use or_particle::list::{effect_rules, emitter_rules, particle_rules};

/// The places in `held`'s order of the names `kept` lacks.
fn gone<'a>(kept: &HashSet<&str>, held: impl Iterator<Item = &'a str>) -> BTreeSet<usize> {
    (held.enumerate())
        .filter(|(_, name)| !kept.contains(*name))
        .map(|(i, _)| i)
        .collect()
}

/// The rules of `stored` whose names `ypt` no longer holds, and the events of the effects kept
/// that name one of them.
pub(crate) fn removals(stored: &ParticleEffects, ypt: &PtxFxList) -> ParticleRemovals {
    let effects: HashSet<&str> = effect_rules(ypt).map(|r| r.name.as_str()).collect();
    let emitters: HashSet<&str> = emitter_rules(ypt).map(|r| r.name.as_str()).collect();
    let particles: HashSet<&str> = particle_rules(ypt).map(|r| r.name.as_str()).collect();
    let mut removals = ParticleRemovals {
        effects: gone(&effects, stored.effects.iter().map(|r| r.name.as_str())),
        emitters: gone(&emitters, stored.emitters.iter().map(|r| r.name.as_str())),
        particles: gone(&particles, stored.particles.iter().map(|r| r.name.as_str())),
        ..Default::default()
    };
    for (i, effect) in stored.effects.iter().enumerate() {
        if removals.effects.contains(&i) {
            continue;
        }
        for (j, ev) in effect.events.iter().enumerate() {
            if removals.emitters.contains(&ev.emitter) || removals.particles.contains(&ev.particle)
            {
                removals.events.insert((i, j));
            }
        }
    }
    removals
}
