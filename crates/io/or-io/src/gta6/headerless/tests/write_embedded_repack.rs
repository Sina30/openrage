use super::head::embedded;
use super::*;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};

/// The word a key record states for [`TEXTURE_KEY`] once embedded.
const WORD: u32 = 0x2468_ACE1;

fn mapped(img: &Image, phys: &[u8]) -> Rpf9Sections {
    Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: img.bytes.clone(),
        physical: phys.to_vec(),
    }
}

/// Made-up bytes, `n` of them, from `seed`.
fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(29) ^ seed).collect()
}

/// A BC1 texture of one level, `width` by `height`, its blocks a pattern.
fn bc1(width: u16, height: u16, seed: u8) -> ModelTexture {
    let blocks = usize::from(width / 4) * usize::from(height / 4) * 8;
    ModelTexture::tiled("hash_embedded", width, height, 71, &[pattern(blocks, seed)])
        .expect("it tiles")
}

/// The width, height, mips, dictionary hash and pixels of the embedded texture of `key`.
fn texture(s: &Rpf9Sections, key: u64) -> Option<(u16, u16, u8, u32, Vec<u8>)> {
    let head =
        read_drawable_head(&s.virtual_section, Some(&s.physical), false).expect("the head reads");
    let t = head.textures.into_iter().flatten().find(|t| t.key == key)?;
    let (d, hash) = (t.texture, t.hash);
    let shape = (d.width, d.height, d.mip_levels);
    let pixels = d.texture.expect("its pixels read").pixels;
    Some((shape.0, shape.1, shape.2, hash, pixels))
}

/// The built drawable's geometry, read where its data now lies.
fn assert_geometry_reads(s: &Rpf9Sections) {
    let d = read_drawable(&s.virtual_section, Some(&s.physical)).expect("the drawable reads");
    let g = d.levels[0].as_ref().expect("level 0")[0].geometries[0]
        .as_ref()
        .expect("the geometry reads");
    assert_eq!(g.triangles, [[0, 1, 2]]);
    assert_eq!(
        g.positions.as_ref().expect("positions"),
        &TRIANGLE_AT.to_vec()
    );
}

/// The block map the virtual section points at from 0x08.
fn map_of(s: &Rpf9Sections) -> usize {
    u64_at(&s.virtual_section, 0x08) as usize & 0x0FFF_FFFF
}

#[test]
fn an_embedded_texture_takes_another_size_and_mips_and_the_geometry_moves_with_the_pack() {
    let (img, phys, _) = embedded(TEXTURE_KEY);
    let mut s = mapped(&img, &phys);
    // The texture's 0x2000 bytes lead the section; the geometry's data follows them.
    let rest = phys.len() - 0x2000;
    let smaller = bc1(64, 64, 3);
    reshape_embedded_texture(&mut s, false, TEXTURE_KEY, &smaller).expect("written");
    let (width, height, mips, _, pixels) = texture(&s, TEXTURE_KEY).expect("still embedded");
    assert_eq!((width, height, mips), (64, 64, 1));
    assert_eq!(pixels, pattern(16 * 16 * 8, 3));
    assert_eq!(s.physical.len(), smaller.payload.len() + rest);
    assert_geometry_reads(&s);
    assert_eq!(s.virtual_section.len(), img.bytes.len());

    // Written again from the same texture, the canonical packing is a fixed point.
    let again = s.clone();
    reshape_embedded_texture(&mut s, false, TEXTURE_KEY, &smaller).expect("written");
    assert_eq!(s, again);
}

#[test]
fn an_unembedded_texture_leaves_the_empty_form_and_embedding_it_again_reads_back() {
    let (img, phys, dictionary) = embedded(TEXTURE_KEY);
    let mut s = mapped(&img, &phys);
    let rest = phys.len() - 0x2000;
    unembed_texture(&mut s, false, TEXTURE_KEY).expect("unembedded");
    assert!(texture(&s, TEXTURE_KEY).is_none());
    let words: Vec<u64> = (0..8)
        .map(|w| u64_at(&s.virtual_section, dictionary + 8 * w))
        .collect();
    assert_eq!(words, [0, 0, 0, 1, 0, 0, 0, 0]);
    assert_eq!(s.physical.len(), rest, "only the geometry's data is left");
    assert_geometry_reads(&s);

    let len = s.virtual_section.len();
    let texture_of = bc1(64, 64, 9);
    embed_texture(&mut s, false, TEXTURE_KEY, WORD, &texture_of).expect("embedded");
    let (width, height, _, hash, pixels) = texture(&s, TEXTURE_KEY).expect("embedded");
    assert_eq!((width, height, hash), (64, 64, WORD));
    assert_eq!(pixels, pattern(16 * 16 * 8, 9));
    // The object, the key record and the two arrays of one, in the group's own dictionary.
    assert_eq!(s.virtual_section.len(), len + 0x70 + 0x20 + 0x10 + 0x10);
    assert_eq!(s.physical.len(), texture_of.payload.len() + rest);
    assert_eq!(key_word(&s.virtual_section, TEXTURE_KEY), Some(WORD));
    assert_eq!(key_word(&s.virtual_section, TEXTURE_KEY ^ 1), None);
    assert_geometry_reads(&s);
}

#[test]
fn a_drawable_with_no_dictionary_takes_one_and_unembedding_returns_its_own_physical_section() {
    let (img, phys, _) = drawable([1.0, 1.0, 0.0]);
    let before = mapped(&img, &phys);
    let mut s = before.clone();
    embed_texture(&mut s, false, TEXTURE_KEY, WORD, &bc1(64, 64, 1)).expect("embedded");
    let len = before.virtual_section.len() + 0x70 + 0x20 + 0x10 + 0x10 + 0x40;
    assert_eq!(s.virtual_section.len(), len);
    assert_eq!(texture(&s, TEXTURE_KEY).map(|t| t.3), Some(WORD));
    assert_geometry_reads(&s);
    unembed_texture(&mut s, false, TEXTURE_KEY).expect("unembedded");
    assert!(texture(&s, TEXTURE_KEY).is_none());
    // The vertex and index data lay as the packer places them, so they come back as they were.
    assert_eq!(s.physical, before.physical);
    assert_geometry_reads(&s);
}

#[test]
fn a_physical_section_that_passes_64_kib_gives_the_block_map_the_slot_it_needs() {
    let (mut img, phys, _) = drawable([1.0, 1.0, 0.0]);
    let map = u64_at(&img.bytes, 0x08) as usize & 0x0FFF_FFFF;
    // No slot to spare: every block the edit adds needs one.
    (img.bytes[map + 8], img.bytes[map + 9]) = (0, 0);
    let mut s = mapped(&img, &phys);
    let wide = bc1(512, 256, 4);
    assert_eq!(wide.payload.len(), 0x1_0000);
    embed_texture(&mut s, false, TEXTURE_KEY, WORD, &wide).expect("embedded");
    let map = map_of(&s);
    let v = &s.virtual_section;
    assert_eq!((v[map + 8], v[map + 9]), (1, 2));
    assert!(v[map + 16..map + 16 + 8 * 3].iter().all(|b| *b == 0));
    assert_eq!(s.physical.len(), 0x1_0000 + phys.len());
    assert_eq!(
        texture(&s, TEXTURE_KEY).map(|t| t.4),
        Some(pattern(128 * 64 * 8, 4))
    );
    assert_geometry_reads(&s);
    let d = read_drawable(&s.virtual_section, Some(&s.physical)).expect("the drawable reads");
    assert_eq!(d.name, "box#dr");
}

#[test]
fn a_block_map_of_an_odd_number_of_slots_still_grows_the_slots_it_needs() {
    let (mut img, phys, _) = embedded(TEXTURE_KEY);
    // A map of one slot, last in the section: its slots end 8 bytes short of a 16-byte boundary.
    let map = img.alloc(0x20);
    img.bytes[map + 8] = 1;
    img.ptr(0x08, map);
    let mut s = mapped(&img, &phys);
    reshape_embedded_texture(&mut s, false, TEXTURE_KEY, &bc1(512, 256, 4)).expect("written");
    let map = map_of(&s);
    let v = &s.virtual_section;
    assert_eq!((v[map + 8], v[map + 9]), (1, 2));
    assert!(v[map + 16..map + 16 + 8 * 3].iter().all(|b| *b == 0));
    assert_geometry_reads(&s);
}

#[test]
fn what_the_re_pack_cannot_place_or_does_not_know_is_refused_and_changes_nothing() {
    let refused = |s: &mut Rpf9Sections, what: Written| {
        let kept = s.clone();
        let why = what(s).expect_err("refused").to_string();
        assert_eq!(*s, kept, "{why}");
        why
    };
    type Written = fn(&mut Rpf9Sections) -> std::result::Result<(), EmbedError>;

    // A 0x6 word that aims at the vertex data's start and is not its buffer's +0x18.
    let (mut img, phys, _) = embedded(TEXTURE_KEY);
    let extra = img.alloc(8);
    img.phys(extra, 0x2000);
    let why = refused(&mut mapped(&img, &phys), |s| {
        reshape_embedded_texture(s, false, TEXTURE_KEY, &bc1(64, 64, 2))
    });
    assert!(why.contains(&format!("{extra:#X}")), "{why}");

    // A second texture in one dictionary, a texture of no 2D shape and a key not embedded.
    let (img, phys, _) = embedded(TEXTURE_KEY);
    let why = refused(&mut mapped(&img, &phys), |s| {
        embed_texture(s, false, TEXTURE_KEY ^ 1, WORD, &bc1(64, 64, 2))
    });
    assert!(why.contains("already holds a texture"), "{why}");
    let why = refused(&mut mapped(&img, &phys), |s| {
        let deep = ModelTexture {
            depth: 2,
            ..bc1(64, 64, 2)
        };
        reshape_embedded_texture(s, false, TEXTURE_KEY, &deep)
    });
    assert!(why.contains("one slice"), "{why}");
    let why = refused(&mut mapped(&img, &phys), |s| {
        unembed_texture(s, false, TEXTURE_KEY ^ 1)
    });
    assert!(why.contains("no texture of that key"), "{why}");

    // Two buffers of one start, which a re-pack would part.
    let (mut img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let vertex_start = u64_at(&img.bytes, parts.vertex_buffer + 0x18) & 0x0FFF_FFFF;
    img.phys(parts.index_buffer + 0x18, vertex_start as usize);
    let why = refused(&mut mapped(&img, &phys), |s| {
        embed_texture(s, false, TEXTURE_KEY, WORD, &bc1(64, 64, 2))
    });
    assert!(why.contains("two buffer objects start"), "{why}");
}
