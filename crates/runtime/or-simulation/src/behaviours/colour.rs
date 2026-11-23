use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::kfp::{bias_value_kpd, key_time_for_mode, KeyframeMode};
use crate::point::PointBuffer;
use glam::Vec4;
use or_particle::behaviour::ColourBehaviour;
#[derive(Debug, Clone)]
pub struct Colour {
    pub data: ColourBehaviour,
}

impl Colour {
    pub fn from_data(data: ColourBehaviour) -> Self {
        Self { data }
    }

    fn mode(&self) -> KeyframeMode {
        match self.data.keyframe_mode {
            or_io::gta5::particle::KeyframeMode::Point => KeyframeMode::Point,
            or_io::gta5::particle::KeyframeMode::Effect => KeyframeMode::Effect,
            or_io::gta5::particle::KeyframeMode::Emitter => KeyframeMode::Emitter,
        }
    }
}

impl BehaviourImpl for Colour {
    fn order(&self) -> f32 {
        order::COLOUR
    }
    fn name(&self) -> &'static str {
        "ptxu_Colour"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        self.update_point(idx, buf, params);
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let t = key_time_for_mode(
            self.mode(),
            buf.life_ratio[idx],
            params.emitter_time_ratio,
            params.effect_time_ratio,
        );
        let rand_idx = buf.rand_index[idx];

        let default_rgba = Vec4::new(
            self.data.rgba_min.r,
            self.data.rgba_min.g,
            self.data.rgba_min.b,
            self.data.rgba_min.a,
        );
        let rgba_min = params.query_kpd(&self.data.rgba_min_keyframes, t, default_rgba);
        let rgba_max = params.query_kpd(
            &self.data.rgba_max_keyframes,
            t,
            Vec4::new(
                self.data.rgba_max.r,
                self.data.rgba_max.g,
                self.data.rgba_max.b,
                self.data.rgba_max.a,
            ),
        );

        let mut rgba = rgba_min;
        if self.data.rgba_max_enable {
            if self.data.rgba_proportional {
                let bias = bias_value_kpd(params.random, &self.data.rgba_min_keyframes, rand_idx);
                rgba = rgba_min.lerp(rgba_max, bias);
            } else {
                let br = bias_value_kpd(params.random, &self.data.rgba_min_keyframes, rand_idx);
                let bg = bias_value_kpd(
                    params.random,
                    &self.data.rgba_min_keyframes,
                    rand_idx.wrapping_add(1),
                );
                let bb = bias_value_kpd(
                    params.random,
                    &self.data.rgba_min_keyframes,
                    rand_idx.wrapping_add(2),
                );
                let ba = bias_value_kpd(
                    params.random,
                    &self.data.rgba_min_keyframes,
                    rand_idx.wrapping_add(3),
                );
                rgba = Vec4::new(
                    rgba_min.x + (rgba_max.x - rgba_min.x) * br,
                    rgba_min.y + (rgba_max.y - rgba_min.y) * bg,
                    rgba_min.z + (rgba_max.z - rgba_min.z) * bb,
                    rgba_min.w + (rgba_max.w - rgba_min.w) * ba,
                );
            }
        }

        let emissive = params.query_kpd(
            &self.data.emissive_intensity_keyframes,
            t,
            Vec4::new(
                self.data.emissive_intensity_min,
                self.data.emissive_intensity_max,
                0.0,
                0.0,
            ),
        );
        let bias_em = bias_value_kpd(
            params.random,
            &self.data.emissive_intensity_keyframes,
            rand_idx,
        );
        let emissive_v = emissive.x + (emissive.y - emissive.x) * bias_em;

        let effect_min = crate::kfp::query_kfp_with_evo(
            &params.effect_rule.colour_tint_min_kfp,
            params.effect_time_ratio,
            params.evo_list,
            params.evo_values,
        );
        let effect_max = crate::kfp::query_kfp_with_evo(
            &params.effect_rule.colour_tint_max_kfp,
            params.effect_time_ratio,
            params.evo_list,
            params.evo_values,
        );
        let effect_min_or_white = if effect_min == Vec4::ZERO {
            Vec4::ONE
        } else {
            effect_min
        };
        let effect_max_or_white = if effect_max == Vec4::ZERO {
            Vec4::ONE
        } else {
            effect_max
        };
        let mut effect_tint = if params.effect_rule.colour_tint_max_enable.get() {
            let bias = params.ptx_random_table.value(params.effect_rgba_rand_index);
            effect_min_or_white.lerp(effect_max_or_white, bias)
        } else {
            effect_min_or_white
        };
        effect_tint *= params.effect_colour_tint_override;
        let apply_tint = |rgba: Vec4, tint: Vec4| -> Vec4 {
            if self.data.rgb_can_tint {
                Vec4::new(rgba.x * tint.x, rgba.y * tint.y, rgba.z * tint.z, rgba.w)
            } else {
                rgba * tint
            }
        };
        rgba = apply_tint(rgba, effect_tint);
        rgba = apply_tint(rgba, params.emitter_colour_tint);

        rgba.w *= params.lod_alpha_mult * buf.alpha_mult[idx];
        buf.alpha_mult[idx] = 1.0;

        buf.color[idx] = rgba;
        buf.emissive_intensity[idx] = emissive_v;
    }
}
