//! The pointer fields of a block-mapped clip dictionary or animation set that a growth of its
//! section moves: the root's, each map's buckets, entries and objects, each set's, animation's
//! and clip's, and the owner words of each properties object, tags object and record; and the
//! spans its reader reads as data, which the growth's guard passes over.
//!
//! ⚠ A nested metadata object's word at 0x30 addresses past its section, so it is no pointer a
//! growth moves and is carried as read; it trips no growth's guard in a section under about
//! 150 MB. A word inside a block's payload, the metadata region or a key the reader compares is
//! data: it is neither moved nor refused.

use std::ops::Range;

use super::super::metadata::{relative, Region};
use super::super::{AnimError, AnimResource, RootKind, Section, ENTRY_LEN, GROUP_LEN};
use super::BLOCK_HEAD;

const PROPERTY_LEN: usize = 0x48;
const TAG_LEN: usize = 0x50;
/// The 8-byte word of a key the reader compares, which may read as an address.
const KEY_LEN: usize = 8;

/// The offsets in `s` of every pointer field of `resource`, null or not, and the data spans.
pub(super) fn pointer_fields(
    s: &Section,
    resource: &AnimResource,
) -> Result<(Vec<usize>, Vec<Range<usize>>), AnimError> {
    if resource.root == RootKind::MotionDictionary {
        return Err(AnimError::Unwritten(
            "a motion dictionary's pointer fields are not known, so its section is not grown"
                .into(),
        ));
    }
    let mut f = Fields {
        s,
        at: Vec::new(),
        data: Vec::new(),
    };
    f.word(0x08);
    f.name(0x38)?;
    if resource.root == RootKind::AnimationSet {
        f.set(0)?;
        return Ok((f.at, f.data));
    }
    f.words([0x60, 0x70, 0xA0]);
    // The region's only pointers are the owner words listed below; the rest is records,
    // attributes and their bytes, reached by relative words.
    if let Some(lo) = s.pointer("metadata region", 0xA0)? {
        let len = s.u32("metadata region", 0xA8)? as usize;
        s.bytes("metadata region", lo, len)?;
        f.data.push(lo..lo + len);
    }
    for element in s.array("animation set list", 0x70, 16)? {
        f.word(element);
        f.data.push(element + 8..element + 8 + KEY_LEN);
        if let Some(set) = s.pointer("animation set", element)? {
            f.set(set)?;
        }
    }
    f.map(0x80, 8, false)?;
    for entry in f.map(0x48, 0x10, true)? {
        f.word(entry + 8);
        f.clip(s.target("clip", entry + 8)?)?;
    }
    // The camera-cut table is a properties object with no owner.
    if let Ok(Some(table)) = s.pointer("camera cut table", 0x60) {
        f.metadata(table, PROPERTY_LEN)?;
    }
    Ok((f.at, f.data))
}

struct Fields<'s, 'a> {
    s: &'s Section<'a>,
    at: Vec<usize>,
    data: Vec<Range<usize>>,
}

impl Fields<'_, '_> {
    /// Lists `at` where its word is null or an address in the section: any other word is data,
    /// which the growth's guard still refuses where it aims at or past the edit.
    fn word(&mut self, at: usize) {
        if self.s.pointer("pointer field", at).is_ok() {
            self.at.push(at);
        }
    }

    fn words(&mut self, at: impl IntoIterator<Item = usize>) {
        for at in at {
            self.word(at);
        }
    }

    /// The 16-byte name at `at`, a pointer where byte 15's top bit is clear.
    fn name(&mut self, at: usize) -> Result<(), AnimError> {
        if self.s.bytes("name", at, 16)?[15] & 0x80 == 0 {
            self.word(at);
        }
        Ok(())
    }

    /// The map at `at`: its buckets' pointer, each bucket and each entry's next at `next_at`;
    /// returns its entries.
    fn map(&mut self, at: usize, next_at: usize, wide: bool) -> Result<Vec<usize>, AnimError> {
        let s = self.s;
        let entries: Vec<usize> = (s.map("map", at, next_at, wide)?.into_iter())
            .map(|(_, entry)| entry)
            .collect();
        self.word(at);
        if let Some(buckets) = s.pointer("map", at)? {
            // The map read checked that its buckets lie in the section.
            let slots = s.u32("map", at + 8)? as usize;
            self.words((0..slots).map(|b| buckets + 8 * b));
        }
        self.words(entries.iter().map(|entry| entry + next_at));
        Ok(entries)
    }

    /// The set at `at`: its name, its animation map and each animation's owner, name, block list,
    /// DOF list and every block pointer, and each block's payload as data.
    fn set(&mut self, at: usize) -> Result<(), AnimError> {
        let s = self.s;
        self.name(at + 0x38)?;
        for entry in self.map(at + 0x48, 0x10, false)? {
            self.word(entry + 8);
            let a = s.target("animation", entry + 8)?;
            self.words([a + 0x18, a + 0x30, a + 0x50, a + 0x60]);
            for slot in s.array("block list", a + 0x50, 8)? {
                self.word(slot);
                if let Some(block) = s.pointer("block", slot)? {
                    let len = s.u32("block", block + 4)? as usize;
                    s.bytes("block", block + BLOCK_HEAD, len)?;
                    self.data.push(block + BLOCK_HEAD..block + BLOCK_HEAD + len);
                }
            }
        }
        Ok(())
    }

    /// The clip at `c`: its dictionary, name, tags and properties, and what it plays.
    fn clip(&mut self, c: usize) -> Result<(), AnimError> {
        let s = self.s;
        self.words([c + 0x18, c + 0x40, c + 0x48, c + 0x68]);
        self.name(c + 0x28)?;
        for key in [c + 0x58, c + 0x70] {
            self.data.push(key..key + KEY_LEN);
        }
        match s.u32("clip", c + 0x20)? {
            1 => {}
            2 => {
                for group in s.array("clip group list", c + 0x68, GROUP_LEN)? {
                    self.word(group);
                    for entry in s.array("clip entry list", group, ENTRY_LEN)? {
                        self.words([entry, entry + 0x10]);
                        self.data.push(entry + 8..entry + 8 + KEY_LEN);
                    }
                }
            }
            other => {
                return Err(AnimError::Unwritten(format!(
                    "a clip of type {other} holds fields no measured rule names, so its section is not grown"
                )))
            }
        }
        if let Some(properties) = s.pointer("clip properties", c + 0x48)? {
            self.metadata(properties, PROPERTY_LEN)?;
        }
        if let Some(tags) = s.pointer("clip tags", c + 0x40)? {
            self.metadata(tags, TAG_LEN)?;
        }
        Ok(())
    }

    /// The properties or tags object at `at`: its owner at 0x10 and each record's at 0x30, the
    /// records `stride` bytes each through the relative array at 0x18, which is no pointer.
    fn metadata(&mut self, at: usize, stride: usize) -> Result<(), AnimError> {
        let s = self.s;
        self.word(at + 0x10);
        let whole = Region {
            lo: 0,
            hi: s.image.len(),
        };
        for record in relative(s, whole, at + 0x18, stride)? {
            self.word(record + 0x30);
        }
        Ok(())
    }
}
