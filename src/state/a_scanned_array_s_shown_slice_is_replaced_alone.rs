use or_io::authoring::{FormatChoice, TextureBuildOptions};
use or_io::codec::texture::RgbaImage;
use or_io::gta6::headerless::Dictionary;

use super::*;
use crate::state::gta6_textures_resolve_and_save::dictionary_texture;
use crate::state::textures::TextureSource as Source;
use crate::state::ttpf_document::{scanned_textures, show_scanned_slice};

/// What the texture `name` of the document `doc` stores, every slice one after another.
fn stored(state: &AppState, doc: DocumentId, name: &str) -> Vec<u8> {
    let payload = &state.document(doc).expect("open").payload;
    let dictionary = crate::state::textures::legacy_dictionary(payload).expect("a dictionary");
    let texture = dictionary.get(name).expect("its texture");
    texture.backing_store.clone()
}

#[test]
fn a_scanned_array_s_shown_slice_is_replaced_alone_and_undone_whole() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    // An archive whose head is not on disk yet is all zeros, and a body of it opens read-only.
    let archive = dir.path().join("data.rpf");
    std::fs::write(&archive, [0u8; 0x1000]).expect("written");
    let path = archive.join("0x10.ytd");
    let entries = vec![dictionary_texture("zq_layers", 3, true)];
    let payload = scanned_textures(&path, Dictionary { entries }).expect("a texture dictionary");
    let mut state = AppState::new();
    let doc = state.insert_document(path, payload, None, None);
    let before = stored(&state, doc, "zq_layers");
    assert_eq!(before.len(), 3 * 8, "three 4x4 BC1 slices");

    show_scanned_slice(state.document(doc).expect("open"), "ZQ_LAYERS", 1);
    let blue = RgbaImage {
        width: 4,
        height: 4,
        data: [0, 0, 0xFF, 0xFF].repeat(16),
        is_srgb: false,
    };
    let options = TextureBuildOptions::default();
    let choice = FormatChoice::Automatic;
    (state.import_texture_over(doc, Source::Document, "zq_layers", &blue, choice, &options))
        .expect("the slice shown takes an image of its size");
    let after = stored(&state, doc, "zq_layers");
    assert_eq!(after.len(), before.len());
    assert_ne!(after[8..16], before[8..16], "slice 1 is replaced");
    assert_eq!(after[..8], before[..8], "slice 0 is left alone");
    assert_eq!(after[16..], before[16..], "slice 2 is left alone");
    let edited = state.document(doc).expect("open");
    assert!(edited_save(edited).is_some(), "its Save As writes it");

    assert_eq!(state.undo().as_deref(), Some("Replace zq_layers"));
    assert_eq!(stored(&state, doc, "zq_layers"), before);
}

/// A scanned volume three deep of `mips` levels of 4x4 BC1, opened as a document of `state`.
fn volume(state: &mut AppState, dir: &std::path::Path, mips: u8) -> DocumentId {
    let archive = dir.join("data.rpf");
    std::fs::write(&archive, [0u8; 0x1000]).expect("written");
    let path = archive.join(format!("0x{mips}0.ytd"));
    let (code, entry) = dictionary_texture("zq_volume", 3, true);
    let entry = entry.map(|e| match e {
        or_io::gta6::headerless::Entry::Texture(mut t) => {
            t.dimension = 3;
            t.mip_levels = mips;
            // Each slice holds a plane of every level, one BC1 block each.
            if let Ok(read) = &mut t.texture {
                let blocks = 3 * usize::from(mips);
                read.levels = mips.into();
                read.pixels = (0..blocks).flat_map(|b| [b as u8 + 1; 8]).collect();
            }
            or_io::gta6::headerless::Entry::Texture(t)
        }
        other => other,
    });
    let entries = vec![(code, entry)];
    let payload = scanned_textures(&path, Dictionary { entries }).expect("a dictionary");
    state.insert_document(path, payload, None, None)
}

#[test]
fn a_scanned_volume_s_shown_plane_is_replaced_and_a_smaller_level_s_plane_in_every_slice_showing_it(
) {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let blue = RgbaImage {
        width: 4,
        height: 4,
        data: [0, 0, 0xFF, 0xFF].repeat(16),
        is_srgb: false,
    };
    let options = TextureBuildOptions::default();

    let doc = volume(&mut state, dir.path(), 1);
    let before = stored(&state, doc, "zq_volume");
    show_scanned_slice(state.document(doc).expect("open"), "zq_volume", 2);
    let choice = FormatChoice::Automatic;
    (state.import_texture_over(doc, Source::Document, "zq_volume", &blue, choice, &options))
        .expect("a one-level volume's plane takes an image of its size");
    let after = stored(&state, doc, "zq_volume");
    assert_ne!(after[16..], before[16..], "plane 2 is replaced");
    assert_eq!(after[..16], before[..16], "planes 0 and 1 are left alone");

    // Two levels: each slice a block of level 0, then the one plane of level 1 every slice shows.
    let doc = volume(&mut state, dir.path(), 2);
    let before = stored(&state, doc, "zq_volume");
    show_scanned_slice(state.document(doc).expect("open"), "zq_volume", 1);
    let choice = FormatChoice::Automatic;
    (state.import_texture_over(doc, Source::Document, "zq_volume", &blue, choice, &options))
        .expect("a volume of two levels takes an image of its size");
    let after = stored(&state, doc, "zq_volume");
    assert_eq!(after.len(), before.len());
    assert_ne!(
        after[16..24],
        before[16..24],
        "plane 1 of level 0 is replaced"
    );
    assert_eq!(after[..8], before[..8], "plane 0 of level 0 is left alone");
    assert_eq!(
        after[32..40],
        before[32..40],
        "plane 2 of level 0 is left alone"
    );
    for slice in [0, 2] {
        let at = 16 * slice + 8;
        assert_eq!(
            after[at..at + 8],
            after[24..32],
            "slice {slice} shows level 1 anew"
        );
    }
    assert_eq!(state.undo().as_deref(), Some("Replace zq_volume"));
    assert_eq!(stored(&state, doc, "zq_volume"), before);
}
