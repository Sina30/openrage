//! Nothing the create-new menu offers refuses when it is clicked.
//!
//! The menu does not have its own opinion. `kinds::creatable()` asks
//! [`why_not_authorable`](super::document_io::why_not_authorable), which reports
//! the error of the same `author_blank` that `empty_file_bytes_for_kind` runs, so
//! the two cannot drift. ⚠ Asking only whether a handler exists —
//! `handler_for(resource, Gen8).is_some()` — says nothing of whether the click
//! succeeds.

use super::document_io::{empty_file_bytes_for_kind, why_not_authorable};
use super::kinds;
use super::DocumentKind;

/// Every enabled row is attempted for real.
#[test]
fn nothing_offered_would_refuse_if_clicked() {
    let offered: Vec<DocumentKind> = kinds::creatable()
        .into_iter()
        .filter(|row| row.available())
        .map(|row| row.info.kind)
        .collect();
    assert!(
        !offered.is_empty(),
        "the menu offers nothing at all, so the query has stopped working"
    );

    for kind in &offered {
        let outcome = empty_file_bytes_for_kind(*kind);
        let bytes = outcome.unwrap_or_else(|e| {
            panic!(
                "the menu offers .{} and authoring one fails: {e}\n\n\
                 This is the defect R28 and R29 both describe. The menu and the \
                 attempt must ask `why_not_authorable`, not two different \
                 questions.",
                kind.extension()
            )
        });
        assert!(
            !bytes.is_empty(),
            ".{} authored to zero bytes",
            kind.extension()
        );
    }
    eprintln!("\n=== {} row(s) offered ===", offered.len());
}

/// The owned resource formats, the archive and the string table are all offered.
#[test]
fn what_opens_owned_is_offered() {
    let rows = kinds::creatable();

    for kind in [
        DocumentKind::Ytd,
        DocumentKind::Ydr,
        DocumentKind::Ydd,
        DocumentKind::Yft,
        DocumentKind::Gxt2,
        DocumentKind::Rpf,
    ] {
        let row = rows
            .iter()
            .find(|r| r.info.kind == kind)
            .unwrap_or_else(|| panic!(".{} must still appear as a row", kind.extension()));
        assert!(
            row.available(),
            ".{} authors from a compiled default: {:?}",
            kind.extension(),
            row.unavailable
        );
    }
}

/// Shown and disabled, not hidden: a greyed row carries its reason, where a vanished one teaches nothing.
#[test]
fn an_unavailable_row_is_still_listed_with_its_reason() {
    let rows = kinds::creatable();

    let unavailable: Vec<&kinds::Creatable> = rows.iter().filter(|r| !r.available()).collect();
    assert!(
        !unavailable.is_empty(),
        "`.fxc` is read-only, so a row must be unavailable; none is, so the \
         verdict has stopped being asked"
    );
    for row in unavailable {
        assert!(
            row.unavailable.as_deref().is_some_and(|r| r.len() > 20),
            ".{}: an unavailable row needs a sentence, not a flag — it is rendered \
             as hover text on a disabled button",
            row.info.kind.extension()
        );
    }

    // And the menu renders them that way rather than filtering them out.
    let menu = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/panels/hierarchy/mod.rs"),
    )
    .expect("hierarchy/mod.rs");
    assert!(
        menu.contains("add_enabled(row.available()") && menu.contains("on_disabled_hover_text"),
        "the menu must disable and explain rather than skip. A `filter` here would \
         satisfy every assertion above and still teach the user nothing."
    );
}

/// The offer and the attempt are the same question, asserted from the source.
///
/// `nothing_offered_would_refuse_if_clicked` catches a drift only for the kinds
/// it tries; this catches the shape.
#[test]
fn the_menu_and_the_attempt_ask_one_function() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/state");
    let kinds_rs = std::fs::read_to_string(dir.join("kinds.rs")).expect("kinds.rs");
    let io = std::fs::read_to_string(dir.join("document_io.rs")).expect("document_io.rs");

    assert!(
        kinds_rs.contains("why_not_authorable"),
        "`kinds::creatable` must ask `why_not_authorable`. Deciding for itself is \
         how the menu came to offer what the click refuses."
    );
    assert!(
        !kinds_rs.contains("handler_for("),
        "`kinds.rs` reaching for `handler_for` is it re-deriving the answer. That \
         is the exact predicate the old `can_be_created` used, and it says nothing \
         of whether the click succeeds."
    );
    // There is no predicate: the menu's verdict is the attempt's own error. Three
    // predictions were tried and each was refuted by a format, `.ypt` twice and
    // `.ycd` once.
    assert!(
        io.contains("fn author_blank("),
        "`author_blank` must exist as the one implementation"
    );
    assert!(
        io.contains("author_blank(kind).err()"),
        "`why_not_authorable` must report the attempt's own error rather than \
         predicting it. Every prediction tried here was a second model of the \
         writers, and each was refuted by a format."
    );
    let delegates = io
        .split("fn empty_file_bytes_for_kind")
        .nth(1)
        .and_then(|tail| tail.split("fn author_blank").next())
        .is_some_and(|body| body.contains("author_blank(kind)"));
    assert!(
        delegates,
        "`empty_file_bytes_for_kind` must delegate to `author_blank`, so there is \
         exactly one way a blank file gets made"
    );

    // And `why_not_authorable` returns a sentence rather than a bool, because the
    // greyed row needs one.
    let because = why_not_authorable(DocumentKind::Fxc).expect(".fxc is read-only");
    assert!(
        because.contains('.'),
        "the reason is user-facing, got {because:?}"
    );
}
