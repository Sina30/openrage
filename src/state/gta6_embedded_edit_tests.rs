use std::io::Cursor;
use std::time::{Duration, Instant};

use or_io::codec::texture::{save_png, RgbaImage};
use or_io::container::rpf::{read_rpf9_body, write_rpf9_resource, Rpf9Form, Rpf9Sections};
use or_io::gta6::headerless::{read_drawable, read_drawable_head};

use super::*;
use crate::state::gta6_edit::tests::drawable_body;
use crate::state::opening_an_rpf9_archive::settle;
use crate::state::textures::Texture;

const V: u64 = 0x5000_0000;
/// Where the embedded texture's object, its key record, its dictionary, the dictionary's hashes
/// and its list of textures lie.
const TEXTURE: usize = 0x400;
const RECORD: usize = 0x480;
const DICTIONARY: usize = 0x4A0;
const HASHES: usize = 0x4E0;
const LIST: usize = 0x4F0;
/// A 128×64 BC1 texture of two levels: 32 × 16 blocks of 8 bytes, then 16 × 8.
const PIXELS: usize = (32 * 16 + 16 * 8) * 8;

fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
    v[at..at + bytes.len()].copy_from_slice(bytes);
}

fn ptr(v: &mut [u8], at: usize, to: usize) {
    put(v, at, &(V + to as u64).to_le_bytes());
}

/// The test drawable with its shader group embedding, under the key its one slot binds, a
/// 128×64 BC1 texture of two levels over the physical section's first 0x2000 bytes.
fn embedding_body() -> (Vec<u8>, u64) {
    let mut v = drawable_body();
    let read = read_drawable(&v, Some(&[])).expect("the drawable reads");
    let key = read.shaders[0].texture_keys[0].expect("its slot binds a key");
    v.resize(0x600, 0);
    // The block map states one physical block.
    v[0x3E9] = 1;
    put(&mut v, TEXTURE + 0x08, &(0x2000u32 / 8).to_le_bytes());
    put(&mut v, TEXTURE + 0x0C, &8u16.to_le_bytes());
    put(&mut v, TEXTURE + 0x0E, &8u16.to_le_bytes());
    put(&mut v, TEXTURE + 0x10, &0x980u32.to_le_bytes());
    put(&mut v, TEXTURE + 0x18, &0x6000_0000u64.to_le_bytes());
    ptr(&mut v, TEXTURE + 0x28, TEXTURE + 0x50);
    ptr(&mut v, TEXTURE + 0x30, RECORD);
    for (i, side) in [128u16, 64, 1].iter().enumerate() {
        put(&mut v, TEXTURE + 0x38 + 2 * i, &side.to_le_bytes());
    }
    v[TEXTURE + 0x3E] = 1;
    v[TEXTURE + 0x3F] = 71;
    v[TEXTURE + 0x40] = 5;
    v[TEXTURE + 0x42] = 2;
    put(&mut v, TEXTURE + 0x46, &1u16.to_le_bytes());
    put(&mut v, TEXTURE + 0x4C, &2u32.to_le_bytes());
    put(&mut v, RECORD + 0x08, &key.to_le_bytes());
    put(&mut v, HASHES, &0x1B04_679Fu32.to_le_bytes());
    ptr(&mut v, LIST, TEXTURE);
    put(&mut v, DICTIONARY + 0x18, &1u32.to_le_bytes());
    ptr(&mut v, DICTIONARY + 0x20, HASHES);
    put(&mut v, DICTIONARY + 0x28, &[1, 0, 1, 0]);
    ptr(&mut v, DICTIONARY + 0x30, LIST);
    put(&mut v, DICTIONARY + 0x38, &[1, 0, 1, 0]);
    ptr(&mut v, 0x100 + 0x08, DICTIONARY);
    (v, key)
}

fn finished(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.is_running() {
        state.poll_extract_job();
        assert!(Instant::now() < deadline, "the save never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The embedded texture's pixels the head of `sections` reads.
fn embedded_pixels(sections: &Rpf9Sections) -> Vec<u8> {
    let (v, p) = (
        &sections.virtual_section,
        Some(sections.physical.as_slice()),
    );
    let head = read_drawable_head(v, p, false).expect("the head reads");
    let [Ok(t)] = head.textures.as_slice() else {
        panic!("one texture, not {:?}", head.textures);
    };
    t.texture
        .texture
        .as_ref()
        .expect("its pixels read")
        .pixels
        .clone()
}

#[test]
fn an_embedded_texture_is_replaced_drawn_undone_and_saved_into_its_own_tiles() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("embedded.rsc");
    let (virtual_section, key) = embedding_body();
    let sections = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section,
        physical: vec![0x5A; 0x2000],
    };
    std::fs::write(&path, write_rpf9_resource(&sections).expect("writes")).expect("written");
    let mut state = AppState::new();
    state.load_file(path.clone());
    settle(&mut state);
    let doc =
        (state.active_document).unwrap_or_else(|| panic!("opened: {}", state.error_dialog_message));
    let name = texture_name(key);
    assert!(embeds(&state, doc, &name));
    assert!(!embeds(&state, doc, "hash_0000000000000001"));
    assert_eq!(state.bound_texture_pack(doc, &name), Ok(doc));

    // A solid image of the texture's own size encodes to one block repeated over both levels.
    let mut image = RgbaImage::new(128, 64);
    for y in 0..64 {
        for x in 0..128 {
            image.set_pixel(x, y, [200, 40, 10, 255]);
        }
    }
    let png = dir.path().join("solid.png");
    save_png(&png, &image).expect("the image is written");
    let (auto, options) = (FormatChoice::Automatic, TextureBuildOptions::default());
    let edit = state.replace_embedded_texture(doc, &name, &png, auto, &options);
    edit.expect("it embeds that texture").expect("replaced");
    let new = replaced(&path, key).expect("the pixels the document holds");
    assert_eq!(new.len(), PIXELS);
    assert!(new.chunks(8).all(|block| block == &new[..8]), "one block");
    assert!(new != vec![0x5A; PIXELS]);

    // The view draws the pixels the document holds.
    let drawn = |state: &AppState| {
        let payload = &state.document(doc).expect("open").payload;
        match super::super::gta6_head::texture_named(payload, None, &name) {
            Some(Texture::Owned(t)) => t.backing_store.clone(),
            _ => panic!("the embedded texture is drawn"),
        }
    };
    assert_eq!(drawn(&state), new);
    state.active_document = Some(doc);
    assert!(state.undo().is_some());
    assert_eq!(
        replaced(&path, key),
        None,
        "undone, the body's pixels are drawn"
    );
    assert_eq!(drawn(&state), vec![0x5A; PIXELS]);
    assert!(state.redo().is_some());
    assert_eq!(drawn(&state), new);

    // Saved over its own file: the texture reads back as replaced, in the same bytes' room.
    assert!(state.save_active_document());
    finished(&mut state);
    let bytes = std::fs::read(&path).expect("the saved file");
    let (find, read) = read_rpf9_body(Cursor::new(bytes), 0, u64::MAX).expect("it reads");
    let saved = Rpf9Sections::of(&find, &read).expect("a resource");
    assert_eq!(embedded_pixels(&saved), new);
    assert_eq!(saved.virtual_section, sections.virtual_section);

    // An image of another size is refused and the document is as it was.
    let small = dir.path().join("small.png");
    save_png(&small, &RgbaImage::new(64, 64)).expect("the image is written");
    let refused = state.replace_embedded_texture(doc, &name, &small, auto, &options);
    assert!(refused.expect("it embeds that texture").is_err());
    assert_eq!(replaced(&path, key), Some(new));

    // The body opened anew, as after a close that did not save, holds none of them.
    super::super::rpf9_scan::read_body(&path, &path, 0).expect("it opens again");
    assert_eq!(replaced(&path, key), None);
}
