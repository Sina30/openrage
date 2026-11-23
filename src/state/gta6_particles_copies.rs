//! The copies a scanned GTA VI particle list's GTA V form holds that its body does not, written
//! past the body's end before its values: each emitter and particle rule duplicated, each event an
//! effect's timeline gained, and each behaviour a rule was given from another.

use std::collections::HashSet;
use std::mem::{discriminant, Discriminant};

use or_io::container::rpf::Rpf9Sections;
use or_io::decl::Ref;
use or_io::gta5::particle::{PtxBehaviourAny, PtxFxList};
use or_io::gta6::headerless::{
    copy_particle_behaviour, duplicate_particle_emitter, duplicate_particle_event,
    duplicate_particle_rule, read_particle_effects, Event, ParticleEffects, ParticleRule,
};
use or_particle::list::{effect_rules, emitter_rules, events, particle_rules, particle_rules_mut};

use super::super::gta6_particles::behaviour;

fn read(s: &Rpf9Sections) -> Result<ParticleEffects, String> {
    read_particle_effects(&s.virtual_section, None).map_err(|e| e.to_string())
}

fn not_a_copy() -> String {
    tr!("A GTA VI particle list takes a new emitter or particle rule only as a copy of one it holds: use Duplicate, and keep the copy's name the name of the rule it copies followed by _copy.").to_string()
}

/// Each name of `shown` that `stored` lacks, with the stored name it copies: the one whose name
/// followed by `_copy` begins it, the longest where several do.
fn copied(stored: &[&str], shown: &[&str]) -> Result<Vec<(String, String)>, String> {
    let held: HashSet<&str> = stored.iter().copied().collect();
    let mut out = Vec::new();
    for name in shown.iter().filter(|n| !held.contains(**n)) {
        let from = (stored.iter())
            .filter(|s| name.starts_with(&format!("{s}_copy")))
            .max_by_key(|s| s.len());
        let Some(from) = from else {
            return Err(not_a_copy());
        };
        out.push((from.to_string(), name.to_string()));
    }
    Ok(out)
}

/// The copies `ypt` holds that `s` does not, written into `s`: rules first, then events, then
/// behaviours, each from a record of its kind `s` holds.
pub(crate) fn written(s: &mut Rpf9Sections, ypt: &PtxFxList) -> Result<(), String> {
    let stored = read(s)?;
    let held: Vec<&str> = stored.emitters.iter().map(|r| r.name.as_str()).collect();
    let shown: Vec<&str> = emitter_rules(ypt).map(|r| r.name.as_str()).collect();
    for (from, name) in copied(&held, &shown)? {
        let list = read(s)?;
        let source = list.emitters.iter().position(|r| r.name == from);
        let source = source.ok_or_else(not_a_copy)?;
        duplicate_particle_emitter(s, source, &name).map_err(|e| e.to_string())?;
    }
    let held: Vec<&str> = stored.particles.iter().map(|r| r.name.as_str()).collect();
    let shown: Vec<&str> = particle_rules(ypt).map(|r| r.name.as_str()).collect();
    for (from, name) in copied(&held, &shown)? {
        let list = read(s)?;
        let source = list.particles.iter().position(|r| r.name == from);
        let source = source.ok_or_else(not_a_copy)?;
        duplicate_particle_rule(s, source, &name).map_err(|e| e.to_string())?;
    }
    events_written(s, ypt)?;
    behaviours_written(s, ypt)
}

/// Each event a timeline of `ypt` holds past the events its effect holds in `s`, copied from the
/// effect's event naming the same two rules, else its first.
fn events_written(s: &mut Rpf9Sections, ypt: &PtxFxList) -> Result<(), String> {
    let stored = read(s)?;
    for (e, effect) in stored.effects.iter().enumerate() {
        let Some(shown) = effect_rules(ypt).find(|r| r.name == effect.name) else {
            continue;
        };
        // An event a rule's deletion left naming no rule goes with that rule.
        let bound = events(shown)
            .filter(|ev| !ev.emitter_rule_name.is_empty() && !ev.particle_rule_name.is_empty());
        for added in bound.skip(effect.events.len()) {
            let same = |ev: &Event| {
                let emitter = stored.emitters.get(ev.emitter).map(|r| r.name.as_str());
                let particle = stored.particles.get(ev.particle).map(|r| r.name.as_str());
                emitter == Some(added.emitter_rule_name.as_str())
                    && particle == Some(added.particle_rule_name.as_str())
            };
            let first = (!effect.events.is_empty()).then_some(0);
            let Some(source) = effect.events.iter().position(same).or(first) else {
                return Err(tr!("An event was added to an effect that holds none to copy: a GTA VI particle list takes a new event only as a copy of one of its effect's events, so use Duplicate on an event.").to_string());
            };
            duplicate_particle_event(s, e, source).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The GTA V forms of the behaviours `rule` of `list` lists, in its order, with each one's index.
fn shown_of(list: &ParticleEffects, rule: &ParticleRule) -> Vec<(usize, PtxBehaviourAny)> {
    (rule.behaviours.iter().enumerate())
        .filter_map(|(place, &b)| Some((b, behaviour(list.behaviours.get(b)?, place)?)))
        .collect()
}

fn kind(b: &PtxBehaviourAny) -> Discriminant<PtxBehaviourAny> {
    discriminant(b)
}

/// Each behaviour a rule of `ypt` holds of a class its rule in `s` shows none of, copied from the
/// behaviour of `s` whose GTA V form it is, else from one of its class.
fn behaviours_written(s: &mut Rpf9Sections, ypt: &PtxFxList) -> Result<(), String> {
    let stored = read(s)?;
    let forms: Vec<(usize, PtxBehaviourAny)> = (stored.particles.iter())
        .flat_map(|rule| shown_of(&stored, rule))
        .collect();
    for (p, rule) in stored.particles.iter().enumerate() {
        let Some(shown) = particle_rules(ypt).find(|r| r.name == rule.name) else {
            continue;
        };
        let kinds: Vec<_> = shown_of(&stored, rule)
            .iter()
            .map(|(_, b)| kind(b))
            .collect();
        let owned = shown.all_behaviours.iter().filter_map(|b| match b {
            Ref::Own(b) => Some(&**b),
            Ref::To(_) => None,
        });
        for added in owned.filter(|b| !kinds.contains(&kind(b))) {
            let found = (forms.iter().find(|(_, f)| f == added))
                .or_else(|| forms.iter().find(|(_, f)| kind(f) == kind(added)));
            let Some(&(source, _)) = found else {
                return Err(tr!("A rule was given a behaviour of a class no rule of the GTA VI particle list holds: a new behaviour is written only as a copy of one the list holds, so use Copy to rule.").to_string());
            };
            copy_particle_behaviour(s, source, p).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// `ypt` with each rule's behaviours in the order `stored`, its list once the copies are written,
/// lists them, as the values are matched to the list's behaviours in that order.
pub(crate) fn ordered(stored: &ParticleEffects, ypt: &PtxFxList) -> PtxFxList {
    let mut out = ypt.clone();
    for rule in particle_rules_mut(&mut out) {
        let Some(held) = stored.particles.iter().find(|p| p.name == rule.name) else {
            continue;
        };
        let kinds: Vec<_> = shown_of(stored, held)
            .iter()
            .map(|(_, b)| kind(b))
            .collect();
        rule.all_behaviours.sort_by_key(|b| match b {
            Ref::Own(b) => kinds
                .iter()
                .position(|k| *k == kind(b))
                .unwrap_or(usize::MAX),
            Ref::To(_) => usize::MAX,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_name_is_a_copy_of_the_longest_stored_name_it_begins_with() {
        let stored = ["fx", "fx_copy"];
        let found = copied(&stored, &["fx", "fx_copy", "fx_copy_copy", "fx_copy_2"]);
        let found = found.expect("each is a copy");
        let pairs: Vec<(&str, &str)> = (found.iter())
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        assert_eq!(pairs, [("fx_copy", "fx_copy_copy"), ("fx", "fx_copy_2")]);
        assert!(copied(&stored, &["other"]).is_err());
        assert_eq!(copied(&stored, &stored).ok(), Some(Vec::new()));
    }

    #[test]
    fn a_rule_the_list_does_not_hold_keeps_its_behaviours_order() {
        let mut ypt = PtxFxList::default();
        or_particle::list::add_particle_rule(&mut ypt, "pt");
        let size = PtxBehaviourAny::PtxuSize(Default::default());
        let age = PtxBehaviourAny::PtxuAge(Default::default());
        if let Some(rule) = particle_rules_mut(&mut ypt).next() {
            rule.all_behaviours = vec![Ref::Own(Box::new(size)), Ref::Own(Box::new(age))];
        }
        let none = ParticleEffects {
            name: String::new(),
            registry: Vec::new(),
            effects: Vec::new(),
            emitters: Vec::new(),
            particles: Vec::new(),
            behaviours: Vec::new(),
            drawable_hashes: Default::default(),
            mesh_hashes: Default::default(),
        };
        assert_eq!(ordered(&none, &ypt), ypt);
    }
}
