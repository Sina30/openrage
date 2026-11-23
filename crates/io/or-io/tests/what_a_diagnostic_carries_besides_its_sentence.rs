//! Grouping refusals by defect, without guessing which words vary.
//!
//! ⚠ `or-cli`'s `search --verify` (321,200 parsed, 7 refused over an install)
//! groups refusals by `shape_of`: split on whitespace, replace any word
//! containing a digit with `N`. That is wrong in both directions:
//!
//! - **It splits one defect into many.** A message that names the field it was
//!   reading has no digits to mask, so one fault becomes one report line per
//!   field.
//! - **It merges different defects.** Any two messages differing only in numbers
//!   collapse, whether or not they are the same fault.
//!
//! A signature is the key plus the argument *names*, independent of wording and
//! of values.

use or_io::{Arg, Diagnostic, Severity};

/// `or-cli`'s rule, copied because `or-cli` sits above this crate;
/// `the_copy_of_shape_of_still_matches_the_original` holds the copy to it.
fn shape_of(message: &str) -> String {
    message
        .split_whitespace()
        .map(|word| {
            if word.chars().any(|c| c.is_ascii_digit()) {
                "N"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// ⚠ One defect across six fields is six report lines by message shape.
#[test]
fn the_signature_groups_what_a_message_shape_cannot() {
    let fields = [
        "params",
        "textures",
        "vertices",
        "indices",
        "bone_tags",
        "matrices",
    ];

    // The same reader, the same fault, once per field it was reading.
    let diagnostics: Vec<Diagnostic> = fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            Diagnostic::error(
                "pointer_out_of_bounds",
                format!("`GrmShader::{field}` points past the end of the heap"),
            )
            .with("field", *field)
            .with("address", Arg::Addr(0x5000_0000 + i as u64 * 0x40))
        })
        .collect();

    let by_shape: std::collections::BTreeSet<String> =
        diagnostics.iter().map(|d| shape_of(&d.message)).collect();
    let by_signature: std::collections::BTreeSet<String> =
        diagnostics.iter().map(|d| d.signature()).collect();

    eprintln!(
        "\n=== {} diagnostic(s), one defect: {} group(s) by message shape, {} by signature ===",
        diagnostics.len(),
        by_shape.len(),
        by_signature.len()
    );

    assert_eq!(
        by_shape.len(),
        fields.len(),
        "the message shape splits one defect into one line per field, because a \
         field name has no digits for the heuristic to mask. This is the failure \
         the grouping exists to prevent, reproduced."
    );
    assert_eq!(
        by_signature.len(),
        1,
        "the signature is the key plus the argument names, so one defect is one \
         group however many fields it touches or wherever it happened"
    );
}

/// ⚠ Two different faults are one group by message shape.
#[test]
fn a_message_shape_merges_faults_that_differ_only_in_a_number() {
    // Two genuinely different refusals from two different readers.
    let version = Diagnostic::error("unsupported_version", "version 165 is not supported")
        .with("version", 165u32);
    let count =
        Diagnostic::error("count_out_of_range", "version 42 is not supported").with("count", 42u32);

    assert_eq!(
        shape_of(&version.message),
        shape_of(&count.message),
        "the heuristic sees `version N is not supported` twice and cannot tell \
         these apart. Reproduced rather than argued: the sweep would report one \
         of them and hide the other."
    );
    assert_ne!(
        version.signature(),
        count.signature(),
        "the signature keys on the reader's own key, so two faults stay two \
         however similar their sentences"
    );
}

/// The signature ignores values and ignores argument order.
///
/// ⚠ Order is ignored because a reader may fill arguments in whatever order it
/// discovers them, and two call sites of one check should not read as two
/// defects. The `args` vector still keeps declaration order, because that is what
/// a report renders.
#[test]
fn a_signature_is_the_names_and_never_the_values_or_their_order() {
    let one = Diagnostic::error("bad_pointer", "x")
        .with("field", "params")
        .with("address", Arg::Addr(0x10));
    let two = Diagnostic::error("bad_pointer", "totally different wording")
        .with("address", Arg::Addr(0xBEEF_BEEF))
        .with("field", "textures");

    assert_eq!(
        one.signature(),
        two.signature(),
        "same key, same argument names — one defect"
    );
    assert_eq!(
        one.signature(),
        "bad_pointer(address,field)",
        "and the signature is readable, because it reaches a user in a report"
    );

    // But a *different* set of arguments is a different check.
    let three = Diagnostic::error("bad_pointer", "x").with("field", "params");
    assert_ne!(
        one.signature(),
        three.signature(),
        "a check that looked at an address and one that did not are different \
         checks, even under one key"
    );

    // Order of declaration survives for rendering.
    assert_eq!(
        one.args.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        vec!["field", "address"],
        "the vector keeps the order the reader filled it in, which is what a \
         report reads from"
    );
}

/// A diagnostic with no arguments is still groupable, by its key alone.
#[test]
fn a_diagnostic_with_no_arguments_signs_as_its_key() {
    let bare = Diagnostic::warning("gen9_packs_skipped", "skipped some packs");
    assert_eq!(bare.signature(), "gen9_packs_skipped");
    assert!(bare.args.is_empty());
    assert_eq!(bare.severity, Severity::Warning);
}

/// ⚠ An address renders as hex and a count as decimal, without the report
/// guessing from the magnitude.
#[test]
fn an_address_and_a_count_render_differently_because_they_are_typed() {
    let d = Diagnostic::error("x", "y")
        .with("address", Arg::Addr(0x5000_0010))
        .with("count", 16u32);

    assert_eq!(
        d.arg("address").expect("address").to_string(),
        "0x50000010",
        "an offset a reader prints as decimal is one somebody has to convert by \
         hand before it matches anything else in the log"
    );
    assert_eq!(d.arg("count").expect("count").to_string(), "16");
    assert!(d.arg("nothing").is_none());
}

/// ⚠ The copy of `or-cli`'s rule above must stay the rule.
///
/// If `shape_of` changes and this file does not, the two failure modes are being
/// measured against something nobody runs.
#[test]
fn the_copy_of_shape_of_still_matches_the_original() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the workspace root is three above or-rage-core")
        .join("crates/tools/or-cli/src/verify.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));

    assert!(
        source.contains("fn shape_of("),
        "`or-cli`'s `shape_of` is gone. If refusals now group by signature, delete \
         the copy above and the two failure-mode tests with it — they are about a \
         workaround that no longer exists. If they group some third way, measure \
         against that instead."
    );
    for rule in ["split_whitespace", "is_ascii_digit", "\"N\""] {
        assert!(
            source.contains(rule),
            "`shape_of` no longer does `{rule}`, so the copy above is measuring a \
             rule nobody runs"
        );
    }
}
