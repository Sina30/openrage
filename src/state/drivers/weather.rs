use or_particle::rules::EvolutionList;

pub const WEATHER_EVOLUTION_NAMES: &[&str] = &["weather", "weathertype", "rain", "wetness"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeatherState {
    #[default]
    Clear,
    Cloudy,
    Rain,
    Storm,
    Fog,
}

impl WeatherState {
    pub const ALL: [WeatherState; 5] = [
        WeatherState::Clear,
        WeatherState::Cloudy,
        WeatherState::Rain,
        WeatherState::Storm,
        WeatherState::Fog,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WeatherState::Clear => tr!("Clear sky"),
            WeatherState::Cloudy => tr!("Cloudy"),
            WeatherState::Rain => tr!("Rain"),
            WeatherState::Storm => tr!("Storm"),
            WeatherState::Fog => tr!("Fog"),
        }
    }

    /// The weather cycle of the install's timecycle this state is.
    pub fn cycle_name(self) -> &'static str {
        match self {
            WeatherState::Clear => "CLEAR",
            WeatherState::Cloudy => "CLOUDS",
            WeatherState::Rain => "RAIN",
            WeatherState::Storm => "THUNDER",
            WeatherState::Fog => "FOGGY",
        }
    }

    /// How wet the world is once this weather has lasted: the install's rain for the cycle, which is one for RAIN and THUNDER and nought for the rest, by the game's multiplier of one and a half and held to one.
    pub fn wetness(self) -> f32 {
        match self {
            WeatherState::Rain | WeatherState::Storm => 1.0,
            WeatherState::Clear | WeatherState::Cloudy | WeatherState::Fog => 0.0,
        }
    }

    pub fn value(self) -> f32 {
        match self {
            WeatherState::Clear => 0.0,
            WeatherState::Cloudy => 0.25,
            WeatherState::Rain => 0.5,
            WeatherState::Storm => 0.75,
            WeatherState::Fog => 1.0,
        }
    }

    pub fn wind_velocity(self) -> glam::Vec3 {
        let magnitude = match self {
            WeatherState::Clear => 0.5,
            WeatherState::Cloudy => 1.5,
            WeatherState::Rain => 4.0,
            WeatherState::Storm => 12.0,
            WeatherState::Fog => 0.8,
        };
        glam::Vec3::new(magnitude, 0.0, 0.0)
    }

    pub fn fog_density(self) -> f32 {
        match self {
            WeatherState::Clear => 0.05,
            WeatherState::Cloudy => 0.15,
            WeatherState::Rain => 0.30,
            WeatherState::Storm => 0.50,
            WeatherState::Fog => 1.00,
        }
    }
}

pub const FOG_EVOLUTION_NAMES: &[&str] = &["fog", "fogdensity", "envfog", "weatherfog"];
pub const WIND_EVOLUTION_NAMES: &[&str] = &["wind", "windspeed", "windstrength"];

#[derive(Debug, Clone, Default)]
pub struct WeatherDriver {
    pub state: WeatherState,
    pub enabled: bool,
}

impl WeatherDriver {
    pub fn set_state(&mut self, new_state: WeatherState) -> bool {
        if self.state == new_state {
            return false;
        }
        self.state = new_state;
        true
    }

    pub fn value(&self) -> f32 {
        self.state.value()
    }

    pub(crate) fn apply(&self, list: &EvolutionList, evo_values: &mut [f32]) {
        if !self.enabled {
            return;
        }
        super::write_named_evolutions(list, WEATHER_EVOLUTION_NAMES, self.value(), evo_values);
        super::write_named_evolutions(
            list,
            FOG_EVOLUTION_NAMES,
            self.state.fog_density(),
            evo_values,
        );
        let wind_strength = self.state.wind_velocity().length();
        super::write_named_evolutions(list, WIND_EVOLUTION_NAMES, wind_strength, evo_values);
    }

    pub fn wind_velocity(&self) -> glam::Vec3 {
        if !self.enabled {
            glam::Vec3::ZERO
        } else {
            self.state.wind_velocity()
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WeatherDrivenWindField {
    pub global: glam::Vec3,
    pub gust_strength: f32,
    pub spatial_frequency: f32,
    pub temporal_frequency: f32,
}

impl Default for WeatherDrivenWindField {
    fn default() -> Self {
        Self {
            global: glam::Vec3::ZERO,
            gust_strength: 0.25,
            spatial_frequency: 0.05,
            temporal_frequency: 0.7,
        }
    }
}

impl WeatherDrivenWindField {
    pub fn from_driver(driver: &WeatherDriver) -> Self {
        Self {
            global: driver.wind_velocity(),
            ..Self::default()
        }
    }
}

impl or_simulation::behaviours::WindField for WeatherDrivenWindField {
    fn sample_local(&self, pos: glam::Vec3, time_seconds: f32) -> glam::Vec3 {
        let mag = self.global.length();
        if mag < 1e-6 {
            return self.global;
        }
        let f = self.spatial_frequency;
        let w = self.temporal_frequency;
        let s1 = (pos.x * f * 1.0 + pos.z * f * 0.7 + time_seconds * w * 1.0).sin();
        let c1 = (pos.y * f * 0.9 + pos.z * f * 0.6 + time_seconds * w * 0.8).cos();
        let s2 = (pos.x * f * 2.3 - pos.z * f * 1.7 + time_seconds * w * 1.9).sin();
        let c2 = (pos.y * f * 1.7 + pos.x * f * 1.1 - time_seconds * w * 1.5).cos();
        let gust = glam::Vec3::new(s1 + s2 * 0.4, c1 * 0.5 + s2 * 0.2, s1 * c1 + c2 * 0.3);
        self.global + gust * mag * self.gust_strength
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_particle::rules::Evolution;

    #[test]
    fn weather_driver_writes_matching_slot() {
        let mut driver = WeatherDriver {
            enabled: true,
            ..Default::default()
        };
        let list = EvolutionList {
            evolutions: vec![Evolution {
                name: "Weather".into(),
                name_hash: 0,
                override_value: 0.0,
                is_overridden: false,
            }],
            ..Default::default()
        };
        let mut evo = vec![0.0_f32; 6];
        driver.set_state(WeatherState::Rain);
        driver.apply(&list, &mut evo);
        assert!((evo[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn set_state_reports_change() {
        let mut driver = WeatherDriver::default();
        assert!(!driver.set_state(WeatherState::Clear), "no-op when same");
        assert!(driver.set_state(WeatherState::Rain));
        assert!(!driver.set_state(WeatherState::Rain));
        assert!(driver.set_state(WeatherState::Fog));
    }
}
