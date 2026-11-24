//! The look of the day a GTA VI install states: its time table, weather cycles, weather set,
//! modifiers and visual settings, found among the clear texts of its common archive by what each
//! opens with, and a moment of that day as a frame built for GTA V's day takes it.

use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::Path;

use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9Content, Rpf9Scan};
use or_io::gta5::data::timecycle::{Keyframe, TimeTable, Timecycle};
use or_io::gta5::data::visual_settings::VisualSettings;
use or_io::gta6::timecycle::{
    apply_modifier, blend_modifiers, frame_hours, frame_keyframe, high_level_vars, modifiers,
    sun_angle, sun_roll, sun_round, sun_towards, DayTable, FrameLight, Modifier, FRAME_DIFFUSE,
    LATITUDE,
};
use or_io::gta6::weather::{WeatherData, WeatherSet};

use crate::gta6_exposure::{light, moon_towards, ExposureSettings};

/// What a text of the day may inflate to; a longer one is passed over.
const BODY_LIMIT: u64 = 16 << 20;
/// The inflated bytes a scan keeps of each body, in which a document's root element is sought.
const HEAD: usize = 512;
/// The root element of each XML document of the day.
const ROOTS: [&str; 4] = [
    "<time_data",
    "<timecycle_keyframe_data",
    "<CContentsOfWeatherXmlFile",
    "<timecycle_modifier_data",
];
/// A name the visual settings state, by which their text is told from any other plain text.
const VISUAL_SETTINGS_MARK: &str = "Tonemapping.";

/// What a GTA VI install states of the look of its day.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DayTables {
    pub table: DayTable,
    pub timecycle: Timecycle,
    /// The `High_Level_Vars` of each cycle, by the cycle's name in lower case.
    pub high_level: BTreeMap<String, BTreeMap<String, f32>>,
    pub weather: WeatherSet,
    pub modifiers: Vec<Modifier>,
    pub visual_settings: VisualSettings,
}

/// One moment of GTA VI's day as a frame built for GTA V's day takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Moment {
    /// The frame's variables, by GTA V's names.
    pub keyframe: Keyframe,
    /// GTA VI's own variables at the moment, by its names.
    pub own: Keyframe,
    /// The time table the frame works its sun and moon out from.
    pub table: TimeTable,
    /// The hour to give the frame: that of its own day at which its sun stands where GTA VI's does.
    pub frame_hours: f32,
    /// The way towards the sun, x east, y north, z up.
    pub sun: [f32; 3],
    /// The way towards the moon.
    pub moon: [f32; 3],
    /// The exposure the moment's light is given at, in stops.
    pub stops: f32,
    /// The moment's light, which [`frame_keyframe`] gives the frame again at another exposure.
    pub light: FrameLight,
    /// What a light the game states in its own unit is multiplied by to be the frame's: the exposure over pi.
    pub light_scale: f32,
    pub moon_dim: f32,
    pub wetness: f32,
}

type Vars = BTreeMap<String, [f32; 2]>;

/// The modifier `of` holds in `slot`.
fn in_slot<'a>(of: &[(&str, &'a Vars)], slot: &str) -> Option<&'a Vars> {
    (of.iter()).find(|(s, _)| *s == slot).map(|(_, vars)| *vars)
}

/// The head of `text` a root element is sought in.
fn head(text: &str) -> &str {
    let mut end = text.len().min(HEAD);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl DayTables {
    /// The tables the texts `documents` state, each told by its root element or, for the visual settings, by a name they state; any other text is passed over.
    pub fn of_texts<'a>(documents: impl IntoIterator<Item = &'a str>) -> Self {
        let mut tables = Self::default();
        for text in documents {
            let opens = |root: &str| head(text).contains(root);
            if opens(ROOTS[0]) {
                tables.table = DayTable::parse(text);
            } else if opens(ROOTS[1]) {
                let before = tables.timecycle.cycles.len();
                tables.timecycle.add_cycles(text);
                let vars = high_level_vars(text);
                for cycle in &tables.timecycle.cycles[before..] {
                    (tables.high_level).insert(cycle.name.to_ascii_lowercase(), vars.clone());
                }
            } else if opens(ROOTS[2]) {
                match WeatherSet::parse(text) {
                    Ok(set) => tables.weather = set,
                    Err(error) => tracing::warn!(%error, "GTA VI's weather set is not read"),
                }
            } else if opens(ROOTS[3]) {
                tables.modifiers.extend(modifiers(text));
            } else if !text.trim_start().starts_with('<') && text.contains(VISUAL_SETTINGS_MARK) {
                tables.visual_settings = VisualSettings::parse(text);
            }
        }
        tables
    }

    /// Whether no cycle or no time table was read, so that no moment can be given.
    pub fn is_empty(&self) -> bool {
        self.timecycle.cycles.is_empty() || self.table.samples.is_empty()
    }

    /// The names a caller may pass [`Self::moment`] as its weather: the cycles read.
    pub fn weathers(&self) -> Vec<&str> {
        (self.timecycle.cycles.iter())
            .map(|c| c.name.as_str())
            .collect()
    }

    /// The latitude, in radians, of a point `y` metres north on the map, by the range the visual settings state.
    pub fn latitude(&self, y: f32) -> f32 {
        self.stated_latitude(y).unwrap_or(LATITUDE)
    }

    fn stated_latitude(&self, y: f32) -> Option<f32> {
        let get = |name: &str| (self.visual_settings.get(name)).and_then(|v| v.parse::<f32>().ok());
        let (low, high) = (get("sky.latitude.YPosMin")?, get("sky.latitude.YPosMax")?);
        let (from, to) = (get("sky.latitude.rangeMin")?, get("sky.latitude.rangeMax")?);
        let along = ((y - low) / (high - low)).clamp(0.0, 1.0);
        (high > low).then(|| (from + (to - from) * along).to_radians())
    }

    /// The weather data `weather` names: that of the weather type of that name, else the data of that name.
    fn data_of(&self, weather: &str) -> Option<&WeatherData> {
        (self.weather.weather(weather))
            .and_then(|w| self.weather.data(&w.data))
            .or_else(|| self.weather.data(weather))
    }

    /// The cycle that stands for `weather`: that of the weather type of that name, else the cycle of that name.
    fn cycle_of(&self, weather: &str) -> Option<&str> {
        let of_type = (self.data_of(weather)).and_then(|d| self.timecycle.cycle(&d.time_cycle));
        (of_type.or_else(|| self.timecycle.cycle(weather))).map(|c| c.name.as_str())
    }

    /// The modifier of the weather type `weather` in each of its slots, by the slot's element, in the file's order.
    fn modifiers_of(&self, weather: &str) -> Vec<(&str, &Vars)> {
        let named =
            |name: &str| (self.modifiers.iter()).find(|m| m.name.eq_ignore_ascii_case(name));
        let slots = self
            .weather
            .weather(weather)
            .map(|w| w.modifiers.as_slice());
        (slots.unwrap_or_default().iter())
            .filter_map(|(slot, name)| Some((slot.as_str(), &named(name)?.vars)))
            .collect()
    }

    /// Lay the modifiers of the weathers `from` and `to` over `own`, slot by slot, each blended by `along`.
    fn modify(&self, own: &mut Keyframe, from: &str, to: &str, along: f32) {
        let (a, b) = (self.modifiers_of(from), self.modifiers_of(to));
        let mut slots: Vec<&str> = a.iter().map(|(slot, _)| *slot).collect();
        let more: Vec<&str> = (b.iter().map(|(slot, _)| *slot))
            .filter(|slot| !slots.contains(slot))
            .collect();
        slots.extend(more);
        let none = BTreeMap::new();
        for slot in slots {
            let (x, y) = (in_slot(&a, slot), in_slot(&b, slot));
            // ⚠ Inferred: a slot one weather leaves empty is the other's modifier whole.
            let blend = blend_modifiers(x.unwrap_or(&none), y.unwrap_or(&none), along);
            apply_modifier(own, &blend);
        }
    }

    /// The moment at `hour` (0 to 24) in `weather` (a weather type's name, else a cycle's) for a camera `y` metres north on the map; `None` where no cycle is read for it or the time table is not GTA VI's.
    pub fn moment(&self, hour: f32, weather: &str, y: f32) -> Option<Moment> {
        self.moment_between(hour, weather, weather, 0.0, y)
    }

    /// The moment at `hour` on the way from weather `from` to weather `to`, `along` (0 to 1) of it: their cycles and their modifiers blended, in the GLOBAL region.
    pub fn moment_between(
        &self,
        hour: f32,
        from: &str,
        to: &str,
        along: f32,
        y: f32,
    ) -> Option<Moment> {
        let latitude = self.latitude(y);
        let round = sun_round(hour);
        let sun = sun_towards(round, latitude);
        let between = self.table.locate(sun_angle(sun))?;
        let (a, b) = (self.cycle_of(from)?, self.cycle_of(to)?);
        let along = along.clamp(0.0, 1.0);
        let mut own = (self.timecycle).keyframe_between(between, a, b, along, 0.0)?;
        self.modify(&mut own, from, to, along);
        let frame_hours = frame_hours(round);
        let visual = &self.visual_settings;
        let degrees = |name: &str| visual.get_f32(name, 0.0).to_radians();
        let moon = {
            // ⚠ Once round a day, the executable's own count at rest: the setting's name is not known.
            let day = hour / 24.0;
            let roll = visual.get_f32("misc.Sun.Roll", 122.0)
                + visual.get_f32("misc.Moon.RollOffset", 240.0);
            let wobble = visual.get_f32("misc.Moon.Wobble.Offset", 0.8)
                + visual.get_f32("misc.Moon.Wobble.Amplitude", 0.1)
                    * (std::f32::consts::TAU
                        * day
                        * visual.get_f32("misc.Moon.Wobble.Frequency", 0.0))
                    .sin();
            moon_towards(day, roll.to_radians(), wobble)
        };
        let exposure = ExposureSettings::of(&|name, default| visual.get_f32(name, default));
        let (light, stops) = light(&own, sun, moon, &exposure);
        let rain = |weather: &str| {
            self.data_of(weather)
                .map_or(0.0, |d| d.rain.clamp(0.0, 1.0))
        };
        Some(Moment {
            keyframe: frame_keyframe(&own, &light),
            own,
            table: TimeTable {
                sun_roll: sun_roll(latitude),
                // ⚠ Whether the game still reads these four of its visual settings is not checked.
                moon_roll: degrees("misc.Moon.RollOffset"),
                moon_wobble_frequency: (self.visual_settings)
                    .get_f32("misc.Moon.Wobble.Frequency", 0.0),
                moon_wobble_amplitude: (self.visual_settings)
                    .get_f32("misc.Moon.Wobble.Amplitude", 0.0),
                moon_wobble_offset: (self.visual_settings).get_f32("misc.Moon.Wobble.Offset", 0.0),
                ..TimeTable::default()
            },
            frame_hours,
            sun,
            moon,
            stops,
            light_scale: light.exposure * FRAME_DIFFUSE,
            light,
            moon_dim: visual.get_f32("misc.MoonDimMult", 1.0),
            wetness: rain(from) + (rain(to) - rain(from)) * along,
        })
    }
}

/// Read the day's tables from the GTA VI archive at `archive`, the install's common archive: every clear XML body one of the day's documents opens, and every clear plain text; `stop` ends the scan with `Interrupted`.
pub fn read_day(archive: &Path, stop: &dyn Fn() -> bool) -> io::Result<DayTables> {
    let file = File::open(archive)?;
    let mut scan = Rpf9Scan::new(&file, 0..u64::MAX)
        .map_err(io::Error::other)?
        .keeping(HEAD);
    let mut texts = Vec::new();
    while let Some(find) = scan.next() {
        if stop() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let find = find.map_err(io::Error::other)?;
        let wanted = match find.kind {
            Rpf9BodyKind::Zlib(run) if run.content == Rpf9Content::Xml => {
                let kept = String::from_utf8_lossy(scan.kept(&find));
                ROOTS.iter().any(|root| kept.contains(root))
            }
            // The visual settings state the name they are told by past the head a scan keeps.
            Rpf9BodyKind::Zlib(run) => run.content == Rpf9Content::Text,
            _ => false,
        };
        if !wanted {
            continue;
        }
        match read_rpf9_body(&file, find.offset, BODY_LIMIT) {
            Ok((_, body)) => texts.push(String::from_utf8_lossy(&body).into_owned()),
            Err(error) => {
                tracing::warn!(offset = find.offset, %error, "a text of the day is not read")
            }
        }
    }
    Ok(DayTables::of_texts(texts.iter().map(String::as_str)))
}

#[cfg(test)]
#[path = "gta6_day_tests.rs"]
mod tests;
