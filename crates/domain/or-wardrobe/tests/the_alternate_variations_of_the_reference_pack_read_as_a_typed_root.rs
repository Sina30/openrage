//! The alternate variations of the owner's server pack, opened as the typed root and held to the file's own text, and its variation and creature
//! files, re-encoded to a fixed point.
//!
//! The pack is in no repository, so `OPENRAGE_REFERENCE_PACK` names its folder, the one that holds `data/`; without
//! it this suite skips, which is not a pass. The pack is only read.

use std::path::PathBuf;

use or_io::decl::File;
use or_io::meta::CAlternateVariations;

const WHAT: &str = "the_alternate_variations_of_the_reference_pack_read_as_a_typed_root";

fn text() -> Option<(PathBuf, String)> {
    let root = std::env::var_os("OPENRAGE_REFERENCE_PACK").map(PathBuf::from);
    let Some(root) = root.filter(|p| p.join("data").is_dir()) else {
        or_test_support::skip_corpus(
            WHAT,
            "OPENRAGE_REFERENCE_PACK names no clothing resource; it is a pack of the owner's server, in no repository",
        );
        return None;
    };
    let path = root.join("data").join("pedalternatevariations.meta");
    let Ok(text) = std::fs::read_to_string(&path) else {
        or_test_support::skip_corpus(
            WHAT,
            "the reference pack has no pedalternatevariations.meta",
        );
        return None;
    };
    Some((path, text))
}

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

#[test]
fn every_switch_and_source_of_the_text_is_in_the_typed_root() {
    let Some((path, text)) = text() else {
        return;
    };
    let root = CAlternateVariations::from_bytes(text.as_bytes()).expect("the pack's file reads");
    // The expected figures are counted in the text, which the typed root never saw.
    let switches: usize = root.peds.iter().map(|p| p.switches.len()).sum();
    let sources: usize = root
        .peds
        .iter()
        .flat_map(|p| &p.switches)
        .map(|s| s.source_assets.len())
        .sum();
    assert_eq!(root.peds.len(), count(&text, "<name>"));
    assert_eq!(switches, count(&text, "<alt "));
    assert_eq!(switches, count(&text, "<sourceAssets>"));
    assert_eq!(
        sources,
        count(&text, "<index ") - switches,
        "every <index> that is not a switch's own is a source's"
    );
    eprintln!(
        "{}: {} peds, {switches} switches, {sources} sources",
        path.display()
            .to_string()
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(""),
        root.peds.len()
    );
    assert!(
        root.peds
            .iter()
            .flat_map(|p| &p.switches)
            .all(|s| s.alt >= 1),
        "a switch's first alternate is 1"
    );
    // ⚠ The pack names components only: no source of it states an anchor, so a prop as a source is the engine's table and no staged file.
    assert!(root
        .peds
        .iter()
        .flat_map(|p| &p.switches)
        .flat_map(|s| &s.source_assets)
        .all(|a| a.component != 255 && a.anchor == 255));
}

#[test]
fn the_canonical_form_of_the_file_is_a_fixed_point_and_means_what_the_file_meant() {
    let Some((_, text)) = text() else {
        return;
    };
    let first = CAlternateVariations::from_bytes(text.as_bytes()).expect("the pack's file reads");
    let once = first.to_bytes().expect("it saves");
    let second = CAlternateVariations::from_bytes(&once).expect("what it saved reads");
    assert!(second == first, "the same switches in the same order");
    let twice = second.to_bytes().expect("it saves again");
    assert!(twice == once, "the canonical form is saved byte for byte");
}

fn find(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
    }
    None
}

#[test]
fn the_variation_and_creature_files_of_the_reference_are_re_encoded_to_a_fixed_point_that_means_what_they_meant(
) {
    use or_io::meta::{CCreatureMetaData, CPedVariationInfo};
    let Some((path, _)) = text() else {
        return;
    };
    let pack = path
        .parent()
        .and_then(|d| d.parent())
        .expect("the pack's folder");
    let stream = pack.join("stream");
    let mut held = 0;
    for name in [
        "mp_m_freemode_01_mp_m_unity_clothes.ymt",
        "mp_f_freemode_01_mp_f_unity_clothes.ymt",
    ] {
        let Some(file) = find(&stream, name) else {
            continue;
        };
        let first =
            CPedVariationInfo::from_bytes(&std::fs::read(&file).unwrap()).expect("it reads");
        let once = first.to_bytes().expect("it re-encodes");
        let second = CPedVariationInfo::from_bytes(&once).expect("what it wrote reads");
        assert!(
            second == first,
            "{name}: the same variation after a re-encode"
        );
        assert!(
            second.to_bytes().unwrap() == once,
            "{name}: a second re-encode changes no byte"
        );
        held += 1;
    }
    let Some(file) = find(&stream, "mp_creaturemetadata_unity_clothes.ymt") else {
        assert_eq!(held, 2, "both variation files of the reference");
        return;
    };
    let first = CCreatureMetaData::from_bytes(&std::fs::read(&file).unwrap()).expect("it reads");
    let once = first.to_bytes().expect("it re-encodes");
    let second = CCreatureMetaData::from_bytes(&once).expect("what it wrote reads");
    assert!(
        second == first,
        "the same creature metadata after a re-encode"
    );
    assert!(
        second.to_bytes().unwrap() == once,
        "a second re-encode changes no byte"
    );
    assert_eq!(held, 2, "both variation files of the reference");
}
