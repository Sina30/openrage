//! How GTA VI exposes its frame: which of the sun and the moon lights and how faded, the exposure
//! curve of its visual settings over a metered luminance with the keyframe's clamps and its
//! adaptation, and the light of a moment that comes of them.

use std::f32::consts::{PI, TAU};

use or_io::gta5::data::timecycle::Keyframe;
use or_io::gta6::timecycle::{FrameLight, DOME_TRANSITION};

use crate::gta6_sky::Atmosphere;

/// The stops the build's curve takes off what its terms give.
const CURVE_OFFSET: f32 = 10.0;
/// The least luminance a curve takes the logarithm of or a key value is divided by.
const LEAST_LUMINANCE: f32 = 1e-6;
/// The grey the stand-in meter sees the world as.
const GREY: f32 = 0.18;
/// The least up part of the way towards the light.
const LOWEST_LIGHT: f32 = 0.15;
/// The lowest a body stands for the sky it lights, which keeps the dusk sky lit while its body is under the horizon.
const LOWEST_SKY_LIGHT: f32 = 0.05;
/// The up part of a way along the horizon, where the dome and the fog take their colours.
const HORIZON: f32 = 0.02;
/// The crossfade a keyframe states for the fades worked out from the zenith angles.
const AUTOMATIC: f32 = -10.0;
/// The zenith angle, in degrees, a threshold the keyframe leaves to the settings stands for.
const DEFAULT_THRESHOLD: f32 = 100.0;

/// The luminance of a colour.
fn luminance(c: [f32; 3]) -> f32 {
    0.2125 * c[0] + 0.7154 * c[1] + 0.0721 * c[2]
}

fn normalised(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    match length > 0.0 {
        true => v.map(|c| c / length),
        false => [0.0, 0.0, 1.0],
    }
}

/// One exposure curve of the visual settings: the stops it gives a metered luminance.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Curve {
    pub scale: f32,
    pub horizontal_offset: f32,
    pub vertical_offset: f32,
    pub bending: f32,
}

impl Curve {
    fn of(get: &dyn Fn(&str, f32) -> f32, prefix: &str) -> Self {
        Self {
            scale: get(&format!("{prefix}scale"), 0.0),
            horizontal_offset: get(&format!("{prefix}horizontalOffset"), 0.0),
            vertical_offset: get(&format!("{prefix}verticalOffset"), 0.0),
            bending: get(&format!("{prefix}bending"), 0.0),
        }
    }

    /// The stops for a frame of average `luminance` cd/m²: the natural logarithm, the bending and the offsets, ten stops down.
    pub fn stops(&self, luminance: f32) -> f32 {
        let shifted = (self.horizontal_offset + luminance).max(LEAST_LUMINANCE);
        self.scale * shifted.ln() + self.bending * luminance + self.vertical_offset - CURVE_OFFSET
    }
}

/// What the visual settings state of the exposure: the curve, the curve it blends to, and the steps adaptation is held between.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ExposureSettings {
    pub curve: Curve,
    pub blend: Curve,
    /// The least and the most stops a second adaptation moves by.
    pub min_step: f32,
    pub max_step: f32,
    /// How fast adaptation closes on its target where a keyframe states no step multiplier of its own.
    pub step_mult: f32,
}

impl ExposureSettings {
    /// The settings `get` reads from the visual settings by name, each with the executable's own value where there is none.
    pub fn of(get: &dyn Fn(&str, f32) -> f32) -> Self {
        Self {
            curve: Curve::of(get, "Exposure.curve."),
            blend: Curve::of(get, "Exposure.curve.blend."),
            min_step: get("Adaptation.min.step.size", 0.15),
            max_step: get("Adaptation.max.step.size", 15.0),
            step_mult: get("Adaptation.step.size.mult", 9.0),
        }
    }
}

/// The compensation curve of the key-value mode at `ev`: two cubic Béziers over the keyframe's seven points, the input held to the first and last x; nought where the keyframe does not state all seven.
pub fn compensation(k: &Keyframe, ev: f32) -> f32 {
    let point = |i: usize| {
        let at = |axis: &str| k.get(&format!("postfx_exposure_compensation_ev_cp{i}_{axis}"));
        Some((at("x")?, at("y")?))
    };
    let Some(points) = (0..7).map(point).collect::<Option<Vec<_>>>() else {
        return 0.0;
    };
    let x = ev.clamp(points[0].0.min(points[6].0), points[6].0.max(points[0].0));
    let segment = match x < points[3].0 {
        true => &points[0..4],
        false => &points[3..7],
    };
    let along = |t: f32, pick: fn(&(f32, f32)) -> f32| {
        let u = 1.0 - t;
        let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|i| pick(&segment[i]));
        u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
    };
    // ponytail: x(t) is solved by halving, which holds while the segment's x rises; which root the game keeps where it does not is not read.
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..32 {
        let middle = 0.5 * (low + high);
        match along(middle, |p| p.0) < x {
            true => low = middle,
            false => high = middle,
        }
    }
    along(0.5 * (low + high), |p| p.1)
}

/// The stops the frame wants at an average luminance of `luminance` cd/m²: the curve, or the key value where the keyframe asks for it, held to the keyframe's least and most with its exposure and tweak added.
pub fn target_stops(settings: &ExposureSettings, k: &Keyframe, luminance: f32) -> f32 {
    let get = |name: &str, default: f32| k.get_or(name, default);
    let ev = match get("postfx_exposure_use_key_value_mode", 0.0) > 0.0 {
        true => {
            let ev =
                (get("postfx_exposure_key_value", GREY) / luminance.max(LEAST_LUMINANCE)).log2();
            ev + compensation(k, ev)
        }
        false => {
            let (a, b) = (
                settings.curve.stops(luminance),
                settings.blend.stops(luminance),
            );
            a + (b - a) * get("postfx_exposure_blend_curve_weight", 0.0)
        }
    };
    let least = get("postfx_exposure_min", f32::NEG_INFINITY);
    let most = get("postfx_exposure_max", f32::INFINITY);
    let hold = |stops: f32| stops.max(least.min(most)).min(most);
    hold(hold(ev) + get("postfx_exposure", 0.0) + get("postfx_exposure_tweak", 0.0))
}

/// The stops `seconds` after a frame at `previous`, moving towards `target` at a rate held between the settings' steps and never past it; a frame with no time behind it takes the target.
pub fn adapt(
    settings: &ExposureSettings,
    k: &Keyframe,
    previous: f32,
    target: f32,
    seconds: f32,
) -> f32 {
    let difference = target - previous;
    if difference == 0.0 || seconds <= 0.0 {
        return target;
    }
    let mult = k.get_or("postfx_exposure_adaption_step_mult", settings.step_mult);
    let rate = (difference.abs() * (seconds * mult).clamp(0.0, 1.0) / seconds)
        .max(settings.min_step.min(settings.max_step))
        .min(settings.max_step);
    let moved = previous + difference.signum() * rate * seconds;
    match difference > 0.0 {
        true => moved.min(target),
        false => moved.max(target),
    }
}

/// The way towards the moon when `day` days have gone by (whole days and the part of one), its round turned by `roll` radians and pushed off its plane by `wobble`.
pub fn moon_towards(day: f32, roll: f32, wobble: f32) -> [f32; 3] {
    let angle = TAU * day.rem_euclid(1.0) - PI;
    let (sin_a, cos_a) = angle.sin_cos();
    let (sin_r, cos_r) = roll.sin_cos();
    normalised([
        sin_a,
        -cos_r * cos_a - wobble * sin_r,
        -sin_r * cos_a + wobble * cos_r,
    ])
}

/// Which body lights a moment, and how.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bodies {
    pub sun_fade: f32,
    pub moon_fade: f32,
    pub sun_leads: bool,
    /// The way towards the body that leads.
    pub body: [f32; 3],
    /// The way the light comes from: the body's, held to 0.15 up at least.
    pub towards: [f32; 3],
    /// The leading body's fade times its own multiplier.
    pub fade: f32,
    /// The leading body's illuminance, and the sun's, in lux.
    pub illuminance: f32,
    pub sun_illuminance: f32,
}

/// Which of the sun towards `sun` and the moon towards `moon` lights the moment `k`, faded by the zenith angles its thresholds state or by its crossfade.
pub fn bodies(k: &Keyframe, sun: [f32; 3], moon: [f32; 3]) -> Bodies {
    let get = |name: &str, default: f32| k.get_or(name, default);
    let zenith = |v: [f32; 3]| v[2].clamp(-1.0, 1.0).acos().to_degrees();
    let threshold = |name: &str| match get(name, -1.0) {
        t if t < 0.0 => DEFAULT_THRESHOLD,
        t => t,
    };
    let crossfade = get("sun_moon_crossfade", AUTOMATIC);
    let (sun_fade, moon_fade) = match crossfade == AUTOMATIC {
        false => (crossfade.clamp(0.0, 1.0), (-crossfade).clamp(0.0, 1.0)),
        true => {
            let (sun_threshold, zs) = (threshold("sun_fade_threshold"), zenith(sun));
            let sun_fade = (sun_threshold - zs).clamp(0.0, 1.0);
            let moon_fade = match sun_fade > 0.0 {
                true => 0.0,
                false => {
                    (zs - sun_threshold).clamp(0.0, 1.0)
                        * (threshold("moon_fade_threshold") - zenith(moon)).clamp(0.0, 1.0)
                }
            };
            (sun_fade, moon_fade)
        }
    };
    let sun_leads = sun_fade > moon_fade;
    let body = if sun_leads { sun } else { moon };
    let multiplier = match sun_leads {
        true => get("light_dir_sun_mult", 1.0),
        false => get("light_dir_moon_mult", 1.0),
    };
    let sun_illuminance = get("sky_scattering_sun_illuminance", 0.0)
        * get("sky_scattering_sun_intensity_multiplier", 1.0);
    let moon_illuminance = get("sky_scattering_moon_illuminance", 0.0)
        * get("sky_scattering_moon_intensity_multiplier", 1.0);
    Bodies {
        sun_fade,
        moon_fade,
        sun_leads,
        body,
        towards: normalised([body[0], body[1], body[2].max(LOWEST_LIGHT)]),
        fade: sun_fade.max(moon_fade) * multiplier,
        illuminance: if sun_leads {
            sun_illuminance
        } else {
            moon_illuminance
        },
        sun_illuminance,
    }
}

/// The light of the moment `k` with the sun towards `sun` and the moon towards `moon`, and the stops it is exposed at: the target of the curve over a grey world under that light, with no history.
// ponytail: a grey world stands in for the game's meter of its own frame (a trimmed mean of 256
// logarithmic bins); the frame has no meter yet, and with one the stops are `adapt`ed from frame to frame.
pub fn light(
    k: &Keyframe,
    sun: [f32; 3],
    moon: [f32; 3],
    settings: &ExposureSettings,
) -> (FrameLight, f32) {
    let get = |name: &str, default: f32| k.get_or(name, default);
    let rgb = |stem: &str| ["r", "g", "b"].map(|c| get(&format!("{stem}_{c}"), 1.0));
    let air = Atmosphere::of(k);
    let lit = bodies(k, sun, moon);
    let body_transmittance = air.transmittance(lit.body[2]);
    let sky_light = normalised([lit.body[0], lit.body[1], lit.body[2].max(LOWEST_SKY_LIGHT)]);
    let sky = air.irradiance(sky_light, lit.illuminance);

    let tint = rgb("light_dir_col");
    let direct = get("light_dir_mult", 1.0) * lit.fade * lit.illuminance;
    let direct = luminance([0, 1, 2].map(|c| tint[c] * body_transmittance[c])) * direct;
    let tints = [rgb("sky_scattering_sky_col"), rgb("ambient_sky_color_mult")];
    let ambient = luminance([0, 1, 2].map(|c| sky[c] * tints[0][c] * tints[1][c]))
        * get("ambient_sky_intensity", 1.0);
    let metered = GREY / PI * (direct * lit.towards[2] + ambient);
    let stops = target_stops(settings, k, metered);

    let radiance = |view: [f32; 3]| air.radiance(view, sky_light, lit.illuminance);
    let level = |x: f32, y: f32| match x.hypot(y) > 1e-3 {
        true => [x, y, HORIZON],
        false => [1.0, 0.0, HORIZON],
    };
    let under = level(lit.body[0], lit.body[1]);
    let opposite = [-under[0], -under[1], HORIZON];
    let across = (1.0 - DOME_TRANSITION * DOME_TRANSITION).sqrt();
    let [east, west, north, south] = [[across, 0.0], [-across, 0.0], [0.0, across], [0.0, -across]]
        .map(|[x, y]| radiance([x, y, DOME_TRANSITION]));
    let transition = [0, 1, 2].map(|c| (north[c] + south[c]) / 2.0);
    let light = FrameLight {
        exposure: stops.exp2(),
        fade: lit.fade,
        illuminance: lit.illuminance,
        sun_illuminance: lit.sun_illuminance,
        body_transmittance,
        sun_transmittance: air.transmittance(sun[2]),
        towards_up: lit.towards[2],
        sky,
        dome: [
            radiance([0.0, 0.0, 1.0]),
            transition,
            radiance([1.0, 0.0, HORIZON]),
            radiance([-1.0, 0.0, HORIZON]),
            radiance([0.0, -1.0, HORIZON]),
        ],
        dome_sides: [east, west],
        fog: [under, [1.0, 0.0, HORIZON], opposite, [0.0, -1.0, HORIZON]].map(radiance),
    };
    (light, stops)
}

#[cfg(test)]
#[path = "gta6_exposure_tests.rs"]
mod tests;
