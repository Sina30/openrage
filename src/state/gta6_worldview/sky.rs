//! The day the GTA VI world is lit by: the install's own tables, read once on a worker and held,
//! as the day the frame takes in place of a GTA V install's.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use or_renderer::game_shaders::lighting::{night_emissive, Day};
use or_renderer::game_shaders::tone::ToneSettings;
use or_title::gta6_day::{read_day, DayTables};

use crate::state::drivers::WeatherState;

/// The archive of an install the tables of the day are in.
const ARCHIVE: &str = "common.rpf";
/// How far into the moon's cycle the first day stands: near its full, as the GTA V day has it.
const NEAR_FULL_MOON: f32 = 13.0;
/// What a light is scaled by while no GTA VI day is read and the frame keeps its own day.
const NO_DAY_LIGHT_SCALE: f32 = 0.1;

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

/// The day the frame is lit by at `hour` of the `days`th day in `weather`, for a camera `y` metres north on the map; `None` where the tables hold no cycle for it.
pub fn day_of(
    tables: &DayTables,
    hour: f32,
    days: f32,
    weather: WeatherState,
    y: f32,
) -> Option<Day> {
    let moment = tables.moment(hour, weather_name(weather), y)?;
    let visual = &tables.visual_settings;
    Some(Day {
        keyframe: moment.keyframe,
        table: moment.table,
        hours: moment.frame_hours,
        cycle_time: NEAR_FULL_MOON + days + hour / 24.0,
        moon_dim: moment.moon_dim,
        // The Weather bar's own wetness where the install's weather states no rain.
        wetness: moment.wetness.max(weather.wetness()),
        // How far a material's glow has gone to its night value: the cycle's own blend, else on by the night hours.
        night_emissive: (moment.own.get("global_emissive_scale_blend_value")).unwrap_or_else(|| {
            night_emissive(
                hour,
                visual.get_f32("emissive.night.start.time", 20.0) as u32,
                visual.get_f32("emissive.night.end.time", 7.0) as u32,
            )
        }),
        // ⚠ GTA VI's exposure curve has the frame's name and another shape (a natural logarithm, a
        // bending and offsets in place of a power): the moment is exposed by it already and the
        // frame's own exposure held at nought, so the frame is never given it.
        tone: ToneSettings::from_visual(
            &|name, default| match name.starts_with("Exposure.curve.") {
                true => default,
                false => visual.get_f32(name, default),
            },
        ),
    })
}

/// What a light the game states in its own unit is multiplied by to be the frame's at `hour` in `weather` for a camera `y` metres north: the moment's exposure over pi, and the old stand-in where no GTA VI day is read.
pub fn light_scale(tables: Option<&DayTables>, hour: f32, weather: WeatherState, y: f32) -> f32 {
    (tables.and_then(|tables| tables.moment(hour, weather_name(weather), y)))
        .map_or(NO_DAY_LIGHT_SCALE, |moment| moment.light_scale)
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
            let exposed = moment.stops.exp2() / std::f32::consts::PI;
            assert!(
                (scale - exposed).abs() <= 1e-6 * exposed,
                "{scale} {exposed}"
            );
        }
        let none = light_scale(None, 12.0, WeatherState::Clear, 0.0);
        assert_eq!(none, NO_DAY_LIGHT_SCALE);
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
