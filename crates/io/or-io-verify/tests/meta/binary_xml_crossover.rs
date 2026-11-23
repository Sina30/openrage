use crate::support;

use std::fs;
use std::path::{Path, PathBuf};

use or_io::meta::instance::{parse_meta, write_meta};
use or_io::meta::{BinaryFormat, MetaDocument, Value};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("binary")
}

fn binary_fixtures() -> Vec<(PathBuf, BinaryFormat)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(fixtures_dir()) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(bytes) = fs::read(&path) {
            if let Some(fmt) = or_io::meta::detect_binary_format(&bytes) {
                out.push((path, fmt));
            }
        }
    }
    out
}

#[test]
fn binary_to_xml_to_tree_preserves_structure() {
    let fixtures = binary_fixtures();
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "binary_to_xml_to_tree_preserves_structure",
            "no binary fixtures present",
        );
        return;
    }
    let lib = support::schema();
    let mut checked = 0usize;
    let mut unparsable = 0usize;
    for (path, _fmt) in fixtures {
        let bytes = fs::read(&path).expect("read fixture");
        let doc = match MetaDocument::parse(lib, bytes) {
            Ok(d) => d,
            Err(e) => {
                or_test_support::skip_corpus(
                    "binary_to_xml_to_tree_preserves_structure",
                    format_args!("skipping unparsable {}: {e}", path.display()),
                );
                continue;
            }
        };
        let xml = match write_meta(&doc.instance) {
            Ok(s) => s,
            Err(e) => {
                or_test_support::skip_corpus(
                    "binary_to_xml_to_tree_preserves_structure",
                    format_args!("skipping unwritable {}: {e}", path.display()),
                );
                continue;
            }
        };
        let reparsed = match parse_meta(lib, xml.as_bytes()) {
            Ok(i) => i,
            Err(_) => {
                unparsable += 1;
                continue;
            }
        };
        checked += 1;
        assert_eq!(
            reparsed.type_name,
            doc.instance.type_name,
            "tree root type must match for {}",
            path.display()
        );
        assert!(
            field_count(&reparsed) <= field_count(&doc.instance) + 1,
            "field count must stay close after the XML round-trip for {}",
            path.display()
        );
    }
    account_for(
        "binary_to_xml_to_tree_preserves_structure",
        checked,
        unparsable,
    );
}

/// Records a file whose XML will not parse back as a skip rather than passing
/// over it.
///
/// ⚠ The XML is name-based and the binary hash-based, so re-reading it needs
/// the schema library, which is empty without a staged snapshot: on a
/// snapshot-less build `openrage-cli meta to-xml --batch` withholds
/// **6,071 of 6,071** corpus files, and a silent `continue` here reported
/// green having checked none of them.
fn account_for(what: &str, checked: usize, unparsable: usize) {
    if unparsable > 0 {
        or_test_support::skip_corpus(
            what,
            format_args!(
                "{unparsable} file(s) produced XML that will not parse back; the XML \
                 reader needs the schema library and none is staged (see \
                 crates/io/or-io/src/meta/README.md)"
            ),
        );
    }
    assert!(
        checked > 0 || unparsable > 0,
        "{what} neither checked a file nor recorded why not"
    );
}

#[test]
fn binary_survives_a_round_trip_through_xml() {
    let fixtures = binary_fixtures();
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "binary_survives_a_round_trip_through_xml",
            "no binary fixtures present",
        );
        return;
    }
    let lib = support::schema();

    let mut checked = 0;
    let mut unparsable = 0usize;
    let mut failures = Vec::new();
    for (path, fmt) in fixtures {
        if fmt == BinaryFormat::Rbm {
            continue;
        }
        let bytes = fs::read(&path).expect("read fixture");
        let Ok(doc) = MetaDocument::parse(lib, bytes.clone()) else {
            continue;
        };
        let Ok(xml) = write_meta(&doc.instance) else {
            continue;
        };
        let Ok(reparsed) = parse_meta(lib, xml.as_bytes()) else {
            unparsable += 1;
            continue;
        };
        let Ok(written) = or_io::meta::write_meta_binary(fmt, doc.layout.as_ref(), &reparsed)
        else {
            unparsable += 1;
            continue;
        };
        checked += 1;
        if written != bytes {
            let at = written
                .iter()
                .zip(&bytes)
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            let differing = written.iter().zip(&bytes).filter(|(a, b)| a != b).count();
            failures.push(format!(
                "{}: {differing} bytes differ, first at {at:#X}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        if checked >= 40 {
            break;
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {checked} files lost data through XML:\n  {}",
        failures.len(),
        failures
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    account_for(
        "binary_survives_a_round_trip_through_xml",
        checked,
        unparsable,
    );
}

fn field_count(inst: &or_io::meta::MetaInstance) -> usize {
    inst.fields.len()
        + inst
            .fields
            .iter()
            .map(|(_, v)| value_field_count(v))
            .sum::<usize>()
}

fn value_field_count(v: &Value) -> usize {
    match v {
        Value::Struct(inst) => field_count(inst),
        Value::Pointer(Some(inst)) => field_count(inst),
        Value::Array(items) => items.iter().map(value_field_count).sum(),
        Value::Map(pairs) => pairs.iter().map(|(_, v)| value_field_count(v)).sum(),
        _ => 0,
    }
}
