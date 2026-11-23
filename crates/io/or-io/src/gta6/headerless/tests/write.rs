use super::dictionary::{root_over, texture_into};
use super::*;
use crate::gta6::hash::name_hash32;

const OTHER_KEY: u64 = 0x0FED_CBA9_8765_4321;

/// The drawable's one shader with constant 0x3333_0002's three words and texture slot 0's key changed.
fn edited(d: &Drawable) -> Vec<Shader> {
    let mut shaders = d.shaders.clone();
    for p in &mut shaders[0].parameters {
        if let (0x3333_0002, ParameterValue::Constant { words, .. }) = (p.hash, &mut p.value) {
            *words = vec![0x3F80_0000, 0x4000_0000, 0x4040_0000];
        }
    }
    shaders[0].texture_keys[0] = Some(OTHER_KEY);
    shaders
}

fn not_in_place(written: Result<()>) -> bool {
    matches!(written, Err(HeaderlessError::NotInPlace(_)))
}

#[test]
fn a_drawable_s_constants_and_key_are_written_where_they_were_read_and_read_back() {
    let (img, phys, _) = drawable([1.0, 1.0, 0.0]);
    let read = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    let mut same = img.bytes.clone();
    write_shaders(Rpf9Role::Drawable, &mut same, None, &read.shaders).expect("written");
    assert!(
        same == img.bytes,
        "an unchanged drawable writes its own bytes"
    );

    let shaders = edited(&read);
    let mut v = img.bytes.clone();
    write_shaders(Rpf9Role::Drawable, &mut v, None, &shaders).expect("written");
    let changed = (v.iter().zip(&img.bytes)).filter(|(a, b)| a != b).count();
    assert!(
        (1..=3 * 4 + 8).contains(&changed),
        "{changed} bytes changed, past the three words and the key"
    );
    let back = read_drawable(&v, Some(&phys)).expect("the written drawable reads");
    assert_eq!(back.shaders, shaders);
    let mut again = v.clone();
    write_shaders(Rpf9Role::Drawable, &mut again, None, &back.shaders).expect("written");
    assert!(again == v, "writing what it read back is byte-identical");
}

#[test]
fn an_edit_that_moves_anything_is_refused_by_name() {
    let (img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let read = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    type Change = fn(&mut Shader);
    let tries: [(&str, Change); 4] = [
        ("a constant of another size", |s| {
            if let ParameterValue::Constant { words, .. } = &mut s.parameters[2].value {
                words.push(1);
            }
        }),
        ("a key for the empty slot", |s| {
            s.texture_keys[1] = Some(OTHER_KEY)
        }),
        ("a slot emptied", |s| s.texture_keys[0] = None),
        ("another effect", |s| s.name_hash ^= 1),
    ];
    for (what, change) in tries {
        let mut shaders = read.shaders.clone();
        change(&mut shaders[0]);
        let mut v = img.bytes.clone();
        assert!(
            not_in_place(write_shaders(Rpf9Role::Drawable, &mut v, None, &shaders)),
            "{what}"
        );
        assert!(v == img.bytes, "{what}: a refused write changed bytes");
    }

    // Both slots bound to one entry take one key only.
    let mut img = img;
    let table = u64_at(&img.bytes, parts.shader + 0x10) as usize & 0x0FFF_FFFF;
    let binding = u64_at(&img.bytes, table) as usize & 0x0FFF_FFFF;
    img.ptr(table + 8, binding);
    let read = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    let mut shaders = read.shaders.clone();
    shaders[0].texture_keys[0] = Some(OTHER_KEY);
    let mut v = img.bytes.clone();
    assert!(not_in_place(write_shaders(
        Rpf9Role::Drawable,
        &mut v,
        None,
        &shaders
    )));
    shaders[0].texture_keys[1] = Some(OTHER_KEY);
    write_shaders(Rpf9Role::Drawable, &mut v, None, &shaders).expect("one key for both");
    let back = read_drawable(&v, Some(&phys)).expect("reads");
    assert_eq!(back.shaders[0].texture_keys, [Some(OTHER_KEY); 2]);
}

#[test]
fn a_fragment_s_and_a_dictionary_s_drawable_are_written_where_their_root_points() {
    // A fragment: its root first, its drawable at 0x18.
    let (mut img, mut phys) = (Image::default(), Vec::new());
    let root = img.alloc(0x100);
    let drawable = drawable_into(&mut img, &mut phys, [1.0, 1.0, 0.0]).root;
    img.ptr(root + 0x18, drawable);
    let name = u64_at(&img.bytes, drawable + 0xA8) as usize & 0x0FFF_FFFF;
    img.ptr(root + 0xA8, name);
    let read = read_drawable_with_bound(&img.bytes, Some(&phys)).expect("the fragment reads");
    let shaders = edited(&read.drawable);
    let role = Rpf9Role::DrawableWithBound;
    write_shaders(role, &mut img.bytes, None, &shaders).expect("written");
    let back = read_drawable_with_bound(&img.bytes, Some(&phys)).expect("reads");
    assert_eq!(back.drawable.shaders, shaders);
    assert!(not_in_place(write_shaders(
        Rpf9Role::Drawable,
        &mut img.bytes,
        Some(7),
        &shaders
    )));

    // A dictionary: the entry of the code named.
    let (mut img, mut phys) = (Image::default(), Vec::new());
    let root = img.alloc(0x40);
    let drawable = drawable_into(&mut img, &mut phys, [1.0, 1.0, 0.0]).root;
    let code = name_hash32(b"box#dr");
    root_over(&mut img, root, &mut [(code, drawable)]);
    let dictionary = read_dictionary(&img.bytes, Some(&phys)).expect("reads");
    let [(_, Ok(Entry::Drawable(d)))] = dictionary.entries.as_slice() else {
        panic!("one drawable");
    };
    let shaders = edited(d);
    let role = Rpf9Role::Dictionary;
    assert!(not_in_place(write_shaders(
        role,
        &mut img.bytes,
        Some(code ^ 1),
        &shaders
    )));
    write_shaders(role, &mut img.bytes, Some(code), &shaders).expect("written");
    let back = read_dictionary(&img.bytes, Some(&phys)).expect("reads");
    let [(_, Ok(Entry::Drawable(d)))] = back.entries.as_slice() else {
        panic!("one drawable");
    };
    assert_eq!(d.shaders, shaders);
}

/// A dictionary of a 128×64 BC1 texture of two levels, "Trophy", and a 16×16×16 R8G8 volume, "fog".
fn textures() -> (Image, Vec<u8>) {
    let mut phys: Vec<u8> = (0..0x4000u32).map(|i| (i * 7 + i / 251) as u8).collect();
    phys[0x2000..].fill(0x11);
    let mut img = Image::default();
    let root = img.alloc(0x40);
    let bc1 = texture_into(&mut img, "Trophy", [128, 64, 1], 1, 71, 2, 8, (0, 0x2000));
    let fog = texture_into(&mut img, "fog", [16, 16, 16], 3, 49, 1, 2, (0x2000, 0x2000));
    let mut entries = [(name_hash32(b"trophy"), bc1), (name_hash32(b"fog"), fog)];
    root_over(&mut img, root, &mut entries);
    (img, phys)
}

/// The pixels of the texture `code` of the dictionary `img` and `phys` hold.
fn pixels_of(img: &Image, phys: &[u8], code: u32) -> Vec<u8> {
    let dictionary = read_dictionary(&img.bytes, Some(phys)).expect("reads");
    let entry = dictionary.entries.into_iter().find(|(c, _)| *c == code);
    let Some((_, Ok(Entry::Texture(t)))) = entry else {
        panic!("a texture");
    };
    t.texture.expect("its pixels read").pixels
}

#[test]
fn a_dictionary_texture_s_levels_are_tiled_back_into_its_memory_and_read_back() {
    let (img, phys) = textures();
    let trophy = name_hash32(b"trophy");
    let read = pixels_of(&img, &phys, trophy);
    let mut same = phys.clone();
    write_texture(&img.bytes, &mut same, "Trophy", &read).expect("written");
    assert!(same == phys, "the pixels it holds write nothing");

    let pixels: Vec<u8> = (0..read.len()).map(|i| (i % 253) as u8 ^ 0x5A).collect();
    let mut p = phys.clone();
    write_texture(&img.bytes, &mut p, "trophy", &pixels).expect("written");
    assert!(p != phys);
    assert!(
        p[0x2000..] == phys[0x2000..],
        "the volume's memory is left alone"
    );
    assert!(
        pixels_of(&img, &p, trophy) == pixels,
        "reads back as written"
    );
    let mut again = p.clone();
    write_texture(
        &img.bytes,
        &mut again,
        "trophy",
        &pixels_of(&img, &p, trophy),
    )
    .expect("written");
    assert!(again == p, "writing what it read back is byte-identical");
}

/// A dictionary of one 64×64 BC1 texture of one level, "layers": an array of three slices, or a
/// cube of six faces where `cube`, each slice's chain one 4 KiB tile after the one before it.
fn layers(cube: bool) -> (Image, Vec<u8>, usize) {
    let slices: usize = if cube { 6 } else { 3 };
    let len = 0x1000 * slices;
    let phys: Vec<u8> = (0..len as u32).map(|i| (i * 11 + i / 241) as u8).collect();
    let mut img = Image::default();
    let root = img.alloc(0x40);
    let size = [64, 64, slices as u16];
    let t = texture_into(
        &mut img,
        "layers",
        size,
        1 + u8::from(cube),
        71,
        1,
        8,
        (0, len),
    );
    if cube {
        img.u16(t + 0x60, 0x292);
    }
    root_over(&mut img, root, &mut [(name_hash32(b"layers"), t)]);
    (img, phys, slices)
}

#[test]
fn an_array_s_or_a_cube_s_slice_is_tiled_back_into_its_own_chain_and_read_back() {
    for cube in [false, true] {
        let (img, phys, slices) = layers(cube);
        let code = name_hash32(b"layers");
        let read = pixels_of(&img, &phys, code);
        let each = read.len() / slices;
        let mut pixels = read.clone();
        for (i, b) in pixels[each..2 * each].iter_mut().enumerate() {
            *b = (i % 251) as u8 ^ 0xA5;
        }
        let mut p = phys.clone();
        write_texture(&img.bytes, &mut p, "layers", &pixels).expect("written");
        assert!(pixels_of(&img, &p, code) == pixels, "reads back as written");
        assert!(p[0x1000..0x2000] != phys[0x1000..0x2000], "slice 1 changed");
        assert!(
            p[..0x1000] == phys[..0x1000] && p[0x2000..] == phys[0x2000..],
            "the other slices' chains are left as they were"
        );
        let mut again = p.clone();
        write_texture(&img.bytes, &mut again, "layers", &pixels).expect("written");
        assert!(again == p, "writing what it read back is byte-identical");
    }
}

#[test]
fn a_texture_of_other_levels_a_volume_or_no_entry_is_refused_by_name() {
    let (img, phys) = textures();
    let read = pixels_of(&img, &phys, name_hash32(b"trophy"));
    let fog = pixels_of(&img, &phys, name_hash32(b"fog"));
    let mut p = phys.clone();
    let tries = [
        ("Trophy", &read[1..]),
        ("fog", &vec![0; fog.len()][..]),
        ("absent", &read[..]),
    ];
    for (name, pixels) in tries {
        assert!(
            not_in_place(write_texture(&img.bytes, &mut p, name, pixels)),
            "{name}"
        );
    }
    assert!(p == phys, "a refused write changed bytes");
}
