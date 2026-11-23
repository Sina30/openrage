use std::io::Cursor;

use super::dictionary::root_over;
use super::*;
use crate::container::rpf::{read_rpf9_body, write_rpf9_resource, Rpf9Form, Rpf9Sections};
use crate::gta6::hash::{name_hash32, name_hash64};

const V: u64 = 0x5000_0000;
const ADDED_TEXTURE: u32 = 0x1111_0003;
const ADDED_CONSTANT: u32 = 0x3333_0005;

fn mapped(img: &Image, phys: &[u8]) -> Rpf9Sections {
    Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: img.bytes.clone(),
        physical: phys.to_vec(),
    }
}

/// A block map of one virtual and one physical block at the end of `img`, its slots zero, as the
/// one the virtual section names at 0x08.
fn map_at_end(img: &mut Image) -> usize {
    let map = img.alloc(0x20);
    img.bytes[map + 8] = 1;
    img.bytes[map + 9] = 1;
    img.ptr(0x08, map);
    map
}

fn drawable_with_map() -> (Image, Vec<u8>, DrawableParts) {
    let (mut img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    map_at_end(&mut img);
    (img, phys, parts)
}

fn added(buffer: u8, offset: u16, words: usize) -> Parameter {
    Parameter {
        hash: ADDED_CONSTANT,
        value: ParameterValue::Constant {
            buffer,
            offset,
            words: (1..=words as u32).collect(),
        },
    }
}

/// The shader with another effect, its empty slot and constant 0x3333_0004 gone, a texture slot
/// that binds nothing added and a constant of four words added in buffer 1.
fn relaid(read: &Shader) -> Shader {
    let mut s = read.clone();
    s.name_hash ^= 0x10;
    s.parameters
        .retain(|p| p.hash != TEXTURES[1] && p.hash != CONSTANTS[3].0);
    s.texture_keys.truncate(1);
    let texture = Parameter {
        hash: ADDED_TEXTURE,
        value: ParameterValue::Texture { slot: 1 },
    };
    s.parameters.insert(1, texture);
    s.texture_keys.push(None);
    s.parameters.push(added(1, 16, 4));
    s
}

/// The sections a loose file of `s` reads back to.
fn loose(s: &Rpf9Sections) -> Rpf9Sections {
    let file = write_rpf9_resource(s).expect("written");
    let (find, bytes) = read_rpf9_body(Cursor::new(&file), 0, u64::MAX).expect("it reads");
    Rpf9Sections::of(&find, &bytes).expect("a resource")
}

fn target(v: &[u8], at: usize) -> usize {
    u64_at(v, at) as usize & 0x0FFF_FFFF
}

#[test]
fn a_shader_laid_out_anew_goes_at_the_virtual_section_s_end_and_reads_back_as_edited() {
    let (img, phys, parts) = drawable_with_map();
    let read = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    let edited = vec![relaid(&read.shaders[0])];
    let mut s = mapped(&img, &phys);
    write_materials(Rpf9Role::Drawable, &mut s, None, &edited).expect("laid out");

    // The infos, then the block: two buffer pointers, the buffers, the texture table.
    let (old, at) = (img.bytes.len(), parts.shader);
    let count = edited[0].parameters.len();
    let infos_len = (8 + 8 * count).next_multiple_of(16);
    let mut ends = [0usize; 2];
    for p in &edited[0].parameters {
        if let ParameterValue::Constant {
            buffer,
            offset,
            words,
        } = &p.value
        {
            let b = usize::from(*buffer);
            ends[b] = ends[b].max(usize::from(*offset) + 4 * words.len());
        }
    }
    let sizes = ends.map(|e| e.next_multiple_of(16));
    let block = old + infos_len;
    let table = block + 16 + sizes[0] + sizes[1];
    let block_len = table + 8 * 2 - block;
    let v = &s.virtual_section;
    assert_eq!(v.len(), block + block_len.next_multiple_of(16));
    assert_eq!(u32_at(v, at), SHADER_HASH ^ 0x10);
    assert_eq!(u64_at(v, at + 0x20), V + old as u64);
    assert_eq!(u64_at(v, at + 0x08), V + block as u64);
    assert_eq!(u64_at(v, at + 0x10), V + table as u64);
    let length = u16::from_le_bytes([v[at + 0x3A], v[at + 0x3B]]);
    assert_eq!(usize::from(length), block_len + 32);
    assert_eq!(&v[old..old + 8], &[2, 2, 0, 0, count as u8, 0, 0, 8][..]);
    assert_eq!(u64_at(v, block), V + (block + 16) as u64);
    assert_eq!(u64_at(v, block + 8), V + (block + 16 + sizes[0]) as u64);
    // The kept slot keeps its binding; the added one binds nothing.
    let binding = u64_at(&img.bytes, target(&img.bytes, at + 0x10));
    assert_eq!((u64_at(v, table), u64_at(v, table + 8)), (binding, 0));
    assert_eq!(
        (u32_at(v, old + 16), u32_at(v, old + 20)),
        (ADDED_TEXTURE, 4)
    );
    let last = old + 8 + 8 * (count - 1);
    let packed = 3 | 1 << 2 | 16 << 8 | 4 << 22;
    assert_eq!(
        (u32_at(v, last), u32_at(v, last + 4)),
        (ADDED_CONSTANT, packed)
    );
    assert!(
        v[..at] == img.bytes[..at] && v[at + 0x40..old] == img.bytes[at + 0x40..],
        "below the old end only the shader's own words changed"
    );
    assert!(s.physical == phys);

    let back = read_drawable(v, Some(&phys)).expect("it reads");
    assert_eq!(back.shaders, edited);
    let mut again = s.clone();
    write_materials(Rpf9Role::Drawable, &mut again, None, &back.shaders).expect("written");
    assert!(again == s, "writing what it read back changes nothing");
    assert!(loose(&s) == s, "the loose file reads back to the sections");
}

#[test]
fn an_unchanged_drawable_writes_its_own_bytes_and_another_effect_alone_moves_nothing() {
    let (img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let read = read_drawable(&img.bytes, Some(&phys)).expect("reads");
    let mut s = mapped(&img, &phys);
    write_materials(Rpf9Role::Drawable, &mut s, None, &read.shaders).expect("written");
    assert!(s == mapped(&img, &phys));

    let mut shaders = read.shaders.clone();
    shaders[0].name_hash = 0x7777_0001;
    write_materials(Rpf9Role::Drawable, &mut s, None, &shaders).expect("written");
    let mut expected = img.bytes.clone();
    expected[parts.shader..parts.shader + 4].copy_from_slice(&0x7777_0001u32.to_le_bytes());
    assert!(s.virtual_section == expected);
    let back = read_drawable(&s.virtual_section, Some(&phys)).expect("reads");
    assert_eq!(back.shaders, shaders);
}

#[test]
fn a_texture_slot_removed_or_emptied_leaves_its_binding_unnamed() {
    let (img, phys, parts) = drawable_with_map();
    let read = read_drawable(&img.bytes, Some(&phys)).expect("reads");
    let mut none = read.shaders[0].clone();
    none.parameters
        .retain(|p| !matches!(p.value, ParameterValue::Texture { .. }));
    none.texture_keys.clear();
    let mut s = mapped(&img, &phys);
    let one = std::slice::from_ref(&none);
    write_materials(Rpf9Role::Drawable, &mut s, None, one).expect("laid out");
    assert_eq!(u64_at(&s.virtual_section, parts.shader + 0x10), 0);
    let back = read_drawable(&s.virtual_section, Some(&phys)).expect("reads");
    assert_eq!(back.shaders, [none]);

    let mut emptied = read.shaders[0].clone();
    emptied.texture_keys[0] = None;
    emptied.parameters.pop();
    let mut s = mapped(&img, &phys);
    let one = std::slice::from_ref(&emptied);
    write_materials(Rpf9Role::Drawable, &mut s, None, one).expect("laid out");
    let v = &s.virtual_section;
    let table = target(v, parts.shader + 0x10);
    assert_eq!((u64_at(v, table), u64_at(v, table + 8)), (0, 0));
    let back = read_drawable(v, Some(&phys)).expect("reads");
    assert_eq!(back.shaders, [emptied]);
}

#[test]
fn a_layout_the_packed_words_cannot_state_is_refused_by_name_and_changes_nothing() {
    let (img, phys, _) = drawable_with_map();
    let read = read_drawable(&img.bytes, Some(&phys)).expect("reads");
    type Change = fn(&mut Shader);
    let tries: [(&str, Change); 6] = [
        ("two constants of one buffer overlap", |s| {
            s.parameters.push(added(0, 4, 1))
        }),
        ("crosses a 16-byte row", |s| {
            s.parameters.push(added(1, 28, 2))
        }),
        ("not a 4-byte word below 16 KiB", |s| {
            s.parameters.push(added(1, 0x4000, 1))
        }),
        ("past the 64 a packed word names", |s| {
            s.parameters.push(added(64, 0, 1))
        }),
        ("comes before a texture slot's record", |s| {
            let texture = Parameter {
                hash: ADDED_TEXTURE,
                value: ParameterValue::Texture { slot: 2 },
            };
            s.parameters.push(texture);
            s.texture_keys.push(None);
        }),
        ("no name or bound entry states", |s| {
            let texture = Parameter {
                hash: ADDED_TEXTURE,
                value: ParameterValue::Texture { slot: 2 },
            };
            s.parameters.insert(2, texture);
            s.texture_keys.push(Some(TEXTURE_KEY ^ 1));
        }),
    ];
    for (want, change) in tries {
        let mut shaders = read.shaders.clone();
        change(&mut shaders[0]);
        let mut s = mapped(&img, &phys);
        let err = write_materials(Rpf9Role::Drawable, &mut s, None, &shaders)
            .expect_err(want)
            .to_string();
        assert!(err.contains(want), "{want}: {err}");
        assert!(
            s == mapped(&img, &phys),
            "{want}: a refused write changed bytes"
        );
    }
}

#[test]
fn a_virtual_section_grown_past_64_kib_takes_another_slot_before_the_new_layout() {
    let (mut img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let end = img.bytes.len().next_multiple_of(16);
    img.alloc(0xFFC0 - end);
    let map = map_at_end(&mut img);
    assert_eq!(map, 0xFFC0);
    let read = read_drawable(&img.bytes, Some(&phys)).expect("reads");
    let edited = vec![relaid(&read.shaders[0])];
    let mut s = mapped(&img, &phys);
    write_materials(Rpf9Role::Drawable, &mut s, None, &edited).expect("laid out");

    // A second virtual block takes a slot: 16 bytes after the old two, before the new infos.
    let v = &s.virtual_section;
    assert_eq!((v[map + 8], v[map + 9]), (2, 1));
    let old = img.bytes.len();
    assert_eq!(u64_at(v, parts.shader + 0x20), V + (old + 16) as u64);
    assert!(v[old..old + 16].iter().all(|b| *b == 0));
    let back = read_drawable(v, Some(&phys)).expect("reads");
    assert_eq!(back.shaders, edited);
    assert!(loose(&s) == s, "the loose file reads back to the sections");
}

#[test]
fn a_dictionary_s_drawable_is_laid_out_anew_under_its_code() {
    let (mut img, mut phys) = (Image::default(), Vec::new());
    let root = img.alloc(0x40);
    let drawable = drawable_into(&mut img, &mut phys, [1.0, 1.0, 0.0]).root;
    let code = name_hash32(b"box#dr");
    root_over(&mut img, root, &mut [(code, drawable)]);
    map_at_end(&mut img);
    let dictionary = read_dictionary(&img.bytes, Some(&phys)).expect("reads");
    let [(_, Ok(Entry::Drawable(d)))] = dictionary.entries.as_slice() else {
        panic!("one drawable");
    };
    let edited = vec![relaid(&d.shaders[0])];
    let mut s = mapped(&img, &phys);
    let role = Rpf9Role::Dictionary;
    let err = write_materials(role, &mut s, Some(code ^ 1), &edited).expect_err("no such code");
    assert!(err
        .to_string()
        .contains("no entry of the dictionary has that code"));
    write_materials(role, &mut s, Some(code), &edited).expect("laid out");
    let back = read_dictionary(&s.virtual_section, Some(&phys)).expect("reads");
    let [(_, Ok(Entry::Drawable(d)))] = back.entries.as_slice() else {
        panic!("one drawable");
    };
    assert_eq!(d.shaders, edited);
}

/// The drawable with a map, the entry its first slot binds pointing at a key record header
/// `{6, word}` of its own, not the 16 bytes before it.
fn drawable_with_record(word: u32) -> (Image, Vec<u8>, DrawableParts) {
    let (mut img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let table = target(&img.bytes, parts.shader + 0x10);
    let entry = target(&img.bytes, target(&img.bytes, table) + 0x30);
    let header = img.alloc(0x10);
    img.u64(header, 6);
    img.u32(header + 8, word);
    img.u64(entry, V + header as u64 + 5);
    map_at_end(&mut img);
    (img, phys, parts)
}

#[test]
fn a_named_key_bound_to_a_new_slot_gets_a_binding_and_a_key_record_of_its_own() {
    let name = b"made_up_tex";
    let key = name_hash64(name);
    let (img, phys, parts) = drawable_with_map();
    let read = read_drawable(&img.bytes, Some(&phys)).expect("reads");
    let mut edited = vec![relaid(&read.shaders[0])];
    edited[0].texture_keys[1] = Some(key);
    let named = move |k: u64| (k == key).then(|| name_hash32(name));
    let mut s = mapped(&img, &phys);
    write_materials_with(Rpf9Role::Drawable, &mut s, None, &edited, &named).expect("laid out");

    let v = &s.virtual_section;
    let table = target(v, parts.shader + 0x10);
    let binding = target(v, table + 8);
    let entry = target(v, binding + 0x30);
    let header = entry - 0x10;
    assert_eq!(header % 16, 0, "the record is 16-aligned");
    let word = u64::from(name_hash32(name));
    assert_eq!((u64_at(v, header), u64_at(v, header + 8)), (6, word));
    assert_eq!(
        (u64_at(v, entry), u64_at(v, entry + 8)),
        (V + header as u64 + 5, key)
    );
    let held: Vec<u64> = (0..10).map(|i| u64_at(v, binding + 8 * i)).collect();
    let mut template = vec![0u64; 10];
    (template[2], template[6]) = (0x980, V + entry as u64);
    (template[7], template[8]) = (0x0001_0001_0000_0000, 0x0001_0000_0001_00FF);
    assert_eq!(held, template);
    assert_ne!(
        binding,
        target(v, table),
        "never a binding another slot holds"
    );

    let back = read_drawable(v, Some(&phys)).expect("it reads");
    assert_eq!(back.shaders, edited);
    let mut again = s.clone();
    write_materials(Rpf9Role::Drawable, &mut again, None, &back.shaders).expect("written");
    assert!(again == s, "writing what it read back changes nothing");
    assert!(loose(&s) == s, "the loose file reads back to the sections");

    // Neither a name nor a key record states the key's word.
    let mut s = mapped(&img, &phys);
    let err = write_materials(Rpf9Role::Drawable, &mut s, None, &edited).expect_err("no word");
    assert!(matches!(err, MaterialLayoutError::UnnamedKey { shader: 0, key: k } if k == key));
    assert!(s == mapped(&img, &phys), "a refused write changed bytes");
}

#[test]
fn a_key_the_body_binds_takes_the_word_its_key_record_states() {
    let word = 0x0BAD_F00D;
    let (img, phys, parts) = drawable_with_record(word);
    let read = read_drawable(&img.bytes, Some(&phys)).expect("reads");
    let mut edited = vec![relaid(&read.shaders[0])];
    edited[0].texture_keys[1] = Some(TEXTURE_KEY);
    let mut s = mapped(&img, &phys);
    write_materials(Rpf9Role::Drawable, &mut s, None, &edited).expect("laid out");
    let v = &s.virtual_section;
    let table = target(v, parts.shader + 0x10);
    let entry = target(v, target(v, table + 8) + 0x30);
    assert_eq!(u32_at(v, entry - 8), word);
    assert_ne!(
        entry,
        target(v, target(v, table) + 0x30),
        "a record of its own"
    );
    let back = read_drawable(v, Some(&phys)).expect("it reads");
    assert_eq!(back.shaders, edited);
}

#[test]
fn a_bound_slot_given_another_key_has_its_key_record_state_that_key_s_word() {
    let (img, phys, parts) = drawable_with_record(0x0BAD_F00D);
    let read = read_drawable(&img.bytes, Some(&phys)).expect("reads");
    let name = b"other_tex";
    let key = name_hash64(name);
    let mut edited = read.shaders.clone();
    edited[0].texture_keys[0] = Some(key);
    let named = move |k: u64| (k == key).then(|| name_hash32(name));
    let mut s = mapped(&img, &phys);
    write_materials_with(Rpf9Role::Drawable, &mut s, None, &edited, &named).expect("written");
    let v = &s.virtual_section;
    let entry = target(v, target(v, target(v, parts.shader + 0x10)) + 0x30);
    let header = target(v, entry) - 5;
    assert_eq!(
        (u64_at(v, entry + 8), u64_at(v, header + 8)),
        (key, u64::from(name_hash32(name)))
    );
    assert_eq!(
        read_drawable(v, Some(&phys)).expect("reads").shaders,
        edited
    );

    // A key no name states leaves the record's word unknown, so the edit is refused.
    let mut s = mapped(&img, &phys);
    let err = write_materials(Rpf9Role::Drawable, &mut s, None, &edited).expect_err("no word");
    assert!(matches!(err, MaterialLayoutError::UnnamedKey { shader: 0, key: k } if k == key));
    assert!(s == mapped(&img, &phys), "a refused write changed bytes");
}
