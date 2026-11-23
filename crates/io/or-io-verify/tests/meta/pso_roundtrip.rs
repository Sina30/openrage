use crate::corpus;
use crate::support;

use corpus::Corpus;
use or_io::meta::binary::layout::ContainerLayout;
use or_io::meta::binary::pso::writer::write_with_coverage;
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::{BinaryFormat, Diagnostics, MetaDocument, SourceFormat, Value};

/// The headline gate: every real PSO file must survive parse → write
/// unchanged, byte for byte. **3,527 of 3,527.**
///
/// ⚠ It was red at **1,556 of 3,527** while the writer wrote an unresolved
/// variant's name hash (`0xAB34CAAD`, `AT_FRG`) where its value (3) belonged,
/// and green only where the gitignored snapshot resolved the name; the writer
/// now resolves through the file's own enum table. See
/// [`or_io::meta::binary::enum_text`].
#[test]
fn pso_fixtures_round_trip_byte_identical() {
    let fixtures = Corpus::of_format(BinaryFormat::Pso);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "pso_fixtures_round_trip_byte_identical",
            "no PSO fixtures present",
        );
        return;
    }

    let lib = support::schema();
    let mut failures = Vec::new();
    for fixture in &fixtures {
        let mut diags = Diagnostics::lenient();
        let (instance, layout) = match parse_meta_binary_full(lib, &fixture.bytes, &mut diags) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{}: parse failed: {e}", fixture.name));
                continue;
            }
        };
        let written = match write_meta_binary(BinaryFormat::Pso, Some(&layout), &instance) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{}: write failed: {e}", fixture.name));
                continue;
            }
        };
        if written != fixture.bytes {
            let at = written
                .iter()
                .zip(&fixture.bytes)
                .position(|(a, b)| a != b)
                .unwrap_or(written.len().min(fixture.bytes.len()));
            failures.push(format!(
                "{}: {} bytes vs {} expected, first difference at {at:#X}",
                fixture.name,
                written.len(),
                fixture.bytes.len()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} PSO fixtures did not round-trip:\n  {}",
        failures.len(),
        fixtures.len(),
        failures
            .iter()
            .take(15)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn pso_writer_actually_writes_the_values() {
    let fixtures = Corpus::of_format(BinaryFormat::Pso);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "pso_writer_actually_writes_the_values",
            "no PSO fixtures present",
        );
        return;
    }
    let lib = support::schema();

    const PADDING_CEILING: usize = 64;

    let mut offenders = Vec::new();
    for fixture in fixtures.iter().take(40) {
        let mut diags = Diagnostics::lenient();
        let Ok((instance, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        let ContainerLayout::Pso(pso) = &layout.container else {
            continue;
        };
        let Ok((_, coverage)) = write_with_coverage(pso, &instance) else {
            continue;
        };
        let big = coverage.runs_longer_than(PADDING_CEILING);
        if !big.is_empty() {
            offenders.push(format!(
                "{}: {} untouched of {} bytes, {} runs over {PADDING_CEILING}B, largest at {:#X} ({}B)",
                fixture.name,
                coverage.untouched_bytes(),
                coverage.total_bytes,
                big.len(),
                big.iter().max_by_key(|(_, l)| *l).map(|(o, _)| *o).unwrap_or(0),
                big.iter().map(|(_, l)| *l).max().unwrap_or(0),
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "the writer left large regions untouched — those values came from the \
         original file, not the value tree:\n  {}",
        offenders
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn pso_single_scalar_mutation_is_surgical() {
    let fixtures = Corpus::of_format(BinaryFormat::Pso);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "pso_single_scalar_mutation_is_surgical",
            "no PSO fixtures present",
        );
        return;
    }
    let lib = support::schema();

    let mut tested = 0;
    for fixture in fixtures.iter() {
        let mut doc = MetaDocument::parse(lib, fixture.bytes.clone()).expect("parse");
        assert_eq!(doc.source_format, SourceFormat::Pso);

        let Some((name, original)) = doc.instance.fields.iter().find_map(|(n, v)| match v {
            Value::Float(f) => Some((n.clone(), *f)),
            _ => None,
        }) else {
            continue;
        };
        let mutated = if original == 0.5 { 0.25 } else { 0.5 };
        doc.instance.set(name.clone(), Value::Float(mutated));

        let written = doc.to_bytes().expect("write");
        assert_ne!(
            written, fixture.bytes,
            "{}: mutation produced identical bytes",
            fixture.name
        );

        let reloaded = MetaDocument::parse(lib, written).expect("re-parse");
        assert_eq!(
            reloaded.instance.get(&name),
            Some(&Value::Float(mutated)),
            "{}: mutated value did not survive",
            fixture.name
        );

        let baseline = MetaDocument::parse(lib, fixture.bytes.clone()).expect("baseline");
        for (n, v) in &baseline.instance.fields {
            if n == &name {
                continue;
            }
            assert_eq!(
                reloaded.instance.get(n),
                Some(v),
                "{}: field '{n}' changed as a side effect",
                fixture.name
            );
        }

        tested += 1;
        if tested >= 10 {
            break;
        }
    }
}

#[test]
fn pso_output_is_reproducible() {
    let fixtures = Corpus::of_format(BinaryFormat::Pso);
    if fixtures.is_empty() {
        or_test_support::skip_corpus("pso_output_is_reproducible", "no PSO fixtures present");
        return;
    }
    let lib = support::schema();
    for fixture in fixtures.iter().take(20) {
        let mut diags = Diagnostics::lenient();
        let Ok((instance, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        let a = write_meta_binary(BinaryFormat::Pso, Some(&layout), &instance).expect("write a");
        let b = write_meta_binary(BinaryFormat::Pso, Some(&layout), &instance).expect("write b");
        assert_eq!(a, b, "{}: two encodes differed", fixture.name);
    }
}

#[test]
fn pso_write_without_layout_explains_itself() {
    let inst = or_io::meta::MetaInstance::new("CSomething");
    let err = write_meta_binary(BinaryFormat::Pso, None, &inst).expect_err("should refuse");
    let msg = err.to_string();
    assert!(msg.contains("without a layout"), "unhelpful error: {msg}");
}
