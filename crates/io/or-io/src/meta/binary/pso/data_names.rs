//! Names for GTA VI's data-side hash values, which hash lower-cased text: the 32-bit `atStringHash`
//! and, with the `gta6` feature, the 64-bit string hash. Schema names hash literally and are not here.

use std::collections::{HashMap, HashSet};

use crate::meta::binary::hash_index::default_hash_index;
use crate::meta::instance::value::HashedString;

/// Longest text a document's own string may be to name a hash value.
const NAME_LEN: usize = 256;

#[derive(Debug, Default)]
pub(crate) struct DataNames {
    by32: HashMap<u32, String>,
    by64: HashMap<u64, String>,
}

impl DataNames {
    /// The names the tree's hash index carries.
    // ponytail: rebuilt for every document; cache it beside the index if opening many gets slow.
    pub(crate) fn of_tree() -> Self {
        let mut names = Self::default();
        for name in default_hash_index().names() {
            names.add(name);
        }
        names
    }

    fn add(&mut self, text: &str) {
        let bytes = text.as_bytes();
        self.by32
            .entry(crate::string_hash(bytes))
            .or_insert_with(|| text.to_string());
        #[cfg(feature = "gta6")]
        self.by64
            .entry(crate::gta6::hash::name_hash64(bytes))
            .or_insert_with(|| text.to_string());
    }

    /// A 32-bit data hash, with the name a lower-cased text gives it where one does.
    pub(crate) fn hashed(&self, hash: u32) -> HashedString {
        HashedString {
            raw: self.by32.get(&hash).cloned().unwrap_or_default(),
            hash,
        }
    }

    /// A namespaced 32-bit hash (0x4D), which hashes lower-cased text mostly and the text as
    /// written for some members, so a literal name names it where no lower-cased one does.
    pub(crate) fn ns_hashed(&self, hash: u32) -> HashedString {
        let mut hashed = self.hashed(hash);
        if hashed.raw.is_empty() {
            if let Some(name) = default_hash_index().lookup(hash) {
                hashed.raw = name.to_string();
            }
        }
        hashed
    }

    pub(crate) fn name64(&self, hash: u64) -> Option<&str> {
        self.by64.get(&hash).map(String::as_str)
    }

    /// Adds the texts of `unnamed` that name one of its hash values; false when none does.
    pub(crate) fn add_naming(&mut self, unnamed: &Unnamed) -> bool {
        let mut added = false;
        for text in &unnamed.texts {
            let bytes = text.as_bytes();
            #[cfg(feature = "gta6")]
            let wide = unnamed
                .wide
                .contains(&crate::gta6::hash::name_hash64(bytes));
            #[cfg(not(feature = "gta6"))]
            let wide = false;
            if wide || unnamed.narrow.contains(&crate::string_hash(bytes)) {
                self.add(text);
                added = true;
            }
        }
        added
    }
}

/// The hash values a read left without a name, and the texts it read that might name them.
#[derive(Debug, Default)]
pub(crate) struct Unnamed {
    pub(crate) narrow: HashSet<u32>,
    pub(crate) wide: HashSet<u64>,
    pub(crate) texts: Vec<String>,
}

impl Unnamed {
    pub(crate) fn text(&mut self, text: &str) {
        if !text.is_empty() && text.len() <= NAME_LEN {
            self.texts.push(text.to_string());
        }
    }
}
