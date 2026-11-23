//! The view from the head of the dressed ped: a camera at the head bone, turned by the pointer, with the engine's default field of view.
//!
//! ⚠ **The field of view is the engine's default and no measurement of its first person camera**: that camera's own value is metadata
//! of the install, which is not read here. The eye is the head bone's place at rest, raised by the heels; a clip that moves the head
//! does not move it.

use glam::Vec3;
use or_renderer::OrbitCamera;
use or_wardrobe::Ped;

use super::Dressed;
use crate::render::viewport::CameraInputs;

/// Where the head looks to begin with: along the ped's forward, the y axis, level.
pub const LOOK: (f32, f32) = (-std::f32::consts::FRAC_PI_2, 0.0);

/// The vertical field of view of the view from the head, in degrees: the engine's default for a camera.
pub const FIELD_OF_VIEW: f32 = 45.0;

/// The bone the eyes are placed at.
const HEAD: &str = "SKEL_Head";

/// How far the camera's target stands in front of the eye: the camera orbits a point, so the head turns about the eye with the target this close.
const AHEAD: f32 = 0.02;

/// The nearest a surface is drawn from the eye, so that the head the eye stands in does not fill the view.
const NEAR: f32 = 0.02;

impl Dressed {
    /// Where the eyes of `ped` stand: the head bone at rest, raised by the heels; none while the skeleton is not read.
    pub fn eyes(&self, ped: Ped) -> Option<Vec3> {
        let (_, at) = self.bone_at_rest(ped, crate::i18n::tr(HEAD))?;
        Some(at.w_axis.truncate() + Vec3::Z * self.lift)
    }

    /// Turn the head by what the pointer did and place the camera at the eyes of `ped`; whether there is a head to place it at.
    pub(super) fn look_from_the_head(&mut self, ped: Ped, inputs: &CameraInputs) -> bool {
        let Some(eyes) = self.eyes(ped) else {
            return false;
        };
        let mut turn = OrbitCamera {
            yaw: self.look.0,
            pitch: self.look.1,
            ..OrbitCamera::default()
        };
        turn.orbit(inputs.orbit.x, inputs.orbit.y);
        self.look = (turn.yaw, turn.pitch);
        // The way from the target to the eye is the way the head does not look.
        let behind = Vec3::new(
            turn.pitch.cos() * turn.yaw.cos(),
            turn.pitch.cos() * turn.yaw.sin(),
            turn.pitch.sin(),
        );
        self.first_person_camera = OrbitCamera {
            target: eyes - behind * AHEAD,
            distance: AHEAD,
            fov_y: FIELD_OF_VIEW.to_radians(),
            near: NEAR,
            ..turn
        };
        true
    }

    /// The camera of the view from the head, as the last frame placed it.
    pub(super) fn head_camera(&self) -> &OrbitCamera {
        &self.first_person_camera
    }
}
