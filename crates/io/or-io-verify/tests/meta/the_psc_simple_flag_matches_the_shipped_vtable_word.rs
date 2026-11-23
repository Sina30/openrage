//! Every class a shipped RBM type table and the PSC both describe, and
//! whether they agree about its vtable word.
//!
//! ⚠ The only check that catches the PSC's `simple` read from the wrong
//! attribute: the hermetic tests assert what the parser does with an attribute
//! this file's author chose. A shipped table states the first member's offset
//! independently, eight for a class with a vtable. It needs a corpus and a
//! schema snapshot and records a skip without them.

use crate::corpus;
use crate::support;

use std::collections::BTreeSet;

use corpus::Corpus;
use or_io::meta::binary::layout::ContainerLayout;
use or_io::meta::binary::parse_meta_binary_full;
use or_io::meta::binary::rbm::schema::RbmClass;
use or_io::meta::binary::synth::VTABLE_BYTES;
use or_io::meta::{BinaryFormat, Diagnostics};

const WHAT: &str = "the_psc_simple_flag_matches_the_shipped_vtable_word";

fn first_member(class: &RbmClass) -> Option<u32> {
    class
        .entries
        .iter()
        .filter(|e| e.name_hash != 0x100)
        .map(|e| e.offset)
        .min()
}

#[test]
fn the_psc_simple_flag_matches_the_shipped_vtable_word() {
    let Some(lib) = support::schema_or_skip(WHAT) else {
        return;
    };
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(WHAT, "no RBM fixtures present");
        return;
    }
    let idx = support::hash_index();

    let mut compared: BTreeSet<String> = BTreeSet::new();
    let mut disagree: BTreeSet<String> = BTreeSet::new();
    for fixture in &fixtures {
        let mut diags = Diagnostics::lenient();
        let Ok((_, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        let ContainerLayout::Rbm(rbm) = &layout.container else {
            continue;
        };
        for class in rbm.model.classes.values() {
            let name = idx.name_or_literal(class.name_hash);
            let (Some(def), Some(first)) = (lib.struct_(&name), first_member(class)) else {
                continue;
            };
            compared.insert(name.clone());
            // ⚠ A simple class may start later than 8 (`CScenarioPoint` starts
            // at 21: its table omits the members before that), so only a class
            // that is not simple below the vtable word, or simple at exactly
            // it, disagrees.
            let vtable = VTABLE_BYTES as u32;
            if (!def.simple && first < vtable) || (def.simple && first == vtable) {
                disagree.insert(format!(
                    "{name}: the PSC says simple={}, a shipped table starts it at {first}",
                    def.simple
                ));
            }
        }
    }

    println!(
        "{} distinct classes in {} RBM fixtures are in both the PSC and a shipped table",
        compared.len(),
        fixtures.len()
    );
    assert!(
        !compared.is_empty(),
        "no shipped class was in the schema, so this run asserted nothing"
    );
    assert!(
        disagree.is_empty(),
        "{} classes disagree:\n  {}",
        disagree.len(),
        disagree.iter().cloned().collect::<Vec<_>>().join("\n  ")
    );
}
