//! Declarations more than one GTA V format uses.

use super::texture_dict::gen8::GrcTexture;
use crate::decl::*;
use crate::desc::Value;
use crate::hash::string_hash_str;

/// `Vec3V`: three floats in a sixteen-byte register. ⚠ The fourth word is
/// carried whatever it holds; the engine packs integers into it.
#[record(size = 0x10)]
pub struct Vec3V {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// `Vector3`: three floats in a sixteen-byte slot, not a register type. ⚠
/// The fourth word is carried whatever it holds.
#[record(size = 0x10)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// `Vec4V`: four floats.
#[record(size = 0x10)]
pub struct Vec4V {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// `QuatV`: a quaternion in a sixteen-byte register, `w` its scalar part.
#[record(size = 0x10)]
pub struct QuatV {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// `Mat33V`: a rotation as three columns, each a sixteen-byte register.
#[record(size = 0x30)]
pub struct Mat33V {
    pub col0: Vec4V,
    pub col1: Vec4V,
    pub col2: Vec4V,
}

/// `Mat34V`: three axes and a translation, each column a sixteen-byte register.
#[record(size = 0x40)]
pub struct Mat34V {
    pub col0: Vec4V,
    pub col1: Vec4V,
    pub col2: Vec4V,
    pub col3: Vec4V,
}

/// `spdSphere`: the centre, and the radius in the fourth lane.
#[record(size = 0x10)]
pub struct SpdSphere {
    pub center_and_radius: Vec4V,
}

/// `spdAABB`: two corners. ⚠ Each fourth lane is carried whatever it holds.
#[record(size = 0x20)]
pub struct SpdAABB {
    pub min: Vec4V,
    pub max: Vec4V,
}

/// `atBitSet`: a run of 32-bit words, and how many of their bits mean anything.
#[record(size = 0x10)]
pub struct AtBitSet {
    #[len(field = "size")]
    pub bits: Vec<u32>,
    #[note("words, written from the run's length")]
    size: u16,
    #[pad_after(4)]
    pub bit_size: u16,
}

/// `atString`: a pointer at the characters, their count and the bytes allocated.
#[record(size = 0x10, recompute = AtString::measure)]
pub struct AtString {
    pub data: String,
    #[note("characters, excluding the terminator; written at save")]
    length: u16,
    #[note("bytes allocated, counting the terminator; kept while the string fits it")]
    #[pad_after(4)]
    allocated: u16,
}

impl AtString {
    /// A string whose length and allocation a save states.
    pub fn new(s: &str) -> Self {
        Self {
            data: s.into(),
            __carry: Carry::default(),
        }
    }

    /// ⚠ An allocation is the game's, like a counted array's capacity: kept
    /// while the string still fits it, and grown to the string when it does not.
    fn measure(
        &self,
        raw: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        let length = u16::try_from(self.data.len())
            .ok()
            .filter(|l| *l < u16::MAX)
            .ok_or_else(|| OwnedError::Invalid {
                what: "AtString.data".into(),
                why: format!("{} bytes, and its length is sixteen bits", self.data.len()),
            })?;
        raw.length = length;
        if length > 0 && raw.allocated <= length {
            raw.allocated = length + 1;
        }
        Ok(())
    }
}

/// `pgBase`: the vtable, and the defragmenter's list head, a runtime pointer.
#[record(size = 0x10)]
pub struct PgBase {
    vft: u64,
    first_node: Ptr<Opaque>,
}

/// `datBase`: the vtable word a polymorphic resource object starts with.
#[record(size = 0x08)]
pub struct DatBase {
    #[note("a runtime address, carried")]
    vft: u64,
}

/// `pgBaseRefCounted`: [`PgBase`] and the count of references keeping it alive.
#[record(size = 0x18)]
pub struct PgBaseRefCounted {
    base: PgBase,
    #[pad_after(4)]
    ref_count: u32,
}

/// `pgDictionary<T>`: entries keyed by a 32-bit name hash, in two runs of one length.
///
/// ⚠ `codes` is ascending and a lookup binary-searches it, so an edit that
/// appends out of order reads the wrong entry rather than failing.
#[record(size = 0x40, recompute = Self::order)]
pub struct PgDictionary<T> {
    pub base: PgBase,
    parent: Ptr<Opaque>,
    #[pad_after(4)]
    #[init(1)]
    ref_count: i32,
    pub codes: Vec<u32>,
    #[keyed_by(codes)]
    pub entries: Vec<Option<Box<T>>>,
}

impl<T: Owned> PgDictionary<T>
where
    Raw<T>: Record,
{
    /// Saves the codes ascending, each entry moved with its code: the order a lookup assumes.
    fn order(
        &self,
        _: &mut Raw<Self>,
        kids: &mut [(u32, Value)],
        at: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        if Self::codes_are_sorted(&self.codes) || self.codes.len() != self.entries.len() {
            return Ok(());
        }
        let codes = at + std::mem::offset_of!(Raw<Self>, codes);
        let entries = at + std::mem::offset_of!(Raw<Self>, entries);
        for (o, kid) in kids.iter_mut() {
            let Value::Ptr {
                target: Some(run), ..
            } = kid
            else {
                continue;
            };
            match run.as_mut() {
                Value::Bytes(b) if *o as usize == codes => {
                    let mut sorted = self.codes.clone();
                    sorted.sort_unstable();
                    *b = sorted.iter().flat_map(|c| c.to_le_bytes()).collect();
                }
                Value::Array { elements, .. } if *o as usize == entries => {
                    let mut keyed: Vec<(u32, Value)> = self
                        .codes
                        .iter()
                        .copied()
                        .zip(std::mem::take(elements))
                        .collect();
                    keyed.sort_by_key(|(code, _)| *code);
                    *elements = keyed.into_iter().map(|(_, entry)| entry).collect();
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Whether the codes are in the ascending order a lookup assumes.
    pub fn codes_are_sorted(codes: &[u32]) -> bool {
        codes.windows(2).all(|w| w[0] <= w[1])
    }
}

/// A Legacy `.ytd`'s textures by name: each code is the [`string_hash_str`] of its texture's name.
impl PgDictionary<GrcTexture> {
    /// The texture named `name`, whatever its case.
    pub fn get(&self, name: &str) -> Option<&GrcTexture> {
        let i = self.position(name)?;
        self.entries.get(i)?.as_deref()
    }

    /// The texture named `name`, whatever its case, to edit.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut GrcTexture> {
        let i = self.position(name)?;
        self.entries.get_mut(i)?.as_deref_mut()
    }

    /// `texture` under the code of its name, keeping the codes ascending; the texture it replaces, if one had that name.
    pub fn insert(&mut self, texture: GrcTexture) -> Option<GrcTexture> {
        if let Some(i) = self.position(&texture.name) {
            if let Some(slot) = self.entries.get_mut(i) {
                return slot.replace(Box::new(texture)).map(|old| *old);
            }
        }
        let code = string_hash_str(&texture.name);
        let at = self.codes.partition_point(|c| *c < code);
        self.codes.insert(at, code);
        self.entries
            .insert(at.min(self.entries.len()), Some(Box::new(texture)));
        None
    }

    fn position(&self, name: &str) -> Option<usize> {
        let code = string_hash_str(name);
        self.codes.iter().position(|c| *c == code)
    }
}

/// A range of a 32-bit word's bits, as the engine's bit fields name them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bits {
    pub low: u32,
    pub width: u32,
}

impl Bits {
    pub const fn mask(self) -> u32 {
        (((1u64 << self.width) - 1) as u32) << self.low
    }

    pub fn get(self, word: u32) -> u32 {
        (word & self.mask()) >> self.low
    }

    /// `word` with the range set to `value`, refused as `what` where the range cannot hold it.
    pub fn put(self, word: u32, value: usize, what: &str) -> Result<u32, OwnedError> {
        match u32::try_from(value) {
            Ok(v) if u64::from(v) >> self.width == 0 => Ok((word & !self.mask()) | (v << self.low)),
            _ => Err(OwnedError::Invalid {
                what: what.into(),
                why: format!("{value}, and it is {} bits wide", self.width),
            }),
        }
    }

    /// The range of the private word at `at`.
    pub fn read(self, carry: &Carry, at: usize) -> usize {
        self.get(word(carry, at)) as usize
    }

    /// Sets the range of the private word at `at`, refused as `what` where it cannot hold `value`.
    pub fn write(
        self,
        carry: &mut Carry,
        at: usize,
        value: usize,
        what: &str,
    ) -> Result<(), OwnedError> {
        let w = self.put(word(carry, at), value, what)?;
        carry.hide(at, w);
        Ok(())
    }
}

/// The private word at `at`: zero in a value built from scratch.
fn word(carry: &Carry, at: usize) -> u32 {
    carry.hidden::<u32>(at).copied().unwrap_or(0)
}

/// The private word at `at` with the bits under `owned` read as zero.
pub(crate) fn flags(carry: &Carry, at: usize, owned: u32) -> u32 {
    word(carry, at) & !owned
}

/// Sets the private word at `at` to `flags`, keeping the bits under `owned`.
pub(crate) fn set_flags(carry: &mut Carry, at: usize, owned: u32, flags: u32) {
    let w = (word(carry, at) & owned) | (flags & !owned);
    carry.hide(at, w);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn each_private_field_is_described_where_offset_of_puts_it() {
        let pack = describe(
            "synthetic",
            "gen8",
            &[
                root::<Raw<PgBaseRefCounted>>,
                root::<Raw<DatBase>>,
                root::<Raw<AtBitSet>>,
            ],
        )
        .expect("describes");
        let at = |t: &str, f: &str| {
            pack.type_by_name(t)
                .and_then(|t| t.field_by_name(f))
                .unwrap_or_else(|| panic!("{t}.{f}"))
                .offset
        };
        for (t, f, o) in [
            (
                "PgBaseRefCounted",
                "base",
                offset_of!(Raw<PgBaseRefCounted>, base),
            ),
            (
                "PgBaseRefCounted",
                "ref_count",
                offset_of!(Raw<PgBaseRefCounted>, ref_count),
            ),
            ("DatBase", "vft", offset_of!(Raw<DatBase>, vft)),
            ("AtBitSet", "size", offset_of!(Raw<AtBitSet>, size)),
        ] {
            assert_eq!(at(t, f), o, "{t}.{f}");
        }
    }
}
