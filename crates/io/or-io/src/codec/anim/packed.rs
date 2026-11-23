use crate::error::{IoError, Result};

pub const MASK_TABLE: [u32; 33] = [
    0x0, 0x1, 0x3, 0x7, 0xf, 0x1f, 0x3f, 0x7f, 0xff, 0x1ff, 0x3ff, 0x7ff, 0xfff, 0x1fff, 0x3fff,
    0x7fff, 0xffff, 0x1ffff, 0x3ffff, 0x7ffff, 0xfffff, 0x001fffff, 0x003fffff, 0x007fffff,
    0x00ffffff, 0x01ffffff, 0x03ffffff, 0x07ffffff, 0x0fffffff, 0x1fffffff, 0x3fffffff, 0x7fffffff,
    0xffffffff,
];

#[derive(Debug, Clone, Copy)]
pub struct PackedArray<'a> {
    words: &'a [u32],
}

impl<'a> PackedArray<'a> {
    pub fn new(words: &'a [u32]) -> Self {
        Self { words }
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn get(&self, bit_address: u32, element_bits: u32) -> Result<u32> {
        if element_bits == 0 {
            return Ok(0);
        }
        if element_bits > 32 {
            return Err(IoError::InvalidData {
                context: format!(
                    "packed array element width {element_bits} exceeds 32 bits — \
                     the channel header is corrupt"
                ),
            });
        }

        let block = (bit_address >> 5) as usize;
        let bit = bit_address & 31;

        let low = *self
            .words
            .get(block)
            .ok_or_else(|| self.out_of_range(bit_address))?;
        let spans_words = bit + element_bits > 32;
        let high = if spans_words {
            u64::from(
                *self
                    .words
                    .get(block + 1)
                    .ok_or_else(|| self.out_of_range(bit_address))?,
            )
        } else {
            0
        };

        let window = u64::from(low) | (high << 32);
        Ok(((window >> bit) as u32) & MASK_TABLE[element_bits as usize])
    }

    pub fn get_signed(&self, bit_address: u32, element_bits: u32) -> Result<i32> {
        let raw = self.get(bit_address, element_bits)?;
        if element_bits == 0 || element_bits >= 32 {
            return Ok(raw as i32);
        }
        let sign_bit = 1u32 << (element_bits - 1);
        Ok(if raw & sign_bit != 0 {
            (raw | !MASK_TABLE[element_bits as usize]) as i32
        } else {
            raw as i32
        })
    }

    pub fn words(&self) -> &'a [u32] {
        self.words
    }

    pub fn slice_from(&self, word_offset: usize) -> Result<Self> {
        self.words
            .get(word_offset..)
            .map(Self::new)
            .ok_or_else(|| IoError::InvalidData {
                context: format!(
                    "packed array word offset {word_offset} is past the end of a {}-word section",
                    self.words.len()
                ),
            })
    }

    fn out_of_range(&self, bit_address: u32) -> IoError {
        IoError::InvalidData {
            context: format!(
                "packed array read at bit {bit_address} (word {}) is past the end of a {}-word \
                 section — the block payload is truncated",
                bit_address >> 5,
                self.words.len()
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PackedSequence<'a> {
    array: PackedArray<'a>,
    bit_address: u32,
}

impl<'a> PackedSequence<'a> {
    pub fn new(words: &'a [u32], bit_address: u32) -> Self {
        Self {
            array: PackedArray::new(words),
            bit_address,
        }
    }

    pub fn bit_address(&self) -> u32 {
        self.bit_address
    }

    pub fn next_value(&mut self, divisor: u32) -> Result<i32> {
        if divisor > 32 {
            return Err(IoError::InvalidData {
                context: format!(
                    "Rice divisor {divisor} exceeds 32 bits — the linear channel header is corrupt"
                ),
            });
        }

        let remainder = self.array.get(self.bit_address, divisor)?;

        let quotient_address = self.bit_address + divisor;
        let tail = self.read_window(quotient_address)?;
        let quotient = tail.trailing_zeros();
        if quotient >= 32 {
            return Err(IoError::InvalidData {
                context: format!(
                    "Rice quotient at bit {quotient_address} has no terminating bit within 32 bits \
                     — the packed sequence is corrupt"
                ),
            });
        }

        let negative = match tail.checked_shr(quotient + 1) {
            Some(shifted) => shifted & 1 == 1,
            None => self.array.get(quotient_address + quotient + 1, 1)? == 1,
        };
        let magnitude = ((quotient << divisor) | remainder) as i32;

        self.bit_address += divisor + quotient + 1 + u32::from(magnitude != 0);

        Ok(if negative { -magnitude } else { magnitude })
    }

    fn read_window(&self, bit_address: u32) -> Result<u32> {
        let block = (bit_address >> 5) as usize;
        let bit = bit_address & 31;
        let words = self.array.words();
        let low = u64::from(*words.get(block).ok_or_else(|| IoError::InvalidData {
            context: format!(
                "Rice sequence read at bit {bit_address} (word {block}) is past the end of a \
                 {}-word section — the block payload is truncated",
                words.len()
            ),
        })?);
        let high = u64::from(words.get(block + 1).copied().unwrap_or(0));
        Ok(((low | (high << 32)) >> bit) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_table_matches_bit_counts() {
        assert_eq!(MASK_TABLE[0], 0);
        assert_eq!(MASK_TABLE[1], 1);
        assert_eq!(MASK_TABLE[8], 0xFF);
        assert_eq!(MASK_TABLE[16], 0xFFFF);
        assert_eq!(MASK_TABLE[31], 0x7FFF_FFFF);
        assert_eq!(MASK_TABLE[32], u32::MAX);
        for bits in 1..32u32 {
            assert_eq!(MASK_TABLE[bits as usize], (1u32 << bits) - 1);
        }
    }

    fn pack(values: &[u32], bits: u32) -> Vec<u32> {
        let total_bits = values.len() as u32 * bits;
        let mut words = vec![0u32; (total_bits as usize).div_ceil(32) + 1];
        for (i, &value) in values.iter().enumerate() {
            let address = i as u32 * bits;
            let block = (address >> 5) as usize;
            let bit = address & 31;
            let masked = u64::from(value & MASK_TABLE[bits as usize]);
            let window = masked << bit;
            words[block] |= window as u32;
            if bit + bits > 32 {
                words[block + 1] |= (window >> 32) as u32;
            }
        }
        words
    }

    #[test]
    fn packed_array_reads_back_what_was_packed() {
        for bits in [1u32, 3, 7, 8, 11, 16, 17, 31, 32] {
            let values: Vec<u32> = (0..40u32)
                .map(|i| i.wrapping_mul(2_654_435_761) & MASK_TABLE[bits as usize])
                .collect();
            let words = pack(&values, bits);
            let array = PackedArray::new(&words);
            for (i, &expected) in values.iter().enumerate() {
                let got = array.get(i as u32 * bits, bits).expect("read");
                assert_eq!(got, expected, "bits = {bits}, index = {i}");
            }
        }
    }

    #[test]
    fn packed_array_handles_word_straddling() {
        let values = vec![0x7FF, 0x001, 0x555, 0x2AA];
        let words = pack(&values, 11);
        let array = PackedArray::new(&words);
        assert_eq!(array.get(22, 11).unwrap(), 0x555);
        assert_eq!(array.get(33, 11).unwrap(), 0x2AA);
    }

    #[test]
    fn packed_array_bounds_are_precise() {
        let words = [0xFFFF_FFFFu32];
        let array = PackedArray::new(&words);

        assert_eq!(array.get(0, 32).unwrap(), u32::MAX);
        assert_eq!(array.get(24, 8).unwrap(), 0xFF);

        let err = array
            .get(25, 8)
            .expect_err("must not zero-fill past the end");
        assert!(err.to_string().contains("truncated"), "{err}");
        assert!(array.get(32, 1).is_err(), "past the last word entirely");
    }

    #[test]
    fn packed_array_rejects_absurd_widths() {
        let words = [0u32; 4];
        let err = PackedArray::new(&words).get(0, 33).expect_err("too wide");
        assert!(err.to_string().contains("exceeds 32 bits"), "{err}");
        assert_eq!(PackedArray::new(&words).get(0, 0).unwrap(), 0);
    }

    #[test]
    fn signed_reads_sign_extend() {
        let words = pack(&[0b111, 0b011, 0b100, 0b000], 3);
        let array = PackedArray::new(&words);
        assert_eq!(array.get_signed(0, 3).unwrap(), -1);
        assert_eq!(array.get_signed(3, 3).unwrap(), 3);
        assert_eq!(array.get_signed(6, 3).unwrap(), -4);
        assert_eq!(array.get_signed(9, 3).unwrap(), 0);
    }

    struct RiceWriter {
        bits: Vec<bool>,
    }

    impl RiceWriter {
        fn new() -> Self {
            Self { bits: Vec::new() }
        }

        fn push(&mut self, value: i32, divisor: u32) {
            let magnitude = value.unsigned_abs();
            let remainder = magnitude & MASK_TABLE[divisor as usize];
            let quotient = magnitude >> divisor;

            for i in 0..divisor {
                self.bits.push((remainder >> i) & 1 == 1);
            }
            for _ in 0..quotient {
                self.bits.push(false);
            }
            self.bits.push(true);
            if magnitude != 0 {
                self.bits.push(value < 0);
            }
        }

        fn finish(&self) -> Vec<u32> {
            let mut words = vec![0u32; self.bits.len().div_ceil(32) + 2];
            for (i, &bit) in self.bits.iter().enumerate() {
                if bit {
                    words[i / 32] |= 1 << (i % 32);
                }
            }
            words
        }
    }

    fn encodable_values(divisor: u32) -> Vec<i32> {
        let max_quotient = 8u32;
        let mut values = vec![0i32];
        for quotient in 0..max_quotient {
            for remainder in [0u32, 1, MASK_TABLE[divisor as usize]] {
                let magnitude =
                    ((quotient << divisor) | (remainder & MASK_TABLE[divisor as usize])) as i32;
                values.push(magnitude);
                if magnitude != 0 {
                    values.push(-magnitude);
                }
            }
        }
        values.push(0);
        values
    }

    #[test]
    fn rice_sequence_roundtrips() {
        for divisor in [0u32, 1, 2, 3, 5, 8] {
            let values = encodable_values(divisor);
            let mut writer = RiceWriter::new();
            for &v in &values {
                writer.push(v, divisor);
            }
            let words = writer.finish();

            let mut sequence = PackedSequence::new(&words, 0);
            for (i, &expected) in values.iter().enumerate() {
                let got = sequence.next_value(divisor).expect("decode");
                assert_eq!(got, expected, "divisor = {divisor}, index = {i}");
            }
        }
    }

    #[test]
    fn rice_rejects_an_unrepresentable_quotient() {
        let mut writer = RiceWriter::new();
        writer.push(40, 0);
        let words = writer.finish();

        let mut sequence = PackedSequence::new(&words, 0);
        let err = sequence.next_value(0).expect_err("quotient is too large");
        assert!(err.to_string().contains("no terminating bit"), "{err}");
    }

    #[test]
    fn zero_consumes_no_sign_bit() {
        let mut writer = RiceWriter::new();
        writer.push(0, 3);
        writer.push(7, 3);
        let words = writer.finish();

        let mut sequence = PackedSequence::new(&words, 0);
        assert_eq!(sequence.next_value(3).unwrap(), 0);
        assert_eq!(sequence.bit_address(), 4);
        assert_eq!(sequence.next_value(3).unwrap(), 7);
    }

    #[test]
    fn rice_rejects_a_missing_terminator() {
        let words = [0u32; 4];
        let mut sequence = PackedSequence::new(&words, 0);
        let err = sequence.next_value(2).expect_err("must not spin or wrap");
        assert!(err.to_string().contains("no terminating bit"), "{err}");
    }

    #[test]
    fn rice_rejects_reads_past_the_end() {
        let words = [0u32; 1];
        let mut sequence = PackedSequence::new(&words, 64);
        assert!(sequence.next_value(2).is_err());
    }

    #[test]
    fn slice_from_bounds_are_checked() {
        let words = [1u32, 2, 3, 4];
        let array = PackedArray::new(&words);
        assert_eq!(array.slice_from(2).unwrap().len(), 2);
        assert_eq!(array.slice_from(4).unwrap().len(), 0);
        assert!(array.slice_from(5).is_err());
    }
}
