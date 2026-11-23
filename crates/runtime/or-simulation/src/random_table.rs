use std::sync::atomic::{AtomicU8, Ordering};
pub const PTX_RANDTABLE_SIZE: usize = 256;
struct Mt19937 {
    state: [u32; 624],
    idx: usize,
}

impl Mt19937 {
    fn new(seed: u32) -> Self {
        let mut state = [0_u32; 624];
        state[0] = seed;
        for i in 1..624 {
            state[i] = 1812433253u32
                .wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        Self { state, idx: 624 }
    }

    fn next_u32(&mut self) -> u32 {
        if self.idx >= 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7FFF_FFFF);
                let mut next = self.state[(i + 397) % 624] ^ (y >> 1);
                if (y & 1) != 0 {
                    next ^= 0x9908_B0DF;
                }
                self.state[i] = next;
            }
            self.idx = 0;
        }
        let mut y = self.state[self.idx];
        self.idx += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9D2C_5680;
        y ^= (y << 15) & 0xEFC6_0000;
        y ^= y >> 18;
        y
    }

    fn next_unit(&mut self) -> f32 {
        (self.next_u32() as f32) / (u32::MAX as f32 + 1.0)
    }
}

#[derive(Debug)]
pub struct PtxRandomTable {
    data: [f32; PTX_RANDTABLE_SIZE],
    cursor: AtomicU8,
}

impl Default for PtxRandomTable {
    fn default() -> Self {
        Self::generate(1)
    }
}

impl Clone for PtxRandomTable {
    fn clone(&self) -> Self {
        Self {
            data: self.data,
            cursor: AtomicU8::new(self.cursor.load(Ordering::Relaxed)),
        }
    }
}

impl PtxRandomTable {
    pub fn generate(seed: u32) -> Self {
        let mut data = [0.0_f32; PTX_RANDTABLE_SIZE];
        let mut rng = Mt19937::new(seed);
        for v in data.iter_mut() {
            *v = rng.next_unit();
        }
        Self {
            data,
            cursor: AtomicU8::new(0),
        }
    }

    #[inline]
    pub fn value(&self, idx: u8) -> f32 {
        self.data[idx as usize]
    }

    #[inline]
    pub fn signed(&self, idx: u8) -> f32 {
        self.value(idx) * 2.0 - 1.0
    }

    #[inline]
    pub fn next_index(&self) -> u8 {
        self.cursor.fetch_add(1, Ordering::Relaxed)
    }

    #[inline]
    pub fn next_value(&self) -> f32 {
        self.value(self.next_index())
    }

    pub fn set_cursor(&self, idx: u8) {
        self.cursor.store(idx, Ordering::Relaxed);
    }

    pub fn cursor(&self) -> u8 {
        self.cursor.load(Ordering::Relaxed)
    }

    pub fn next_signed_vec3(&self) -> glam::Vec3 {
        glam::Vec3::new(
            self.signed(self.next_index()),
            self.signed(self.next_index()),
            self.signed(self.next_index()),
        )
    }

    pub fn next_ranged_different(&self, min: u8, max: u8, current: u8) -> u8 {
        if min >= max {
            return current;
        }
        for _ in 0..6 {
            let bias = self.next_value();
            let cand = (min as f32 + (max as f32 - min as f32) * bias).round() as i32;
            let cand = cand.clamp(min as i32, max as i32) as u8;
            if cand != current {
                return cand;
            }
        }
        let next = current.wrapping_add(1);
        next.clamp(min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_wraps_at_256() {
        let t = PtxRandomTable::generate(0);
        for _ in 0..PTX_RANDTABLE_SIZE {
            t.next_index();
        }
        assert_eq!(t.cursor(), 0);
    }

    #[test]
    fn next_index_post_increments() {
        let t = PtxRandomTable::generate(0);
        t.set_cursor(7);
        assert_eq!(t.next_index(), 7);
        assert_eq!(t.next_index(), 8);
    }

    #[test]
    fn values_are_uniform_in_range() {
        let t = PtxRandomTable::generate(0xDEAD_BEEF);
        for i in 0..=u8::MAX {
            let v = t.value(i);
            assert!((0.0..1.0).contains(&v), "out of range: {v}");
        }
    }
}
