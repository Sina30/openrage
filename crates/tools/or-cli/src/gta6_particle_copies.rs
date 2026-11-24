//! `rpf scan-particle-copies`: a scanned GTA VI particle effects list's effect rule, emitter rule,
//! particle rule, event and behaviour each copied, then the copy removed, in memory; whether the
//! list reads as stored again.

use std::collections::BTreeSet;

use or_io::container::rpf::Rpf9Sections;
use or_io::gta6::headerless::{
    copy_particle_behaviour, duplicate_particle_effect, duplicate_particle_emitter,
    duplicate_particle_event, duplicate_particle_rule, read_particle_effects,
    remove_from_particle_effects, ParticleEffects, ParticleRemovals,
};

use super::gta6_inverse::{said, sections, verdict, Outcome, READ};

const USAGE: &str = "usage: rpf scan-particle-copies <archive.rpf> <offset> [--limit <bytes>]";

/// The class bits of a sprite's, a trail's and a model's behaviour, which follow a rule's draw
/// type and are not copied.
const DRAW_BITS: [u8; 3] = [27, 28, 29];

pub(crate) fn cmd_scan_particle_copies(args: &[String]) -> Result<(), String> {
    let (_, body) = sections(args, USAGE)?;
    let stored = read_particle_effects(&body.virtual_section, None)
        .map_err(|e| format!("the list does not read: {e}"))?;
    let checks = [
        effect(&body, &stored),
        emitter(&body, &stored),
        particle(&body, &stored),
        event(&body, &stored),
        behaviour(&body, &stored),
    ];
    let mut held = true;
    for (what, outcome) in checks {
        held &= said(&what, outcome);
    }
    verdict(held)
}

/// `list` with no behaviour's size, which the bytes past the section's end change.
fn sizeless(list: &ParticleEffects) -> ParticleEffects {
    let mut list = list.clone();
    for b in &mut list.behaviours {
        b.size = 0;
    }
    list
}

/// `name` with `_copy` appended until `held` holds no such name.
fn fresh(name: &str, held: impl Fn(&str) -> bool) -> String {
    let mut out = format!("{name}_copy");
    while held(&out) {
        out.push_str("_copy");
    }
    out
}

/// `copy` made to `body`, then the `removals` the copied list gives taken out, compared as read
/// with `stored`.
fn round_trip(
    body: &Rpf9Sections,
    stored: &ParticleEffects,
    copy: impl FnOnce(&mut Rpf9Sections) -> Result<(), String>,
    removals: impl FnOnce(&ParticleEffects) -> Option<ParticleRemovals>,
) -> Outcome {
    let mut work = body.clone();
    if let Err(e) = copy(&mut work) {
        return Outcome::Refused(e);
    }
    let copied = match read_particle_effects(&work.virtual_section, None) {
        Ok(list) => list,
        Err(e) => return Outcome::Differs(format!("the list with the copy does not read: {e}")),
    };
    let Some(removals) = removals(&copied) else {
        return Outcome::Differs("the copy is not in the list it was made in".to_string());
    };
    if let Err(e) = remove_from_particle_effects(&mut work.virtual_section, &removals) {
        return Outcome::Differs(format!("the copy's removal is refused: {e}"));
    }
    match read_particle_effects(&work.virtual_section, None) {
        Ok(again) if sizeless(&again) == sizeless(stored) => Outcome::Same(READ),
        Ok(_) => Outcome::Differs("the list reads otherwise than stored".to_string()),
        Err(e) => Outcome::Differs(format!("the list does not read: {e}")),
    }
}

fn effect(body: &Rpf9Sections, stored: &ParticleEffects) -> (String, Outcome) {
    let Some(rule) = stored.effects.first() else {
        let none = Outcome::Refused("the list holds no effect rule".to_string());
        return ("an effect rule copied".to_string(), none);
    };
    let name = fresh(&rule.name, |n| stored.effects.iter().any(|e| e.name == n));
    let outcome = round_trip(
        body,
        stored,
        |b| duplicate_particle_effect(b, 0, &name).map_err(|e| e.to_string()),
        |list| {
            let at = list.effects.iter().position(|e| e.name == name)?;
            Some(ParticleRemovals {
                effects: BTreeSet::from([at]),
                ..ParticleRemovals::default()
            })
        },
    );
    let what = format!(
        "effect rule {} copied as {name}, then the copy removed",
        rule.name
    );
    (what, outcome)
}

fn emitter(body: &Rpf9Sections, stored: &ParticleEffects) -> (String, Outcome) {
    let Some(rule) = stored.emitters.first() else {
        let none = Outcome::Refused("the list holds no emitter rule".to_string());
        return ("an emitter rule copied".to_string(), none);
    };
    let name = fresh(&rule.name, |n| stored.emitters.iter().any(|e| e.name == n));
    let outcome = round_trip(
        body,
        stored,
        |b| duplicate_particle_emitter(b, 0, &name).map_err(|e| e.to_string()),
        |list| {
            let at = list.emitters.iter().position(|e| e.name == name)?;
            Some(ParticleRemovals {
                emitters: BTreeSet::from([at]),
                ..ParticleRemovals::default()
            })
        },
    );
    let what = format!(
        "emitter rule {} copied as {name}, then the copy removed",
        rule.name
    );
    (what, outcome)
}

fn particle(body: &Rpf9Sections, stored: &ParticleEffects) -> (String, Outcome) {
    let Some(rule) = stored.particles.first() else {
        let none = Outcome::Refused("the list holds no particle rule".to_string());
        return ("a particle rule copied".to_string(), none);
    };
    let name = fresh(&rule.name, |n| stored.particles.iter().any(|p| p.name == n));
    let outcome = round_trip(
        body,
        stored,
        |b| duplicate_particle_rule(b, 0, &name).map_err(|e| e.to_string()),
        |list| {
            let at = list.particles.iter().position(|p| p.name == name)?;
            Some(ParticleRemovals {
                particles: BTreeSet::from([at]),
                ..ParticleRemovals::default()
            })
        },
    );
    let what = format!(
        "particle rule {} copied as {name}, then the copy removed",
        rule.name
    );
    (what, outcome)
}

/// The first event the copy takes, copied to the end of its effect rule's timeline, then removed.
// An event holding an object at +0x60 is refused a copy, so each is tried in turn.
fn event(body: &Rpf9Sections, stored: &ParticleEffects) -> (String, Outcome) {
    let mut last = Outcome::Refused("no effect rule holds an event".to_string());
    for (e, rule) in stored.effects.iter().enumerate() {
        let count = rule.events.len();
        for i in 0..count {
            let outcome = round_trip(
                body,
                stored,
                |b| duplicate_particle_event(b, e, i).map_err(|x| x.to_string()),
                |_| {
                    Some(ParticleRemovals {
                        events: BTreeSet::from([(e, count)]),
                        ..ParticleRemovals::default()
                    })
                },
            );
            if !matches!(outcome, Outcome::Refused(_)) {
                let what = format!(
                    "event {i} of effect rule {} copied, then the copy removed",
                    rule.name
                );
                return (what, outcome);
            }
            last = outcome;
        }
    }
    ("an event copied, then the copy removed".to_string(), last)
}

/// The first behaviour whose class a particle rule lacks copied into that rule, then removed.
fn behaviour(body: &Rpf9Sections, stored: &ParticleEffects) -> (String, Outcome) {
    let pair = stored.particles.iter().enumerate().find_map(|(p, rule)| {
        let (b, bit) = (stored.behaviours.iter().enumerate()).find_map(|(b, held)| {
            let bit = held
                .class_bit
                .filter(|bit| *bit < 32 && !DRAW_BITS.contains(bit))?;
            ((rule.class_bits >> bit) & 1 == 0).then_some((b, bit))
        })?;
        Some((p, b, bit))
    });
    let Some((p, b, bit)) = pair else {
        let none = Outcome::Refused(
            "every particle rule holds a behaviour of each class the list holds".to_string(),
        );
        return ("a behaviour copied".to_string(), none);
    };
    let rule = &stored.particles[p];
    // The copy takes its class bit's place among the rule's behaviours.
    let below = (rule.class_bits & ((1u32 << bit) - 1)).count_ones() as usize;
    let place = below.min(rule.behaviours.len());
    let outcome = round_trip(
        body,
        stored,
        |s| copy_particle_behaviour(s, b, p).map_err(|e| e.to_string()),
        |_| {
            Some(ParticleRemovals {
                behaviours: BTreeSet::from([(p, place)]),
                ..ParticleRemovals::default()
            })
        },
    );
    let what = format!(
        "behaviour {b} (class bit {bit}) copied into particle rule {}, then the copy removed",
        rule.name
    );
    (what, outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_named_apart_from_every_name_held() {
        let held = ["smoke_copy", "smoke_copy_copy"];
        let taken = |n: &str| held.contains(&n);
        assert_eq!(fresh("smoke", taken), "smoke_copy_copy_copy");
        assert_eq!(fresh("fire", taken), "fire_copy");
    }
}
