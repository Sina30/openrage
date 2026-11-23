use crate::behaviour::{order, BehaviourImpl, BehaviourParams, EmitSink};
use crate::point::{PointBuffer, PointFlags};
use glam::{Vec3, Vec4};
use or_particle::behaviour::DecalPoolBehaviour;
use std::sync::Mutex;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecalPoolEmit {
    pub pos: Vec3,
    pub normal: Vec3,
    pub color: Vec4,
    pub mtl_id: u8,
    pub liquid_type: i32,
    pub decal_id: u32,
    pub start_size: f32,
    pub end_size: f32,
    pub growth_rate: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecalPoolDecal {
    pub pos: Vec3,
    pub normal: Vec3,
    pub radius: f32,
    pub end_radius: f32,
    pub growth_rate: f32,
    pub color: Vec4,
    pub mtl_id: u8,
    pub liquid_type: i32,
    pub decal_id: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecalPoolBatch {
    pub decals: Vec<DecalPoolDecal>,
}

#[derive(Debug)]
pub struct DecalPool {
    pub data: DecalPoolBehaviour,
    pending: Mutex<Vec<DecalPoolEmit>>,
}

impl DecalPool {
    pub fn from_data(data: DecalPoolBehaviour) -> Self {
        Self {
            data,
            pending: Mutex::new(Vec::new()),
        }
    }

    fn emit_from_collision(&self, idx: usize, buf: &PointBuffer, params: &BehaviourParams<'_>) {
        if !buf.flags[idx].contains(PointFlags::HAS_COLLIDED) {
            return;
        }
        let speed = buf.curr_vel[idx].length();
        if speed < self.data.velocity_thresh {
            return;
        }
        let pos = buf.coln_pos[idx];
        let normal = buf.coln_normal[idx];
        let color = buf.color[idx];
        let mtl = buf.coln_mtl_id[idx];
        let emit = DecalPoolEmit {
            pos,
            normal,
            color: Vec4::new(color.x, color.y, color.z, color.w * buf.alpha_mult[idx]),
            mtl_id: mtl,
            liquid_type: self.data.liquid_type as i32,
            decal_id: self.data.decal_id,
            start_size: self.data.start_size,
            end_size: self.data.end_size,
            growth_rate: self.data.growth_rate,
        };
        if let Ok(mut q) = self.pending.lock() {
            q.push(emit);
        }
        if let Some(sink) = params.sinks.decal_pool {
            sink.add_pool_decal(crate::sinks::DecalPoolInfo {
                pos,
                normal,
                radius: self.data.start_size,
                colour: Vec4::new(color.x, color.y, color.z, color.w * buf.alpha_mult[idx]),
                life_seconds: 1.0 / buf.one_over_life[idx].max(1e-6),
                material_id: mtl,
            });
        }
    }
}

impl BehaviourImpl for DecalPool {
    fn order(&self) -> f32 {
        order::DECAL_POOL
    }
    fn name(&self) -> &'static str {
        "ptxu_DecalPool"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        buf.flags[idx].insert(PointFlags::COLLIDES);
        self.emit_from_collision(idx, buf, params);
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        self.emit_from_collision(idx, buf, params);
    }

    fn update_finalize(&self, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        for i in 0..buf.len() {
            self.emit_from_collision(i, buf, params);
        }
    }

    fn drain_emits(&self, sink: &mut EmitSink) {
        if let Ok(mut q) = self.pending.lock() {
            sink.decal_pool.append(&mut *q);
        }
    }
}
