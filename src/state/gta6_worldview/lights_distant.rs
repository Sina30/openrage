//! The distant lights the frame draws as sprites: every light of the distant lists within its size
//! class's reach, beside the volume that may light around it, as GTA VI's distant light pass draws it.

use glam::Vec3;
use or_io::gta5::data::visual_settings::VisualSettings;
use or_renderer::game_shaders::distant_lights::SpriteLook;
use or_renderer::DistantLight;

use super::lights_lod::Distant;

/// How far the distant lights of each size class (small, medium, large) are drawn, the distances they fade in over, what their stated intensity is multiplied by, and how a sprite is drawn; the default draws nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DistantReach {
    pub range: [f32; 3],
    pub near: [f32; 3],
    pub far: [f32; 3],
    pub gain: f32,
    pub look: SpriteLook,
}

impl DistantReach {
    /// As the install's visual settings state it, with the defaults the game takes for a setting it does not find; a range of nought, which draws no sprite, where they do not state one.
    pub fn of(settings: &VisualSettings) -> Self {
        let size = ["small", "medium", "large"];
        let class =
            |s: &str, what: &str| settings.get_f32(&format!("lodlight.distant.{s}.{what}"), 0.0);
        let get =
            |what: &str, default: f32| settings.get_f32(&format!("distantlights.{what}"), default);
        Self {
            range: size.map(|s| class(s, "range")),
            near: size.map(|s| class(s, "fadeNear")),
            far: size.map(|s| class(s, "fadeFar")),
            gain: get("streetlight.HDRIntensity", 4.0),
            look: SpriteLook {
                coverage: get("lightScreenCoverage", 0.011_111),
                area: get("lightWorldArea", 0.1),
                // The settings state it a kilometre.
                extinction: get("transmittanceScale", 0.7) * 0.001,
                exposure: 1.0,
            },
        }
    }
}

/// The sprites of the distant lights `lists` hold, each list with its size class, seen from `eye`, their stated intensity times the settings' gain. A light drawn as a volume keeps its sprite: the volume lights what stands around it, the sprite is the glow of its source.
pub fn sprites<'a>(
    lists: impl IntoIterator<Item = (&'a [Distant], u16)>,
    eye: Vec3,
    reach: &DistantReach,
) -> Vec<DistantLight> {
    if reach.gain <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (list, category) in lists {
        let c = usize::from(category).min(2);
        let (range, near, far) = (reach.range[c], reach.near[c], reach.far[c]);
        for light in list {
            let position = Vec3::from(light.position);
            let distance = position.distance(eye);
            if distance > range || light.intensity <= 0.0 {
                continue;
            }
            // The pass fades a light in by the square of how far it has come across its class's fade.
            let across = ((distance - near) / (far - near).max(1e-3)).clamp(0.0, 1.0);
            let fade = across * across;
            if fade <= 0.0 {
                continue;
            }
            out.push(DistantLight {
                position,
                colour: Vec3::from(light.rgb.map(|c| f32::from(c) / 255.0)),
                strength: light.intensity * reach.gain * fade,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reach() -> DistantReach {
        DistantReach {
            range: [3000.0, 6500.0, 10000.0],
            near: [30.0, 37.0, 70.0],
            far: [37.0, 100.0, 150.0],
            gain: 20.0,
            look: SpriteLook::default(),
        }
    }

    fn light(x: f32, intensity: f32) -> Distant {
        Distant {
            position: [x, 0.0, 0.0],
            rgb: [255, 128, 0],
            intensity,
            light_type: 1,
        }
    }

    #[test]
    fn a_sprite_carries_its_intensity_times_the_gain_and_its_colour() {
        let list = [light(400.0, 120.0)];
        let out = sprites([(&list[..], 0)], Vec3::ZERO, &reach());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, Vec3::new(400.0, 0.0, 0.0));
        assert_eq!(out[0].colour, Vec3::new(1.0, 128.0 / 255.0, 0.0));
        assert!((out[0].strength - 120.0 * 20.0).abs() < 1e-3, "{out:?}");
    }

    #[test]
    fn a_sprite_comes_in_by_the_square_of_its_class_fade_and_goes_past_its_class_range() {
        let list = [
            light(20.0, 1.0),
            light(33.5, 1.0),
            light(50.0, 1.0),
            light(3001.0, 1.0),
        ];
        let small = sprites([(&list[..], 0)], Vec3::ZERO, &reach());
        let strengths: Vec<f32> = small.iter().map(|s| s.strength).collect();
        assert_eq!(strengths.len(), 2, "{strengths:?}");
        assert!(
            (strengths[0] / strengths[1] - 0.25).abs() < 1e-3,
            "{strengths:?}"
        );
        let large = sprites([(&list[..], 2)], Vec3::ZERO, &reach());
        assert_eq!(
            large.len(),
            1,
            "only the one at 3,001 m is past the large class's fade"
        );
        let none = sprites([(&list[..], 0)], Vec3::ZERO, &DistantReach::default());
        assert!(none.is_empty());
    }

    #[test]
    fn the_settings_state_the_gain_and_the_look() {
        let settings = VisualSettings::parse(
            "distantlights.streetlight.HDRIntensity 20\ndistantlights.lightScreenCoverage 0.02\n\
             distantlights.lightWorldArea 0.05\ndistantlights.transmittanceScale 0.7\n",
        );
        let reach = DistantReach::of(&settings);
        assert_eq!(reach.gain, 20.0);
        assert_eq!(reach.look.coverage, 0.02);
        assert_eq!(reach.look.area, 0.05);
        assert!((reach.look.extinction - 0.0007).abs() < 1e-9);
        assert_eq!(reach.range, [0.0; 3], "no range, no sprite");
    }
}
