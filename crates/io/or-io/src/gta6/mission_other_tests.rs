use super::*;
use crate::meta::binary::pso::table_form::tests::{body, member, pointer, Class};
use crate::meta::instance::value::Value;

const PAIR: u32 = 0x0000_0105;
const WIDE_PAIR: u32 = 0x0000_0106;
const TABLE: u32 = 0x0000_0107;
const SHAPE: u32 = 0x1111_00C0;
const LIST: u32 = 0x1111_00A0;
const HASHES: [u32; 2] = [0x3333_0001, 0x3333_0002];

/// A pair of a key at +0 of `code` and a structure of [`SHAPE`] at +0x10.
fn pair(name: u32, code: u8) -> Class {
    Class {
        name,
        size: 0x40,
        members: vec![
            member(KEY, 0, code, 0, 0),
            member(VALUE, 0x10, 0x05, 0, SHAPE),
        ],
    }
}

/// A [`SHAPE`] value: a vector, an array of hashes in block 2 (null where `list` is false), a
/// u32 and a bool.
fn value(x: f32, list: bool, number: u32) -> [u8; 0x30] {
    let mut v = [0u8; 0x30];
    for (k, lane) in [x, 2.0, 3.0, x].into_iter().enumerate() {
        v[4 * k..4 * k + 4].copy_from_slice(&lane.to_le_bytes());
    }
    if list {
        v[0x10..0x18].copy_from_slice(&pointer(2, 0).to_le_bytes());
        v[0x18..0x1A].copy_from_slice(&(HASHES.len() as u16).to_le_bytes());
        v[0x1A..0x1C].copy_from_slice(&(HASHES.len() as u16).to_le_bytes());
    }
    v[0x20..0x24].copy_from_slice(&number.to_le_bytes());
    v[0x24] = 1;
    v
}

/// A body of two narrow-keyed elements, the hashes the first lists, one wide-keyed element and a
/// state-table-like block whose value is a map.
fn sample() -> Vec<u8> {
    let shape = Class {
        name: SHAPE,
        size: 0x30,
        members: vec![
            member(0x2222_0100, 0x00, 0x33, 0, 0),
            member(0x2222_0101, 0x10, 0x52, 3, 0),
            member(0x2222_0102, 0x20, 0x15, 0, 0),
            member(ANONYMOUS, 0, 0x4A, 0, 0),
            member(0x2222_0103, 0x24, 0x01, 0, 0),
        ],
    };
    let table = Class {
        name: TABLE,
        size: 0x20,
        members: vec![member(KEY, 0, 0x4D, 0, 0), member(VALUE, 8, 0x71, 0, 0)],
    };
    let mut narrow = vec![0u8; 0x80];
    for (n, x) in [(0usize, 1.0f32), (1, 5.0)] {
        narrow[0x40 * n..0x40 * n + 4].copy_from_slice(&(0xAAAA_0000 + n as u32).to_le_bytes());
        let v = value(x, n == 0, 7 + n as u32);
        narrow[0x40 * n + 0x10..0x40 * n + 0x40].copy_from_slice(&v);
    }
    let list: Vec<u8> = HASHES.iter().flat_map(|h| h.to_le_bytes()).collect();
    let mut wide = vec![0u8; 0x40];
    wide[..8].copy_from_slice(&0x0123_4567_89AB_CDEFu64.to_le_bytes());
    wide[0x10..].copy_from_slice(&value(0.0, false, 9));
    body(
        &[pair(PAIR, 0x4D), pair(WIDE_PAIR, WIDE_HASH), shape, table],
        &[
            (PAIR, narrow),
            (LIST, list),
            (WIDE_PAIR, wide),
            (TABLE, vec![0; 0x20]),
        ],
        1,
    )
}

fn record(block: usize, count: u16) -> StateRecord {
    StateRecord {
        key: 1,
        word_08: 1,
        run: Some((block, 0)),
        count,
    }
}

fn read(bytes: &[u8], record: &StateRecord) -> Result<Vec<OtherElement>, OtherShapeError> {
    let body = Body::open(bytes).expect("a body");
    Declared::open(bytes)?.elements(&body, record)
}

#[test]
fn an_element_reads_as_its_key_and_its_value_s_declared_members() {
    let bytes = sample();
    let body = Body::open(&bytes).expect("a body");
    let elements = read(&bytes, &record(0, 2)).expect("the elements read");
    assert_eq!(elements.len(), 2);
    let (first, second) = (&elements[0], &elements[1]);
    assert_eq!(
        (first.at, first.key, first.wide),
        (body.blocks[0].offset, 0xAAAA_0000, false)
    );
    assert_eq!(second.at, body.blocks[0].offset + 0x40);
    // The anonymous row is the array's element, not a member of its own.
    let fields: Vec<&Value> = first.value.fields.iter().map(|(_, v)| v).collect();
    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0], &Value::Vec3([1.0, 2.0, 3.0]));
    let Value::Array(items) = fields[1] else {
        panic!("an array, not {:?}", fields[1]);
    };
    let hashes: Vec<u32> = (items.iter())
        .map(|item| match item {
            Value::Hash(h) => h.hash,
            other => panic!("a hash, not {other:?}"),
        })
        .collect();
    assert_eq!(hashes, HASHES);
    assert_eq!(
        (fields[2], fields[3]),
        (&Value::UInt(7), &Value::Bool(true))
    );
    assert_eq!(second.value.fields[0].1, Value::Vec3([5.0, 2.0, 3.0]));
    assert_eq!(second.value.fields[1].1, Value::Array(Vec::new()));
    assert_eq!(second.value.fields[2].1, Value::UInt(8));
}

#[test]
fn a_wide_key_reads_as_its_eight_bytes() {
    let elements = read(&sample(), &record(2, 1)).expect("the element reads");
    assert_eq!(elements.len(), 1);
    assert_eq!(
        (elements[0].key, elements[0].wide),
        (0x0123_4567_89AB_CDEF, true)
    );
    assert_eq!(elements[0].value.fields[2].1, Value::UInt(9));
}

#[test]
fn a_run_that_is_no_pair_or_leaves_its_room_is_refused() {
    let bytes = sample();
    // A state table's own record has a map where the value structure is.
    let table = read(&bytes, &record(3, 1));
    assert!(
        matches!(table, Err(OtherShapeError::NotAPair { block: 4, .. })),
        "{table:?}"
    );
    // A third element lies past the block's two.
    let past = read(&bytes, &record(0, 3));
    assert!(
        matches!(past, Err(OtherShapeError::Element { block: 1, .. })),
        "{past:?}"
    );
    // The first element's array counts three hashes where its block holds two.
    let mut grown = bytes.clone();
    let count = Body::open(&bytes).expect("a body").blocks[0].offset + 0x28;
    grown[count] = 3;
    grown[count + 2] = 3;
    let below = read(&grown, &record(0, 1));
    assert!(
        matches!(below, Err(OtherShapeError::Element { block: 1, .. })),
        "{below:?}"
    );
    assert!(matches!(
        Declared::open(&[0u8; 0x40]),
        Err(OtherShapeError::Model(_))
    ));
    // A state that owns nothing has no element.
    let none = StateRecord {
        run: None,
        ..record(0, 0)
    };
    assert!(read(&bytes, &none).expect("nothing to read").is_empty());
}
