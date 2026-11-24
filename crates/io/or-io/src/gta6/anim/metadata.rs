//! The tags and properties of a clip or a motion: records of typed attributes in the root's
//! metadata region, reached through arrays that address themselves.
//!
//! A relative array is `{u32 count, u32 capacity, i64 offset}`, its elements at the offset field's
//! own address plus the offset; an empty one is `{0, 0 or 0x8000_0000, 1}`.

use super::{AnimError, Section};
use crate::gta6::hash::name_hash32;

const PROPERTIES_LEN: usize = 0x28;
const TAGS_LEN: usize = 0x30;
const PROPERTY_LEN: usize = 0x48;
const TAG_LEN: usize = 0x50;
const SHORT_ATTRIBUTE: usize = 12;
const LONG_ATTRIBUTE: usize = 0x20;
/// A record's kind word: 1 a property, 2 a tag, 3 an object nested in an attribute.
const PROPERTY: u16 = 1;
const TAG: u16 = 2;
const NESTED: u16 = 3;
const DATA_LEN: usize = 0x28;
const OBJECT_LEN: usize = 0x40;
/// How deep a nested object may lie, a record's own attributes at depth 1.
const OBJECT_DEPTH: usize = 4;

/// Names of tags, properties and attributes, found where a record's hash is a name's lower-cased hash.
const NAMES: [&str; 21] = [
    "audio",
    "blend",
    "constraint",
    "door",
    "facial",
    "foot",
    "guid",
    "heel",
    "id",
    "interface",
    "jump",
    "lens",
    "max",
    "min",
    "name",
    "parameters",
    "pose",
    "pulse",
    "right",
    "value",
    "weight",
];

/// The name whose lower-cased 32-bit hash `hash` is, among those known.
pub fn known_name(hash: u32) -> Option<&'static str> {
    NAMES
        .into_iter()
        .find(|name| name_hash32(name.as_bytes()) == hash)
}

/// A tag or a property: a named record of typed attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// The name's lower-cased 32-bit hash; see [`known_name`].
    pub name: u32,
    /// Equal for two records of one kind, name and attribute values.
    pub signature: u32,
    /// A tag's start and end phase, finite and nearly all in 0 to 1, the start after the end for a tag that wraps; none for a property.
    pub phases: Option<[f32; 2]>,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// The name's lower-cased 32-bit hash; see [`known_name`].
    pub name: u32,
    pub value: AttributeValue,
}

/// An attribute's value, by its type word.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    /// 1.
    Float(f32),
    /// 2.
    Int(i32),
    /// 3.
    Bool(bool),
    /// 12.
    Hash(u32),
    /// 19: a word whose role is not known.
    Word(u32),
    /// 22: a word whose role is not known, in a nested object.
    OtherWord(u32),
    /// 4.
    String(String),
    /// 5: `bits` bits, bit i of the set bit `i % 16` of word `i / 16`.
    BitSet { bits: u16, words: Vec<u16> },
    /// 6.
    Vector3([f32; 3]),
    /// 7.
    Vector4([f32; 4]),
    /// 8: near unit length in all but a few, which are shown as they are.
    Quaternion([f32; 4]),
    /// 11: an object of bytes, whose format is not known.
    Data(Vec<u8>),
    /// 13: a nested object of attributes, named by the attribute that refers to it.
    Object(Vec<Attribute>),
    /// 14.
    Guid([u8; 16]),
    /// 16: a float kept in the 32-byte form, whose role is not known.
    FloatWide(f32),
    /// 18: a word kept in the 32-byte form, whose role is not known.
    WordWide(u32),
    /// 21: 16 zero bytes, whose role is not known.
    Empty,
    /// 23.
    Floats(Vec<f32>),
    /// 24.
    Words(Vec<u32>),
    /// 33.
    Hashes(Vec<u32>),
}

impl AttributeValue {
    /// The type word the attribute states.
    pub fn type_word(&self) -> u16 {
        match self {
            Self::Float(_) => 1,
            Self::Int(_) => 2,
            Self::Bool(_) => 3,
            Self::Hash(_) => 12,
            Self::Word(_) => 19,
            Self::OtherWord(_) => 22,
            Self::String(_) => 4,
            Self::BitSet { .. } => 5,
            Self::Vector3(_) => 6,
            Self::Vector4(_) => 7,
            Self::Quaternion(_) => 8,
            Self::Data(_) => 11,
            Self::Object(_) => 13,
            Self::Guid(_) => 14,
            Self::FloatWide(_) => 16,
            Self::WordWide(_) => 18,
            Self::Empty => 21,
            Self::Floats(_) => 23,
            Self::Words(_) => 24,
            Self::Hashes(_) => 33,
        }
    }

    /// The indices of a bit set's set bits; none for another value.
    pub fn set_bits(&self) -> Vec<usize> {
        let Self::BitSet { bits, words } = self else {
            return Vec::new();
        };
        (0..usize::from(*bits))
            .filter(|i| words.get(i / 16).is_some_and(|w| (w >> (i % 16)) & 1 == 1))
            .collect()
    }
}

impl Record {
    /// The names of a bit set of `bits` bits: the record's one hash array of that many entries.
    pub fn bit_names(&self, bits: u16) -> Option<&[u32]> {
        let mut arrays = self.attributes.iter().filter_map(|a| match &a.value {
            AttributeValue::Hashes(names) if names.len() == usize::from(bits) => Some(names),
            _ => None,
        });
        match (arrays.next(), arrays.next()) {
            (Some(names), None) => Some(names.as_slice()),
            _ => None,
        }
    }
}

/// The byte range root 0xA0 states, which holds every record and every array they reach.
#[derive(Debug, Clone, Copy)]
pub(super) struct Region {
    pub(super) lo: usize,
    pub(super) hi: usize,
}

impl Region {
    fn holds(&self, at: usize, len: usize) -> Result<(), AnimError> {
        let inside = at >= self.lo && at.checked_add(len).is_some_and(|end| end <= self.hi);
        inside.then_some(()).ok_or_else(|| AnimError::Metadata {
            at,
            why: format!(
                "{len} bytes here leave the metadata region {:#X}..{:#X}",
                self.lo, self.hi
            ),
        })
    }
}

fn bad(at: usize, why: impl Into<String>) -> AnimError {
    AnimError::Metadata {
        at,
        why: why.into(),
    }
}

/// A clip's properties, through the pointer at clip 0x48, and its tags, 0x28 after them through clip 0x40.
pub(super) fn of_clip(
    s: &Section,
    region: Region,
    clip: usize,
) -> Result<(Vec<Record>, Vec<Record>), AnimError> {
    let properties_at = s.target("clip properties", clip + 0x48)?;
    let properties = properties(s, region, properties_at)?;
    let tags = match s.pointer("clip tags", clip + 0x40)? {
        None => Vec::new(),
        Some(at) if at == properties_at + PROPERTIES_LEN => tags(s, region, clip, at)?,
        Some(at) => return Err(bad(at, "the clip's tags are not 0x28 after its properties")),
    };
    Ok((properties, tags))
}

/// The properties object at `at`: 16 zero bytes, a word, and a relative array of properties.
pub(super) fn properties(s: &Section, region: Region, at: usize) -> Result<Vec<Record>, AnimError> {
    region.holds(at, PROPERTIES_LEN)?;
    if s.bytes("properties", at, 16)?.iter().any(|b| *b != 0) {
        return Err(bad(at, "a properties object's first 16 bytes are not zero"));
    }
    relative(s, region, at + 0x18, PROPERTY_LEN)?
        .into_iter()
        .enumerate()
        .map(|(i, record)| read_record(s, region, record, at, PROPERTY, i))
        .collect()
}

/// The tags object at `at` of the clip or motion at `owner`: 16 zero bytes, the owner, a relative
/// array of tags and a word.
pub(super) fn tags(
    s: &Section,
    region: Region,
    owner: usize,
    at: usize,
) -> Result<Vec<Record>, AnimError> {
    region.holds(at, TAGS_LEN)?;
    if s.bytes("tags", at, 16)?.iter().any(|b| *b != 0)
        || s.pointer("tags owner", at + 0x10)? != Some(owner)
    {
        return Err(bad(
            at,
            "a tags object's first 16 bytes are not zero or it names another owner",
        ));
    }
    relative(s, region, at + 0x18, TAG_LEN)?
        .into_iter()
        .enumerate()
        .map(|(i, record)| read_record(s, region, record, at, TAG, i))
        .collect()
}

/// The elements of the relative array at `at`, each `stride` bytes, all inside `region`.
pub(super) fn relative(
    s: &Section,
    region: Region,
    at: usize,
    stride: usize,
) -> Result<Vec<usize>, AnimError> {
    let count = s.u32("relative array", at)? as usize;
    let capacity = s.u32("relative array", at + 4)?;
    let offset = s.u64("relative array", at + 8)? as i64;
    if count == 0 {
        if matches!(capacity, 0 | 0x8000_0000) && offset == 1 {
            return Ok(Vec::new());
        }
        return Err(bad(
            at,
            format!("an empty array is {{0, {capacity:#X}, {offset}}}, not {{0, 0, 1}}"),
        ));
    }
    if ((capacity & 0x7FFF_FFFF) as usize) < count {
        return Err(bad(
            at,
            format!("an array of {count} states a capacity of {capacity:#X}"),
        ));
    }
    let first = (at as i64)
        .checked_add(8)
        .and_then(|field| field.checked_add(offset))
        .and_then(|first| usize::try_from(first).ok())
        .ok_or_else(|| {
            bad(
                at,
                format!("an array's offset {offset} lands before the section"),
            )
        })?;
    let len = count
        .checked_mul(stride)
        .ok_or_else(|| bad(at, format!("an array of {count} cannot fit")))?;
    region.holds(first, len)?;
    s.spend(len)?;
    Ok((0..count).map(|i| first + i * stride).collect())
}

fn read_record(
    s: &Section,
    region: Region,
    at: usize,
    back: usize,
    kind: u16,
    index: usize,
) -> Result<Record, AnimError> {
    let head = s.bytes("record", at, 0x48)?;
    let word = |o: usize| u32::from_le_bytes([head[o], head[o + 1], head[o + 2], head[o + 3]]);
    let stated_kind = u16::from_le_bytes([head[0x38], head[0x39]]);
    let stated_index = u16::from_le_bytes([head[0x3A], head[0x3B]]);
    if head[..16].iter().any(|b| *b != 0)
        || s.pointer("record owner", at + 0x30)? != Some(back)
        || stated_kind != kind
        || word(0x44) != 0
    {
        return Err(bad(at, format!("a record's head is not {{16 zero bytes, its object at 0x30, kind {kind} at 0x38, 0 at 0x44}}")));
    }
    let index_holds = match kind {
        TAG => stated_index == 0xFFFF || usize::from(stated_index) == index,
        _ => stated_index == 0xFFFF,
    };
    if !index_holds {
        return Err(bad(
            at,
            format!("record {index} states index {stated_index:#X}"),
        ));
    }
    let phases = if kind == TAG {
        let phases = [s.f32("tag", at + 0x48)?, s.f32("tag", at + 0x4C)?];
        if !phases.iter().all(|p| p.is_finite()) {
            return Err(bad(
                at,
                format!(
                    "a tag's phases {} and {} are not both finite",
                    phases[0], phases[1]
                ),
            ));
        }
        Some(phases)
    } else {
        None
    };
    let attributes = attributes(s, region, at, 1)?;
    // A tag names the bits of each of its bit sets by a hash array of as many entries.
    for a in attributes.iter().filter(|_| kind == TAG) {
        let AttributeValue::BitSet { bits, .. } = &a.value else {
            continue;
        };
        let named = |h: &Attribute| match &h.value {
            AttributeValue::Hashes(names) => names.len() == usize::from(*bits),
            _ => false,
        };
        if !attributes.iter().any(named) {
            return Err(bad(
                at,
                format!("a tag's bit set of {bits} bits has no hash array of {bits} names"),
            ));
        }
    }
    Ok(Record {
        name: word(0x40),
        signature: word(0x3C),
        phases,
        attributes,
    })
}

/// `{u32 name, u16 type, u16 0xFFFF}`, the head of both attribute sizes.
fn attribute_head(s: &Section, at: usize) -> Result<(u32, u16), AnimError> {
    let name = s.u32("attribute", at)?;
    let kind = s.u16("attribute", at + 4)?;
    if s.u16("attribute", at + 6)? != 0xFFFF {
        return Err(bad(at, "an attribute's word at +6 is not 0xFFFF"));
    }
    Ok((name, kind))
}

/// The 12-byte attributes, then the 32-byte ones, of the record or nested object at `at`, which
/// lies `depth` objects deep.
fn attributes(
    s: &Section,
    region: Region,
    at: usize,
    depth: usize,
) -> Result<Vec<Attribute>, AnimError> {
    let mut attributes = Vec::new();
    for a in relative(s, region, at + 0x10, SHORT_ATTRIBUTE)? {
        attributes.push(short_attribute(s, a)?);
    }
    for a in relative(s, region, at + 0x20, LONG_ATTRIBUTE)? {
        attributes.push(long_attribute(s, region, a, depth)?);
    }
    Ok(attributes)
}

/// A word whose top half is 1 and whose next nibble is 4, as every data and nested object's first
/// is: by its values a table address of the machine that wrote the file.
fn vtable_like(word: u64) -> bool {
    word >> 32 == 1 && (word >> 28) & 0xF == 4
}

fn short_attribute(s: &Section, at: usize) -> Result<Attribute, AnimError> {
    let (name, kind) = attribute_head(s, at)?;
    let word = s.u32("attribute", at + 8)?;
    let value = match kind {
        // A float that is not finite is shown as it is.
        1 => AttributeValue::Float(f32::from_bits(word)),
        2 => AttributeValue::Int(word as i32),
        3 if word <= 1 => AttributeValue::Bool(word == 1),
        12 => AttributeValue::Hash(word),
        19 => AttributeValue::Word(word),
        22 => AttributeValue::OtherWord(word),
        3 => {
            return Err(bad(
                at,
                format!(
                "an attribute of type {kind} holds {word:#010X}, which is not a value of that type"
            ),
            ))
        }
        other => {
            return Err(bad(
                at,
                format!("a 12-byte attribute of type {other}, which no record was seen to hold"),
            ))
        }
    };
    Ok(Attribute { name, value })
}

fn long_attribute(
    s: &Section,
    region: Region,
    at: usize,
    depth: usize,
) -> Result<Attribute, AnimError> {
    let (name, kind) = attribute_head(s, at)?;
    if s.u64("attribute", at + 8)? != 0 {
        return Err(bad(at, "a 32-byte attribute's word at +8 is not zero"));
    }
    // A component that is not finite is shown as it is, as a 12-byte float is: a vector of
    // +infinity and zeros is stored as such.
    let floats = || -> Result<[f32; 4], AnimError> {
        let mut v = [0f32; 4];
        for (i, x) in v.iter_mut().enumerate() {
            *x = s.f32("attribute", at + 0x10 + 4 * i)?;
        }
        Ok(v)
    };
    let reference = || -> Result<usize, AnimError> {
        let offset = s.u64("attribute", at + 0x18)? as i64;
        let target = (at as i64 + 0x18)
            .checked_add(offset)
            .and_then(|t| usize::try_from(t).ok())
            .filter(|t| *t >= region.lo && *t < region.hi);
        match (s.u64("attribute", at + 0x10)?, target) {
            (0, Some(target)) => Ok(target),
            _ => Err(bad(
                at,
                format!("an attribute of type {kind} refers outside the metadata region"),
            )),
        }
    };
    let value = match kind {
        6 => {
            let [x, y, z, _] = floats()?;
            AttributeValue::Vector3([x, y, z])
        }
        7 => AttributeValue::Vector4(floats()?),
        8 => AttributeValue::Quaternion(floats()?),
        14 => {
            let mut guid = [0; 16];
            guid.copy_from_slice(s.bytes("attribute", at + 0x10, 16)?);
            AttributeValue::Guid(guid)
        }
        16 | 18 => {
            let word = s.u32("attribute", at + 0x10)?;
            if s.u32("attribute", at + 0x14)? != 0 || s.u64("attribute", at + 0x18)? != 1 {
                return Err(bad(
                    at,
                    format!("a type-{kind} attribute is not {{a word, 0, 1}}"),
                ));
            }
            match kind {
                16 => AttributeValue::FloatWide(f32::from_bits(word)),
                _ => AttributeValue::WordWide(word),
            }
        }
        21 => {
            if s.bytes("attribute", at + 0x10, 16)?.iter().any(|b| *b != 0) {
                return Err(bad(at, "an attribute of type 21 holds a non-zero byte"));
            }
            AttributeValue::Empty
        }
        4 if s.u64("attribute", at + 0x10)? == 0 && s.u64("attribute", at + 0x18)? == 1 => {
            AttributeValue::String(String::new())
        }
        4 => {
            let from = reference()?;
            let rest = &s.bytes("attribute text", from, region.hi - from)?;
            let text = rest
                .iter()
                .position(|b| *b == 0)
                .map(|end| &rest[..end])
                .filter(|t| !t.is_empty() && t.iter().all(|b| (32..127).contains(b)))
                .ok_or_else(|| {
                    bad(
                        from,
                        "a string attribute's text is not printable and ended inside the region",
                    )
                })?;
            AttributeValue::String(String::from_utf8_lossy(text).into_owned())
        }
        5 => bit_set(s, region, reference()?)?,
        11 => data(s, region, reference()?, name)?,
        13 if depth <= OBJECT_DEPTH => object(s, region, reference()?, depth)?,
        13 => {
            return Err(bad(
                at,
                format!("a nested object lies more than {OBJECT_DEPTH} objects deep"),
            ))
        }
        23 | 24 | 33 => {
            let words = relative(s, region, at + 0x10, 4)?
                .into_iter()
                .map(|e| s.u32("attribute array", e))
                .collect::<Result<Vec<_>, _>>()?;
            match kind {
                23 => {
                    let floats: Vec<f32> = words.into_iter().map(f32::from_bits).collect();
                    if !floats.iter().all(|f| f.is_finite()) {
                        return Err(bad(
                            at,
                            "a float array attribute holds a value that is not finite",
                        ));
                    }
                    AttributeValue::Floats(floats)
                }
                24 => AttributeValue::Words(words),
                _ => AttributeValue::Hashes(words),
            }
        }
        other => {
            return Err(bad(
                at,
                format!("a 32-byte attribute of type {other}, which no record was seen to hold"),
            ))
        }
    };
    Ok(Attribute { name, value })
}

/// The bit set at `at`: `{u16 n, u16 words[ceil(n / 16)]}` on an even byte, no bit past n set.
fn bit_set(s: &Section, region: Region, at: usize) -> Result<AttributeValue, AnimError> {
    region.holds(at, 2)?;
    let bits = s.u16("bit set", at)?;
    if !at.is_multiple_of(2) || bits == 0 {
        return Err(bad(
            at,
            format!("a bit set of {bits} bits starts on an odd byte or holds no bit"),
        ));
    }
    let len = 2 * usize::from(bits).div_ceil(16);
    region.holds(at + 2, len)?;
    s.spend(len)?;
    let words: Vec<u16> = (s.bytes("bit set", at + 2, len)?.as_chunks::<2>().0.iter())
        .map(|w| u16::from_le_bytes(*w))
        .collect();
    let ones: u32 = words.iter().map(|w| w.count_ones()).sum();
    let value = AttributeValue::BitSet { bits, words };
    if value.set_bits().len() != ones as usize {
        return Err(bad(
            at,
            format!("a bit set of {bits} bits sets a bit past its last"),
        ));
    }
    Ok(value)
}

/// The data object at `at`: `{table word, u64 0, u16 11, u16 0xFFFF, its attribute's name, a
/// relative array of n bytes of capacity n}` on 8 bytes.
fn data(s: &Section, region: Region, at: usize, name: u32) -> Result<AttributeValue, AnimError> {
    region.holds(at, DATA_LEN)?;
    let count = s.u32("data", at + 0x18)?;
    let holds = at.is_multiple_of(8)
        && vtable_like(s.u64("data", at)?)
        && s.u64("data", at + 8)? == 0
        && s.u16("data", at + 0x10)? == 11
        && s.u16("data", at + 0x12)? == 0xFFFF
        && s.u32("data", at + 0x14)? == name
        && count >= 1
        && s.u32("data", at + 0x1C)? == count;
    if !holds {
        return Err(bad(at, "a data object's head is not {a table word, 0, 11, 0xFFFF, its attribute's name, n bytes of capacity n} on 8 bytes"));
    }
    let offset = s.u64("data", at + 0x20)? as i64;
    let from = (at as i64 + 0x20)
        .checked_add(offset)
        .and_then(|from| usize::try_from(from).ok())
        .ok_or_else(|| bad(at, "a data object's bytes lie before the section"))?;
    let len = count as usize;
    region.holds(from, len)?;
    s.spend(len)?;
    Ok(AttributeValue::Data(s.bytes("data", from, len)?.to_vec()))
}

/// The object at `at`, `depth` objects deep: `{table word, u64 0, its 12- and 32-byte attributes,
/// an address-like word, u16 3, u16 0xFFFF or 0, u32 signature}` on 8 bytes.
fn object(
    s: &Section,
    region: Region,
    at: usize,
    depth: usize,
) -> Result<AttributeValue, AnimError> {
    region.holds(at, OBJECT_LEN)?;
    let holds = at.is_multiple_of(8)
        && vtable_like(s.u64("nested object", at)?)
        && s.u64("nested object", at + 8)? == 0
        && s.u64("nested object", at + 0x30)? >> 28 == 5
        && s.u16("nested object", at + 0x38)? == NESTED
        && matches!(s.u16("nested object", at + 0x3A)?, 0 | 0xFFFF);
    if !holds {
        return Err(bad(at, "a nested object's head is not {a table word, 0, two attribute arrays, a 0x5XXXXXXX word, 3, 0xFFFF or 0} on 8 bytes"));
    }
    Ok(AttributeValue::Object(attributes(
        s,
        region,
        at,
        depth + 1,
    )?))
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
