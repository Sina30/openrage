use crate::emitter_inst::{BehaviourParamsTemplate, EmitterInst};
use crate::kfp::MAX_EVOLUTIONS;
use crate::manager::WorldHooks;
use crate::random::RandomTable;
use crate::random_table::PtxRandomTable;
use glam::{Mat4, Vec3};
use or_io::decl::Gen8;
use or_io::gta5::particle::PtxEffectRule;
use or_particle::list;
use or_particle::rules::EvolutionList;
pub struct EffectInst {
    /// ⚠ Its timeline is dropped: the emitters hold what its events play.
    pub rule: PtxEffectRule<Gen8>,
    pub emitters: Vec<EmitterInst>,
    pub matrix: Mat4,
    pub prev_matrix: Mat4,
    pub time_seconds: f32,
    pub prev_time_ratio: f32,
    pub duration: f32,
    pub previous_pos: Vec3,
    pub current_pos: Vec3,
    pub evolution_list: Option<EvolutionList>,
    pub evo_values: Vec<f32>,
    pub current_zoom: f32,
    pub camera_pos: Vec3,
    pub playback_rate_scalar: f32,
    pub rgba_rand_index: u8,
    /// Engine `m_zoomScalarRndIdx` — captured at `Start` and used to
    /// lerp the per-instance zoom scalar.
    pub zoom_scalar_rand_index: u8,
    pub trail_tex_info_rand_index: u8,
    pub high_quality_evo: f32,
    pub is_update_culled: bool,
    pub is_emit_culled: bool,
    pub lod_alpha_mult: f32,
    pub spawned_scalars: crate::emitter_inst::SpawnedScalars,
    pub parent_attachment: Option<u8>,
    pub is_camera_relative: bool,
    pub ambient_natural_scale: f32,
    pub ambient_artificial_scale: f32,
    pub is_in_interior: bool,
    pub camera_basis: glam::Mat3,
    /// Engine `m_currLoop` — number of completed loop crossings.
    /// Engine semantics:
    ///   - `num_loops == -1` → infinite looping (counter still
    ///     advances but is never compared).
    ///   - `num_loops == 0`  → play once (no wrap; clamps).
    ///   - `num_loops == N`  → loop `N` times then terminate
    ///     (`m_currLoop >= N` ⇒ `IS_FINISHED`).
    pub curr_loop: u32,
    pub is_finished: bool,
}

impl EffectInst {
    pub fn new(rule: PtxEffectRule<Gen8>, emitters: Vec<EmitterInst>) -> Self {
        Self::with_evolution(rule, emitters, None)
    }

    pub fn with_random_table(
        rule: PtxEffectRule<Gen8>,
        emitters: Vec<EmitterInst>,
        table: &PtxRandomTable,
    ) -> Self {
        let mut e = Self::with_evolution(rule, emitters, None);
        e.rgba_rand_index = table.next_index();
        e.zoom_scalar_rand_index = table.next_index();
        e.trail_tex_info_rand_index = table.next_index();
        e
    }

    pub fn with_evolution(
        rule: PtxEffectRule<Gen8>,
        emitters: Vec<EmitterInst>,
        evolution_list: Option<EvolutionList>,
    ) -> Self {
        let mut rule = rule;
        let combined: Vec<or_io::gta5::particle::PtxBiasLink> = emitters
            .iter()
            .flat_map(|em| em.rules.particle.bias_links.iter().cloned())
            .collect();
        crate::kfp::apply_bias_links_to_effect_rule(&combined, &mut rule);
        let dmin = rule.duration_min.min(rule.duration_max).max(0.001);
        let dmax = rule.duration_max.max(rule.duration_min).max(dmin);
        let duration = if (dmax - dmin).abs() < 1e-4 {
            dmax
        } else {
            let seed = list::code(&rule.name).wrapping_add(0xD0DA_F00D);
            let bias = crate::random::RandomTable::with_seed(seed).value(0xDB_DB);
            (dmin + (dmax - dmin) * bias).max(0.001)
        };
        let mut evo_values = vec![0.0_f32; MAX_EVOLUTIONS];
        if let Some(list) = evolution_list.as_ref() {
            for (i, ev) in list.evolutions.iter().take(MAX_EVOLUTIONS).enumerate() {
                if ev.is_overridden {
                    evo_values[i] = ev.override_value;
                }
            }
        }
        let current_zoom = 1.0_f32;
        let is_camera_relative = false;
        let seed = list::code(&rule.name).wrapping_add(0x0000_EFFE_C770);
        let table = crate::random::RandomTable::with_seed(seed);
        let bias = table.value(0xC0FFEE);
        let playback_rate_scalar = (rule.playback_rate_scalar_min
            + (rule.playback_rate_scalar_max - rule.playback_rate_scalar_min) * bias)
            .max(0.001);
        Self {
            rule,
            emitters,
            matrix: Mat4::IDENTITY,
            prev_matrix: Mat4::IDENTITY,
            time_seconds: 0.0,
            prev_time_ratio: 0.0,
            duration,
            previous_pos: Vec3::ZERO,
            current_pos: Vec3::ZERO,
            evolution_list,
            evo_values,
            current_zoom,
            camera_pos: Vec3::ZERO,
            playback_rate_scalar,
            rgba_rand_index: 0,
            zoom_scalar_rand_index: 0,
            trail_tex_info_rand_index: 0,
            high_quality_evo: 0.0,
            is_update_culled: false,
            is_emit_culled: false,
            lod_alpha_mult: 1.0,
            spawned_scalars: crate::emitter_inst::SpawnedScalars::IDENTITY,
            parent_attachment: None,
            is_camera_relative,
            ambient_natural_scale: 1.0,
            ambient_artificial_scale: 1.0,
            is_in_interior: false,
            camera_basis: glam::Mat3::IDENTITY,
            curr_loop: 0,
            is_finished: false,
        }
    }

    pub fn set_ambient_scales(&mut self, natural: f32, artificial: f32, in_interior: bool) {
        self.ambient_natural_scale = natural;
        self.ambient_artificial_scale = artificial;
        self.is_in_interior = in_interior;
    }

    pub fn apply_spawned_scalars(&mut self, s: crate::emitter_inst::SpawnedScalars) {
        self.spawned_scalars = s;
        self.duration = (self.duration * s.duration_scalar).max(0.001);
        self.playback_rate_scalar = (self.playback_rate_scalar * s.playback_rate_scalar).max(0.001);
    }

    pub fn apply_inherited_life(&mut self, parent_remaining_life_seconds: f32) {
        let truncated = parent_remaining_life_seconds.max(0.001);
        if truncated < self.duration {
            self.duration = truncated;
        }
    }

    pub fn set_camera_pos(&mut self, pos: Vec3) {
        self.camera_pos = pos;
    }

    pub fn set_camera_basis(&mut self, right: Vec3, up: Vec3, forward: Vec3) {
        self.camera_basis = glam::Mat3::from_cols(right, up, forward);
    }

    pub fn time_ratio(&self) -> f32 {
        (self.time_seconds / self.duration).clamp(0.0, 1.0)
    }

    pub fn live_count(&self) -> usize {
        self.emitters.iter().map(|e| e.live_count()).sum()
    }

    pub fn reset(&mut self) {
        self.time_seconds = 0.0;
        self.curr_loop = 0;
        self.is_finished = false;
        for e in &mut self.emitters {
            e.reset();
        }
    }

    pub fn stop(&mut self) {
        self.time_seconds = self.duration;
        for em in &mut self.emitters {
            em.has_fired_one_shot = true;
        }
    }

    pub fn finish(&mut self, kill_emitted: bool) {
        self.is_finished = true;
        self.time_seconds = self.duration;
        if kill_emitted {
            for em in &mut self.emitters {
                em.points.clear();
            }
        }
    }

    pub fn relocate(&mut self, new_matrix: Mat4) {
        let old_pos = self.matrix.w_axis.truncate();
        let new_pos = new_matrix.w_axis.truncate();
        let delta = new_pos - old_pos;
        self.matrix = new_matrix;
        self.previous_pos = new_pos;
        self.current_pos = new_pos;
        if delta.length_squared() > 1e-8 {
            for em in &mut self.emitters {
                for i in 0..em.points.len() {
                    em.points.curr_pos[i] += delta;
                    em.points.prev_pos[i] += delta;
                    em.points.spawn_pos[i] += delta;
                }
            }
        }
    }

    pub fn update(
        &mut self,
        dt: f32,
        random: &RandomTable,
        ptx_random_table: &PtxRandomTable,
        gravity: f32,
        wind_velocity: Vec3,
    ) {
        self.update_with_world(
            dt,
            random,
            ptx_random_table,
            gravity,
            wind_velocity,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_with_world(
        &mut self,
        dt: f32,
        random: &RandomTable,
        ptx_random_table: &PtxRandomTable,
        gravity: f32,
        wind_velocity: Vec3,
        collision_bvh: Option<&crate::collision::BvhCollider>,
        river_field: Option<&(dyn crate::behaviours::RiverField + Send + Sync)>,
    ) {
        let hooks = WorldHooks {
            gravity,
            wind_velocity,
            collision_bvh,
            river_field,
            water_field: None,
            wind_field: None,
            wall_time_seconds: 0.0,
            water_z_camera: 0.0,
            air_resistance: 1.0,
            lod_distance_override: None,
            sinks: crate::sinks::PtfxSinks::none(),
        };
        self.update_with_hooks(dt, random, ptx_random_table, &hooks);
    }

    pub fn update_with_hooks(
        &mut self,
        dt: f32,
        random: &RandomTable,
        ptx_random_table: &PtxRandomTable,
        hooks: &WorldHooks<'_>,
    ) {
        let gravity = hooks.gravity;
        let wind_velocity = hooks.wind_velocity;
        let collision_bvh = hooks.collision_bvh;
        let river_field = hooks.river_field;
        let water_field = hooks.water_field;
        let wind_field = hooks.wind_field;
        let wall_time_seconds = hooks.wall_time_seconds;
        let water_z_camera = hooks.water_z_camera;
        let air_resistance = hooks.air_resistance;
        let lod_distance_override = hooks.lod_distance_override;
        self.prev_matrix = self.matrix;
        self.prev_time_ratio = self.time_ratio();
        let real_dt = dt;
        let dt = dt * self.playback_rate_scalar;
        if self.is_camera_relative {
            let cam = self.camera_pos;
            self.matrix.w_axis = glam::Vec4::new(cam.x, cam.y, cam.z, 1.0);
        }
        if self.high_quality_evo > 0.0 {
            if let Some(list) = self.evolution_list.as_ref() {
                for (idx, evo) in list.evolutions.iter().enumerate() {
                    if idx >= self.evo_values.len() {
                        break;
                    }
                    if evo.name.eq_ignore_ascii_case("hq") {
                        self.evo_values[idx] = self.high_quality_evo;
                    }
                }
            }
        }
        let cam_dist = (self.camera_pos - self.matrix.w_axis.truncate()).length();
        let dist_cull = self.rule.distance_culling_cull_dist.max(0.0);
        let viewport_cull_radius = self.rule.viewport_culling_sphere_radius.max(0.0);
        let dist_culled = dist_cull > 0.0 && cam_dist > dist_cull;
        let viewport_culled = viewport_cull_radius > 0.0 && cam_dist > viewport_cull_radius;
        self.is_update_culled = (dist_culled && !self.rule.update_when_distance_culled.get())
            || (viewport_culled && !self.rule.update_when_viewport_culled.get());
        self.is_emit_culled = (dist_culled && !self.rule.emit_when_distance_culled.get())
            || (viewport_culled && !self.rule.emit_when_viewport_culled.get());
        self.lod_alpha_mult = if dist_cull > 0.0 {
            let fade_start = dist_cull * 0.75;
            if cam_dist <= fade_start {
                1.0
            } else if cam_dist >= dist_cull {
                0.0
            } else {
                1.0 - ((cam_dist - fade_start) / (dist_cull - fade_start)).clamp(0.0, 1.0)
            }
        } else {
            1.0
        };
        if self.is_update_culled {
            return;
        }
        if let Some(list) = self.evolution_list.as_ref() {
            let cam_dist = (self.camera_pos - self.matrix.w_axis.truncate()).length();
            let lod_min = self.rule.lod_evo_dist_min;
            let lod_max = self.rule.lod_evo_dist_max;
            let lod_value = if let Some(v) = lod_distance_override {
                v.clamp(0.0, 1.0)
            } else if lod_max > lod_min {
                (1.0 - ((cam_dist - lod_min) / (lod_max - lod_min)).clamp(0.0, 1.0)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let mut tagged_slots: [bool; crate::kfp::MAX_EVOLUTIONS] =
                [false; crate::kfp::MAX_EVOLUTIONS];
            for prop in &list.evolved_keyframe_props {
                for ek in &prop.evolved_keyframes {
                    if ek.is_lod_evolution {
                        let idx = ek.evolution_index as usize;
                        let evo = list
                            .evolutions
                            .get(idx)
                            .map(|e| e.is_overridden)
                            .unwrap_or(false);
                        if !evo && idx < self.evo_values.len() {
                            self.evo_values[idx] = lod_value;
                        }
                        if idx < tagged_slots.len() {
                            tagged_slots[idx] = true;
                        }
                    }
                }
            }
            let auto_lod_active = lod_max > lod_min || lod_distance_override.is_some();
            if auto_lod_active {
                for (idx, evo) in list.evolutions.iter().enumerate() {
                    if idx >= self.evo_values.len() || idx >= tagged_slots.len() {
                        continue;
                    }
                    if tagged_slots[idx] || evo.is_overridden {
                        continue;
                    }
                    let lowered = evo.name.to_ascii_lowercase();
                    if lowered == "lod" || lowered == "distance" {
                        self.evo_values[idx] = lod_value;
                    }
                }
            }
        }
        // The editor's sim clock layers its own loop on top, but this wrap
        // keeps the time ratio in `[0, 1]` even when the host hands us a
        // single huge dt. The manager drops a spawned inst once it is finished.
        self.time_seconds += dt;
        let mut just_looped = false;
        if self.time_seconds >= self.duration {
            if self.rule.num_loops == 0 {
                self.time_seconds = self.duration;
                self.is_finished = true;
            } else {
                self.time_seconds = self.time_seconds.rem_euclid(self.duration);
                just_looped = true;
                self.curr_loop = self.curr_loop.saturating_add(1);
                if self.rule.num_loops > 0 && self.curr_loop >= self.rule.num_loops as u32 {
                    self.is_finished = true;
                }
                let dmin = self
                    .rule
                    .duration_min
                    .min(self.rule.duration_max)
                    .max(0.001);
                let dmax = self.rule.duration_max.max(self.rule.duration_min).max(dmin);
                if (dmax - dmin).abs() > 1e-4 {
                    let seed =
                        (self.time_seconds * 1009.0).abs() as u32 ^ list::code(&self.rule.name);
                    let bias = random.value(seed);
                    self.duration = (dmin + (dmax - dmin) * bias).max(0.001);
                }
            }
        }
        if just_looped {
            for em in self.emitters.iter_mut() {
                em.notify_looped();
            }
        }
        if just_looped {
            let r = random;
            let seed = (self.time_seconds * 101.0).abs() as u32;
            let bx = r.value(seed) * 2.0 - 1.0;
            let by = r.value(seed + 1) * 2.0 - 1.0;
            let bz = r.value(seed + 2) * 2.0 - 1.0;
            let off = Vec3::new(
                self.rule.rand_offset_pos.x * bx,
                self.rule.rand_offset_pos.y * by,
                self.rule.rand_offset_pos.z * bz,
            );
            self.matrix.w_axis += glam::Vec4::new(off.x, off.y, off.z, 0.0);
        }

        self.previous_pos = self.current_pos;
        self.current_pos = self.matrix.w_axis.truncate();
        let velocity = if dt > 1e-5 {
            (self.current_pos - self.previous_pos) / dt
        } else {
            Vec3::ZERO
        };

        let zoom_kfp = crate::kfp::query_kfp_with_evo(
            &self.rule.zoom_scalar_kfp,
            self.time_ratio(),
            self.evolution_list.as_ref(),
            &self.evo_values,
        );
        let zoom_kfp_unit = if zoom_kfp == glam::Vec4::ZERO {
            1.0
        } else {
            let bias = ptx_random_table.value(self.zoom_scalar_rand_index);
            let lerped = zoom_kfp.x + (zoom_kfp.y - zoom_kfp.x) * bias;
            lerped * 0.01
        };
        self.current_zoom = (zoom_kfp_unit * self.spawned_scalars.zoom_scalar).max(0.001);
        let template = BehaviourParamsTemplate {
            effect_time_ratio: self.time_ratio(),
            prev_effect_time_ratio: self.prev_time_ratio,
            effect_zoom: self.current_zoom,
            effect_matrix: self.matrix,
            prev_effect_matrix: self.prev_matrix,
            effect_velocity: velocity,
            gravity,
            wind_velocity,
            random,
            ptx_random_table,
            effect_rgba_rand_index: self.rgba_rand_index,
            evo_list: self.evolution_list.as_ref(),
            evo_values: &self.evo_values,
            effect_rule: &self.rule,
            camera_pos: self.camera_pos,
            camera_basis: self.camera_basis,
            spawned_colour_tint: self.spawned_scalars.colour_tint,
            collision_bvh,
            river_field,
            wind_field,
            wall_time_seconds,
            real_dt,
            water_field,
            water_z_camera,
            air_resistance,
            lod_alpha_mult: self.lod_alpha_mult,
            sinks: hooks.sinks,
        };
        crate::parallel::tick_emitters(&mut self.emitters, dt, template);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rule_with_game_flags(game_flags: u8) -> PtxEffectRule<Gen8> {
        let mut rule = PtxEffectRule::<Gen8>::default();
        (rule.duration_min, rule.duration_max) = (1.0, 1.0);
        (rule.playback_rate_scalar_min, rule.playback_rate_scalar_max) = (1.0, 1.0);
        rule.game_flags = game_flags;
        rule
    }

    #[test]
    fn game_flags_bit_zero_does_not_make_effect_follow_camera() {
        let rule = rule_with_game_flags(0x01);
        let mut eff = EffectInst::new(rule, vec![]);
        assert!(
            !eff.is_camera_relative,
            "rule.game_flags bit 0 is the engine's 'Sort Within GTA \
             Scene' flag and must not be re-purposed as a camera-relative \
             snap trigger"
        );
        eff.matrix.w_axis = glam::Vec4::new(0.0, 0.0, 0.0, 1.0);

        let random = RandomTable::default();
        let ptx = PtxRandomTable::default();
        eff.set_camera_pos(Vec3::new(10.0, 20.0, 30.0));
        eff.update_with_world(0.016, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        let t = eff.matrix.w_axis.truncate();
        assert!(
            t.length() < 1e-5,
            "matrix translation must stay at the authored origin \
             instead of snapping to the camera, got {t:?}"
        );

        eff.set_camera_pos(Vec3::new(-5.0, 0.0, 7.0));
        eff.update_with_world(0.016, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        let t = eff.matrix.w_axis.truncate();
        assert!(
            t.length() < 1e-5,
            "matrix translation must remain at origin after the \
             camera moves, got {t:?}"
        );
    }

    #[test]
    fn explicit_camera_relative_opt_in_follows_camera() {
        let rule = rule_with_game_flags(0x00);
        let mut eff = EffectInst::new(rule, vec![]);
        assert!(!eff.is_camera_relative);
        eff.is_camera_relative = true;

        let random = RandomTable::default();
        let ptx = PtxRandomTable::default();
        eff.set_camera_pos(Vec3::new(10.0, 20.0, 30.0));
        eff.update_with_world(0.016, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        let t = eff.matrix.w_axis.truncate();
        assert!((t - Vec3::new(10.0, 20.0, 30.0)).length() < 1e-5);

        eff.set_camera_pos(Vec3::new(-5.0, 0.0, 7.0));
        eff.update_with_world(0.016, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        let t = eff.matrix.w_axis.truncate();
        assert!((t - Vec3::new(-5.0, 0.0, 7.0)).length() < 1e-5);
    }

    #[test]
    fn lod_evolution_clamps_at_max_dist() {
        use or_particle::rules::{Evolution, EvolutionList};
        let mut rule = rule_with_game_flags(0x00);
        rule.lod_evo_dist_min = 5.0;
        rule.lod_evo_dist_max = 50.0;
        let evo_list = EvolutionList {
            evolutions: vec![Evolution {
                name: "LOD".into(),
                name_hash: 0,
                override_value: 0.0,
                is_overridden: false,
            }],
            evolved_keyframe_props: Vec::new(),
        };
        let mut eff = EffectInst::with_evolution(rule, vec![], Some(evo_list));
        let random = RandomTable::default();
        let ptx = PtxRandomTable::default();

        eff.set_camera_pos(Vec3::new(1000.0, 0.0, 0.0));
        eff.update_with_world(0.016, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        assert!(
            eff.evo_values[0].abs() < 1e-5,
            "far-plane should clamp to 0, got {}",
            eff.evo_values[0]
        );

        eff.set_camera_pos(Vec3::new(0.0, 0.0, 0.0));
        eff.update_with_world(0.016, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        assert!(
            (eff.evo_values[0] - 1.0).abs() < 1e-5,
            "near-plane should clamp to 1, got {}",
            eff.evo_values[0]
        );

        eff.set_camera_pos(Vec3::new(27.5, 0.0, 0.0));
        eff.update_with_world(0.016, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        assert!(
            (eff.evo_values[0] - 0.5).abs() < 0.01,
            "mid-distance should be ~0.5, got {}",
            eff.evo_values[0]
        );
    }

    #[test]
    fn non_camera_relative_effect_stays_put() {
        let rule = rule_with_game_flags(0x00);
        let mut eff = EffectInst::new(rule, vec![]);
        assert!(!eff.is_camera_relative);
        eff.matrix.w_axis = glam::Vec4::new(99.0, 99.0, 99.0, 1.0);
        let random = RandomTable::default();
        let ptx = PtxRandomTable::default();
        eff.set_camera_pos(Vec3::new(10.0, 20.0, 30.0));
        eff.update_with_world(0.016, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        let t = eff.matrix.w_axis.truncate();
        assert!((t - Vec3::new(99.0, 99.0, 99.0)).length() < 1e-5);
    }

    #[test]
    fn finite_loops_terminate_at_count() {
        let mut rule = rule_with_game_flags(0x00);
        rule.duration_min = 1.0;
        rule.duration_max = 1.0;
        rule.num_loops = 3;
        let mut eff = EffectInst::new(rule, vec![]);
        let random = RandomTable::default();
        let ptx = PtxRandomTable::default();
        for _ in 0..3 {
            eff.update_with_world(1.1, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        }
        assert_eq!(eff.curr_loop, 3, "should have wrapped 3 times");
        assert!(eff.is_finished, "finite-loop effect must mark finished");
    }

    #[test]
    fn play_once_finishes_at_duration() {
        let mut rule = rule_with_game_flags(0x00);
        rule.duration_min = 0.5;
        rule.duration_max = 0.5;
        rule.num_loops = 0;
        let mut eff = EffectInst::new(rule, vec![]);
        let random = RandomTable::default();
        let ptx = PtxRandomTable::default();
        eff.update_with_world(0.6, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        assert!(
            eff.is_finished,
            "play-once must mark finished after duration"
        );
        assert_eq!(eff.curr_loop, 0, "play-once never increments curr_loop");
    }

    #[test]
    fn infinite_loops_never_finish() {
        let mut rule = rule_with_game_flags(0x00);
        rule.duration_min = 0.25;
        rule.duration_max = 0.25;
        rule.num_loops = -1;
        let mut eff = EffectInst::new(rule, vec![]);
        let random = RandomTable::default();
        let ptx = PtxRandomTable::default();
        for _ in 0..20 {
            eff.update_with_world(0.3, &random, &ptx, 0.0, Vec3::ZERO, None, None);
        }
        assert!(!eff.is_finished, "infinite loops must never finish");
        assert!(
            eff.curr_loop >= 10,
            "loop counter still advances; got {}",
            eff.curr_loop
        );
    }
}
