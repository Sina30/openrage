//! The lights and the fog the effects of the last update gave, as what the renderer draws in the air of Shaded.

use or_renderer::{FogLight, FogVolume, PointLight, SpotLight, ViewportRenderer};
use or_simulation::sinks::{FogLighting, FogVolumeInfo, LightInfo};

use crate::state::sinks::EditorSinks;

/// The volume the renderer draws for what an effect asked for.
pub fn volume_of(info: &FogVolumeInfo) -> FogVolume {
    FogVolume {
        position: info.pos,
        scale: info.scale,
        rotation: info.rotation,
        range: info.range,
        density: info.density,
        falloff: info.falloff,
        tint: info.colour.truncate(),
        hdr_mult: info.hdr_mult,
        light: match info.lighting {
            FogLighting::FogHdr => FogLight::DayFog,
            FogLighting::Directional => FogLight::Sun,
            FogLighting::None => FogLight::None,
        },
        ground_tint: info.use_ground_fog_colour,
    }
}

/// The lights that shine all round and the ones that shine in a cone, of what the effects gave: a light with an axis and an angle is a cone, and one without reaches no way.
pub fn lights_of(infos: Vec<LightInfo>) -> (Vec<PointLight>, Vec<SpotLight>) {
    let (mut points, mut spots) = (Vec::new(), Vec::new());
    for light in infos.into_iter().filter(|light| light.range > 0.0) {
        if light.cone_half_angle > 0.0 && light.direction != glam::Vec3::ZERO {
            spots.push(SpotLight {
                position: light.pos,
                direction: light.direction,
                colour: light.colour,
                intensity: light.intensity,
                radius: light.range,
                falloff: light.falloff,
                outer_degrees: light.cone_half_angle,
            });
        } else {
            points.push(PointLight {
                position: light.pos,
                colour: light.colour,
                intensity: light.intensity,
                radius: light.range,
                falloff: light.falloff,
            });
        }
    }
    (points, spots)
}

/// Give the renderer the lights and the fog of the last update that ran.
pub fn give(renderer: &mut ViewportRenderer, sinks: &EditorSinks) {
    let (points, spots) = lights_of(sinks.lights.lights());
    renderer.set_point_lights(points);
    renderer.set_spot_lights(spots);
    renderer.set_fog_volumes(sinks.fog.volumes().iter().map(volume_of).collect());
}

/// Leave the air of Shaded empty.
pub fn clear(renderer: &mut ViewportRenderer) {
    renderer.set_point_lights(Vec::new());
    renderer.set_spot_lights(Vec::new());
    renderer.set_fog_volumes(Vec::new());
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Vec3, Vec4};

    fn light(direction: Vec3, angle: f32, range: f32) -> LightInfo {
        LightInfo {
            pos: Vec3::new(1.0, 2.0, 3.0),
            colour: Vec3::ONE,
            intensity: 2.0,
            range,
            direction,
            cone_half_angle: angle,
            falloff: 3.0,
            corona_size: 0.0,
        }
    }

    #[test]
    fn a_light_with_an_axis_and_an_angle_is_a_cone_and_the_others_shine_all_round() {
        let (points, spots) = lights_of(vec![
            light(Vec3::ZERO, 0.0, 5.0),
            light(Vec3::NEG_Z, 40.0, 5.0),
            light(Vec3::NEG_Z, 40.0, 0.0),
            light(Vec3::X, 0.0, 5.0),
        ]);
        assert_eq!(points.len(), 2, "no axis or no angle shines all round");
        assert_eq!(spots.len(), 1, "and a light that reaches no way is none");
        assert_eq!(
            (spots[0].direction, spots[0].outer_degrees, spots[0].falloff),
            (Vec3::NEG_Z, 40.0, 3.0)
        );
    }

    #[test]
    fn a_volume_keeps_what_the_effect_asked_for_and_names_its_light_as_the_renderer_does() {
        let info = FogVolumeInfo {
            pos: Vec3::new(1.0, 2.0, 3.0),
            scale: Vec3::new(2.0, 1.0, 1.0),
            rotation: Vec3::new(0.1, 0.2, 0.3),
            range: 6.0,
            colour: Vec4::new(0.5, 0.25, 0.125, 1.0),
            density: 40.0,
            falloff: 2.0,
            hdr_mult: 1.5,
            lighting: FogLighting::Directional,
            use_ground_fog_colour: true,
            sort_with_particles: false,
        };
        let volume = volume_of(&info);
        assert_eq!(
            (volume.position, volume.scale, volume.rotation),
            (info.pos, info.scale, info.rotation)
        );
        assert_eq!(
            (volume.range, volume.density, volume.falloff),
            (6.0, 40.0, 2.0)
        );
        assert_eq!(volume.tint, Vec3::new(0.5, 0.25, 0.125));
        assert_eq!(volume.hdr_mult, 1.5);
        assert!(volume.ground_tint);
        for (lighting, light) in [
            (FogLighting::FogHdr, FogLight::DayFog),
            (FogLighting::Directional, FogLight::Sun),
            (FogLighting::None, FogLight::None),
        ] {
            let lit = volume_of(&FogVolumeInfo { lighting, ..info });
            assert_eq!(lit.light, light);
        }
    }
}
