use glam::Vec3;
#[derive(Debug, Clone)]
pub struct EffectHistoryEntry {
    pub fx_hash: u32,
    pub sub_type: u32,
    pub world_pos: Vec3,
    pub remaining_alive: f32,
}

#[derive(Debug, Default, Clone)]
pub struct EffectHistory {
    entries: Vec<EffectHistoryEntry>,
    pub max_size: usize,
}

impl EffectHistory {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max_size),
            max_size: max_size.max(1),
        }
    }

    pub fn add(&mut self, fx_hash: u32, sub_type: u32, world_pos: Vec3, alive_time: f32) {
        if self.entries.len() >= self.max_size {
            self.entries.remove(0);
        }
        self.entries.push(EffectHistoryEntry {
            fx_hash,
            sub_type,
            world_pos,
            remaining_alive: alive_time.max(0.0),
        });
    }

    pub fn check(&self, fx_hash: u32, sub_type: u32, world_pos: Vec3, range: f32) -> bool {
        let r2 = range * range;
        self.entries.iter().any(|e| {
            e.fx_hash == fx_hash
                && e.sub_type == sub_type
                && e.remaining_alive > 0.0
                && (e.world_pos - world_pos).length_squared() <= r2
        })
    }

    pub fn tick(&mut self, dt: f32) {
        for e in &mut self.entries {
            e.remaining_alive -= dt;
        }
        self.entries.retain(|e| e.remaining_alive > 0.0);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn add_then_check_within_range_returns_true() {
        let mut h = EffectHistory::new(4);
        h.add(0xDEAD_BEEF, 0, Vec3::ZERO, 1.0);
        assert!(h.check(0xDEAD_BEEF, 0, Vec3::new(0.4, 0.0, 0.0), 0.5));
        assert!(!h.check(0xDEAD_BEEF, 0, Vec3::new(2.0, 0.0, 0.0), 0.5));
        assert!(!h.check(0xDEAD_BEEF, 1, Vec3::ZERO, 0.5));
    }

    #[test]
    fn tick_expires_entries() {
        let mut h = EffectHistory::new(4);
        h.add(0xAAAA, 0, Vec3::ZERO, 0.5);
        h.tick(0.6);
        assert_eq!(h.len(), 0);
        assert!(!h.check(0xAAAA, 0, Vec3::ZERO, 1.0));
    }

    #[test]
    fn fifo_eviction_when_at_capacity() {
        let mut h = EffectHistory::new(2);
        h.add(0x1, 0, Vec3::ZERO, 1.0);
        h.add(0x2, 0, Vec3::ZERO, 1.0);
        h.add(0x3, 0, Vec3::ZERO, 1.0);
        assert_eq!(h.len(), 2);
        assert!(!h.check(0x1, 0, Vec3::ZERO, 1.0));
        assert!(h.check(0x3, 0, Vec3::ZERO, 1.0));
    }
}
