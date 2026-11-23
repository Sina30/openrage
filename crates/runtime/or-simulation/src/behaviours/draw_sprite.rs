use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::{PointBuffer, PointFlags};
use or_io::gta5::particle::AlignmentMode;
use or_particle::behaviour::DrawSpriteBehaviour;
#[derive(Debug, Clone)]
pub struct DrawSprite {
    pub data: DrawSpriteBehaviour,
}

impl DrawSprite {
    pub fn from_data(data: DrawSpriteBehaviour) -> Self {
        Self { data }
    }

    pub fn alignment(&self) -> AlignmentMode {
        self.data.alignment_mode
    }
}

impl BehaviourImpl for DrawSprite {
    fn order(&self) -> f32 {
        order::DRAW_SPRITE
    }
    fn name(&self) -> &'static str {
        "ptxd_Sprite"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        buf.flags[idx].insert(PointFlags::IS_SPRITE);
        let rand_idx = buf.rand_index[idx];
        if self.data.flip_chance_u > 0.0 {
            let r = params.random.value(rand_idx as u32 + 0x11);
            if r < self.data.flip_chance_u {
                buf.flags[idx].insert(PointFlags::FLIP_U);
            }
        }
        if self.data.flip_chance_v > 0.0 {
            let r = params.random.value(rand_idx as u32 + 0x22);
            if r < self.data.flip_chance_v {
                buf.flags[idx].insert(PointFlags::FLIP_V);
            }
        }
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if (self.data.near_clip || self.data.far_clip)
            && self.data.alignment_mode != AlignmentMode::Camera
        {
            let dim = buf.dimensions[idx];
            let half_ext = dim
                .x
                .abs()
                .max(dim.y.abs())
                .max(dim.z.abs())
                .max(dim.w.abs())
                .max(1e-3);
            let to_curr = buf.curr_pos[idx] - buf.spawn_pos[idx];
            let raw_dist = to_curr.length();
            let signed_dist = {
                let vel = buf.curr_vel[idx];
                if vel.length_squared() > 1e-6 {
                    let up_axis = vel.normalize_or_zero();
                    if to_curr.dot(up_axis) < 0.0 {
                        -raw_dist
                    } else {
                        raw_dist
                    }
                } else {
                    raw_dist
                }
            };
            let mut alpha_clip = 1.0_f32;
            if self.data.near_clip {
                let near = self.data.near_clip_dist;
                if signed_dist < near - half_ext {
                    alpha_clip = 0.0;
                } else if signed_dist < near + half_ext {
                    alpha_clip =
                        ((signed_dist - (near - half_ext)) / (2.0 * half_ext)).clamp(0.0, 1.0);
                }
            }
            if self.data.far_clip {
                let far = self.data.far_clip_dist;
                if signed_dist > far + half_ext {
                    alpha_clip = 0.0;
                } else if signed_dist > far - half_ext {
                    let t = ((signed_dist - (far - half_ext)) / (2.0 * half_ext)).clamp(0.0, 1.0);
                    alpha_clip = alpha_clip.min(1.0 - t);
                }
            }
            buf.alpha_mult[idx] *= alpha_clip;
            buf.flags[idx].set(PointFlags::CLIPPED_BY_DISTANCE, alpha_clip <= 0.0);
        } else {
            buf.flags[idx].remove(PointFlags::CLIPPED_BY_DISTANCE);
        }
        let _ = params;
    }
}
