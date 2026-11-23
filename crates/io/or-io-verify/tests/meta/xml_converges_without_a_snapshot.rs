//! The canonical form over the staged corpus with no struct definition but
//! each document's own type table.
//!
//! ⚠ The library handed to [`converge`] is empty whatever the machine has
//! staged, so this is what a clean clone imports with. The names still come
//! from the default index, so the `hash_` spelling is checked only where no
//! snapshot is installed (`OPENRAGE_SCHEMA=none`).

use crate::corpus;
use crate::support;

use std::collections::BTreeMap;

use corpus::{format_label, Corpus};
use or_io::meta::binary::parse_meta_binary_full;
use or_io::meta::canonical::{self, converge};
use or_io::meta::{BinaryFormat, Diagnostics, SchemaLibrary};
use rayon::prelude::*;

const WHAT: &str = "every_document_converges_on_its_own_type_table";

/// How many documents a failing run names.
const REPORTED: usize = 12;

/// Per mille of each container's population that may be refused before the
/// run stops being evidence.
const ALLOWANCE: usize = 20;

/// What one document did, read off a worker thread.
enum Outcome {
    Lost,
    /// The canonical form exported; the spelling of a name XML cannot carry, if it holds one.
    Converged(Option<String>),
    Refused(String),
    Moved(String),
}

#[derive(Default)]
struct Tally {
    population: usize,
    converged: usize,
    lost: Vec<String>,
    refused: Vec<String>,
    moved: Vec<String>,
    numeric_names: Vec<String>,
}

fn listed(items: &[String]) -> String {
    items
        .iter()
        .take(REPORTED)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n    ")
}

/// A tag or a pointer's `type=` the export spelt as a number XML cannot name.
fn numeric_name(xml: &str) -> Option<&str> {
    ["<0x", "</0x", "type=\"0x"].iter().find_map(|needle| {
        xml.find(needle)
            .map(|at| &xml[at..(at + 24).min(xml.len())])
    })
}

#[test]
fn every_document_converges_on_its_own_type_table() {
    let corpus = Corpus::load();
    if corpus.is_empty() {
        or_test_support::skip_corpus(WHAT, "no binary fixtures staged");
        return;
    }
    let names_from_index_only = support::schema().structs.is_empty();
    let empty = SchemaLibrary::default();

    // Each document converges on its own, so the cores share the corpus; the tallies fold in file order.
    let fixtures: Vec<_> = corpus.iter().collect();
    let outcomes: Vec<_> = fixtures
        .into_par_iter()
        .map(|fixture| {
            let outcome = match converge(&empty, &fixture.bytes) {
                Ok(c) if c.values_moved => Outcome::Lost,
                Ok(c) => {
                    let mut diags = Diagnostics::lenient();
                    let (root, layout) = parse_meta_binary_full(&empty, &c.bytes, &mut diags)
                        .expect("the canonical form reads");
                    let xml = canonical::export(&root, &layout).expect("exports");
                    Outcome::Converged(
                        numeric_name(&xml).map(|at| format!("{}: {at}", fixture.name)),
                    )
                }
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
            Outcome::Lost => tally.lost.push(fixture.name.clone()),
            Outcome::Converged(numeric) => {
                tally.converged += 1;
                tally.numeric_names.extend(numeric);
            }
            Outcome::Refused(why) => tally.refused.push(why),
            Outcome::Moved(why) => tally.moved.push(why),
        }
    }

    for (label, t) in &tallies {
        println!(
            "{label}: {} of {} converged on their own type table; {} refused; {} lost a value; \
             {} not a fixed point",
            t.converged,
            t.population,
            t.refused.len(),
            t.lost.len(),
            t.moved.len()
        );
        if !t.refused.is_empty() {
            println!("  refused:\n    {}", listed(&t.refused));
        }
    }

    for (label, t) in &tallies {
        assert!(
            t.moved.is_empty(),
            "{label}: {} documents are not a fixed point:\n    {}",
            t.moved.len(),
            listed(&t.moved)
        );
        assert!(
            t.lost.is_empty(),
            "{label}: {} documents changed what their XML imports as on the first cycle:\n    {}",
            t.lost.len(),
            listed(&t.lost)
        );
        let bound = t.population * ALLOWANCE / 1000;
        assert!(
            t.converged > 0 && t.refused.len() <= bound,
            "{label}: {} of {} refused, over the {bound} this suite treats as evidence:\n    {}",
            t.refused.len(),
            t.population,
            listed(&t.refused)
        );
        if names_from_index_only {
            assert!(
                t.numeric_names.is_empty(),
                "{label}: {} exports spell a name as 0x, which XML cannot carry:\n    {}",
                t.numeric_names.len(),
                listed(&t.numeric_names)
            );
        }
    }
}

/// The span of the first `value="N"` that holds a small unsigned integer.
fn integer_value(xml: &str) -> Option<(usize, usize)> {
    const ATTR: &str = " value=\"";
    xml.match_indices(ATTR).find_map(|(i, _)| {
        let at = i + ATTR.len();
        let end = at + xml[at..].find('"')?;
        let text = &xml[at..end];
        (!text.is_empty() && text.len() < 4 && text.bytes().all(|b| b.is_ascii_digit()))
            .then_some((at, end))
    })
}

/// A value changed in the exported text is the value the file holds after
/// import, with the file being replaced as the only source of structure.
#[test]
fn a_hand_edited_value_survives_the_import() {
    const WHAT: &str = "a_hand_edited_value_survives_the_import";
    let empty = SchemaLibrary::default();
    let mut edited = 0usize;
    let mut staged = 0usize;
    for format in [BinaryFormat::Pso, BinaryFormat::Rbm] {
        let fixtures = Corpus::of_format(format);
        staged += usize::from(!fixtures.is_empty());
        let Some((xml, layout)) = fixtures.iter().find_map(|f| {
            let c = converge(&empty, &f.bytes).ok()?;
            let mut diags = Diagnostics::lenient();
            let (root, layout) = parse_meta_binary_full(&empty, &c.bytes, &mut diags).ok()?;
            let xml = canonical::export(&root, &layout).ok()?;
            integer_value(&xml).is_some().then_some((xml, layout))
        }) else {
            continue;
        };

        let (at, end) = integer_value(&xml).unwrap();
        let replacement = if &xml[at..end] == "7" { "8" } else { "7" };
        let hand_edited = format!("{}{replacement}{}", &xml[..at], &xml[end..]);

        let bytes = canonical::import(&empty, hand_edited.as_bytes(), format, Some(&layout))
            .unwrap_or_else(|e| panic!("{format:?}: the edited XML imports: {e}"));
        let mut diags = Diagnostics::lenient();
        let (root, layout) =
            parse_meta_binary_full(&empty, &bytes, &mut diags).expect("the import reads");
        let again = canonical::export(&root, &layout).expect("exports");
        assert_eq!(again, hand_edited, "{format:?}: the edit did not survive");
        edited += 1;
    }
    if staged == 0 {
        or_test_support::skip_corpus(WHAT, "no PSO or RBM fixtures staged");
        return;
    }
    assert_eq!(
        edited, staged,
        "a staged container has no document that converges holding an integer value"
    );
}
