use std::cell::Cell;

use super::*;
use crate::decl::{Aligned, Image};

const VIRTUAL: u64 = 0x5000_0000;
const LEN: usize = 0x400;
const OWNER: usize = 0x40;
const TAGS: usize = 0x100;
const TAG_AT: usize = 0x140;
const SHORTS: usize = 0x190;
const LONGS: usize = 0x1B0;
const NAMES: usize = 0x300;
const BITS: usize = 0x310;
const DATA: usize = 0x320;
const BLOB: usize = 0x360;
const OBJECT: usize = 0x370;
const INNER: usize = 0x3C0;
const VTABLE: u64 = 0x1_4012_3450;

struct Body(Vec<u8>);

impl Body {
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn u16(&mut self, at: usize, v: u16) {
        self.put(at, &v.to_le_bytes());
    }
    fn u32(&mut self, at: usize, v: u32) {
        self.put(at, &v.to_le_bytes());
    }
    fn u64(&mut self, at: usize, v: u64) {
        self.put(at, &v.to_le_bytes());
    }
    fn ptr(&mut self, at: usize, to: usize) {
        self.u64(at, VIRTUAL + to as u64);
    }
    /// A relative array at `at` of `count` elements at `to`, or the empty one.
    fn relative(&mut self, at: usize, count: u32, to: usize) {
        self.u32(at, count);
        self.u32(at + 4, count);
        let offset = if count == 0 {
            1
        } else {
            to as i64 - (at as i64 + 8)
        };
        self.u64(at + 8, offset as u64);
    }
    /// A 32-byte attribute at `at` of `name` and `kind` referring to `to`.
    fn refers(&mut self, at: usize, name: u32, kind: u16, to: usize) {
        self.attribute(at, name, kind);
        self.u64(at + 0x18, (to as i64 - (at as i64 + 0x18)) as u64);
    }
    fn attribute(&mut self, at: usize, name: u32, kind: u16) {
        self.u32(at, name);
        self.u16(at + 4, kind);
        self.u16(at + 6, 0xFFFF);
    }
}

/// A tags object of one tag holding a NaN float, a type-22 word, three bit names, a bit set of
/// bits 0 and 2, four data bytes, a nested object of one hash, an empty string, a type-16 float,
/// a type-21 value and a quaternion two long.
fn body() -> Body {
    let mut b = Body(vec![0; LEN]);
    b.ptr(TAGS + 0x10, OWNER);
    b.relative(TAGS + 0x18, 1, TAG_AT);
    b.relative(TAG_AT + 0x10, 2, SHORTS);
    b.relative(TAG_AT + 0x20, 8, LONGS);
    b.ptr(TAG_AT + 0x30, TAGS);
    b.u16(TAG_AT + 0x38, TAG);
    b.u16(TAG_AT + 0x3A, 0xFFFF);
    b.u32(TAG_AT + 0x3C, 0x0BAD_5157);
    b.u32(TAG_AT + 0x40, 0x7A61_0001);
    b.put(TAG_AT + 0x48, &(-0.05f32).to_le_bytes());
    b.put(TAG_AT + 0x4C, &1.000_000_1f32.to_le_bytes());
    b.attribute(SHORTS, 0xA1, 1);
    b.u32(SHORTS + 8, 0xFFC0_0000);
    b.attribute(SHORTS + 12, 0xA2, 22);
    b.u32(SHORTS + 20, 0x0000_0D0E);
    let long = |i: usize| LONGS + 0x20 * i;
    b.attribute(long(0), 0xB0, 33);
    b.relative(long(0) + 0x10, 3, NAMES);
    for (i, name) in [0xC1u32, 0xC2, 0xC3].into_iter().enumerate() {
        b.u32(NAMES + 4 * i, name);
    }
    b.refers(long(1), 0xB1, 5, BITS);
    b.u16(BITS, 3);
    b.u16(BITS + 2, 0b101);
    b.refers(long(2), 0xB2, 11, DATA);
    b.u64(DATA, VTABLE);
    b.u16(DATA + 0x10, 11);
    b.u16(DATA + 0x12, 0xFFFF);
    b.u32(DATA + 0x14, 0xB2);
    b.relative(DATA + 0x18, 4, BLOB);
    b.put(BLOB, &[9, 8, 7, 6]);
    b.refers(long(3), 0xB3, 13, OBJECT);
    b.u64(OBJECT, VTABLE);
    b.relative(OBJECT + 0x10, 1, INNER);
    b.relative(OBJECT + 0x20, 0, 0);
    b.u64(OBJECT + 0x30, 0x5123_4560);
    b.u16(OBJECT + 0x38, NESTED);
    b.u32(OBJECT + 0x3C, 0x51C0_0003);
    b.attribute(INNER, 0xD1, 12);
    b.u32(INNER + 8, 0xD1D1_0012);
    b.attribute(long(4), 0xB4, 4);
    b.u64(long(4) + 0x18, 1);
    b.attribute(long(5), 0xB5, 16);
    b.put(long(5) + 0x10, &2.5f32.to_le_bytes());
    b.u64(long(5) + 0x18, 1);
    b.attribute(long(6), 0xB6, 21);
    b.attribute(long(7), 0xB7, 8);
    b.put(long(7) + 0x10, &2.0f32.to_le_bytes());
    b
}

fn read_tags(b: &Body) -> Result<Vec<Record>, AnimError> {
    let aligned = Aligned::from_bytes(&b.0);
    let s = Section {
        image: Image::new(aligned.bytes(), b.0.len()),
        left: Cell::new(b.0.len() * 2),
    };
    tags(&s, Region { lo: TAGS, hi: LEN }, OWNER, TAGS)
}

#[test]
fn a_tag_reads_its_bit_set_data_object_and_the_values_once_refused() {
    let tags = read_tags(&body()).expect("reads");
    let [tag] = tags.as_slice() else {
        panic!("one tag, not {}", tags.len());
    };
    assert_eq!(tag.phases, Some([-0.05, 1.000_000_1]));
    let values: Vec<&AttributeValue> = tag.attributes.iter().map(|a| &a.value).collect();
    assert!(matches!(values[0], AttributeValue::Float(f) if f.is_nan()));
    assert_eq!(values[1], &AttributeValue::OtherWord(0x0D0E));
    let bit_set = AttributeValue::BitSet {
        bits: 3,
        words: vec![0b101],
    };
    assert_eq!(values[3], &bit_set);
    assert_eq!(bit_set.set_bits(), [0, 2]);
    assert_eq!(tag.bit_names(3), Some([0xC1, 0xC2, 0xC3].as_slice()));
    assert_eq!(values[4], &AttributeValue::Data(vec![9, 8, 7, 6]));
    let inner = Attribute {
        name: 0xD1,
        value: AttributeValue::Hash(0xD1D1_0012),
    };
    assert_eq!(values[5], &AttributeValue::Object(vec![inner]));
    assert_eq!(values[6], &AttributeValue::String(String::new()));
    assert_eq!(values[7], &AttributeValue::FloatWide(2.5));
    assert_eq!(values[8], &AttributeValue::Empty);
    assert_eq!(values[9], &AttributeValue::Quaternion([2.0, 0.0, 0.0, 0.0]));
    let kinds: Vec<u16> = values.iter().map(|v| v.type_word()).collect();
    assert_eq!(kinds, [1, 22, 33, 5, 11, 13, 4, 16, 21, 8]);
}

#[test]
fn a_bit_set_data_or_object_that_breaks_its_layout_is_refused() {
    let mut past = body();
    past.u16(BITS + 2, 0b1000);
    let mut unnamed = body();
    unnamed.u32(LONGS + 0x10, 2);
    unnamed.u32(LONGS + 0x14, 2);
    let mut odd = body();
    odd.refers(LONGS + 0x20, 0xB1, 5, BITS + 1);
    let mut misnamed = body();
    misnamed.u32(DATA + 0x14, 0xB3);
    let mut capacity = body();
    capacity.u32(DATA + 0x1C, 5);
    let mut kind = body();
    kind.u16(OBJECT + 0x38, PROPERTY);
    let mut wide = body();
    wide.u64(LONGS + 0xA0 + 0x18, 2);
    let mut nonzero = body();
    nonzero.put(LONGS + 0xC0 + 0x1F, &[1]);
    // An object holding itself is refused at the depth cap rather than read without end.
    let mut endless = body();
    endless.relative(OBJECT + 0x20, 1, 0x2C0);
    endless.refers(0x2C0, 0xE1, 13, OBJECT);
    let broken = [
        past, unnamed, odd, misnamed, capacity, kind, wide, nonzero, endless,
    ];
    for (i, b) in broken.iter().enumerate() {
        assert!(
            matches!(read_tags(b), Err(AnimError::Metadata { .. })),
            "body {i} reads"
        );
    }
}

#[test]
fn a_tag_whose_phases_are_not_finite_is_refused() {
    let mut b = body();
    b.u32(TAG_AT + 0x48, 0x7FC0_0000);
    assert!(matches!(read_tags(&b), Err(AnimError::Metadata { .. })));
}
