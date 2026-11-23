//! `File ▸ New` authors a blank from a compiled default.
//!
//! Every resource row opens an owned value, so its blank is its
//! `PayloadHandler::empty` and needs no pack.

#[cfg(test)]
mod tests {
    use crate::state::document_io::{parse_payload, serialize_payload};
    use crate::state::payload_handlers::handler_for;
    use crate::state::prefs::RpfOutputMode;
    use crate::state::{DocPayload, DocumentKind};
    use or_io::container::resource::{ResourceEnvelope, Rsc7};

    /// `.ydr`, `.ydd` and `.yft` have no image when new, so the first save takes the owned writer.
    #[test]
    fn a_new_drawable_takes_the_owned_writer() {
        let mut state = crate::state::AppState::new();

        for kind in [DocumentKind::Ydr, DocumentKind::Ydd, DocumentKind::Yft] {
            let id = state.new_empty_document(kind);
            let doc = state.document(id).expect("the document exists");
            assert_eq!(doc.payload.kind(), kind, "a blank of the wrong kind");
            assert!(
                doc.dirty,
                ".{}: a new document must be dirty or Save silently no-ops",
                kind.extension()
            );
            let source = state.save_source_for(id).expect("a save source");
            assert!(
                matches!(source, crate::state::file_io::SaveSource::Payload(..)),
                ".{}: a new owned document saves its payload",
                kind.extension()
            );
        }
    }

    /// The smallest corpus file of `ext` whose RSC7 version names `target`.
    fn smallest_at(
        format: &or_io::gta5::formats::Format,
        target: or_io::Generation,
    ) -> Option<std::path::PathBuf> {
        let query = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), format.ext);
        let query = if target.is_gen9() {
            query.target(or_test_support::TARGET_GEN9)
        } else {
            query
        };
        (query.find().into_iter())
            .filter(|p| {
                std::fs::read(p).is_ok_and(|bytes| {
                    Rsc7::peek(&bytes).is_ok_and(|seal| {
                        format
                            .target_of_version(seal.version())
                            .is_ok_and(|t| t == target)
                    })
                })
            })
            .min_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX))
    }

    /// Every target of every declared format authors a blank, and the blank and the smallest staged file open, save and reopen equal, with no pack.
    #[test]
    fn every_declared_format_opens_with_no_pack() {
        let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
            std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());
        let (mut blanks, mut files) = (0usize, Vec::new());

        for format in or_io::gta5::formats::FORMATS {
            let ext = format.ext;
            let path = std::path::PathBuf::from(format!("blank.{ext}"));
            let kind = DocumentKind::from_path(&path)
                .unwrap_or_else(|| panic!(".{ext} is declared and the app names no kind for it"));
            let resource = kind
                .resource_type()
                .unwrap_or_else(|| panic!(".{ext}: {kind:?} is not a paged resource"));

            for target in format.targets() {
                let at = format!(".{ext} ({})", target.edition_label());
                let handler = handler_for(resource, target)
                    .unwrap_or_else(|| panic!("{at}: no payload handler"));
                assert_eq!(
                    handler.id.target, target,
                    "{at}: served by another target's handler"
                );

                let open = |what: &str, bytes: &[u8]| {
                    let payload = parse_payload(&path, kind, bytes, crypto.clone())
                        .unwrap_or_else(|e| panic!("{at}: {what} does not open with no pack: {e}"));
                    assert!(
                        payload.kind() == kind && payload.generation() == target,
                        "{at}: {what} opened as a {:?} {:?}",
                        payload.generation(),
                        payload.kind()
                    );
                    payload
                };
                let round_trip = |what: &str, bytes: &[u8]| {
                    let payload = open(what, bytes);
                    let saved = serialize_payload(&payload, RpfOutputMode::Open, crypto.clone())
                        .unwrap_or_else(|e| panic!("{at}: {what} does not save with no pack: {e}"));
                    let (DocPayload::Resource(was), DocPayload::Resource(again)) =
                        (&payload, open(what, &saved))
                    else {
                        panic!("{at}: {what} is not an owned value");
                    };
                    // ⚠ Or equal as bytes: a navmesh's or a cloth's floats can be NaN.
                    assert!(
                        again == *was || again.to_bytes().ok() == was.to_bytes().ok(),
                        "{at}: {what} reopens different after a save"
                    );
                };

                let blank = handler
                    .save(&(handler.empty)())
                    .unwrap_or_else(|e| panic!("{at}: no blank authors with no pack: {e}"));
                round_trip("a blank", &blank);
                blanks += 1;

                if let Some(file) = smallest_at(format, target) {
                    let bytes = std::fs::read(&file).expect("read a staged file");
                    round_trip(&file.display().to_string(), &bytes);
                    files.push(at);
                }
            }
        }

        assert!(blanks > 0, "no declared format was tried");
        eprintln!(
            "{blanks} blank(s) and {} staged file(s) opened, saved and reopened with no pack: {}",
            files.len(),
            files.join(", ")
        );
    }
}
