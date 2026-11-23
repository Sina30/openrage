use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::authoring::{FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{decode_level, GrcTextureFormat, RgbaImage};
use or_io::container::rpf::{read_rpf9_body, write_rpf9_resource, Rpf9Sections};
use or_io::gta6::headerless::{
    read_dictionary, read_dictionary_model, write_dictionary, DictionaryModel, Entry, ModelTexture,
};

use crate::state::opening_an_rpf9_archive::settle;
use crate::state::texture_edit::TextureEdit;
use crate::state::textures::{legacy_dictionary, TextureSource};
use crate::state::{AppState, DocumentId};

fn finished(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.is_running() {
        state.poll_extract_job();
        assert!(Instant::now() < deadline, "the save never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The sections a read at offset 0 of the file at `path` returns.
fn read_back(path: &Path) -> Rpf9Sections {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (find, read) = read_rpf9_body(Cursor::new(bytes), 0, u64::MAX).expect("it reads");
    Rpf9Sections::of(&find, &read).expect("a resource")
}

/// A loose dictionary of one 64x64 BC1 texture of one level, "zq_paint", its pixels a pattern.
fn dictionary_file(dir: &Path) -> (PathBuf, Rpf9Sections) {
    let pixels: Vec<u8> = (0..16 * 16 * 8)
        .map(|i: usize| (i * 13 + i / 7) as u8)
        .collect();
    let paint = ModelTexture::tiled("zq_paint", 64, 64, 71, &[pixels]).expect("it tiles");
    let model = DictionaryModel {
        textures: vec![paint],
        capacity: 1,
    };
    let sections = write_dictionary(&model).expect("planned");
    let path = dir.join("paint.rsc");
    std::fs::write(&path, write_rpf9_resource(&sections).expect("writes")).expect("written");
    let stored = read_back(&path);
    (path, stored)
}

fn image(side: u32, colour: [u8; 4]) -> RgbaImage {
    RgbaImage {
        width: side,
        height: side,
        data: colour.repeat((side * side) as usize),
        is_srgb: false,
    }
}

/// Each texture the document `doc` shows: its name, size, mips and stored pixels.
fn textures(state: &AppState, doc: DocumentId) -> Vec<(String, u16, u16, u8, Vec<u8>)> {
    let payload = &state.document(doc).expect("open").payload;
    let dictionary = legacy_dictionary(payload).expect("a dictionary");
    (dictionary.entries.iter().flatten())
        .map(|t| {
            let pixels = t.backing_store.clone();
            (t.name.clone(), t.width, t.height, t.mip_count, pixels)
        })
        .collect()
}

#[test]
fn a_scanned_dictionary_texture_resized_re_encoded_and_added_saves_planned_anew_and_undoes() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (path, stored) = dictionary_file(dir.path());
    let mut state = AppState::new();
    state.load_file(path);
    settle(&mut state);
    let doc = (state.active_document).unwrap_or_else(|| panic!("{}", state.error_dialog_message));
    let opened = textures(&state, doc);
    let start = state.document(doc).expect("open").history.at();
    let (at, options) = (TextureSource::Document, TextureBuildOptions::default());
    let auto = FormatChoice::Automatic;

    // An image of another size: the texture keeps its format and takes the image's size.
    let red = image(32, [255, 0, 0, 255]);
    let edit = (state.import_texture_over(doc, at, "zq_paint", &red, auto, &options))
        .expect("an image of another size");
    assert!(
        matches!(
            edit,
            TextureEdit::Imported {
                format: GrcTextureFormat::DXT1,
                ..
            }
        ),
        "{edit:?}"
    );
    let dxt5 = FormatChoice::Fixed(GrcTextureFormat::DXT5);
    let edit = (state.reencode_texture_in_place(doc, at, "zq_paint", dxt5, &options))
        .expect("another format");
    assert!(
        matches!(
            edit,
            TextureEdit::Reencoded {
                format: GrcTextureFormat::DXT5,
                ..
            }
        ),
        "{edit:?}"
    );
    let blue = image(16, [0, 0, 255, 255]);
    assert!(
        (state.import_texture_as_new(doc, "zq_\u{e9}", &blue, auto, &options)).is_err(),
        "a stored name is ASCII"
    );
    let dxt3 = FormatChoice::Fixed(GrcTextureFormat::DXT3);
    let refused = (state.import_texture_as_new(doc, "zq_dxt3", &blue, dxt3, &options))
        .expect_err("no sRGB DXT3 is held, so the add is refused when it is made");
    assert!(refused.contains("DXT3"), "{refused}");
    (state.import_texture_as_new(doc, "zq_added", &blue, auto, &options)).expect("a new texture");
    let edited = textures(&state, doc);
    assert_eq!(edited.len(), 2);

    let saved = dir.path().join("saved.rsc");
    state.active_document = Some(doc);
    assert!(state.save_active_document_as(saved.clone()));
    finished(&mut state);
    let said: Vec<_> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(saved.exists(), "{said:?} {}", state.error_dialog_message);
    let sections = read_back(&saved);
    let read =
        read_dictionary(&sections.virtual_section, Some(&sections.physical)).expect("it reads");
    for (name, width, height, mips, pixels) in &edited {
        let t = (read.entries.iter())
            .find_map(|(_, entry)| match entry {
                Ok(Entry::Texture(t)) if t.name == *name => Some(t),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name} is saved"));
        assert_eq!((t.width, t.height, t.mip_levels), (*width, *height, *mips));
        let back = &t.texture.as_ref().expect("its pixels read").pixels;
        // The document's store pads its levels; the dictionary holds them exactly.
        assert!(pixels.starts_with(back), "{name} reads back as edited");
        assert!(pixels[back.len()..].iter().all(|&b| b == 0), "{name}");
    }
    let formats: Vec<u8> = (read.entries.iter())
        .filter_map(|(_, entry)| match entry {
            Ok(Entry::Texture(t)) => Some(t.format),
            _ => None,
        })
        .collect();
    // The options ask for sRGB, as the editor's do by default: the linear texture reshaped stays
    // linear, and only the added one takes the options' sRGB.
    assert!(options.srgb, "the default options are sRGB");
    let number = |format: GrcTextureFormat, srgb: bool| {
        u8::try_from(format.to_dxgi(srgb).to_raw()).expect("a DXGI number under 256")
    };
    let (kept, added) = (
        number(GrcTextureFormat::DXT5, false),
        number(GrcTextureFormat::DXT1, true),
    );
    assert!(
        formats.len() == 2 && formats.contains(&kept) && formats.contains(&added),
        "{formats:?}"
    );
    let model = (read_dictionary_model(&sections.virtual_section, &sections.physical))
        .expect("its model reads");
    assert!(
        write_dictionary(&model).expect("planned again") == sections,
        "the saved sections are the plan of their textures"
    );
    let twice = dir.path().join("twice.rsc");
    assert!(state.save_active_document_as(twice.clone()));
    finished(&mut state);
    assert!(
        std::fs::read(&twice).ok() == std::fs::read(&saved).ok(),
        "a second save writes the same bytes"
    );

    state.go_to_step(doc, start);
    assert!(
        textures(&state, doc) == opened,
        "undone, it is as it opened"
    );
    let copy = dir.path().join("copy.rsc");
    assert!(state.save_active_document_as(copy.clone()));
    finished(&mut state);
    assert!(
        read_back(&copy) == stored,
        "undone, Save As copies the body"
    );
}

#[test]
fn a_scanned_array_resized_from_one_image_takes_it_in_its_shown_slice_and_saves_each_slice() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    // Two 16x16 BC1 slices of one level, each a pattern of its own.
    let slice = |seed: usize| -> Vec<Vec<u8>> {
        vec![(0..4 * 4 * 8)
            .map(|i: usize| (i * 7 + seed) as u8)
            .collect()]
    };
    let (first, second) = (slice(1), slice(90));
    let slices = [first.as_slice(), second.as_slice()];
    let array = ModelTexture::tiled_slices("zq_array", 16, 16, 1, 71, &slices).expect("it tiles");
    let model = DictionaryModel {
        textures: vec![array],
        capacity: 1,
    };
    let sections = write_dictionary(&model).expect("planned");
    let path = dir.path().join("array.rsc");
    std::fs::write(&path, write_rpf9_resource(&sections).expect("writes")).expect("written");
    let mut state = AppState::new();
    state.load_file(path);
    settle(&mut state);
    let doc = (state.active_document).unwrap_or_else(|| panic!("{}", state.error_dialog_message));
    let opened = textures(&state, doc);
    let start = state.document(doc).expect("open").history.at();
    let options = TextureBuildOptions {
        mip_levels: Some(2),
        ..TextureBuildOptions::default()
    };
    let (at, auto) = (TextureSource::Document, FormatChoice::Automatic);
    let red = image(32, [255, 0, 0, 255]);
    (state.import_texture_over(doc, at, "zq_array", &red, auto, &options))
        .expect("an array takes another size");
    let edited = textures(&state, doc);
    assert_eq!((edited[0].1, edited[0].2, edited[0].3), (32, 32, 2));

    let saved = dir.path().join("saved.rsc");
    state.active_document = Some(doc);
    assert!(state.save_active_document_as(saved.clone()));
    finished(&mut state);
    let sections = read_back(&saved);
    let read =
        read_dictionary(&sections.virtual_section, Some(&sections.physical)).expect("it reads");
    let t = (read.entries.iter())
        .find_map(|(_, entry)| match entry {
            Ok(Entry::Texture(t)) => Some(t),
            _ => None,
        })
        .expect("the array is saved");
    assert_eq!((t.width, t.height, t.depth, t.mip_levels), (32, 32, 2, 2));
    let back = t.texture.as_ref().expect("its pixels read");
    assert!(
        back.pixels == edited[0].4,
        "each slice reads back as edited"
    );
    let shown = decode_level(&back.face(0).expect("slice 0"), 0).expect("decodes");
    assert!(shown.data.chunks(4).all(|pixel| pixel == [255, 0, 0, 255]));

    state.go_to_step(doc, start);
    assert!(
        textures(&state, doc) == opened,
        "undone, it is as it opened"
    );
}
