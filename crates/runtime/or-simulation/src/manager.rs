use crate::behaviours::{
    DecalPoolBatch, DecalPoolDecal, DecalPoolEmit, LiquidBatch, LiquidEmit, LiquidPool, RiverField,
    WaterField, WindField,
};
use crate::collision::BvhCollider;
use crate::effect_inst::EffectInst;
use crate::emitter_inst::EmitterInst;
use crate::point::PointBuffer;
use crate::random::RandomTable;
use crate::random_table::PtxRandomTable;
use crate::time::SimClock;
use glam::{Mat4, Vec3};
use or_io::decl::OwnedOrKeyed;
use or_io::gta5::particle::PtxFxList;
use or_particle::list;
use std::sync::Arc;
#[derive(Debug, Clone)]
pub struct SnapshotFrame {
    pub effect_time_seconds: f32,
    pub emitter_states: Vec<EmitterSnapshot>,
    pub spawned: Vec<SpawnedSnapshot>,
    pub ptx_random_table_cursor: u8,
    pub root_rgba_rand_index: u8,
    pub root_zoom_scalar_rand_index: u8,
    pub root_trail_tex_info_rand_index: u8,
}

#[derive(Debug, Clone)]
pub struct EmitterSnapshot {
    pub points: PointBuffer,
    pub spawn_accumulator: f32,
    pub current_life_ratio: f32,
    pub next_rand_index: u8,
    pub prev_world_pos: glam::Vec3,
    pub has_prev_world_pos: bool,
    pub pending_spawns: Vec<crate::emitter_inst::PendingSpawn>,
    pub evo_values: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct SpawnedSnapshot {
    pub effect_rule_name: String,
    pub matrix: glam::Mat4,
    pub effect_time_seconds: f32,
    pub emitter_states: Vec<EmitterSnapshot>,
    pub rgba_rand_index: u8,
    pub zoom_scalar_rand_index: u8,
    pub trail_tex_info_rand_index: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct WorldEnvironment {
    pub gravity: f32,
    pub wind_velocity: Vec3,
    pub high_quality_evo: f32,
    pub water_z_camera: f32,
    pub air_resistance: f32,
    pub lod_distance_override: Option<f32>,
    pub camera_pos: Vec3,
}

impl Default for WorldEnvironment {
    fn default() -> Self {
        Self {
            gravity: 9.81,
            wind_velocity: Vec3::ZERO,
            high_quality_evo: 0.0,
            water_z_camera: 0.0,
            air_resistance: 1.0,
            lod_distance_override: None,
            camera_pos: Vec3::ZERO,
        }
    }
}

pub struct PtfxManager {
    pub clock: SimClock,
    pub random: RandomTable,
    pub ptx_random_table: PtxRandomTable,
    pub env: WorldEnvironment,
    pub effect: Option<EffectInst>,
    pub spawned_effects: Vec<EffectInst>,
    pub asset_snapshot: Option<Arc<PtxFxList>>,
    pub max_dt: f32,
    pub snapshots: Vec<SnapshotFrame>,
    pub snapshot_interval: f32,
    snapshot_accum: f32,
    pub last_seek_replayed_steps: u32,
    pub collision_bvh: Option<Arc<BvhCollider>>,
    pub river_field: Option<Arc<dyn RiverField + Send + Sync>>,
    pub water_field: Option<Arc<dyn WaterField + Send + Sync>>,
    pub wind_field: Option<Arc<dyn WindField + Send + Sync>>,
    pub wall_time_seconds: f32,
    pub sink_fire: Option<Arc<dyn crate::sinks::FireSink>>,
    pub sink_decal: Option<Arc<dyn crate::sinks::DecalSink>>,
    pub sink_decal_pool: Option<Arc<dyn crate::sinks::DecalPoolSink>>,
    pub sink_liquid: Option<Arc<dyn crate::sinks::LiquidSink>>,
    pub sink_fog_volume: Option<Arc<dyn crate::sinks::FogVolumeSink>>,
    pub sink_light: Option<Arc<dyn crate::sinks::LightSink>>,
    pub sink_audio: Option<Arc<dyn crate::sinks::AudioSink>>,
    pub liquid_batches: LiquidBatch,
    pub decal_pool_batches: DecalPoolBatch,
    pub history: crate::effect_history::EffectHistory,
    pub history_check_radius: f32,
    /// The on-cull spawner each particle rule has, by its name, given every effect built.
    pub zcull_spawners: crate::zcull_spawner::ZCullSpawners,
}

const MAX_SPAWNED_EFFECTS: usize = 64;

/// `asset` without what the simulation never reads, to share between managers: its
/// textures and models, and an event's own copy of a rule the list holds by that name.
pub fn rules_of(mut asset: PtxFxList) -> Arc<PtxFxList> {
    (asset.texture_dict, asset.model_dict) = (None, None);
    for rule in list::particle_rules_mut(&mut asset) {
        crate::emitter_inst::drop_meshes(rule);
    }
    let emitters: std::collections::HashSet<String> = list::emitter_rules(&asset)
        .map(|r| r.name.clone())
        .collect();
    let particles: std::collections::HashSet<String> = list::particle_rules(&asset)
        .map(|r| r.name.clone())
        .collect();
    for effect in list::effect_rules_mut(&mut asset) {
        for event in list::events_mut(effect) {
            if emitters.contains(&event.emitter_rule_name) {
                event.emitter_rule = OwnedOrKeyed::Own(None);
            }
            if particles.contains(&event.particle_rule_name) {
                event.particle_rule = OwnedOrKeyed::Own(None);
            }
            if let OwnedOrKeyed::Own(Some(rule)) = &mut event.particle_rule {
                crate::emitter_inst::drop_meshes(rule);
            }
        }
    }
    Arc::new(asset)
}
#[derive(Clone, Copy)]
pub struct WorldHooks<'a> {
    pub gravity: f32,
    pub wind_velocity: Vec3,
    pub collision_bvh: Option<&'a crate::collision::BvhCollider>,
    pub river_field: Option<&'a (dyn RiverField + Send + Sync)>,
    pub wind_field: Option<&'a (dyn crate::behaviours::WindField + Send + Sync)>,
    pub wall_time_seconds: f32,
    pub water_field: Option<&'a (dyn WaterField + Send + Sync)>,
    pub water_z_camera: f32,
    pub air_resistance: f32,
    pub lod_distance_override: Option<f32>,
    pub sinks: crate::sinks::PtfxSinks<'a>,
}

impl Default for PtfxManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PtfxManager {
    pub fn new() -> Self {
        Self {
            clock: SimClock::default(),
            random: RandomTable::new(),
            ptx_random_table: PtxRandomTable::default(),
            env: WorldEnvironment::default(),
            effect: None,
            spawned_effects: Vec::new(),
            asset_snapshot: None,
            max_dt: 0.066,
            snapshots: Vec::new(),
            snapshot_interval: 0.25,
            snapshot_accum: 0.0,
            last_seek_replayed_steps: 0,
            collision_bvh: None,
            river_field: None,
            water_field: None,
            wind_field: None,
            wall_time_seconds: 0.0,
            sink_fire: None,
            sink_decal: None,
            sink_decal_pool: None,
            sink_liquid: None,
            sink_fog_volume: None,
            sink_light: None,
            sink_audio: None,
            liquid_batches: LiquidBatch::default(),
            decal_pool_batches: DecalPoolBatch::default(),
            history: crate::effect_history::EffectHistory::new(64),
            history_check_radius: 0.0,
            zcull_spawners: Default::default(),
        }
    }

    pub fn set_collision_bvh(&mut self, bvh: Option<Arc<BvhCollider>>) {
        self.collision_bvh = bvh;
    }

    pub fn set_river_field(&mut self, field: Option<Arc<dyn RiverField + Send + Sync>>) {
        self.river_field = field;
    }

    pub fn set_water_field(&mut self, field: Option<Arc<dyn WaterField + Send + Sync>>) {
        self.water_field = field;
    }

    pub fn set_asset_snapshot(&mut self, asset: PtxFxList) {
        self.asset_snapshot = Some(rules_of(asset));
    }

    pub fn set_effect(&mut self, mut effect: EffectInst) {
        self.clock.set_duration(effect.duration);
        effect.matrix = Mat4::IDENTITY;
        effect.high_quality_evo = self.env.high_quality_evo;
        crate::zcull_spawner::give(&mut effect, &self.zcull_spawners);
        let r = &self.random;
        let seed = or_particle::list::code(&effect.rule.name).wrapping_add(0xC0FF_EE01);
        let bx = r.value(seed) * 2.0 - 1.0;
        let by = r.value(seed.wrapping_add(1)) * 2.0 - 1.0;
        let bz = r.value(seed.wrapping_add(2)) * 2.0 - 1.0;
        let off = glam::Vec3::new(
            effect.rule.rand_offset_pos.x * bx,
            effect.rule.rand_offset_pos.y * by,
            effect.rule.rand_offset_pos.z * bz,
        );
        effect.matrix.w_axis += glam::Vec4::new(off.x, off.y, off.z, 0.0);
        if effect.rgba_rand_index == 0 {
            effect.rgba_rand_index = self.ptx_random_table.next_index();
        }
        if effect.zoom_scalar_rand_index == 0 {
            effect.zoom_scalar_rand_index = self.ptx_random_table.next_index();
        }
        for em in effect.emitters.iter_mut() {
            em.next_rand_index = self.ptx_random_table.next_index();
        }
        let pre = effect.rule.pre_update_time.max(0.0);
        let interval = effect.rule.pre_update_time_interval.max(0.016);
        if pre > 0.0 {
            let mut t = 0.0_f32;
            while t < pre {
                let step = (pre - t).min(interval);
                effect.update(
                    step,
                    &self.random,
                    &self.ptx_random_table,
                    self.env.gravity,
                    self.env.wind_velocity,
                );
                t += step;
            }
        }
        self.effect = Some(effect);
        self.clock.seek(0.0);
        if let Some(audio) = self.sink_audio.as_deref() {
            if let Some(eff) = self.effect.as_ref() {
                let pos = eff.matrix.w_axis.truncate();
                audio.on_event(crate::sinks::AudioEvent::Start {
                    name: &eff.rule.name,
                    pos,
                });
            }
        }
    }

    pub fn clear_effect(&mut self) {
        if let Some(audio) = self.sink_audio.as_deref() {
            if let Some(eff) = self.effect.as_ref() {
                let pos = eff.matrix.w_axis.truncate();
                audio.on_event(crate::sinks::AudioEvent::Stop {
                    name: &eff.rule.name,
                    pos,
                });
            }
        }
        self.effect = None;
        self.spawned_effects.clear();
        self.snapshots.clear();
        self.snapshot_accum = 0.0;
        self.liquid_batches.pools.clear();
        self.decal_pool_batches.decals.clear();
    }

    pub fn capture_snapshot(&mut self) {
        let Some(eff) = self.effect.as_ref() else {
            return;
        };
        let snapshot_emitters = |emitters: &[EmitterInst]| -> Vec<EmitterSnapshot> {
            emitters
                .iter()
                .map(|em| EmitterSnapshot {
                    points: em.points.clone(),
                    spawn_accumulator: em.spawn_accumulator,
                    current_life_ratio: em.current_life_ratio,
                    next_rand_index: em.next_rand_index,
                    prev_world_pos: em.prev_world_pos,
                    has_prev_world_pos: em.has_prev_world_pos,
                    pending_spawns: em.pending_spawns.clone(),
                    evo_values: em.evo_values.clone(),
                })
                .collect()
        };
        let frame = SnapshotFrame {
            effect_time_seconds: eff.time_seconds,
            emitter_states: snapshot_emitters(&eff.emitters),
            spawned: self
                .spawned_effects
                .iter()
                .map(|child| SpawnedSnapshot {
                    effect_rule_name: child.rule.name.clone(),
                    matrix: child.matrix,
                    effect_time_seconds: child.time_seconds,
                    emitter_states: snapshot_emitters(&child.emitters),
                    rgba_rand_index: child.rgba_rand_index,
                    zoom_scalar_rand_index: child.zoom_scalar_rand_index,
                    trail_tex_info_rand_index: child.trail_tex_info_rand_index,
                })
                .collect(),
            ptx_random_table_cursor: self.ptx_random_table.cursor(),
            root_rgba_rand_index: eff.rgba_rand_index,
            root_zoom_scalar_rand_index: eff.zoom_scalar_rand_index,
            root_trail_tex_info_rand_index: eff.trail_tex_info_rand_index,
        };
        self.snapshots.push(frame);
        const MAX_SNAPSHOTS: usize = 80;
        if self.snapshots.len() > MAX_SNAPSHOTS {
            self.snapshots.remove(0);
        }
    }

    pub fn restore_snapshot(&mut self, frame: &SnapshotFrame) -> bool {
        let Some(eff) = self.effect.as_mut() else {
            return false;
        };
        if eff.emitters.len() != frame.emitter_states.len() {
            return false;
        }
        self.ptx_random_table
            .set_cursor(frame.ptx_random_table_cursor);
        eff.time_seconds = frame.effect_time_seconds;
        eff.rgba_rand_index = frame.root_rgba_rand_index;
        eff.zoom_scalar_rand_index = frame.root_zoom_scalar_rand_index;
        eff.trail_tex_info_rand_index = frame.root_trail_tex_info_rand_index;
        for (em, snap) in eff.emitters.iter_mut().zip(&frame.emitter_states) {
            em.points = snap.points.clone();
            em.spawn_accumulator = snap.spawn_accumulator;
            em.current_life_ratio = snap.current_life_ratio;
            em.next_rand_index = snap.next_rand_index;
            em.prev_world_pos = snap.prev_world_pos;
            em.has_prev_world_pos = snap.has_prev_world_pos;
            em.pending_spawns = snap.pending_spawns.clone();
            if snap.evo_values.len() == em.evo_values.len() {
                em.evo_values = snap.evo_values.clone();
            }
        }
        self.spawned_effects.clear();
        if let Some(asset) = self.asset_snapshot.as_ref() {
            for child_snap in &frame.spawned {
                let Some(eff_idx) = or_particle::list::effect_rules(asset)
                    .position(|e| e.name == child_snap.effect_rule_name)
                else {
                    continue;
                };
                let Some(mut inst) =
                    crate::build::build(asset, crate::build::PreviewTarget::Effect(eff_idx))
                else {
                    continue;
                };
                crate::zcull_spawner::give(&mut inst, &self.zcull_spawners);
                inst.matrix = child_snap.matrix;
                inst.time_seconds = child_snap.effect_time_seconds;
                inst.rgba_rand_index = child_snap.rgba_rand_index;
                inst.zoom_scalar_rand_index = child_snap.zoom_scalar_rand_index;
                inst.trail_tex_info_rand_index = child_snap.trail_tex_info_rand_index;
                if inst.emitters.len() == child_snap.emitter_states.len() {
                    for (em, snap) in inst.emitters.iter_mut().zip(&child_snap.emitter_states) {
                        em.points = snap.points.clone();
                        em.spawn_accumulator = snap.spawn_accumulator;
                        em.current_life_ratio = snap.current_life_ratio;
                        em.next_rand_index = snap.next_rand_index;
                        em.prev_world_pos = snap.prev_world_pos;
                        em.has_prev_world_pos = snap.has_prev_world_pos;
                        em.pending_spawns = snap.pending_spawns.clone();
                        if snap.evo_values.len() == em.evo_values.len() {
                            em.evo_values = snap.evo_values.clone();
                        }
                    }
                }
                self.spawned_effects.push(inst);
            }
        }
        true
    }

    pub fn live_count(&self) -> usize {
        let root = self.effect.as_ref().map(|e| e.live_count()).unwrap_or(0);
        let spawned: usize = self.spawned_effects.iter().map(|e| e.live_count()).sum();
        root + spawned
    }

    pub fn current_time(&self) -> f32 {
        self.clock.time
    }

    pub fn duration(&self) -> f32 {
        self.effect.as_ref().map(|e| e.duration).unwrap_or(0.0)
    }

    pub fn tick(&mut self, real_dt: f32) {
        if self.effect.is_none() {
            return;
        }

        if let Some(t) = self.clock.take_seek() {
            let mut steps: u32 = 0;
            self.snapshots.retain(|s| s.effect_time_seconds <= t);
            let best = self
                .snapshots
                .iter()
                .rev()
                .find(|s| s.effect_time_seconds <= t)
                .cloned();
            if let Some(snap) = best {
                let snap_t = snap.effect_time_seconds;
                self.restore_snapshot(&snap);
                let hooks = WorldHooks {
                    gravity: self.env.gravity,
                    wind_velocity: self.env.wind_velocity,
                    collision_bvh: self.collision_bvh.as_deref(),
                    river_field: self.river_field.as_deref(),
                    water_field: self.water_field.as_deref(),
                    wind_field: self.wind_field.as_deref(),
                    wall_time_seconds: self.wall_time_seconds,
                    water_z_camera: self.env.water_z_camera,
                    air_resistance: self.env.air_resistance,
                    lod_distance_override: self.env.lod_distance_override,
                    sinks: crate::sinks::PtfxSinks {
                        fire: self.sink_fire.as_deref(),
                        decal: self.sink_decal.as_deref(),
                        decal_pool: self.sink_decal_pool.as_deref(),
                        liquid: self.sink_liquid.as_deref(),
                        fog_volume: self.sink_fog_volume.as_deref(),
                        light: self.sink_light.as_deref(),
                        audio: self.sink_audio.as_deref(),
                    },
                };
                let max_dt = self.max_dt;
                let random = &self.random;
                let ptx_table = &self.ptx_random_table;
                let effect = self.effect.as_mut().unwrap();
                let mut accum = snap_t;
                while accum < t {
                    let step = (t - accum).min(max_dt).max(1e-3);
                    hooks.sinks.begin();
                    effect.update_with_hooks(step, random, ptx_table, &hooks);
                    accum += step;
                    steps += 1;
                }
            } else {
                self.snapshots.clear();
                let hooks = WorldHooks {
                    gravity: self.env.gravity,
                    wind_velocity: self.env.wind_velocity,
                    collision_bvh: self.collision_bvh.as_deref(),
                    river_field: self.river_field.as_deref(),
                    water_field: self.water_field.as_deref(),
                    wind_field: self.wind_field.as_deref(),
                    wall_time_seconds: self.wall_time_seconds,
                    water_z_camera: self.env.water_z_camera,
                    air_resistance: self.env.air_resistance,
                    lod_distance_override: self.env.lod_distance_override,
                    sinks: crate::sinks::PtfxSinks {
                        fire: self.sink_fire.as_deref(),
                        decal: self.sink_decal.as_deref(),
                        decal_pool: self.sink_decal_pool.as_deref(),
                        liquid: self.sink_liquid.as_deref(),
                        fog_volume: self.sink_fog_volume.as_deref(),
                        light: self.sink_light.as_deref(),
                        audio: self.sink_audio.as_deref(),
                    },
                };
                let max_dt = self.max_dt;
                let random = &self.random;
                let ptx_table = &self.ptx_random_table;
                let effect = self.effect.as_mut().unwrap();
                effect.reset();
                let mut accum = 0.0;
                while accum < t {
                    let step = (t - accum).min(max_dt).max(1e-3);
                    hooks.sinks.begin();
                    effect.update_with_hooks(step, random, ptx_table, &hooks);
                    accum += step;
                    steps += 1;
                }
            }
            self.last_seek_replayed_steps = steps;
            self.snapshot_accum = 0.0;
        }
        let hooks = WorldHooks {
            gravity: self.env.gravity,
            wind_velocity: self.env.wind_velocity,
            collision_bvh: self.collision_bvh.as_deref(),
            river_field: self.river_field.as_deref(),
            water_field: self.water_field.as_deref(),
            wind_field: self.wind_field.as_deref(),
            wall_time_seconds: self.wall_time_seconds,
            water_z_camera: self.env.water_z_camera,
            air_resistance: self.env.air_resistance,
            lod_distance_override: self.env.lod_distance_override,
            sinks: crate::sinks::PtfxSinks {
                fire: self.sink_fire.as_deref(),
                decal: self.sink_decal.as_deref(),
                decal_pool: self.sink_decal_pool.as_deref(),
                liquid: self.sink_liquid.as_deref(),
                fog_volume: self.sink_fog_volume.as_deref(),
                light: self.sink_light.as_deref(),
                audio: self.sink_audio.as_deref(),
            },
        };
        let effect = self.effect.as_mut().unwrap();

        let sim_dt = self.clock.tick(real_dt).min(self.max_dt);
        if sim_dt <= 0.0 {
            return;
        }
        hooks.sinks.begin();
        effect.update_with_hooks(sim_dt, &self.random, &self.ptx_random_table, &hooks);
        self.snapshot_accum += sim_dt;
        if self.snapshot_accum >= self.snapshot_interval {
            self.snapshot_accum = 0.0;
            self.capture_snapshot();
        }

        let mut requests: Vec<crate::emitter_inst::PendingSpawn> = Vec::new();
        if let Some(eff) = self.effect.as_mut() {
            for em in eff.emitters.iter_mut() {
                requests.append(&mut em.pending_spawns);
            }
        }
        for child in self.spawned_effects.iter_mut() {
            for em in child.emitters.iter_mut() {
                requests.append(&mut em.pending_spawns);
            }
        }
        self.history.tick(sim_dt);
        for req in requests {
            if self.spawned_effects.len() >= MAX_SPAWNED_EFFECTS {
                tracing::warn!("EffectSpawner: hit MAX_SPAWNED_EFFECTS cap, ignoring further");
                break;
            }
            if let Some(asset) = self.asset_snapshot.as_ref() {
                if let Some(eff_idx) = or_particle::list::effect_rules(asset)
                    .position(|e| e.name == req.effect_rule_name)
                {
                    let rule_hash =
                        or_particle::rage::hash::string_hash(req.effect_rule_name.as_bytes());
                    let world_pos = req.matrix.w_axis.truncate();
                    if self.history_check_radius > 0.0
                        && self
                            .history
                            .check(rule_hash, 0, world_pos, self.history_check_radius)
                    {
                        continue;
                    }
                    if let Some(mut inst) =
                        crate::build::build(asset, crate::build::PreviewTarget::Effect(eff_idx))
                    {
                        crate::zcull_spawner::give(&mut inst, &self.zcull_spawners);
                        inst.matrix = req.matrix;
                        inst.apply_spawned_scalars(req.scalars);
                        if let Some(life_left) = req.parent_remaining_life {
                            inst.apply_inherited_life(life_left);
                        }
                        inst.parent_attachment = req.parent_rand_index;
                        self.history.add(rule_hash, 0, world_pos, inst.duration);
                        self.spawned_effects.push(inst);
                    }
                }
            }
        }
        let mut by_rand: std::collections::HashMap<u8, glam::Vec3> =
            std::collections::HashMap::new();
        if let Some(root) = self.effect.as_ref() {
            for em in root.emitters.iter() {
                for i in 0..em.points.len() {
                    by_rand.insert(em.points.rand_index[i], em.points.curr_pos[i]);
                }
            }
        }
        for ef in self.spawned_effects.iter() {
            for em in ef.emitters.iter() {
                for i in 0..em.points.len() {
                    by_rand.insert(em.points.rand_index[i], em.points.curr_pos[i]);
                }
            }
        }
        for child in self.spawned_effects.iter_mut() {
            if let Some(rand_idx) = child.parent_attachment {
                if let Some(pos) = by_rand.get(&rand_idx).copied() {
                    let (s, r, _) = child.matrix.to_scale_rotation_translation();
                    child.matrix = glam::Mat4::from_scale_rotation_translation(s, r, pos);
                }
            }
        }
        let child_hooks = WorldHooks {
            gravity: self.env.gravity,
            wind_velocity: self.env.wind_velocity,
            collision_bvh: self.collision_bvh.as_deref(),
            river_field: self.river_field.as_deref(),
            water_field: self.water_field.as_deref(),
            wind_field: self.wind_field.as_deref(),
            wall_time_seconds: self.wall_time_seconds,
            water_z_camera: self.env.water_z_camera,
            air_resistance: self.env.air_resistance,
            lod_distance_override: self.env.lod_distance_override,
            sinks: crate::sinks::PtfxSinks {
                fire: self.sink_fire.as_deref(),
                decal: self.sink_decal.as_deref(),
                decal_pool: self.sink_decal_pool.as_deref(),
                liquid: self.sink_liquid.as_deref(),
                fog_volume: self.sink_fog_volume.as_deref(),
                light: self.sink_light.as_deref(),
                audio: self.sink_audio.as_deref(),
            },
        };
        for child in self.spawned_effects.iter_mut() {
            child.update_with_hooks(sim_dt, &self.random, &self.ptx_random_table, &child_hooks);
        }

        self.spawned_effects.retain(|e| !e.is_finished);

        let mut liquid_inbox: Vec<LiquidEmit> = Vec::new();
        let mut decal_inbox: Vec<DecalPoolEmit> = Vec::new();
        if let Some(eff) = self.effect.as_mut() {
            for em in eff.emitters.iter_mut() {
                liquid_inbox.append(&mut em.pending_liquid_emits);
                decal_inbox.append(&mut em.pending_decal_pool_emits);
            }
        }
        for child in self.spawned_effects.iter_mut() {
            for em in child.emitters.iter_mut() {
                liquid_inbox.append(&mut em.pending_liquid_emits);
                decal_inbox.append(&mut em.pending_decal_pool_emits);
            }
        }
        for pool in self.liquid_batches.pools.iter_mut() {
            pool.radius = (pool.radius + pool.growth_rate * sim_dt).min(pool.end_radius);
        }
        for decal in self.decal_pool_batches.decals.iter_mut() {
            decal.radius = (decal.radius + decal.growth_rate * sim_dt).min(decal.end_radius);
        }
        for emit in liquid_inbox {
            absorb_liquid_emit(&mut self.liquid_batches.pools, emit);
        }
        for emit in decal_inbox {
            absorb_decal_pool_emit(&mut self.decal_pool_batches.decals, emit);
        }
    }

    pub fn liquid_batch(&self) -> &LiquidBatch {
        &self.liquid_batches
    }

    pub fn decal_pool_batch(&self) -> &DecalPoolBatch {
        &self.decal_pool_batches
    }

    pub fn emitters(&self) -> impl Iterator<Item = &EmitterInst> {
        self.effect
            .iter()
            .flat_map(|e| e.emitters.iter())
            .chain(self.spawned_effects.iter().flat_map(|e| e.emitters.iter()))
    }

    pub fn emitters_sorted_by_camera(&self, camera_pos: glam::Vec3) -> Vec<&EmitterInst> {
        let mut effects: Vec<(f32, &EffectInst)> = self
            .effect
            .iter()
            .chain(self.spawned_effects.iter())
            .filter_map(|e| {
                if e.emitters.iter().all(|em| em.points.is_empty()) {
                    return None;
                }
                let pos = e.current_pos;
                let d = (pos - camera_pos).length_squared();
                Some((d, e))
            })
            .collect();
        effects.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        effects
            .into_iter()
            .flat_map(|(_, e)| e.emitters.iter())
            .collect()
    }
}

fn absorb_liquid_emit(pools: &mut Vec<LiquidPool>, emit: LiquidEmit) {
    for pool in pools.iter_mut() {
        if pool.liquid_type != emit.liquid_type {
            continue;
        }
        let dist = (pool.pos - emit.pos).length();
        if dist <= pool.radius + emit.pool_start_size {
            pool.end_radius = pool.end_radius.max(emit.pool_end_size);
            pool.growth_rate = pool.growth_rate.max(emit.pool_growth_rate);
            return;
        }
    }
    pools.push(LiquidPool {
        pos: emit.pos,
        normal: emit.normal,
        radius: emit.pool_start_size.max(0.0),
        end_radius: emit.pool_end_size.max(emit.pool_start_size),
        growth_rate: emit.pool_growth_rate.max(0.0),
        color: emit.color,
        mtl_id: emit.mtl_id,
        liquid_type: emit.liquid_type,
    });
}

fn absorb_decal_pool_emit(decals: &mut Vec<DecalPoolDecal>, emit: DecalPoolEmit) {
    for decal in decals.iter_mut() {
        if decal.liquid_type != emit.liquid_type || decal.decal_id != emit.decal_id {
            continue;
        }
        let dist = (decal.pos - emit.pos).length();
        if dist <= decal.radius + emit.start_size {
            decal.end_radius = decal.end_radius.max(emit.end_size);
            decal.growth_rate = decal.growth_rate.max(emit.growth_rate);
            return;
        }
    }
    decals.push(DecalPoolDecal {
        pos: emit.pos,
        normal: emit.normal,
        radius: emit.start_size.max(0.0),
        end_radius: emit.end_size.max(emit.start_size),
        growth_rate: emit.growth_rate.max(0.0),
        color: emit.color,
        mtl_id: emit.mtl_id,
        liquid_type: emit.liquid_type,
        decal_id: emit.decal_id,
    });
}
