//! What the World screen keeps between runs: the hour, the weather, the game's modes, the
//! level-of-detail scale and the camera's speed.

use serde::{Deserialize, Serialize};

use crate::state::drivers::WeatherState;
use crate::state::gta6_world::worldview::Viewer;

/// The World screen's controls, as the person last left them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorldViewerPrefs {
    pub hour: f32,
    /// The weather by the name of its cycle in the install's timecycle.
    pub weather: &'static str,
    /// Bit 0 the base game, bit 1 story, bit 2 the shared minigames.
    pub modes: u8,
    pub lod_scale: f32,
    /// Metres a second.
    pub speed: f32,
}

impl Default for WorldViewerPrefs {
    fn default() -> Self {
        let viewer = Viewer::default();
        Self {
            hour: viewer.hour,
            weather: WeatherState::default().cycle_name(),
            modes: viewer.enabled.modes,
            lod_scale: viewer.lod_scale,
            speed: viewer.camera.speed,
        }
    }
}

/// The ranges the screen's controls take, which a value read from the file is held to.
pub const HOURS: std::ops::Range<f32> = 0.0..24.0;
pub const LOD_SCALES: std::ops::RangeInclusive<f32> = 0.5..=4.0;
pub const SPEEDS: std::ops::RangeInclusive<f32> = 1.0..=2000.0;

impl WorldViewerPrefs {
    /// What the screen shows now.
    pub fn of(viewer: &Viewer, weather: WeatherState) -> Self {
        Self {
            hour: viewer.hour,
            weather: weather.cycle_name(),
            modes: viewer.enabled.modes,
            lod_scale: viewer.lod_scale,
            speed: viewer.camera.speed,
        }
    }

    /// The weather kept.
    pub fn weather(&self) -> WeatherState {
        (WeatherState::ALL.into_iter())
            .find(|w| w.cycle_name() == self.weather)
            .unwrap_or_default()
    }

    /// Give the viewer what is kept; the weather and the hour of the frame's day are the caller's.
    pub fn put(&self, viewer: &mut Viewer) {
        viewer.hour = self.hour;
        viewer.enabled.modes = self.modes;
        viewer.lod_scale = self.lod_scale;
        viewer.camera.speed = self.speed;
    }

    /// ⚠ Field by field: a value that does not read, or reads out of its range, costs only itself.
    fn from_value(value: &serde_json::Value) -> Self {
        let mut out = Self::default();
        let Some(map) = value.as_object() else {
            return out;
        };
        let number = |key: &str| {
            (map.get(key).and_then(serde_json::Value::as_f64))
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
        };
        out.hour = number("hour")
            .filter(|h| HOURS.contains(h))
            .unwrap_or(out.hour);
        out.lod_scale =
            (number("lod_scale").filter(|s| LOD_SCALES.contains(s))).unwrap_or(out.lod_scale);
        out.speed = number("speed")
            .filter(|s| SPEEDS.contains(s))
            .unwrap_or(out.speed);
        if let Some(weather) =
            (map.get("weather").and_then(serde_json::Value::as_str)).and_then(|name| {
                WeatherState::ALL
                    .into_iter()
                    .find(|w| w.cycle_name() == name)
            })
        {
            out.weather = weather.cycle_name();
        }
        // The base game always streams: a kept value only adds the packs.
        if let Some(modes) = map.get("modes").and_then(serde_json::Value::as_u64) {
            out.modes = (modes as u8 & 0b110) | 1;
        }
        out
    }
}

impl<'de> Deserialize<'de> for WorldViewerPrefs {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        Ok(Self::from_value(&value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_screen_shows_is_kept_and_read_back() {
        let kept = WorldViewerPrefs {
            hour: 21.5,
            weather: WeatherState::Rain.cycle_name(),
            modes: 0b101,
            lod_scale: 2.0,
            speed: 120.0,
        };
        let text = serde_json::to_string(&kept).expect("written");
        let read: WorldViewerPrefs = serde_json::from_str(&text).expect("read");
        assert_eq!(read, kept);
        assert_eq!(read.weather(), WeatherState::Rain);
        let mut viewer = Viewer::default();
        read.put(&mut viewer);
        assert_eq!(WorldViewerPrefs::of(&viewer, read.weather()), kept);
    }

    #[test]
    fn a_value_that_does_not_read_costs_only_itself() {
        let read: WorldViewerPrefs = serde_json::from_str(
            r#"{"hour": 30, "weather": "SNOW", "modes": 6, "lod_scale": "far", "speed": 50}"#,
        )
        .expect("read");
        let default = WorldViewerPrefs::default();
        assert_eq!(read.hour, default.hour, "an hour past the day");
        assert_eq!(read.weather, default.weather, "a weather no cycle names");
        assert_eq!(
            read.lod_scale, default.lod_scale,
            "a scale that is no number"
        );
        assert_eq!(read.speed, 50.0);
        assert_eq!(read.modes, 0b111, "the base game streams whatever is kept");
        let nothing: WorldViewerPrefs = serde_json::from_str("[1, 2]").expect("read");
        assert_eq!(nothing, default);
    }
}
