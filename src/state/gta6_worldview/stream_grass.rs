//! Where an instance of a grass list stands: scaled, turned about its own up, tilted to the
//! ground it grows on, then moved to its place in the list's box.

use glam::{Mat4, Quat, Vec3};
use or_io::gta6::map::stream::{GrassInstance, StreamGrassList};

use super::read::{rows_of, Rows};

/// The rows that place `instance` of `list` at `origin`, its place in the world.
pub(super) fn rows(list: &StreamGrassList, instance: &GrassInstance, origin: Vec3) -> Rows {
    let [across, up] = list.scale_of(instance);
    // The list says how much of the ground's tilt its instances take.
    let to_ground = Quat::from_rotation_arc(Vec3::Z, Vec3::from(instance.normal()));
    let tilt = Quat::IDENTITY.slerp(to_ground, list.orient_to_terrain.clamp(0.0, 1.0));
    rows_of(Mat4::from_scale_rotation_translation(
        Vec3::new(across, across, up),
        tilt * Quat::from_rotation_z(instance.yaw()),
        origin,
    ))
}
