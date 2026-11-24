//! Rules, events, behaviours and keys taken out in place, and events bound to other rules, over
//! the body the parent lays out and copies of its rules appended to it.

use super::*;

const SECOND_EMITTER: &str = "test_spray_emit_two";
const SECOND_PARTICLE: &str = "test_spray_two";

/// A copy of the `len`-byte rule at `from` appended under `name`, pointed at from `name_at`.
fn copy_rule(b: &mut Body, from: usize, len: usize, name: &str, name_at: usize) -> usize {
    let at = b.alloc(len);
    let bytes = b.v[from..from + len].to_vec();
    b.put(at, &bytes);
    let text = b.text(name);
    b.ptr(at + name_at, text);
    b.u32(at + 0x10, 1);
    at
}

/// The dictionary the root points at from `slot` made to hold the two rules given, by code.
fn two(b: &mut Body, slot: usize, rules: [(&str, usize); 2]) {
    let mut coded = rules.map(|(name, at)| (hash(name), at));
    coded.sort_unstable();
    let codes = b.alloc(8);
    let entries = b.alloc(16);
    for (i, (code, at)) in coded.into_iter().enumerate() {
        b.u32(codes + 4 * i, code);
        b.ptr(entries + 8 * i, at);
    }
    let d = (u64_at(&b.v, slot) & 0x0FFF_FFFF) as usize;
    b.array(d + 0x20, codes, 2, 2);
    b.array(d + 0x30, entries, 2, 2);
}

/// The body with a second emitter and a second particle rule, copies of the first under names of
/// their own that no event names.
fn two_of_each() -> Vec<u8> {
    let (v, at) = body();
    let mut b = Body { v };
    let emitter = copy_rule(&mut b, at.emitter, 0x300, SECOND_EMITTER, 0x20);
    b.u32(emitter + 0x1C, hash(SECOND_EMITTER));
    let kfps: Vec<usize> = (0..10).map(|i| emitter + 0x48 + 0x40 * i).collect();
    b.kfp_list(emitter + 0x2C8, &kfps, 10);
    let particle = copy_rule(&mut b, at.particle, 0x430, SECOND_PARTICLE, 0x178);
    two(
        &mut b,
        0x30,
        [(EMITTER, at.emitter), (SECOND_EMITTER, emitter)],
    );
    two(
        &mut b,
        0x38,
        [(PARTICLE, at.particle), (SECOND_PARTICLE, particle)],
    );
    b.v
}

fn emitter_named(list: &ParticleEffects, name: &str) -> usize {
    (list.emitters.iter().position(|r| r.name == name)).expect("the emitter rule")
}

fn particle_named(list: &ParticleEffects, name: &str) -> usize {
    (list.particles.iter().position(|r| r.name == name)).expect("the particle rule")
}

#[test]
fn a_list_of_two_rules_of_each_kind_reads() {
    let list = read_list(&two_of_each()).expect("it reads");
    assert_eq!((list.emitters.len(), list.particles.len()), (2, 2));
    let event = &list.effects[0].events[0];
    assert_eq!(list.emitters[event.emitter].name, EMITTER);
    assert_eq!(list.particles[event.particle].name, PARTICLE);
}

#[test]
fn an_event_bound_to_other_rules_points_at_them_and_each_rule_counts_its_events() {
    let mut v = two_of_each();
    let size = v.len();
    let mut list = read_list(&v).expect("it reads");
    let (emitter, particle) = (
        emitter_named(&list, SECOND_EMITTER),
        particle_named(&list, SECOND_PARTICLE),
    );
    let (first_emitter, first_particle) = (
        emitter_named(&list, EMITTER),
        particle_named(&list, PARTICLE),
    );
    list.effects[0].events[0].emitter = emitter;
    list.effects[0].events[0].particle = particle;
    write_particle_effects(&mut v, &list).expect("it writes");
    assert_eq!(v.len(), size);
    let back = read_list(&v).expect("it reads again");
    assert_eq!(back, list);
    let count = |at: u64| u32_at(&v, at as usize + 0x10);
    assert_eq!(count(back.emitters[emitter].at), 2);
    assert_eq!(count(back.emitters[first_emitter].at), 1);
    assert_eq!(count(back.particles[particle].at), 2);
    assert_eq!(count(back.particles[first_particle].at), 1);

    // A rule the list does not hold, which writes nothing.
    let before = v.clone();
    let mut wrong = back.clone();
    wrong.effects[0].events[0].emitter = 5;
    assert!(write_particle_effects(&mut v, &wrong).is_err());
    assert_eq!(v, before);
}

#[test]
fn a_key_removed_shortens_its_value_in_place_and_the_key_after_it_takes_its_step_again() {
    let (mut v, _) = body();
    let size = v.len();
    let mut list = read_list(&v).expect("it reads");
    list.emitters[0].kfps[0].entries.remove(0);
    write_particle_effects(&mut v, &list).expect("it writes");
    assert_eq!(v.len(), size);
    let back = read_list(&v).expect("it reads again");
    let first = KfpEntry {
        time: [1.0, 0.0, 0.0, 0.0],
        value: [20.0, 30.0, 0.0, 0.0],
    };
    assert_eq!(back.emitters[0].kfps[0].entries, [first]);

    // The last key removed leaves a value of no keys.
    let mut list = back;
    list.emitters[0].kfps[0].entries.clear();
    write_particle_effects(&mut v, &list).expect("it writes");
    let back = read_list(&v).expect("it reads again");
    assert!(back.emitters[0].kfps[0].entries.is_empty());
}

#[test]
fn a_key_moved_in_time_takes_the_reciprocal_of_its_new_step() {
    let (mut v, _) = body();
    let mut list = read_list(&v).expect("it reads");
    list.emitters[0].kfps[0].entries[1].time[0] = 0.5;
    write_particle_effects(&mut v, &list).expect("it writes");
    let back = read_list(&v).expect("it reads again");
    assert_eq!(
        back.emitters[0].kfps[0].entries[1].time,
        [0.5, 1.0 / 0.5, 0.0, 0.0]
    );
}

#[test]
fn taking_nothing_out_leaves_the_body_byte_identical() {
    let mut v = two_of_each();
    let before = v.clone();
    remove_from_particle_effects(&mut v, &ParticleRemovals::default()).expect("nothing to remove");
    assert_eq!(v, before);
}

#[test]
fn a_rule_no_event_names_leaves_its_dictionary_in_place() {
    let mut v = two_of_each();
    let size = v.len();
    let list = read_list(&v).expect("it reads");
    let removals = ParticleRemovals {
        particles: BTreeSet::from([particle_named(&list, SECOND_PARTICLE)]),
        emitters: BTreeSet::from([emitter_named(&list, SECOND_EMITTER)]),
        ..Default::default()
    };
    remove_from_particle_effects(&mut v, &removals).expect("it removes");
    assert_eq!(v.len(), size);
    let back = read_list(&v).expect("it reads again");
    let names: Vec<&str> = back.particles.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, [PARTICLE]);
    assert_eq!(back.emitters.len(), 1);
    assert_eq!(
        (
            back.effects[0].events[0].emitter,
            back.effects[0].events[0].particle
        ),
        (0, 0)
    );
    assert_eq!(
        back.mesh_hashes, list.mesh_hashes,
        "the emitter kept has the mesh"
    );
}

#[test]
fn a_rule_an_event_names_goes_only_with_the_event_and_the_counts_follow() {
    let mut v = two_of_each();
    let list = read_list(&v).expect("it reads");
    let mut removals = ParticleRemovals {
        emitters: BTreeSet::from([emitter_named(&list, EMITTER)]),
        ..Default::default()
    };
    let before = v.clone();
    let refused = remove_from_particle_effects(&mut v, &removals).expect_err("an event names it");
    assert!(
        refused.to_string().contains("remove the event too"),
        "{refused}"
    );
    assert_eq!(v, before);
    removals.events.insert((0, 0));
    remove_from_particle_effects(&mut v, &removals).expect("it removes");
    let back = read_list(&v).expect("it reads again");
    assert!(back.effects[0].events.is_empty());
    let names: Vec<&str> = back.emitters.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, [SECOND_EMITTER]);
    let particle = back.particles[particle_named(&back, PARTICLE)].at as usize;
    assert_eq!(
        u32_at(&v, particle + 0x10),
        1,
        "no event names the particle rule now"
    );
}

#[test]
fn every_emitter_taken_out_empties_the_model_set() {
    let mut v = two_of_each();
    let removals = ParticleRemovals {
        emitters: BTreeSet::from([0, 1]),
        events: BTreeSet::from([(0, 0)]),
        ..Default::default()
    };
    remove_from_particle_effects(&mut v, &removals).expect("it removes");
    let back = read_list(&v).expect("it reads again");
    assert!(back.emitters.is_empty());
    assert!(back.mesh_hashes.is_empty());
    assert!(
        v[0x58..0x70].iter().all(|b| *b == 0),
        "a set of no keys has no buckets"
    );
}

#[test]
fn an_effect_goes_with_its_events_unless_a_spawner_starts_it() {
    let (v, at) = body();
    let mut gone = v.clone();
    let removals = ParticleRemovals {
        effects: BTreeSet::from([0]),
        ..Default::default()
    };
    remove_from_particle_effects(&mut gone, &removals).expect("it removes");
    let back = read_list(&gone).expect("it reads again");
    assert!(back.effects.is_empty());
    assert_eq!(u32_at(&gone, at.emitter + 0x10), 1);
    assert_eq!(u32_at(&gone, at.particle + 0x10), 1);

    let mut started = v;
    let effect = read_list(&started).expect("it reads").effects[0].at;
    let to = (0x5000_0000 + effect).to_le_bytes();
    started[at.particle + 0x68..at.particle + 0x70].copy_from_slice(&to);
    let before = started.clone();
    let refused =
        remove_from_particle_effects(&mut started, &removals).expect_err("a spawner starts it");
    assert!(refused.to_string().contains("spawner"), "{refused}");
    assert_eq!(started, before);
}

/// The body with a third behaviour, of no named class at bit 2, listed between Age and Size.
fn three_behaviours() -> (Vec<u8>, usize) {
    let (v, at) = body();
    let list = read_list(&v).expect("it reads");
    let listed: Vec<usize> = (list.particles[0].behaviours.iter())
        .map(|&b| list.behaviours[b].at as usize)
        .collect();
    let mut b = Body { v };
    let array = b.alloc(24);
    let third = b.alloc(0x10);
    for (i, to) in [listed[0], third, listed[1]].into_iter().enumerate() {
        b.ptr(array + 8 * i, to);
    }
    b.array(at.particle + 0x188, array, 3, 3);
    class_words(&mut b.v, at.particle, AGE_AND_SIZE | (1 << 2));
    (b.v, at.particle)
}

#[test]
fn a_behaviour_taken_out_leaves_its_rules_list_and_class_word_without_it() {
    let (mut v, rule) = three_behaviours();
    let list = read_list(&v).expect("it reads");
    assert_eq!(list.particles[0].behaviours.len(), 3);
    let removals = ParticleRemovals {
        behaviours: BTreeSet::from([(0, 1)]),
        ..Default::default()
    };
    remove_from_particle_effects(&mut v, &removals).expect("it removes");
    let back = read_list(&v).expect("it reads again");
    assert_eq!(back.particles[0].class_bits, AGE_AND_SIZE);
    assert_eq!(back.behaviours.len(), 2);
    assert_eq!(u32_at(&v, rule + 0x198), AGE_AND_SIZE);
    let classes: Vec<_> = (back.particles[0].behaviours.iter())
        .map(|&b| back.behaviours[b].class)
        .collect();
    assert_eq!(classes, [None, Some("ptxu_Size")]);

    // ptxu_Size is a class every rule lists.
    let (mut v, _) = three_behaviours();
    let before = v.clone();
    let size = ParticleRemovals {
        behaviours: BTreeSet::from([(0, 2)]),
        ..Default::default()
    };
    let refused = remove_from_particle_effects(&mut v, &size).expect_err("every rule lists it");
    assert!(
        refused.to_string().contains("every rule lists"),
        "{refused}"
    );
    assert_eq!(v, before);
}

#[test]
fn a_removal_of_what_the_list_does_not_hold_is_refused() {
    let (mut v, _) = body();
    let before = v.clone();
    for removals in [
        ParticleRemovals {
            particles: BTreeSet::from([3]),
            ..Default::default()
        },
        ParticleRemovals {
            events: BTreeSet::from([(0, 4)]),
            ..Default::default()
        },
        ParticleRemovals {
            behaviours: BTreeSet::from([(0, 9)]),
            ..Default::default()
        },
    ] {
        let refused = remove_from_particle_effects(&mut v, &removals);
        assert!(matches!(refused, Err(HeaderlessError::Particles(_))));
        assert_eq!(v, before);
    }
}
