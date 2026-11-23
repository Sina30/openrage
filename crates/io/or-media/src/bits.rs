//! A bit reader that takes the lowest bit of each byte first, the order Bink stores its bits in.
//!
//! Origin: written for OpenRage to the contract of FFmpeg's little-endian bit reader (libavcodec's
//! get_bits.h, n7.0): a read past the end yields zero bits, as FFmpeg's zero padding does, and
//! `bits_left` goes negative so the caller decides when that is an error.

pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    /// The next bit to read, counted from the lowest bit of byte 0.
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn bit_pos(&self) -> usize {
        self.pos
    }

    /// Bits not yet read; negative once a read has run past the end.
    pub(crate) fn bits_left(&self) -> i64 {
        (self.data.len() * 8) as i64 - self.pos as i64
    }

    /// The next `n` bits (at most 32) without taking them.
    pub(crate) fn peek(&self, n: u32) -> u32 {
        debug_assert!(n <= 32);
        let byte = self.pos >> 3;
        let word = match self.data.get(byte..byte + 8) {
            Some(eight) => u64::from_le_bytes(eight.try_into().unwrap_or([0; 8])),
            None => {
                let mut padded = [0u8; 8];
                if let Some(rest) = self.data.get(byte..) {
                    padded[..rest.len()].copy_from_slice(rest);
                }
                u64::from_le_bytes(padded)
            }
        };
        let bits = word >> (self.pos & 7);
        (bits & ((1u64 << n) - 1)) as u32
    }

    pub(crate) fn skip(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n);
    }

    /// The next `n` bits (at most 32), the first read being the lowest.
    pub(crate) fn get(&mut self, n: u32) -> u32 {
        let value = self.peek(n);
        self.skip(n as usize);
        value
    }

    pub(crate) fn get1(&mut self) -> u32 {
        self.get(1)
    }

    /// Skips to the next multiple of 32 bits.
    pub(crate) fn align32(&mut self) {
        self.pos = self.pos.saturating_add(31) & !31;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lowest_bit_of_a_byte_comes_first_and_a_read_may_cross_bytes() {
        let mut bits = BitReader::new(&[0b1010_0110, 0b0000_0001, 0xFF]);
        assert_eq!(bits.get1(), 0);
        assert_eq!(bits.get1(), 1);
        assert_eq!(bits.get(3), 0b001, "bits 2..5 of 0b1010_0110, lowest first");
        assert_eq!(
            bits.get(8),
            0b0000_1101,
            "three bits of byte 0 then five of byte 1"
        );
        assert_eq!(bits.bit_pos(), 13);
        assert_eq!(
            bits.peek(11),
            0b111_1111_1000,
            "the rest of byte 1 then byte 2"
        );
    }

    #[test]
    fn a_read_past_the_end_is_zero_and_leaves_bits_left_negative() {
        let mut bits = BitReader::new(&[0xFF]);
        assert_eq!(bits.get(4), 0xF);
        assert_eq!(bits.get(8), 0x0F, "four real bits, then zero padding");
        assert_eq!(bits.bits_left(), -4);
        assert_eq!(bits.get(32), 0);
        assert_eq!(BitReader::new(&[]).peek(5), 0);
    }

    #[test]
    fn align32_moves_to_the_next_word_and_stays_on_one() {
        let data = [0u8; 16];
        let mut bits = BitReader::new(&data);
        bits.align32();
        assert_eq!(bits.bit_pos(), 0);
        bits.skip(1);
        bits.align32();
        assert_eq!(bits.bit_pos(), 32);
        bits.skip(31);
        bits.align32();
        assert_eq!(bits.bit_pos(), 64);
    }

    #[test]
    fn thirty_two_bits_read_whole() {
        let mut bits = BitReader::new(&[0x78, 0x56, 0x34, 0x12, 0xEF, 0xCD, 0xAB, 0x90]);
        assert_eq!(bits.get(32), 0x1234_5678);
        bits.skip(4);
        assert_eq!(
            bits.get(32),
            0x090A_BCDE,
            "an unaligned read ends in zero padding"
        );
    }
}
