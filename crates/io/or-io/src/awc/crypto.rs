//! The XXTEA variant an obfuscated `.awc` is scrambled with.

use crate::container::rpf::crypto::AWC_KEY_WORDS;

/// The key an obfuscated `.awc` is scrambled with, as four 32-bit words.
///
/// ⚠ **Zero when the key set has none.** `or_io::container::rpf::crypto`'s
/// fingerprint scan recovers it from both editions' executables, but a cache
/// an older build wrote in AES-only mode carries none, and
/// `AwcKey::is_usable` is what a caller asks before offering to open one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AwcKey(pub [u32; AWC_KEY_WORDS]);

impl AwcKey {
    pub fn is_usable(self) -> bool {
        self.0.iter().any(|w| *w != 0)
    }
}

impl From<[u32; AWC_KEY_WORDS]> for AwcKey {
    fn from(words: [u32; AWC_KEY_WORDS]) -> Self {
        Self(words)
    }
}

/// XXTEA's magic constant.
const DELTA: u32 = 0x9E37_79B9;

/// The constant Rockstar added to stock XXTEA's `MX` macro. It is the whole
/// difference between this and the reference cipher, and getting it wrong
/// produces plausible-looking noise rather than an error.
const RS_TWEAK: u32 = 0x7B3A_207F;

fn rounds(n: usize) -> u32 {
    6 + 52 / n as u32
}

/// Decrypt in place. Operates on whole 32-bit words, so a trailing partial
/// word is left untouched — the caller decides whether that is a refusal.
pub fn decrypt(key: AwcKey, data: &mut [u8]) {
    let n = data.len() / 4;
    if n < 2 {
        return;
    }
    let mut v: Vec<u32> = (0..n)
        .map(|i| u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect();

    let key = key.0;
    let mut sum = DELTA.wrapping_mul(rounds(n));
    let mut y = v[0];
    while sum != 0 {
        let e = (sum >> 2) & 3;
        for p in (0..n).rev() {
            let z = v[if p > 0 { p - 1 } else { n - 1 }];
            let mx = ((z >> 5) ^ (y << 2)).wrapping_add((y >> 3) ^ (z << 4))
                ^ (sum ^ y).wrapping_add(key[(p & 3) ^ e as usize] ^ z ^ RS_TWEAK);
            v[p] = v[p].wrapping_sub(mx);
            y = v[p];
        }
        sum = sum.wrapping_sub(DELTA);
    }

    for (i, word) in v.iter().enumerate() {
        data[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
}

/// Encrypt in place, the exact inverse of [`decrypt`].
pub fn encrypt(key: AwcKey, data: &mut [u8]) {
    let n = data.len() / 4;
    if n < 2 {
        return;
    }
    let mut v: Vec<u32> = (0..n)
        .map(|i| u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect();

    let key = key.0;
    let mut sum: u32 = 0;
    let mut z = v[n - 1];
    for _ in 0..rounds(n) {
        sum = sum.wrapping_add(DELTA);
        let e = (sum >> 2) & 3;
        for p in 0..n {
            let y = v[(p + 1) % n];
            let mx = ((z >> 5) ^ (y << 2)).wrapping_add((y >> 3) ^ (z << 4))
                ^ (sum ^ y).wrapping_add(key[(p & 3) ^ e as usize] ^ z ^ RS_TWEAK);
            v[p] = v[p].wrapping_add(mx);
            z = v[p];
        }
    }

    for (i, word) in v.iter().enumerate() {
        data[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_key() -> AwcKey {
        AwcKey([0x1234_5678, 0x9ABC_DEF0, 0x0F1E_2D3C, 0xDEAD_BEEF])
    }

    /// ⚠ The round trip is the only self-check this cipher admits of. The
    /// round count is `6 + 52/n`, so block counts of 2, 13 and 53 take
    /// 32, 10 and 6 rounds and exercise different `e` sequences.
    #[test]
    fn what_encrypt_writes_decrypt_reads_back() {
        for words in [2usize, 4, 13, 53, 512] {
            let original: Vec<u8> = (0..words * 4).map(|i| (i * 37 + 11) as u8).collect();
            let mut buf = original.clone();
            encrypt(a_key(), &mut buf);
            assert_ne!(buf, original, "{words} words: encryption changed nothing");
            decrypt(a_key(), &mut buf);
            assert_eq!(buf, original, "{words} words");
        }
    }

    /// ⚠ The Rockstar tweak is what separates this from stock XXTEA. Without
    /// a test, a stock implementation would round-trip perfectly against
    /// itself and decode every real file to noise.
    #[test]
    fn the_rockstar_tweak_is_load_bearing() {
        assert_eq!(RS_TWEAK, 0x7B3A_207F);
        let plain: Vec<u8> = (0..64).collect();

        let mut with = plain.clone();
        encrypt(a_key(), &mut with);

        // The tweak folds into the key word, so keying with `k ^ tweak`
        // reproduces stock XXTEA's schedule exactly. If the two agreed, the
        // constant would be doing nothing.
        let stock_equivalent = AwcKey(a_key().0.map(|w| w ^ RS_TWEAK));
        let mut without = plain.clone();
        encrypt(stock_equivalent, &mut without);
        assert_ne!(with, without);
    }

    #[test]
    fn a_different_key_gives_different_bytes() {
        let plain: Vec<u8> = (0..128).collect();
        let mut a = plain.clone();
        let mut b = plain.clone();
        encrypt(a_key(), &mut a);
        encrypt(AwcKey([1, 2, 3, 4]), &mut b);
        assert_ne!(a, b);
    }

    /// Fewer than two words is not a cipher block, and XXTEA's round count
    /// divides by the block count.
    #[test]
    fn a_buffer_too_short_to_be_a_block_is_left_alone() {
        for len in [0usize, 1, 3, 4, 7] {
            let original: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let mut buf = original.clone();
            decrypt(a_key(), &mut buf);
            assert_eq!(buf, original, "len {len}");
            encrypt(a_key(), &mut buf);
            assert_eq!(buf, original, "len {len}");
        }
    }

    #[test]
    fn an_all_zero_key_is_not_a_usable_one() {
        assert!(!AwcKey::default().is_usable());
        assert!(AwcKey([0, 0, 0, 1]).is_usable());
    }
}
