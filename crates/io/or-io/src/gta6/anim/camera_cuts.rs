//! The table a `cin@` clip dictionary's root 0x60 points at: a properties object whose every
//! attribute is a nested object of an index and a window of time.

use std::cell::Cell;

use super::metadata::{self, Region};
use super::{AnimError, Attribute, AttributeValue, Section};
use crate::decl::{Aligned, Image};
use crate::gta6::hash::name_hash32;

/// Where the root states the table's pointer.
const TABLE: usize = 0x60;

/// One entry of the table; that it is a camera cut is inferred from where it lies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraCut {
    /// The attribute's name hash, which names nothing known.
    pub name: u32,
    pub index: i32,
    pub min_time: f32,
    pub max_time: f32,
}

/// The table's entries by index, from a clip dictionary's virtual section; none for a null word.
pub fn read_camera_cuts(section: &[u8]) -> Result<Vec<CameraCut>, AnimError> {
    let aligned = Aligned::from_bytes(section);
    let s = Section {
        image: Image::new(aligned.bytes(), section.len()),
        left: Cell::new(section.len().saturating_mul(2)),
    };
    let Some(at) = s.pointer("camera cut table", TABLE)? else {
        return Ok(Vec::new());
    };
    let region = Region {
        lo: 0,
        hi: section.len(),
    };
    let mut cuts = Vec::new();
    for record in metadata::properties(&s, region, at)? {
        for a in &record.attributes {
            cuts.push(cut(at, a)?);
        }
    }
    cuts.sort_by_key(|c| c.index);
    Ok(cuts)
}

/// The value of `fields` named `name`, by its lower-cased hash.
fn value<'a>(fields: &'a [Attribute], name: &str) -> Option<&'a AttributeValue> {
    (fields.iter())
        .find(|f| f.name == name_hash32(name.as_bytes()))
        .map(|f| &f.value)
}

fn cut(at: usize, a: &Attribute) -> Result<CameraCut, AnimError> {
    let refused = |why: &str| AnimError::Metadata {
        at,
        why: format!("camera cut {:#010X} {why}", a.name),
    };
    let AttributeValue::Object(fields) = &a.value else {
        return Err(refused("is not a nested object"));
    };
    let read = (
        value(fields, "mintime"),
        value(fields, "index"),
        value(fields, "maxtime"),
    );
    match read {
        (
            Some(AttributeValue::Float(min_time)),
            Some(AttributeValue::Int(index)),
            Some(AttributeValue::Float(max_time)),
        ) if fields.len() == 3 => Ok(CameraCut {
            name: a.name,
            index: *index,
            min_time: *min_time,
            max_time: *max_time,
        }),
        _ => Err(refused(
            "does not hold exactly a float MinTime, an int Index and a float MaxTime",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIRTUAL: u64 = 0x5000_0000;
    const LEN: usize = 0x240;
    const OBJECT: usize = 0x80;
    const RECORD: usize = 0xB0;
    const LONGS: usize = 0x100;
    /// Each entry's nested object, then its three 12-byte attributes 0x40 after it.
    const ENTRIES: [usize; 2] = [0x140, 0x1B0];
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
        fn attribute(&mut self, at: usize, name: u32, kind: u16) {
            self.u32(at, name);
            self.u16(at + 4, kind);
            self.u16(at + 6, 0xFFFF);
        }
    }

    /// A table of two entries, stored with index 1 before index 0.
    fn body() -> Body {
        let mut b = Body(vec![0; LEN]);
        b.u64(TABLE, VIRTUAL + OBJECT as u64);
        b.relative(OBJECT + 0x18, 1, RECORD);
        b.relative(RECORD + 0x10, 0, 0);
        b.relative(RECORD + 0x20, 2, LONGS);
        b.u64(RECORD + 0x30, VIRTUAL + OBJECT as u64);
        b.u16(RECORD + 0x38, 1);
        b.u16(RECORD + 0x3A, 0xFFFF);
        b.u32(RECORD + 0x40, 0x2FCE_9897);
        let values = [(1, 2.0f32, 3.5f32), (0, 0.0, 2.0)];
        for (i, (at, (index, min, max))) in ENTRIES.into_iter().zip(values).enumerate() {
            let long = LONGS + 0x20 * i;
            b.attribute(long, 0xA1 + i as u32, 13);
            b.u64(long + 0x18, (at as i64 - (long as i64 + 0x18)) as u64);
            b.u64(at, VTABLE);
            b.relative(at + 0x10, 3, at + 0x40);
            b.relative(at + 0x20, 0, 0);
            b.u64(at + 0x30, 0x5123_4560);
            b.u16(at + 0x38, 3);
            b.u16(at + 0x3A, 0xFFFF);
            let shorts = at + 0x40;
            b.attribute(shorts, name_hash32(b"mintime"), 1);
            b.put(shorts + 8, &min.to_le_bytes());
            b.attribute(shorts + 12, name_hash32(b"index"), 2);
            b.u32(shorts + 20, index);
            b.attribute(shorts + 24, name_hash32(b"maxtime"), 1);
            b.put(shorts + 32, &max.to_le_bytes());
        }
        b
    }

    #[test]
    fn a_table_s_entries_read_by_index_with_their_window() {
        let cuts = read_camera_cuts(&body().0).expect("reads");
        let entry = |name, index, min_time, max_time| CameraCut {
            name,
            index,
            min_time,
            max_time,
        };
        assert_eq!(cuts, [entry(0xA2, 0, 0.0, 2.0), entry(0xA1, 1, 2.0, 3.5)]);
    }

    #[test]
    fn a_null_root_word_holds_no_table() {
        let mut b = body();
        b.u64(TABLE, 0);
        assert!(read_camera_cuts(&b.0).expect("reads").is_empty());
    }

    #[test]
    fn an_entry_without_its_index_or_a_table_past_the_section_is_refused() {
        let mut unindexed = body();
        unindexed.attribute(ENTRIES[0] + 0x40 + 12, 0x1234_5678, 2);
        assert!(matches!(
            read_camera_cuts(&unindexed.0),
            Err(AnimError::Metadata { .. })
        ));
        let mut past = body();
        past.relative(RECORD + 0x20, 2, LEN - 0x20);
        assert!(read_camera_cuts(&past.0).is_err());
    }
}
