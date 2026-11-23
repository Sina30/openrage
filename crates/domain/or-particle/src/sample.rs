//! A small valid `.ypt`, for the tests of everything that reads, makes a source of or builds a particle list.

use or_io::decl::{File, Gen8};
use or_io::gta5::particle::{PtxEventEmitter, PtxFxList};

use crate::behaviour::{AccelerationBehaviour, Behaviour};
use crate::list;

/// A Legacy list called `name` that holds the `effects`, each firing one emitter rule and one particle rule of its own (`em_<effect>` and `pt_<effect>`).
pub fn list_of(name: &str, effects: &[&str]) -> Vec<u8> {
    let mut file = PtxFxList::<Gen8> {
        name: name.into(),
        ..Default::default()
    };
    for effect in effects {
        let (emitter, particle) = (format!("em_{effect}"), format!("pt_{effect}"));
        assert!(
            list::add_effect(&mut file, effect),
            "{effect} is held twice"
        );
        assert!(list::add_emitter_rule(&mut file, &emitter));
        assert!(list::add_particle_rule(&mut file, &particle));
        let rule = list::effect_rules_mut(&mut file)
            .find(|rule| rule.name == *effect)
            .expect("the effect was added");
        rule.num_loops = 2;
        rule.timeline.events.push(Some(Box::new(PtxEventEmitter {
            emitter_rule_name: emitter,
            particle_rule_name: particle.clone(),
            ..Default::default()
        })));
        let rule = list::particle_rules_mut(&mut file)
            .find(|rule| rule.name == particle)
            .expect("the particle rule was added");
        list::set_behaviours(
            rule,
            &[Behaviour::Acceleration(AccelerationBehaviour::default())],
        );
    }
    list::canonical(&mut file);
    file.to_bytes()
        .expect("a list built from the editor's own calls is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_list_opens_and_holds_the_effects_it_was_made_of() {
        let bytes = list_of("fx_test", &["spark", "smoke"]);
        let list = PtxFxList::<Gen8>::from_bytes(&bytes).unwrap();
        assert_eq!(list.name, "fx_test");
        let mut named: Vec<&str> = crate::list::effect_rules(&list)
            .map(|rule| rule.name.as_str())
            .collect();
        named.sort_unstable();
        assert_eq!(named, ["smoke", "spark"]);
    }
}
