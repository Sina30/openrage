//! Every array member the PSC and a shipped type table both describe, and
//! whether they agree about its shape.
//!
//! ⚠ The only check that catches a size the parser reads from the wrong
//! attribute: the hermetic tests in [`or_io::meta::schema`] assert that
//! `size="16"` becomes 16, which such a parser also satisfies. A PSO type
//! table states the element count independently, zero for a heap array. It
//! needs a corpus and a schema snapshot and records a skip without them.

use crate::corpus;
use crate::support;

use std::collections::{BTreeMap, BTreeSet};

use corpus::Corpus;
use or_io::meta::binary::layout::ContainerLayout;
use or_io::meta::binary::parse_meta_binary_full;
use or_io::meta::binary::pso::schema::{PsoDataType, PsoSchemaItem, ARRAY_INFO_NAME_HASH};
use or_io::meta::{ArrayKind, BinaryFormat, Diagnostics, FieldType};

const WHAT: &str = "no_array_shape_disagrees_between_the_psc_and_a_shipped_type_table";

/// What the shipped type tables say each `(struct, field)` array member's
/// element count is. A member several documents carry must say one thing.
fn counts_from_the_corpus() -> BTreeMap<(String, String), BTreeSet<u16>> {
    let idx = support::hash_index();
    let mut out: BTreeMap<(String, String), BTreeSet<u16>> = BTreeMap::new();
    for fixture in Corpus::of_format(BinaryFormat::Pso) {
        let mut diags = Diagnostics::lenient();
        let Ok((_, layout)) = parse_meta_binary_full(support::schema(), &fixture.bytes, &mut diags)
        else {
            continue;
        };
        let ContainerLayout::Pso(pso) = &layout.container else {
            continue;
        };
        for item in &pso.schema.items {
            let PsoSchemaItem::Struct(info) = item else {
                continue;
            };
            for e in &info.entries {
                if e.name_hash == ARRAY_INFO_NAME_HASH || e.data_type != PsoDataType::Array {
                    continue;
                }
                out.entry((
                    idx.name_or_literal(info.name_hash),
                    idx.name_or_literal(e.name_hash),
                ))
                .or_default()
                .insert(e.count());
            }
        }
    }
    out
}

#[test]
fn no_array_shape_disagrees_between_the_psc_and_a_shipped_type_table() {
    let lib = support::schema();
    if lib.structs.is_empty() {
        or_test_support::skip_corpus(
            WHAT,
            "no schema snapshot is installed; see crates/io/or-io/src/meta/README.md",
        );
        return;
    }
    let binary = counts_from_the_corpus();
    if binary.is_empty() {
        or_test_support::skip_corpus(WHAT, "no PSO fixtures staged");
        return;
    }

    let mut fixed_and_equal = 0usize;
    let mut heap_on_both = 0usize;
    // The PSC states the size as a C++ constant, so the parser has no number
    // to carry.
    let mut unstated = 0usize;
    let mut wrong: Vec<String> = Vec::new();

    for ((owner, field), counts) in &binary {
        let Some(def) = lib.struct_(owner) else {
            continue;
        };
        let Some(FieldType::Array { kind, .. }) =
            def.fields.iter().find(|f| &f.name == field).map(|f| &f.ty)
        else {
            continue;
        };
        if counts.len() != 1 {
            wrong.push(format!(
                "{owner}.{field}: the corpus itself states {counts:?}"
            ));
            continue;
        }
        let stored = u32::from(*counts.iter().next().expect("one count"));
        match (stored, kind) {
            (0, ArrayKind::AtArray | ArrayKind::AtPtrArray) => heap_on_both += 1,
            (n, ArrayKind::AtFixedArray(m) | ArrayKind::Member(m)) if n == *m => {
                fixed_and_equal += 1
            }
            (_, ArrayKind::AtFixedArray(0) | ArrayKind::Member(0)) => unstated += 1,
            (n, k) => wrong.push(format!(
                "{owner}.{field}: the file says {}, the schema says {k:?}",
                if n == 0 {
                    "a heap array".to_string()
                } else {
                    format!("{n} elements")
                }
            )),
        }
    }

    let exercised = fixed_and_equal + heap_on_both + unstated + wrong.len();
    println!(
        "{exercised} array members are described on both sides: {fixed_and_equal} fixed and \
         agreeing, {heap_on_both} heap on both, {unstated} sized only in the file"
    );

    // ⚠ Every comparison needs the member's name to resolve on both sides,
    // and a name that stops resolving turns this into a loop over nothing that
    // reports no disagreement. 273 of the 320 array members the corpus type
    // tables name are compared; the other 47 name a struct or a member the
    // schema does not carry.
    assert!(
        exercised >= 250,
        "only {exercised} array members were compared at all, which is too few for the \
         absence of a disagreement to mean anything"
    );
    assert!(
        fixed_and_equal >= 15,
        "only {fixed_and_equal} fixed arrays were checked against a stated size, so this run \
         says nothing about whether the parser carries one"
    );
    assert!(
        wrong.is_empty(),
        "{} array members are shaped differently by the PSC and by the files that hold \
         them:\n  {}",
        wrong.len(),
        wrong
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
