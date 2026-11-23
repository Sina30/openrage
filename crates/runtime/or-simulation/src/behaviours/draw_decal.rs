use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::PointBuffer;
use glam::Vec4;
use or_particle::behaviour::DecalBehaviour;
#[derive(Debug, Clone)]
pub struct DrawDecal {
    pub data: DecalBehaviour,
}

impl DrawDecal {
    pub fn from_data(data: DecalBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for DrawDecal {
    fn order(&self) -> f32 {
        order::DRAW_DECAL
    }
    fn name(&self) -> &'static str {
        "ptxu_Decal"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let dims = params.query_kpd(
            &self.data.dimensions_keyframes,
            buf.life_ratio[idx],
            Vec4::ONE,
        );
        let alpha = params
            .query_kpd(&self.data.alpha_keyframes, buf.life_ratio[idx], Vec4::ONE)
            .x;
        buf.dimensions[idx] = dims;
        buf.alpha_mult[idx] *= alpha;

        if let Some(sink) = params.sinks.decal {
            use crate::point::PointFlags;
            if !buf.flags[idx].contains(PointFlags::IS_BEING_INITIALISED) {
                let normal = if buf.curr_vel[idx].length_squared() > 1e-6 {
                    buf.curr_vel[idx].normalize_or_zero()
                } else {
                    glam::Vec3::Z
                };
                sink.add_decal(crate::sinks::DecalInfo {
                    pos: buf.curr_pos[idx],
                    normal,
                    size: glam::Vec3::new(dims.x, dims.y, dims.z),
                    uv_mult: Vec4::ONE,
                    colour: Vec4::new(
                        buf.color[idx].x,
                        buf.color[idx].y,
                        buf.color[idx].z,
                        buf.color[idx].w * alpha,
                    ),
                    life_seconds: 1.0 / buf.one_over_life[idx].max(1e-6),
                    fade_in_seconds: 0.05,
                    fade_out_seconds: 0.25,
                    priority: 128,
                    disallow_duplicates: false,
                    material_id: buf.coln_mtl_id[idx],
                });
            }
        }
    }
}
