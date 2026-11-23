use or_io::gta5::particle::{Axis, PtxDomain, PtxKeyframeProp};

use super::{Domain, Vec3};

/// `ptxDomainShape`: what a domain's `shape` states.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PtxDomainShape {
    #[default]
    Box = 0,
    Sphere = 1,
    Cylinder = 2,
    Attractor = 3,
}

impl PtxDomainShape {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Box),
            1 => Some(Self::Sphere),
            2 => Some(Self::Cylinder),
            3 => Some(Self::Attractor),
            _ => None,
        }
    }
}

fn first(k: &PtxKeyframeProp) -> (f32, f32, f32) {
    k.keyframe
        .entries
        .first()
        .map_or((0.0, 0.0, 0.0), |e| (e.value.x, e.value.y, e.value.z))
}

impl Domain {
    /// The editor's shape for a domain, from its first keys.
    pub fn of(d: &PtxDomain) -> Self {
        let (px, py, pz) = first(&d.position_kfp);
        let position = Vec3::new(px, py, pz);
        let (ox, oy, oz) = first(&d.size_outer_kfp);
        let (ix, _, _) = first(&d.size_inner_kfp);
        match PtxDomainShape::from_u32(d.shape as u32) {
            Some(PtxDomainShape::Box) => Domain::Box {
                min: Vec3::new(position.x - ox, position.y - oy, position.z - oz),
                max: Vec3::new(position.x + ox, position.y + oy, position.z + oz),
            },
            Some(PtxDomainShape::Sphere) => Domain::Sphere {
                center: position,
                radius_inner: ix.abs(),
                radius_outer: ox.abs().max(0.001),
            },
            Some(PtxDomainShape::Cylinder) => {
                let (rx, ry, rz) = first(&d.rotation_kfp);
                use std::f32::consts::FRAC_PI_2;
                let near_pi2 = |v: f32| (v.abs() - FRAC_PI_2).abs() < 0.1;
                let axis = if near_pi2(rz) && rx.abs() < 0.1 && ry.abs() < 0.1 {
                    Axis::X
                } else if near_pi2(rx) && ry.abs() < 0.1 && rz.abs() < 0.1 {
                    Axis::Z
                } else {
                    Axis::Y
                };
                Domain::Cylinder {
                    center: position,
                    radius: ox.abs().max(0.001),
                    height: oy.abs().max(0.001),
                    axis,
                }
            }
            Some(PtxDomainShape::Attractor) => Domain::Attractor {
                position,
                strength: ox,
            },
            None => Domain::Point(position),
        }
    }
}
