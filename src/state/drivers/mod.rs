mod exposure;
mod lod_distance;
mod time_of_day;
pub mod weather;
mod world;
mod world_background;

pub use exposure::ExposureDriver;
pub use lod_distance::LodDistanceDriver;
pub use time_of_day::TimeOfDayDriver;
pub use weather::{WeatherDriver, WeatherState};
pub use world::{WorldEvolutionDriver, WorldPhysicsDriver};
pub use world_background::WorldBackgroundDriver;

use or_particle::rules::EvolutionList;
use or_simulation::{EffectInst, PtfxManager};

#[derive(Debug, Clone, Default)]
pub struct Drivers {
    pub time_of_day: TimeOfDayDriver,
    pub weather: WeatherDriver,
    pub exposure: ExposureDriver,
    pub world_background: WorldBackgroundDriver,
    pub lod_distance: LodDistanceDriver,
    pub world_evolution: WorldEvolutionDriver,
    pub world_physics: WorldPhysicsDriver,
}

impl Drivers {
    /// The drivers a launch starts with: the viewport's background and exposure as `viewport` keeps them.
    pub fn restored(viewport: &super::ViewportPrefs) -> Self {
        let mut drivers = Self::default();
        drivers.world_background.colour = viewport.background;
        drivers.world_background.sky = viewport.sky;
        drivers.world_background.ground = viewport.ground;
        drivers.exposure.set_value(viewport.exposure);
        drivers
    }

    pub fn apply_to_eff(&self, eff: &mut EffectInst) {
        let (natural, artificial, in_interior) = self.time_of_day.ambient_scales();
        eff.set_ambient_scales(natural, artificial, in_interior);
        let Some(list) = eff.evolution_list.as_ref() else {
            return;
        };
        let snapshot = list.clone();
        self.time_of_day.apply(&snapshot, &mut eff.evo_values);
        self.weather.apply(&snapshot, &mut eff.evo_values);
        self.exposure.apply(&snapshot, &mut eff.evo_values);
        self.world_evolution.apply(&snapshot, &mut eff.evo_values);
    }

    pub fn apply_to_eff_with_pinned(&self, manager: &mut PtfxManager) {
        self.lod_distance.apply(manager);
        self.world_physics.apply(manager);
        if let Some(eff) = manager.effect.as_mut() {
            self.apply_to_eff(eff);
        }
        for eff in manager.spawned_effects.iter_mut() {
            self.apply_to_eff(eff);
        }
    }
}

pub(crate) fn write_named_evolutions(
    list: &EvolutionList,
    candidates: &[&str],
    value: f32,
    evo_values: &mut [f32],
) -> usize {
    let mut written = 0usize;
    for (idx, evo) in list.evolutions.iter().enumerate() {
        if evo.is_overridden {
            continue;
        }
        if idx >= evo_values.len() {
            continue;
        }
        let lowered = evo.name.to_ascii_lowercase();
        if candidates.contains(&lowered.as_str()) {
            evo_values[idx] = value;
            written += 1;
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_particle::rules::Evolution;

    fn list_with(names: &[(&str, bool)]) -> EvolutionList {
        EvolutionList {
            evolutions: names
                .iter()
                .map(|(n, ovr)| Evolution {
                    name: (*n).to_string(),
                    name_hash: 0,
                    override_value: 0.0,
                    is_overridden: *ovr,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn write_named_evolutions_skips_overridden() {
        let list = list_with(&[("TimeOfDay", true), ("Weather", false)]);
        let mut evo = vec![0.0_f32; 6];
        let n = write_named_evolutions(&list, &["timeofday"], 0.5, &mut evo);
        assert_eq!(n, 0, "overridden slots must not be written");
        assert_eq!(evo[0], 0.0);
    }

    #[test]
    fn write_named_evolutions_writes_matching_slots() {
        let list = list_with(&[("TimeOfDay", false), ("WEATHER", false), ("other", false)]);
        let mut evo = vec![0.0_f32; 6];
        let n = write_named_evolutions(&list, &["timeofday", "weather"], 0.75, &mut evo);
        assert_eq!(n, 2);
        assert_eq!(evo[0], 0.75);
        assert_eq!(evo[1], 0.75);
        assert_eq!(evo[2], 0.0, "non-matching slot stays at default");
    }
}
