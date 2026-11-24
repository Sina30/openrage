//! The light of one moment of the day as the shared constant buffer holds it: the sun or the moon, the ambient lights and the fog.

use std::f32::consts::PI;

use glam::{Mat4, Vec3, Vec4};
use or_io::gta5::data::timecycle::{Keyframe, TimeTable};

use super::tone::{Tone, ToneSettings};

/// The hour the sun rises, and the hour it sets; the moon takes over an hour and a half after.
const SUN_RISE: f32 = 6.0;
const SUN_SET: f32 = 20.0;
const SUN_TO_MOON: f32 = SUN_SET + 1.5;
const MOON_TO_SUN: f32 = SUN_RISE - 1.5;
/// The lowest the directional light's source may sit, as the z of its direction, so that shadows stay short.
const LOWEST_SOURCE: f32 = 0.33;

/// How far the night emissives are on at `hours`, from nought to one: on from the hour `on` through the hour before `off`, fading in over the hour before and out over the last hour, as the game counts the hours of the night.
pub fn night_emissive(hours: f32, on: u32, off: u32) -> f32 {
    let on_hours = 24u32.saturating_sub(on) + off;
    let is_on = |hour: u32| (0..on_hours).any(|i| (i + on) % 24 == hour % 24);
    let hour = hours.max(0.0).floor() as u32 % 24;
    let fade = hours.max(0.0).fract();
    match (is_on(hour), is_on(hour + 1)) {
        (false, true) => fade,
        (true, false) => 1.0 - fade,
        (true, true) => 1.0,
        (false, false) => 0.0,
    }
}

/// A moment of the day, from which the lights are worked out.
#[derive(Debug, Clone, Copy)]
pub struct Moment<'a> {
    pub keyframe: &'a Keyframe,
    pub table: &'a TimeTable,
    /// The hour, 0 to 24.
    pub hours: f32,
    /// Days into the moon's cycle, which sets its place in the sky and its phase.
    pub cycle_time: f32,
    /// How much of its light the moon keeps at the dimmest phase.
    pub moon_dim: f32,
    /// How wet the world is, from nought to one: what the weather leaves on every surface.
    pub wetness: f32,
    /// How far the night emissives are on, from nought by day to one by night.
    pub night_emissive: f32,
    /// The bodies of a day whose keyframe states its light whole; none works them out from the hours.
    pub stated: Option<Stated>,
}

/// Where a day that states its light whole puts the sun and the moon, the way its light comes from, and how much each body shows: its directional light is then faded by nothing more.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stated {
    pub sun: Vec3,
    pub moon: Vec3,
    pub towards_light: Vec3,
    pub sun_fade: f32,
    pub moon_fade: f32,
    /// The linear exposure every light of the day was given at, which the tone curves are blended by.
    pub exposure: f32,
}

/// What the lights need to know of the view: where the camera is and how it projects.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub camera_height: f32,
    pub projection: Mat4,
    pub near: f32,
}

/// A moment of the day with the data it is worked out from, which a renderer holds from frame to frame.
#[derive(Debug, Clone, Default)]
pub struct Day {
    pub keyframe: Keyframe,
    pub table: TimeTable,
    pub hours: f32,
    pub cycle_time: f32,
    pub moon_dim: f32,
    /// How wet the world is, from nought to one, which a wet surface shows in its shade and its shine.
    pub wetness: f32,
    /// How far the night emissives are on, from nought by day to one by night.
    pub night_emissive: f32,
    /// What the install's visual settings state of how the frame is mapped to the screen.
    pub tone: ToneSettings,
    /// The bodies of a day whose keyframe states its light whole.
    pub stated: Option<Stated>,
    /// The sky of a day that states it as a table of radiance, drawn in place of the dome's colours.
    pub sky_view: Option<super::sky_view::SkyView>,
}

impl Day {
    /// How a frame lit by `lighting` is mapped to the screen at this moment.
    pub fn tone(&self, lighting: &Lighting) -> Tone {
        let tone = Tone::new(&self.tone, Some(&self.keyframe), lighting);
        match self.stated {
            Some(stated) => tone.blended_by(stated.exposure),
            None => tone,
        }
    }

    /// How strongly the ambient light is darkened where surfaces stand close around a pixel: nothing where the day states none.
    pub fn occlusion(&self) -> f32 {
        self.keyframe.get_or("ssao_inten", 0.0) * super::ambient::STRENGTH_PER_SETTING
    }

    /// The lights of this day seen from `view`.
    pub fn lighting(&self, view: &View) -> Lighting {
        Lighting::at(
            &Moment {
                keyframe: &self.keyframe,
                table: &self.table,
                hours: self.hours,
                cycle_time: self.cycle_time,
                moon_dim: self.moon_dim,
                wetness: self.wetness,
                night_emissive: self.night_emissive,
                stated: self.stated,
            },
            view,
        )
    }
}

/// The values of the lighting globals, as the game writes them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lighting {
    /// The direction the directional light travels in, and the reflection factor in `w`.
    pub directional_light: Vec4,
    /// The colour of the directional light scaled by its strength.
    pub directional_colour: Vec4,
    /// The natural ambient light from below and from above.
    pub natural_ambient: [Vec4; 2],
    pub artificial_interior_ambient: [Vec4; 2],
    pub artificial_exterior_ambient: [Vec4; 2],
    pub directional_ambient: Vec4,
    /// The least ambient a surface that bakes its own shade keeps, how far it rises above it, and how wet the world is.
    pub dynamic_bakes: Vec4,
    /// Fog: three blocks of distances and densities, then the sun's and the moon's direction with a power each.
    pub fog_params: [Vec4; 5],
    /// Fog colours: the sun's side, the atmosphere, the ground and the moon's side, the haze in each `w`.
    pub fog_colours: [Vec4; 4],
    /// How far the night emissives are on: the game writes it with the scalars every surface reads, and the emissive lights and windows are scaled by it.
    pub night_emissive: f32,
}

fn rgb(k: &Keyframe, stem: &str, default: [f32; 3]) -> Vec3 {
    Vec3::new(
        k.get_or(&format!("{stem}_r"), default[0]),
        k.get_or(&format!("{stem}_g"), default[1]),
        k.get_or(&format!("{stem}_b"), default[2]),
    )
}

fn direction(k: &Keyframe, stem: &str) -> Vec3 {
    Vec3::new(
        k.get_or(&format!("{stem}_x"), 0.0),
        k.get_or(&format!("{stem}_y"), -1.0),
        k.get_or(&format!("{stem}_z"), 0.0),
    )
}

/// Where the sun and the moon are and how much each lights: directions towards them, how much of its light the sun and the moon each give, and the strength of whichever lights the day.
struct Sky {
    sun: Vec3,
    moon: Vec3,
    towards_light: Vec3,
    sun_fade: f32,
    moon_fade: f32,
    strength: f32,
}

fn sky(moment: &Moment<'_>) -> Sky {
    if let Some(stated) = moment.stated {
        return Sky {
            sun: stated.sun.normalize_or_zero(),
            moon: stated.moon.normalize_or_zero(),
            towards_light: stated.towards_light.normalize_or_zero(),
            sun_fade: stated.sun_fade,
            moon_fade: stated.moon_fade,
            strength: 1.0,
        };
    }
    let (hours, table, k) = (moment.hours, moment.table, moment.keyframe);
    // The angle the sun has turned through: up at the sunrise hour, over at the sunset hour
    let angle = if hours < SUN_RISE {
        0.5 * PI * (hours / SUN_RISE)
    } else if hours < SUN_SET {
        0.5 * PI + PI * ((hours - SUN_RISE) / (SUN_SET - SUN_RISE))
    } else {
        1.5 * PI + 0.5 * PI * ((hours - SUN_SET) / (24.0 - SUN_SET))
    };
    let override_light = k.get_or("light_direction_override", 0.0);
    let override_sun = k.get_or("light_direction_override_overrides_sun", 1.0);
    let slope = |roll: f32| Vec3::new(0.0, roll.cos(), roll.sin());
    let sun_slope = slope(table.sun_roll);
    let moon_slope = slope(table.sun_roll + table.moon_roll);

    // The moon turns faster than the sun, and wobbles a little
    let moon_angle = angle + PI * ((moment.cycle_time.rem_euclid(55.0) - 27.0) / 27.0);
    let moon_phase_day = moment.cycle_time.rem_euclid(27.0);
    let wobble = table.moon_wobble_offset
        + (moon_phase_day * 2.0 * PI * table.moon_wobble_frequency).sin()
            * table.moon_wobble_amplitude;
    let mut actual_moon = moon_slope * -moon_angle.cos() + Vec3::X * moon_angle.sin();
    actual_moon += Vec3::X.cross(moon_slope) * wobble;
    let actual_moon = actual_moon.normalize_or_zero();

    let (cos_yaw, sin_yaw) = (table.sun_yaw.cos(), table.sun_yaw.sin());
    let sun_path = (sun_slope * -angle.cos() + Vec3::X * angle.sin()).normalize_or_zero();
    let actual_sun = sun_path * Vec3::new(cos_yaw, cos_yaw, 1.0)
        + Vec3::new(sun_path.y, sun_path.x, 0.0) * Vec3::new(-sin_yaw, sin_yaw, 0.0);

    let over_sun = direction(k, "sun_direction");
    let over_moon = direction(k, "moon_direction");
    let lerp = |a: Vec3, b: Vec3, t: f32| a + (b - a) * t;
    let moon = lerp(actual_moon, over_moon, override_light * override_sun).normalize_or_zero();
    let sun = lerp(actual_sun, over_sun, override_light * override_sun).normalize_or_zero();

    let saturate = |x: f32| x.clamp(0.0, 1.0);
    let (mut sun_fade, mut moon_fade) = (1.0, 0.0);
    if hours > SUN_SET {
        sun_fade = 1.0 - saturate((hours - SUN_TO_MOON) * 4.0);
        moon_fade = saturate((hours - SUN_TO_MOON - 0.25) * 4.0);
    } else if hours < SUN_RISE {
        sun_fade = saturate((hours - MOON_TO_SUN - 0.25) * 4.0);
        moon_fade = 1.0 - saturate((hours - MOON_TO_SUN) * 4.0);
    }
    moon_fade *= saturate(moon.z / 0.1);

    // The light is the sun's and the moon's directions weighted by how much each shows, the sun weighed a hundred to one
    let total = sun_fade * 100.0 + moon_fade;
    let mut towards_light = Vec3::ZERO;
    if total > f32::EPSILON {
        let (sun_weight, moon_weight) = (sun_fade * 100.0 / total, moon_fade / total);
        let actual = actual_sun * sun_weight + actual_moon * moon_weight;
        let over = over_sun * sun_weight + over_moon * moon_weight;
        towards_light = lerp(actual, over, override_light).normalize_or_zero();
    }

    // A moon of the phase furthest from full gives less light
    let mut strength = sun_fade.max(moon_fade);
    let phase = moon_phase_day / 26.0;
    let dim = ((phase - 0.5).abs() * 2.0) * moon_fade;
    strength += (strength * moment.moon_dim - strength) * dim;

    towards_light.z = towards_light.z.max(LOWEST_SOURCE);
    Sky {
        sun,
        moon,
        towards_light: towards_light.normalize_or_zero(),
        sun_fade,
        moon_fade,
        strength,
    }
}

impl Lighting {
    /// The way towards the sun and the way towards the moon, which the sky draws them in and which the light of the day does not always follow.
    pub fn sun_and_moon(&self) -> (Vec3, Vec3) {
        (self.fog_params[3].truncate(), self.fog_params[4].truncate())
    }

    /// The lights of `moment` seen from `view`.
    pub fn at(moment: &Moment<'_>, view: &View) -> Self {
        let k = moment.keyframe;
        let sky = sky(moment);
        // The light travels away from its source
        let travel = -sky.towards_light;

        let direction_multiplier = k.get_or("light_dir_mult", 14.75) * sky.strength;
        let colour = rgb(k, "light_dir_col", [0.890, 0.675, 0.392]);
        let multiplier = if direction_multiplier < 1e-6 || colour.max_element() < 1e-6 {
            0.0
        } else {
            direction_multiplier
        };
        let directional_light = travel.extend(k.get_or("reflection_tweak_directional", 1.0));
        let directional_colour = (colour * multiplier).extend(0.0);

        // The ambient light that bounces up off the ground comes from the sun mirrored in it
        let bounce = {
            let source = sky.towards_light;
            let ground = Vec3::new(source.x, source.y, -source.z);
            let enabled = k.get_or("light_directional_amb_bounce_enabled", 1.0);
            source + (ground - source) * enabled
        };
        let wrap = k.get_or("light_amb_down_wrap", 1.0);
        let natural_down_intensity = k.get_or("light_natural_amb_down_intensity", 6.25);
        let natural_up_intensity = k.get_or("light_natural_amb_up_intensity", 8.5)
            * k.get_or("light_natural_amb_up_intensity_mult", 1.0);
        let natural_ambient = [
            (rgb(k, "light_natural_amb_down_col", [0.541, 0.655, 1.0]) * natural_down_intensity)
                .extend(wrap),
            (rgb(k, "light_natural_amb_up_col", [1.0, 0.847, 0.459]) * natural_up_intensity)
                .extend(1.0 / (1.0 + wrap)),
        ];
        let artificial_interior_ambient = [
            (rgb(k, "light_artificial_int_down_col", [0.541, 0.655, 1.0])
                * k.get_or("light_artificial_int_down_intensity", 0.0))
            .extend(k.get_or("light_natural_push", 0.0)),
            (rgb(k, "light_artificial_int_up_col", [1.0, 0.847, 0.459])
                * k.get_or("light_artificial_int_up_intensity", 0.0))
            .extend(bounce.x),
        ];
        let artificial_exterior_ambient = [
            (rgb(k, "light_artificial_ext_down_col", [0.0; 3])
                * k.get_or("light_artificial_ext_down_intensity", 0.0))
            .extend(bounce.y),
            (rgb(k, "light_artificial_ext_up_col", [0.0; 3])
                * k.get_or("light_artificial_ext_up_intensity", 0.0))
            .extend(bounce.z),
        ];
        let directional_ambient_strength = k.get_or("light_directional_amb_intensity", 0.0)
            * k.get_or("light_directional_amb_intensity_mult", 1.0)
            * sky.strength;
        let directional_ambient = (rgb(k, "light_directional_amb_col", [0.0; 3])
            * directional_ambient_strength)
            .extend(k.get_or("light_ambient_bake_ramp", 1.0));

        // The tweak runs from 2, where a surface keeps the most of its own shade, to -2, the least
        let mix = ((k.get_or("light_dynamic_bake_tweak", 0.0) - 2.0) * -0.25).clamp(0.0, 1.0);
        let bake_min = (0.7 + (0.3 - 0.7) * mix).clamp(0.0, 2.0);
        let bake_max = (1.7 + (1.3 - 1.7) * mix).clamp(0.0, 2.0);
        let dynamic_bakes = Vec4::new(
            bake_min,
            bake_max - bake_min,
            moment.wetness.clamp(0.0, 1.0),
            0.0,
        );

        let (fog_params, fog_colours) = fog(k, &sky, view);
        Self {
            directional_light,
            directional_colour,
            natural_ambient,
            artificial_interior_ambient,
            artificial_exterior_ambient,
            directional_ambient,
            dynamic_bakes,
            fog_params,
            fog_colours,
            night_emissive: moment.night_emissive,
        }
    }

    /// Every variable of the lighting buffer this writes, by name.
    pub fn values(&self) -> Vec<(&'static str, Vec<f32>)> {
        let v = |v: Vec4| v.to_array().to_vec();
        vec![
            ("gDirectionalLight", v(self.directional_light)),
            ("gDirectionalColour", v(self.directional_colour)),
            ("gLightNaturalAmbient0", v(self.natural_ambient[0])),
            ("gLightNaturalAmbient1", v(self.natural_ambient[1])),
            (
                "gLightArtificialIntAmbient0",
                v(self.artificial_interior_ambient[0]),
            ),
            (
                "gLightArtificialIntAmbient1",
                v(self.artificial_interior_ambient[1]),
            ),
            (
                "gLightArtificialExtAmbient0",
                v(self.artificial_exterior_ambient[0]),
            ),
            (
                "gLightArtificialExtAmbient1",
                v(self.artificial_exterior_ambient[1]),
            ),
            ("gDirectionalAmbientColour", v(self.directional_ambient)),
            ("gDynamicBakesAndWetness", v(self.dynamic_bakes)),
            (
                "globalFogParams",
                self.fog_params.iter().flat_map(|p| p.to_array()).collect(),
            ),
            ("globalFogColor", v(self.fog_colours[0])),
            ("globalFogColorE", v(self.fog_colours[1])),
            ("globalFogColorN", v(self.fog_colours[2])),
            ("globalFogColorMoon", v(self.fog_colours[3])),
        ]
    }
}

/// The fog of the moment: its distances and densities, and its colours.
fn fog(k: &Keyframe, sky: &Sky, view: &View) -> ([Vec4; 5], [Vec4; 4]) {
    let start = k.get_or("fog_start", 73.0);
    let end = k.get_or("far_clip", 1500.0).max(start);

    // Where the fog begins, as the depth of the point on the edge of the view at that distance
    let (tan_h, tan_v) = (
        1.0 / view.projection.x_axis.x,
        1.0 / view.projection.y_axis.y,
    );
    let edge = Vec3::new(tan_h, tan_v, -1.0).normalize_or_zero() * start;
    let start_depth = if edge.z < -view.near {
        (edge.z * view.projection.z_axis.z + view.projection.w_axis.z) / -edge.z
    } else {
        0.0
    };

    let near_density = k.get_or("fog_density", 1.0) / 10000.0;
    let near_falloff = k.get_or("fog_falloff", 0.5) / 1000.0;
    let near_alpha = k.get_or("fog_alpha", 1.0);
    let ground = k.get_or("fog_base_height", 0.0);
    // Below the ground of the fog the exponent is held, so that it does not run away
    let density_at_viewer = (-near_falloff * (view.camera_height - ground))
        .min(20.0)
        .exp();
    let far_density = k.get_or("fog_haze_density", 0.5) / 10000.0;
    let far_alpha = k.get_or("fog_haze_alpha", 1.0);
    let tint = k.get_or("fog_horizon_tint_scale", 4.0);
    let haze_start = k.get_or("fog_haze_start", 0.0);

    // The density is stored negated and the viewer's multiplied in, to save the shader work
    let params = [
        Vec4::new(start, end, start_depth, 0.0),
        Vec4::new(
            -far_density,
            far_alpha,
            tint / end,
            -(density_at_viewer * near_density).min(1.0),
        ),
        Vec4::new(
            haze_start,
            near_alpha,
            near_falloff,
            (1.0 - (far_density * end).exp()).clamp(0.0, 1.0) * far_alpha,
        ),
        sky.sun.extend(k.get_or("fog_sun_lighting_calc_pow", 8.0)),
        sky.moon
            .extend(k.get_or("fog_moon_lighting_calc_pow", 18.0)),
    ];

    let hdr = k.get_or("fog_hdr", 1.0);
    let sun = rgb(k, "fog_col", [0.412, 0.584, 0.780]) * hdr;
    let moon = rgb(k, "fog_moon_col", [0.5; 3]) * hdr;
    let mut atmosphere = rgb(k, "fog_east_col", [0.412, 0.584, 0.780]) * hdr;
    let near = rgb(k, "fog_near_col", [0.012, 0.604, 1.0]) * hdr;
    let haze = rgb(k, "fog_haze_col", [0.412, 0.584, 0.780]) * k.get_or("fog_haze_hdr", 1.0);
    // The far colours give way to the near one where the fog is thick at the viewer
    let blend = density_at_viewer.clamp(0.0, 1.0).powf(0.3);
    atmosphere += (near - atmosphere) * blend;
    let fade = sky.moon_fade.max(sky.sun_fade);
    let sun_side = atmosphere + (sun - atmosphere) * fade;
    let colours = [
        sun_side.extend(haze.x),
        atmosphere.extend(haze.y),
        near.extend(haze.z),
        moon.extend(0.0),
    ];
    (params, colours)
}

#[cfg(test)]
mod tests;
