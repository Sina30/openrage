use or_particle::rules::EvolutionList;

pub const TOD_EVOLUTION_NAMES: &[&str] = &["timeofday", "tod", "time_of_day", "hour"];

/// How fast a running day goes: a whole one in 24 seconds.
const HOURS_PER_SECOND: f32 = 1.0;

#[derive(Debug, Clone)]
pub struct TimeOfDayDriver {
    pub hour: f32,
    pub enabled: bool,
    /// Whether the day runs.
    pub playing: bool,
    /// The whole days the running day has gone through, which the moon's phase and the clouds follow.
    pub days: f32,
}

impl Default for TimeOfDayDriver {
    fn default() -> Self {
        Self {
            hour: 12.0,
            enabled: true,
            playing: false,
            days: 0.0,
        }
    }
}

impl TimeOfDayDriver {
    pub fn set_hour(&mut self, h: f32) {
        self.hour = h.clamp(0.0, 24.0);
    }

    /// Let `seconds` of a running day go by: the hour moves on, and the day after the last hour is the next one.
    pub fn advance(&mut self, seconds: f32) {
        if !self.playing {
            return;
        }
        let hour = self.hour + seconds * HOURS_PER_SECOND;
        self.days += (hour / 24.0).floor();
        self.hour = hour.rem_euclid(24.0);
    }

    pub fn value(&self) -> f32 {
        (self.hour / 24.0).clamp(0.0, 1.0)
    }

    pub fn ambient_scales(&self) -> (f32, f32, bool) {
        let t = ((self.hour - 12.0) / 24.0) * std::f32::consts::TAU;
        let day = (t.cos() + 1.0) * 0.5;
        let natural = 0.05 + day * 0.95;
        let artificial = 0.1 + (1.0 - day) * 0.9;
        (natural, artificial, false)
    }

    pub(crate) fn apply(&self, list: &EvolutionList, evo_values: &mut [f32]) {
        if !self.enabled {
            return;
        }
        super::write_named_evolutions(list, TOD_EVOLUTION_NAMES, self.value(), evo_values);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_particle::rules::Evolution;

    fn make_list() -> EvolutionList {
        EvolutionList {
            evolutions: vec![Evolution {
                name: "TimeOfDay".into(),
                name_hash: 0,
                override_value: 0.0,
                is_overridden: false,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn time_of_day_drives_evolution_values() {
        let mut driver = TimeOfDayDriver::default();
        let list = make_list();
        let mut evo = vec![0.0_f32; 6];
        driver.set_hour(6.0);
        driver.apply(&list, &mut evo);
        assert!(
            (evo[0] - 0.25).abs() < 1e-6,
            "6h / 24h should normalize to 0.25, got {}",
            evo[0]
        );
        driver.set_hour(18.0);
        driver.apply(&list, &mut evo);
        assert!(
            (evo[0] - 0.75).abs() < 1e-6,
            "18h / 24h should normalize to 0.75, got {}",
            evo[0]
        );
    }

    #[test]
    fn disabled_driver_writes_nothing() {
        let driver = TimeOfDayDriver {
            enabled: false,
            ..Default::default()
        };
        let list = make_list();
        let mut evo = vec![0.5_f32; 6];
        driver.apply(&list, &mut evo);
        assert_eq!(evo[0], 0.5, "disabled driver must not mutate slots");
    }

    #[test]
    fn a_running_day_moves_the_hour_on_and_counts_the_days_it_goes_through() {
        let mut driver = TimeOfDayDriver::default();
        driver.set_hour(23.0);
        driver.advance(5.0);
        assert_eq!(
            (driver.hour, driver.days),
            (23.0, 0.0),
            "a day that is not running stands"
        );
        driver.playing = true;
        driver.advance(2.5);
        assert_eq!((driver.hour, driver.days), (1.5, 1.0));
        driver.advance(48.0);
        assert_eq!((driver.hour, driver.days), (1.5, 3.0));
    }

    #[test]
    fn hour_clamps_into_range() {
        let mut driver = TimeOfDayDriver::default();
        driver.set_hour(-5.0);
        assert_eq!(driver.hour, 0.0);
        driver.set_hour(99.0);
        assert_eq!(driver.hour, 24.0);
    }
}
