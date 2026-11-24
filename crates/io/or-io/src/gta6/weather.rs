//! GTA VI's weather set (`CContentsOfWeatherXmlFile`): the cycle files, what each weather is made
//! of, and the named weathers that lay modifiers over one of them.

use std::collections::BTreeMap;

use crate::gta5::data::timecycle::{attr, tokens, Token};

const ROOT: &str = "CContentsOfWeatherXmlFile";

/// Why a text is not a weather set.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WeatherError {
    #[error("the text is not a weather set: its root element is `{0}`")]
    NotAWeatherSet(String),
    #[error("the weather set states `{value}` for `{element}`, which is no number")]
    Value { element: String, value: String },
}

/// One `WeatherTypeDatas` item: what a weather is made of.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeatherData {
    pub name: String,
    /// The name of its cycle, as the cycle's file states it.
    pub time_cycle: String,
    pub sun: f32,
    pub rain: f32,
    pub snow: f32,
    pub fog: f32,
    pub cloud_type: f32,
    pub wind_min: f32,
    pub wind_max: f32,
    pub wind_angle: f32,
    /// Every other single value of the item, by element name, a flag as 0 or 1: the water and ripple settings among them.
    pub values: BTreeMap<String, f32>,
    pub cloud_maps: [String; 2],
}

/// One `WeatherTypes` item: a named weather, its data and the modifiers laid over it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeatherType {
    pub name: String,
    pub short_name: String,
    pub data: String,
    /// The modifiers its `Mod...` elements name, in the file's order: the element, which is the modifier's slot, and the modifier's name.
    pub modifiers: Vec<(String, String)>,
    pub app: String,
}

/// What a weather set states.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeatherSet {
    pub cycle_files: Vec<String>,
    pub datas: Vec<WeatherData>,
    pub types: Vec<WeatherType>,
    /// `WeatherTypeIndirectionList`: a name and the type it stands for.
    pub indirections: Vec<(String, String)>,
}

impl WeatherData {
    fn set(&mut self, element: &str, value: &str) -> Result<(), WeatherError> {
        let number = match value.trim() {
            "true" => 1.0,
            "false" => 0.0,
            other => other.parse().map_err(|_| WeatherError::Value {
                element: element.to_string(),
                value: value.to_string(),
            })?,
        };
        match element {
            "Sun" => self.sun = number,
            "Rain" => self.rain = number,
            "Snow" => self.snow = number,
            "Fog" => self.fog = number,
            "CloudType" => self.cloud_type = number,
            "WindMin" => self.wind_min = number,
            "WindMax" => self.wind_max = number,
            "WindAngle" => self.wind_angle = number,
            _ => {
                self.values.insert(element.to_string(), number);
            }
        }
        Ok(())
    }
}

impl WeatherSet {
    pub fn parse(text: &str) -> Result<Self, WeatherError> {
        let mut set = Self::default();
        let mut path: Vec<&str> = Vec::new();
        let mut rooted = false;
        for token in tokens(text) {
            match token {
                Token::Open { name, attrs, empty } => {
                    if !rooted && name != ROOT {
                        return Err(WeatherError::NotAWeatherSet(name.to_string()));
                    }
                    rooted = true;
                    path.push(name);
                    match &path[1..] {
                        ["WeatherTypeDatas", "Item"] => set.datas.push(WeatherData::default()),
                        ["WeatherTypes", "Item"] => set.types.push(WeatherType::default()),
                        ["WeatherTypeIndirectionList", "Item"] => {
                            set.indirections.push(Default::default())
                        }
                        ["WeatherTypeDatas", "Item", field] => {
                            if let (Some(data), Some(value)) =
                                (set.datas.last_mut(), attr(attrs, "value"))
                            {
                                data.set(field, value)?;
                            }
                        }
                        _ => {}
                    }
                    if empty {
                        path.pop();
                    }
                }
                Token::Close(_) => {
                    path.pop();
                }
                Token::Text(text) => set.text(&path, text),
            }
        }
        match rooted {
            true => Ok(set),
            false => Err(WeatherError::NotAWeatherSet(String::new())),
        }
    }

    /// Take the text of the element at `path`.
    fn text(&mut self, path: &[&str], text: &str) {
        let text = text.to_string();
        match path.get(1..).unwrap_or_default() {
            ["WeatherTimeCycleFileNames", "Item"] => self.cycle_files.push(text),
            ["WeatherTypeDatas", "Item", field] => {
                let Some(data) = self.datas.last_mut() else {
                    return;
                };
                match *field {
                    "Name" => data.name = text,
                    "TimeCycleName" => data.time_cycle = text,
                    "CloudMapA" => data.cloud_maps[0] = text,
                    "CloudMapB" => data.cloud_maps[1] = text,
                    _ => {}
                }
            }
            ["WeatherTypes", "Item", field] => {
                let Some(weather) = self.types.last_mut() else {
                    return;
                };
                match *field {
                    "Name" => weather.name = text,
                    "ShortName" => weather.short_name = text,
                    "DataName" => weather.data = text,
                    "WeatherAppDataName" => weather.app = text,
                    field if field.starts_with("Mod") => {
                        weather.modifiers.push((field.to_string(), text))
                    }
                    _ => {}
                }
            }
            ["WeatherTypeIndirectionList", "Item", field] => {
                let Some(pair) = self.indirections.last_mut() else {
                    return;
                };
                match *field {
                    "From" => pair.0 = text,
                    "To" => pair.1 = text,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// The type of `name`, through an indirection where one names it; case is not told apart.
    pub fn weather(&self, name: &str) -> Option<&WeatherType> {
        let named = |name: &str| (self.types.iter()).find(|t| t.name.eq_ignore_ascii_case(name));
        let through = (self.indirections.iter())
            .find(|(from, _)| from.eq_ignore_ascii_case(name))
            .and_then(|(_, to)| named(to));
        through.or_else(|| named(name))
    }

    pub fn data(&self, name: &str) -> Option<&WeatherData> {
        (self.datas.iter()).find(|d| d.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
#[path = "weather_tests.rs"]
mod tests;
