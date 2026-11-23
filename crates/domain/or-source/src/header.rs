//! The header words of a texture as `textures.json` states them.
//!
//! A word of flag bits is a list: the names the engine gives its bits, then, for every bit it gives
//! none, one `0x…` string. A number reads as well, since every source written before the names was.
//! The names come from `or-io`'s declarations and are computed from them, never copied.

use std::fmt;

use or_io::codec::texture::GrcDX11InfoBits;
use or_io::decl::OpenEnum;
use or_io::gta5::gen9::TileMode;
use or_io::gta5::texture_dict::gen8::PROCESSED;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// The words that are flags, in the order `WORDS` holds them.
const RESOURCE_TYPE: usize = 0;
const TEMPLATE_TYPE: usize = 1;
const EXTRA_FLAGS: usize = 2;
const INFO_BITS: usize = 3;
const FLAGS: usize = 4;
const USAGE_DATA: usize = 5;

/// A flag word: its name in the JSON and how many bits it has.
const WORDS: [(&str, u32); 6] = [
    ("resource_type_and_conversion_flags", 8),
    ("template_type", 32),
    ("extra_flags", 32),
    ("info_bits", 8),
    ("flags", 32),
    ("usage_data", 32),
];

/// The bits `word` has a name for, each with its mask. `info_bits` is a byte: its bit 0, sRGB, is the DDS's to state.
fn names(word: usize) -> Vec<(&'static str, u32)> {
    let info = |set: fn(&mut GrcDX11InfoBits)| {
        let mut bits = GrcDX11InfoBits::default();
        set(&mut bits);
        u32::from(u8::from(bits))
    };
    match word {
        RESOURCE_TYPE => vec![("processed", u32::from(PROCESSED))],
        INFO_BITS => vec![
            ("owns_backing_store", info(|b| b.owns_backing_store = true)),
            ("has_been_deleted", info(|b| b.has_been_deleted = true)),
            ("dynamic", info(|b| b.dynamic = true)),
            ("dirty", info(|b| b.dirty = true)),
        ],
        _ => Vec::new(),
    }
}

/// The bits of `word`, as many as its field holds.
fn width(word: usize) -> u32 {
    WORDS[word].1
}

fn write<S: Serializer>(word: usize, value: u32, s: S) -> Result<S::Ok, S::Error> {
    let mut items = Vec::new();
    let mut rest = value;
    for (name, mask) in names(word) {
        if value & mask == mask {
            items.push(name.to_string());
            rest &= !mask;
        }
    }
    if rest != 0 {
        items.push(format!(
            "0x{rest:0digits$x}",
            digits = (width(word) / 4) as usize
        ));
    }
    s.collect_seq(items)
}

struct Bits(usize);

impl Bits {
    fn fit<E: de::Error>(&self, value: u64) -> Result<u32, E> {
        let bits = width(self.0);
        if bits < 32 && value >> bits != 0 || value > u64::from(u32::MAX) {
            return Err(E::custom(format!(
                "{value:#x} does not fit the {bits} bits of {}",
                WORDS[self.0].0
            )));
        }
        Ok(value as u32)
    }

    fn named<E: de::Error>(&self, item: &str) -> Result<u32, E> {
        if let Some((_, mask)) = names(self.0).into_iter().find(|(n, _)| *n == item) {
            return Ok(mask);
        }
        if let Some(hex) = item.strip_prefix("0x").or_else(|| item.strip_prefix("0X")) {
            if let Ok(value) = u64::from_str_radix(hex, 16) {
                return self.fit(value);
            }
        }
        let known: Vec<_> = names(self.0).into_iter().map(|(n, _)| n).collect();
        Err(E::custom(format!(
            "{item:?} is not a bit of {} that the engine names ({}); state any other bit as 0x…",
            WORDS[self.0].0,
            if known.is_empty() {
                "it names none".to_string()
            } else {
                format!("it names {}", known.join(", "))
            }
        )))
    }
}

impl<'de> Visitor<'de> for Bits {
    type Value = u32;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} as a number or a list of bit names and 0x… strings",
            WORDS[self.0].0
        )
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<u32, E> {
        self.fit(value)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<u32, E> {
        u64::try_from(value)
            .map_err(|_| E::invalid_value(de::Unexpected::Signed(value), &self))
            .and_then(|value| self.fit(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<u32, A::Error> {
        let mut bits = 0;
        while let Some(item) = seq.next_element::<String>()? {
            bits |= self.named::<A::Error>(&item)?;
        }
        Ok(bits)
    }
}

fn read<'de, D: Deserializer<'de>>(word: usize, d: D) -> Result<u32, D::Error> {
    d.deserialize_any(Bits(word))
}

/// The JSON Schema of a flag word: a number, or a list of its names and `0x…` strings.
fn flags_schema(word: usize) -> schemars::Schema {
    let mut named: Vec<serde_json::Value> =
        names(word).into_iter().map(|(n, _)| n.into()).collect();
    named.sort_by_key(ToString::to_string);
    let mut item = serde_json::json!({ "type": "string", "pattern": "^0[xX][0-9a-fA-F]+$" });
    if !named.is_empty() {
        item = serde_json::json!({ "anyOf": [{ "enum": named }, item] });
    }
    schemars::json_schema!({
        "anyOf": [
            { "type": "integer", "minimum": 0 },
            { "type": "array", "items": item },
        ],
        "description": format!(
            "{}: the names of its bits and, for every other bit, one 0x… string; a number reads as well",
            WORDS[word].0
        ),
    })
}

macro_rules! flag_word {
    ($module:ident, $word:expr, $ty:ty) => {
        pub(crate) mod $module {
            use super::*;

            pub fn serialize<S: Serializer>(value: &$ty, s: S) -> Result<S::Ok, S::Error> {
                write($word, u32::from(*value), s)
            }

            pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<$ty, D::Error> {
                // `fit` has bounded it to the field.
                read($word, d).map(|value| value as $ty)
            }

            pub fn schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                flags_schema($word)
            }
        }
    };
}

flag_word!(resource_type, RESOURCE_TYPE, u8);
flag_word!(template_type, TEMPLATE_TYPE, u32);
flag_word!(extra_flags, EXTRA_FLAGS, u32);
flag_word!(info_bits, INFO_BITS, u8);
flag_word!(flags, FLAGS, u32);
flag_word!(usage_data, USAGE_DATA, u32);

/// An Enhanced texture's tile mode: the name `or-io` declares for it, or a number no variant has.
pub(crate) mod tile_mode {
    use super::*;

    pub fn serialize<S: Serializer>(value: &u8, s: S) -> Result<S::Ok, S::Error> {
        match <TileMode as OpenEnum>::describe().name_of(i64::from(*value)) {
            Some(name) => s.serialize_str(name),
            None => s.serialize_u8(*value),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Stated {
            Number(u8),
            Name(String),
        }
        match Stated::deserialize(d)? {
            Stated::Number(raw) => Ok(raw),
            Stated::Name(name) => {
                let tile = <TileMode as OpenEnum>::describe();
                tile.variants
                    .iter()
                    .find(|v| v.name == name)
                    .and_then(|v| u8::try_from(v.value).ok())
                    .ok_or_else(|| {
                        let known: Vec<_> = tile.variants.iter().map(|v| v.name.as_str()).collect();
                        de::Error::custom(format!(
                            "tile_mode {name:?} is not one the engine names ({}); a number reads as well",
                            known.join(", ")
                        ))
                    })
            }
        }
    }

    pub fn schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let names: Vec<_> = <TileMode as OpenEnum>::describe()
            .variants
            .into_iter()
            .map(|v| v.name)
            .collect();
        schemars::json_schema!({
            "anyOf": [{ "enum": names }, { "type": "integer", "minimum": 0, "maximum": 255 }],
            "description": "How the pixels are arranged: the name the engine's declaration gives the mode, or its number",
        })
    }
}

/// The header words of a Legacy texture that its DDS cannot state; a word that is absent is zero.
#[derive(Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct LegacyHeader {
    #[serde(skip_serializing_if = "is_zero", with = "resource_type")]
    #[schemars(schema_with = "resource_type::schema")]
    pub resource_type_and_conversion_flags: u8,
    #[serde(skip_serializing_if = "is_zero", with = "template_type")]
    #[schemars(schema_with = "template_type::schema")]
    pub template_type: u32,
    #[serde(skip_serializing_if = "is_zero", with = "extra_flags")]
    #[schemars(schema_with = "extra_flags::schema")]
    pub extra_flags: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub handle_index: u32,
    /// Without bit 0, the sRGB flag, which the DDS states.
    #[serde(skip_serializing_if = "is_zero", with = "info_bits")]
    #[schemars(schema_with = "info_bits::schema")]
    pub info_bits: u8,
    #[serde(skip_serializing_if = "is_zero")]
    pub stereo_rt_mode: i32,
}

/// The header words of an Enhanced texture that its DDS cannot state; a word that is absent is zero.
#[derive(Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct EnhancedHeader {
    #[serde(skip_serializing_if = "is_zero", with = "flags")]
    #[schemars(schema_with = "flags::schema")]
    pub flags: u32,
    #[serde(skip_serializing_if = "is_zero", with = "usage_data")]
    #[schemars(schema_with = "usage_data::schema")]
    pub usage_data: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub anti_alias_type: u8,
    #[serde(skip_serializing_if = "is_zero", with = "tile_mode")]
    #[schemars(schema_with = "tile_mode::schema")]
    pub tile_mode: u8,
}

fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}
