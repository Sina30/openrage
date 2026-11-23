use crate::kfp::query_kfp;
use crate::random::RandomTable;
use glam::{Mat4, Quat, Vec3, Vec4};
use or_io::gta5::particle::PtxDomain;
use or_particle::domain::PtxDomainShape;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainSlot {
    Creation,
    Target,
    Attractor,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DomainRefFrames {
    pub creation_world: Option<Mat4>,
    pub target_world: Option<Mat4>,
}

#[derive(Debug, Clone)]
pub struct DomainInst {
    pub shape: PtxDomainShape,
    pub position: Vec3,
    pub rotation: Quat,
    pub size_outer: Vec3,
    pub size_inner: Vec3,
    pub local_to_world: Mat4,
    /// The model surface a mesh domain emits from, which a refresh keeps; `None` samples `shape`.
    pub mesh: Option<std::sync::Arc<crate::domain_mesh::DomainMesh>>,
}

impl Default for DomainInst {
    fn default() -> Self {
        Self {
            shape: PtxDomainShape::Box,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            size_outer: Vec3::splat(0.5),
            size_inner: Vec3::ZERO,
            local_to_world: Mat4::IDENTITY,
            mesh: None,
        }
    }
}

impl DomainInst {
    pub fn refresh(&mut self, src: &PtxDomain, emitter_time: f32, parent: Mat4) {
        self.refresh_with_zoom(src, emitter_time, parent, 1.0);
    }

    pub fn refresh_with_zoom(
        &mut self,
        src: &PtxDomain,
        emitter_time: f32,
        parent: Mat4,
        zoom_scale: f32,
    ) {
        self.refresh_for_slot(
            src,
            emitter_time,
            parent,
            zoom_scale,
            DomainSlot::Creation,
            DomainRefFrames::default(),
        );
    }

    pub fn refresh_for_slot(
        &mut self,
        src: &PtxDomain,
        emitter_time: f32,
        effect_matrix: Mat4,
        zoom_scale: f32,
        slot: DomainSlot,
        frames: DomainRefFrames,
    ) {
        self.shape = PtxDomainShape::from_u32(src.shape as u32).unwrap_or(PtxDomainShape::Box);
        let pos = query_kfp(&src.position_kfp, emitter_time);
        let rot = query_kfp(&src.rotation_kfp, emitter_time);
        let so = query_kfp(&src.size_outer_kfp, emitter_time);
        let si = query_kfp(&src.size_inner_kfp, emitter_time);
        let s = zoom_scale.max(0.001);

        self.position = Vec3::new(pos.x, pos.y, pos.z) * s;
        self.rotation = Quat::from_euler(glam::EulerRot::XYZ, rot.x, rot.y, rot.z);
        self.size_outer = Vec3::new(
            so.x.abs().max(0.0) * s,
            so.y.abs().max(0.0) * s,
            so.z.abs().max(0.0) * s,
        );
        self.size_inner = Vec3::new(
            si.x.abs().max(0.0) * s,
            si.y.abs().max(0.0) * s,
            si.z.abs().max(0.0) * s,
        );

        let mut parent = effect_matrix;
        match slot {
            DomainSlot::Creation => {}
            DomainSlot::Target => {
                if src.is_creation_relative.get() {
                    if let Some(creation) = frames.creation_world {
                        parent = creation;
                    }
                }
            }
            DomainSlot::Attractor => {
                if src.is_creation_relative.get() {
                    if let Some(creation) = frames.creation_world {
                        parent = creation;
                    }
                } else if src.is_target_relative.get() {
                    if let Some(target) = frames.target_world {
                        parent = target;
                    }
                }
            }
        }
        if src.is_world_space.get() {
            let translation = parent.w_axis;
            parent = Mat4::from_cols(
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
                translation,
            );
        }

        let local = Mat4::from_rotation_translation(self.rotation, self.position);
        self.local_to_world = parent * local;
    }

    pub fn sample_position(&self, uvw: [f32; 3]) -> Vec3 {
        if let Some(point) = self.surface_point(uvw) {
            return point;
        }
        let local = sample_in_local_space(self.shape, self.size_outer, self.size_inner, uvw);
        self.local_to_world.transform_point3(local)
    }

    /// The point of its model's surface `uvw` picks, in the world; `None` with no mesh.
    pub fn surface_point(&self, uvw: [f32; 3]) -> Option<Vec3> {
        let mesh = self.mesh.as_ref()?;
        Some(self.local_to_world.transform_point3(mesh.sample(uvw)))
    }

    pub fn sample_direction(&self, uvw: [f32; 3]) -> Vec3 {
        let local = match &self.mesh {
            Some(mesh) => mesh.sample(uvw),
            None => sample_in_local_space(self.shape, self.size_outer, self.size_inner, uvw),
        };
        self.local_to_world.transform_vector3(local)
    }

    pub fn world_position(&self) -> Vec3 {
        self.local_to_world.transform_point3(Vec3::ZERO)
    }
}

fn sample_in_local_space(shape: PtxDomainShape, outer: Vec3, inner: Vec3, uvw: [f32; 3]) -> Vec3 {
    let [u, v, w] = uvw;
    match shape {
        PtxDomainShape::Box => {
            let candidate = Vec3::new(
                (u * 2.0 - 1.0) * outer.x,
                (v * 2.0 - 1.0) * outer.y,
                (w * 2.0 - 1.0) * outer.z,
            );
            let has_inner = inner.x > 1e-5 || inner.y > 1e-5 || inner.z > 1e-5;
            if has_inner
                && candidate.x.abs() < inner.x
                && candidate.y.abs() < inner.y
                && candidate.z.abs() < inner.z
            {
                let mut out = candidate;
                let dx = outer.x - inner.x;
                let dy = outer.y - inner.y;
                let dz = outer.z - inner.z;
                if dx >= dy && dx >= dz {
                    out.x = if candidate.x >= 0.0 {
                        inner.x + (outer.x - inner.x) * u
                    } else {
                        -(inner.x + (outer.x - inner.x) * u)
                    };
                } else if dy >= dz {
                    out.y = if candidate.y >= 0.0 {
                        inner.y + (outer.y - inner.y) * v
                    } else {
                        -(inner.y + (outer.y - inner.y) * v)
                    };
                } else {
                    out.z = if candidate.z >= 0.0 {
                        inner.z + (outer.z - inner.z) * w
                    } else {
                        -(inner.z + (outer.z - inner.z) * w)
                    };
                }
                out
            } else {
                candidate
            }
        }
        PtxDomainShape::Sphere => {
            let theta = 2.0 * std::f32::consts::PI * u;
            let phi = (1.0 - 2.0 * v).acos();
            let r_outer = outer.x.max(outer.y).max(outer.z).max(1e-4);
            let r_inner = inner.x.min(inner.y).min(inner.z).max(0.0).min(r_outer);
            let r3_inner = r_inner.powi(3);
            let r3_outer = r_outer.powi(3);
            let r_cubed = r3_inner + w * (r3_outer - r3_inner);
            let r = r_cubed.max(0.0).powf(1.0 / 3.0);
            Vec3::new(
                r * phi.sin() * theta.cos(),
                r * phi.sin() * theta.sin(),
                r * phi.cos(),
            )
        }
        PtxDomainShape::Cylinder => {
            let theta = 2.0 * std::f32::consts::PI * u;
            let r_outer = outer.x.max(1e-4);
            let r_inner = inner.x.max(0.0).min(r_outer);
            let r2 = r_inner * r_inner + v * (r_outer * r_outer - r_inner * r_inner);
            let r = r2.max(0.0).sqrt();
            let h = (w * 2.0 - 1.0) * outer.y;
            Vec3::new(r * theta.cos(), h, r * theta.sin())
        }
        PtxDomainShape::Attractor => Vec3::ZERO,
    }
}

pub fn point_domain_at(pos: Vec3) -> DomainInst {
    DomainInst {
        shape: PtxDomainShape::Box,
        position: pos,
        rotation: Quat::IDENTITY,
        size_outer: Vec3::ZERO,
        size_inner: Vec3::ZERO,
        local_to_world: Mat4::from_translation(pos),
        mesh: None,
    }
}

pub fn three_uniforms(table: &RandomTable, base: u32) -> [f32; 3] {
    [
        table.value(base),
        table.value(base + 1),
        table.value(base + 2),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn box_sample_is_inside_outer() {
        let d = DomainInst {
            shape: PtxDomainShape::Box,
            size_outer: Vec3::new(1.0, 2.0, 3.0),
            ..Default::default()
        };
        let p = d.sample_position([0.5, 0.5, 0.5]);
        assert!(p.x.abs() <= 1.001);
        assert!(p.y.abs() <= 2.001);
        assert!(p.z.abs() <= 3.001);
    }

    #[test]
    fn sphere_sample_is_inside_radius() {
        let d = DomainInst {
            shape: PtxDomainShape::Sphere,
            size_outer: Vec3::splat(2.0),
            ..Default::default()
        };
        for u in 0..8 {
            for v in 0..8 {
                for w in 0..8 {
                    let uvw = [u as f32 / 7.0, v as f32 / 7.0, w as f32 / 7.0];
                    let p = d.sample_position(uvw);
                    assert!(p.length() <= 2.001, "sample outside sphere: {:?}", p);
                }
            }
        }
    }

    #[test]
    fn cylinder_sample_is_inside_volume() {
        let d = DomainInst {
            shape: PtxDomainShape::Cylinder,
            size_outer: Vec3::new(1.0, 2.0, 0.0),
            ..Default::default()
        };
        for i in 0..16 {
            let t = i as f32 / 15.0;
            let p = d.sample_position([t, t, t]);
            let r = (p.x * p.x + p.z * p.z).sqrt();
            assert!(r <= 1.001);
            assert!(p.y.abs() <= 2.001);
        }
    }

    #[test]
    fn hollow_box_pushes_samples_out_of_inner_cuboid() {
        let d = DomainInst {
            shape: PtxDomainShape::Box,
            size_outer: Vec3::new(2.0, 2.0, 2.0),
            size_inner: Vec3::new(1.0, 1.0, 1.0),
            ..Default::default()
        };
        let p = d.sample_position([0.5, 0.5, 0.5]);
        let inside_inner =
            p.x.abs() < 1.0 - 1e-3 && p.y.abs() < 1.0 - 1e-3 && p.z.abs() < 1.0 - 1e-3;
        assert!(
            !inside_inner,
            "hollow box leaked sample inside inner: {p:?}"
        );
        assert!(p.x.abs() <= 2.001 && p.y.abs() <= 2.001 && p.z.abs() <= 2.001);
    }

    #[test]
    fn hollow_sphere_respects_inner_radius() {
        let d = DomainInst {
            shape: PtxDomainShape::Sphere,
            size_outer: Vec3::splat(2.0),
            size_inner: Vec3::splat(1.0),
            ..Default::default()
        };
        for i in 0..32 {
            let t = i as f32 / 31.0;
            let p = d.sample_position([t, (t * 1.7) % 1.0, (t * 2.3) % 1.0]);
            let r = p.length();
            assert!(r >= 0.99, "sample inside inner radius: r={r}");
            assert!(r <= 2.01, "sample outside outer radius: r={r}");
        }
    }

    /// `is_world_space` must strip the parent matrix's
    /// rotation, leaving only the translation. Engine call:
    /// `vTransformMtx.SetIdentity3x3()`.
    #[test]
    fn world_space_strips_parent_rotation() {
        let src = PtxDomain {
            is_world_space: or_io::decl::Bool8::new(true),
            ..Default::default()
        };
        let parent = Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0))
            * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let mut d = DomainInst::default();
        d.refresh_for_slot(
            &src,
            0.0,
            parent,
            1.0,
            DomainSlot::Creation,
            DomainRefFrames::default(),
        );
        let p = d.local_to_world.transform_point3(Vec3::new(1.0, 0.0, 0.0));
        assert!(
            (p - Vec3::new(6.0, 0.0, 0.0)).length() < 1e-4,
            "world_space should strip rotation; got {p:?}"
        );
    }

    #[test]
    fn target_creation_relative_uses_creation_frame() {
        let src = PtxDomain {
            is_creation_relative: or_io::decl::Bool8::new(true),
            ..Default::default()
        };
        let creation = Mat4::from_translation(Vec3::new(100.0, 200.0, 300.0));
        let effect = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let mut d = DomainInst::default();
        d.refresh_for_slot(
            &src,
            0.0,
            effect,
            1.0,
            DomainSlot::Target,
            DomainRefFrames {
                creation_world: Some(creation),
                target_world: None,
            },
        );
        let origin = d.local_to_world.transform_point3(Vec3::ZERO);
        assert!(
            (origin - Vec3::new(100.0, 200.0, 300.0)).length() < 1e-4,
            "target with is_creation_relative should anchor at creation; got {origin:?}"
        );
    }

    #[test]
    fn attractor_target_relative_uses_target_frame() {
        let src = PtxDomain {
            is_target_relative: or_io::decl::Bool8::new(true),
            ..Default::default()
        };
        let target = Mat4::from_translation(Vec3::new(7.0, 8.0, 9.0));
        let effect = Mat4::IDENTITY;
        let mut d = DomainInst::default();
        d.refresh_for_slot(
            &src,
            0.0,
            effect,
            1.0,
            DomainSlot::Attractor,
            DomainRefFrames {
                creation_world: None,
                target_world: Some(target),
            },
        );
        let origin = d.local_to_world.transform_point3(Vec3::ZERO);
        assert!((origin - Vec3::new(7.0, 8.0, 9.0)).length() < 1e-4);
    }
}
