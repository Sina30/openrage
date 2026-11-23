//! Which name-hashes real metadata carries, and whether anything names them.

use crate::corpus;
use crate::support;

use std::collections::BTreeSet;

use corpus::Corpus;
use or_io::hash::literal_string_hash;
use or_io::meta::binary::hash_index::HashIndex;
use or_io::meta::binary::layout::{ContainerLayout, MetaLayout};
use or_io::meta::binary::parse_meta_binary_full;
use or_io::meta::binary::pso::schema::{PsoSchemaItem, ARRAY_INFO_NAME_HASH};
use or_io::meta::Diagnostics;

/// The names that were reachable only through the retired CodeWalker table.
///
/// ⚠ Names, never hashes: [`literal_string_hash`] computes the number, so a
/// wrong string here matches nothing rather than mis-naming something.
const RETIRED_FROM_THE_NAME_TABLE: &[&str] = &[
    "Content",
    "PlateProbabilities",
    "strRecordedRequest",
    "strRequestRecording",
    "strResourceReference",
];

/// Every hash a document asks to have named.
///
/// The population is what the readers themselves look up: a type, a field, an
/// enum and a variant in PSO and RBM, and an RBF tag hashed the way the index
/// would hash it. ⚠ [`ARRAY_INFO_NAME_HASH`] is excluded because it is a
/// structural key rather than a name — both readers and both writers skip it,
/// so counting it would put a permanent un-nameable entry in the denominator.
fn carried_hashes(layout: &MetaLayout, into: &mut BTreeSet<u32>) {
    match &layout.container {
        ContainerLayout::Pso(pso) => {
            for item in &pso.schema.items {
                into.insert(item.name_hash());
                match item {
                    PsoSchemaItem::Struct(s) => into.extend(
                        s.entries
                            .iter()
                            .map(|e| e.name_hash)
                            .filter(|h| *h != ARRAY_INFO_NAME_HASH),
                    ),
                    PsoSchemaItem::Enum(e) => {
                        into.extend(e.entries.iter().map(|v| v.name_hash));
                    }
                }
            }
        }
        ContainerLayout::Rbm(rbm) => {
            for class in rbm.model.classes.values() {
                into.insert(class.name_hash);
                into.extend(
                    class
                        .entries
                        .iter()
                        .map(|e| e.name_hash)
                        .filter(|h| *h != ARRAY_INFO_NAME_HASH),
                );
            }
            for enumeration in rbm.model.enums.values() {
                into.insert(enumeration.name_hash);
                into.extend(enumeration.values.iter().map(|v| v.name_hash));
            }
        }
        ContainerLayout::Rbf(rbf) => into.extend(
            rbf.descriptors
                .iter()
                .map(|d| literal_string_hash(d.name.as_bytes())),
        ),
    }
}

struct Fold {
    hashes: BTreeSet<u32>,
    parsed: usize,
    staged: usize,
}

fn fold_corpus() -> Fold {
    let corpus = Corpus::load();
    let lib = support::schema();
    let mut fold = Fold {
        hashes: BTreeSet::new(),
        parsed: 0,
        staged: corpus.len(),
    };
    for fixture in corpus.iter() {
        let mut diags = Diagnostics::lenient();
        let Ok((_, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        fold.parsed += 1;
        carried_hashes(&layout, &mut fold.hashes);
    }
    fold
}

/// Holds with no schema snapshot staged: nothing else names these five.
#[test]
fn the_retired_names_resolve_from_the_in_tree_list_alone() {
    let index = HashIndex::default().with_public_names();
    let mut resolved = 0;
    for name in RETIRED_FROM_THE_NAME_TABLE {
        assert_eq!(
            index.lookup(literal_string_hash(name.as_bytes())),
            Some(*name),
            "{name} is not reachable without a library"
        );
        resolved += 1;
    }
    assert_eq!(resolved, 5, "the residue the name table was carrying");
}

/// A name nothing carries is decoration, and decoration is how the table this
/// replaced reached 440 KB.
///
/// Four of the five: `Content` is named by the root type table for documents
/// outside this corpus, so it is held by
/// [`the_retired_names_resolve_from_the_in_tree_list_alone`] rather than here.
#[test]
fn the_retired_names_the_corpus_carries_are_the_four_it_carries() {
    let fold = fold_corpus();
    if fold.staged == 0 {
        or_test_support::skip_corpus(
            "binary metadata corpus",
            "no binary fixtures staged; run crates/io/or-io-verify/scripts/stage-meta-corpus.ps1",
        );
        return;
    }

    assert_eq!(
        fold.parsed, fold.staged,
        "{} of {} staged documents parsed; a fold over fewer is a smaller denominator \
         wearing the same number",
        fold.parsed, fold.staged
    );
    assert!(
        fold.hashes.len() > 1_000,
        "only {} distinct name-hashes over {} documents",
        fold.hashes.len(),
        fold.parsed
    );

    let carried: Vec<&str> = RETIRED_FROM_THE_NAME_TABLE
        .iter()
        .copied()
        .filter(|n| fold.hashes.contains(&literal_string_hash(n.as_bytes())))
        .collect();
    assert_eq!(
        carried,
        [
            "PlateProbabilities",
            "strRecordedRequest",
            "strRequestRecording",
            "strResourceReference"
        ],
        "over {} distinct hashes from {} documents",
        fold.hashes.len(),
        fold.parsed
    );
}

/// The whole population with what names it, one line per hash, so two
/// revisions can be compared rather than two counts.
#[test]
#[ignore = "diagnostic tool; run with --ignored --nocapture"]
fn name_coverage_report() {
    let fold = fold_corpus();
    if fold.staged == 0 {
        eprintln!("(no binary fixtures staged)");
        return;
    }
    let everything = support::hash_index();
    let library_only = HashIndex::from_library(support::schema());
    let in_tree_only = HashIndex::default().with_public_names();

    let named = |idx: &HashIndex| {
        fold.hashes
            .iter()
            .filter(|h| idx.lookup(**h).is_some())
            .count()
    };

    eprintln!(
        "documents          {} of {} staged",
        fold.parsed, fold.staged
    );
    eprintln!("snapshot           {:?}", or_io::meta::default_origin());
    eprintln!("distinct hashes    {}", fold.hashes.len());
    eprintln!("named, everything  {}", named(everything));
    eprintln!("named, library     {}", named(&library_only));
    eprintln!("named, in-tree     {}", named(&in_tree_only));
    eprintln!(
        "unnamed            {}",
        fold.hashes.len() - named(everything)
    );

    for h in &fold.hashes {
        println!("{h:#010X}\t{}", everything.lookup(*h).unwrap_or("-"));
    }
}
