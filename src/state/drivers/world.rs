use or_particle::rules::EvolutionList;
use or_simulation::{behaviours::ConstantWaterField, PtfxManager};
use std::sync::Arc;

pub const FIRE_EVOLUTION_NAMES: &[&str] = &[
    "fire",
    "fireintensity",
    "fireptfxlevel",
    "fireptfxintensity",
];
pub const LIGHTNING_EVOLUTION_NAMES: &[&str] =
    &["lightning", "lightningintensity", "lightningptfxintensity"];
pub const VEHICLE_SPEED_EVOLUTION_NAMES: &[&str] =
    &["speed", "vehiclespeed", "ptfxspeed", "carspeed"];
pub const WETNESS_EVOLUTION_NAMES: &[&str] = &["wet", "wetlevel", "wetness", "rainsoak"];
pub const TEMPERATURE_EVOLUTION_NAMES: &[&str] = &["temp", "temperature", "ptfxtemp"];

#[derive(Debug, Clone)]
pub struct WorldEvolutionDriver {
    pub fire: f32,
    pub lightning: f32,
    pub vehicle_speed: f32,
    pub wetness: f32,
    pub temperature: f32,
    pub enabled: bool,
}

impl Default for WorldEvolutionDriver {
    fn default() -> Self {
        Self {
            fire: 0.0,
            lightning: 0.0,
            vehicle_speed: 0.0,
            wetness: 0.0,
            temperature: 0.5,
            enabled: true,
        }
    }
}

impl WorldEvolutionDriver {
    pub(crate) fn apply(&self, list: &EvolutionList, evo_values: &mut [f32]) {
        if !self.enabled {
            return;
        }
        super::write_named_evolutions(list, FIRE_EVOLUTION_NAMES, self.fire, evo_values);
        super::write_named_evolutions(list, LIGHTNING_EVOLUTION_NAMES, self.lightning, evo_values);
        super::write_named_evolutions(
            list,
            VEHICLE_SPEED_EVOLUTION_NAMES,
            self.vehicle_speed,
            evo_values,
        );
        super::write_named_evolutions(list, WETNESS_EVOLUTION_NAMES, self.wetness, evo_values);
        super::write_named_evolutions(
            list,
            TEMPERATURE_EVOLUTION_NAMES,
            self.temperature,
            evo_values,
        );
    }
}

#[derive(Debug, Clone)]
pub struct WorldPhysicsDriver {
    pub gravity: f32,
    pub air_resistance: f32,
    pub water_z: Option<f32>,
    pub enabled: bool,
}

impl Default for WorldPhysicsDriver {
    fn default() -> Self {
        Self {
            gravity: 9.81,
            air_resistance: 1.0,
            water_z: None,
            enabled: true,
        }
    }
}

impl WorldPhysicsDriver {
    pub(crate) fn apply(&self, manager: &mut PtfxManager) {
        if !self.enabled {
            return;
        }
        manager.env.gravity = self.gravity;
        manager.env.air_resistance = self.air_resistance;
        match self.water_z {
            Some(z) => {
                manager.env.water_z_camera = z;
                manager.set_water_field(Some(Arc::new(ConstantWaterField { surface_z: z })));
            }
            None => {
                manager.env.water_z_camera = 0.0;
                manager.set_water_field(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_particle::rules::Evolution;

    fn list_with(names: &[&str]) -> EvolutionList {
        EvolutionList {
            evolutions: names
                .iter()
                .map(|n| Evolution {
                    name: (*n).to_string(),
                    name_hash: 0,
                    override_value: 0.0,
                    is_overridden: false,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn world_evolution_driver_writes_all_named_slots() {
        let d = WorldEvolutionDriver {
            fire: 0.7,
            lightning: 0.3,
            vehicle_speed: 0.2,
            wetness: 0.9,
            temperature: 0.4,
            ..Default::default()
        };
        let list = list_with(&[
            "Fire",
            "Lightning",
            "VehicleSpeed",
            "WetLevel",
            "Temperature",
        ]);
        let mut evo = vec![0.0_f32; 8];
        d.apply(&list, &mut evo);
        assert!((evo[0] - 0.7).abs() < 1e-6);
        assert!((evo[1] - 0.3).abs() < 1e-6);
        assert!((evo[2] - 0.2).abs() < 1e-6);
        assert!((evo[3] - 0.9).abs() < 1e-6);
        assert!((evo[4] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn world_physics_driver_pushes_into_env_and_water_field() {
        let mut mgr = PtfxManager::new();
        let mut d = WorldPhysicsDriver {
            gravity: 1.62,
            air_resistance: 0.5,
            water_z: Some(-3.5),
            ..Default::default()
        };
        d.apply(&mut mgr);
        assert!((mgr.env.gravity - 1.62).abs() < 1e-6);
        assert!((mgr.env.air_resistance - 0.5).abs() < 1e-6);
        assert!((mgr.env.water_z_camera - -3.5).abs() < 1e-6);
        assert!(mgr.water_field.is_some());

        d.water_z = None;
        d.apply(&mut mgr);
        assert!(mgr.water_field.is_none());
    }
}
