//! Which archives a stock install ships, and which NG key each one uses.
//!
//! An NG-encrypted archive picks its key from its own *name* and size, so a
//! `.rpf` extracted out of another one has lost the thing that identifies
//! its key. Recovery searches the key table and gets back an index; turning
//! that index into a name a user recognises needs the list of names a stock
//! install has, which is install knowledge and lives here rather than in the
//! editor.

use std::collections::BTreeSet;
use std::sync::Arc;

use or_io::container::Gta5Keys;

const STOCK_ARCHIVE_NAMES: &[&str] = &[
    "common.rpf",
    "update.rpf",
    "update2.rpf",
    "x64a.rpf",
    "x64b.rpf",
    "x64c.rpf",
    "x64d.rpf",
    "x64e.rpf",
    "x64f.rpf",
    "x64g.rpf",
    "x64h.rpf",
    "x64i.rpf",
    "x64j.rpf",
    "x64k.rpf",
    "x64l.rpf",
    "x64m.rpf",
    "x64n.rpf",
    "x64o.rpf",
    "x64p.rpf",
    "x64q.rpf",
    "x64r.rpf",
    "x64s.rpf",
    "x64t.rpf",
    "x64u.rpf",
    "x64v.rpf",
    "x64w.rpf",
    "data.rpf",
    "anim.rpf",
    "audio_rel.rpf",
    "patchday1ng.rpf",
    "patchday2ng.rpf",
    "patchday3ng.rpf",
    "patchday4ng.rpf",
    "patchday5ng.rpf",
    "patchday6ng.rpf",
    "patchday7ng.rpf",
    "patchday8ng.rpf",
    "patchday9ng.rpf",
    "patchday10ng.rpf",
    "patchday11ng.rpf",
    "patchday12ng.rpf",
    "patchday13ng.rpf",
    "patchday14ng.rpf",
    "patchday15ng.rpf",
    "patchday16ng.rpf",
    "patchday17ng.rpf",
    "patchday18ng.rpf",
    "patchday19ng.rpf",
    "patchday20ng.rpf",
    "patchday21ng.rpf",
    "patchday22ng.rpf",
    "patchday23ng.rpf",
    "patchday24ng.rpf",
    "patchday25ng.rpf",
    "patchday26ng.rpf",
    "patchday27ng.rpf",
    "patchday28ng.rpf",
    "patchday29ng.rpf",
    "patchday30ng.rpf",
    "patchday31ng.rpf",
];

#[derive(Debug, Default, Clone)]
pub struct KnownArchiveNames {
    names: BTreeSet<String>,
}

impl KnownArchiveNames {
    pub fn with_stock() -> Self {
        let mut names = BTreeSet::new();
        for n in STOCK_ARCHIVE_NAMES {
            names.insert((*n).to_string());
        }
        Self { names }
    }

    pub fn insert(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !name.is_empty() {
            self.names.insert(name);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(|s| s.as_str())
    }

    pub fn build_hint(&self, keys: &Gta5Keys, recovered_index: usize, archive_size: u32) -> String {
        let mut matches: Vec<&str> = self
            .iter()
            .filter(|name| keys.ng_key_index_for(name, archive_size) == recovered_index)
            .collect();
        matches.sort_unstable();
        matches.dedup();
        let suffix = format!("NG key {recovered_index}");
        match matches.as_slice() {
            [] => format!("{suffix} recovered (original name unknown to the registry)"),
            [one] => format!("originally {one} ({suffix})"),
            many if many.len() <= 3 => {
                format!("originally {} ({suffix})", many.join(" or "))
            }
            many => {
                format!(
                    "originally one of {} (+{} more) ({suffix})",
                    many[..3].join(" or "),
                    many.len() - 3
                )
            }
        }
    }
}

pub fn format_recovery_hint(
    registry: &KnownArchiveNames,
    keys: Option<&Arc<Gta5Keys>>,
    recovered_index: usize,
    archive_size: u32,
) -> String {
    match keys {
        Some(k) => registry.build_hint(k, recovered_index, archive_size),
        None => format!("NG key {recovered_index} recovered (no keys loaded)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_registry_contains_update_rpf() {
        let r = KnownArchiveNames::with_stock();
        assert!(r.iter().any(|n| n == "update.rpf"));
        assert!(r.iter().any(|n| n == "update2.rpf"));
    }

    #[test]
    fn insert_is_idempotent() {
        let mut r = KnownArchiveNames::default();
        r.insert("custom.rpf");
        r.insert("custom.rpf");
        assert_eq!(r.iter().filter(|n| *n == "custom.rpf").count(), 1);
    }

    #[test]
    fn empty_keys_path_returns_unknown_hint() {
        let r = KnownArchiveNames::with_stock();
        let hint = format_recovery_hint(&r, None, 42, 1024);
        assert!(hint.contains("NG key 42"));
        assert!(hint.contains("no keys"));
    }
}
