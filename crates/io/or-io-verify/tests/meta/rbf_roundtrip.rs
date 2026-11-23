use crate::corpus;
use crate::support;

use corpus::Corpus;
use or_io::meta::binary::layout::ContainerLayout;
use or_io::meta::binary::{parse_meta_binary_full, write_meta_binary};
use or_io::meta::{BinaryFormat, Diagnostics, MetaDocument, SourceFormat, Value};

#[test]
fn rbf_fixtures_round_trip_byte_identical() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbf);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "rbf_fixtures_round_trip_byte_identical",
            "no RBF fixtures present",
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
        let written = match write_meta_binary(BinaryFormat::Rbf, Some(&layout), &instance) {
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
        "{} of {} RBF fixtures did not round-trip:\n  {}",
        failures.len(),
        fixtures.len(),
        failures
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn rbf_shapes_are_consistent_per_tag() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbf);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "rbf_shapes_are_consistent_per_tag",
            "no RBF fixtures present",
        );
        return;
    }
    let lib = support::schema();
    let mut offenders = Vec::new();
    for fixture in &fixtures {
        let mut diags = Diagnostics::lenient();
        let Ok((_, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        if let ContainerLayout::Rbf(rbf) = &layout.container {
            if !rbf.conflicts.is_empty() {
                offenders.push(format!("{}: {:?}", fixture.name, rbf.conflicts));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "tags with more than one struct shape in the same file — per-tag layout is unsafe:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn rbf_edits_are_written() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbf);
    if fixtures.is_empty() {
        or_test_support::skip_corpus("rbf_edits_are_written", "no RBF fixtures present");
        return;
    }
    let lib = support::schema();

    for fixture in fixtures.iter().take(20) {
        let mut doc = MetaDocument::parse(lib, fixture.bytes.clone()).expect("parse");
        assert_eq!(doc.source_format, SourceFormat::Rbf);

        let Some(slot) = doc
            .instance
            .fields
            .iter_mut()
            .find_map(|(_, v)| matches!(v, Value::String(_)).then_some(v))
        else {
            continue;
        };
        *slot = Value::String("openrage-edit".into());

        let written = doc.to_bytes().expect("write");
        assert_ne!(
            written, fixture.bytes,
            "{}: edit did not change the output",
            fixture.name
        );

        let reloaded = MetaDocument::parse(lib, written).expect("re-parse");
        let found = reloaded
            .instance
            .fields
            .iter()
            .any(|(_, v)| matches!(v, Value::String(s) if s == "openrage-edit"));
        assert!(found, "{}: edited value did not survive", fixture.name);
        return;
    }
}

#[test]
fn rbf_document_save_is_lossless() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbf);
    if fixtures.is_empty() {
        or_test_support::skip_corpus("rbf_document_save_is_lossless", "no RBF fixtures present");
        return;
    }
    let lib = support::schema();
    for fixture in fixtures.iter().take(50) {
        let doc = MetaDocument::parse(lib, fixture.bytes.clone()).expect("parse");
        let written = doc.to_bytes().expect("write");
        assert_eq!(
            written, fixture.bytes,
            "{}: MetaDocument save was not lossless",
            fixture.name
        );
    }
}
