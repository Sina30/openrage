pub const RANDOM_TABLE_LEN: usize = 256;
#[derive(Debug, Clone)]
pub struct RandomTable {
    values: [f32; RANDOM_TABLE_LEN],
}

impl Default for RandomTable {
    fn default() -> Self {
        Self::new()
    }
}

impl RandomTable {
    pub fn new() -> Self {
        Self::with_seed(0x1234_5678)
    }

    pub fn with_seed(seed: u32) -> Self {
        let mut values = [0.0_f32; RANDOM_TABLE_LEN];
        let mut state: u32 = seed.wrapping_add(0x9E37_79B9);
        for v in values.iter_mut() {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = (state as f32) / (u32::MAX as f32);
        }
        Self { values }
    }

    #[inline]
    pub fn value(&self, index: u32) -> f32 {
        self.values[(index as usize) & (RANDOM_TABLE_LEN - 1)]
    }

    #[inline]
    pub fn signed(&self, index: u32) -> f32 {
        self.value(index) * 2.0 - 1.0
    }

    pub fn vec3(&self, base_index: u32) -> [f32; 3] {
        [
            self.value(base_index),
            self.value(base_index + 1),
            self.value(base_index + 2),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn different_seeds_diverge() {
        let a = RandomTable::with_seed(0x1111_1111);
        let b = RandomTable::with_seed(0x2222_2222);
        let mut differs = 0;
        for i in 0..32 {
            if (a.value(i) - b.value(i)).abs() > 1e-6 {
                differs += 1;
            }
        }
        assert!(differs >= 28, "only {differs}/32 entries diverged");
    }

    #[test]
    fn values_stay_in_range() {
        let t = RandomTable::new();
        for i in 0..RANDOM_TABLE_LEN as u32 {
            let v = t.value(i);
            assert!((0.0..1.0).contains(&v), "value out of range: {v}");
        }
    }
}
