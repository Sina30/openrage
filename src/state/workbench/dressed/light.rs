//! The key light the dressed ped is seen in: where it comes from and how strong it is.
//!
//! The viewport's plain modes (solid and material) light a surface by one key light, a head light that follows the camera
//! and an ambient light. This sets the first of them and leaves the others, and the tint of a garment multiplies the
//! colour of the surface as it did, so a light never changes what a tint shows.

use glam::Vec3;
use or_renderer::viewport::{KEY_LIGHT_STRENGTH, KEY_LIGHT_TOWARDS};

/// The strongest a person may make the key light.
pub const STRONGEST: f32 = 2.0;

/// Where the key light is and how strongly it lights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// The direction round the ped the light comes from, in degrees: 0 from the x axis, turning towards the y axis.
    pub azimuth: f32,
    /// How high the light stands, in degrees: 0 along the floor, 90 straight above.
    pub elevation: f32,
    pub strength: f32,
    /// Light from where the sun stands at the hour the viewport's time of day says, in place of the direction above.
    pub sun: bool,
}

impl Default for Light {
    /// The light the viewport had before it could be set.
    fn default() -> Self {
        Self::towards(KEY_LIGHT_TOWARDS, KEY_LIGHT_STRENGTH)
    }
}

impl Light {
    /// The light that comes from the way `towards` points, with `strength`.
    pub fn towards(towards: Vec3, strength: f32) -> Self {
        let way = towards.normalize_or_zero();
        Self {
            azimuth: way.y.atan2(way.x).to_degrees().rem_euclid(360.0),
            elevation: way.z.clamp(-1.0, 1.0).asin().to_degrees(),
            strength,
            sun: false,
        }
    }

    /// The unit way towards the light, from the direction and the height.
    pub fn way(self) -> Vec3 {
        let (azimuth, elevation) = (self.azimuth.to_radians(), self.elevation.to_radians());
        Vec3::new(
            elevation.cos() * azimuth.cos(),
            elevation.cos() * azimuth.sin(),
            elevation.sin(),
        )
    }

    /// Whether this is the light the viewport began with.
    pub fn is_the_first(self) -> bool {
        let first = Self::default();
        self.sun == first.sun
            && (self.way() - first.way()).length() < 1e-4
            && (self.strength - first.strength).abs() < 1e-4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_the_viewport_began_with_is_the_default_and_its_way_comes_back_from_the_angles() {
        let first = Light::default();
        let want = KEY_LIGHT_TOWARDS.normalize();
        assert!(
            (first.way() - want).length() < 1e-5,
            "{:?} against {want:?}",
            first.way()
        );
        assert_eq!(first.strength, KEY_LIGHT_STRENGTH);
        assert!(!first.sun && first.is_the_first());
    }

    #[test]
    fn a_direction_and_a_height_give_a_unit_way_and_each_angle_is_one_side_of_it() {
        let east = Light {
            azimuth: 0.0,
            elevation: 0.0,
            strength: 1.0,
            sun: false,
        };
        assert!((east.way() - Vec3::X).length() < 1e-6);
        let north = Light {
            azimuth: 90.0,
            ..east
        };
        assert!((north.way() - Vec3::Y).length() < 1e-6);
        let above = Light {
            elevation: 90.0,
            ..east
        };
        assert!((above.way() - Vec3::Z).length() < 1e-6);
        for azimuth in [0.0, 37.0, 180.0, 299.0] {
            for elevation in [-60.0, 0.0, 45.0, 90.0] {
                let light = Light {
                    azimuth,
                    elevation,
                    ..east
                };
                assert!((light.way().length() - 1.0).abs() < 1e-5);
                let back = Light::towards(light.way(), 1.0);
                assert!(
                    (back.way() - light.way()).length() < 1e-4,
                    "{azimuth} {elevation}"
                );
            }
        }
        assert!(!above.is_the_first());
    }
}
