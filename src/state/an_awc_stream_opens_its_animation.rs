//! A GTA VI animation resource an audio stream stores beside its audio opens in the app, as
//! `rpf scan-open` reads it.

use super::opening_an_rpf9_archive::settle;
use super::*;

/// A lone animation set named `dict_a` with no animation, stored whole in an RSC8 of version 65.
pub(super) fn stored_set() -> Vec<u8> {
    let mut root = vec![0u8; 0xC0];
    root[0x38..0x3E].copy_from_slice(b"dict_a");
    root[0x47] = 0x80 | 6;
    root[0x5F] = 1;
    root[0x68..0x6C].copy_from_slice(&or_io::gta6::hash::name_hash32(b"dict_a").to_le_bytes());
    root[0x08..0x10].copy_from_slice(&0x5000_00B0u64.to_le_bytes());
    root[0xB8] = 1;
    let mut stored = b"RSC8".to_vec();
    for word in [0x1F41u32, root.len() as u32 | 4, 1] {
        stored.extend(word.to_le_bytes());
    }
    stored.extend_from_slice(&root);
    stored
}

/// An `ADAT` container of one stream, id 0x123, whose one chunk is `animation`.
pub(super) fn adat(animation: &[u8]) -> Vec<u8> {
    let tables = 16 + 4 + 8;
    let mut bytes = b"ADAT".to_vec();
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(0xFF02u16.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes.extend((tables as u32).to_le_bytes());
    bytes.extend((1u32 << 29 | 0x123).to_le_bytes());
    let entry = 0x5Cu64 << 56 | (animation.len() as u64) << 28 | tables as u64;
    bytes.extend(entry.to_le_bytes());
    bytes.extend_from_slice(animation);
    bytes
}

#[test]
fn a_stream_s_animation_opens_as_a_gta_vi_animation() {
    let awc = crate::audio::AwcDocument::open(&adat(&stored_set())).expect("an ADAT container");
    let mut state = AppState::new();
    let doc = state.insert_document(
        "C:/sounds/voice.awc".into(),
        DocPayload::Awc(awc),
        None,
        None,
    );
    let file = state.awc_file(doc).expect("its container");
    assert!(crate::audio::stores_gta6_animation(&file.streams[0]));
    state
        .awc_open_animation(doc, 0)
        .expect("the stream stores one");
    settle(&mut state);
    let opened = state
        .active_document
        .filter(|&d| d != doc)
        .expect("a new document");
    let d = state.document(opened).expect("open");
    assert_eq!(d.kind, DocumentKind::Gta6Anim);
    assert_eq!(
        d.path.as_deref(),
        Some(std::path::Path::new("C:/sounds/voice.awc/0x00000123.anim"))
    );
    let DocPayload::Gta6Anim(resource) = &d.payload else {
        unreachable!()
    };
    assert_eq!(resource.name, "dict_a");

    // Asked again, the one open is shown.
    state.set_active_document(doc);
    state
        .awc_open_animation(doc, 0)
        .expect("the stream stores one");
    settle(&mut state);
    assert_eq!(state.active_document, Some(opened));
    assert_eq!(state.documents.len(), 2);
}

#[test]
fn a_stream_with_no_animation_says_so() {
    let mut chunk = stored_set();
    chunk[0..4].copy_from_slice(b"RSC7");
    let awc = crate::audio::AwcDocument::open(&adat(&chunk)).expect("an ADAT container");
    let mut state = AppState::new();
    let doc = state.insert_document(
        "C:/sounds/voice.awc".into(),
        DocPayload::Awc(awc),
        None,
        None,
    );
    let file = state.awc_file(doc).expect("its container");
    assert!(
        !crate::audio::stores_gta6_animation(&file.streams[0]),
        "a GTA V clip dictionary is no GTA VI animation"
    );
    state.awc_open_animation(doc, 0).expect("a chunk to read");
    settle(&mut state);
    assert_eq!(state.documents.len(), 1);
    assert!(
        state
            .error_dialog_message
            .contains("opens only from a scan"),
        "{}",
        state.error_dialog_message
    );
    assert!(state.awc_open_animation(doc, 5).is_err());
}
