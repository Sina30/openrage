use or_simulation::PtfxManager;

#[derive(Debug, Clone)]
pub struct LodDistanceDriver {
    pub value: f32,
    pub enabled: bool,
}

impl Default for LodDistanceDriver {
    fn default() -> Self {
        Self {
            value: 1.0,
            enabled: false,
        }
    }
}

impl LodDistanceDriver {
    pub fn apply(&self, manager: &mut PtfxManager) {
        manager.env.lod_distance_override = if self.enabled {
            Some(self.value.clamp(0.0, 1.0))
        } else {
            None
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_writes_override_when_enabled() {
        let mut mgr = PtfxManager::new();
        let mut d = LodDistanceDriver {
            enabled: true,
            ..Default::default()
        };
        d.value = 0.42;
        d.apply(&mut mgr);
        assert_eq!(mgr.env.lod_distance_override, Some(0.42));
    }

    #[test]
    fn apply_clears_override_when_disabled() {
        let mut mgr = PtfxManager::new();
        mgr.env.lod_distance_override = Some(0.7);
        let d = LodDistanceDriver::default();
        d.apply(&mut mgr);
        assert!(mgr.env.lod_distance_override.is_none());
    }
}
