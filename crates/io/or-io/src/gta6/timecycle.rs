//! What GTA VI's day adds to its weather cycles, which GTA V's reader reads as they are: a time
//! table whose samples are places of the sun, the sun's own round, the modifiers, and the
//! variables a frame built for GTA V's day takes, worked out from GTA VI's.

use std::collections::BTreeMap;
use std::f32::consts::{PI, TAU};

use crate::gta5::data::timecycle::{attr, tokens, Keyframe, Token};

/// When the sun is highest, as a part of the day: one o'clock by the summer clock and the
/// minutes the first day of spring adds.
const SOLAR_NOON: f32 = (13.0 * 60.0 + 27.0) / 1440.0;
/// The parts of the day the clock is bent between: the first and last quarter hour the sun is on the horizon.
const SUNRISE: f32 = 29.0 / 96.0;
const SUNSET: f32 = 78.0 / 96.0;
/// How the clock lingers near sunrise and sunset: the power the way through half a day or half
/// a night is raised to.
const HORIZON_LINGER: f32 = 1.25;
/// The latitude at the middle of the map, in radians, for an install whose settings state no range.
pub const LATITUDE: f32 = 25.8 * PI / 180.0;

/// Where a cycle's samples stand on the sun's round, east to west through the top: the angle of
/// the sun in the plane of east and up, nought where it rises, and the sample that holds there.
// The half turn is written short, and the round is closed by as short a turn: neither is pi.
#[allow(clippy::approx_constant)]
const SAMPLE_ANGLES: [(f32, usize); 13] = [
    (-3.14, 9),
    (-3.09, 10),
    (-2.77, 11),
    (-2.42, 12),
    (-1.57, 0),
    (-0.45, 1),
    (-0.22, 2),
    (-0.04, 3),
    (0.0, 4),
    (0.3, 5),
    (1.57, 6),
    (2.57, 7),
    (3.09, 8),
];

/// The hours of a frame's own day between which its sun rises and sets.
// ⚠ These are the frame's, not GTA VI's: `frame_hours` and `sun_lights` hold only while the
// frame's sun rises at 6, sets at 20 and hands over to the moon an hour and three quarters after.
const FRAME_RISE: f32 = 6.0;
const FRAME_SET: f32 = 20.0;
const FRAME_HANDOVER: f32 = 1.75;

/// The directional multiplier a frame holds where a day states none, which GTA VI's sun at its
/// stated default illuminance is given.
const FRAME_SUN: f32 = 14.75;
/// The illuminance of the sun where a cycle states none, in lux.
const SUN_ILLUMINANCE: f32 = 100_000.0;
/// The natural ambient a frame holds where a day states none: from above, and from below.
const FRAME_AMBIENT_DOWN: f32 = 6.25;
const FRAME_AMBIENT_UP: f32 = 8.5;
const FRAME_AMBIENT_DOWN_COLOUR: [f32; 3] = [0.541, 0.655, 1.0];
// ponytail: three stand-in shares, none the game's. GTA VI states its moon in lux and meters its
// exposure from the frame; a frame with no such meter is given the moon at a tenth of the sun,
// a sky at three tenths of the light that makes it, and a horizon half way to white. Read the
// game's exposure and atmosphere to replace them.
const MOON_SHARE: f32 = 0.1;
const SKY_SHARE: f32 = 0.3;
const HORIZON_PALE: f32 = 0.5;

/// How the way from one sample to the next is bent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Easing {
    #[default]
    Linear,
    QuarticInOut,
}

impl Easing {
    /// `along` (0 to 1) as this easing bends it.
    pub fn apply(self, along: f32) -> f32 {
        let t = along.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            // ponytail: the usual curve of that name; which of the game's easing kinds the name selects is not read.
            Self::QuarticInOut if t < 0.5 => 8.0 * t.powi(4),
            Self::QuarticInOut => 1.0 - 8.0 * (1.0 - t).powi(4),
        }
    }
}

/// One sample of the day: a place of the sun, named by an hour.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: String,
    pub hour: f32,
    pub easing: Easing,
}

/// GTA VI's time table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DayTable {
    pub samples: Vec<Sample>,
    pub regions: Vec<String>,
}

impl DayTable {
    pub fn parse(text: &str) -> Self {
        let mut table = Self::default();
        for token in tokens(text) {
            let Token::Open { name, attrs, .. } = token else {
                continue;
            };
            match name {
                "sample" => table.samples.push(Sample {
                    name: attr(attrs, "name").unwrap_or_default().to_string(),
                    hour: (attr(attrs, "hour").and_then(|v| v.trim().parse().ok())).unwrap_or(0.0),
                    easing: match attr(attrs, "interpolation_type") {
                        Some(kind) if kind.eq_ignore_ascii_case("EASE_QUARTIC_INOUT") => {
                            Easing::QuarticInOut
                        }
                        _ => Easing::Linear,
                    },
                }),
                "region" => (table.regions).extend(attr(attrs, "name").map(str::to_string)),
                _ => {}
            }
        }
        table
    }

    /// The two samples the sun at `angle` lies between and how far along, eased; `None` unless the table has the samples the angles are of.
    #[allow(clippy::approx_constant)]
    pub fn locate(&self, angle: f32) -> Option<(usize, usize, f32)> {
        if self.samples.len() < SAMPLE_ANGLES.len() {
            return None;
        }
        // The round is closed a little short of a whole turn.
        let angle = if angle < -3.14 { angle + 6.28 } else { angle };
        let first = (SAMPLE_ANGLES.iter())
            .rposition(|(at, _)| *at <= angle)
            .unwrap_or(0);
        let second = (first + 1) % SAMPLE_ANGLES.len();
        let (from, to) = (SAMPLE_ANGLES[first].0, SAMPLE_ANGLES[second].0);
        let to = if to < from { to + TAU } else { to };
        let along = ((angle - from) / (to - from)).clamp(0.0, 1.0);
        let (first, second) = (SAMPLE_ANGLES[first].1, SAMPLE_ANGLES[second].1);
        Some((first, second, self.samples[first].easing.apply(along)))
    }
}

/// The clock as the sun follows it: `day` is the part of the day gone by, and each half of the
/// day and of the night is slowed near the horizon.
fn sun_clock(day: f32) -> f32 {
    let (rise, set) = (SUNRISE, SUNSET);
    let (from, to) = match day {
        day if day < rise => (set - 1.0, rise),
        day if day > set => (set, rise + 1.0),
        _ => (rise, set),
    };
    let half = (to - from) / 2.0;
    match day - from < half {
        true => from + half * ((day - from) / half).powf(HORIZON_LINGER),
        false => to - half * ((to - day) / half).powf(HORIZON_LINGER),
    }
}

/// How far the sun has gone on its own round at `hours` (0 to 24) of the first day of spring: nought as it rises, a quarter turn at its top, in `-PI..=PI`.
pub fn sun_round(hours: f32) -> f32 {
    let turn = (sun_clock(hours.rem_euclid(24.0) / 24.0) - SOLAR_NOON) * TAU;
    let round = PI / 2.0 + turn;
    (round + PI).rem_euclid(TAU) - PI
}

/// The way towards the sun `round` along its round at `latitude` (radians): x east, y north, z up.
pub fn sun_towards(round: f32, latitude: f32) -> [f32; 3] {
    let (up, east) = round.sin_cos();
    [east, -up * latitude.sin(), up * latitude.cos()]
}

/// The angle a cycle's samples are found by: the sun's in the plane of east and up.
pub fn sun_angle(towards: [f32; 3]) -> f32 {
    towards[2].atan2(towards[0])
}

/// The lean of the sun's path a frame's own time table takes for a sun at `latitude`, in radians.
pub fn sun_roll(latitude: f32) -> f32 {
    PI / 2.0 + latitude
}

/// The hour of a frame's own day at which its sun is `round` along its round.
pub fn frame_hours(round: f32) -> f32 {
    let round = round.rem_euclid(TAU);
    if round <= PI {
        return FRAME_RISE + (FRAME_SET - FRAME_RISE) * round / PI;
    }
    // The night is two quarter turns, each as long as the frame's clock gives it.
    let quarter = (round - PI) / (PI / 2.0);
    match quarter < 1.0 {
        true => FRAME_SET + (24.0 - FRAME_SET) * quarter,
        false => FRAME_RISE * (quarter - 1.0),
    }
}

/// Whether a frame at the hour `frame_hours` of its own day is lit by its sun, not its moon.
pub fn sun_lights(frame_hours: f32) -> bool {
    (FRAME_RISE - FRAME_HANDOVER + 0.5..FRAME_SET + FRAME_HANDOVER).contains(&frame_hours)
}

/// The single values a cycle's file states before its cycle (`High_Level_Vars`), by name.
pub fn high_level_vars(text: &str) -> BTreeMap<String, f32> {
    let mut vars = BTreeMap::new();
    let (mut inside, mut open) = (false, None);
    for token in tokens(text) {
        match token {
            Token::Open {
                name: "High_Level_Vars",
                ..
            } => inside = true,
            Token::Close("High_Level_Vars") => break,
            Token::Open { name, empty, .. } if inside && !empty => open = Some(name),
            Token::Text(text) => {
                if let (Some(name), Ok(value)) = (open, text.trim().parse()) {
                    vars.insert(name.to_string(), value);
                }
            }
            _ => open = None,
        }
    }
    vars
}

/// A timecycle modifier: each variable it states with its two values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Modifier {
    pub name: String,
    pub user_flags: u32,
    pub vars: BTreeMap<String, [f32; 2]>,
}

/// Every modifier a `timecycle_modifier_data` text holds; a variable that does not state two numbers is left out.
pub fn modifiers(text: &str) -> Vec<Modifier> {
    let mut out: Vec<Modifier> = Vec::new();
    let (mut inside, mut open) = (false, None);
    for token in tokens(text) {
        match token {
            Token::Open {
                name: "modifier",
                attrs,
                empty,
            } => {
                out.push(Modifier {
                    name: attr(attrs, "name").unwrap_or_default().to_string(),
                    user_flags: (attr(attrs, "userFlags").and_then(|v| v.trim().parse().ok()))
                        .unwrap_or(0),
                    vars: BTreeMap::new(),
                });
                inside = !empty;
            }
            Token::Close("modifier") => inside = false,
            Token::Open { name, empty, .. } if inside && !empty => open = Some(name),
            Token::Text(text) => {
                let values: Vec<f32> = (text.split_whitespace())
                    .filter_map(|v| v.parse().ok())
                    .collect();
                if let (Some(name), Some(modifier), &[a, b]) = (open, out.last_mut(), &values[..]) {
                    modifier.vars.insert(name.to_string(), [a, b]);
                }
            }
            _ => open = None,
        }
    }
    out
}

/// The variables GTA VI states under the name and in the unit a frame built for GTA V's day reads them by.
const SAME: &[&str] = &[
    "far_clip",
    "fog_start",
    "light_artificial_int_down_col_r",
    "light_artificial_int_down_col_g",
    "light_artificial_int_down_col_b",
    "light_artificial_int_down_intensity",
    "light_artificial_int_up_col_r",
    "light_artificial_int_up_col_g",
    "light_artificial_int_up_col_b",
    "light_artificial_int_up_intensity",
    "light_direction_override",
    "light_direction_override_overrides_sun",
    "sun_direction_x",
    "sun_direction_y",
    "sun_direction_z",
    "moon_direction_x",
    "moon_direction_y",
    "moon_direction_z",
    "postfx_correct_col_r",
    "postfx_correct_col_g",
    "postfx_correct_col_b",
    "postfx_correct_cutoff",
    "postfx_shift_col_r",
    "postfx_shift_col_g",
    "postfx_shift_col_b",
    "postfx_shift_cutoff",
    "postfx_desaturation",
    "postfx_exposure",
    "postfx_intensity_bloom",
    "postfx_tonemap_filmic_override_dark",
    "postfx_tonemap_filmic_exposure_dark",
    "postfx_tonemap_filmic_a",
    "postfx_tonemap_filmic_b",
    "postfx_tonemap_filmic_c",
    "postfx_tonemap_filmic_d",
    "postfx_tonemap_filmic_e",
    "postfx_tonemap_filmic_f",
    "postfx_tonemap_filmic_w",
    "postfx_tonemap_filmic_override_bright",
    "postfx_tonemap_filmic_exposure_bright",
    "postfx_tonemap_filmic_a_bright",
    "postfx_tonemap_filmic_b_bright",
    "postfx_tonemap_filmic_c_bright",
    "postfx_tonemap_filmic_d_bright",
    "postfx_tonemap_filmic_e_bright",
    "postfx_tonemap_filmic_f_bright",
    "postfx_tonemap_filmic_w_bright",
    "reflection_tweak_directional",
    "sky_moon_col_r",
    "sky_moon_col_g",
    "sky_moon_col_b",
    "sky_moon_disc_size",
    "sky_sun_disc_size",
];

/// The variables [`frame_keyframe`] works out from GTA VI's under other names.
const WORKED_OUT: &[&str] = &[
    "light_dir_col_r",
    "light_dir_col_g",
    "light_dir_col_b",
    "light_dir_mult",
    "light_natural_amb_down_col_r",
    "light_natural_amb_down_col_g",
    "light_natural_amb_down_col_b",
    "light_natural_amb_down_intensity",
    "light_natural_amb_up_intensity",
    "fog_hdr",
    "fog_haze_hdr",
    "sky_cloud_hdr",
    "sky_sun_col_r",
    "sky_sun_col_g",
    "sky_sun_col_b",
    "sky_sun_disc_col_r",
    "sky_sun_disc_col_g",
    "sky_sun_disc_col_b",
    "sky_sun_hdr",
    "sky_zenith_col_r",
    "sky_zenith_col_g",
    "sky_zenith_col_b",
    "sky_zenith_col_inten",
    "sky_zenith_transition_col_r",
    "sky_zenith_transition_col_g",
    "sky_zenith_transition_col_b",
    "sky_zenith_transition_col_inten",
    "sky_azimuth_east_col_r",
    "sky_azimuth_east_col_g",
    "sky_azimuth_east_col_b",
    "sky_azimuth_east_col_inten",
    "sky_azimuth_west_col_r",
    "sky_azimuth_west_col_g",
    "sky_azimuth_west_col_b",
    "sky_azimuth_west_col_inten",
    "sky_azimuth_transition_col_r",
    "sky_azimuth_transition_col_g",
    "sky_azimuth_transition_col_b",
    "sky_azimuth_transition_col_inten",
];

/// The names of a frame's variables [`frame_keyframe`] fills from a keyframe that states all it reads.
pub fn filled() -> impl Iterator<Item = &'static str> {
    SAME.iter().chain(WORKED_OUT).copied()
}

/// How strong the body that lights the moment is beside the sun at its stated default: its illuminance, its fade at the moment and the cycle's multipliers.
pub fn light_share(own: &Keyframe, sun_lit: bool) -> f32 {
    let get = |name: &str, default: f32| own.get_or(name, default);
    let body = match sun_lit {
        true => {
            get("light_dir_sun_mult", 1.0)
                * get("sky_scattering_sun_intensity_multiplier", 1.0)
                * get("sky_scattering_sun_illuminance", SUN_ILLUMINANCE)
                / SUN_ILLUMINANCE
        }
        false => {
            get("light_dir_moon_mult", 1.0)
                * get("sky_scattering_moon_intensity_multiplier", 1.0)
                * get("sky_scattering_moon_illuminance", 1.0)
                * MOON_SHARE
        }
    };
    get("light_dir_mult", 1.0) * body
}

/// The variables of a frame built for GTA V's day, worked out from GTA VI's keyframe `own` for a moment lit by the sun (`sun_lit`) or the moon; what GTA VI does not state is left out, so the frame keeps its own default.
pub fn frame_keyframe(own: &Keyframe, sun_lit: bool) -> Keyframe {
    let mut frame = Keyframe::default();
    for name in SAME {
        if let Some(value) = own.get(name) {
            frame.set(name, value);
        }
    }
    let get = |name: &str, default: f32| own.get_or(name, default);
    let rgb =
        |stem: &str, default: f32| ["r", "g", "b"].map(|c| get(&format!("{stem}_{c}"), default));
    let set_rgb = |frame: &mut Keyframe, stem: &str, colour: [f32; 3]| {
        for (c, value) in ["r", "g", "b"].iter().zip(colour) {
            frame.set(&format!("{stem}_{c}"), value);
        }
    };
    let times = |a: [f32; 3], b: [f32; 3]| [a[0] * b[0], a[1] * b[1], a[2] * b[2]];

    let share = light_share(own, sun_lit);
    let body = if sun_lit { "sun" } else { "moon" };
    let body_tint = rgb(&format!("sky_scattering_{body}_col"), 1.0);
    set_rgb(
        &mut frame,
        "light_dir_col",
        times(rgb("light_dir_col", 1.0), body_tint),
    );
    frame.set("light_dir_mult", FRAME_SUN * share);

    // The sky's light follows the body that makes it, tinted as the cycle tints the sky.
    let sky_tint = times(
        rgb("sky_scattering_sky_col", 1.0),
        rgb("ambient_sky_color_mult", 1.0),
    );
    set_rgb(
        &mut frame,
        "light_natural_amb_down_col",
        times(FRAME_AMBIENT_DOWN_COLOUR, sky_tint),
    );
    frame.set(
        "light_natural_amb_down_intensity",
        FRAME_AMBIENT_DOWN * share,
    );
    frame.set("light_natural_amb_up_intensity", FRAME_AMBIENT_UP * share);
    frame.set("fog_hdr", share);
    frame.set("fog_haze_hdr", share);
    frame.set("sky_cloud_hdr", 0.8 * share);

    let sun_tint = rgb("sky_scattering_sun_col", 1.0);
    set_rgb(&mut frame, "sky_sun_col", sun_tint);
    set_rgb(&mut frame, "sky_sun_disc_col", sun_tint);
    frame.set("sky_sun_hdr", FRAME_SUN * light_share(own, true));

    // A stand-in sky: the colour the air scatters at the top, paler towards the horizon.
    let air = rgb("sky_new_ray_scattering_col", 0.5);
    let most = air.iter().copied().fold(1e-6, f32::max);
    // The frame squares a sky colour, so the root is given.
    let zenith = times(air.map(|c| (c / most).sqrt()), sky_tint.map(f32::sqrt));
    let horizon = zenith.map(|c| c + (1.0 - c) * HORIZON_PALE);
    let strength = FRAME_SUN * SKY_SHARE * share;
    for (stem, colour) in [
        ("sky_zenith_col", zenith),
        ("sky_zenith_transition_col", zenith),
        ("sky_azimuth_east_col", horizon),
        ("sky_azimuth_west_col", horizon),
        ("sky_azimuth_transition_col", horizon),
    ] {
        set_rgb(&mut frame, stem, colour);
        frame.set(&format!("{stem}_inten"), strength);
    }
    frame
}

#[cfg(test)]
#[path = "timecycle_tests.rs"]
mod tests;
