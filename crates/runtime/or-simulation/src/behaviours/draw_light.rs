use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::PointBuffer;
use glam::Vec4;
use or_particle::behaviour::LightBehaviour;
#[derive(Debug, Clone)]
pub struct DrawLight {
    pub data: LightBehaviour,
}

impl DrawLight {
    pub fn from_data(data: LightBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for DrawLight {
    fn order(&self) -> f32 {
        order::DRAW_LIGHT
    }
    fn name(&self) -> &'static str {
        "ptxu_Light"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let life = buf.life_ratio[idx];
        let rand_idx = buf.rand_index[idx] as u32;
        let rgb_min = params.query_kpd(&self.data.rgb_min_keyframes, life, Vec4::ONE);
        let rgb_max = params.query_kpd(&self.data.rgb_max_keyframes, life, Vec4::ONE);
        let bias = params.random.value(rand_idx.wrapping_add(0x4117_0117));
        let rgb = Vec4::new(
            rgb_min.x + (rgb_max.x - rgb_min.x) * bias,
            rgb_min.y + (rgb_max.y - rgb_min.y) * bias,
            rgb_min.z + (rgb_max.z - rgb_min.z) * bias,
            1.0,
        );
        let intensity_v = params.query_kpd(&self.data.intensity_keyframes, life, Vec4::splat(1.0));
        let intensity = intensity_v.x + (intensity_v.y - intensity_v.x) * bias;
        let range_v = params.query_kpd(&self.data.range_keyframes, life, Vec4::splat(1.0));
        let range = range_v.x + (range_v.y - range_v.x) * bias;

        buf.color[idx] = rgb;
        buf.emissive_intensity[idx] = intensity;
        buf.dimensions[idx] = Vec4::splat(range);
        let _ = (
            self.data.corona_use_light_col,
            self.data.corona_z_bias,
            self.data.casts_shadows,
        );

        if let Some(sink) = params.sinks.light {
            // The intensity curve states the falloff exponent as its third and fourth numbers, the range curve the cone's angle
            let falloff = intensity_v.z + (intensity_v.w - intensity_v.z) * bias;
            let angle = range_v.z + (range_v.w - range_v.z) * bias;
            let (direction, cone_half_angle) = match self.data.light_type {
                0 => (glam::Vec3::ZERO, 0.0),
                kind => (spot_axis(kind, params), angle),
            };
            sink.add_light(crate::sinks::LightInfo {
                pos: buf.curr_pos[idx],
                colour: glam::Vec3::new(rgb.x, rgb.y, rgb.z),
                intensity,
                range,
                direction,
                cone_half_angle,
                falloff,
                corona_size: 0.0,
            });
        }
    }
}

/// The way a spot light of `kind` shines: the effect's X, Y or Z axis for one to three, the emitter's for four to six.
fn spot_axis(kind: u32, params: &BehaviourParams<'_>) -> glam::Vec3 {
    let (matrix, axis) = match kind {
        1..=3 => (params.effect_matrix, kind - 1),
        _ => (params.emitter_matrix, kind.saturating_sub(4).min(2)),
    };
    let column = match axis {
        0 => matrix.x_axis,
        1 => matrix.y_axis,
        _ => matrix.z_axis,
    };
    column.truncate().normalize_or_zero()
}

impl DrawLight {
    pub fn corona_data(&self, life_ratio: f32) -> ([f32; 3], f32, f32, f32, f32, bool) {
        let life = life_ratio.clamp(0.0, 1.0);
        let cmin = sample_kfp_rgb(&self.data.corona_rgb_min_keyframes, life, [1.0; 3]);
        let intensity = sample_kfp_scalar(&self.data.corona_intensity_keyframes, life, 0.0);
        let size = sample_kfp_scalar(&self.data.corona_size_keyframes, life, 0.0);
        let flare = sample_kfp_scalar(&self.data.corona_flare_keyframes, life, 0.0);
        (
            cmin,
            intensity,
            size,
            flare,
            self.data.corona_z_bias,
            self.data.casts_shadows,
        )
    }

    pub fn corona_use_light_col(&self) -> bool {
        self.data.corona_use_light_col
    }
}

fn sample_kfp_scalar(
    kfp: &Option<or_particle::keyframe::KeyframePropData>,
    life_ratio: f32,
    default: f32,
) -> f32 {
    crate::kfp::query_kpd(kfp, life_ratio, glam::Vec4::splat(default)).x
}

fn sample_kfp_rgb(
    kfp: &Option<or_particle::keyframe::KeyframePropData>,
    life_ratio: f32,
    default: [f32; 3],
) -> [f32; 3] {
    let v = crate::kfp::query_kpd(
        kfp,
        life_ratio,
        glam::Vec4::new(default[0], default[1], default[2], 1.0),
    );
    [v.x, v.y, v.z]
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta5::particle::RawKeyframeEntry;
    use or_particle::behaviour::LightBehaviour;
    use or_particle::keyframe::KeyframePropData;
    fn make_curve(values: &[(f32, [f32; 4])]) -> KeyframePropData {
        let mut entries = Vec::with_capacity(values.len());
        for i in 0..values.len() {
            let (t, v) = values[i];
            let dt_inv = if i > 0 {
                let prev_t = values[i - 1].0;
                if (t - prev_t).abs() > 1e-6 {
                    1.0 / (t - prev_t)
                } else {
                    0.0
                }
            } else {
                0.0
            };
            entries.push(RawKeyframeEntry {
                time: [t, dt_inv, 0.0, 0.0],
                value: v,
            });
        }
        KeyframePropData {
            entries,
            property_id: 0,
            rand_index: 0,
            invert_bias_link: 0,
        }
    }

    #[test]
    fn corona_data_samples_intensity_at_life_ratio() {
        let data = LightBehaviour {
            corona_intensity_keyframes: Some(make_curve(&[
                (0.0, [0.0; 4]),
                (1.0, [1.0, 0.0, 0.0, 0.0]),
            ])),
            ..Default::default()
        };
        let light = DrawLight::from_data(data);
        let (_rgb, mid_intensity, _size, _flare, _z_bias, _shadows) = light.corona_data(0.5);
        assert!(
            (mid_intensity - 0.5).abs() < 1e-3,
            "expected ~0.5 at mid-life, got {mid_intensity}"
        );
        let (_rgb, end_intensity, _, _, _, _) = light.corona_data(1.0);
        assert!(
            (end_intensity - 1.0).abs() < 1e-3,
            "expected ~1.0 at end-of-life, got {end_intensity}"
        );
        let (_rgb, start_intensity, _, _, _, _) = light.corona_data(0.0);
        assert!(
            start_intensity.abs() < 1e-3,
            "expected ~0.0 at spawn, got {start_intensity}"
        );
    }

    #[test]
    fn corona_data_falls_back_to_defaults_when_kfp_absent() {
        let light = DrawLight::from_data(LightBehaviour::default());
        let (rgb, intensity, size, flare, _z, _shadows) = light.corona_data(0.5);
        assert_eq!(rgb, [1.0, 1.0, 1.0]);
        assert_eq!(intensity, 0.0);
        assert_eq!(size, 0.0);
        assert_eq!(flare, 0.0);
    }
}
