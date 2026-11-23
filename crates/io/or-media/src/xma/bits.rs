//! Bit-level reading and the bit reservoir a frame that spans packets is assembled in.
//!
//! Origin: OpenRage's own. The operations are named for FFmpeg's `get_bits`, `show_bits`, `get_sbits`,
//! `get_bitsz`, `get_bits_left` and `ff_copy_bits` (tag `n7.0`), and no code of FFmpeg's is in it.

/// A most-significant-bit-first reader over `len` bits of `data`, which reads zeros past its end.
pub(super) struct BitReader<'a> {
    data: &'a [u8],
    len: usize,
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8], len: usize) -> Self {
        Self { data, len, pos: 0 }
    }

    /// Bits read so far.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Bits the reader may read.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Bits left before `len`, negative once a read has gone past it.
    pub fn left(&self) -> isize {
        self.len as isize - self.pos as isize
    }

    /// The next `n` bits (at most 32) without consuming them.
    pub fn peek(&self, n: u32) -> u32 {
        debug_assert!(n <= 32);
        if n == 0 {
            return 0;
        }
        let first = self.pos / 8;
        let mut window = 0u64;
        for i in 0..8 {
            let byte = if first + i < self.data.len() && (first + i) * 8 < self.len {
                self.data[first + i]
            } else {
                0
            };
            window = (window << 8) | u64::from(byte);
        }
        let shifted = window << (self.pos % 8);
        (shifted >> (64 - n)) as u32
    }

    pub fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    pub fn get(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.pos += n as usize;
        v
    }

    pub fn bit(&mut self) -> bool {
        self.get(1) == 1
    }

    /// `n` bits (at most 31) as a two's-complement number.
    pub fn get_signed(&mut self, n: u32) -> i32 {
        let v = self.get(n) as i32;
        (v << (32 - n)) >> (32 - n)
    }

    /// The next `n` bits, or zero for `n` of zero.
    pub fn get_z(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.get(n)
        }
    }
}

/// Bits saved from one or more packets until they hold a whole frame, byte-aligned at bit 0.
#[derive(Default)]
pub(super) struct Reservoir {
    bytes: Vec<u8>,
    bits: usize,
}

impl Reservoir {
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.bits = 0;
    }

    pub fn bits(&self) -> usize {
        self.bits
    }

    pub fn reader(&self) -> BitReader<'_> {
        BitReader::new(&self.bytes, self.bits)
    }

    /// Appends `n` bits of `src` starting at bit `from`.
    pub fn append(&mut self, src: &[u8], from: usize, n: usize) {
        let mut src = BitReader::new(src, from + n);
        src.skip(from);
        let mut left = n;
        while left > 0 {
            let take = left.min(32) as u32;
            self.push(src.get(take), take);
            left -= take as usize;
        }
    }

    fn push(&mut self, value: u32, n: u32) {
        for i in (0..n).rev() {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                let last = self.bytes.len() - 1;
                self.bytes[last] |= 0x80 >> (self.bits % 8);
            }
            self.bits += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_reads_msb_first_across_bytes_and_zeros_past_the_end() {
        let data = [0b1010_0101, 0b1111_0000];
        let mut r = BitReader::new(&data, 16);
        assert_eq!(r.get(3), 0b101);
        assert_eq!(r.get(7), 0b001_0111);
        assert_eq!(r.pos(), 10);
        assert_eq!(r.left(), 6);
        assert_eq!(r.get(10), 0b11_0000_0000);
        assert_eq!(r.left(), -4);
        let mut r = BitReader::new(&data, 16);
        r.skip(5);
        assert_eq!(r.get_signed(6), 0b10_1111 - 64);
    }

    #[test]
    fn a_signed_read_extends_its_sign() {
        let data = [0b1111_1000];
        assert_eq!(BitReader::new(&data, 8).get_signed(6), -2);
        assert_eq!(BitReader::new(&data, 8).get_signed(1), -1);
        let data = [0b0111_1100];
        assert_eq!(BitReader::new(&data, 8).get_signed(6), 31);
    }

    #[test]
    fn a_reservoir_holds_the_bits_appended_from_any_bit_offset_in_order() {
        let a = [0b1100_1010, 0b0101_1111];
        let b = [0b0011_0011];
        let mut pool = Reservoir::default();
        pool.append(&a, 3, 9); // 0 1010 0101
        pool.append(&b, 2, 5); // 1 1001
        assert_eq!(pool.bits(), 14);
        let mut r = pool.reader();
        assert_eq!(r.get(9), 0b0_1010_0101);
        assert_eq!(r.get(5), 0b1_1001);
        assert_eq!(r.get(2), 0);
        pool.clear();
        assert_eq!(pool.bits(), 0);
    }
}
