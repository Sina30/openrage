use or_particle::rules::EvolutionList;

pub const EXPOSURE_EVOLUTION_NAMES: &[&str] = &["exposure", "hdrexposure", "luminance"];

pub const DEFAULT_EXPOSURE: f32 = 0.5;

#[derive(Debug, Clone)]
pub struct ExposureDriver {
    pub value: f32,
    pub enabled: bool,
}

impl Default for ExposureDriver {
    fn default() -> Self {
        Self {
            value: DEFAULT_EXPOSURE,
            enabled: true,
        }
    }
}

impl ExposureDriver {
    pub fn set_value(&mut self, v: f32) {
        self.value = v.clamp(0.0, 1.0);
    }

    pub(crate) fn apply(&self, list: &EvolutionList, evo_values: &mut [f32]) {
        if !self.enabled {
            return;
        }
        super::write_named_evolutions(list, EXPOSURE_EVOLUTION_NAMES, self.value, evo_values);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_particle::rules::Evolution;

    #[test]
    fn exposure_slider_drives_evolution_values() {
        let mut driver = ExposureDriver::default();
        let list = EvolutionList {
            evolutions: vec![Evolution {
                name: "Exposure".into(),
                name_hash: 0,
                override_value: 0.0,
                is_overridden: false,
            }],
            ..Default::default()
        };
        let mut evo = vec![0.0_f32; 6];
        driver.set_value(0.7);
        driver.apply(&list, &mut evo);
        assert!((evo[0] - 0.7).abs() < 1e-6);
    }
}
