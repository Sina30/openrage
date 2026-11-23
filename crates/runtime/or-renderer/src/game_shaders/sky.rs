//! The colour of the sky in every direction at a moment of the day, and the dual-paraboloid map of it that a surface's reflections read.

use std::collections::BTreeMap;

use glam::Vec3;
use or_io::gta5::data::timecycle::Keyframe;

/// The sky of one moment: the colours the timecycle gives it, as the sky's own shader blends them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sky {
    east: Vec3,
    west: Vec3,
    transition: Vec3,
    transition_position: f32,
    zenith: Vec3,
    zenith_transition: Vec3,
    zenith_transition_position: f32,
    zenith_transition_east: f32,
    zenith_transition_west: f32,
    /// One less the share of the way up the sky at which the zenith colour is whole.
    one_minus_blend_start: f32,
    hdr: f32,
    /// The intensity the sky is drawn at itself, which is not what its reflections are lit by.
    drawn_hdr: f32,
    /// The way towards the sun.
    toward_sun: Vec3,
    sun_colour: Vec3,
    /// The colour of the sun's glow in the sky as drawn itself.
    drawn_sun_colour: Vec3,
    /// The colour of the disc of the sun, and how big it is.
    disc_colour: Vec3,
    disc_size: f32,
    /// Where the sun and the moon stand as the game draws them: a way towards each, which the light the day is lit by does not always follow.
    drawn_sun: Vec3,
    moon: Vec3,
    /// The intensity of the sun and of the stars, and what the moon draws with: its colour, intensity and size.
    sun_hdr: f32,
    stars_intensity: f32,
    moon_colour: Vec3,
    moon_intensity: f32,
    moon_disc_size: f32,
    /// How far the glow round the sun reaches and how strong it is, and the same for the moon.
    effects: [f32; 4],
    /// The game's cycle time, in days: it sets the moon's phase and how far the clouds have drifted.
    time: f32,
    clouds: Clouds,
    phase_two: f32,
    phase_square_plus_one: f32,
    constant_times_scatter: f32,
    mie_intensity: f32,
    /// What the ground gives back where the sky would be below the horizon.
    ground: Vec3,
}

/// A colour of the timecycle as it states it, which is `default` where it states none.
fn rgb(keyframe: &Keyframe, stem: &str, default: f32) -> Vec3 {
    Vec3::new(
        keyframe.get_or(&format!("{stem}_r"), default),
        keyframe.get_or(&format!("{stem}_g"), default),
        keyframe.get_or(&format!("{stem}_b"), default),
    )
}

/// A colour of the timecycle as the sky takes it: squared, so that it is linear, and times its intensity.
fn linear(keyframe: &Keyframe, name: &str, intensity: &str) -> Vec3 {
    let c = Vec3::new(
        keyframe.get_or(&format!("{name}_r"), 0.0),
        keyframe.get_or(&format!("{name}_g"), 0.0),
        keyframe.get_or(&format!("{name}_b"), 0.0),
    );
    c * c * keyframe.get_or(intensity, 1.0)
}

/// What the sky shader cuts its clouds from: the colours and strengths the timecycle gives them, and how the noise they are cut from is made.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Clouds {
    base_minus_mid: Vec3,
    mid: Vec3,
    shadow_minus_base: Vec3,
    detail: [f32; 4],
    large: [f32; 4],
    shading: [f32; 4],
    small: [f32; 4],
    small_colour: Vec3,
    /// How often the noise repeats, how big it is, where it turns to cloud and how softly, and how much denser a second reading of it is.
    noise: [f32; 5],
}

impl Clouds {
    fn of(keyframe: &Keyframe) -> Self {
        let get = |name: &str, default: f32| keyframe.get_or(name, default);
        let colour = |stem: &str| rgb(keyframe, stem, 0.5);
        let (base, shadow, mid, small) = (
            colour("sky_cloud_base_col"),
            colour("sky_cloud_shadow_col"),
            colour("sky_cloud_mid_col"),
            colour("sky_small_cloud_col"),
        );
        let shadow_strength = get("sky_cloud_shadow_strength", 0.2);
        // The sizes of the detail laid over a cloud are the visual settings' own, which the install states as these
        const EDGE_DETAIL_SCALE: f32 = 16.0;
        const OVERALL_DETAIL_SCALE: f32 = 8.0;
        Self {
            base_minus_mid: base * base - mid * mid,
            mid,
            shadow_minus_base: (shadow * shadow - base * base) * shadow_strength,
            detail: [
                get("sky_cloud_edge_strength", 0.5),
                EDGE_DETAIL_SCALE,
                get("sky_cloud_overall_strength", 0.5),
                OVERALL_DETAIL_SCALE,
            ],
            large: [
                get("sky_cloud_base_strength", 0.9),
                get("sky_cloud_density_mult", 2.24),
                get("sky_cloud_density_bias", 0.66),
                get("sky_cloud_fadeout", 0.3),
            ],
            shading: [
                shadow_strength,
                get("sky_cloud_offset", 0.1),
                get("sky_cloud_overall_color", 0.5),
                get("sky_cloud_hdr", 0.8),
            ],
            small: [
                get("sky_small_cloud_detail_scale", 3.6),
                get("sky_small_cloud_detail_strength", 0.5),
                get("sky_small_cloud_density_mult", 1.7),
                get("sky_small_cloud_density_bias", 0.558),
            ],
            small_colour: small * get("sky_cloud_hdr", 0.8),
            noise: [
                get("sky_cloud_gen_frequency", 2.0),
                get("sky_cloud_gen_scale", 64.0),
                get("sky_cloud_gen_threshold", 0.65),
                get("sky_cloud_gen_softness", 0.2),
                get("sky_cloud_gen_density_offset", 0.15),
            ],
        }
    }
}

fn mix(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

impl Sky {
    /// The sky `keyframe` states with the sun in the direction `toward_sun`, and the ground sending back `ground`.
    ///
    /// A reflection takes its light times what the moment says reflections are lit by.
    pub fn new(keyframe: &Keyframe, toward_sun: Vec3, ground: Vec3) -> Self {
        let get = |name: &str, default: f32| keyframe.get_or(name, default);
        let reflection = get("reflection_hdr_mult", 1.0);
        let sun = Vec3::new(
            get("sky_sun_col_r", 1.0),
            get("sky_sun_col_g", 1.0),
            get("sky_sun_col_b", 1.0),
        );
        let disc = Vec3::new(
            get("sky_sun_disc_col_r", 0.0),
            get("sky_sun_disc_col_g", 0.0),
            get("sky_sun_disc_col_b", 0.0),
        );
        let moon = Vec3::new(
            get("sky_moon_col_r", 1.0),
            get("sky_moon_col_g", 1.0),
            get("sky_moon_col_b", 1.0),
        );
        // The shape of the sun's glow: a phase function of the angle to it
        let g = -0.49 - get("sky_sun_miephase", 0.0) * 0.5;
        let g2 = g * g;
        let scatter = get("sky_sun_miescatter", 0.0) / 1000.0;
        Self {
            east: linear(
                keyframe,
                "sky_azimuth_east_col",
                "sky_azimuth_east_col_inten",
            ),
            west: linear(
                keyframe,
                "sky_azimuth_west_col",
                "sky_azimuth_west_col_inten",
            ),
            transition: linear(
                keyframe,
                "sky_azimuth_transition_col",
                "sky_azimuth_transition_col_inten",
            ),
            transition_position: get("sky_azimuth_transition_position", 0.5),
            zenith: linear(keyframe, "sky_zenith_col", "sky_zenith_col_inten"),
            zenith_transition: linear(
                keyframe,
                "sky_zenith_transition_col",
                "sky_zenith_transition_col_inten",
            ),
            zenith_transition_position: get("sky_zenith_transition_position", 0.8),
            zenith_transition_east: get("sky_zenith_transition_east_blend", 0.5),
            zenith_transition_west: get("sky_zenith_transition_west_blend", 0.5),
            one_minus_blend_start: 1.0 - get("sky_zenith_blend_start", 0.0),
            hdr: get("sky_hdr", 1.0) * reflection,
            drawn_hdr: get("sky_hdr", 1.0),
            toward_sun: toward_sun.normalize_or_zero(),
            sun_colour: sun * sun * get("sky_sun_hdr", 1.0) * reflection,
            drawn_sun_colour: sun * sun * get("sky_sun_hdr", 1.0),
            disc_colour: disc * disc * get("sky_sun_hdr", 1.0),
            disc_size: get("sky_sun_disc_size", 2.5),
            drawn_sun: toward_sun.normalize_or_zero(),
            moon: -toward_sun.normalize_or_zero(),
            sun_hdr: get("sky_sun_hdr", 1.0),
            stars_intensity: get("sky_stars_iten", 1.0),
            moon_colour: moon * moon,
            moon_intensity: get("sky_moon_iten", 1.0),
            moon_disc_size: get("sky_moon_disc_size", 2.5),
            time: 13.0,
            clouds: Clouds::of(keyframe),
            effects: [
                get("sky_sun_influence_radius", 5.0),
                get("sky_sun_scatter_inten", 2.0),
                get("sky_moon_influence_radius", 5.0),
                get("sky_moon_scatter_inten", 2.0),
            ],
            phase_two: g * 2.0,
            phase_square_plus_one: g2 + 1.0,
            constant_times_scatter: scatter * (1.5 * ((1.0 - g2) / (2.0 + g2))),
            mie_intensity: get("sky_sun_mie_intensity_mult", 1.0),
            ground,
        }
    }

    /// The variables of the game's own sky shader, by the names it gives them, with the horizon `horizon_level` high in the world.
    pub fn constants(&self, horizon_level: f32) -> BTreeMap<&'static str, Vec<f32>> {
        let three = |v: Vec3| v.to_array().to_vec();
        // How fast the detail laid over a cloud and the large noise move, in the visual settings' own unit per day
        const DETAIL_SPEED: f32 = 1.0;
        const LARGE_SPEED: f32 = 5.0;
        let drift = |speed: f32| (0.5 + speed * self.time / 11.0).rem_euclid(1.0);
        BTreeMap::from([
            ("azimuthEastColor", three(self.east)),
            ("azimuthWestColor", three(self.west)),
            ("azimuthTransitionColor", three(self.transition)),
            ("azimuthTransitionPosition", vec![self.transition_position]),
            ("zenithColor", three(self.zenith)),
            ("zenithTransitionColor", three(self.zenith_transition)),
            (
                "zenithConstants",
                vec![
                    self.zenith_transition_position,
                    self.zenith_transition_east,
                    self.zenith_transition_west,
                    self.one_minus_blend_start,
                ],
            ),
            ("hdrIntensity", vec![self.drawn_hdr]),
            ("sunColorHdr", three(self.drawn_sun_colour)),
            ("sunDiscColorHdr", three(self.disc_colour)),
            (
                "sunConstants",
                vec![
                    self.phase_two,
                    self.phase_square_plus_one,
                    self.constant_times_scatter,
                    self.mie_intensity,
                ],
            ),
            ("sunDirection", three(self.drawn_sun)),
            ("moonDirection", three(self.moon)),
            ("moonColor", three(self.moon_colour)),
            ("moonIntensity", vec![self.moon_intensity]),
            ("starfieldIntensity", vec![self.stars_intensity]),
            ("effectsConstants", self.effects.to_vec()),
            ("lunarCycle", three(self.lunar_cycle())),
            ("cloudBaseMinusMidColour", three(self.clouds.base_minus_mid)),
            ("cloudMidColour", three(self.clouds.mid)),
            (
                "cloudShadowMinusBaseColourTimesShadowStrength",
                three(self.clouds.shadow_minus_base),
            ),
            ("cloudDetailConstants", self.clouds.detail.to_vec()),
            ("cloudConstants1", self.clouds.large.to_vec()),
            ("cloudConstants2", self.clouds.shading.to_vec()),
            ("smallCloudConstants", self.clouds.small.to_vec()),
            ("smallCloudColorHdr", three(self.clouds.small_colour)),
            ("speedConstants", vec![drift(DETAIL_SPEED); 3]),
            ("noiseFrequency", vec![self.clouds.noise[0]]),
            ("noiseScale", vec![self.clouds.noise[1]]),
            ("noiseThreshold", vec![self.clouds.noise[2]]),
            ("noiseSoftness", vec![self.clouds.noise[3]]),
            ("noiseDensityOffset", vec![self.clouds.noise[4]]),
            (
                "noisePhase",
                vec![(LARGE_SPEED * self.time * 0.24).rem_euclid(1.0); 2],
            ),
            ("horizonLevel", vec![horizon_level]),
        ])
    }

    /// Put the sun and the moon where the game draws them, a way towards each, in place of where the light of the day comes from.
    pub fn placed(self, sun: Vec3, moon: Vec3) -> Self {
        Self {
            drawn_sun: sun.normalize_or_zero(),
            moon: moon.normalize_or_zero(),
            ..self
        }
    }

    /// Set the game's cycle time, in days, which sets the moon's phase and how far the clouds have drifted.
    pub fn with_time(self, time: f32) -> Self {
        Self { time, ..self }
    }

    /// The direction the moon's light falls on its disc from, turned about the vertical by the cycle: from the one side at the new moon to the other at the end of it.
    fn lunar_cycle(&self) -> Vec3 {
        const START: f32 = 2.90;
        const ADJUST: f32 = -0.2;
        let angle =
            -START + (self.time.rem_euclid(27.0) / 26.0).clamp(0.0, 1.0) * (2.0 * START + ADJUST);
        let (sin, cos) = angle.sin_cos();
        Vec3::new(-sin, cos, 0.0)
    }

    /// The way towards the sun as the game draws it.
    pub fn sun_direction(&self) -> Vec3 {
        self.drawn_sun
    }

    /// The way towards the moon as the game draws it, and its disc: its colour, its size in the game's own unit, and how strong it is.
    pub fn moon(&self) -> (Vec3, Vec3, f32, f32) {
        (
            self.moon,
            self.moon_colour,
            self.moon_disc_size,
            self.moon_intensity,
        )
    }

    /// Which of the sky shader's passes draws this sky, as the game picks it: by what is up (bit 8 the sun, 4 the moon, 2 the clouds, 1 the stars), with the stars and the clouds left out where `stars` and `clouds` say there is no map of them to draw.
    pub fn variant(&self, stars: bool, clouds: bool) -> usize {
        let up = |way: Vec3| way.z >= -1e-6;
        let mut pass = 0;
        if stars && self.stars_intensity >= 1e-4 {
            pass |= 1;
        }
        if clouds {
            pass |= 2;
        }
        if self.moon_intensity >= 1e-4 && up(self.moon) {
            pass |= 4;
        }
        if self.sun_hdr >= 1e-4 && up(self.drawn_sun) {
            pass |= 8;
        }
        pass
    }

    /// The disc of the sun: its colour, and how big it is in the game's own unit.
    pub fn disc(&self) -> (Vec3, f32) {
        (self.disc_colour, self.disc_size)
    }

    /// The colour of the atmosphere towards `view`, which has no sun in it.
    fn atmosphere(&self, view: Vec3) -> Vec3 {
        // East is the way the first axis points, and the blend runs from there to the west
        let azimuth_blend = (-view.x * 0.5 + 0.5).max(0.0).sqrt();
        let zenith_blend = view.z.abs();
        let position = self.transition_position.max(1e-4);
        let azimuth = if azimuth_blend < position {
            mix(self.east, self.transition, azimuth_blend / position)
        } else {
            mix(
                self.transition,
                self.west,
                (azimuth_blend - position) / (1.0 - position).max(1e-4),
            )
        };
        let transition_blend = self.zenith_transition_east
            + (self.zenith_transition_west - self.zenith_transition_east) * azimuth_blend;
        let transition = mix(azimuth, self.zenith_transition, transition_blend);
        let up = self.zenith_transition_position.max(1e-4);
        if zenith_blend < up {
            mix(azimuth, transition, zenith_blend / up)
        } else {
            let to_top = ((zenith_blend - up) / (1.0 - up).max(1e-4)).clamp(0.0, 1.0);
            mix(
                transition,
                self.zenith,
                (to_top / self.one_minus_blend_start.max(1e-4)).clamp(0.0, 1.0),
            )
        }
    }

    /// The colour of the sky towards `view`, a unit vector with the third axis up: the atmosphere, the glow of the sun in it, and below the horizon the ground.
    pub fn colour(&self, view: Vec3) -> Vec3 {
        // The sky's phase is worked out from the angle to the way the light travels, which is away from the sun
        let cos = -view.dot(self.toward_sun);
        let phase = (1.0 + cos * cos)
            / (self.phase_square_plus_one - self.phase_two * cos)
                .max(1e-4)
                .powf(1.5)
            * self.constant_times_scatter;
        let sky = self.atmosphere(view) * self.hdr * (1.0 - phase.clamp(0.0, 1.0))
            + self.sun_colour * phase.clamp(0.0, 1.0) * self.mie_intensity;
        // The ground takes over under the horizon over a little way, as a horizon is not a line
        let below = (-view.z / 0.2).clamp(0.0, 1.0);
        mix(sky, self.ground, below * below * (3.0 - 2.0 * below))
    }
}

/// The direction a texel of a dual-paraboloid map looks towards, in the map of two paraboloids side by side: the upper hemisphere on the right and the lower on the left.
pub fn paraboloid_direction(u: f32, v: f32) -> Option<Vec3> {
    let upper = u >= 0.5;
    let s = if upper {
        glam::Vec2::new((u - 0.75) / 0.25, (0.5 - v) / 0.5)
    } else {
        glam::Vec2::new((0.25 - u) / 0.25, (0.5 - v) / 0.5)
    };
    let squared = s.length_squared();
    if squared > 1.0 {
        return None;
    }
    let z = (1.0 - squared) / (1.0 + squared);
    let xy = s * (2.0 / (1.0 + squared));
    Some(Vec3::new(xy.x, xy.y, if upper { z } else { -z }))
}

/// The map of the sky that reflections read: the levels of a texture, each half the size of the one before, as linear colour with an alpha of one.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionMap {
    pub width: u32,
    pub height: u32,
    /// The texels of each level, row by row.
    pub levels: Vec<Vec<[f32; 4]>>,
}

impl ReflectionMap {
    /// The map of `sky`, `width` texels across and half as many down, with `levels` levels.
    pub fn of(sky: &Sky, width: u32, levels: usize) -> Self {
        let height = (width / 2).max(1);
        let mut first = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let u = (x as f32 + 0.5) / width as f32;
                let v = (y as f32 + 0.5) / height as f32;
                // A texel outside both paraboloids holds what the nearest edge does
                let colour = paraboloid_direction(u, v)
                    .map(|d| sky.colour(d))
                    .unwrap_or_else(|| sky.colour(Vec3::new(1.0, 0.0, 0.0).cross(Vec3::Z)));
                first.push([colour.x, colour.y, colour.z, 1.0]);
            }
        }
        let mut map = Self {
            width,
            height,
            levels: vec![first],
        };
        while map.levels.len() < levels {
            let (w, h) = map.size_of(map.levels.len() - 1);
            if w == 1 && h == 1 {
                break;
            }
            let below = map.halved(map.levels.len() - 1);
            map.levels.push(below);
        }
        map
    }

    /// The width and the height of level `level`.
    pub fn size_of(&self, level: usize) -> (u32, u32) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }

    /// Level `level` with each texel the average of four.
    fn halved(&self, level: usize) -> Vec<[f32; 4]> {
        let (w, h) = self.size_of(level);
        let (nw, nh) = self.size_of(level + 1);
        let from = &self.levels[level];
        let at = |x: u32, y: u32| from[(y.min(h - 1) * w + x.min(w - 1)) as usize];
        let mut out = Vec::with_capacity((nw * nh) as usize);
        for y in 0..nh {
            for x in 0..nw {
                let taps = [
                    at(2 * x, 2 * y),
                    at(2 * x + 1, 2 * y),
                    at(2 * x, 2 * y + 1),
                    at(2 * x + 1, 2 * y + 1),
                ];
                let mut sum = [0.0f32; 4];
                for tap in taps {
                    for (s, t) in sum.iter_mut().zip(tap) {
                        *s += t * 0.25;
                    }
                }
                out.push(sum);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
