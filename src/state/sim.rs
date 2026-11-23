use std::collections::HashMap;
use std::time::Instant;

use or_io::decl::Heap;
use or_io::gta5::PtxFxList;
use or_simulation::manager::{EmitterSnapshot, SnapshotFrame};
use or_simulation::{build, EffectInst, EmitterInst, PointBuffer, PreviewTarget, PtfxManager};

use super::memory::{hash_table_bytes, vec_bytes};
use super::DocumentId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimBuildKey {
    pub doc_id: DocumentId,
    pub target: SerialisableTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SerialisableTarget {
    Effect(usize),
    Emitter { effect: usize, event: usize },
    Particle(usize),
    None,
}

impl SerialisableTarget {
    pub fn into_preview(self) -> Option<PreviewTarget> {
        match self {
            SerialisableTarget::Effect(i) => Some(PreviewTarget::Effect(i)),
            SerialisableTarget::Emitter { effect, event } => {
                Some(PreviewTarget::Emitter { effect, event })
            }
            SerialisableTarget::Particle(i) => Some(PreviewTarget::Particle(i)),
            SerialisableTarget::None => None,
        }
    }
}

pub struct SimSession {
    pub manager: PtfxManager,
    pub last_built: Option<SimBuildKey>,
    last_tick: Option<Instant>,
    pub pending_rebuild: bool,
    pub last_sim_ms: f32,
    pub pinned_effects: Vec<SimBuildKey>,
    pub pin_offsets: HashMap<SimBuildKey, f32>,
}

impl Default for SimSession {
    fn default() -> Self {
        Self {
            manager: PtfxManager::new(),
            last_built: None,
            last_tick: None,
            pending_rebuild: false,
            last_sim_ms: 0.0,
            pinned_effects: Vec::new(),
            pin_offsets: HashMap::new(),
        }
    }
}

impl SimSession {
    pub fn step_real_dt(&mut self) -> f32 {
        let now = Instant::now();
        let dt = self
            .last_tick
            .replace(now)
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.0);
        dt.max(0.0)
    }

    pub fn seek(&mut self, t: f32) {
        self.manager.clock.seek(t);
    }

    pub fn needs_rebuild(&self, doc_id: DocumentId, target: &SerialisableTarget) -> bool {
        if self.pending_rebuild {
            return true;
        }
        match &self.last_built {
            Some(k) => k.doc_id != doc_id || &k.target != target,
            None => true,
        }
    }

    pub fn rebuild(
        &mut self,
        doc_id: DocumentId,
        asset: &or_io::gta5::PtxFxList,
        target: SerialisableTarget,
    ) {
        let key = SimBuildKey { doc_id, target };
        if !self.pending_rebuild && self.last_built.as_ref() == Some(&key) {
            return;
        }
        self.last_built = Some(key);
        self.pending_rebuild = false;
        let Some(preview) = target.into_preview() else {
            self.manager.clear_effect();
            return;
        };
        let Some(inst) = build(asset, preview) else {
            self.manager.clear_effect();
            return;
        };
        self.manager.set_asset_snapshot(asset.clone());
        self.manager.set_effect(inst);
        self.manager.spawned_effects.clear();
        for pin in self.pinned_effects.iter().filter(|p| p.doc_id == doc_id) {
            if let Some(preview) = pin.target.into_preview() {
                if let Some(mut inst) = build(asset, preview) {
                    if let Some(off) = self.pin_offsets.get(pin).copied() {
                        if off.abs() > 1e-6 && inst.duration > 0.0 {
                            inst.time_seconds = off.rem_euclid(inst.duration);
                        }
                    }
                    self.manager.spawned_effects.push(inst);
                }
            }
        }
        self.manager.clock.play();
    }

    /// What the simulation keeps: the rules it runs from, each emitter's particles, the snapshots a seek replays from and the pins.
    pub fn held_bytes(&self) -> usize {
        let manager = &self.manager;
        let rules = (manager.asset_snapshot.as_ref()).map_or(0, |asset| {
            std::mem::size_of::<PtxFxList>() + asset.heap_bytes()
        });
        let live: usize = (manager.effect.iter().chain(&manager.spawned_effects))
            .map(effect_bytes)
            .sum();
        let frames: usize = manager.snapshots.iter().map(frame_bytes).sum();
        std::mem::size_of::<PtfxManager>()
            + rules
            + live
            + vec_bytes(&manager.spawned_effects)
            + vec_bytes(&manager.snapshots)
            + frames
            + vec_bytes(&self.pinned_effects)
            + hash_table_bytes::<(SimBuildKey, f32)>(self.pin_offsets.capacity())
    }

    pub fn toggle_pin(&mut self, key: SimBuildKey) -> bool {
        if let Some(pos) = self.pinned_effects.iter().position(|p| p == &key) {
            self.pinned_effects.remove(pos);
            self.pending_rebuild = true;
            false
        } else {
            self.pinned_effects.push(key);
            self.pending_rebuild = true;
            true
        }
    }

    pub fn set_pin_offset(&mut self, key: SimBuildKey, offset_seconds: f32) -> f32 {
        let prev = self.pin_offsets.get(&key).copied().unwrap_or(0.0);
        if (prev - offset_seconds).abs() < 1e-6 {
            return prev;
        }
        if offset_seconds.abs() < 1e-6 {
            self.pin_offsets.remove(&key);
        } else {
            self.pin_offsets.insert(key, offset_seconds);
        }
        self.pending_rebuild = true;
        prev
    }

    pub fn pin_offset(&self, key: &SimBuildKey) -> f32 {
        self.pin_offsets.get(key).copied().unwrap_or(0.0)
    }

    pub fn target_from_selection(selection: Option<u64>) -> SerialisableTarget {
        use crate::ui::panels::selection_ids::{
            ypt_tag_of, ypt_unpack_effect, ypt_unpack_emitter, ypt_unpack_emitter_lib,
            ypt_unpack_particle, ypt_unpack_particle_lib, YPT_EFFECT, YPT_EMITTER, YPT_EMITTER_LIB,
            YPT_PARTICLE, YPT_PARTICLE_LIB,
        };
        let Some(id) = selection else {
            return SerialisableTarget::None;
        };
        match ypt_tag_of(id) {
            YPT_EFFECT => SerialisableTarget::Effect(ypt_unpack_effect(id)),
            YPT_EMITTER => {
                let (effect, event) = ypt_unpack_emitter(id);
                SerialisableTarget::Emitter { effect, event }
            }
            YPT_PARTICLE => {
                let (effect, event) = ypt_unpack_particle(id);
                SerialisableTarget::Emitter { effect, event }
            }
            YPT_EMITTER_LIB => {
                let _ = ypt_unpack_emitter_lib(id);
                SerialisableTarget::None
            }
            YPT_PARTICLE_LIB => SerialisableTarget::Particle(ypt_unpack_particle_lib(id)),
            _ => SerialisableTarget::None,
        }
    }

    pub fn forget_doc(&mut self, doc_id: DocumentId) {
        if self
            .last_built
            .as_ref()
            .map(|k| k.doc_id == doc_id)
            .unwrap_or(false)
        {
            self.manager.clear_effect();
            self.last_built = None;
        }
        let before = self.pinned_effects.len();
        self.pinned_effects.retain(|p| p.doc_id != doc_id);
        self.pin_offsets.retain(|k, _| k.doc_id != doc_id);
        if self.pinned_effects.len() != before {
            self.pending_rebuild = true;
        }
    }
}

/// A point buffer's columns at their capacity.
///
/// ⚠ Destructured without `..`, so a column the simulation adds does not compile until it counts.
fn points_bytes(points: &PointBuffer) -> usize {
    let PointBuffer {
        init_rotation,
        coln_pos,
        coln_speed,
        coln_normal,
        pause_timer,
        one_over_life,
        emitter_life_ratio,
        playback_rate,
        sort_val,
        flags,
        coln_mtl_id,
        rand_index,
        prev_pos,
        curr_pos,
        spawn_pos,
        matrix_weight,
        curr_vel,
        tex_frame_ratio,
        dimensions,
        curr_rot,
        curr_angle,
        color,
        emissive_intensity,
        tex_frame_a,
        tex_frame_b,
        init_tex_frame,
        alpha_mult,
        is_dead,
        is_broken_trail,
        life_ratio,
        uv_offset,
        evo_values,
        velocity_noise_extra,
        age_atmos_extra,
        colour_tint_extra,
        collision_bounce_extra,
        attractor_orbit_extra,
        prev_in_chain,
        next_in_chain,
        chain_head: _,
        chain_tail: _,
    } = points;
    vec_bytes(init_rotation)
        + vec_bytes(coln_pos)
        + vec_bytes(coln_speed)
        + vec_bytes(coln_normal)
        + vec_bytes(pause_timer)
        + vec_bytes(one_over_life)
        + vec_bytes(emitter_life_ratio)
        + vec_bytes(playback_rate)
        + vec_bytes(sort_val)
        + vec_bytes(flags)
        + vec_bytes(coln_mtl_id)
        + vec_bytes(rand_index)
        + vec_bytes(prev_pos)
        + vec_bytes(curr_pos)
        + vec_bytes(spawn_pos)
        + vec_bytes(matrix_weight)
        + vec_bytes(curr_vel)
        + vec_bytes(tex_frame_ratio)
        + vec_bytes(dimensions)
        + vec_bytes(curr_rot)
        + vec_bytes(curr_angle)
        + vec_bytes(color)
        + vec_bytes(emissive_intensity)
        + vec_bytes(tex_frame_a)
        + vec_bytes(tex_frame_b)
        + vec_bytes(init_tex_frame)
        + vec_bytes(alpha_mult)
        + vec_bytes(is_dead)
        + vec_bytes(is_broken_trail)
        + vec_bytes(life_ratio)
        + vec_bytes(uv_offset)
        + vec_bytes(evo_values)
        + vec_bytes(velocity_noise_extra)
        + vec_bytes(age_atmos_extra)
        + vec_bytes(colour_tint_extra)
        + vec_bytes(collision_bounce_extra)
        + vec_bytes(attractor_orbit_extra)
        + vec_bytes(prev_in_chain)
        + vec_bytes(next_in_chain)
}

/// An emitter: its particles, the rules it was built from and the lists it keeps.
fn emitter_bytes(emitter: &EmitterInst) -> usize {
    let rules = &emitter.rules;
    let spawns: usize = (emitter.pending_spawns.iter())
        .map(|spawn| spawn.effect_rule_name.capacity())
        .sum();
    std::mem::size_of::<EmitterInst>()
        + points_bytes(&emitter.points)
        + rules.rule.heap_bytes()
        + rules.particle.heap_bytes()
        + rules.creation_domain.heap_bytes()
        + rules.velocity_domain.heap_bytes()
        + rules.attractor_domain.heap_bytes()
        + vec_bytes(&rules.behaviours)
        + vec_bytes(&rules.shader_vars)
        + vec_bytes(&emitter.behaviours)
        + vec_bytes(&emitter.pending_spawns)
        + spawns
        + vec_bytes(&emitter.evo_values)
        + vec_bytes(&emitter.pending_liquid_emits)
        + vec_bytes(&emitter.pending_decal_pool_emits)
}

fn effect_bytes(effect: &EffectInst) -> usize {
    let emitters: usize = effect.emitters.iter().map(emitter_bytes).sum();
    std::mem::size_of::<EffectInst>()
        + effect.rule.heap_bytes()
        + vec_bytes(&effect.emitters)
        + emitters
        + vec_bytes(&effect.evo_values)
}

fn states_bytes(states: &Vec<EmitterSnapshot>) -> usize {
    let held: usize = states
        .iter()
        .map(|state| {
            points_bytes(&state.points)
                + vec_bytes(&state.pending_spawns)
                + vec_bytes(&state.evo_values)
        })
        .sum();
    vec_bytes(states) + held
}

fn frame_bytes(frame: &SnapshotFrame) -> usize {
    let spawned: usize = (frame.spawned.iter())
        .map(|child| child.effect_rule_name.capacity() + states_bytes(&child.emitter_states))
        .sum();
    states_bytes(&frame.emitter_states) + vec_bytes(&frame.spawned) + spawned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(doc: u64, idx: usize) -> SimBuildKey {
        SimBuildKey {
            doc_id: doc as DocumentId,
            target: SerialisableTarget::Effect(idx),
        }
    }

    #[test]
    fn toggle_pin_round_trips() {
        let mut sim = SimSession::default();
        assert!(sim.pinned_effects.is_empty());
        assert!(sim.toggle_pin(key(1, 0)));
        assert!(sim.pinned_effects.contains(&key(1, 0)));
        assert!(sim.pending_rebuild);
        sim.pending_rebuild = false;
        assert!(!sim.toggle_pin(key(1, 0)));
        assert!(!sim.pinned_effects.contains(&key(1, 0)));
        assert!(sim.pending_rebuild);
    }

    #[test]
    fn set_pin_offset_round_trips_and_arms_rebuild() {
        let mut sim = SimSession::default();
        let k = key(7, 3);
        sim.toggle_pin(k.clone());
        sim.pending_rebuild = false;

        let prev = sim.set_pin_offset(k.clone(), 0.5);
        assert!((prev - 0.0).abs() < 1e-6);
        assert!((sim.pin_offset(&k) - 0.5).abs() < 1e-6);
        assert!(sim.pending_rebuild);

        sim.pending_rebuild = false;
        let prev = sim.set_pin_offset(k.clone(), 0.5);
        assert!((prev - 0.5).abs() < 1e-6);
        assert!(!sim.pending_rebuild);

        let prev = sim.set_pin_offset(k.clone(), 0.0);
        assert!((prev - 0.5).abs() < 1e-6);
        assert!(!sim.pin_offsets.contains_key(&k));
        assert!(sim.pending_rebuild);
    }

    #[test]
    fn forget_doc_drops_only_matching_pins() {
        let mut sim = SimSession::default();
        sim.toggle_pin(key(1, 0));
        sim.toggle_pin(key(2, 0));
        sim.toggle_pin(key(2, 1));
        sim.set_pin_offset(key(2, 1), 0.75);
        sim.set_pin_offset(key(1, 0), 0.25);
        sim.pending_rebuild = false;
        sim.forget_doc(2);
        assert!(sim.pinned_effects.contains(&key(1, 0)));
        assert!(!sim.pinned_effects.contains(&key(2, 0)));
        assert!(!sim.pinned_effects.contains(&key(2, 1)));
        assert!(sim.pending_rebuild, "forget_doc must arm pending_rebuild");
        assert!((sim.pin_offset(&key(1, 0)) - 0.25).abs() < 1e-6);
        assert!(!sim.pin_offsets.contains_key(&key(2, 1)));
    }

    #[test]
    fn pin_offset_advances_pinned_effect_time_seconds() {
        use or_particle::list;
        let mut asset = or_io::gta5::PtxFxList::default();
        for name in ["primary", "pinned"] {
            list::add_effect(&mut asset, name);
        }
        for effect in list::effect_rules_mut(&mut asset) {
            (effect.duration_min, effect.duration_max) = (4.0, 4.0);
        }
        list::add_particle_rule(&mut asset, "particle");
        let at = |name: &str| {
            list::effect_rules(&asset)
                .position(|e| e.name == name)
                .expect("the list holds the effect")
        };
        let (primary, pinned) = (at("primary"), at("pinned"));

        let mut sim = SimSession::default();
        let primary_doc = 1u64;
        let pin = SimBuildKey {
            doc_id: primary_doc,
            target: SerialisableTarget::Effect(pinned),
        };
        sim.toggle_pin(pin.clone());
        sim.set_pin_offset(pin.clone(), 1.5);
        sim.rebuild(primary_doc, &asset, SerialisableTarget::Effect(primary));
        let pinned = sim.manager.spawned_effects.first().expect("pin built");
        assert!(
            (pinned.time_seconds - 1.5).abs() < 1e-5,
            "pinned effect time_seconds should reflect 1.5s offset, got {}",
            pinned.time_seconds
        );
    }
}
