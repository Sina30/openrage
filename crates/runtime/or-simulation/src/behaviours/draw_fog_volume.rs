use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::PointBuffer;
use crate::sinks::{FogLighting, FogVolumeInfo};
use glam::{Vec3, Vec4};
use or_particle::behaviour::FogVolumeBehaviour;
#[derive(Debug, Clone)]
pub struct DrawFogVolume {
    pub data: FogVolumeBehaviour,
}

impl DrawFogVolume {
    pub fn from_data(data: FogVolumeBehaviour) -> Self {
        Self { data }
    }

    /// The volume the rule gives the point at `idx`, or none where it has no density, no range, no tint or no scale.
    fn volume(
        &self,
        idx: usize,
        buf: &PointBuffer,
        params: &BehaviourParams<'_>,
    ) -> Option<FogVolumeInfo> {
        let (life, data) = (buf.life_ratio[idx], &self.data);
        let rand_idx = buf.rand_index[idx] as u32;
        let bias = |salt: u32| params.random.value(rand_idx.wrapping_add(salt));
        let (density_bias, mix_bias, rotation_bias) =
            (bias(0xF06F_0516), bias(0xF06F_0518), bias(0xF06F_0519));

        // The density and the range are mixed by the one bias, the tint and the scale by another
        let density_range = params.query_kpd(
            &data.density_range_keyframes,
            life,
            Vec4::new(1.0, 1.0, 3.0, 3.0),
        );
        let density = density_range.x + (density_range.y - density_range.x) * density_bias;
        let range = density_range.z + (density_range.w - density_range.z) * density_bias;
        let tint_min = params.query_kpd(&data.rgb_tint_min_keyframes, life, Vec4::ONE);
        let tint_max = params.query_kpd(&data.rgb_tint_max_keyframes, life, Vec4::ONE);
        let mut tint = tint_min.lerp(tint_max, mix_bias);
        if data.colour_tint_from_particle {
            tint *= buf.color[idx];
        }
        let scale_min = params.query_kpd(&data.scale_min_keyframes, life, Vec4::ONE);
        let scale_max = params.query_kpd(&data.scale_max_keyframes, life, Vec4::ONE);
        let scale = scale_min.lerp(scale_max, mix_bias).truncate();
        let rotation_min = params.query_kpd(&data.rotation_min_keyframes, life, Vec4::ZERO);
        let rotation_max = params.query_kpd(&data.rotation_max_keyframes, life, Vec4::ZERO);
        let degrees = rotation_min.lerp(rotation_max, rotation_bias).truncate();

        (density != 0.0 && range != 0.0 && tint != Vec4::ZERO && scale != Vec3::ZERO).then(|| {
            FogVolumeInfo {
                pos: buf.curr_pos[idx],
                scale,
                rotation: degrees * (std::f32::consts::PI / 180.0),
                range,
                colour: tint,
                density: density * params.effect_colour_tint_override.w * params.lod_alpha_mult,
                falloff: data.falloff,
                hdr_mult: data.hdr_mult,
                lighting: FogLighting::of(data.lighting_type),
                use_ground_fog_colour: data.use_ground_fog_colour,
                sort_with_particles: data.sort_with_particles,
            }
        })
    }
}

impl BehaviourImpl for DrawFogVolume {
    fn order(&self) -> f32 {
        order::DRAW_FOG_VOLUME
    }
    fn name(&self) -> &'static str {
        "ptxu_FogVolume"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        // Before the colour is overwritten: the particle's own tints the volume where the rule asks
        let volume = self.volume(idx, buf, params);
        let life = buf.life_ratio[idx];
        let rand_idx = buf.rand_index[idx] as u32;
        let tint_bias = params.random.value(rand_idx.wrapping_add(0xF06F_0517));
        let tint_min = params.query_kpd(&self.data.rgb_tint_min_keyframes, life, Vec4::ONE);
        let tint_max = params.query_kpd(&self.data.rgb_tint_max_keyframes, life, Vec4::ONE);
        let tint = Vec4::new(
            tint_min.x + (tint_max.x - tint_min.x) * tint_bias,
            tint_min.y + (tint_max.y - tint_min.y) * tint_bias,
            tint_min.z + (tint_max.z - tint_min.z) * tint_bias,
            1.0,
        );
        let scale_bias = params.random.value(rand_idx.wrapping_add(0xF06F_0518));
        let scale_min = params.query_kpd(&self.data.scale_min_keyframes, life, Vec4::ONE);
        let scale_max = params.query_kpd(&self.data.scale_max_keyframes, life, Vec4::ONE);
        let scale = scale_min.lerp(scale_max, scale_bias);
        let density_range = params.query_kpd(
            &self.data.density_range_keyframes,
            life,
            Vec4::new(0.4, 1.0, 0.0, 0.0),
        );
        let density = density_range.x.clamp(0.0, 1.0);
        buf.color[idx] = Vec4::new(tint.x, tint.y, tint.z, density);
        buf.dimensions[idx] = scale;

        if let (Some(sink), Some(volume)) = (params.sinks.fog_volume, volume) {
            sink.add_fog_volume(volume);
        }
    }
}
