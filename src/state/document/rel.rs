//! The audio metadata document: a `.rel` or `.datNN` container, its items
//! typed by the file's `rel_type` and grouped by type, and the item the
//! inspector shows.

use std::collections::BTreeMap;
use std::path::Path;

use or_io::rel::sound::SoundItem;
use or_io::rel::{RelFile, RelIndex, TypedItem};

#[derive(Debug, Clone)]
pub struct RelDocument {
    pub file: RelFile,
    /// Each indexed item, typed, in index order; or why the file's items stay bytes.
    pub typed: Result<Vec<TypedItem>, String>,
    /// A `.dat54` item's three bytes after its class id, by index position; empty for any other file.
    pub dat54_bytes: Vec<[u8; 3]>,
    /// Each type name, or class id where the items stay bytes, with its items' index positions.
    pub groups: Vec<(String, Vec<usize>)>,
    /// The index position the inspector shows.
    pub selected: Option<usize>,
}

impl RelDocument {
    pub fn open(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        let file = RelFile::open(bytes).map_err(|e| e.to_string())?;
        let dat54 = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dat54"));
        let (typed, dat54_bytes) = if dat54 {
            match typed_dat54(&file) {
                Ok((bytes, items)) => (Ok(items), bytes),
                Err(why) => (Err(why), Vec::new()),
            }
        } else {
            (file.typed_items().map_err(|e| e.to_string()), Vec::new())
        };
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        match &typed {
            Ok(items) => {
                for (i, item) in items.iter().enumerate() {
                    groups
                        .entry(item.type_name().to_string())
                        .or_default()
                        .push(i);
                }
            }
            Err(_) => {
                for (i, item) in file.items().map_err(|e| e.to_string())?.iter().enumerate() {
                    groups
                        .entry(format!("class id {}", item.type_id()))
                        .or_default()
                        .push(i);
                }
            }
        }
        Ok(Self {
            file,
            typed,
            dat54_bytes,
            groups: groups.into_iter().collect(),
            selected: None,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        self.file.save().map_err(|e| e.to_string())
    }

    /// The item at index position `i`, typed.
    pub fn item(&self, i: usize) -> Option<&TypedItem> {
        self.typed.as_ref().ok()?.get(i)
    }

    /// The bytes of the item at index position `i`.
    pub fn body(&self, i: usize) -> Option<&[u8]> {
        let (offset, length) = match &self.file.index {
            RelIndex::Hashed(entries) => entries.get(i).map(|e| (e.offset, e.length))?,
            RelIndex::Named(entries) => entries.get(i).map(|e| (e.offset, e.length))?,
        };
        let start = offset as usize;
        self.file
            .data
            .get(start..start.checked_add(length as usize)?)
    }

    /// The key the index lists item `i` under: its name, or its hash in hex.
    pub fn key(&self, i: usize) -> String {
        match &self.file.index {
            RelIndex::Hashed(entries) => entries
                .get(i)
                .map(|e| format!("0x{:08X}", e.hash))
                .unwrap_or_default(),
            RelIndex::Named(entries) => entries.get(i).map(|e| e.name.clone()).unwrap_or_default(),
        }
    }

    pub fn item_count(&self) -> usize {
        match &self.file.index {
            RelIndex::Hashed(entries) => entries.len(),
            RelIndex::Named(entries) => entries.len(),
        }
    }

    /// What a `rel_type` holds, as the files that ship it are named.
    pub fn taxonomy(&self) -> &'static str {
        match (self.file.rel_type, &self.file.index) {
            (4, RelIndex::Named(_)) => "audio configuration",
            (4, RelIndex::Hashed(_)) => "speech",
            (10, _) => "synths",
            (15, _) => "dynamic mixes",
            (16, _) => "curves",
            (22, _) => "categories",
            (54, _) => "sounds",
            (149..=151, _) => "game data",
            _ => "unknown",
        }
    }

    /// The container's bytes and its index; the typed items' own heap is not walked.
    pub fn held_bytes(&self) -> usize {
        let names: usize = self.file.names.iter().map(String::len).sum();
        let typed = self.typed.as_ref().map_or(0, Vec::len) * size_of::<TypedItem>();
        self.file.data.len() + names + self.item_count() * 12 + typed + self.dat54_bytes.len() * 3
    }
}

/// A `.dat54`'s items, each with its three extra bytes, typed as sounds.
fn typed_dat54(file: &RelFile) -> Result<(Vec<[u8; 3]>, Vec<TypedItem>), String> {
    if file.rel_type != 54 {
        return Err(format!(
            "a .dat54 holds rel_type 54, and this one states {}",
            file.rel_type
        ));
    }
    file.items()
        .map_err(|e| e.to_string())?
        .iter()
        .enumerate()
        .map(|(i, item)| {
            SoundItem::decode_dat54(item.body())
                .map(|(bytes, sound)| (bytes, TypedItem::Sound(sound)))
                .map_err(|e| format!("item {i}: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|pairs| pairs.into_iter().unzip())
}
