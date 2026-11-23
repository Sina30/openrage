use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::kfp::{key_time_for_mode, KeyframeMode as SimKeyframeMode};
use crate::point::PointBuffer;
use glam::{Vec2, Vec4};
use or_io::gta5::particle::{AnimTexLoopMode, KeyframeMode};
use or_particle::behaviour::AnimateTextureBehaviour;
#[derive(Debug, Clone)]
pub struct AnimateTexture {
    pub data: AnimateTextureBehaviour,
    pub frame_min: u8,
    pub frame_max: u8,
}

impl AnimateTexture {
    pub fn from_data(data: AnimateTextureBehaviour) -> Self {
        Self {
            data,
            frame_min: 0,
            frame_max: 0,
        }
    }

    pub fn with_frame_range(mut self, lo: u32, hi: u32) -> Self {
        self.frame_min = lo.min(255) as u8;
        self.frame_max = hi.min(255) as u8;
        self
    }

    fn kfp_time(&self, idx: usize, buf: &PointBuffer, params: &BehaviourParams<'_>) -> f32 {
        let mode = match self.data.keyframe_mode {
            KeyframeMode::Point => SimKeyframeMode::Point,
            KeyframeMode::Effect => SimKeyframeMode::Effect,
            KeyframeMode::Emitter => SimKeyframeMode::Emitter,
        };
        key_time_for_mode(
            mode,
            buf.life_ratio[idx],
            params.emitter_time_ratio,
            params.effect_time_ratio,
        )
    }
}

impl BehaviourImpl for AnimateTexture {
    fn order(&self) -> f32 {
        order::ANIMATE_TEXTURE
    }
    fn name(&self) -> &'static str {
        "ptxu_AnimateTexture"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let init_frame = if self.data.is_randomised {
            let r = params.random.value(buf.rand_index[idx] as u32);
            self.frame_min + ((self.frame_max - self.frame_min) as f32 * r) as u8
        } else {
            self.frame_min
        };
        buf.init_tex_frame[idx] = init_frame;
        buf.tex_frame_a[idx] = init_frame;
        buf.tex_frame_b[idx] = init_frame.saturating_add(1).min(self.frame_max);
        buf.tex_frame_ratio[idx] = 0.0;
        buf.uv_offset[idx] = Vec2::ZERO;
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let kfp_t = self.kfp_time(idx, buf, params);
        const ANIM_RATE_SHADER_VAR_HASH: u32 = 0x1A7E_9D0F;
        let rate_from_var = crate::kfp::eval_shader_var_keyframe(
            params.shader_vars,
            ANIM_RATE_SHADER_VAR_HASH,
            kfp_t,
        )
        .map(|v| v.x);
        let rate = rate_from_var
            .unwrap_or_else(|| {
                params
                    .query_kpd(
                        &self.data.anim_rate_keyframes,
                        kfp_t,
                        Vec4::splat(self.data.anim_rate_min),
                    )
                    .x
            })
            .max(0.0);
        let dt_frames = if self.data.is_scaled_over_particle_life {
            let frames_total = (self.frame_max as i32 - self.frame_min as i32 + 1).max(1) as f32;
            let pr = buf.playback_rate[idx].max(1e-6);
            frames_total * buf.one_over_life[idx] / pr * params.dt
        } else {
            rate * params.dt * buf.playback_rate[idx]
        };
        let mut ratio = buf.tex_frame_ratio[idx] + dt_frames;
        let frames_total = (self.frame_max as i32 - self.frame_min as i32 + 1).max(1);
        while ratio >= 1.0 {
            ratio -= 1.0;
            buf.tex_frame_a[idx] = buf.tex_frame_b[idx];
            let mut nb = (buf.tex_frame_b[idx] as i32) + 1;
            if nb > self.frame_max as i32 {
                let new_init: u8 = match self.data.loop_mode {
                    AnimTexLoopMode::ZERO => 0,
                    AnimTexLoopMode::MIN => self.frame_min,
                    AnimTexLoopMode::MAX => self.frame_max,
                    AnimTexLoopMode::RAND_MIN_MAX => {
                        let lo = self.frame_min as f32;
                        let hi = self.frame_max as f32;
                        let span = hi - lo;
                        if span <= 0.0 {
                            self.frame_min
                        } else {
                            let current = buf.tex_frame_a[idx];
                            let mut chosen = current;
                            for k in 0..6 {
                                let bias_seed = (buf.rand_index[idx] as u32)
                                    .wrapping_add(0x4E1_FA70)
                                    .wrapping_add(k);
                                let bias = params.random.value(bias_seed);
                                let cand = (lo + span * bias).round() as u8;
                                let cand = cand.clamp(self.frame_min, self.frame_max);
                                if cand != current {
                                    chosen = cand;
                                    break;
                                }
                            }
                            if chosen == current {
                                let next = (current + 1) % (self.frame_max + 1).max(1);
                                chosen = next.clamp(self.frame_min, self.frame_max);
                            }
                            chosen
                        }
                    }
                    _ => {
                        if self.data.is_held_on_last_frame {
                            ratio = 0.0;
                            self.frame_max
                        } else {
                            buf.init_tex_frame[idx]
                        }
                    }
                };
                buf.init_tex_frame[idx] = new_init;
                nb = new_init as i32;
            }
            buf.tex_frame_b[idx] = (nb.clamp(0, 255) as u8).min(self.frame_max);
            if frames_total == 1 {
                break;
            }
        }
        if !self.data.do_frame_blending {
            ratio = 0.0;
        }
        buf.tex_frame_ratio[idx] = ratio.clamp(0.0, 1.0);
        let u_rate = params
            .query_kpd(&self.data.uv_scroll_rate_u_keyframes, kfp_t, Vec4::ZERO)
            .x;
        let v_rate = params
            .query_kpd(&self.data.uv_scroll_rate_v_keyframes, kfp_t, Vec4::ZERO)
            .x;
        if u_rate.abs() > 0.0 || v_rate.abs() > 0.0 {
            let dt = params.dt * buf.playback_rate[idx];
            buf.uv_offset[idx] += Vec2::new(u_rate, v_rate) * dt;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point::PointFlags;
    use crate::random::RandomTable;
    use glam::{Mat4, Vec3};
    use or_io::decl::Gen8;
    use or_io::gta5::particle::RawKeyframeEntry;
    use or_io::gta5::particle::{PtxEffectRule, PtxParticleRule};
    use or_particle::keyframe::KeyframePropData;
    fn make_buf() -> PointBuffer {
        let mut buf = PointBuffer::default();
        let _ = buf.spawn(0);
        buf.life_ratio[0] = 0.0;
        buf.playback_rate[0] = 1.0;
        buf.flags[0].remove(PointFlags::IS_BEING_INITIALISED);
        buf
    }

    #[test]
    fn uv_scroll_offset_advances_per_tick() {
        let data = AnimateTextureBehaviour {
            anim_rate_min: 0.0,
            anim_rate_max: 0.0,
            uv_scroll_rate_u_keyframes: Some(KeyframePropData {
                entries: vec![RawKeyframeEntry {
                    time: [0.0; 4],
                    value: [0.5, 0.0, 0.0, 0.0],
                }],
                ..Default::default()
            }),
            uv_scroll_rate_v_keyframes: Some(KeyframePropData {
                entries: vec![RawKeyframeEntry {
                    time: [0.0; 4],
                    value: [-1.0, 0.0, 0.0, 0.0],
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let behaviour = AnimateTexture::from_data(data);
        let mut buf = make_buf();
        let random = RandomTable::default();
        let ptx_random_table = crate::random_table::PtxRandomTable::default();
        let rule = PtxEffectRule::<Gen8>::default();
        let particle_rule = PtxParticleRule::<Gen8>::default();
        let emitter_rule = or_io::gta5::particle::PtxEmitterRule::default();
        let evo: Vec<f32> = vec![0.0; 6];
        let params = BehaviourParams {
            dt: 0.5,
            effect_time_ratio: 0.0,
            emitter_time_ratio: 0.0,
            effect_zoom: 1.0,
            emitter_zoom: 1.0,
            effect_matrix: Mat4::IDENTITY,
            emitter_matrix: Mat4::IDENTITY,
            effect_velocity: Vec3::ZERO,
            emitter_velocity: Vec3::ZERO,
            gravity: 0.0,
            wind_velocity: Vec3::ZERO,
            random: &random,
            ptx_random_table: &ptx_random_table,
            evo_list: None,
            evo_values: &evo,
            particle_rule: &particle_rule,
            shader_vars: &[],
            effect_rule: &rule,
            emitter_rule: &emitter_rule,
            camera_pos: Vec3::ZERO,
            camera_basis: glam::Mat3::IDENTITY,
            emitter_colour_tint: glam::Vec4::ONE,
            emitter_attractor_pos: Vec3::ZERO,
            attractor_geometry: crate::behaviour::AttractorGeometry::default(),
            effect_colour_tint_override: glam::Vec4::ONE,
            effect_rgba_rand_index: 0,
            collision_bvh: None,
            river_field: None,
            wind_field: None,
            wall_time_seconds: 0.0,
            real_dt: 0.5,
            water_field: None,
            water_z_camera: 0.0,
            air_resistance: 1.0,
            lod_alpha_mult: 1.0,
            sinks: crate::sinks::PtfxSinks::none(),
            effect_spawner_trigger_ratio: -1.0,
        };
        behaviour.update_point(0, &mut buf, &params);
        behaviour.update_point(0, &mut buf, &params);
        let off = buf.uv_offset[0];
        assert!((off.x - 0.5).abs() < 1e-5, "u offset = {}", off.x);
        assert!((off.y + 1.0).abs() < 1e-5, "v offset = {}", off.y);
    }

    #[test]
    fn no_uv_scroll_kfps_leaves_offset_at_zero() {
        let data = AnimateTextureBehaviour::default();
        let behaviour = AnimateTexture::from_data(data);
        let mut buf = make_buf();
        let random = RandomTable::default();
        let ptx_random_table = crate::random_table::PtxRandomTable::default();
        let rule = PtxEffectRule::<Gen8>::default();
        let particle_rule = PtxParticleRule::<Gen8>::default();
        let emitter_rule = or_io::gta5::particle::PtxEmitterRule::default();
        let evo: Vec<f32> = vec![0.0; 6];
        let params = BehaviourParams {
            dt: 1.0,
            effect_time_ratio: 0.0,
            emitter_time_ratio: 0.0,
            effect_zoom: 1.0,
            emitter_zoom: 1.0,
            effect_matrix: Mat4::IDENTITY,
            emitter_matrix: Mat4::IDENTITY,
            effect_velocity: Vec3::ZERO,
            emitter_velocity: Vec3::ZERO,
            gravity: 0.0,
            wind_velocity: Vec3::ZERO,
            random: &random,
            ptx_random_table: &ptx_random_table,
            evo_list: None,
            evo_values: &evo,
            particle_rule: &particle_rule,
            shader_vars: &[],
            effect_rule: &rule,
            emitter_rule: &emitter_rule,
            camera_pos: Vec3::ZERO,
            camera_basis: glam::Mat3::IDENTITY,
            emitter_colour_tint: glam::Vec4::ONE,
            emitter_attractor_pos: Vec3::ZERO,
            attractor_geometry: crate::behaviour::AttractorGeometry::default(),
            effect_colour_tint_override: glam::Vec4::ONE,
            effect_rgba_rand_index: 0,
            collision_bvh: None,
            river_field: None,
            wind_field: None,
            wall_time_seconds: 0.0,
            real_dt: 1.0,
            water_field: None,
            water_z_camera: 0.0,
            air_resistance: 1.0,
            lod_alpha_mult: 1.0,
            sinks: crate::sinks::PtfxSinks::none(),
            effect_spawner_trigger_ratio: -1.0,
        };
        behaviour.update_point(0, &mut buf, &params);
        assert_eq!(buf.uv_offset[0], Vec2::ZERO);
    }
}
