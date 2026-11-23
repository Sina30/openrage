use serde::{Deserialize, Serialize};

use crate::meta::schema::TypeName;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Hash(HashedString),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat34([[f32; 4]; 3]),
    Color(u32),
    Enum {
        type_name: TypeName,
        variant: String,
    },
    Bitset {
        type_name: TypeName,
        bits: u128,
    },
    Struct(Box<MetaInstance>),
    Pointer(Option<Box<MetaInstance>>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    ByteArray(Vec<u8>),
    Raw(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HashedString {
    pub raw: String,
    pub hash: u32,
}

impl HashedString {
    pub fn new(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let hash = crate::string_hash(raw.as_bytes());
        Self { raw, hash }
    }

    pub fn rehash(&mut self) {
        self.hash = crate::string_hash(self.raw.as_bytes());
    }

    pub fn from_hash(hash: u32) -> Self {
        Self {
            raw: String::new(),
            hash,
        }
    }

    pub fn to_literal(&self) -> String {
        format!("hash_{:08X}", self.hash)
    }

    /// ⚠ **A name is written only if it hashes back to the stored value.**
    /// The index a reader names a hash through holds both hash flavours, and
    /// [`Self::from_xml_text`] rehashes with one: `0xAF167C70` is the literal
    /// hash of `MaxTimeForAmbientReaction`, whose joaat is `0x692ECE40`, so
    /// writing the name changed an archetype's hash on re-import.
    pub fn to_xml_text(&self) -> String {
        let named = !self.raw.is_empty() && crate::string_hash(self.raw.as_bytes()) == self.hash;
        if named {
            self.raw.clone()
        } else if self.hash != 0 {
            format!("hash_{:08X}", self.hash)
        } else {
            String::new()
        }
    }

    pub fn from_xml_text(text: &str) -> Self {
        let trimmed = text.trim();
        match crate::meta::binary::hash_index::spelled_hash(trimmed) {
            Some(hash) => Self::from_hash(hash),
            None => Self::new(trimmed.to_string()),
        }
    }
}

pub type Field = (String, Value);

/// An element no declaration names, kept whole with what it holds so a save writes it as it came.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct UnknownNode {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    #[serde(default)]
    pub children: Vec<UnknownNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetaInstance {
    pub type_name: TypeName,
    pub fields: Vec<Field>,
    pub extras: Vec<UnknownNode>,
}

impl MetaInstance {
    pub fn new(type_name: impl Into<String>) -> Self {
        Self {
            type_name: type_name.into(),
            fields: Vec::new(),
            extras: Vec::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.fields
            .iter_mut()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    pub fn set(&mut self, name: impl Into<String>, value: Value) {
        let name = name.into();
        if let Some(slot) = self.fields.iter_mut().find(|(n, _)| n == &name) {
            slot.1 = value;
        } else {
            self.fields.push((name, value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_hash_writes_its_name() {
        let h = HashedString::new("SKEL_L_Hand");
        assert_eq!(h.to_xml_text(), "SKEL_L_Hand");
        assert_eq!(HashedString::from_xml_text(&h.to_xml_text()), h);
    }

    #[test]
    fn unresolvable_hash_survives_the_xml_round_trip() {
        let h = HashedString::from_hash(0xDEAD_BEEF);
        assert_eq!(h.to_xml_text(), "hash_DEADBEEF");
        let back = HashedString::from_xml_text("hash_DEADBEEF");
        assert_eq!(back.hash, 0xDEAD_BEEF);
        assert!(back.raw.is_empty());
    }

    #[test]
    fn a_name_from_the_other_hash_flavour_is_not_written_as_the_value() {
        let name = "MaxTimeForAmbientReaction";
        let stored = crate::literal_string_hash(name.as_bytes());
        assert_ne!(stored, crate::string_hash(name.as_bytes()));
        let h = HashedString {
            raw: name.to_string(),
            hash: stored,
        };
        assert_eq!(HashedString::from_xml_text(&h.to_xml_text()).hash, stored);
    }

    #[test]
    fn zero_hash_stays_empty() {
        let h = HashedString::from_hash(0);
        assert_eq!(h.to_xml_text(), "");
        assert_eq!(HashedString::from_xml_text("").hash, 0);
    }

    #[test]
    fn literal_hex_spelling_is_also_accepted() {
        assert_eq!(HashedString::from_xml_text("0x1234ABCD").hash, 0x1234_ABCD);
    }

    #[test]
    fn a_name_that_looks_like_a_placeholder_is_not_misread() {
        let h = HashedString::from_xml_text("hash_table");
        assert_eq!(h.raw, "hash_table");
        assert_eq!(h.hash, crate::string_hash(b"hash_table"));
    }
}
