//! The canonical form over the staged corpus, typed by the schema snapshot
//! the suite installed rather than by each document's own type table.
//!
//! ⚠ This is the leg [`or_io::meta::canonical`] defines and `to-xml` gates on.
//! `synthesis_fixed_point.rs` and `rbm_synthesis_fixed_point.rs` recover
//! struct definitions from the file under test, so they cannot see a schema
//! that disagrees with a shipped file; here only the XML and the snapshot
//! reach the output.
//!
//! ⚠ With no snapshot every document is typed by its own type table, and the
//! same bounds hold; `xml_converges_without_a_snapshot.rs` measures that leg
//! whatever is staged.
//!
//! Measured with the 3,128-struct snapshot: 3,520 of 3,527 PSO, 2,331 of
//! 2,338 RBM and 206 of 206 RBF converge, and none that reaches the property
//! fails it. Stamping each synthesised type with a per-call counter — hidden
//! state — fails 3,501 of 3,501 PSO and 2,331 of 2,331 RBM; the same in an
//! RBF shape word refuses 204 of 206 and moves the other 2.

use crate::corpus;
use crate::support;

use std::collections::{BTreeMap, BTreeSet};

use corpus::{format_label, Corpus};
use or_io::meta::binary::layout::ContainerLayout;
use or_io::meta::binary::parse_meta_binary_full;
use or_io::meta::binary::pso::schema::{PsoSchemaItem, ARRAY_INFO_NAME_HASH};
use or_io::meta::binary::synth::{name_hash, SHIPPED_SYMBOLIC_SIZES};
use or_io::meta::canonical::converge;
use or_io::meta::{BinaryFormat, Diagnostics};
use rayon::prelude::*;

const WHAT: &str = "every_document_that_imports_reaches_its_xml_fixed_point";

/// How many documents a failing run names.
const REPORTED: usize = 12;

/// Per mille of each container's population that may be refused, or may
/// converge only after losing a value on the first cycle.
///
/// ⚠ The refusal bound is what catches a collapse: a defect that stops
/// documents reaching the property leaves every one that still reaches it a
/// fixed point, so asserting only mismatches reports it green. It admits one
/// thing, a file whose type table predates a member the PSC now declares: 7
/// PSO and 7 RBM. Laying out only what a document carries would reach them,
/// and would let an authored document that omits a field lose it silently.
fn allowance(format: BinaryFormat) -> usize {
    match format {
        BinaryFormat::Pso => 3,
        BinaryFormat::Rbm => 5,
        BinaryFormat::Rbf => 0,
    }
}

/// What one document did, read off a worker thread.
enum Outcome {
    Converged {
        reshaped: Vec<String>,
        values_moved: bool,
        undeclared: Vec<String>,
    },
    Refused(String),
    Moved(String),
}

#[derive(Default)]
struct Tally {
    population: usize,
    converged: usize,
    /// The first cycle dropped a member the schema does not declare.
    values_moved: Vec<String>,
    /// The first cycle changed the XML with nothing undeclared to blame.
    lost: Vec<String>,
    /// Members the first cycle stored as another kind of value.
    reshaped: BTreeMap<String, usize>,
    refused: Vec<String>,
    moved: Vec<String>,
}

fn listed(items: &[String]) -> String {
    items
        .iter()
        .take(REPORTED)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n    ")
}

#[test]
fn every_document_that_imports_reaches_its_xml_fixed_point() {
    let corpus = Corpus::load();
    if corpus.is_empty() {
        or_test_support::skip_corpus(
            WHAT,
            "no binary fixtures staged; run crates/io/or-io-verify/scripts/stage-meta-corpus.ps1",
        );
        return;
    }
    let library = support::schema();

    // Each document converges on its own, so the cores share the corpus; the tallies fold in file order.
    let fixtures: Vec<_> = corpus.iter().collect();
    let outcomes: Vec<_> = fixtures
        .into_par_iter()
        .map(|fixture| {
            let outcome = match converge(library, &fixture.bytes) {
                Ok(c) => Outcome::Converged {
                    reshaped: c.reshaped.iter().cloned().collect(),
                    values_moved: c.values_moved,
                    undeclared: c.undeclared.iter().cloned().collect(),
                },
                Err(e) if e.is_refusal() => Outcome::Refused(format!("{}: {e}", fixture.name)),
                Err(e) => Outcome::Moved(format!("{}: {e}", fixture.name)),
            };
            (fixture, outcome)
        })
        .collect();
    let mut tallies: BTreeMap<&'static str, Tally> = BTreeMap::new();
    for (fixture, outcome) in outcomes {
        let tally = tallies.entry(format_label(fixture.format)).or_default();
        tally.population += 1;
        match outcome {
            Outcome::Converged {
                reshaped,
                values_moved,
                undeclared,
            } => {
                tally.converged += 1;
                for member in reshaped {
                    *tally.reshaped.entry(member).or_default() += 1;
                }
                if values_moved && undeclared.is_empty() {
                    tally.lost.push(fixture.name.clone());
                } else if values_moved {
                    tally
                        .values_moved
                        .push(format!("{}: {}", fixture.name, undeclared.join(", ")));
                }
            }
            Outcome::Refused(why) => tally.refused.push(why),
            Outcome::Moved(why) => tally.moved.push(why),
        }
    }

    for (label, t) in &tallies {
        println!(
            "{label}: {} of {} converged ({} dropped a member the schema does not declare, {} \
             lost something else); {} refused; {} not a fixed point",
            t.converged,
            t.population,
            t.values_moved.len(),
            t.lost.len(),
            t.refused.len(),
            t.moved.len()
        );
        if !t.refused.is_empty() {
            println!("  refused:\n    {}", listed(&t.refused));
        }
        if !t.values_moved.is_empty() {
            println!(
                "  dropped an undeclared member:\n    {}",
                listed(&t.values_moved)
            );
        }
        for (member, n) in &t.reshaped {
            println!("  stored as another kind of value in {n} document(s): {member}");
        }
    }

    for format in [BinaryFormat::Pso, BinaryFormat::Rbm, BinaryFormat::Rbf] {
        let label = format_label(format);
        let Some(t) = tallies.get(label) else {
            continue;
        };
        assert!(
            t.moved.is_empty(),
            "{label}: {} of {} documents that reached the property are not a fixed point:\n    {}",
            t.moved.len(),
            t.converged + t.moved.len(),
            listed(&t.moved)
        );
        assert!(
            t.lost.is_empty(),
            "{label}: {} documents changed their XML on the first cycle with no undeclared \
             member to account for it, which is a value the export or import mangles:\n    {}",
            t.lost.len(),
            listed(&t.lost)
        );

        // ⚠ RBF is transcribed rather than typed, so it has no schema to
        // disagree with the file; its reshaping is the text round trip's.
        if format != BinaryFormat::Rbf {
            assert!(
                t.reshaped.is_empty(),
                "{label}: the schema lays these members out as another kind of value than the \
                 file stores them: {:?}",
                t.reshaped
            );
        }

        let bound = t.population * allowance(format) / 1000;
        assert!(
            t.converged > 0 && t.refused.len() <= bound,
            "{label}: {} of {} refused before the property could be tested, over the {bound} \
             this suite treats as evidence:\n    {}",
            t.refused.len(),
            t.population,
            listed(&t.refused)
        );
        assert!(
            t.values_moved.len() <= bound,
            "{label}: {} of {} dropped an undeclared member on the first cycle, over {bound}:\n    {}",
            t.values_moved.len(),
            t.population,
            listed(&t.values_moved)
        );
    }
}

/// Every type [`or_io::meta::shipped::SHIPPED_TYPES`] transcribes has, member for
/// member and in order, the names and data types every staged type table
/// carrying it states — and each is carried by at least one.
///
/// ⚠ The member *kind* is compared, not the subtype: `Extension` is an
/// embedded `char[8]` in the file and the type model reads a `type="member"`
/// string as a pointer, as it does the 200 the PSC corpus declares.
#[test]
fn every_shipped_type_is_what_its_type_table_says() {
    const WHAT: &str = "every_shipped_type_is_what_its_type_table_says";
    let corpus = Corpus::load();
    if corpus.is_empty() {
        or_test_support::skip_corpus(WHAT, "no binary fixtures staged");
        return;
    }
    let shipped = or_io::meta::shipped::extend(&or_io::meta::SchemaLibrary::default(), false);
    let members = |schema: &or_io::meta::binary::pso::schema::PsoSchemaSection, hash: u32| {
        schema.struct_for(hash).map(|s| {
            s.entries
                .iter()
                .filter(|e| e.name_hash != ARRAY_INFO_NAME_HASH)
                .map(|e| (e.name_hash, e.data_type))
                .collect::<Vec<_>>()
        })
    };
    let mut compared: BTreeMap<String, usize> = BTreeMap::new();
    for def in or_io::meta::shipped::shipped_types() {
        let expected = or_io::meta::binary::synth::pso_layout_for(&shipped, &def.name)
            .unwrap_or_else(|e| panic!("{}: {e}", def.name));
        let expected = members(&expected.schema, name_hash(&def.name)).expect("laid out");
        for fixture in corpus.iter() {
            let mut diags = Diagnostics::lenient();
            let Ok((_, layout)) =
                parse_meta_binary_full(support::schema(), &fixture.bytes, &mut diags)
            else {
                continue;
            };
            let ContainerLayout::Pso(p) = &layout.container else {
                continue;
            };
            let Some(stated) = members(&p.schema, name_hash(&def.name)) else {
                continue;
            };
            assert_eq!(
                stated, expected,
                "{}: {} states a different member list",
                def.name, fixture.name
            );
            *compared.entry(def.name.clone()).or_default() += 1;
        }
    }
    println!("shipped types compared with the tables that carry them: {compared:?}");
    assert_eq!(
        compared.len(),
        or_io::meta::shipped::shipped_types().len(),
        "a shipped type no staged table carries is a transcription nothing checks: {compared:?}"
    );
}

/// Every count [`SHIPPED_SYMBOLIC_SIZES`] states is the one every staged
/// type table carrying that member gives it, and every entry is carried by
/// at least one.
#[test]
fn every_shipped_symbolic_size_is_what_the_type_tables_say() {
    const WHAT: &str = "every_shipped_symbolic_size_is_what_the_type_tables_say";
    let corpus = Corpus::load();
    if corpus.is_empty() {
        or_test_support::skip_corpus(WHAT, "no binary fixtures staged");
        return;
    }
    let wanted: BTreeMap<(u32, u32), (&str, &str)> = SHIPPED_SYMBOLIC_SIZES
        .iter()
        .map(|(o, m, _)| ((name_hash(o), name_hash(m)), (*o, *m)))
        .collect();
    let mut stated: BTreeMap<(u32, u32), BTreeSet<u32>> = BTreeMap::new();
    for fixture in corpus.iter() {
        let mut diags = Diagnostics::lenient();
        let Ok((_, layout)) = parse_meta_binary_full(support::schema(), &fixture.bytes, &mut diags)
        else {
            continue;
        };
        match &layout.container {
            ContainerLayout::Pso(p) => {
                for item in &p.schema.items {
                    let PsoSchemaItem::Struct(s) = item else {
                        continue;
                    };
                    for e in &s.entries {
                        if wanted.contains_key(&(s.name_hash, e.name_hash)) {
                            stated
                                .entry((s.name_hash, e.name_hash))
                                .or_default()
                                .insert(u32::from(e.count()));
                        }
                    }
                }
            }
            ContainerLayout::Rbm(r) => {
                for c in r.model.classes.values() {
                    for e in &c.entries {
                        if wanted.contains_key(&(c.name_hash, e.name_hash)) {
                            stated
                                .entry((c.name_hash, e.name_hash))
                                .or_default()
                                .insert(e.array_count());
                        }
                    }
                }
            }
            ContainerLayout::Rbf(_) => {}
        }
    }
    for (owner, member, count) in SHIPPED_SYMBOLIC_SIZES {
        let key = (name_hash(owner), name_hash(member));
        let seen = stated.get(&key);
        assert_eq!(
            seen,
            Some(&BTreeSet::from([*count])),
            "{owner}.{member} is stated as {count}, and the staged type tables say {seen:?}"
        );
    }
    println!(
        "{} symbolic sizes checked against the staged type tables",
        SHIPPED_SYMBOLIC_SIZES.len()
    );
}
