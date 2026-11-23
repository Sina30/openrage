//! A GTA VI animation an audio stream stores opens with its section, so once edited it is written
//! back as the stream's animation chunk, a step of the container's history.

use std::sync::Arc;

use or_io::awc::AwcChunkKind;
use or_io::container::rpf::{write_rpf9_resource, Rpf9Form};

use super::an_awc_stream_opens_its_animation::{adat, stored_set};
use super::opening_an_rpf9_archive::settle;
use super::*;

fn chunk(state: &mut AppState, doc: DocumentId) -> Vec<u8> {
    let file = state.awc_file(doc).expect("its container");
    let chunk = file.streams[0]
        .chunk(AwcChunkKind::Animation)
        .expect("a chunk");
    chunk.payload.clone()
}

#[test]
fn an_edited_stream_animation_is_written_back_as_its_chunk_and_undoes() {
    let _serial = crate::ui::theme::serial();
    let awc = crate::audio::AwcDocument::open(&adat(&stored_set())).expect("an ADAT container");
    let mut state = AppState::new();
    let doc = state.insert_document(
        "C:/sounds/voice.awc".into(),
        DocPayload::Awc(awc),
        None,
        None,
    );
    let stored = chunk(&mut state, doc);
    state
        .awc_open_animation(doc, 0)
        .expect("the stream stores one");
    settle(&mut state);
    let anim = (state.active_document)
        .filter(|&d| d != doc)
        .expect("a new document");
    let Some(DocPayload::Gta6Anim(resource)) = state.document(anim).map(|d| &d.payload) else {
        panic!("a GTA VI animation");
    };
    let sections = resource.sections.clone().expect("its section is kept");
    assert_eq!(sections.form, Rpf9Form::StoredRsc8 { config: 0 });
    assert_eq!(sections.virtual_section, stored[16..]);
    assert!(!resource.edited);

    // Nothing edited: nothing is written back.
    assert!(state.awc_put_animation(doc, 0).is_err());
    assert_eq!(chunk(&mut state, doc), stored);

    // Edited, here by a section that grows, it is written back as a stored resource.
    let Some(DocPayload::Gta6Anim(resource)) = state.document_mut(anim).map(|d| &mut d.payload)
    else {
        panic!("a GTA VI animation");
    };
    let held = Arc::make_mut(resource);
    let mut grown = (*sections).clone();
    grown.virtual_section.extend([0u8; 32]);
    held.sections = Some(Arc::new(grown.clone()));
    held.edited = true;
    state.awc_put_animation(doc, 0).expect("written back");
    let written = write_rpf9_resource(&grown).expect("writes");
    assert_eq!(chunk(&mut state, doc), written);
    assert_eq!(written.len(), stored.len() + 32);

    state.active_document = Some(doc);
    assert!(state.undo().is_some());
    assert_eq!(chunk(&mut state, doc), stored);
}
