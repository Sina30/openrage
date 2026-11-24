//! The day the GTA VI world is lit by: the install's own tables, read once on a worker and held,
//! as the day the frame takes in place of a GTA V install's.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use glam::Vec3;
use or_renderer::game_shaders::lighting::{night_emissive, Day, Stated};
use or_renderer::game_shaders::sky_view::SkyView;
use or_renderer::game_shaders::tone::ToneSettings;
use or_renderer::viewport::Metered;
use or_title::gta6_day::{read_day, DayTables, Moment};
use or_title::gta6_sky::Atmosphere;

use crate::state::drivers::WeatherState;

/// The archive of an install the tables of the day are in.
const ARCHIVE: &str = "common.rpf";
/// How far into the moon's cycle the first day stands: near its full, as the GTA V day has it.
const NEAR_FULL_MOON: f32 = 13.0;
/// What a light is scaled by while no GTA VI day is read and the frame keeps its own day.
const NO_DAY_LIGHT_SCALE: f32 = 0.1;
/// How far north, in metres, a camera moves before its moment is worked out again.
const LATITUDE_STEP: f32 = 100.0;
/// The least change of exposure, in stops, the frame is given a new day for.
const STEADY: f32 = 0.01;
/// The up part of the way to a body below which it lights no sky: some twenty degrees under the horizon.
const LOWEST_BODY: f32 = -0.35;

type Read = Arc<OnceLock<Arc<DayTables>>>;

// ponytail: read again each session, about a second; kept beside the world index if it shows.
static HELD: Mutex<Vec<(PathBuf, Read)>> = Mutex::new(Vec::new());

/// The cycle of the install that stands for the Weather bar's `state`.
pub fn weather_name(state: WeatherState) -> &'static str {
    match state {
        WeatherState::Clear => "Sunny",
        WeatherState::Cloudy => "CLOUDS",
        WeatherState::Fog => "Fog",
        WeatherState::Rain => "Rain",
        WeatherState::Storm => "THUNDERSTORM",
    }
}

/// The day's tables of the install at `root`, read once on a worker and held: `None` until they are read, and where the install states none.
pub fn tables(root: &Path) -> Option<Arc<DayTables>> {
    let archive = root.join(ARCHIVE);
    let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
    let slot = match held.iter().find(|(path, _)| *path == archive) {
        Some((_, slot)) => Arc::clone(slot),
        None => {
            let slot = Read::default();
            held.push((archive.clone(), Arc::clone(&slot)));
            let filled = Arc::clone(&slot);
            std::thread::spawn(move || {
                let tables = read_day(&archive, &|| false).unwrap_or_else(|error| {
                    tracing::warn!(archive = %archive.display(), %error, "GTA VI's tables of the day are not read");
                    DayTables::default()
                });
                tracing::info!(
                    cycles = tables.timecycle.cycles.len(),
                    samples = tables.table.samples.len(),
                    "GTA VI's tables of the day were read"
                );
                let _ = filled.set(Arc::new(tables));
            });
            slot
        }
    };
    let tables = slot.get()?;
    (!tables.is_empty()).then(|| Arc::clone(tables))
}

/// The moment of GTA VI's day at `hour` in `weather` for a camera `y` metres north on the map, which a frame takes its day, its lights' scale and its exposure from; `None` where the tables hold no cycle for it.
pub fn moment(tables: &DayTables, hour: f32, weather: WeatherState, y: f32) -> Option<Moment> {
    type Key = (usize, u32, WeatherState, i32);
    // ponytail: the last moment held, as a frame asks for the same one many times over.
    static LAST: Mutex<Option<(Key, Option<Moment>)>> = Mutex::new(None);
    // The latitude moves the sun by a hundredth of a degree over a hundred metres.
    let rows = (y / LATITUDE_STEP).round();
    let key = (
        std::ptr::from_ref(tables) as usize,
        hour.to_bits(),
        weather,
        rows as i32,
    );
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((_, moment)) = last.as_ref().filter(|(held, _)| *held == key) {
        return moment.clone();
    }
    let moment = tables.moment(hour, weather_name(weather), rows * LATITUDE_STEP);
    *last = Some((key, moment.clone()));
    moment
}

/// Where the frame's exposure stands: the moment it was metered for, its stops and when they were set, and whether the next reading is the first of that moment.
struct Adapting {
    of: (u32, WeatherState),
    stops: f32,
    at: Instant,
    fresh: bool,
}

// ponytail: one exposure for the one World view; a second view would share it.
static ADAPTING: Mutex<Option<Adapting>> = Mutex::new(None);

/// `moment` exposed by the frame's own luminance, `metered` as last read from a frame drawn under the day set now: adapted from frame to frame at the pace of the settings, taken whole at the first reading of a new hour or weather, and held where nothing was read.
pub fn exposed(
    tables: &DayTables,
    moment: Moment,
    hour: f32,
    weather: WeatherState,
    metered: Option<&Metered>,
) -> Moment {
    let of = (hour.to_bits(), weather);
    let now = Instant::now();
    let mut held = ADAPTING.lock().unwrap_or_else(|p| p.into_inner());
    let Some(adapting) = held.as_mut().filter(|a| a.of == of) else {
        // ⚠ What is read now was drawn under the hour before: the new one waits for its own frame.
        *held = Some(Adapting {
            of,
            stops: moment.stops,
            at: now,
            fresh: true,
        });
        return moment;
    };
    // ⚠ Inferred: the build names these percentages; they are taken as parts of the frame.
    let low = moment.own.get_or("postfx_exposure_lowlights_percent", 0.0);
    let high = moment.own.get_or("postfx_exposure_highlights_percent", 0.0);
    let Some(luminance) = metered.and_then(|m| m.average(low, high)) else {
        return moment.exposed_at(adapting.stops);
    };
    let seconds = match std::mem::take(&mut adapting.fresh) {
        true => 0.0,
        false => now.duration_since(adapting.at).as_secs_f32(),
    };
    let exposed = moment.metered(
        &tables.exposure_settings(),
        luminance,
        adapting.stops,
        seconds,
    );
    // ponytail: a step too small to see is not taken, so that a still frame keeps its day.
    if (exposed.stops - adapting.stops).abs() < STEADY {
        return moment.exposed_at(adapting.stops);
    }
    (adapting.stops, adapting.at) = (exposed.stops, now);
    exposed
}

/// The day the frame is lit by at `hour` of the `days`th day in `weather`, for a camera `y` metres north on the map; `None` where the tables hold no cycle for it.
#[cfg(test)]
pub fn day_of(
    tables: &DayTables,
    hour: f32,
    days: f32,
    weather: WeatherState,
    y: f32,
) -> Option<Day> {
    let moment = moment(tables, hour, weather, y)?;
    Some(day_of_moment(tables, moment, hour, days, weather))
}

/// The day the frame is lit by from `moment`, at `hour` of the `days`th day in `weather`.
pub fn day_of_moment(
    tables: &DayTables,
    moment: Moment,
    hour: f32,
    days: f32,
    weather: WeatherState,
) -> Day {
    let visual = &tables.visual_settings;
    // The moment's light is stated whole, faded already by its own bodies.
    let stated = Stated {
        sun: Vec3::from(moment.sun),
        moon: Vec3::from(moment.moon),
        towards_light: Vec3::from(moment.bodies.towards),
        sun_fade: moment.bodies.sun_fade,
        moon_fade: moment.bodies.moon_fade,
    };
    let sky_view = SkyView {
        radiance: sky_radiance(&moment),
        exposure: moment.light.exposure,
    };
    Day {
        keyframe: moment.keyframe,
        table: moment.table,
        hours: moment.frame_hours,
        cycle_time: NEAR_FULL_MOON + days + hour / 24.0,
        moon_dim: moment.moon_dim,
        // The Weather bar's own wetness where the install's weather states no rain.
        wetness: moment.wetness.max(weather.wetness()),
        // How far a material's glow has gone to its night value: the cycle's own blend, else on by the night hours.
        night_emissive: (moment.own.get("global_emissive_scale_blend_value")).unwrap_or_else(
            || {
                night_emissive(
                    hour,
                    visual.get_f32("emissive.night.start.time", 20.0) as u32,
                    visual.get_f32("emissive.night.end.time", 7.0) as u32,
                )
            },
        ),
        // ⚠ GTA VI's exposure curve has the frame's name and another shape (a natural logarithm, a
        // bending and offsets in place of a power): the moment is exposed by it already and the
        // frame's own exposure held at nought, so the frame is never given it.
        tone: ToneSettings::from_visual(
            &|name, default| match name.starts_with("Exposure.curve.") {
                true => default,
                false => visual.get_f32(name, default),
            },
        ),
        stated: Some(stated),
        sky_view: Some(sky_view),
    }
}

/// What the sky's radiance table is worked out from.
#[derive(PartialEq)]
struct SkyOf {
    air: Atmosphere,
    sun: [f32; 3],
    moon: [f32; 3],
    illuminance: [f32; 2],
    tint: [f32; 3],
}

/// The sky's radiance in every way of the table at `moment`: what the air scatters of the sun and of the moon, each of its own illuminance, tinted by the moment's sky colour; worked out again only when one of those changes.
fn sky_radiance(moment: &Moment) -> Arc<Vec<[f32; 3]>> {
    // ponytail: one table held, a few tenths of a second to work out; per moment it changes with the hour and the weather.
    static HELD: Mutex<Option<(SkyOf, Arc<Vec<[f32; 3]>>)>> = Mutex::new(None);
    let own = &moment.own;
    let of = SkyOf {
        air: Atmosphere::of(own),
        sun: moment.sun,
        moon: moment.moon,
        illuminance: [
            moment.light.sun_illuminance,
            own.get_or("sky_scattering_moon_illuminance", 0.0)
                * own.get_or("sky_scattering_moon_intensity_multiplier", 1.0),
        ],
        tint: ["r", "g", "b"].map(|c| own.get_or(&format!("sky_scattering_sky_col_{c}"), 1.0)),
    };
    let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((_, table)) = held.as_ref().filter(|(was, _)| *was == of) {
        return Arc::clone(table);
    }
    // A body far under the horizon lights no air the camera sees.
    let lit = |body: [f32; 3], lux: f32| lux > 0.0 && body[2] > LOWEST_BODY;
    let bodies: Vec<([f32; 3], f32)> = [(of.sun, of.illuminance[0]), (of.moon, of.illuminance[1])]
        .into_iter()
        .filter(|(body, lux)| lit(*body, *lux))
        .collect();
    let table = SkyView::of(
        |view| {
            let mut sum = [0.0f32; 3];
            for (body, lux) in &bodies {
                let seen = of.air.radiance(view.to_array(), *body, *lux);
                sum = [0, 1, 2].map(|c| sum[c] + seen[c]);
            }
            [0, 1, 2].map(|c| sum[c] * of.tint[c])
        },
        1.0,
    )
    .radiance;
    *held = Some((of, Arc::clone(&table)));
    table
}

/// What a light the game states in its own unit is multiplied by to be the frame's at `hour` in `weather` for a camera `y` metres north: the moment's exposure over pi, and the old stand-in where no GTA VI day is read.
#[cfg(test)]
pub fn light_scale(tables: Option<&DayTables>, hour: f32, weather: WeatherState, y: f32) -> f32 {
    scale_of(
        tables
            .and_then(|tables| moment(tables, hour, weather, y))
            .as_ref(),
    )
}

/// What a light the game states in its own unit is multiplied by under `moment`: its exposure over pi, and the old stand-in where no GTA VI day is read.
pub fn scale_of(moment: Option<&Moment>) -> f32 {
    moment.map_or(NO_DAY_LIGHT_SCALE, |moment| moment.light_scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_renderer::game_shaders::lighting::View;

    fn tables() -> DayTables {
        let samples: String = (0..13)
            .map(|k| format!(r#"<sample name="s{k}" hour="{k}"/>"#))
            .collect();
        let time = format!("<time_data>{samples}</time_data>");
        let cycle = r#"<timecycle_keyframe_data><cycle name="Sunny" regions="1"><region name="GLOBAL">
            <light_dir_mult> 1 1 1 1 1 1 1 1 1 1 1 1 1</light_dir_mult>
            </region></cycle></timecycle_keyframe_data>"#;
        let settings = "Tonemapping.dark.filmic.W 11.2\nExposure.curve.scale -2.0\nemissive.night.start.time 22\nemissive.night.end.time 4\n";
        DayTables::of_texts([time.as_str(), cycle, settings])
    }

    #[test]
    fn the_frames_sun_stands_where_gta_vis_does_at_every_hour_of_its_day() {
        let tables = tables();
        let view = View {
            camera_height: 0.0,
            projection: glam::Mat4::IDENTITY,
            near: 0.1,
        };
        for hour in [7.5f32, 9.0, 13.45, 16.0, 19.0, 23.0, 2.0] {
            let moment = tables
                .moment(hour, "Sunny", 0.0)
                .expect("the cycle is read");
            let day = day_of(&tables, hour, 0.0, WeatherState::Clear, 0.0).expect("a day");
            let (sun, _) = day.lighting(&view).sun_and_moon();
            let own = glam::Vec3::from(moment.sun);
            assert!(
                (sun - own).length() < 1e-3,
                "at {hour}: {sun:?} and {own:?}"
            );
        }
    }

    #[test]
    fn the_day_takes_the_installs_curve_and_night_hours_and_leaves_its_exposure_curve() {
        let tables = tables();
        let day = day_of(&tables, 23.0, 2.0, WeatherState::Rain, 0.0);
        assert!(day.is_none(), "no cycle stands for rain in these tables");
        let day = day_of(&tables, 23.0, 2.0, WeatherState::Clear, 0.0).expect("a day");
        assert_eq!(day.tone.dark.w, 11.2);
        assert_eq!(day.tone.curve_scale, ToneSettings::default().curve_scale);
        assert_eq!(day.night_emissive, 1.0);
        assert_eq!(day.cycle_time, NEAR_FULL_MOON + 2.0 + 23.0 / 24.0);
        let noon = day_of(&tables, 12.0, 0.0, WeatherState::Clear, 0.0).expect("a day");
        assert_eq!(noon.night_emissive, 0.0);
    }

    #[test]
    fn the_frame_maps_the_moment_at_nought_stops_and_the_lights_take_its_exposure() {
        let tables = tables();
        let view = View {
            camera_height: 0.0,
            projection: glam::Mat4::IDENTITY,
            near: 0.1,
        };
        for hour in [13.45f32, 1.0] {
            let day = day_of(&tables, hour, 0.0, WeatherState::Clear, 0.0).expect("a day");
            assert_eq!(day.tone(&day.lighting(&view)).stops, 0.0, "at {hour}");
            let moment = tables.moment(hour, "Sunny", 0.0).expect("a moment");
            let scale = light_scale(Some(&tables), hour, WeatherState::Clear, 0.0);
            assert_eq!(scale, moment.light_scale);
            assert_eq!(scale_of(Some(&moment)), scale);
            let exposed = moment.stops.exp2() / std::f32::consts::PI;
            assert!(
                (scale - exposed).abs() <= 1e-6 * exposed,
                "{scale} {exposed}"
            );
            let once = day_of_moment(&tables, moment, hour, 0.0, WeatherState::Clear);
            assert_eq!(once.hours, day.hours, "at {hour}");
        }
        let none = light_scale(None, 12.0, WeatherState::Clear, 0.0);
        assert_eq!(none, NO_DAY_LIGHT_SCALE);
        assert_eq!(scale_of(None), NO_DAY_LIGHT_SCALE);
    }

    #[test]
    fn the_frame_is_exposed_by_its_own_luminance_from_the_first_reading_of_its_own_hour() {
        let tables = tables();
        let at = |metered: Option<&Metered>| {
            let moment = moment(&tables, 9.5, WeatherState::Clear, 0.0).expect("a moment");
            exposed(&tables, moment, 9.5, WeatherState::Clear, metered)
        };
        let unread = moment(&tables, 9.5, WeatherState::Clear, 0.0).expect("a moment");
        // What is read in the first frame of an hour was drawn under the hour before.
        let bright = Metered::of(vec![8.0; 16]);
        let grey = at(Some(&bright));
        assert_eq!(grey.stops, unread.stops);
        // The first reading of its own is taken whole: the curve over the scene's luminance.
        let first = at(Some(&bright));
        let scene = 8.0 / grey.stops.exp2();
        let curve = -2.0 * scene.ln() - 10.0;
        assert!(
            (first.stops - curve).abs() < 1e-3,
            "{} {curve}",
            first.stops
        );
        assert_eq!(first.light_scale, first.stops.exp2() / std::f32::consts::PI);
        // A frame that reads the same scene keeps its exposure; nothing read holds it.
        let same = Metered::of(vec![scene * first.stops.exp2(); 16]);
        assert_eq!(at(Some(&same)).stops, first.stops);
        assert_eq!(at(None).stops, first.stops);
        // A scene four times as bright is moved towards, not jumped to, a moment later.
        let brighter = Metered::of(vec![4.0 * scene * first.stops.exp2(); 16]);
        let next = at(Some(&brighter)).stops;
        let wanted = curve - 2.0 * 4f32.ln();
        assert!(next <= first.stops && next > wanted, "{next} {wanted}");
    }

    #[test]
    fn every_weather_of_the_bar_names_a_cycle() {
        use WeatherState::*;
        let names = [Clear, Cloudy, Fog, Rain, Storm].map(weather_name);
        assert!(names.iter().all(|name| !name.is_empty()));
        assert_eq!(
            names.iter().collect::<std::collections::HashSet<_>>().len(),
            5
        );
    }
}
