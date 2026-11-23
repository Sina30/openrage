use crate::behaviour::{order, BehaviourImpl, BehaviourParams, DrawKind};
use crate::kfp::bias_value_kpd;
use crate::point::PointBuffer;
use glam::{Quat, Vec3, Vec4};
use or_particle::behaviour::{RotationBehaviour, RotationSpeedBehaviour};

// Sprites/trails store spin radians in quat.x (not a real quaternion); models use full orientation.
fn rotation_quat(draw_kind: DrawKind, a: Vec3, rotate_init_angle_axes: bool) -> Quat {
    match draw_kind {
        DrawKind::Sprite | DrawKind::Trail => Quat::from_xyzw(a.x, 0.0, 0.0, 0.0),
        DrawKind::Model | DrawKind::Decal => {
            if rotate_init_angle_axes {
                Quat::from_axis_angle(Vec3::X, a.x)
                    * Quat::from_axis_angle(Vec3::Y, a.y)
                    * Quat::from_axis_angle(Vec3::Z, a.z)
            } else {
                Quat::from_euler(glam::EulerRot::XYZ, a.x, a.y, a.z)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rotation {
    pub data: RotationBehaviour,
}

impl Rotation {
    pub fn from_data(data: RotationBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for Rotation {
    fn order(&self) -> f32 {
        order::ROTATION
    }
    fn name(&self) -> &'static str {
        "ptxu_Rotation"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let bias = bias_value_kpd(
            params.random,
            &self.data.initial_angle_min_keyframes,
            buf.rand_index[idx],
        );
        let key = params.emitter_time_ratio;
        let amin = params.query_kpd(&self.data.initial_angle_min_keyframes, key, Vec4::ZERO);
        let amax = params.query_kpd(&self.data.initial_angle_max_keyframes, key, Vec4::ZERO);
        let a = amin.lerp(amax, bias) * std::f32::consts::PI / 180.0;
        let draw_kind = DrawKind::from_raw(params.particle_rule.draw_type);
        let q = rotation_quat(
            draw_kind,
            Vec3::new(a.x, a.y, a.z),
            self.data.rotate_init_angle_axes,
        );
        buf.init_rotation[idx] = q;
        buf.curr_rot[idx] = q;
        buf.curr_angle[idx] = Vec3::new(a.x, a.y, a.z);
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let t = buf.life_ratio[idx];
        let bias = bias_value_kpd(
            params.random,
            &self.data.angle_min_keyframes,
            buf.rand_index[idx],
        );
        let amin = params.query_kpd(&self.data.angle_min_keyframes, t, Vec4::ZERO);
        let amax = params.query_kpd(&self.data.angle_max_keyframes, t, Vec4::ZERO);
        let a = amin.lerp(amax, bias) * std::f32::consts::PI / 180.0;
        let dt = params.dt * buf.playback_rate[idx];
        let speed_fade = if self.data.speed_fade_thresh > 1e-5 {
            let speed = buf.curr_vel[idx].length();
            if speed < self.data.speed_fade_thresh {
                1.0
            } else {
                speed / self.data.speed_fade_thresh
            }
        } else {
            1.0
        };
        if self.data.accumulate_angle {
            buf.curr_angle[idx] += Vec3::new(a.x, a.y, a.z) * dt * speed_fade;
        } else {
            buf.curr_angle[idx] = Vec3::new(a.x, a.y, a.z);
        }
        let draw_kind = DrawKind::from_raw(params.particle_rule.draw_type);
        let curr_a = buf.curr_angle[idx];
        let combined = match draw_kind {
            DrawKind::Sprite | DrawKind::Trail => {
                let init_x = buf.init_rotation[idx].x;
                Quat::from_xyzw(init_x + curr_a.x, 0.0, 0.0, 0.0)
            }
            DrawKind::Model | DrawKind::Decal => {
                let delta = rotation_quat(draw_kind, curr_a, self.data.rotate_angle_axes);
                (buf.init_rotation[idx] * delta).normalize()
            }
        };
        buf.curr_rot[idx] = combined;
    }
}

#[derive(Debug, Clone)]
pub struct RotationSpeed {
    pub data: RotationSpeedBehaviour,
}

impl RotationSpeed {
    pub fn from_data(data: RotationSpeedBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for RotationSpeed {
    fn order(&self) -> f32 {
        order::ROTATION_SPEED
    }
    fn name(&self) -> &'static str {
        "ptxu_RotationRate"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let bias = bias_value_kpd(params.random, &self.data.keyframes, buf.rand_index[idx]);
        let speed_kfp = params.query_kpd(&self.data.keyframes, buf.life_ratio[idx], Vec4::ZERO);
        let speed_min = if speed_kfp.x != 0.0 {
            speed_kfp.x
        } else {
            self.data.speed_min
        };
        let speed_max = if speed_kfp.y != 0.0 {
            speed_kfp.y
        } else {
            self.data.speed_max
        };
        let speed = speed_min + (speed_max - speed_min) * bias;
        let dt = params.dt * buf.playback_rate[idx];
        let draw_kind = DrawKind::from_raw(params.particle_rule.draw_type);
        match draw_kind {
            DrawKind::Sprite | DrawKind::Trail => {
                let curr = buf.curr_rot[idx];
                buf.curr_rot[idx] = Quat::from_xyzw(curr.x + speed * dt, 0.0, 0.0, 0.0);
            }
            DrawKind::Model | DrawKind::Decal => {
                let axis = Vec3::new(self.data.axis.x, self.data.axis.y, self.data.axis.z)
                    .try_normalize()
                    .unwrap_or(Vec3::Z);
                let dq = Quat::from_axis_angle(axis, speed * dt);
                buf.curr_rot[idx] = (dq * buf.curr_rot[idx]).normalize();
            }
        }
    }
}
