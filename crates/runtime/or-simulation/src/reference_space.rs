use crate::behaviour::BehaviourParams;
use glam::{Mat3, Vec3};
use or_io::gta5::particle::ReferenceSpace;
#[inline]
#[allow(dead_code)]
pub fn to_glam(v: or_particle::domain::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub fn transform_vector(space: ReferenceSpace, v: Vec3, params: &BehaviourParams<'_>) -> Vec3 {
    match space {
        ReferenceSpace::EFFECT => Mat3::from_mat4(params.effect_matrix) * v,
        ReferenceSpace::EMITTER => Mat3::from_mat4(params.emitter_matrix) * v,
        ReferenceSpace::CAMERA => params.camera_basis * v,
        _ => v,
    }
}
