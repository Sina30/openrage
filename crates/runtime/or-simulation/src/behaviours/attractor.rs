use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::kfp::bias_value_kpd;
use crate::point::{PointBuffer, PointFlags};
use glam::{Vec3, Vec4};
use or_particle::behaviour::AttractorBehaviour;
#[derive(Debug, Clone)]
pub struct Attractor {
    pub data: AttractorBehaviour,
}

impl Attractor {
    pub fn from_data(data: AttractorBehaviour) -> Self {
        Self { data }
    }
}

fn closest_point_on_segment(a: Vec3, b: Vec3, target: Vec3) -> Vec3 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq <= 1e-12 {
        return a;
    }
    let t = ((target - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}

impl BehaviourImpl for Attractor {
    fn order(&self) -> f32 {
        order::ATTRACTOR
    }
    fn name(&self) -> &'static str {
        "ptxu_Attractor"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if buf.flags[idx].contains(PointFlags::AT_REST) {
            return;
        }
        let strength_kfp = params.query_kpd(
            &self.data.strength_keyframes,
            buf.life_ratio[idx],
            Vec4::ZERO,
        );
        let bias = bias_value_kpd(
            params.random,
            &self.data.strength_keyframes,
            buf.rand_index[idx],
        );
        let mut strength_lerp = strength_kfp.x + (strength_kfp.y - strength_kfp.x) * bias;
        if strength_kfp == Vec4::ZERO {
            strength_lerp =
                self.data.strength_min + (self.data.strength_max - self.data.strength_min) * bias;
        }

        let dt = params.dt * buf.playback_rate[idx];
        let particle_pos = buf.curr_pos[idx];

        let attr = params.attractor_geometry;
        if attr.length <= 1e-4 || attr.radius_far <= 1e-4 {
            return;
        }

        let line_a = attr.pos + attr.dir * attr.length;
        let line_b = attr.pos - attr.dir * attr.length;
        let closest = closest_point_on_segment(line_a, line_b, particle_pos);

        let to_line = closest - particle_pos;
        let dist = to_line.length();
        let radius_far = if attr.radius_far > attr.radius_near {
            attr.radius_far
        } else {
            attr.radius_near + 1e-3
        };
        let radius_near = attr.radius_near;
        let denom = (radius_far - radius_near).max(1e-6);
        let mut falloff = ((dist - radius_near) / denom).clamp(0.0, 1.0);
        falloff = 1.0 - falloff;
        falloff *= falloff;
        let strength = falloff * strength_lerp;

        let to_line_norm = to_line.try_normalize().unwrap_or(Vec3::X);
        let tangent = to_line_norm
            .cross(attr.dir)
            .try_normalize()
            .unwrap_or(Vec3::Y);
        let vel_norm = buf.curr_vel[idx].try_normalize().unwrap_or(Vec3::X);
        let target = if vel_norm.dot(tangent) > 0.0 {
            closest - tangent * attr.radius_orbit
        } else {
            closest + tangent * attr.radius_orbit
        };
        let dir = (target - particle_pos)
            .try_normalize()
            .unwrap_or(Vec3::ZERO);
        buf.curr_vel[idx] += dir * (strength * dt);
    }
}
