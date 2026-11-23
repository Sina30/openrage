use super::dictionary::texture_into;
use super::*;

/// The hash the built embedded dictionary lists for its texture.
const HASH: u32 = 0x1B04_679F;

/// Where the shader group of the built drawable lies.
fn group_of(img: &Image, parts: &DrawableParts) -> usize {
    u64_at(&img.bytes, parts.root + 0x10) as usize & 0x0FFF_FFFF
}

/// A 128×64 BC1 texture of two levels at p+0, its tail tile all 0xBB and its top tile all 0xAA,
/// in a dictionary the built drawable's shader group points at, its key record stating `key`; the
/// drawable's geometry data after it.
pub(super) fn embedded(key: u64) -> (Image, Vec<u8>, usize) {
    let mut phys = vec![0xBB; 0x1000];
    phys.extend([0xAA; 0x1000]);
    let mut img = Image::default();
    let parts = drawable_into(&mut img, &mut phys, [1.0, 1.0, 0.0]);
    let texture = texture_into(&mut img, "unused", [128, 64, 1], 1, 71, 2, 8, (0, 0x2000));
    let record = img.alloc(0x20);
    // A tagged word where a dictionary texture's name record holds zero.
    img.u64(record, 0x5000_0005 + record as u64);
    img.u64(record + 0x08, key);
    img.ptr(texture + 0x30, record);
    let hashes = img.alloc(4);
    img.u32(hashes, HASH);
    let textures = img.pointers(&[texture]);
    let dictionary = img.alloc(0x40);
    img.u32(dictionary + 0x18, 1);
    img.ptr(dictionary + 0x20, hashes);
    img.u16(dictionary + 0x28, 1);
    img.u16(dictionary + 0x2A, 1);
    img.ptr(dictionary + 0x30, textures);
    img.u16(dictionary + 0x38, 1);
    img.u16(dictionary + 0x3A, 1);
    img.ptr(group_of(&img, &parts) + 0x08, dictionary);
    (img, phys, dictionary)
}

#[test]
fn a_body_led_by_its_embedded_texture_reads_its_geometry_and_the_texture_a_slot_binds() {
    let (img, phys, _) = embedded(TEXTURE_KEY);
    let d = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    let models = d.levels[0].as_ref().expect("level 0");
    let g = models[0].geometries[0]
        .as_ref()
        .expect("the geometry reads");
    assert_eq!(g.triangles, [[0, 1, 2]]);
    assert_eq!(
        g.positions.as_ref().expect("positions"),
        &TRIANGLE_AT.to_vec()
    );

    let head = read_drawable_head(&img.bytes, Some(&phys), false).expect("the head reads");
    assert_eq!(head.name, d.name);
    assert_eq!(head.lead, Some(BufferKind::Texture));
    assert_eq!(head.unlisted_vertex_buffers, 0);
    assert!(head.palettes.is_empty());
    let [Ok(t)] = head.textures.as_slice() else {
        panic!("one texture, not {:?}", head.textures);
    };
    assert_eq!((t.key, t.hash), (TEXTURE_KEY, HASH));
    assert_eq!(d.shaders[0].texture_keys[0], Some(t.key));
    assert_eq!(t.texture.name, format!("hash_{TEXTURE_KEY:016X}"));
    assert_eq!(
        (t.texture.width, t.texture.height, t.texture.pixels_at),
        (128, 64, 0)
    );
    let mut want = vec![0xAA; 32 * 16 * 8];
    want.extend([0xBB; 16 * 8 * 8]);
    let pixels = t.texture.texture.as_ref().expect("its pixels read");
    assert!(
        pixels.pixels == want,
        "mip0 last, mip1 in the tail tile first"
    );
}

#[test]
fn an_embedded_dictionary_whose_two_counts_differ_is_refused() {
    let (mut img, phys, dictionary) = embedded(TEXTURE_KEY);
    img.u16(dictionary + 0x38, 2);
    assert!(matches!(
        read_drawable_head(&img.bytes, Some(&phys), false),
        Err(HeaderlessError::Disagree {
            first: 1,
            second: 2,
            ..
        })
    ));
}

/// A 0x982 object over `words` at `at` bytes into the physical section.
fn palette_object(img: &mut Image, words: usize, at: usize) -> usize {
    let object = img.alloc(0x50);
    img.u32(object + 0x08, words as u32);
    img.u16(object + 0x0C, 4);
    img.u16(object + 0x0E, 8);
    img.u32(object + 0x10, 0x982);
    img.phys(object + 0x18, at);
    object
}

/// The built drawable whose shader group's table reaches a palette of `words` at p+0 from row 0's
/// +0x00, and one of `second` at p+0x100 from its +0x08.
fn with_palettes(words: &[u32], second: &[u32]) -> Result<DrawableHead> {
    let mut phys = vec![0u8; 0x200];
    for (k, w) in words.iter().enumerate() {
        phys[4 * k..4 * k + 4].copy_from_slice(&w.to_le_bytes());
    }
    for (k, w) in second.iter().enumerate() {
        phys[0x100 + 4 * k..0x104 + 4 * k].copy_from_slice(&w.to_le_bytes());
    }
    let mut img = Image::default();
    let parts = drawable_into(&mut img, &mut phys, [1.0, 1.0, 0.0]);
    let first = palette_object(&mut img, words.len(), 0);
    let other = palette_object(&mut img, second.len(), 0x100);
    let table = img.alloc(0x40);
    img.ptr(table, first);
    img.ptr(table + 0x08, other);
    img.u64(table + 0x10, 6);
    img.u32(table + 0x18, 0x8207_9169);
    img.ptr(group_of(&img, &parts) + 0x30, table);
    read_drawable_head(&img.bytes, Some(&phys), false)
}

#[test]
fn a_tint_palette_is_the_leading_run_of_opaque_colours_its_group_s_table_reaches() {
    let words = [0xFF11_2233, 0xFF44_5566, 0xFFC7_08E9, 0, 0];
    let head = with_palettes(&words, &[0xFFAB_CDEF]).expect("the head reads");
    assert_eq!(head.lead, Some(BufferKind::CpuTexture));
    let colours = |w: &[u32]| w.iter().map(|w| w.to_le_bytes()).collect::<Vec<_>>();
    let read: Vec<&TintPalette> = head.palettes.iter().flatten().collect();
    assert_eq!(
        read,
        [
            &TintPalette {
                at: 0,
                colours: colours(&words[..3]),
            },
            &TintPalette {
                at: 0x100,
                colours: colours(&[0xFFAB_CDEF]),
            },
        ]
    );

    // A word after the first zero, or one whose alpha is not 0xFF, is refused by its place.
    for (words, at, found) in [
        (vec![0xFF11_2233, 0, 0xFF44_5566], 8, 0xFF44_5566),
        (vec![0xFF11_2233, 0x7F44_5566], 4, 0x7F44_5566),
    ] {
        let head = with_palettes(&words, &[0xFFAB_CDEF]).expect("the head reads");
        assert!(
            matches!(
                head.palettes[0],
                Err(HeaderlessError::Constant { at: a, found: f, .. }) if (a, f) == (at, found)
            ),
            "{:?}",
            head.palettes[0]
        );
        assert!(head.palettes[1].is_ok());
    }
}

#[test]
fn a_vertex_buffer_no_model_lists_is_counted() {
    let (mut img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let head = read_drawable_head(&img.bytes, Some(&phys), false).expect("the head reads");
    assert_eq!(
        (head.lead, head.unlisted_vertex_buffers),
        (Some(BufferKind::Vertex), 0)
    );
    let vb = img.alloc(0x50);
    let listed = parts.vertex_buffer;
    let header = img.bytes[listed..listed + 0x50].to_vec();
    img.put(vb, &header);
    let head = read_drawable_head(&img.bytes, Some(&phys), false).expect("the head reads");
    assert_eq!(head.unlisted_vertex_buffers, 1);
}
