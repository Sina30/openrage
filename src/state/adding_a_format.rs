//! What does it actually cost to add a format?
//!
//! [What a format costs the app](../../docs/reference/Crate_Reference.md#what-a-format-costs-the-app) sets a gate, and it is
//! the same shape as the one the title reorganization set: *a format
//! registered at runtime is fully usable in the editor, with no lines added to
//! `src/`*. That claim is the definition of done for the whole section, and
//! this file is what checks it — by declaring a format here, touching nothing
//! else, and seeing how far it gets.
//!
//! It is modelled directly on
//! [`crates/title/or-title/tests/adding_a_title.rs`](../../crates/title/or-title/tests/adding_a_title.rs),
//! which does the same for titles and is the standing proof that adding one
//! costs what it claims.
//!
//! # Why this is in `src/` and not in `tests/`
//!
//! `adding_a_title.rs` is an integration test because `or-title` is a library.
//! The `openrage` package has **no library target** — it is two binaries — so
//! an integration test cannot reach `DocumentKind`, `DocPayload` or
//! `document_io` at all. The suite therefore has to be a `#[cfg(test)]` module
//! compiled into the binary, and the claim it checks is about *production*
//! lines: registering a format must not add an arm, a variant or a row to any
//! file under `src/`. The test proving that is still a test.
//!
//! It lives under `state/` rather than at the crate root because
//! [`super::document_io`] is private to `crate::state`, and opening a document
//! is one of the rungs.
//!
//! # The ladder
//!
//! [`Rung`] is the ordered list of things that have to work, and
//! [`first_missing_rung`] walks it. Stating the gate as a ladder rather than
//! one assertion is what makes a partial result useful: "it stops at
//! [`Rung::Named`]" is a measurement, and "the test fails" is not.
//!
//! # It was red on purpose, and it is green now
//!
//! [`a_synthetic_format_is_fully_usable`] is the gate, and it reaches the top
//! rung. While it did not, it was `#[ignore]`d with the rung named in the
//! ignore message and a second test pinned that rung green, so the day the
//! gap closed was a red test rather than a discovery. Both are gone; the pair
//! is worth copying for the next gate that lands before the thing it gates.

use std::sync::Once;

use or_io::container::resource::ResourceType;
use or_io::Generation;

use super::format_handlers::{self as formats, FormatHandler, FormatId, Tier};
use super::{capabilities, kinds, payload_handlers, DocPayload, DocumentKind};

const EXTENSION: &str = "orsynthetic";

/// ⚠ **The builtin extension the synthetic registration replaces, and it has
/// to keep it.** A handler registered over a builtin `FormatId` replaces its
/// whole row, extensions included, so listing only [`EXTENSION`] takes
/// `.ypm` out of the live registry for the rest of the process — and
/// `from_path`, the Open dialog and `openable_extensions` all read it from
/// there.
const BUILTIN_EXTENSION: &str = "ypm";

/// ⚠ The stand-in has to be a resource type the editor does **not** name a
/// kind for, or registering over it collides with a builtin. `Ypm` is a
/// registered resource a retail install ships zero of.
const RESOURCE: ResourceType = ResourceType::Ypm;

#[derive(Debug, PartialEq, Eq)]
struct SyntheticDocument {
    byte_count: usize,
}

fn register_synthetic_format() -> FormatId {
    static REGISTER: Once = Once::new();
    let id = FormatId::new(RESOURCE, Generation::Gen8);
    REGISTER.call_once(|| {
        formats::register_at(
            Tier::Preferred,
            FormatHandler {
                id,
                extensions: &[BUILTIN_EXTENSION, EXTENSION],
            },
        );

        payload_handlers::register_at(
            Tier::Preferred,
            payload_handlers::PayloadHandler {
                id,
                parse: |_path, data| {
                    Ok(DocPayload::Registered {
                        format: FormatId::new(RESOURCE, Generation::Gen8),
                        data: std::sync::Arc::new(SyntheticDocument {
                            byte_count: data.len(),
                        }),
                    })
                },
                serialize: |payload, _| {
                    let DocPayload::Registered { data, .. } = payload else {
                        return Err("the synthetic writer was handed another payload".to_string());
                    };
                    let doc = data
                        .downcast_ref::<SyntheticDocument>()
                        .ok_or_else(|| "not a synthetic document".to_string())?;
                    Ok((vec![0u8; doc.byte_count], Vec::new()))
                },
                // ⚠ Non-zero: an empty document that serializes
                // to no bytes is a format the create menu offers and cannot
                // author, which `nothing_offered_would_refuse_if_clicked`
                // refuses.
                empty: || DocPayload::Registered {
                    format: FormatId::new(RESOURCE, Generation::Gen8),
                    data: std::sync::Arc::new(SyntheticDocument { byte_count: 1 }),
                },
            },
        );

        capabilities::register_at(
            Tier::Preferred,
            capabilities::FormatCapabilities {
                selection_summary: Some(synthetic_summary),
                ..capabilities::FormatCapabilities::none(DocumentKind::Registered(RESOURCE))
            },
        );
    });
    id
}

fn synthetic_summary(payload: &DocPayload, _selection: u64) -> String {
    let DocPayload::Registered { data, .. } = payload else {
        return String::new();
    };
    match data.downcast_ref::<SyntheticDocument>() {
        Some(doc) => format!("{} bytes, read by a registered reader", doc.byte_count),
        None => String::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rung {
    Detected,
    Named,
    Opens,
    Described,
}

impl Rung {
    fn what_it_buys(self) -> &'static str {
        match self {
            Rung::Detected => "the file is identified at all",
            Rung::Named => "the Open dialog offers it and a path resolves to a document kind",
            Rung::Opens => "the bytes parse into a document",
            Rung::Described => "a panel can ask what to draw for it",
        }
    }
}

fn first_missing_rung() -> Option<Rung> {
    let probe = std::path::PathBuf::from(format!("probe.{EXTENSION}"));

    match formats::by_extension(EXTENSION).first() {
        Some(found) if found.id.resource == RESOURCE => {}
        _ => return Some(Rung::Detected),
    }

    let Some(kind) = DocumentKind::from_path(&probe) else {
        return Some(Rung::Named);
    };
    if !kinds::openable_extensions().contains(&EXTENSION) {
        return Some(Rung::Named);
    }

    let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
        std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());
    match super::document_io::parse_payload(&probe, kind, &[], crypto) {
        Ok(payload) if payload.kind() == kind => {}
        _ => return Some(Rung::Opens),
    }

    if capabilities::for_kind(kind).is_none() {
        return Some(Rung::Described);
    }

    None
}

#[test]
fn a_format_registered_at_runtime_is_detected() {
    let id = register_synthetic_format();

    let found = formats::by_extension(EXTENSION);
    let found = found.first().expect("registered above");
    assert_eq!(found.id, id);
    assert!(formats::known_extensions().contains(&EXTENSION));
}

#[test]
fn the_capability_table_takes_a_runtime_registration_too() {
    register_synthetic_format();

    fn registered_by_a_test(_: &super::DocPayload, _: u64) -> String {
        "registered by a test".to_string()
    }
    // ⚠ Start from the row being overridden, not from an empty one.
    // Registration **replaces** a row rather than merging into it, so
    // `..FormatCapabilities::none(Ybn)` silently strips the inspector and
    // all four geometry cells from every later reader in this process.
    let base = capabilities::for_kind(DocumentKind::Ybn).expect("a builtin row exists");
    capabilities::register_at(
        Tier::Preferred,
        capabilities::FormatCapabilities {
            selection_summary: Some(registered_by_a_test),
            ..base
        },
    );

    let row = capabilities::for_kind(DocumentKind::Ybn).expect("a builtin row exists");
    let summary = row.selection_summary.expect("registered above");
    assert_eq!(
        summary(
            &super::DocPayload::Resource(or_io::Asset::Bounds(
                or_io::gta5::PhBoundAny::PhBoundSphere(Default::default())
            )),
            0
        ),
        "registered by a test",
        "a runtime registration should supersede the builtin row"
    );

    assert!(
        row.inspector.is_some() && row.meshes.is_some(),
        "an override that names one cell must not strip the others"
    );
}

#[test]
fn a_synthetic_format_is_fully_usable() {
    register_synthetic_format();

    if let Some(rung) = first_missing_rung() {
        panic!(
            "a format registered at runtime stops at {rung:?} — {}",
            rung.what_it_buys()
        );
    }
}

#[test]
fn what_registering_a_format_now_costs_and_buys() {
    register_synthetic_format();
    let probe = std::path::PathBuf::from(format!("probe.{EXTENSION}"));

    let found = formats::by_extension(EXTENSION);
    let found = found.first().expect("rung 1");
    assert_eq!(found.id.resource, RESOURCE);

    let kind = DocumentKind::from_path(&probe).expect("rung 2");
    assert_eq!(kind, DocumentKind::Registered(RESOURCE));
    assert!(kinds::openable_extensions().contains(&EXTENSION));

    let info = kind.info();
    assert_eq!(info.kind, kind);
    assert!(!info.noun.is_empty(), "a kind with no noun renders blank");
    assert!(
        info.workspace.handles_kind(kind),
        "it would be opened and immediately bounced"
    );

    let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
        std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());
    let payload =
        super::document_io::parse_payload(&probe, kind, &[1, 2, 3], crypto).expect("rung 3");
    assert_eq!(payload.kind(), kind);
    let DocPayload::Registered { data, .. } = &payload else {
        panic!("a registered reader should produce a registered payload");
    };
    assert_eq!(
        data.downcast_ref::<SyntheticDocument>(),
        Some(&SyntheticDocument { byte_count: 3 }),
        "the payload should still hold what the registrant put in it"
    );

    let handler = payload_handlers::handler_for(RESOURCE, Generation::Gen8).expect("a handler");
    assert_eq!(handler.save(&payload), Ok(vec![0u8; 3]));

    let row = capabilities::for_kind(kind).expect("rung 4");
    let summary = row.selection_summary.expect("the cell registered above");
    assert_eq!(summary(&payload, 0), "3 bytes, read by a registered reader");

    assert!(
        kinds::creatable().iter().any(|row| row.info.kind == kind),
        "a registered format with a reader should be creatable"
    );
}

#[test]
fn the_editor_now_names_a_format_it_was_not_compiled_with() {
    register_synthetic_format();

    let probe = std::path::PathBuf::from(format!("probe.{EXTENSION}"));
    let kind = DocumentKind::from_path(&probe).expect("rung 2: the editor names it");
    assert_eq!(
        kind.resource_type(),
        Some(RESOURCE),
        "the kind should carry the identity the registry detected, not a new one"
    );

    assert!(kinds::openable_extensions().contains(&EXTENSION));
    assert!(
        kind.extensions().contains(&EXTENSION),
        "a registered kind's extensions come from its handler, not from \
         ResourceType::extensions()"
    );
}

/// The generic inspector's two inputs are reachable for a registered format,
/// and neither is keyed on the enum naming the kind: the described root comes
/// off the declared format for the kind's extension, and the image off
/// [`Document::resource`](crate::state::Document), which the loader keeps for
/// any kind with a `resource_type()`. Rung 4 checks only that a row exists.
#[test]
fn a_registered_format_reaches_the_generic_inspector() {
    use or_io::container::resource::{Compression, ResourceEnvelope, Rsc7};

    let kind = DocumentKind::Registered(ResourceType::Ybd);
    let format = or_io::gta5::formats::format(kind.extension()).expect(".ybd is declared");
    let version = ResourceType::Ybd
        .read_versions()
        .into_iter()
        .find(|v| format.target_of_version(*v).is_ok())
        .expect("a declared version");

    let data = vec![0u8; 0x4000];
    let seal = Rsc7::invent_single_chunk_seal(version, data.len()).expect("a seal");
    let bytes = Rsc7::reseal(&data, &seal, Compression::default()).expect("a resource");

    let mut doc = super::Document::new(
        1,
        None,
        DocPayload::Registered {
            format: FormatId::new(ResourceType::Ybd, Generation::Gen8),
            data: std::sync::Arc::new(()),
        },
    );
    doc.resource = super::document_io::resource_image_for(kind, &bytes);
    assert!(
        doc.resource.is_some(),
        "a registered kind carries a ResourceType, so the loader keeps the \
         bytes the generic panels read"
    );
    assert!(
        super::described_root(&doc).is_some(),
        "a registered kind with a declared format has a described root without a pack"
    );
}

/// ⚠ **The second half asks the builtin table rather than the live one.**
/// `.ypm` is the resource this file registers its synthetic format over, so
/// in a process where any test above has run the live registry *does* have a
/// reader for it; `payload_handlers::HANDLERS` is what the build ships.
#[test]
fn a_format_with_no_registered_reader_still_stops_at_opens() {
    let kind = DocumentKind::Registered(ResourceType::Ypm);
    let probe = std::path::PathBuf::from("probe.ypm");

    assert_eq!(
        DocumentKind::from_path(&probe),
        Some(kind),
        "rung 2 is closed for every resource, not only the registered one"
    );

    assert!(
        !payload_handlers::HANDLERS
            .iter()
            .any(|handler| handler.id.resource == ResourceType::Ypm),
        "this build ships a reader for .ypm, so it is no longer the resource \
         nothing implements and this file's synthetic stand-in has to move"
    );
}
