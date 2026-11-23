// .NET's `System.Random`, reproduced exactly: `magic.dat` is masked with its
// `NextBytes` stream, seeded from a hash of the user's AES key.

const MBIG: i32 = i32::MAX;
const MSEED: i32 = 161_803_398;

pub struct DotNetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl DotNetRandom {
    pub fn new(seed: i32) -> Self {
        let subtraction = if seed == i32::MIN {
            i32::MAX
        } else {
            seed.wrapping_abs()
        };
        let mut seed_array = [0i32; 56];
        let mut mj = MSEED.wrapping_sub(subtraction);
        seed_array[55] = mj;
        let mut mk: i32 = 1;
        for i in 1..55 {
            let ii = (21 * i) % 55;
            seed_array[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(MBIG);
            }
            mj = seed_array[ii];
        }
        for _ in 1..5 {
            for i in 1..56 {
                let idx2 = 1 + (i + 30) % 55;
                seed_array[i] = seed_array[i].wrapping_sub(seed_array[idx2]);
                if seed_array[i] < 0 {
                    seed_array[i] = seed_array[i].wrapping_add(MBIG);
                }
            }
        }
        Self {
            seed_array,
            inext: 0,
            inextp: 21,
        }
    }

    fn internal_sample(&mut self) -> i32 {
        let mut loc_inext = self.inext + 1;
        if loc_inext >= 56 {
            loc_inext = 1;
        }
        let mut loc_inextp = self.inextp + 1;
        if loc_inextp >= 56 {
            loc_inextp = 1;
        }
        let mut ret = self.seed_array[loc_inext].wrapping_sub(self.seed_array[loc_inextp]);
        if ret == MBIG {
            ret -= 1;
        }
        if ret < 0 {
            ret = ret.wrapping_add(MBIG);
        }
        self.seed_array[loc_inext] = ret;
        self.inext = loc_inext;
        self.inextp = loc_inextp;
        ret
    }

    pub fn next_bytes(&mut self, buffer: &mut [u8]) {
        for slot in buffer.iter_mut() {
            *slot = (self.internal_sample() % 256) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_dotnet_for_seed_42() {
        let mut r = DotNetRandom::new(42);
        let mut buf = [0u8; 16];
        r.next_bytes(&mut buf);
        let expected: [u8; 16] = [
            0x3E, 0x17, 0xBA, 0x96, 0xAE, 0x04, 0xCD, 0x3B, 0x99, 0x86, 0x9E, 0x56, 0xF0, 0xAD,
            0xBF, 0x3A,
        ];
        assert_eq!(buf, expected);
    }

    #[test]
    fn matches_dotnet_for_seed_0() {
        let mut r = DotNetRandom::new(0);
        let mut buf = [0u8; 8];
        r.next_bytes(&mut buf);
        let expected: [u8; 8] = [0x1A, 0x0C, 0x46, 0x6F, 0x5D, 0x75, 0xE4, 0xD8];
        assert_eq!(buf, expected);
    }

    #[test]
    fn matches_dotnet_for_seed_neg_1() {
        let mut r = DotNetRandom::new(-1);
        let mut buf = [0u8; 8];
        r.next_bytes(&mut buf);
        let expected: [u8; 8] = [0x46, 0xD0, 0x86, 0x82, 0x40, 0x97, 0xE4, 0xA3];
        assert_eq!(buf, expected);
    }

    #[test]
    fn matches_dotnet_for_large_seed() {
        let mut r = DotNetRandom::new(1_234_567_890);
        let mut buf = [0u8; 8];
        r.next_bytes(&mut buf);
        let expected: [u8; 8] = [0x8E, 0x56, 0x15, 0x22, 0x78, 0xA2, 0x4E, 0xDD];
        assert_eq!(buf, expected);
    }

    #[test]
    fn int_min_seed_aliases_int_max() {
        let mut r1 = DotNetRandom::new(i32::MIN);
        let mut buf1 = [0u8; 16];
        r1.next_bytes(&mut buf1);
        let mut r2 = DotNetRandom::new(i32::MAX);
        let mut buf2 = [0u8; 16];
        r2.next_bytes(&mut buf2);
        assert_eq!(buf1, buf2);
    }
}
