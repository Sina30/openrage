//! A third effect spawner a list may give a particle rule beside GTA V's two: it starts its effect
//! where the rule's `ptxu_ZCull` culls a point.

use std::collections::HashMap;

use or_io::decl::Gen8;
use or_io::gta5::particle::PtxEffectSpawner;

use crate::effect_inst::EffectInst;
use crate::emitter_inst::{EmitterInst, PendingSpawn, SpawnedScalars};
use crate::point::PointFlags;
use crate::random::RandomTable;

/// Each particle rule's on-cull spawner, by the rule's name.
pub type ZCullSpawners = HashMap<String, PtxEffectSpawner<Gen8>>;

/// Give each emitter of `effect` the on-cull spawner `spawners` holds for its particle rule.
pub fn give(effect: &mut EffectInst, spawners: &ZCullSpawners) {
    for emitter in &mut effect.emitters {
        let held = spawners.get(&emitter.rules.particle.name).cloned();
        emitter.rules.effect_spawner_on_zcull = held;
    }
}

impl EmitterInst {
    /// Queue the on-cull spawner's effect at each point culled this step at its trigger speed.
    // ponytail: the trigger value read as a speed, as the collision spawner's, is inferred.
    pub(crate) fn check_zcull_spawner(&mut self, random: &RandomTable) {
        let Some(spawner) = self.rules.effect_spawner_on_zcull.as_ref() else {
            return;
        };
        if spawner.name.is_empty() {
            return;
        }
        let threshold = spawner.trigger_info.max(0.0);
        for i in 0..self.points.len() {
            let culled = self.points.flags[i].contains(PointFlags::ZCULLED);
            if !culled || self.points.curr_vel[i].length() < threshold {
                continue;
            }
            let rand_index = self.points.rand_index[i];
            self.pending_spawns.push(PendingSpawn {
                effect_rule_name: spawner.name.clone(),
                matrix: glam::Mat4::from_translation(self.points.curr_pos[i]),
                scalars: SpawnedScalars::sample_from_spawner(random, rand_index, spawner),
                parent_remaining_life: None,
                parent_rand_index: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emitter_inst::EmitterRules;
    use or_io::gta5::particle::{PtxEffectRule, PtxEmitterRule, PtxEventEmitter, PtxParticleRule};

    fn effect(rule: &str) -> EffectInst {
        let particle = PtxParticleRule::<Gen8> {
            name: rule.into(),
            ..Default::default()
        };
        let event = PtxEventEmitter::<Gen8>::default();
        let rules = EmitterRules::from_event(PtxEmitterRule::default(), particle, &event);
        EffectInst::new(
            PtxEffectRule::<Gen8>::default(),
            vec![EmitterInst::new(rules)],
        )
    }

    #[test]
    fn a_point_its_zcull_culls_starts_the_rules_third_spawner_where_it_died() {
        let spawner = PtxEffectSpawner::<Gen8> {
            name: "splash".into(),
            trigger_info: 1.0,
            ..Default::default()
        };
        let spawners = ZCullSpawners::from([("drop".to_string(), spawner)]);
        let mut other = effect("mist");
        give(&mut other, &spawners);
        assert!(other.emitters[0].rules.effect_spawner_on_zcull.is_none());

        let mut fx = effect("drop");
        give(&mut fx, &spawners);
        let em = &mut fx.emitters[0];
        let (slow, fast, alive) = (em.points.spawn(1), em.points.spawn(2), em.points.spawn(3));
        for (i, speed) in [(slow, 0.5), (fast, 2.0), (alive, 2.0)] {
            em.points.curr_vel[i] = glam::Vec3::new(0.0, 0.0, -speed);
            em.points.curr_pos[i] = glam::Vec3::new(i as f32, 0.0, 0.0);
        }
        em.points.flags[slow].insert(PointFlags::ZCULLED);
        em.points.flags[fast].insert(PointFlags::ZCULLED);
        em.check_zcull_spawner(&RandomTable::new());
        let [spawned] = &em.pending_spawns[..] else {
            panic!("one effect started: the fast culled point's");
        };
        assert_eq!(spawned.effect_rule_name, "splash");
        assert_eq!(spawned.matrix.w_axis.x, fast as f32);
    }
}
