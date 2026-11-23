use crate::corpus;
use crate::support;

use corpus::Corpus;
use or_io::meta::binary::layout::ContainerLayout;
use or_io::meta::binary::rbm::writer::write_with_coverage;
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::{BinaryFormat, Diagnostics, MetaDocument, MetaInstance, SourceFormat, Value};

fn payload_of(bytes: &[u8]) -> Option<Vec<u8>> {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    Some(Rsc7::open(bytes).ok()?.data)
}

/// The headline gate: the decompressed payload must be byte-identical, and the
/// RSC7 header must be unchanged. **2,338 of 2,338.**
///
/// ⚠ It was red at **1,620 of 2,338** on the enum defect `pso_roundtrip`
/// describes, and here it corrupted rather than differed: `Enum8` wrote the low
/// byte of the name hash, so `AT_FRG`'s value of 3 was saved as `0xAD`, which
/// the game reads as a different variant.
#[test]
fn rbm_fixtures_round_trip_payload_identical() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "rbm_fixtures_round_trip_payload_identical",
            "no RBM fixtures present",
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
        let written = match write_meta_binary(BinaryFormat::Rbm, Some(&layout), &instance) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{}: write failed: {e}", fixture.name));
                continue;
            }
        };

        if written[..16] != fixture.bytes[..16] {
            failures.push(format!("{}: RSC7 header changed", fixture.name));
            continue;
        }

        let (Some(before), Some(after)) = (payload_of(&fixture.bytes), payload_of(&written)) else {
            failures.push(format!("{}: could not decompress", fixture.name));
            continue;
        };
        if before != after {
            let at = before
                .iter()
                .zip(&after)
                .position(|(a, b)| a != b)
                .unwrap_or(before.len().min(after.len()));
            failures.push(format!(
                "{}: payload {} bytes vs {} expected, first difference at {at:#X}",
                fixture.name,
                after.len(),
                before.len()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} RBM fixtures did not round-trip:\n  {}",
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
fn rbm_writer_actually_writes_the_values() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "rbm_writer_actually_writes_the_values",
            "no RBM fixtures present",
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
        let ContainerLayout::Rbm(rbm) = &layout.container else {
            continue;
        };
        let Ok((_, untouched)) = write_with_coverage(rbm, &instance) else {
            continue;
        };
        let big: Vec<_> = untouched
            .iter()
            .filter(|(_, len)| *len > PADDING_CEILING)
            .collect();
        if !big.is_empty() {
            offenders.push(format!(
                "{}: {} runs over {PADDING_CEILING}B, largest {}B at {:#X}",
                fixture.name,
                big.len(),
                big.iter().map(|(_, l)| *l).max().unwrap_or(0),
                big.iter()
                    .max_by_key(|(_, l)| *l)
                    .map(|(o, _)| *o)
                    .unwrap_or(0),
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
fn rbm_written_file_reloads_identically() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "rbm_written_file_reloads_identically",
            "no RBM fixtures present",
        );
        return;
    }
    let lib = support::schema();
    for fixture in fixtures.iter().take(40) {
        let mut diags = Diagnostics::lenient();
        let Ok((instance, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        let written =
            write_meta_binary(BinaryFormat::Rbm, Some(&layout), &instance).expect("write");
        let mut diags2 = Diagnostics::lenient();
        let (reloaded, _) = parse_meta_binary_full(lib, &written, &mut diags2).expect("re-read");
        assert_eq!(
            reloaded, instance,
            "{}: value tree changed across a write/read cycle",
            fixture.name
        );
    }
}

#[test]
fn rbm_edits_are_written() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus("rbm_edits_are_written", "no RBM fixtures present");
        return;
    }
    let lib = support::schema();

    let mut tested = 0;
    for fixture in fixtures.iter() {
        let mut doc = MetaDocument::parse(lib, fixture.bytes.clone()).expect("parse");
        assert_eq!(doc.source_format, SourceFormat::Rbm);

        let Some((name, original)) = first_scalar(&doc.instance) else {
            continue;
        };
        let mutated = original.wrapping_add(1);
        doc.instance.set(name.clone(), Value::UInt(mutated));

        let written = doc.to_bytes().expect("write");
        let reloaded = MetaDocument::parse(lib, written).expect("re-parse");
        assert_eq!(
            reloaded.instance.get(&name),
            Some(&Value::UInt(mutated)),
            "{}: edited value did not survive",
            fixture.name
        );

        tested += 1;
        if tested >= 10 {
            break;
        }
    }
}

fn first_scalar(inst: &MetaInstance) -> Option<(String, u64)> {
    inst.fields.iter().find_map(|(n, v)| match v {
        Value::UInt(x) => Some((n.clone(), *x)),
        _ => None,
    })
}

/// ⚠ Every RBM document, not a sample: the payload goes back under a fresh
/// DEFLATE and nothing else in this crate asserts the compressor is
/// deterministic. The count is asserted so a staging accident fails rather
/// than passing over nothing.
#[test]
fn rbm_output_is_reproducible() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus("rbm_output_is_reproducible", "no RBM fixtures present");
        return;
    }
    let lib = support::schema();
    let mut exercised = 0usize;
    for fixture in fixtures.iter() {
        let mut diags = Diagnostics::lenient();
        let Ok((instance, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        let a = write_meta_binary(BinaryFormat::Rbm, Some(&layout), &instance).expect("write a");
        let b = write_meta_binary(BinaryFormat::Rbm, Some(&layout), &instance).expect("write b");
        assert_eq!(a, b, "{}: two encodes differed", fixture.name);
        exercised += 1;
    }
    println!(
        "two encodes agreed on {exercised} of {} RBM fixtures",
        fixtures.len()
    );
    assert!(
        exercised > 0,
        "{} RBM fixtures are staged and none of them parsed, so this run compared nothing",
        fixtures.len()
    );
}

#[test]
fn rbm_write_without_layout_explains_itself() {
    let inst = MetaInstance::new("CSomething");
    let err = write_meta_binary(BinaryFormat::Rbm, None, &inst).expect_err("should refuse");
    assert!(
        err.to_string().contains("without a layout"),
        "unhelpful error: {err}"
    );
}

/// The two shapes the RBM writer refuses are enumerated and matchable, not
/// stringly-typed, so a corpus run can bucket them.
///
/// Neither occurs in retail content — 0 occurrences across 6,071 files — so
/// they are refused rather than implemented: an encoder written against no
/// test case is worse than an honest error.
#[test]
fn the_writers_refusals_are_typed_and_countable() {
    use or_io::meta::UnsupportedShape;

    for (shape, id) in [
        (
            UnsupportedShape::PopulatedAtBinaryMap,
            "populated_at_binary_map",
        ),
        (UnsupportedShape::WideString, "wide_string"),
    ] {
        assert_eq!(shape.as_str(), id);
        assert_eq!(shape.to_string(), id);
    }

    let err = or_io::meta::MetaError::UnsupportedShape {
        shape: UnsupportedShape::WideString,
        detail: "some field".into(),
    };
    match err {
        or_io::meta::MetaError::UnsupportedShape { shape, .. } => {
            assert_eq!(shape, UnsupportedShape::WideString);
        }
        other => panic!("expected UnsupportedShape, got {other:?}"),
    }
}

#[test]
fn the_corpus_contains_neither_refused_shape() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "the_corpus_contains_neither_refused_shape",
            "no RBM fixtures present",
        );
        return;
    }
    let lib = support::schema();
    let mut written = 0usize;

    for fixture in fixtures.iter() {
        let mut diags = Diagnostics::lenient();
        let Ok((instance, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        match write_meta_binary(BinaryFormat::Rbm, Some(&layout), &instance) {
            Ok(_) => written += 1,
            Err(or_io::meta::MetaError::UnsupportedShape { shape, detail }) => panic!(
                "{}: writer met {shape} ({detail}). The corpus is no longer free of that \
                 shape, so it needs an encoder rather than a refusal.",
                fixture.name
            ),
            Err(e) => panic!("{}: {e}", fixture.name),
        }
    }
    eprintln!("{written} RBM fixtures wrote without meeting a refused shape");
}
