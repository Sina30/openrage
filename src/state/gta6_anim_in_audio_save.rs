//! Save of a GTA VI animation opened from an audio stream writes it back as that stream's
//! animation chunk, a step of the container's history, as Save of an archive entry writes it back
//! into its archive; saving the container writes it to disk.

use super::{AppState, DocPayload, DocumentId, DocumentKind};

impl AppState {
    /// Save of an animation an audio stream stores, into its open container; `None` for any other.
    pub(super) fn save_animation_into_its_audio(&mut self, doc: DocumentId) -> Option<bool> {
        let document = self.document(doc)?;
        if document.kind != DocumentKind::Gta6Anim {
            return None;
        }
        let path = document.path.clone()?;
        let id = u32::from_str_radix(path.file_stem()?.to_str()?.strip_prefix("0x")?, 16).ok()?;
        let container = path.parent()?;
        let named = |p: &std::path::Path, ext: &str| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
        };
        if !named(path.as_path(), "anim") || !named(container, "awc") {
            return None;
        }
        let holder = (self.documents.iter())
            .find(|d| {
                d.path.as_deref() == Some(container) && matches!(d.payload, DocPayload::Awc(_))
            })
            .map(|d| d.id);
        let Some(holder) = holder else {
            self.show_error_toast(tr!(
                "Cannot save: open the audio container this animation came from, which it is written back into."
            ));
            return Some(false);
        };
        let stream = (self.awc_file(holder).ok())
            .and_then(|file| file.streams.iter().position(|s| s.id == id));
        let Some(stream) = stream else {
            self.show_error_toast(tr!("This stream stores no animation."));
            return Some(false);
        };
        match self.awc_put_animation(holder, stream) {
            Ok(()) => {
                if let Some(document) = self.document_mut(doc) {
                    document.mark_saved();
                }
                self.show_info_toast(tr!(
                    "Written back into its audio container; save the container to write it to disk."
                ));
                Some(true)
            }
            Err(e) => {
                self.show_error_toast(tr!("Cannot save: {reason}", reason = e));
                Some(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use or_io::awc::AwcChunkKind;
    use or_io::container::rpf::write_rpf9_resource;

    use super::super::an_awc_stream_opens_its_animation::{adat, stored_set};
    use super::super::opening_an_rpf9_archive::{said, settle};
    use super::super::{AppState, DocPayload, DocumentId, ToastType};

    fn chunk(state: &mut AppState, doc: DocumentId) -> Vec<u8> {
        let file = state.awc_file(doc).expect("its container");
        let chunk = file.streams[0].chunk(AwcChunkKind::Animation);
        chunk.expect("a chunk").payload.clone()
    }

    #[test]
    fn a_save_of_an_edited_stream_animation_writes_it_back_into_its_open_container() {
        let _serial = crate::ui::theme::serial();
        let awc = crate::audio::AwcDocument::open(&adat(&stored_set())).expect("an ADAT container");
        let mut state = AppState::new();
        let container = state.insert_document(
            "C:/sounds/voice.awc".into(),
            DocPayload::Awc(awc),
            None,
            None,
        );
        let stored = chunk(&mut state, container);
        state
            .awc_open_animation(container, 0)
            .expect("the stream stores one");
        settle(&mut state);
        let anim = (state.active_document)
            .filter(|&d| d != container)
            .expect("a new document");

        // Edited, here by a section that grows, a plain Save writes it into the stream.
        let Some(DocPayload::Gta6Anim(resource)) = state.document_mut(anim).map(|d| &mut d.payload)
        else {
            panic!("a GTA VI animation");
        };
        let held = Arc::make_mut(resource);
        let mut grown = held.sections.as_deref().expect("its section").clone();
        grown.virtual_section.extend([0u8; 32]);
        held.sections = Some(Arc::new(grown.clone()));
        held.edited = true;
        assert!(state.save_active_document());
        let written = write_rpf9_resource(&grown).expect("writes");
        assert_eq!(chunk(&mut state, container), written);
        assert!(state.document(anim).is_some_and(|d| !d.dirty));

        // The container's history holds the step.
        state.active_document = Some(container);
        assert!(state.undo().is_some());
        assert_eq!(chunk(&mut state, container), stored);

        // With its container closed, the save is refused by name and writes nothing anywhere.
        state.documents.retain(|d| d.id != container);
        state.active_document = Some(anim);
        state.toasts.clear();
        assert!(!state.save_active_document());
        assert!(said(&state, ToastType::Error, &["audio container"]));
    }
}
