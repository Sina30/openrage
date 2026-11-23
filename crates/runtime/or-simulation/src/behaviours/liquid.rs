use crate::behaviour::{order, BehaviourImpl, BehaviourParams, EmitSink};
use crate::point::{PointBuffer, PointFlags};
use glam::{Vec3, Vec4};
use or_particle::behaviour::LiquidBehaviour;
use std::sync::Mutex;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidEmit {
    pub pos: Vec3,
    pub normal: Vec3,
    pub color: Vec4,
    pub mtl_id: u8,
    pub liquid_type: i32,
    pub trail_width_min: f32,
    pub trail_width_max: f32,
    pub pool_start_size: f32,
    pub pool_end_size: f32,
    pub pool_growth_rate: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidPool {
    pub pos: Vec3,
    pub normal: Vec3,
    pub radius: f32,
    pub end_radius: f32,
    pub growth_rate: f32,
    pub color: Vec4,
    pub mtl_id: u8,
    pub liquid_type: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiquidBatch {
    pub pools: Vec<LiquidPool>,
}

#[derive(Debug)]
pub struct Liquid {
    pub data: LiquidBehaviour,
    pending: Mutex<Vec<LiquidEmit>>,
}

impl Liquid {
    pub fn from_data(data: LiquidBehaviour) -> Self {
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
        let emit = LiquidEmit {
            pos,
            normal,
            color: Vec4::new(color.x, color.y, color.z, color.w * buf.alpha_mult[idx]),
            mtl_id: mtl,
            liquid_type: self.data.liquid_type as i32,
            trail_width_min: self.data.trail_width_min,
            trail_width_max: self.data.trail_width_max,
            pool_start_size: self.data.pool_start_size,
            pool_end_size: self.data.pool_end_size,
            pool_growth_rate: self.data.pool_growth_rate,
        };
        if let Ok(mut q) = self.pending.lock() {
            q.push(emit);
        }
        if let Some(sink) = params.sinks.liquid {
            sink.add_liquid(crate::sinks::LiquidInfo {
                pos,
                normal,
                size: glam::Vec3::splat(self.data.pool_start_size),
                colour: Vec4::new(color.x, color.y, color.z, color.w * buf.alpha_mult[idx]),
                material_id: mtl,
            });
        }
    }
}

impl BehaviourImpl for Liquid {
    fn order(&self) -> f32 {
        order::LIQUID
    }
    fn name(&self) -> &'static str {
        "ptxu_Liquid"
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
            sink.liquid.append(&mut *q);
        }
    }
}
