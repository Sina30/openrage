//! The distant lights the frame draws as sprites: every light of the distant lists within its size
//! class's reach that is not drawn as a volume, as bright as its stated intensity seen from afar.

use glam::Vec3;
use or_io::gta5::data::visual_settings::VisualSettings;
use or_renderer::game_shaders::distant_lights::{distance_gain, dot_coverage, SpriteSize};
use or_renderer::DistantLight;

use super::lights_frame::Places;
use super::lights_lod::Distant;

/// How far the distant lights of each size class (small, medium, large) are drawn, the distances over which they fade in, and how large a sprite is; the default draws nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DistantReach {
    pub range: [f32; 3],
    /// ⚠ Inferred: the settings' `fadeNear` and `fadeFar`, read as metres, the sprite coming in between them.
    pub near: [f32; 3],
    pub far: [f32; 3],
    pub size: SpriteSize,
}

impl DistantReach {
    /// As the install's visual settings state it; nought, which draws no sprite, where they do not.
    pub fn of(settings: &VisualSettings) -> Self {
        let size = ["small", "medium", "large"];
        let get =
            |s: &str, what: &str| settings.get_f32(&format!("lodlight.distant.{s}.{what}"), 0.0);
        let upscale = settings.get_f32("distantlights.sizeUpscale", 1.0);
        let dist = settings.get_f32("distantlights.sizeDist", 0.0);
        Self {
            range: size.map(|s| get(s, "range")),
            near: size.map(|s| get(s, "fadeNear")),
            far: size.map(|s| get(s, "fadeFar")),
            size: SpriteSize {
                side: settings.get_f32("distantlights.size", 0.0),
                // ⚠ Inferred: the pass grows a sprite by its distance along the view times this, past one; the settings' upscale is reached at their distance.
                per_metre: if dist > 0.0 { upscale / dist } else { 0.0 },
            },
        }
    }
}

/// The sprites of the distant lights `lists` hold, each list with its size class, seen from `eye`, their stated intensity times `scale` (the frame's units for a stated intensity, which already holds the diffuse 1/pi). A light `kept` holds as a volume shows as much less of its sprite as the volume's reach leaves of it.
pub fn sprites<'a>(
    lists: impl IntoIterator<Item = (&'a [Distant], u16)>,
    eye: Vec3,
    scale: f32,
    reach: &DistantReach,
    kept: &Places,
) -> Vec<DistantLight> {
    let side = reach.size.side;
    if !(side > 0.0 && scale > 0.0) {
        return Vec::new();
    }
    // A source of intensity I seen at d lights the eye by I/d²: a sprite of side s at d spans s²/d², so its
    // radiance is I over s² and over how much of its square its shape fills. The frame's radiance is the
    // stated one times the exposure, which `scale` holds over pi.
    let per_intensity = scale * std::f32::consts::PI / (side * side * dot_coverage());
    // No light past the farthest one kept is kept.
    let kept_within = kept.farthest_from(eye);
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
            let volume = match distance <= kept_within {
                true => kept.near(position).unwrap_or(0.0),
                false => 0.0,
            };
            let fade =
                ((distance - near) / (far - near).max(1e-3)).clamp(0.0, 1.0) * (1.0 - volume);
            if fade <= 0.0 {
                continue;
            }
            // The pass's own gain with distance is taken back out, so that a sprite lights the eye as its source does.
            out.push(DistantLight {
                position,
                colour: Vec3::from(light.rgb.map(|c| f32::from(c) / 255.0)),
                strength: light.intensity * per_intensity * fade / distance_gain(distance),
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
            size: SpriteSize {
                side: 4.0,
                per_metre: 0.002,
            },
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

    fn none_kept() -> Places {
        Places::of([])
    }

    #[test]
    fn a_sprite_lights_the_eye_as_its_source_would_from_where_it_stands() {
        for d in [400.0f32, 1000.0, 2900.0] {
            let list = [light(d, 120.0)];
            let out = sprites([(&list[..], 0)], Vec3::ZERO, 0.5, &reach(), &none_kept());
            assert_eq!(out.len(), 1);
            let sprite = out[0];
            assert_eq!(sprite.position, Vec3::new(d, 0.0, 0.0));
            assert_eq!(sprite.colour, Vec3::new(1.0, 128.0 / 255.0, 0.0));
            // The pass's radiance, gain included, over the angle the sprite spans is the source's I/d² in the frame's units.
            let seen = sprite.strength * distance_gain(d) * dot_coverage() * 16.0 / (d * d);
            let wanted = 120.0 * 0.5 * std::f32::consts::PI / (d * d);
            assert!(
                (seen - wanted).abs() < 1e-4 * wanted,
                "{d}: {seen} {wanted}"
            );
        }
    }

    #[test]
    fn a_sprite_comes_in_over_its_class_fade_and_goes_past_its_class_range() {
        let list = [
            light(20.0, 1.0),
            light(33.5, 1.0),
            light(50.0, 1.0),
            light(3001.0, 1.0),
        ];
        let small = sprites([(&list[..], 0)], Vec3::ZERO, 1.0, &reach(), &none_kept());
        let strengths: Vec<f32> = small.iter().map(|s| s.strength).collect();
        assert_eq!(strengths.len(), 2, "{strengths:?}");
        assert!(
            (strengths[0] / strengths[1] - 0.5).abs() < 1e-3,
            "{strengths:?}"
        );
        let large = sprites([(&list[..], 2)], Vec3::ZERO, 1.0, &reach(), &none_kept());
        assert_eq!(
            large.len(),
            1,
            "only the one at 3,001 m is past the large class's fade"
        );
        let none = sprites(
            [(&list[..], 0)],
            Vec3::ZERO,
            1.0,
            &DistantReach::default(),
            &none_kept(),
        );
        assert!(none.is_empty());
    }

    #[test]
    fn a_light_drawn_as_a_volume_shows_its_sprite_only_where_its_reach_fades_it() {
        let list = [light(200.0, 1.0), light(300.0, 1.0), light(400.0, 1.0)];
        let kept = Places::of([
            (Vec3::new(200.0, 0.0, 0.0), 1.0),
            (Vec3::new(300.01, 0.0, 0.0), 0.25),
        ]);
        let out = sprites([(&list[..], 0)], Vec3::ZERO, 1.0, &reach(), &kept);
        let xs: Vec<f32> = out.iter().map(|s| s.position.x).collect();
        assert_eq!(xs, [300.0, 400.0], "the whole volume shows no sprite");
        let ratio = out[0].strength / out[1].strength;
        assert!((ratio - 0.75).abs() < 1e-3, "{ratio}");
    }
}
