use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::collision::Hit;
use crate::point::{PointBuffer, PointFlags};
use glam::{Vec3, Vec4};
use or_particle::behaviour::CollisionBehaviour;
fn apply_bounce_variance(
    reflected: Vec3,
    coln_normal: Vec3,
    rotation_var: f32,
    elevation_var: f32,
    phi_rand: f32,
    theta_rand: f32,
) -> Vec3 {
    use std::f32::consts::PI;
    let mag = reflected.length();
    if mag < 1e-6 {
        return reflected;
    }
    let r_norm = reflected / mag;

    let v_rand = Vec3::new(-0.353, 0.712, 0.607);
    let col2 = coln_normal.normalize_or_zero();
    let col1 = col2.cross(v_rand).normalize_or_zero();
    let col0 = col1.cross(col2).normalize_or_zero();
    if col1.length_squared() < 1e-10 || col0.length_squared() < 1e-10 {
        return reflected;
    }

    let local_r = Vec3::new(r_norm.dot(col0), r_norm.dot(col1), r_norm.dot(col2));

    let theta_cos = local_r.z.clamp(-1.0, 1.0);
    let phi_sin = local_r.y.clamp(-1.0, 1.0);
    let phi_cos = local_r.x.clamp(-1.0, 1.0);
    let theta = theta_cos.acos();
    let phi = phi_sin.atan2(phi_cos);

    let phi_rand_v = (phi_rand * 2.0 - 1.0) * PI;
    let theta_max = PI * 0.5 - 0.01;
    let theta_rand_v = theta_rand.clamp(0.0, 1.0) * theta_max;
    let new_phi = phi + (phi_rand_v - phi) * rotation_var.clamp(0.0, 1.0);
    let new_theta = theta + (theta_rand_v - theta) * elevation_var.clamp(0.0, 1.0);

    let (sin_t, cos_t) = new_theta.sin_cos();
    let (sin_p, cos_p) = new_phi.sin_cos();
    let new_local = Vec3::new(sin_t * cos_p, sin_t * sin_p, cos_t) * mag;

    col0 * new_local.x + col1 * new_local.y + col2 * new_local.z
}

#[derive(Debug, Clone)]
pub struct Collision {
    pub data: CollisionBehaviour,
}

impl Collision {
    pub fn from_data(data: CollisionBehaviour) -> Self {
        Self { data }
    }

    fn rolls_collides(&self, rand_index: u8, params: &BehaviourParams<'_>) -> bool {
        let chance = self.data.collision_chance.clamp(0, 100);
        if chance >= 100 {
            return true;
        }
        if chance == 0 {
            return false;
        }
        let r = params.ptx_random_table.value(rand_index);
        r * 100.0 <= chance as f32
    }

    fn apply_hit(
        &self,
        idx: usize,
        buf: &mut PointBuffer,
        params: &BehaviourParams<'_>,
        hit_pos: Vec3,
        hit_normal: Vec3,
        mtl_id: u8,
    ) {
        let v_in = buf.curr_vel[idx];
        buf.coln_pos[idx] = hit_pos;
        buf.coln_normal[idx] = hit_normal;
        buf.coln_speed[idx] = v_in.length();
        buf.coln_mtl_id[idx] = mtl_id;
        buf.flags[idx].insert(PointFlags::HAS_COLLIDED);

        let bounce_kfp = params.query_kpd(
            &self.data.bounciness_keyframes,
            buf.life_ratio[idx],
            Vec4::ZERO,
        );
        let bounce_bias = params.random.value(buf.rand_index[idx] as u32 + 0xB0_BB_1E);
        let bounce_lerp = bounce_kfp.x + (bounce_kfp.y - bounce_kfp.x) * bounce_bias;
        let bounciness = (bounce_lerp * 0.01).clamp(0.0, 1.0);
        let bdv_kfp = params.query_kpd(
            &self.data.bounce_dir_var_keyframes,
            buf.life_ratio[idx],
            Vec4::ZERO,
        );
        let bdv_bias = params.random.value(buf.rand_index[idx] as u32 + 0xB0_BD_1F);
        let rot_var = (bdv_kfp.x + (bdv_kfp.y - bdv_kfp.x) * bdv_bias).clamp(0.0, 1.0);
        let elev_var = (bdv_kfp.z + (bdv_kfp.w - bdv_kfp.z) * bdv_bias).clamp(0.0, 1.0);

        if v_in != Vec3::ZERO {
            let r = v_in - 2.0 * hit_normal * v_in.dot(hit_normal);
            let r_seed = buf.rand_index[idx] as u32;
            let phi_rand = params.random.value(r_seed.wrapping_add(0x00C0_DE15));
            let theta_rand = params.random.value(r_seed.wrapping_add(0x00C0_DE16));
            let r = apply_bounce_variance(r, hit_normal, rot_var, elev_var, phi_rand, theta_rand);
            buf.curr_vel[idx] = r * bounciness;
        }

        let rest_speed = self.data.rest_speed.max(0.0);
        if buf.curr_vel[idx].length_squared() <= rest_speed * rest_speed {
            buf.curr_vel[idx] = Vec3::ZERO;
            buf.flags[idx].insert(PointFlags::AT_REST);
        }

        let penetration = (buf.curr_pos[idx] - hit_pos).dot(hit_normal).abs();
        buf.curr_pos[idx] = hit_pos + hit_normal * (penetration + 1e-4);

        let kill = (self.data.kill_chance as f32) * 0.01;
        if kill > 0.0 {
            let kill_roll = params
                .random
                .value(buf.rand_index[idx] as u32 + 0xDEAD_BEEF);
            if kill_roll < kill {
                buf.is_dead[idx] = true;
            }
        }
    }
}

impl BehaviourImpl for Collision {
    fn order(&self) -> f32 {
        order::COLLISION
    }
    fn name(&self) -> &'static str {
        "ptxu_Collision"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if self.rolls_collides(buf.rand_index[idx], params) {
            buf.flags[idx].insert(PointFlags::COLLIDES);
        }
        self.update_point(idx, buf, params);
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if !buf.flags[idx].contains(PointFlags::COLLIDES) {
            return;
        }
        if buf.flags[idx].contains(PointFlags::AT_REST) {
            return;
        }

        if let Some(bvh) = params.collision_bvh {
            let particle_r = buf.dimensions[idx].w.abs();
            let radius_mult = if self.data.radius_mult > 1e-5 {
                self.data.radius_mult
            } else {
                1.0
            };
            let scaled = particle_r * radius_mult;
            let radius_pad = scaled.max(self.data.override_min_radius).max(0.01);
            let prev = buf.prev_pos[idx];
            let curr = buf.curr_pos[idx];
            let delta = curr - prev;
            let dist = delta.length();
            if dist <= 1e-6 {
                return;
            }
            let dir = delta / dist;
            let too_fast = dist > 2.0 * radius_pad;
            if too_fast {
                let step = (2.0 * radius_pad).max(0.01);
                let nsteps = ((dist / step).ceil() as usize).max(1);
                let sub_step = dist / nsteps as f32;
                for s in 0..nsteps {
                    let from = prev + dir * (s as f32 * sub_step);
                    if let Some(Hit {
                        t: _,
                        pos,
                        normal,
                        mtl_id,
                    }) = bvh.ray_cast(from, dir, sub_step + radius_pad)
                    {
                        self.apply_hit(idx, buf, params, pos, normal, mtl_id);
                        return;
                    }
                }
                return;
            }
            let max_t = dist + radius_pad;
            if let Some(Hit {
                t: _,
                pos,
                normal,
                mtl_id,
            }) = bvh.ray_cast(prev, dir, max_t)
            {
                self.apply_hit(idx, buf, params, pos, normal, mtl_id);
                return;
            }
            return;
        }

        if buf.curr_pos[idx].z >= 0.0 {
            return;
        }
        let hit_pos = Vec3::new(buf.curr_pos[idx].x, buf.curr_pos[idx].y, 0.0);
        let hit_normal = Vec3::Z;
        let mtl_id = 0;
        self.apply_hit(idx, buf, params, hit_pos, hit_normal, mtl_id);
    }
}
