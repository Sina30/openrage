use crate::corpus;
use crate::support;

use corpus::Corpus;
use or_io::meta::binary::parse_meta_binary_with;
use or_io::meta::{Diagnostics, MetaInstance, Value};

#[derive(Default, Debug)]
struct Stats {
    values: usize,
    raw: Vec<String>,
    enums: usize,
    unresolved_enums: usize,
    hashes: usize,
    unresolved_hashes: usize,
    strings: usize,
    non_empty_strings: usize,
    arrays: usize,
    non_empty_arrays: usize,
    maps: usize,
    non_empty_maps: usize,
    structs: usize,
    pointers: usize,
}

fn walk(value: &Value, stats: &mut Stats) {
    stats.values += 1;
    match value {
        Value::Raw(text) => stats.raw.push(text.clone()),
        Value::Enum { variant, .. } => {
            stats.enums += 1;
            if variant.starts_with("0x") {
                stats.unresolved_enums += 1;
            }
        }
        Value::Hash(h) => {
            stats.hashes += 1;
            if h.raw.is_empty() {
                stats.unresolved_hashes += 1;
            }
        }
        Value::String(s) => {
            stats.strings += 1;
            if !s.is_empty() {
                stats.non_empty_strings += 1;
            }
        }
        Value::Array(items) => {
            stats.arrays += 1;
            if !items.is_empty() {
                stats.non_empty_arrays += 1;
            }
            for item in items {
                walk(item, stats);
            }
        }
        Value::Map(pairs) => {
            stats.maps += 1;
            if !pairs.is_empty() {
                stats.non_empty_maps += 1;
            }
            for (k, v) in pairs {
                walk(k, stats);
                walk(v, stats);
            }
        }
        Value::Struct(inner) => {
            stats.structs += 1;
            walk_instance(inner, stats);
        }
        Value::Pointer(Some(inner)) => {
            stats.pointers += 1;
            walk_instance(inner, stats);
        }
        _ => {}
    }
}

fn walk_instance(inst: &MetaInstance, stats: &mut Stats) {
    for (_, value) in &inst.fields {
        walk(value, stats);
    }
}

fn corpus_stats() -> (Stats, usize) {
    let corpus = Corpus::load();
    let lib = support::schema();
    let mut stats = Stats::default();
    let mut files = 0;
    for fixture in corpus.iter() {
        let mut diags = Diagnostics::lenient();
        let Ok(inst) = parse_meta_binary_with(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        files += 1;
        walk_instance(&inst, &mut stats);
    }
    (stats, files)
}

#[test]
fn no_placeholder_values_survive() {
    let corpus = Corpus::load();
    require_corpus!(corpus);
    let (stats, files) = corpus_stats();
    assert!(
        stats.raw.is_empty(),
        "{} placeholder values across {files} files; first 5: {:?}",
        stats.raw.len(),
        &stats.raw[..stats.raw.len().min(5)]
    );
}

/// Enum members must resolve against the file's own enum table. A numeric
/// fallback means either the enum table was mis-parsed or the value was read
/// at the wrong width — both of which a gap census cannot see.
///
/// ⚠ An unresolved *name* spells `hash_HHHHHHHH` and an unlisted *value*
/// `0xHHHHHHHH` ([`or_io::meta::binary::enum_text`]), which is what makes this
/// measurable with no snapshot: while both spelled `0x`, a snapshot-less build
/// counted **100% of 303,512 enum values across 6,071 files** as unresolved.
#[test]
fn enums_resolve_to_named_variants() {
    let corpus = Corpus::load();
    require_corpus!(corpus);

    let (stats, files) = corpus_stats();
    if stats.enums == 0 {
        return;
    }
    let unresolved_pct = (stats.unresolved_enums * 100) / stats.enums;
    assert!(
        unresolved_pct <= 10,
        "{}% of {} enum values across {files} files did not resolve to a variant name",
        unresolved_pct,
        stats.enums
    );
}

#[test]
fn strings_are_mostly_populated() {
    let corpus = Corpus::load();
    require_corpus!(corpus);
    let (stats, files) = corpus_stats();
    if stats.strings == 0 {
        return;
    }
    let populated_pct = (stats.non_empty_strings * 100) / stats.strings;
    assert!(
        populated_pct >= 50,
        "only {populated_pct}% of {} strings across {files} files were non-empty",
        stats.strings
    );
}

#[test]
fn arrays_are_mostly_populated() {
    let corpus = Corpus::load();
    require_corpus!(corpus);
    let (stats, files) = corpus_stats();
    if stats.arrays == 0 {
        return;
    }
    let populated_pct = (stats.non_empty_arrays * 100) / stats.arrays;
    assert!(
        populated_pct >= 40,
        "only {populated_pct}% of {} arrays across {files} files had elements",
        stats.arrays
    );
}

#[test]
#[ignore = "diagnostic tool; run with --ignored --nocapture"]
fn decode_quality_report() {
    let corpus = Corpus::load();
    if corpus.is_empty() {
        eprintln!(
            "(no binary fixtures present — run crates/io/or-io-verify/scripts/stage-meta-corpus.ps1)"
        );
        return;
    }
    let (stats, files) = corpus_stats();
    println!("\n=== decode quality over {files} files ===");
    println!("total values      {}", stats.values);
    println!("placeholders      {}", stats.raw.len());
    println!(
        "enums             {} ({} unresolved)",
        stats.enums, stats.unresolved_enums
    );
    println!(
        "hashes            {} ({} unresolved)",
        stats.hashes, stats.unresolved_hashes
    );
    println!(
        "strings           {} ({} non-empty)",
        stats.strings, stats.non_empty_strings
    );
    println!(
        "arrays            {} ({} non-empty)",
        stats.arrays, stats.non_empty_arrays
    );
    println!(
        "maps              {} ({} non-empty)",
        stats.maps, stats.non_empty_maps
    );
    println!("structs           {}", stats.structs);
    println!("pointers          {}", stats.pointers);
}

#[test]
#[ignore = "diagnostic tool; set OR_YMT and run with --ignored --nocapture"]
fn dump_xml() {
    let Ok(path) = std::env::var("OR_YMT") else {
        eprintln!("(set OR_YMT to a fixture path)");
        return;
    };
    let bytes = std::fs::read(&path).expect("read fixture");
    let mut diags = Diagnostics::lenient();
    let inst = parse_meta_binary_with(support::schema(), &bytes, &mut diags).expect("parse");
    println!("{}", or_io::meta::write_meta(&inst).expect("to xml"));
    for gap in diags.gaps() {
        eprintln!("gap: {gap}");
    }
}
