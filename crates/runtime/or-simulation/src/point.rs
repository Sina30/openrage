use bytemuck::{Pod, Zeroable};
use glam::{Quat, Vec2, Vec3, Vec4};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PointFlags(u32);
impl PointFlags {
    pub const IS_SPRITE: Self = Self(1 << 0);
    pub const IS_MODEL: Self = Self(1 << 1);
    pub const IS_TRAIL: Self = Self(1 << 2);
    pub const COLLIDES: Self = Self(1 << 3);
    pub const HAS_COLLIDED: Self = Self(1 << 4);
    pub const AT_REST: Self = Self(1 << 5);
    pub const SPAWN_EFFECT: Self = Self(1 << 6);
    pub const LOOPED: Self = Self(1 << 7);
    pub const IS_BEING_INITIALISED: Self = Self(1 << 8);
    pub const LIFE_PAUSED: Self = Self(1 << 9);
    pub const LIFE_UNPAUSED: Self = Self(1 << 10);
    pub const FLIP_U: Self = Self(1 << 11);
    pub const FLIP_V: Self = Self(1 << 12);
    pub const CLIPPED_BY_DISTANCE: Self = Self(1 << 13);
    /// A `ptxu_ZCull` culled the point this step.
    pub const ZCULLED: Self = Self(1 << 14);
    pub const fn empty() -> Self {
        Self(0)
    }
    #[inline]
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
    #[inline]
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
    #[inline]
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
    #[inline]
    pub fn set(&mut self, other: Self, on: bool) {
        if on {
            self.insert(other);
        } else {
            self.remove(other);
        }
    }
}

impl std::ops::BitOr for PointFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct GpuParticleInstance {
    pub position: [f32; 3],
    pub life_ratio: f32,
    pub dimensions: [f32; 4],
    pub rotation: [f32; 4], // quaternion xyzw
    pub color: [f32; 4],
    pub velocity: [f32; 3],
    pub _pad0: f32,
    pub tex_frame_a: f32,
    pub tex_frame_b: f32,
    pub tex_blend: f32,
    pub _pad1: f32,
    pub uv_offset: [f32; 2],
    pub _pad2: [f32; 2],
    pub custom1: [f32; 4],
    pub custom2: [f32; 4],
}

#[derive(Debug, Default, Clone)]
pub struct PointBuffer {
    pub init_rotation: Vec<Quat>,
    pub coln_pos: Vec<Vec3>,
    pub coln_speed: Vec<f32>,
    pub coln_normal: Vec<Vec3>,
    pub pause_timer: Vec<f32>,
    pub one_over_life: Vec<f32>,
    pub emitter_life_ratio: Vec<f32>,
    pub playback_rate: Vec<f32>,
    pub sort_val: Vec<f32>,
    pub flags: Vec<PointFlags>,
    pub coln_mtl_id: Vec<u8>,
    pub rand_index: Vec<u8>,
    pub prev_pos: Vec<Vec3>,
    pub curr_pos: Vec<Vec3>,
    pub spawn_pos: Vec<Vec3>,
    pub matrix_weight: Vec<f32>,
    pub curr_vel: Vec<Vec3>,
    pub tex_frame_ratio: Vec<f32>,
    pub dimensions: Vec<Vec4>,
    pub curr_rot: Vec<Quat>,
    pub curr_angle: Vec<Vec3>,
    pub color: Vec<Vec4>,
    pub emissive_intensity: Vec<f32>,
    pub tex_frame_a: Vec<u8>,
    pub tex_frame_b: Vec<u8>,
    pub init_tex_frame: Vec<u8>,
    pub alpha_mult: Vec<f32>,
    pub is_dead: Vec<bool>,
    pub is_broken_trail: Vec<bool>,
    pub life_ratio: Vec<f32>,
    pub uv_offset: Vec<Vec2>,
    pub evo_values: Vec<f32>,
    pub velocity_noise_extra: Vec<Vec4>,
    pub age_atmos_extra: Vec<Vec4>,
    pub colour_tint_extra: Vec<Vec4>,
    pub collision_bounce_extra: Vec<Vec4>,
    pub attractor_orbit_extra: Vec<Vec4>,
    pub prev_in_chain: Vec<Option<usize>>,
    pub next_in_chain: Vec<Option<usize>>,
    // Spawn-order doubly-linked list (trail renderer walks this; sweep_dead preserves it).
    pub chain_head: Option<usize>,
    pub chain_tail: Option<usize>,
}

impl PointBuffer {
    pub fn len(&self) -> usize {
        self.curr_pos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        self.curr_pos.capacity()
    }

    pub fn shadow_sort_distances(&self, shadow_cam_pos: glam::Vec3) -> Vec<f32> {
        self.curr_pos
            .iter()
            .map(|p| (*p - shadow_cam_pos).length_squared())
            .collect()
    }

    pub fn reserve(&mut self, n: usize) {
        self.prev_in_chain.reserve(n);
        self.next_in_chain.reserve(n);
        self.init_rotation.reserve(n);
        self.coln_pos.reserve(n);
        self.coln_speed.reserve(n);
        self.coln_normal.reserve(n);
        self.pause_timer.reserve(n);
        self.one_over_life.reserve(n);
        self.emitter_life_ratio.reserve(n);
        self.playback_rate.reserve(n);
        self.sort_val.reserve(n);
        self.flags.reserve(n);
        self.coln_mtl_id.reserve(n);
        self.rand_index.reserve(n);
        self.prev_pos.reserve(n);
        self.curr_pos.reserve(n);
        self.spawn_pos.reserve(n);
        self.matrix_weight.reserve(n);
        self.curr_vel.reserve(n);
        self.tex_frame_ratio.reserve(n);
        self.dimensions.reserve(n);
        self.curr_rot.reserve(n);
        self.curr_angle.reserve(n);
        self.color.reserve(n);
        self.emissive_intensity.reserve(n);
        self.tex_frame_a.reserve(n);
        self.tex_frame_b.reserve(n);
        self.init_tex_frame.reserve(n);
        self.alpha_mult.reserve(n);
        self.is_dead.reserve(n);
        self.is_broken_trail.reserve(n);
        self.life_ratio.reserve(n);
        self.uv_offset.reserve(n);
        self.evo_values.reserve(n * crate::kfp::MAX_EVOLUTIONS);
        self.velocity_noise_extra.reserve(n);
        self.age_atmos_extra.reserve(n);
        self.colour_tint_extra.reserve(n);
        self.collision_bounce_extra.reserve(n);
        self.attractor_orbit_extra.reserve(n);
    }

    pub fn clear(&mut self) {
        self.prev_in_chain.clear();
        self.next_in_chain.clear();
        self.chain_head = None;
        self.chain_tail = None;
        self.init_rotation.clear();
        self.coln_pos.clear();
        self.coln_speed.clear();
        self.coln_normal.clear();
        self.pause_timer.clear();
        self.one_over_life.clear();
        self.emitter_life_ratio.clear();
        self.playback_rate.clear();
        self.sort_val.clear();
        self.flags.clear();
        self.coln_mtl_id.clear();
        self.rand_index.clear();
        self.prev_pos.clear();
        self.curr_pos.clear();
        self.spawn_pos.clear();
        self.matrix_weight.clear();
        self.curr_vel.clear();
        self.tex_frame_ratio.clear();
        self.dimensions.clear();
        self.curr_rot.clear();
        self.curr_angle.clear();
        self.color.clear();
        self.emissive_intensity.clear();
        self.tex_frame_a.clear();
        self.tex_frame_b.clear();
        self.init_tex_frame.clear();
        self.alpha_mult.clear();
        self.is_dead.clear();
        self.is_broken_trail.clear();
        self.life_ratio.clear();
        self.uv_offset.clear();
        self.evo_values.clear();
        self.velocity_noise_extra.clear();
        self.age_atmos_extra.clear();
        self.colour_tint_extra.clear();
        self.collision_bounce_extra.clear();
        self.attractor_orbit_extra.clear();
    }

    pub fn spawn(&mut self, rand_index: u8) -> usize {
        let idx = self.len();
        self.prev_in_chain.push(self.chain_tail);
        self.next_in_chain.push(None);
        if let Some(prev) = self.chain_tail {
            self.next_in_chain[prev] = Some(idx);
        } else {
            self.chain_head = Some(idx);
        }
        self.chain_tail = Some(idx);
        self.init_rotation.push(Quat::IDENTITY);
        self.coln_pos.push(Vec3::ZERO);
        self.coln_speed.push(0.0);
        self.coln_normal.push(Vec3::Z);
        self.pause_timer.push(0.0);
        self.one_over_life.push(1.0);
        self.emitter_life_ratio.push(0.0);
        self.playback_rate.push(1.0);
        self.sort_val.push(0.0);
        self.flags.push(PointFlags::IS_BEING_INITIALISED);
        self.coln_mtl_id.push(0);
        self.rand_index.push(rand_index);
        self.prev_pos.push(Vec3::ZERO);
        self.curr_pos.push(Vec3::ZERO);
        self.spawn_pos.push(Vec3::ZERO);
        self.matrix_weight.push(0.0);
        self.curr_vel.push(Vec3::ZERO);
        self.tex_frame_ratio.push(0.0);
        self.dimensions.push(Vec4::ONE);
        self.curr_rot.push(Quat::IDENTITY);
        self.curr_angle.push(Vec3::ZERO);
        self.color.push(Vec4::ONE);
        self.emissive_intensity.push(0.0);
        self.tex_frame_a.push(0);
        self.tex_frame_b.push(0);
        self.init_tex_frame.push(0);
        self.alpha_mult.push(1.0);
        self.is_dead.push(false);
        self.is_broken_trail.push(false);
        self.life_ratio.push(0.0);
        self.uv_offset.push(Vec2::ZERO);
        self.velocity_noise_extra.push(Vec4::ZERO);
        self.age_atmos_extra.push(Vec4::ZERO);
        self.colour_tint_extra.push(Vec4::ONE);
        self.collision_bounce_extra.push(Vec4::ZERO);
        self.attractor_orbit_extra.push(Vec4::ZERO);
        for _ in 0..crate::kfp::MAX_EVOLUTIONS {
            self.evo_values.push(0.0);
        }
        idx
    }

    pub fn set_evo_snapshot(&mut self, idx: usize, evo_values: &[f32]) {
        let stride = crate::kfp::MAX_EVOLUTIONS;
        let base = idx * stride;
        for (slot, v) in evo_values.iter().take(stride).enumerate() {
            self.evo_values[base + slot] = *v;
        }
    }

    #[inline]
    pub fn evo_snapshot(&self, idx: usize) -> &[f32] {
        let stride = crate::kfp::MAX_EVOLUTIONS;
        let base = idx * stride;
        if base + stride > self.evo_values.len() {
            return &[];
        }
        &self.evo_values[base..base + stride]
    }

    // Order-preserving compact: dead slots removed, spawn-order chain remapped in place.
    pub fn sweep_dead(&mut self) {
        let n = self.len();
        if n == 0 {
            return;
        }
        for i in 0..n {
            if self.is_dead[i] {
                if let Some(next) = self.next_in_chain[i] {
                    self.is_broken_trail[next] = true;
                }
            }
        }
        let mut new_index: Vec<Option<usize>> = vec![None; n];
        let mut next = 0usize;
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            if !self.is_dead[i] {
                new_index[i] = Some(next);
                next += 1;
            }
        }
        let new_len = next;
        if new_len == n {
            return;
        }
        compact_in_place(&mut self.init_rotation, &new_index);
        compact_in_place(&mut self.coln_pos, &new_index);
        compact_in_place(&mut self.coln_speed, &new_index);
        compact_in_place(&mut self.coln_normal, &new_index);
        compact_in_place(&mut self.pause_timer, &new_index);
        compact_in_place(&mut self.one_over_life, &new_index);
        compact_in_place(&mut self.emitter_life_ratio, &new_index);
        compact_in_place(&mut self.playback_rate, &new_index);
        compact_in_place(&mut self.sort_val, &new_index);
        compact_in_place(&mut self.flags, &new_index);
        compact_in_place(&mut self.coln_mtl_id, &new_index);
        compact_in_place(&mut self.rand_index, &new_index);
        compact_in_place(&mut self.prev_pos, &new_index);
        compact_in_place(&mut self.curr_pos, &new_index);
        compact_in_place(&mut self.spawn_pos, &new_index);
        compact_in_place(&mut self.matrix_weight, &new_index);
        compact_in_place(&mut self.curr_vel, &new_index);
        compact_in_place(&mut self.tex_frame_ratio, &new_index);
        compact_in_place(&mut self.dimensions, &new_index);
        compact_in_place(&mut self.curr_rot, &new_index);
        compact_in_place(&mut self.curr_angle, &new_index);
        compact_in_place(&mut self.color, &new_index);
        compact_in_place(&mut self.emissive_intensity, &new_index);
        compact_in_place(&mut self.tex_frame_a, &new_index);
        compact_in_place(&mut self.tex_frame_b, &new_index);
        compact_in_place(&mut self.init_tex_frame, &new_index);
        compact_in_place(&mut self.alpha_mult, &new_index);
        compact_in_place(&mut self.is_dead, &new_index);
        compact_in_place(&mut self.is_broken_trail, &new_index);
        compact_in_place(&mut self.life_ratio, &new_index);
        compact_in_place(&mut self.uv_offset, &new_index);
        compact_in_place(&mut self.velocity_noise_extra, &new_index);
        compact_in_place(&mut self.age_atmos_extra, &new_index);
        compact_in_place(&mut self.colour_tint_extra, &new_index);
        compact_in_place(&mut self.collision_bounce_extra, &new_index);
        compact_in_place(&mut self.attractor_orbit_extra, &new_index);
        #[allow(clippy::needless_range_loop)]
        if !self.evo_values.is_empty() {
            let stride = crate::kfp::MAX_EVOLUTIONS;
            let mut next_w = 0usize;
            for i in 0..n {
                if let Some(j) = new_index[i] {
                    debug_assert_eq!(j, next_w);
                    if i != next_w {
                        let src = i * stride;
                        let dst = next_w * stride;
                        for s in 0..stride {
                            self.evo_values[dst + s] = self.evo_values[src + s];
                        }
                    }
                    next_w += 1;
                }
            }
            self.evo_values.truncate(new_len * stride);
        }
        self.prev_in_chain.truncate(new_len);
        self.next_in_chain.truncate(new_len);
        for i in 0..new_len {
            self.prev_in_chain[i] = if i == 0 { None } else { Some(i - 1) };
            self.next_in_chain[i] = if i + 1 == new_len { None } else { Some(i + 1) };
        }
        if new_len == 0 {
            self.chain_head = None;
            self.chain_tail = None;
        } else {
            self.chain_head = Some(0);
            self.chain_tail = Some(new_len - 1);
        }
    }

    pub fn first_in_chain(&self) -> Option<usize> {
        self.chain_head
    }

    pub fn to_gpu_instances(&self) -> Vec<GpuParticleInstance> {
        let n = self.len();
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let r = self.curr_rot[i];
            let flip_bits = {
                let mut b = 0.0_f32;
                if self.flags[i].contains(PointFlags::FLIP_U) {
                    b += 1.0;
                }
                if self.flags[i].contains(PointFlags::FLIP_V) {
                    b += 2.0;
                }
                b
            };
            let life = self.life_ratio[i];
            let col = self.color[i];
            let alpha_mod = col.w * self.alpha_mult[i];
            out.push(GpuParticleInstance {
                position: [self.curr_pos[i].x, self.curr_pos[i].y, self.curr_pos[i].z],
                life_ratio: life,
                dimensions: [
                    self.dimensions[i].x,
                    self.dimensions[i].y,
                    self.dimensions[i].z,
                    self.dimensions[i].w,
                ],
                rotation: [r.x, r.y, r.z, r.w],
                color: [col.x, col.y, col.z, alpha_mod],
                velocity: [self.curr_vel[i].x, self.curr_vel[i].y, self.curr_vel[i].z],
                _pad0: 0.0,
                tex_frame_a: self.tex_frame_a[i] as f32,
                tex_frame_b: self.tex_frame_b[i] as f32,
                tex_blend: self.tex_frame_ratio[i],
                _pad1: flip_bits,
                uv_offset: [self.uv_offset[i].x, self.uv_offset[i].y],
                _pad2: [0.0, 0.0],
                custom1: [life, col.x, col.y, col.z],
                custom2: [alpha_mod, 0.0, 0.0, 0.0],
            });
        }
        out
    }
}

fn compact_in_place<T>(vec: &mut Vec<T>, new_index: &[Option<usize>]) {
    #[allow(clippy::needless_range_loop)]
    for i in 0..new_index.len() {
        if let Some(j) = new_index[i] {
            if j != i {
                vec.swap(i, j);
            }
        }
    }
    let new_len = new_index.iter().filter(|x| x.is_some()).count();
    vec.truncate(new_len);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spawn_pushes_one_row_per_column() {
        let mut buf = PointBuffer::default();
        let i = buf.spawn(7);
        assert_eq!(i, 0);
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.rand_index[0], 7);
        assert!(buf.flags[0].contains(PointFlags::IS_BEING_INITIALISED));
        assert_eq!(buf.uv_offset.len(), 1);
        assert_eq!(buf.uv_offset[0], Vec2::ZERO);
    }

    #[test]
    fn sweep_dead_compacts_in_place() {
        let mut buf = PointBuffer::default();
        for _ in 0..4 {
            buf.spawn(0);
        }
        buf.is_dead[1] = true;
        buf.is_dead[3] = true;
        buf.sweep_dead();
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn spawn_chain_is_doubly_linked_in_order() {
        let mut buf = PointBuffer::default();
        for _ in 0..3 {
            buf.spawn(0);
        }
        assert_eq!(buf.first_in_chain(), Some(0));
        assert_eq!(buf.next_in_chain[0], Some(1));
        assert_eq!(buf.next_in_chain[1], Some(2));
        assert_eq!(buf.next_in_chain[2], None);
        assert_eq!(buf.prev_in_chain[2], Some(1));
        assert_eq!(buf.prev_in_chain[1], Some(0));
        assert_eq!(buf.prev_in_chain[0], None);
    }

    #[test]
    fn sweep_dead_marks_broken_trail_at_neighbour() {
        let mut buf = PointBuffer::default();
        for _ in 0..3 {
            buf.spawn(0);
        }
        buf.is_dead[1] = true;
        buf.sweep_dead();
        assert_eq!(buf.len(), 2);
        assert!(buf.is_broken_trail[1], "broken-trail not propagated");
    }
}
