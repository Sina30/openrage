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
        let cos = |what: &str, default: f32| get(what, default).to_radians().cos();
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
                offset: get("offsetAlongLightDirection", 0.0),
                cones: [
                    cos("fadeInnerAngleStart", 0.0),
                    cos("fadeOuterAngleStart", 180.0),
                    cos("fadeInnerAngleEnd", 0.0),
                    cos("fadeOuterAngleEnd", 180.0),
                ],
                cone_distances: [
                    get("angularFadeOutDistanceStart", 0.0),
                    get("angularFadeOutDistanceEnd", 1.0),
                ],
                twinkle: [
                    get("twinkleDistanceMin", 0.0),
                    get("twinkleDistanceMax", 1.0),
                    get("twinkleAmount", 0.0),
                ],
            },
        }
    }
}

/// The light type of a spot in a distant list, the one kind whose sprite shines one way.
const SPOT: u8 = 2;

/// The sprites of the distant lights `lists` hold, each list with its size class and the way each of its lights points where the map of its level-of-detail lights is read, seen from `eye`, their stated intensity times the settings' gain. A light drawn as a volume keeps its sprite: the volume lights what stands around it, the sprite is the glow of its source.
pub fn sprites<'a>(
    lists: impl IntoIterator<Item = (&'a [Distant], u16, Option<&'a [[f32; 3]]>)>,
    eye: Vec3,
    reach: &DistantReach,
) -> Vec<DistantLight> {
    if reach.gain <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (list, category, directions) in lists {
        // The two lists stand one for one, or not at all.
        let directions = directions.filter(|d| d.len() == list.len());
        let c = usize::from(category).min(2);
        let (range, near, far) = (reach.range[c], reach.near[c], reach.far[c]);
        for (i, light) in list.iter().enumerate() {
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
            let direction = match (light.light_type, directions) {
                (SPOT, Some(d)) => Vec3::from(d[i]).normalize_or_zero(),
                _ => Vec3::ZERO,
            };
            out.push(DistantLight {
                position,
                colour: Vec3::from(light.rgb.map(|c| f32::from(c) / 255.0)),
                strength: light.intensity * reach.gain * fade,
                direction,
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
        let out = sprites([(&list[..], 0, None)], Vec3::ZERO, &reach());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, Vec3::new(400.0, 0.0, 0.0));
        assert_eq!(out[0].colour, Vec3::new(1.0, 128.0 / 255.0, 0.0));
        assert!((out[0].strength - 120.0 * 20.0).abs() < 1e-3, "{out:?}");
        assert_eq!(out[0].direction, Vec3::ZERO);
    }

    #[test]
    fn a_spot_takes_the_way_its_level_of_detail_light_points_and_a_point_none() {
        let mut spot = light(400.0, 1.0);
        spot.light_type = 2;
        let list = [spot, light(500.0, 1.0)];
        let down = [[0.0, 0.0, -2.0], [0.0, 0.0, -1.0]];
        let out = sprites([(&list[..], 0, Some(&down[..]))], Vec3::ZERO, &reach());
        let ways: Vec<Vec3> = out.iter().map(|s| s.direction).collect();
        assert_eq!(ways, [Vec3::NEG_Z, Vec3::ZERO]);
        // Lists that do not stand one for one give no way at all.
        let short = sprites([(&list[..], 0, Some(&down[..1]))], Vec3::ZERO, &reach());
        assert!(short.iter().all(|s| s.direction == Vec3::ZERO));
    }

    #[test]
    fn a_sprite_comes_in_by_the_square_of_its_class_fade_and_goes_past_its_class_range() {
        let list = [
            light(20.0, 1.0),
            light(33.5, 1.0),
            light(50.0, 1.0),
            light(3001.0, 1.0),
        ];
        let small = sprites([(&list[..], 0, None)], Vec3::ZERO, &reach());
        let strengths: Vec<f32> = small.iter().map(|s| s.strength).collect();
        assert_eq!(strengths.len(), 2, "{strengths:?}");
        assert!(
            (strengths[0] / strengths[1] - 0.25).abs() < 1e-3,
            "{strengths:?}"
        );
        let large = sprites([(&list[..], 2, None)], Vec3::ZERO, &reach());
        assert_eq!(
            large.len(),
            1,
            "only the one at 3,001 m is past the large class's fade"
        );
        let none = sprites([(&list[..], 0, None)], Vec3::ZERO, &DistantReach::default());
        assert!(none.is_empty());
    }

    #[test]
    fn the_settings_state_the_gain_and_the_look() {
        let settings = VisualSettings::parse(
            "distantlights.streetlight.HDRIntensity 20\ndistantlights.lightScreenCoverage 0.02\n\
             distantlights.lightWorldArea 0.05\ndistantlights.transmittanceScale 0.7\n\
             distantlights.fadeOuterAngleEnd 120.0\ndistantlights.twinkleAmount 100\n\
             distantlights.offsetAlongLightDirection 0.1\n",
        );
        let reach = DistantReach::of(&settings);
        assert!(
            (reach.look.cones[3] + 0.5).abs() < 1e-6,
            "{:?}",
            reach.look.cones
        );
        assert_eq!((reach.look.twinkle[2], reach.look.offset), (100.0, 0.1));
        assert_eq!(reach.gain, 20.0);
        assert_eq!(reach.look.coverage, 0.02);
        assert_eq!(reach.look.area, 0.05);
        assert!((reach.look.extinction - 0.0007).abs() < 1e-9);
        assert_eq!(reach.range, [0.0; 3], "no range, no sprite");
    }
}
