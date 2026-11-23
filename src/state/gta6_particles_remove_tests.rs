//! A rule deleted from a scanned list's GTA V form and an event bound to another rule, as the
//! removals and the values its save writes.

use or_particle::list::{
    add_effect, canonical, duplicate_effect, effect_rules_mut, events_mut, remove_effect,
    remove_emitter_rule,
};

use super::*;
use crate::state::gta6_particles_save::{duplicates, from_ypt, remove::removals};

/// `list()` with a second particle rule that no event names.
fn two_particle_rules() -> ParticleEffects {
    let mut list = list();
    let mut second = list.particles[0].clone();
    (second.name, second.code, second.at) = ("part_b".into(), hash("part_b"), 0xC00);
    list.particles.push(second);
    list.particles.sort_by_key(|p| p.code);
    let first = (list.particles.iter().position(|p| p.name == "part_a")).expect("part_a");
    list.effects[0].events[0].particle = first;
    list
}

fn shown(list: &ParticleEffects) -> PtxFxList {
    let mut ypt = as_ypt(list);
    canonical(&mut ypt);
    ypt
}

#[test]
fn an_event_bound_to_another_rule_names_that_rule_in_the_gta_vi_list() {
    let stored = two_particle_rules();
    let mut ypt = shown(&stored);
    let effect = effect_rules_mut(&mut ypt).next().expect("an effect");
    events_mut(effect)
        .next()
        .expect("an event")
        .particle_rule_name = "part_b".into();
    assert!(removals(&stored, &ypt).is_empty(), "no rule is gone");
    let edited = from_ypt(&stored, &ypt).expect("the bind has a place");
    let second = (edited.particles.iter().position(|p| p.name == "part_b")).expect("part_b");
    assert_eq!(edited.effects[0].events[0].particle, second);

    // A name no rule of the list holds.
    let effect = effect_rules_mut(&mut ypt).next().expect("an effect");
    events_mut(effect)
        .next()
        .expect("an event")
        .particle_rule_name = "part_z".into();
    assert!(from_ypt(&stored, &ypt).is_err());
}

#[test]
fn a_rule_deleted_takes_out_the_events_naming_it_and_the_rest_saves() {
    let stored = list();
    let mut ypt = shown(&stored);
    assert!(remove_emitter_rule(&mut ypt, "emit_a"));
    let effect = effect_rules_mut(&mut ypt).next().expect("an effect");
    events_mut(effect)
        .next()
        .expect("an event")
        .emitter_rule_name
        .clear();
    let gone = removals(&stored, &ypt);
    assert_eq!(gone.emitters, BTreeSet::from([0]));
    assert_eq!(gone.events, BTreeSet::from([(0, 0)]));
    assert!(gone.effects.is_empty() && gone.particles.is_empty());

    // The list as the removal leaves it, which the save reads again before writing its values.
    let mut after = stored.clone();
    after.emitters.clear();
    after.effects[0].events.clear();
    after.mesh_hashes.clear();
    assert_eq!(from_ypt(&after, &ypt), Ok(after));

    // An event that names no rule while its rules stay is refused, not dropped.
    let mut cleared = shown(&stored);
    let effect = effect_rules_mut(&mut cleared).next().expect("an effect");
    events_mut(effect)
        .next()
        .expect("an event")
        .emitter_rule_name
        .clear();
    assert!(removals(&stored, &cleared).is_empty());
    assert!(from_ypt(&stored, &cleared).is_err());
}

#[test]
fn an_effect_duplicated_is_copied_from_the_effect_its_name_begins_with() {
    let stored = list();
    let mut ypt = shown(&stored);
    let name = stored.effects[0].name.clone();
    let copy = format!("{name}_copy_2");
    assert!(duplicate_effect(&mut ypt, &name, &copy));
    assert!(removals(&stored, &ypt).is_empty(), "no rule is gone");
    assert_eq!(duplicates(&stored, &ypt), Ok(vec![(name, copy.clone())]));

    // An effect made from nothing has no stored effect to copy.
    assert!(remove_effect(&mut ypt, &copy));
    assert!(add_effect(&mut ypt, "new_effect"));
    assert!(duplicates(&stored, &ypt).is_err());
}
