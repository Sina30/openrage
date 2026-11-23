//! Prefix-code tables built the way FFmpeg builds them from code lengths, and decoded by search.
//!
//! Origin: FFmpeg, `libavcodec/vlc.c`, tag `n7.0` (commit `083443d67cb159ce469e5d902346b8d0c2cd1c93`).
//! Licence: LGPL-2.1-or-later.
//! Copyright (c) 2000, 2001 Fabrice Bellard
//! Copyright (c) 2002-2004 Michael Niedermayer
//! Copyright (c) 2010 Loren Merritt
//!
//! What is taken is the code assignment of `ff_vlc_init_from_lengths`: codes count up in table
//! order, each aligned to its own length, and a length of zero takes no code. The decode is
//! OpenRage's own: because codes only rise down the table, the entry a bit pattern belongs to is the
//! last whose code does not exceed it.

use super::bits::BitReader;

/// One table of codes: `(code left-aligned in 32 bits, length, symbol)`, rising by code.
pub(super) struct Vlc {
    codes: Vec<(u32, u8, i32)>,
}

impl Vlc {
    /// The table for `lens` and `symbols` taken in step, each symbol plus `offset`; `None` for lengths that do not form a prefix code.
    pub fn from_lengths(
        lens: impl IntoIterator<Item = u8>,
        symbols: impl IntoIterator<Item = u32>,
        offset: i32,
    ) -> Option<Self> {
        let mut codes = Vec::new();
        let mut code = 0u64;
        for (len, sym) in lens.into_iter().zip(symbols) {
            if len == 0 {
                continue;
            }
            if len > 32 || code & ((1u64 << (32 - len)) - 1) != 0 {
                return None;
            }
            codes.push((code as u32, len, sym as i32 + offset));
            code += 1u64 << (32 - len);
            if code > 1 << 32 {
                return None;
            }
        }
        Some(Self { codes })
    }

    /// The symbol the next bits spell, or `None` where they spell no code; the reader moves past a code it finds.
    pub fn decode(&self, r: &mut BitReader<'_>) -> Option<i32> {
        let v = r.peek(32);
        let at = self.codes.partition_point(|e| e.0 <= v).checked_sub(1)?;
        let (code, len, sym) = self.codes[at];
        if v >> (32 - len) != code >> (32 - len) {
            return None;
        }
        r.skip(usize::from(len));
        Some(sym)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_count_up_in_table_order_and_decode_back() {
        // 0, 10, 110, 111 in table order, with a skipped entry between.
        let vlc =
            Vlc::from_lengths([1, 2, 0, 3, 3], [10, 20, 99, 30, 40], -1).expect("a prefix code");
        let data = [0b0101_1011, 0b1000_0000];
        let mut r = BitReader::new(&data, 16);
        let got: Vec<_> = (0..5).map(|_| vlc.decode(&mut r)).collect();
        assert_eq!(got, [Some(9), Some(19), Some(29), Some(39), Some(9)]);
    }

    #[test]
    fn lengths_that_do_not_align_are_refused() {
        assert!(
            Vlc::from_lengths([2, 1], [0, 1], 0).is_none(),
            "a 1-bit code cannot start at 01"
        );
        assert!(
            Vlc::from_lengths([1, 1, 1], [0, 1, 2], 0).is_none(),
            "three 1-bit codes overflow"
        );
    }

    #[test]
    fn bits_that_spell_no_code_are_none_and_stay_unread() {
        let vlc = Vlc::from_lengths([1, 2], [0, 1], 0).expect("an incomplete code");
        let data = [0b1110_0000];
        let mut r = BitReader::new(&data, 8);
        assert_eq!(vlc.decode(&mut r), None);
        assert_eq!(r.pos(), 0);
    }
}
