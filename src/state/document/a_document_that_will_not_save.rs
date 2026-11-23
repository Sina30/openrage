//! What the metadata editor refuses to save, and whether it says why.
//!
//! ⚠ **The validation under test is the save.** `refresh_validation` calls
//! `to_bytes` and keeps the error, so it cannot disagree with the writer; a
//! validator that could would be found out when the user's edit is lost.
//!
//! Nothing here renders: a render-only egui test proves only that drawing is
//! side-effect-free, so the message is tested and its widget is not.

use super::MetaDocument;
use or_io::meta::instance::Value;

fn corpus_files() -> Vec<std::path::PathBuf> {
    let meta = or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        "OPENRAGE_META_CORPUS",
        "binary",
    );
    let mut out = or_test_support::Corpus::with_root(meta.path(), "ymt").find();
    out.extend(or_test_support::Corpus::with_root(meta.path(), "ymf").find());
    out
}

fn a_real_binary_document() -> Option<MetaDocument> {
    for path in corpus_files() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if or_io::meta::detect_binary_format(&bytes).is_none() {
            continue;
        }
        let doc = MetaDocument::from_bytes(bytes);
        if doc.parsed.is_some() && doc.layout.is_some() {
            return Some(doc);
        }
    }
    None
}

/// A document that has an enum-valued root field, and that field's name.
///
/// ⚠ Searched for across the corpus: the first document that parses may have
/// no root enum, and a falsifier that skips on that reports green having
/// asserted nothing.
fn a_document_with_an_enum() -> Option<(MetaDocument, String)> {
    for path in corpus_files() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if or_io::meta::detect_binary_format(&bytes).is_none() {
            continue;
        }
        let doc = MetaDocument::from_bytes(bytes);
        if doc.layout.is_none() {
            continue;
        }
        let Some(parsed) = doc.parsed.as_ref() else {
            continue;
        };
        let field = parsed
            .instance
            .fields
            .iter()
            .find(|(_, v)| matches!(v, Value::Enum { .. }))
            .map(|(name, _)| name.clone());
        if let Some(field) = field {
            return Some((doc, field));
        }
    }
    None
}

#[test]
fn a_document_as_read_says_nothing_because_it_saves() {
    let Some(mut doc) = a_real_binary_document() else {
        or_test_support::skip_corpus(
            "meta validation",
            "no binary .ymt or .ymf staged, so there is no document to validate",
        );
        return;
    };

    doc.refresh_validation();
    assert_eq!(
        doc.why_it_will_not_save(),
        None,
        "a document straight off disk must encode, or the corpus round-trip suite is wrong \
         about it too"
    );
}

/// The falsifier: an enum replaced by the text it renders as, the edit a
/// round-trip through the Text tab makes because `write_meta` emits an
/// enum variant as bare text. The writer's refusal must reach the editor with
/// the constraint on it.
#[test]
fn an_enum_replaced_by_its_own_text_is_refused_and_the_reason_says_what_was_expected() {
    let Some((mut doc, field)) = a_document_with_an_enum() else {
        or_test_support::skip_corpus(
            "meta validation",
            "no staged .ymt or .ymf has an enum-valued root field, so the refusal this is \
             about cannot be provoked",
        );
        return;
    };

    doc.refresh_validation();
    assert_eq!(
        doc.why_it_will_not_save(),
        None,
        "the control is a clean document"
    );

    let parsed = doc.parsed.as_mut().expect("checked");
    let Some(Value::Enum { variant, .. }) = parsed.instance.get(&field).cloned() else {
        unreachable!("selected by matching on Enum");
    };
    parsed.instance.set(field.clone(), Value::String(variant));
    parsed.revision += 1;

    doc.refresh_validation();
    let why = doc
        .why_it_will_not_save()
        .unwrap_or_else(|| panic!("`{field}` is a string where the file wants an enum"));

    // The writer's own wording is checked: a shell rewording would be a
    // second claim that could drift from the refusal the save makes.
    assert!(
        why.contains("enum"),
        "the refusal must name what was expected, and says: {why}"
    );
}

/// The cache is keyed on the revision, so a document that has not changed is
/// not re-encoded — and one that has, is.
#[test]
fn validation_is_recomputed_when_the_revision_moves_and_not_before() {
    let Some(mut doc) = a_real_binary_document() else {
        or_test_support::skip_corpus("meta validation", "no binary .ymt or .ymf staged");
        return;
    };

    doc.refresh_validation();
    let first = doc.validation.clone();
    // ⚠ Re-encoding an unchanged document repeats its answer, so only a
    // planted one no encode produces tells a cache hit from a re-encode.
    let planted = Some((doc.revision(), Some("planted".to_owned())));
    doc.validation = planted.clone();
    doc.refresh_validation();
    assert_eq!(
        doc.validation, planted,
        "an unchanged revision re-uses its answer"
    );

    doc.parsed.as_mut().expect("parsed").revision += 1;
    doc.refresh_validation();
    assert_ne!(
        doc.validation.as_ref().map(|(at, _)| *at),
        first.as_ref().map(|(at, _)| *at),
        "a moved revision is validated again"
    );
}
