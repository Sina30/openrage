//! The staged ped metadata the game ships, which the fields of a garment are measured on.

#![allow(dead_code)]

use or_io::decl::File;
use or_io::meta::{CCreatureMetaData, CPedVariationInfo};

/// Every staged `.ymt` that opens as variation metadata, and every one that opens as creature metadata.
pub struct Staged {
    pub variations: Vec<(String, CPedVariationInfo)>,
    pub creatures: Vec<(String, CCreatureMetaData)>,
}

/// The documents of the metadata corpus, or none (with the skip said) when the corpus is not staged.
pub fn load(what: &str) -> Option<Staged> {
    let origin = or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        "OPENRAGE_META_CORPUS",
        "binary",
    );
    if !origin.found() {
        or_test_support::skip_corpus(what, "the metadata corpus is not staged");
        return None;
    }
    let mut paths: Vec<_> = std::fs::read_dir(origin.path())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ymt"))
        .collect();
    paths.sort();
    let mut staged = Staged {
        variations: Vec::new(),
        creatures: Vec::new(),
    };
    for path in paths {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Ok(info) = CPedVariationInfo::from_bytes(&bytes) {
            staged.variations.push((name, info));
        } else if let Ok(creature) = CCreatureMetaData::from_bytes(&bytes) {
            staged.creatures.push((name, creature));
        }
    }
    if staged.variations.is_empty() {
        or_test_support::skip_corpus(what, "no variation metadata is staged");
        return None;
    }
    Some(staged)
}
