use crate::behaviour::{AttractorGeometry, BehaviourImpl, BehaviourParams, DrawKind};
use crate::behaviours;
use crate::domain_sample::{point_domain_at, three_uniforms, DomainInst};
use crate::kfp::query_kfp_with_evo;
use crate::point::{PointBuffer, PointFlags};
use crate::random::RandomTable;
use glam::{Mat4, Vec3, Vec4};
use or_io::decl::Gen8;
use or_io::gta5::particle::{
    PtxDomain, PtxEffectSpawner, PtxEmitterRule, PtxEventEmitter, PtxParticleRule,
    PtxSpawnedEffectScalars, ShaderVar,
};
use or_particle::behaviour::Behaviour;
use or_particle::list;
#[derive(Debug, Clone, Copy)]
pub struct SpawnedScalars {
    pub duration_scalar: f32,
    pub playback_rate_scalar: f32,
    pub colour_tint: glam::Vec4,
    pub zoom_scalar: f32,
    pub inherits_point_life: bool,
    pub tracks_point_pos: bool,
}

impl SpawnedScalars {
    pub const IDENTITY: Self = Self {
        duration_scalar: 1.0,
        playback_rate_scalar: 1.0,
        colour_tint: glam::Vec4::ONE,
        zoom_scalar: 1.0,
        inherits_point_life: false,
        tracks_point_pos: false,
    };

    pub fn sample_from_spawner(
        random: &RandomTable,
        rand_index: u8,
        spawner: &PtxEffectSpawner<Gen8>,
    ) -> Self {
        let bd = random.value(rand_index as u32 + 0x10);
        let bp = random.value(rand_index as u32 + 0x20);
        let bz = random.value(rand_index as u32 + 0x30);
        let bc = random.value(rand_index as u32 + 0x40);
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let min = &spawner.spawned_effect_scalars_min;
        let max = &spawner.spawned_effect_scalars_max;
        let duration_scalar = lerp(min.duration_scalar, max.duration_scalar, bd).max(0.001);
        let playback_rate_scalar =
            lerp(min.playback_rate_scalar, max.playback_rate_scalar, bp).max(0.001);
        let zoom_scalar = lerp(min.zoom_scalar, max.zoom_scalar, bz).max(0.001);
        let colour_tint = lerp_packed_rgba(min.colour_tint_scalar, max.colour_tint_scalar, bc);
        let colour_tint = if colour_tint == glam::Vec4::ZERO {
            glam::Vec4::ONE
        } else {
            colour_tint
        };
        Self {
            duration_scalar,
            playback_rate_scalar,
            colour_tint,
            zoom_scalar,
            inherits_point_life: spawner.inherits_point_life.get(),
            tracks_point_pos: spawner.tracks_point_pos.get(),
        }
    }

    pub fn sample(
        random: &RandomTable,
        rand_index: u8,
        min: &PtxSpawnedEffectScalars,
        max: &PtxSpawnedEffectScalars,
    ) -> Self {
        let bd = random.value(rand_index as u32 + 0x10);
        let bp = random.value(rand_index as u32 + 0x20);
        let bz = random.value(rand_index as u32 + 0x30);
        let bc = random.value(rand_index as u32 + 0x40);
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let duration_scalar = lerp(min.duration_scalar, max.duration_scalar, bd).max(0.001);
        let playback_rate_scalar =
            lerp(min.playback_rate_scalar, max.playback_rate_scalar, bp).max(0.001);
        let zoom_scalar = lerp(min.zoom_scalar, max.zoom_scalar, bz).max(0.001);
        let colour_tint = lerp_packed_rgba(min.colour_tint_scalar, max.colour_tint_scalar, bc);
        let colour_tint = if colour_tint == glam::Vec4::ZERO {
            glam::Vec4::ONE
        } else {
            colour_tint
        };
        Self {
            duration_scalar,
            playback_rate_scalar,
            colour_tint,
            zoom_scalar,
            inherits_point_life: false,
            tracks_point_pos: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmitterRules {
    pub rule: PtxEmitterRule,
    /// ⚠ Its drawables name their models and hold no mesh.
    pub particle: PtxParticleRule<Gen8>,
    /// `particle`'s behaviours, each once, as the simulation runs them.
    pub behaviours: Vec<Behaviour>,
    pub shader_vars: Vec<ShaderVar>,
    pub creation_domain: PtxDomain,
    pub velocity_domain: PtxDomain,
    pub attractor_domain: Option<PtxDomain>,
    pub event_start_ratio: f32,
    pub event_end_ratio: f32,
    pub event_playback_scalar: f32,
    pub event_zoom_scalar: f32,
    pub event_colour_tint: glam::Vec4,
    /// A spawner GTA V's rule has no place for, started where a `ptxu_ZCull` culls a point.
    pub effect_spawner_on_zcull: Option<PtxEffectSpawner<Gen8>>,
}

impl EmitterRules {
    pub fn from_event(
        rule: PtxEmitterRule,
        mut particle: PtxParticleRule<Gen8>,
        event: &PtxEventEmitter<Gen8>,
    ) -> Self {
        let domain = |obj: &or_io::gta5::particle::PtxDomainObj| obj.domain.as_deref().cloned();
        let creation_domain = domain(&rule.creation_domain_obj).unwrap_or_default();
        let velocity_domain = domain(&rule.target_domain_obj).unwrap_or_default();
        let attractor_domain = domain(&rule.attractor_domain_obj);
        drop_meshes(&mut particle);
        let behaviours = list::behaviours(&particle);
        let shader_vars = list::shader_vars(&particle.shader_inst);
        let index = event.base.index as u32;
        let table_seed = list::code(&rule.name)
            .wrapping_mul(0x9E37_79B9)
            .wrapping_add(index.wrapping_mul(2654435761));
        let table = crate::random::RandomTable::with_seed(table_seed);
        let seed = index.wrapping_mul(31).wrapping_add(7);
        let bias_pb = table.value(seed);
        let bias_zoom = table.value(seed + 17);
        let bias_col = table.value(seed + 41);
        let event_playback_scalar = (event.playback_rate_scalar_min
            + (event.playback_rate_scalar_max - event.playback_rate_scalar_min) * bias_pb)
            .max(0.001);
        let event_zoom_scalar = (event.zoom_scalar_min
            + (event.zoom_scalar_max - event.zoom_scalar_min) * bias_zoom)
            .max(0.001);
        let tint = lerp_packed_rgba(event.colour_tint_min, event.colour_tint_max, bias_col);
        Self {
            rule,
            particle,
            behaviours,
            shader_vars,
            creation_domain,
            velocity_domain,
            attractor_domain,
            event_start_ratio: event.base.start_ratio.clamp(0.0, 1.0),
            event_end_ratio: event.base.end_ratio.clamp(0.0, 1.0),
            event_playback_scalar,
            event_zoom_scalar,
            event_colour_tint: tint,
            effect_spawner_on_zcull: None,
        }
    }
}

/// `rule`'s drawables without their meshes, which the simulation never reads.
pub(crate) fn drop_meshes(rule: &mut PtxParticleRule<Gen8>) {
    for d in &mut rule.drawables {
        d.drawable = or_io::decl::OwnedOrKeyed::Own(None);
    }
}

#[inline]
fn lerp_mat4(a: Mat4, b: Mat4, t: f32) -> Mat4 {
    let a = a.to_cols_array();
    let b = b.to_cols_array();
    let mut out = [0.0_f32; 16];
    for i in 0..16 {
        out[i] = a[i] + (b[i] - a[i]) * t;
    }
    Mat4::from_cols_array(&out)
}

#[inline]
fn mats_close(a: Mat4, b: Mat4) -> bool {
    let a = a.to_cols_array();
    let b = b.to_cols_array();
    let mut max_d = 0.0_f32;
    for i in 0..16 {
        max_d = max_d.max((a[i] - b[i]).abs());
    }
    max_d <= 1e-6
}

fn lerp_packed_rgba(a: u32, b: u32, t: f32) -> glam::Vec4 {
    let unpack = |c: u32| -> glam::Vec4 {
        glam::Vec4::new(
            ((c >> 24) & 0xFF) as f32 / 255.0,
            ((c >> 16) & 0xFF) as f32 / 255.0,
            ((c >> 8) & 0xFF) as f32 / 255.0,
            (c & 0xFF) as f32 / 255.0,
        )
    };
    let av = unpack(a);
    let bv = unpack(b);
    av.lerp(bv, t)
}

#[derive(Debug, Clone)]
pub struct PendingSpawn {
    pub effect_rule_name: String,
    pub matrix: glam::Mat4,
    pub scalars: SpawnedScalars,
    pub parent_remaining_life: Option<f32>,
    pub parent_rand_index: Option<u8>,
}

pub struct EmitterInst {
    pub rules: EmitterRules,
    pub points: PointBuffer,
    pub draw_kind: DrawKind,
    pub behaviours: Vec<Box<dyn BehaviourImpl>>,
    pub creation_domain: DomainInst,
    pub velocity_domain: DomainInst,
    pub attractor_domain: Option<DomainInst>,
    pub current_life_ratio: f32,
    pub prev_life_ratio: f32,
    pub spawn_accumulator: f32,
    pub max_particles_soft_cap: usize,
    pub next_rand_index: u8,
    pub pending_spawns: Vec<PendingSpawn>,
    pub evolution_list: Option<or_particle::rules::EvolutionList>,
    pub evo_values: Vec<f32>,
    pub prev_world_pos: Vec3,
    pub has_prev_world_pos: bool,
    pub has_fired_one_shot: bool,
    pub pending_liquid_emits: Vec<crate::behaviours::LiquidEmit>,
    pub pending_decal_pool_emits: Vec<crate::behaviours::DecalPoolEmit>,
}

impl EmitterInst {
    pub fn new(rules: EmitterRules) -> Self {
        let draw_kind = behaviours::detect_draw_kind(
            &rules.behaviours,
            DrawKind::from_raw(rules.particle.draw_type),
        );
        let mut rules = rules;
        crate::kfp::apply_bias_links_to_behaviours(
            &rules.particle.bias_links,
            &mut rules.behaviours,
        );
        crate::kfp::apply_bias_links_to_emitter_rule(&rules.particle.bias_links, &mut rules.rule);
        let bs = behaviours::build_dispatch(&rules.behaviours, &rules.particle);
        Self {
            rules,
            points: PointBuffer::default(),
            draw_kind,
            behaviours: bs,
            creation_domain: DomainInst::default(),
            velocity_domain: DomainInst::default(),
            attractor_domain: None,
            current_life_ratio: 0.0,
            prev_life_ratio: -1.0,
            spawn_accumulator: 0.0,
            max_particles_soft_cap: 4096,
            next_rand_index: 0,
            pending_spawns: Vec::new(),
            evolution_list: None,
            evo_values: vec![0.0_f32; crate::kfp::MAX_EVOLUTIONS],
            prev_world_pos: Vec3::ZERO,
            has_prev_world_pos: false,
            has_fired_one_shot: false,
            pending_liquid_emits: Vec::new(),
            pending_decal_pool_emits: Vec::new(),
        }
    }

    pub fn notify_looped(&mut self) {
        self.has_fired_one_shot = false;
    }

    pub fn set_evolution_list(&mut self, list: Option<or_particle::rules::EvolutionList>) {
        for v in self.evo_values.iter_mut() {
            *v = 0.0;
        }
        if let Some(list_ref) = list.as_ref() {
            for (i, ev) in list_ref
                .evolutions
                .iter()
                .take(crate::kfp::MAX_EVOLUTIONS)
                .enumerate()
            {
                if ev.is_overridden {
                    self.evo_values[i] = ev.override_value;
                }
            }
        }
        self.evolution_list = list;
    }

    pub fn live_count(&self) -> usize {
        self.points.len()
    }

    pub fn reset(&mut self) {
        self.points.clear();
        self.current_life_ratio = 0.0;
        self.prev_life_ratio = -1.0;
        self.spawn_accumulator = 0.0;
        self.next_rand_index = 0;
        self.has_fired_one_shot = false;
        self.has_prev_world_pos = false;
    }

    pub fn update(&mut self, dt: f32, params_template: BehaviourParamsTemplate<'_>) {
        let in_window = params_template.effect_time_ratio >= self.rules.event_start_ratio
            && params_template.effect_time_ratio <= self.rules.event_end_ratio;

        self.refresh_domains(
            params_template.effect_matrix,
            params_template.effect_time_ratio,
            params_template.effect_zoom,
        );

        if !in_window {
            return;
        }

        let curr_world_pos = self.creation_domain.world_position();
        let emitter_velocity = if self.has_prev_world_pos && dt > 1e-6 {
            (curr_world_pos - self.prev_world_pos) / dt
        } else {
            Vec3::ZERO
        };
        self.accumulate_and_spawn(dt, &params_template);

        let dt = dt * self.rules.event_playback_scalar;
        let attractor_pos = self
            .attractor_domain
            .as_ref()
            .map(|d| d.world_position())
            .unwrap_or_else(|| self.creation_domain.world_position());
        let attractor_geometry = if let Some(att) = self.attractor_domain.as_ref() {
            let dir = att
                .local_to_world
                .transform_vector3(Vec3::Y)
                .try_normalize()
                .unwrap_or(Vec3::Y);
            AttractorGeometry {
                pos: att.world_position(),
                dir,
                radius_near: att.size_inner.x,
                radius_far: att.size_outer.x,
                radius_orbit: att.size_inner.y,
                length: att.size_outer.y,
            }
        } else {
            AttractorGeometry::default()
        };

        let n = self.points.len();
        for i in 0..n {
            if !self.points.flags[i].contains(PointFlags::IS_BEING_INITIALISED) {
                self.points.prev_pos[i] = self.points.curr_pos[i];
            }
        }
        let live_evo_fallback: &[f32] = if self.evolution_list.is_some() {
            &self.evo_values
        } else {
            params_template.evo_values
        };
        for i in 0..n {
            let was_initing = self.points.flags[i].contains(PointFlags::IS_BEING_INITIALISED);
            let mut snapshot_buf = [0.0_f32; crate::kfp::MAX_EVOLUTIONS];
            let snapshot = self.points.evo_snapshot(i);
            let evo_slice: &[f32] = if !snapshot.is_empty() {
                let n_copy = snapshot.len().min(snapshot_buf.len());
                snapshot_buf[..n_copy].copy_from_slice(&snapshot[..n_copy]);
                &snapshot_buf[..n_copy]
            } else {
                live_evo_fallback
            };
            // A mesh attractor draws each particle to a point of its surface of its own.
            let meshed = self.attractor_domain.as_ref().filter(|d| d.mesh.is_some());
            let (random, rand) = (params_template.random, u32::from(self.points.rand_index[i]));
            let surface = meshed.and_then(|d| d.surface_point(three_uniforms(random, rand + 6)));
            let (attractor_pos, attractor_geometry) = match surface {
                Some(pos) => (
                    pos,
                    AttractorGeometry {
                        pos,
                        ..attractor_geometry
                    },
                ),
                None => (attractor_pos, attractor_geometry),
            };
            let params = BehaviourParams {
                dt,
                effect_time_ratio: params_template.effect_time_ratio,
                emitter_time_ratio: params_template.effect_time_ratio,
                effect_zoom: params_template.effect_zoom * self.rules.event_zoom_scalar,
                emitter_zoom: self.rules.event_zoom_scalar,
                effect_matrix: params_template.effect_matrix,
                emitter_matrix: self.creation_domain.local_to_world,
                effect_velocity: params_template.effect_velocity,
                emitter_velocity,
                gravity: params_template.gravity,
                wind_velocity: params_template.wind_velocity,
                random: params_template.random,
                ptx_random_table: params_template.ptx_random_table,
                evo_list: self.evolution_list.as_ref().or(params_template.evo_list),
                evo_values: evo_slice,
                particle_rule: &self.rules.particle,
                shader_vars: &self.rules.shader_vars,
                effect_rule: params_template.effect_rule,
                emitter_rule: &self.rules.rule,
                camera_pos: params_template.camera_pos,
                camera_basis: params_template.camera_basis,
                emitter_colour_tint: self.rules.event_colour_tint,
                emitter_attractor_pos: attractor_pos,
                attractor_geometry,
                effect_colour_tint_override: params_template.spawned_colour_tint,
                effect_rgba_rand_index: params_template.effect_rgba_rand_index,
                collision_bvh: params_template.collision_bvh,
                river_field: params_template.river_field,
                wind_field: params_template.wind_field,
                wall_time_seconds: params_template.wall_time_seconds,
                real_dt: params_template.real_dt,
                water_field: params_template.water_field,
                water_z_camera: params_template.water_z_camera,
                air_resistance: params_template.air_resistance,
                lod_alpha_mult: params_template.lod_alpha_mult,
                sinks: params_template.sinks,
                effect_spawner_trigger_ratio: if !self
                    .rules
                    .particle
                    .effect_spawner_at_ratio
                    .name
                    .is_empty()
                {
                    self.rules
                        .particle
                        .effect_spawner_at_ratio
                        .trigger_info
                        .clamp(0.0, 1.0)
                } else {
                    -1.0
                },
            };
            for b in &self.behaviours {
                if was_initing {
                    b.init_point(i, &mut self.points, &params);
                }
                b.update_point(i, &mut self.points, &params);
            }
            if was_initing {
                self.points.flags[i].remove(PointFlags::IS_BEING_INITIALISED);
            }
        }

        self.check_spawner_triggers(params_template.random);
        self.check_zcull_spawner(params_template.random);
        let finalize_params = BehaviourParams {
            dt,
            effect_time_ratio: params_template.effect_time_ratio,
            emitter_time_ratio: self.current_life_ratio,
            effect_zoom: params_template.effect_zoom,
            emitter_zoom: 1.0,
            effect_matrix: params_template.effect_matrix,
            emitter_matrix: self.creation_domain.local_to_world,
            effect_velocity: params_template.effect_velocity,
            emitter_velocity,
            gravity: params_template.gravity,
            wind_velocity: params_template.wind_velocity,
            random: params_template.random,
            ptx_random_table: params_template.ptx_random_table,
            evo_list: self.evolution_list.as_ref().or(params_template.evo_list),
            evo_values: if self.evolution_list.is_some() {
                &self.evo_values
            } else {
                params_template.evo_values
            },
            particle_rule: &self.rules.particle,
            shader_vars: &self.rules.shader_vars,
            effect_rule: params_template.effect_rule,
            emitter_rule: &self.rules.rule,
            camera_pos: params_template.camera_pos,
            camera_basis: params_template.camera_basis,
            emitter_colour_tint: self.rules.event_colour_tint,
            emitter_attractor_pos: attractor_pos,
            attractor_geometry,
            effect_colour_tint_override: params_template.spawned_colour_tint,
            effect_rgba_rand_index: params_template.effect_rgba_rand_index,
            collision_bvh: params_template.collision_bvh,
            river_field: params_template.river_field,
            wind_field: params_template.wind_field,
            wall_time_seconds: params_template.wall_time_seconds,
            real_dt: params_template.real_dt,
            water_field: params_template.water_field,
            water_z_camera: params_template.water_z_camera,
            air_resistance: params_template.air_resistance,
            lod_alpha_mult: params_template.lod_alpha_mult,
            sinks: params_template.sinks,
            effect_spawner_trigger_ratio: if !self
                .rules
                .particle
                .effect_spawner_at_ratio
                .name
                .is_empty()
            {
                self.rules
                    .particle
                    .effect_spawner_at_ratio
                    .trigger_info
                    .clamp(0.0, 1.0)
            } else {
                -1.0
            },
        };
        for b in &self.behaviours {
            b.update_finalize(&mut self.points, &finalize_params);
        }

        let mut sink = crate::behaviour::EmitSink::default();
        for b in &self.behaviours {
            b.drain_emits(&mut sink);
        }
        if !sink.liquid.is_empty() {
            self.pending_liquid_emits.append(&mut sink.liquid);
        }
        if !sink.decal_pool.is_empty() {
            self.pending_decal_pool_emits.append(&mut sink.decal_pool);
        }

        self.points.sweep_dead();

        self.current_life_ratio = params_template.effect_time_ratio.clamp(0.0, 1.0);
    }

    pub fn domain_world_matrix(&self) -> Mat4 {
        self.creation_domain.local_to_world
    }

    fn check_spawner_triggers(&mut self, random: &RandomTable) {
        let at_ratio = &self.rules.particle.effect_spawner_at_ratio;
        let on_coln = &self.rules.particle.effect_spawner_on_coln;
        let has_at_ratio = !at_ratio.name.is_empty();
        let has_on_coln = !on_coln.name.is_empty();
        if !has_at_ratio && !has_on_coln {
            return;
        }
        let trigger_ratio = at_ratio.trigger_info.clamp(0.0, 1.0);
        let coln_speed_threshold = on_coln.trigger_info.max(0.0);
        let tracks_dir = on_coln.tracks_point_dir.get();
        let tracks_neg_dir = on_coln.tracks_point_neg_dir.get();

        for i in 0..self.points.len() {
            let flags = self.points.flags[i];
            let trigger_consumed = flags.contains(crate::point::PointFlags::SPAWN_EFFECT);
            let trigger_fallback = !trigger_consumed && self.points.life_ratio[i] >= trigger_ratio;
            if has_at_ratio && (trigger_consumed || trigger_fallback) {
                let _ = trigger_ratio;
                let pos = self.points.curr_pos[i];
                let mtx = glam::Mat4::from_translation(pos);
                let scalars = SpawnedScalars::sample_from_spawner(
                    random,
                    self.points.rand_index[i],
                    at_ratio,
                );
                let parent_remaining_life = if scalars.inherits_point_life {
                    let inv = self.points.one_over_life[i].max(1e-6);
                    Some(((1.0 - self.points.life_ratio[i]) / inv).max(0.001))
                } else {
                    None
                };
                let parent_rand_index = if scalars.tracks_point_pos {
                    Some(self.points.rand_index[i])
                } else {
                    None
                };
                self.pending_spawns.push(PendingSpawn {
                    effect_rule_name: at_ratio.name.clone(),
                    matrix: mtx,
                    scalars,
                    parent_remaining_life,
                    parent_rand_index,
                });
                self.points.flags[i].insert(crate::point::PointFlags::SPAWN_EFFECT);
            }
            if has_on_coln
                && flags.contains(crate::point::PointFlags::HAS_COLLIDED)
                && self.points.coln_speed[i] >= coln_speed_threshold
            {
                let pos = self.points.coln_pos[i];
                let normal = self.points.coln_normal[i]
                    .try_normalize()
                    .unwrap_or(glam::Vec3::Z);
                let forward = if tracks_dir {
                    self.points.curr_vel[i].try_normalize().unwrap_or(normal)
                } else if tracks_neg_dir {
                    -self.points.curr_vel[i].try_normalize().unwrap_or(-normal)
                } else {
                    normal
                };
                let rot = glam::Quat::from_rotation_arc(glam::Vec3::Z, forward);
                let mtx = glam::Mat4::from_rotation_translation(rot, pos);
                let scalars =
                    SpawnedScalars::sample_from_spawner(random, self.points.rand_index[i], on_coln);
                let parent_remaining_life = if scalars.inherits_point_life {
                    let inv = self.points.one_over_life[i].max(1e-6);
                    Some(((1.0 - self.points.life_ratio[i]) / inv).max(0.001))
                } else {
                    None
                };
                let parent_rand_index = if scalars.tracks_point_pos {
                    Some(self.points.rand_index[i])
                } else {
                    None
                };
                self.pending_spawns.push(PendingSpawn {
                    effect_rule_name: on_coln.name.clone(),
                    matrix: mtx,
                    scalars,
                    parent_remaining_life,
                    parent_rand_index,
                });
            }
        }
        for i in 0..self.points.len() {
            self.points.flags[i].remove(crate::point::PointFlags::HAS_COLLIDED);
        }
    }

    fn refresh_domains(&mut self, effect_matrix: Mat4, effect_time_ratio: f32, effect_zoom: f32) {
        use crate::domain_sample::{DomainRefFrames, DomainSlot};
        let zoom = effect_zoom * self.rules.event_zoom_scalar;
        self.creation_domain.refresh_for_slot(
            &self.rules.creation_domain,
            effect_time_ratio,
            effect_matrix,
            zoom,
            DomainSlot::Creation,
            DomainRefFrames::default(),
        );
        let creation_world = Some(self.creation_domain.local_to_world);
        self.velocity_domain.refresh_for_slot(
            &self.rules.velocity_domain,
            effect_time_ratio,
            effect_matrix,
            zoom,
            DomainSlot::Target,
            DomainRefFrames {
                creation_world,
                target_world: None,
            },
        );
        let target_world = Some(self.velocity_domain.local_to_world);
        if let Some(att) = &self.rules.attractor_domain {
            let mut inst = self.attractor_domain.take().unwrap_or_default();
            inst.refresh_for_slot(
                att,
                effect_time_ratio,
                effect_matrix,
                zoom,
                DomainSlot::Attractor,
                DomainRefFrames {
                    creation_world,
                    target_world,
                },
            );
            self.attractor_domain = Some(inst);
        } else {
            self.attractor_domain = Some(point_domain_at(self.creation_domain.world_position()));
        }
    }

    fn accumulate_and_spawn(&mut self, dt: f32, params: &BehaviourParamsTemplate<'_>) {
        let spawn_kfp = query_kfp_with_evo(
            &self.rules.rule.spawn_rate_over_time_kfp,
            params.effect_time_ratio,
            params.evo_list,
            params.evo_values,
        );
        let life_kfp = query_kfp_with_evo(
            &self.rules.rule.particle_life_kfp,
            params.effect_time_ratio,
            params.evo_list,
            params.evo_values,
        );
        let bias_seed = (params.effect_time_ratio * 1009.0).abs() as u32 ^ 0xACE_C0DE;
        let bias_rate = params.random.value(bias_seed);
        let bias_life = params.random.value(bias_seed.wrapping_add(7));
        let lerp_or_x = |x: f32, y: f32, b: f32| {
            if y.abs() < 1e-6 {
                x
            } else {
                x + (y - x) * b
            }
        };
        let mut spawn_rate = lerp_or_x(spawn_kfp.x, spawn_kfp.y, bias_rate).max(0.0);
        let particle_life = lerp_or_x(life_kfp.x, life_kfp.y, bias_life).max(0.05);
        let pb_kfp = query_kfp_with_evo(
            &self.rules.rule.playback_rate_scalar_kfp,
            params.effect_time_ratio,
            params.evo_list,
            params.evo_values,
        )
        .x;
        if pb_kfp > 1e-6 {
            spawn_rate *= pb_kfp;
        }
        let z_cap = spawn_kfp.z;
        let live_cap = if z_cap > 0.5 {
            (z_cap as usize).min(self.max_particles_soft_cap)
        } else {
            self.max_particles_soft_cap
        };
        let cap_left = live_cap.saturating_sub(self.points.len());
        if cap_left == 0 {
            self.prev_life_ratio = params.effect_time_ratio;
            self.prev_world_pos = self.creation_domain.world_position();
            self.has_prev_world_pos = true;
            return;
        }

        if self.rules.rule.is_one_shot.get() && !self.points.is_empty() {
            self.prev_life_ratio = params.effect_time_ratio;
            return;
        }

        let curr_world_pos = self.creation_domain.world_position();
        let prev_world_pos = if self.has_prev_world_pos {
            self.prev_world_pos
        } else {
            curr_world_pos
        };

        if self.rules.rule.is_one_shot.get() {
            if self.has_fired_one_shot {
                return;
            }
            let to_spawn = spawn_rate.round() as usize;
            for _ in 0..to_spawn.min(cap_left) {
                self.spawn_one(particle_life, params, Vec3::ZERO);
            }
            self.prev_world_pos = curr_world_pos;
            self.has_prev_world_pos = true;
            self.has_fired_one_shot = true;
            self.prev_life_ratio = params.effect_time_ratio;
            return;
        }

        let orig_emitter_life_ratio = params.effect_time_ratio;
        let event_start = self.rules.event_start_ratio;
        let event_end = self.rules.event_end_ratio;
        let first_tick = self.prev_life_ratio < 0.0;
        let prev_life_ratio = if first_tick {
            0.0
        } else {
            self.prev_life_ratio
        };
        let mut frame_coverage_start: f32 = 0.0;
        let mut frame_coverage_end: f32 = 1.0;
        let mut emitter_ended_this_frame = false;
        if !first_tick && prev_life_ratio < orig_emitter_life_ratio {
            if prev_life_ratio < event_start {
                let span = (orig_emitter_life_ratio - prev_life_ratio).max(1e-6);
                frame_coverage_start = ((event_start - prev_life_ratio) / span).clamp(0.0, 1.0);
            }
            if orig_emitter_life_ratio > event_end {
                let span = (orig_emitter_life_ratio - prev_life_ratio).max(1e-6);
                frame_coverage_end = ((event_end - prev_life_ratio) / span).clamp(0.0, 1.0);
                emitter_ended_this_frame = true;
            }
        }
        let frame_coverage_ratio = (frame_coverage_end - frame_coverage_start).clamp(0.0, 1.0);

        let dist_kfp = query_kfp_with_evo(
            &self.rules.rule.spawn_rate_over_dist_kfp,
            params.effect_time_ratio,
            params.evo_list,
            params.evo_values,
        );
        let bias_dist = params.random.value(bias_seed.wrapping_add(13));
        let spawn_rate_over_dist = lerp_or_x(dist_kfp.x, dist_kfp.y, bias_dist).max(0.0);

        let dist_travelled = (curr_world_pos - prev_world_pos).length();

        if spawn_rate_over_dist > 0.0 && dist_travelled > 25.0 {
            self.spawn_accumulator = 0.0;
            self.prev_world_pos = curr_world_pos;
            self.has_prev_world_pos = true;
            self.prev_life_ratio = orig_emitter_life_ratio;
            return;
        }

        let mut spawn_count_this_frame = spawn_rate * dt * frame_coverage_ratio
            + dist_travelled * spawn_rate_over_dist * frame_coverage_ratio;

        if spawn_count_this_frame > 0.0 && emitter_ended_this_frame {
            spawn_count_this_frame += 0.001;
        }

        self.spawn_accumulator += spawn_count_this_frame;
        let mut n_to_spawn = self.spawn_accumulator.floor() as i32;
        if n_to_spawn < 0 {
            n_to_spawn = 0;
        }
        let mut n_to_spawn = n_to_spawn as usize;
        if n_to_spawn > cap_left {
            n_to_spawn = cap_left;
        }
        self.spawn_accumulator -= self.spawn_accumulator.floor();
        let is_screen_space = (self.rules.particle.flags & 0x02) != 0;
        let matrix_changed = !mats_close(params.effect_matrix, params.prev_effect_matrix);
        let needs_subframe_refresh = n_to_spawn > 1 && matrix_changed && !is_screen_space;
        let saved_creation = self.creation_domain.clone();
        let saved_velocity = self.velocity_domain.clone();
        let saved_attractor = self.attractor_domain.clone();
        for i in 0..n_to_spawn {
            let t = (i as f32 + 1.0) / (n_to_spawn as f32 + 1.0);

            let offset = if needs_subframe_refresh {
                let sub_mtx = lerp_mat4(params.prev_effect_matrix, params.effect_matrix, t);
                let sub_time_ratio = params.prev_effect_time_ratio
                    + (params.effect_time_ratio - params.prev_effect_time_ratio) * t;
                self.refresh_domains(sub_mtx, sub_time_ratio, params.effect_zoom);
                Vec3::ZERO
            } else if is_screen_space {
                Vec3::ZERO
            } else {
                let interp = prev_world_pos.lerp(curr_world_pos, t);
                interp - curr_world_pos
            };
            self.spawn_one(particle_life, params, offset);
        }
        if needs_subframe_refresh {
            self.creation_domain = saved_creation;
            self.velocity_domain = saved_velocity;
            self.attractor_domain = saved_attractor;
        }

        self.prev_world_pos = curr_world_pos;
        self.has_prev_world_pos = true;
        self.prev_life_ratio = orig_emitter_life_ratio;
    }

    fn spawn_one(
        &mut self,
        particle_life: f32,
        params: &BehaviourParamsTemplate<'_>,
        position_offset: Vec3,
    ) {
        let _ = params.ptx_random_table;
        let rand_idx = self.next_rand_index;
        self.next_rand_index = self.next_rand_index.wrapping_add(1);
        let i = self.points.spawn(rand_idx);

        let pos_uvw = three_uniforms(params.random, rand_idx as u32);
        let mut pos = self.creation_domain.sample_position(pos_uvw) + position_offset;
        let vel_uvw = three_uniforms(params.random, rand_idx as u32 + 3);
        let vel = self.velocity_domain.sample_direction(vel_uvw);

        let inherit_v = query_kfp_with_evo(
            &self.rules.rule.inherit_velocity_kfp,
            params.effect_time_ratio,
            params.evo_list,
            params.evo_values,
        )
        .x;
        let inherit_world = params.effect_velocity * inherit_v;

        self.points.curr_pos[i] = pos;
        self.points.prev_pos[i] = pos;
        self.points.spawn_pos[i] = pos;
        self.points.curr_vel[i] = vel + inherit_world;
        self.points.one_over_life[i] = 1.0 / particle_life;
        self.points.life_ratio[i] = 0.0;
        self.points.emitter_life_ratio[i] = self.current_life_ratio;
        self.points.playback_rate[i] = 1.0;

        let evo_for_snap = if let Some(emitter_evo) = self.evolution_list.as_ref() {
            let _ = emitter_evo;
            self.evo_values.as_slice()
        } else {
            params.evo_values
        };
        if !evo_for_snap.is_empty() {
            self.points.set_evo_snapshot(i, evo_for_snap);
        }

        let cam_pos = params.camera_pos;
        let sort_val = match self.rules.particle.sort_type {
            0 | 1 => (pos - cam_pos).length_squared(),
            2 | 3 => params.effect_time_ratio,
            4 => params.random.value(rand_idx as u32 + 0x57_07) * 100.0,
            _ => (pos - cam_pos).length_squared(),
        };
        self.points.sort_val[i] = sort_val;
        let _ = &mut pos;
    }
}

#[derive(Clone, Copy)]
pub struct BehaviourParamsTemplate<'a> {
    pub effect_time_ratio: f32,
    pub prev_effect_time_ratio: f32,
    pub effect_zoom: f32,
    pub effect_matrix: Mat4,
    pub prev_effect_matrix: Mat4,
    pub effect_velocity: Vec3,
    pub gravity: f32,
    pub wind_velocity: Vec3,
    pub random: &'a RandomTable,
    pub ptx_random_table: &'a crate::random_table::PtxRandomTable,
    pub evo_list: Option<&'a or_particle::rules::EvolutionList>,
    pub evo_values: &'a [f32],
    pub effect_rule: &'a or_io::gta5::particle::PtxEffectRule<or_io::decl::Gen8>,
    pub camera_pos: Vec3,
    pub camera_basis: glam::Mat3,
    pub effect_rgba_rand_index: u8,
    pub spawned_colour_tint: Vec4,
    pub collision_bvh: Option<&'a crate::collision::BvhCollider>,
    pub river_field: Option<&'a (dyn crate::behaviours::RiverField + Send + Sync)>,
    pub wind_field: Option<&'a (dyn crate::behaviours::WindField + Send + Sync)>,
    pub wall_time_seconds: f32,
    pub real_dt: f32,
    pub water_field: Option<&'a (dyn crate::behaviours::WaterField + Send + Sync)>,
    pub water_z_camera: f32,
    pub air_resistance: f32,
    pub lod_alpha_mult: f32,
    pub sinks: crate::sinks::PtfxSinks<'a>,
}

#[allow(dead_code)]
fn _vec4_unused() -> Vec4 {
    Vec4::ZERO
}
