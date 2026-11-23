use super::*;

/// The built drawable's one geometry, read.
fn geometry_of(img: &Image, phys: &[u8]) -> Result<Geometry> {
    let mut d = read_drawable(&img.bytes, Some(phys)).expect("the drawable reads");
    let mut models = d.levels[0].take().expect("level 0");
    models.remove(0).geometries.remove(0)
}

/// A stream of one 4-byte element a vertex.
fn words(channel: u8, code: u8, words: [u32; 3]) -> Stream {
    Stream {
        channel,
        code,
        size: 4,
        bytes: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    }
}

/// The drawable of [`drawable`] whose geometry holds `streams` beside its float positions.
fn with_streams(streams: Vec<Stream>) -> (Image, Vec<u8>, DrawableParts) {
    let (mut img, mut phys, parts) = drawable([1.0, 1.0, 0.0]);
    let mut all = vec![float_positions()];
    all.extend(streams);
    put_vertex_block(&mut img, &mut phys, &parts, 3, all, None);
    (img, phys, parts)
}

fn near(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

#[test]
fn a_one_byte_channel_pads_the_stream_after_it_to_four_bytes() {
    let indices = Stream {
        channel: 20,
        code: 0x3E,
        size: 1,
        bytes: vec![7, 1, 2],
    };
    let colours = words(24, 0x1C, [0xFF33_00FF, 0, u32::MAX]);
    let (img, phys, parts) = with_streams(vec![indices, colours]);
    let d = parts.declaration;
    let offset = |k: usize| u32_at(&img.bytes, d + 4 * k);
    assert_eq!(offset(24), (offset(20) + 3).next_multiple_of(4));
    assert_ne!(offset(24), offset(20) + 3);
    let g = geometry_of(&img, &phys).expect("the geometry reads");
    assert_eq!(
        g.vertices.colours,
        [(24, vec![[1.0, 0.0, 0.2, 1.0], [0.0; 4], [1.0; 4]])]
    );
    assert_eq!(
        g.vertices.joints,
        [(20, vec![[7, 0, 0, 0], [1, 0, 0, 0], [2, 0, 0, 0]])]
    );
    assert!(g.vertices.skipped.is_empty());
}

#[test]
fn a_two_byte_index_and_four_two_byte_indices_read_as_their_types_state() {
    let one = Stream {
        channel: 20,
        code: 0x39,
        size: 2,
        bytes: [0x0102u16, 3, 0xFFFF]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect(),
    };
    let four = Stream {
        channel: 21,
        code: 0x0C,
        size: 8,
        bytes: [[1u16, 2, 3, 0x0104], [0; 4], [5, 6, 7, 8]]
            .iter()
            .flatten()
            .flat_map(|x| x.to_le_bytes())
            .collect(),
    };
    let (img, phys, _) = with_streams(vec![one, four]);
    let v = geometry_of(&img, &phys)
        .expect("the geometry reads")
        .vertices;
    assert_eq!(
        v.joints,
        [
            (20, vec![[0x0102, 0, 0, 0], [3, 0, 0, 0], [0xFFFF, 0, 0, 0]]),
            (21, vec![[1, 2, 3, 0x0104], [0; 4], [5, 6, 7, 8]]),
        ]
    );
}

#[test]
fn streams_that_do_not_follow_one_another_refuse_their_geometry() {
    let (mut img, phys, parts) = with_streams(vec![words(24, 0x1C, [0; 3])]);
    let at = parts.declaration + 4 * 24;
    let moved = u32_at(&img.bytes, at) + 4;
    img.u32(at, moved);
    assert!(matches!(
        geometry_of(&img, &phys),
        Err(HeaderlessError::Declaration {
            why: "its channels do not follow one another",
            ..
        })
    ));
}

/// The physical offset of the built geometry's vertex data.
fn vertex_data(img: &Image, parts: &DrawableParts) -> usize {
    u64_at(&img.bytes, parts.vertex_buffer + 0x18) as usize & 0x0FFF_FFFF
}

#[test]
fn a_slot_that_states_another_offset_refuses_its_geometry() {
    let (img, mut phys, parts) = with_streams(vec![]);
    let slot = vertex_data(&img, &parts);
    phys[slot + 1] += 2;
    assert!(matches!(
        geometry_of(&img, &phys),
        Err(HeaderlessError::Streams {
            why: "a slot's offset is not its channel's"
        })
    ));
}

#[test]
fn a_slot_no_measured_body_sets_refuses_its_geometry() {
    let (img, mut phys, parts) = with_streams(vec![]);
    let slot = vertex_data(&img, &parts) + 4 * 20;
    phys[slot..slot + 4].copy_from_slice(&0x4026u32.to_le_bytes());
    assert!(matches!(
        geometry_of(&img, &phys),
        Err(HeaderlessError::Streams {
            why: "it sets a slot no measured body sets"
        })
    ));
}

#[test]
fn a_vertex_buffer_whose_flags_are_not_the_measured_ones_refuses_its_geometry() {
    let (mut img, phys, parts) = with_streams(vec![]);
    img.u32(parts.vertex_buffer + 0x10, 0x1600);
    assert!(matches!(
        geometry_of(&img, &phys),
        Err(HeaderlessError::Constant {
            what: "buffer's flags",
            found: 0x1600,
            ..
        })
    ));
}

#[test]
fn the_frame_word_holds_a_normal_and_a_tangent_in_either_form() {
    // Bit 31 set: a 10:10 octahedral normal of +x, and the tangent's angle about it, 0 and a
    // quarter turn. Bit 31 clear: an 8:8 normal of +x and a 7:7 tangent of +y.
    let frames = [
        (1 << 31) | (1 << 30) | (511 << 10) | 1023,
        (1 << 31) | (256 << 20) | (511 << 10) | 1023,
        (1 << 30) | (127 << 23) | (63 << 16) | (127 << 8) | 255,
    ];
    let (img, phys, _) = with_streams(vec![words(4, 0x29, frames)]);
    let v = geometry_of(&img, &phys)
        .expect("the geometry reads")
        .vertices;
    for n in &v.normals {
        assert!(near(n, &[1.0, 0.0, 0.0]), "{n:?}");
    }
    let expected = [
        [0.0, 0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0, -1.0],
        [0.0, 1.0, 0.0, 1.0],
    ];
    for (t, e) in v.tangents.iter().zip(expected) {
        assert!(near(t, &e), "{t:?} against {e:?}");
    }
    assert_eq!(v.tangents.len(), 3);
}

#[test]
fn texcoords_weights_indices_and_tangents_decode_by_their_type() {
    let s16 = |u: i16, v: i16| u32::from(u as u16) | (u32::from(v as u16) << 16);
    let uv = words(28, 0x26, [s16(8192, -4096), 0, s16(i16::MIN, i16::MAX)]);
    let floats = Stream {
        channel: 29,
        code: 0x10,
        size: 8,
        bytes: [0.5f32, -2.0, 0.0, 0.0, 3.0, 4.0]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect(),
    };
    let weights = words(16, 0x1C, [0xFF, 0x0080_007F, 0]);
    let joints = words(20, 0x1E, [0x0403_0201, 0, 0x00FF_0000]);
    // Bits 12 to 31 of each lane are not the index.
    let wide: Vec<u32> = [[0xB210_0005, 0x0001_2007, 0, 0xFFF], [0; 4], [1, 2, 3, 4]].concat();
    let wide = Stream {
        channel: 21,
        code: 0x03,
        size: 16,
        bytes: wide.iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let tangent = words(
        9,
        0x18,
        [(3 << 30) | (511 << 20) | (511 << 10) | 1023, 1023, 1023],
    );
    // Channel 8 is texcoord 0's tangent in the same form, signs 0 and 3.
    let uv0_tangent = words(
        8,
        0x18,
        [(511 << 20) | (511 << 10) | 1023, (3 << 30) | 1023, 1023],
    );
    // Lane 0's bit 15 set on vertices 0 and 1 only: their lanes 1 to 3 are a reference position.
    let reference: [[u16; 4]; 3] = [
        [0x8000, 32768 + 8192, 32768 - 4096, 32768],
        [0x806F, 0, 0xFFFF, 32768 + 2048],
        [0x7FFF, 32768 + 8192, 32768 + 8192, 32768 + 8192],
    ];
    let data = Stream {
        channel: 33,
        code: 0x10,
        size: 8,
        bytes: reference
            .iter()
            .flatten()
            .flat_map(|q| q.to_le_bytes())
            .collect(),
    };
    let streams = vec![
        uv,
        floats,
        uv0_tangent,
        weights,
        joints,
        wide,
        tangent,
        data,
    ];
    let (img, phys, _) = with_streams(streams);
    let v = geometry_of(&img, &phys)
        .expect("the geometry reads")
        .vertices;
    assert_eq!(
        v.uvs,
        [
            (28, vec![[1.0, -0.5], [0.0, 0.0], [-4.0, 32767.0 / 8192.0]]),
            (29, vec![[0.5, -2.0], [0.0, 0.0], [3.0, 4.0]]),
        ]
    );
    assert_eq!(
        v.weights,
        [(
            16,
            vec![
                [1.0, 0.0, 0.0, 0.0],
                [127.0 / 255.0, 0.0, 128.0 / 255.0, 0.0],
                [0.0; 4]
            ]
        )]
    );
    assert_eq!(
        v.joints,
        [
            (20, vec![[1, 2, 3, 4], [0; 4], [0, 0, 0xFF, 0]]),
            (21, vec![[5, 7, 0, 0xFFF], [0; 4], [1, 2, 3, 4]]),
        ]
    );
    let (channel, tangents) = &v.uv_tangents[0];
    assert_eq!((*channel, tangents.len()), (9, 3));
    assert!(
        near(&tangents[0], &[1.0, 0.0, 0.0, -1.0]),
        "{:?}",
        tangents[0]
    );
    let third = 1.0 / 3f32.sqrt();
    assert!(
        near(&tangents[1], &[third, -third, -third, 1.0]),
        "{:?}",
        tangents[1]
    );
    assert_eq!(
        v.skipped.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        [21, 33],
        "the wide form's upper bits and channel 33's bit-15-clear lanes are named"
    );
    assert_eq!(
        v.reference,
        [
            Some([1.0, -0.5, 0.0]),
            Some([-4.0, 32767.0 / 8192.0, 0.25]),
            None
        ]
    );
    assert_eq!(v.tangents.len(), 3);
    assert!(
        near(&v.tangents[0], &[1.0, 0.0, 0.0, 1.0]),
        "{:?}",
        v.tangents[0]
    );
    assert!(
        near(&v.tangents[1], &[third, -third, -third, -1.0]),
        "{:?}",
        v.tangents[1]
    );
    assert!(v.normals.is_empty() && v.colours.is_empty());
}

/// A morph target's 240-byte virtual section under `key` and its physical section of `scale`, an
/// origin then a step per lane, and `elements`.
fn morph_target(key: u32, scale: [f32; 6], elements: &[[u32; 3]]) -> (Image, Vec<u8>) {
    let mut img = Image::default();
    let root = img.alloc(0x70);
    let vb = img.alloc(0x50);
    img.bytes.resize(240, 0);
    img.ptr(root + 0x10, vb);
    img.u32(root + 0x58, key);
    let mut phys = Vec::new();
    phys.extend(1u32.to_le_bytes());
    for x in scale {
        phys.extend(x.to_le_bytes());
    }
    phys.extend(0u32.to_le_bytes());
    for e in elements {
        phys.extend(pack_lanes(*e).to_le_bytes());
    }
    img.u32(vb + 0x08, (phys.len() / 4) as u32);
    img.u16(vb + 0x0C, 4);
    img.u16(vb + 0x0E, 9);
    img.u32(vb + 0x10, 0x1601);
    img.phys(vb + 0x18, 0);
    (img, phys)
}

const MORPH_SCALE: [f32; 6] = [
    -0.25,
    -0.5,
    -0.75,
    1.0 / 16_777_216.0,
    1.0 / 1_048_576.0,
    1.0 / 65_536.0,
];
/// Elements whose every lane reaches both faces of its box.
const MORPH_ELEMENTS: [[u32; 3]; 2] = [[1, 2, 3], [0xFF_FFFF, 0xFF_8001, 0xFFFF]];

#[test]
fn a_morph_target_reads_its_mesh_key_quantisation_and_elements() {
    let (mut img, phys) = morph_target(0x1357_9BDF, MORPH_SCALE, &MORPH_ELEMENTS);
    let read = read_morph_target(&img.bytes, Some(&phys)).expect("it reads");
    assert_eq!(
        read,
        MorphTarget {
            mesh_key: 0x1357_9BDF,
            origin: [-0.25, -0.5, -0.75],
            step: [2f32.powi(-24), 2f32.powi(-20), 2f32.powi(-16)],
            elements: MORPH_ELEMENTS.to_vec(),
        }
    );
    assert!(!read.is_empty() && read.mesh().is_none());
    let [a, b, c] = MORPH_ELEMENTS[1];
    let expected = [
        -0.25 + a as f32 * 2f32.powi(-24),
        -0.5 + b as f32 * 2f32.powi(-20),
        -0.75 + c as f32 * 2f32.powi(-16),
    ];
    assert_eq!(read.value(MORPH_ELEMENTS[1]), expected);

    img.ptr(0x70 + 0x40, 0);
    assert!(matches!(
        read_morph_target(&img.bytes, Some(&phys)),
        Err(HeaderlessError::Constant {
            what: "morph target's vertex declaration",
            ..
        })
    ));
}

#[test]
fn a_morph_target_names_its_base_head_and_level_by_its_key() {
    for (name, lod) in [
        ("head_fr1_base_000_m_full", 0),
        ("head_mi1_base_000_i_full_lod2", 2),
        ("head_mk1_base_000_k_full_lod3", 3),
    ] {
        let key = crate::gta6::hash::name_hash32(name.as_bytes());
        let (img, phys) = morph_target(key, MORPH_SCALE, &MORPH_ELEMENTS);
        let read = read_morph_target(&img.bytes, Some(&phys)).expect("it reads");
        assert_eq!(read.mesh(), Some((name.to_string(), lod)));
    }
}

#[test]
fn an_empty_morph_target_reads_only_with_zero_elements() {
    let mut scale = MORPH_SCALE;
    scale[3..].fill(0.0);
    let (img, phys) = morph_target(7, scale, &[[0; 3]; 3]);
    let read = read_morph_target(&img.bytes, Some(&phys)).expect("an empty target reads");
    assert!(read.is_empty());
    let (img, phys) = morph_target(7, scale, &[[0; 3], [0, 1, 0]]);
    assert!(matches!(
        read_morph_target(&img.bytes, Some(&phys)),
        Err(HeaderlessError::Streams { .. })
    ));
}

#[test]
fn a_morph_target_off_the_measured_rules_is_refused() {
    fn refused(img: &Image, phys: &[u8]) -> bool {
        read_morph_target(&img.bytes, Some(phys)).is_err()
    }
    let (img, phys) = morph_target(7, MORPH_SCALE, &MORPH_ELEMENTS);
    assert!(!refused(&img, &phys));
    // A virtual section of another length, and a root word that is not zero.
    let mut longer = Image {
        bytes: img.bytes.clone(),
    };
    longer.bytes.resize(256, 0);
    assert!(refused(&longer, &phys));
    let mut stray = Image {
        bytes: img.bytes.clone(),
    };
    stray.u32(0x30, 1);
    assert!(refused(&stray, &phys));
    // A lane that stops short of a face of its box.
    let short = [[1, 2, 3], [0xFF_FFFF, 0xFE_0000, 0xFFFF]];
    let (img, phys) = morph_target(7, MORPH_SCALE, &short);
    assert!(refused(&img, &phys));
    // A box that leaves zero out, and a step that is not finite.
    for (lane, x) in [(1, 0.25), (5, f32::NAN)] {
        let mut scale = MORPH_SCALE;
        scale[lane] = x;
        let (img, phys) = morph_target(7, scale, &MORPH_ELEMENTS);
        assert!(refused(&img, &phys), "lane value {lane}");
    }
}

/// A vertex buffer object at a new place in `img` over `count` vertices of `streams`, with its
/// declaration; the object's offset.
fn vertex_buffer_into(
    img: &mut Image,
    phys: &mut Vec<u8>,
    count: u32,
    streams: Vec<Stream>,
) -> usize {
    let declaration = img.alloc(0x140);
    let vb = img.alloc(0x50);
    img.u16(vb + 0x0C, 4);
    img.u16(vb + 0x0E, 9);
    img.u32(vb + 0x10, 0x1601);
    img.ptr(vb + 0x40, declaration);
    let parts = DrawableParts {
        root: 0,
        shader: 0,
        parameter_infos: 0,
        vertex_buffer: vb,
        declaration,
        index_buffer: 0,
        index_data: 0,
        geometry_box: 0,
    };
    put_vertex_block(img, phys, &parts, count, streams, None);
    vb
}

#[test]
fn an_array_of_vertex_buffers_reads_each_buffer_s_count_and_channels() {
    let mut img = Image::default();
    let root = img.alloc(0x30);
    let mut phys = Vec::new();
    let first = vertex_buffer_into(&mut img, &mut phys, 3, vec![float_positions()]);
    let uv = Stream {
        channel: 28,
        code: 0x26,
        size: 4,
        bytes: vec![1; 8],
    };
    let two = Stream {
        channel: 0,
        code: 6,
        size: 12,
        bytes: vec![0; 24],
    };
    let second = vertex_buffer_into(&mut img, &mut phys, 2, vec![two, uv]);
    // Channel 3, which no declaration here uses, holds a word, so the root's eighth slot is not zero.
    img.u32(0x30 + 4 * 3, 0x40);
    let array = img.pointers(&[first, second]);
    let map = img.alloc(0x10);
    img.ptr(root + 0x08, map);
    img.ptr(root + 0x10, array);
    img.u16(root + 0x18, 2);
    img.u16(root + 0x1A, 2);
    let signature = Rpf9Signature::of(&img.bytes, img.bytes.len() as u64).expect("a signature");
    assert_eq!(signature.to_string(), "0PPh00hh");
    let Ok(Resource::VertexBuffers(set)) = read(signature, &img.bytes, Some(&phys)) else {
        panic!("{signature} reads as vertex buffers");
    };
    let indices = |n: usize| set.buffers[n].1.iter().map(|c| c.index).collect::<Vec<_>>();
    assert_eq!(set.buffers.len(), 2);
    assert_eq!((set.buffers[0].0, indices(0)), (3, vec![0]));
    assert_eq!((set.buffers[1].0, indices(1)), (2, vec![0, 28]));

    img.u16(root + 0x1A, 3);
    assert!(matches!(
        read_vertex_buffers(&img.bytes, Some(&phys)),
        Err(HeaderlessError::Disagree {
            first: 2,
            second: 3,
            ..
        })
    ));
}

/// A keyed blob's root: the block map pointer, then the key and the stated length.
fn blob_root(img: &mut Image, key: u64, len: u64) -> usize {
    let root = img.alloc(0x30);
    img.u64(root + 0x20, key);
    img.u64(root + 0x28, len);
    root
}

#[test]
fn a_keyed_blob_reads_its_key_and_length_in_either_section() {
    let key = 0x2C9F_A9DD_9914_3D1B;
    // Behind a raw buffer object at 0x30, its words in the physical section.
    let mut img = Image::default();
    let root = blob_root(&mut img, key, 24);
    let raw = img.alloc(0x50);
    img.u32(raw + 0x08, 6);
    img.u16(raw + 0x0C, 4);
    img.u16(raw + 0x0E, 8);
    img.u32(raw + 0x10, 0x1380);
    img.phys(raw + 0x18, 0);
    let map = img.alloc(0x10);
    img.ptr(root + 0x08, map);
    img.ptr(root + 0x10, raw);
    let phys = vec![7; 24];
    let signature = Rpf9Signature::of(&img.bytes, img.bytes.len() as u64).expect("a signature");
    assert_eq!(signature.to_string(), "0PP0hh0h");
    let Ok(Resource::KeyedBlob(blob)) = read(signature, &img.bytes, Some(&phys)) else {
        panic!("{signature} reads as a keyed blob");
    };
    assert_eq!((blob.key, blob.len, blob.physical), (key, 24, true));

    // Its bytes in the virtual section from 0x30 to the block map.
    let mut img = Image::default();
    let root = blob_root(&mut img, key ^ 1, 0x20);
    let data = img.alloc(0x20);
    img.bytes[data..data + 0x20].fill(0x11);
    let map = img.alloc(0x10);
    img.ptr(root + 0x08, map);
    img.ptr(root + 0x18, data);
    let signature = Rpf9Signature::of(&img.bytes, img.bytes.len() as u64).expect("a signature");
    assert_eq!(signature.to_string(), "0P0Phhhh");
    let Ok(Resource::KeyedBlob(blob)) = read(signature, &img.bytes, Some(&[])) else {
        panic!("{signature} reads as a keyed blob");
    };
    assert_eq!((blob.key, blob.len, blob.physical), (key ^ 1, 0x20, false));

    img.u64(root + 0x28, 0x30);
    assert!(matches!(
        read_keyed_blob(&img.bytes, Some(&[])),
        Err(HeaderlessError::Disagree {
            first: 0x30,
            second: 0x20,
            ..
        })
    ));
}
