use std::io::Cursor;

use super::super::geometry_encode::{
    frame_word, meshlet_bounds, meshlets, pack_lanes as packed, s16_pair, Meshlet, Quantisation,
};
use super::super::streams::{frame, lanes};
use super::*;
use crate::container::rpf::{read_rpf9_body, write_rpf9_resource, Rpf9Form, Rpf9Sections};
use crate::half_to_f32;

const AT: GeometryAt = GeometryAt {
    level: 0,
    model: 0,
    geometry: 0,
};
const UP: [f32; 3] = [0.0, 0.0, 1.0];
const ALONG_X: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const COLOUR: [u8; 4] = [0xFF, 0x80, 0x00, 0xFF];

/// A stream of `elements`, each its bytes.
fn stream<const N: usize>(channel: u8, code: u8, elements: &[[u8; N]]) -> Stream {
    Stream {
        channel,
        code,
        size: N as u8,
        bytes: elements.iter().flatten().copied().collect(),
    }
}

/// The drawable of [`drawable`] whose one geometry holds `positions` quantised in their own box,
/// a frame word, a colour and texcoord 0 a vertex, and where `skinned` blend weights and indices;
/// its triangles [`drawable`]'s one, its block map an allocation of its own.
fn quantised(positions: &[[f32; 3]], skinned: bool) -> (Rpf9Sections, DrawableParts) {
    let (mut img, mut phys, parts) = drawable([1.0, 1.0, 0.0]);
    let map = img.alloc(0x20);
    (img.bytes[map + 8], img.bytes[map + 9]) = (1, 1);
    img.ptr(0x08, map);
    let corner = |pick: fn(f32, f32) -> f32| -> [f32; 3] {
        std::array::from_fn(|a| {
            positions
                .iter()
                .map(|p| p[a])
                .reduce(pick)
                .expect("vertices")
        })
    };
    let (min, max) = (corner(f32::min), corner(f32::max));
    img.vec3(parts.geometry_box, min);
    img.vec3(parts.geometry_box + 0x10, max);
    let block = Quantisation::of_box(min, max);
    let mut header = [0u8; 0x80];
    block.write(&mut header);
    let n = positions.len();
    let uvs: Vec<[u8; 8]> = (0..n)
        .map(|i| {
            let uv = [i as f32 * 0.25, 1.0 - i as f32 * 0.5];
            let mut e = [0u8; 8];
            e[..4].copy_from_slice(&uv[0].to_le_bytes());
            e[4..].copy_from_slice(&uv[1].to_le_bytes());
            e
        })
        .collect();
    let mut streams = vec![
        stream(
            0,
            0x10,
            &positions
                .iter()
                .map(|p| packed(block.quanta(*p).0))
                .collect::<Vec<_>>(),
        ),
        stream(4, 0x29, &vec![frame_word(UP, ALONG_X).to_le_bytes(); n]),
        stream(24, 0x1C, &vec![COLOUR; n]),
        stream(28, 0x10, &uvs),
    ];
    if skinned {
        streams.push(stream(16, 0x1C, &vec![[0xFF, 0, 0, 0]; n]));
        streams.push(stream(20, 0x1E, &vec![[0u8; 4]; n]));
    }
    let mut quantisation = [0u8; 0x20];
    quantisation.copy_from_slice(&header[0x60..]);
    put_vertex_block(
        &mut img,
        &mut phys,
        &parts,
        n as u32,
        streams,
        Some(quantisation),
    );
    let sections = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: img.bytes,
        physical: phys,
    };
    (sections, parts)
}

fn geometry_in(s: &Rpf9Sections) -> Geometry {
    let mut d = read_drawable(&s.virtual_section, Some(&s.physical)).expect("the drawable reads");
    let mut models = d.levels[0].take().expect("level 0");
    models
        .remove(0)
        .geometries
        .remove(0)
        .expect("the geometry reads")
}

/// The edit that gives a geometry the values its reader read.
fn edit_of(g: &Geometry) -> GeometryEdit {
    GeometryEdit {
        positions: g.positions.as_ref().expect("positions").clone(),
        normals: g.vertices.normals.clone(),
        tangents: g.vertices.tangents.clone(),
        uvs: g.vertices.uvs.clone(),
        colours: g.vertices.colours.clone(),
        triangles: g.triangles.clone(),
        ..GeometryEdit::default()
    }
}

fn target(v: &[u8], at: usize) -> usize {
    u64_at(v, at) as usize & 0x0FFF_FFFF
}

/// The index data of the built geometry's index buffer, as words.
fn index_data(s: &Rpf9Sections, parts: &DrawableParts) -> Vec<u32> {
    let v = &s.virtual_section;
    let (at, words) = (
        target(v, parts.index_buffer + 0x18),
        u32_at(v, parts.index_buffer + 0x08) as usize,
    );
    (0..words)
        .map(|k| u32_at(&s.physical, at + 4 * k))
        .collect()
}

/// Whether each meshlet's stored sphere holds every vertex its list names.
fn spheres_hold(words: &[u32], positions: &[[f32; 3]]) -> bool {
    let (m, t) = ((words[0] & 0xFFFF) as usize, words[1] as usize);
    let list = words[2] as usize;
    let entry = |j: usize| {
        let w = words[3 + 4 * m + t + j / 2];
        (if j.is_multiple_of(2) {
            w & 0xFFFF
        } else {
            w >> 16
        }) as usize
    };
    (0..m).all(|k| {
        let [w0, w1, w2, w3] = [0, 1, 2, 3].map(|o| words[3 + 4 * k + o]);
        let radius = f64::from(half_to_f32((w0 >> 16) as u16));
        let scale = 2f64.powi((w2 >> 28) as i32 - 4);
        let field = |f: u32| (f64::from(f & 0xFFF) - 2048.0) * scale;
        let centre = [field(w2), field(w2 >> 12), field(w3)];
        let first = (w1 & 0xFFFF) as usize;
        let last = match k + 1 < m {
            true => (words[3 + 4 * (k + 1) + 1] & 0xFFFF) as usize,
            false => list,
        };
        // The sphere holds the decoded vertices; their f32 lie within a quantum of them.
        (first..last).all(|j| {
            let p = positions[entry(j)];
            let d: f64 = (0..3).map(|a| (f64::from(p[a]) - centre[a]).powi(2)).sum();
            d.sqrt() <= radius + 1e-5
        })
    })
}

fn near(a: &[[f32; 3]], b: &[[f32; 3]], within: f32) -> bool {
    a.len() == b.len()
        && (a.iter().zip(b)).all(|(p, q)| (0..3).all(|k| (p[k] - q[k]).abs() <= within))
}

#[test]
fn an_unchanged_edit_writes_nothing_and_saves_byte_identical() {
    let (sections, _) = quantised(&TRIANGLE_AT, true);
    let edit = edit_of(&geometry_in(&sections));
    let mut written = sections.clone();
    let report =
        write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit).expect("written");
    assert_eq!(written, sections);
    assert_eq!(report.carried, [16, 20], "no edit gives weights or indices");
    assert!(!report.triangles_rebuilt && !report.bounds_rewritten && !report.root_reboxed);
    assert_eq!(report.physical_grown, 0);
    assert_eq!(
        write_rpf9_resource(&written).expect("written"),
        write_rpf9_resource(&sections).expect("written")
    );
}

#[test]
fn a_moved_vertex_is_quantised_again_and_its_boxes_and_meshlet_bounds_follow() {
    let (sections, parts) = quantised(&TRIANGLE_AT, true);
    let mut edit = edit_of(&geometry_in(&sections));
    edit.positions[2] = [0.0, 2.0, 0.0];
    let mut written = sections.clone();
    let report =
        write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit).expect("written");
    assert!(report.bounds_rewritten && report.root_reboxed && !report.triangles_rebuilt);
    assert_eq!(
        written.physical.len(),
        sections.physical.len(),
        "the same bytes"
    );
    let g = geometry_in(&written);
    assert_eq!(g.triangles, [[0, 1, 2]]);
    let read = g.positions.as_ref().expect("positions");
    assert!(near(read, &edit.positions, 1e-6), "{read:?}");

    let v = &written.virtual_section;
    let four = |at: usize| [0, 4, 8, 12].map(|o| f32::from_bits(u32_at(v, at + o)));
    // The geometry's box with w = x, the drawable's with w = 0.
    let x = edit.positions[1][0];
    assert_eq!(four(parts.geometry_box), [0.0; 4]);
    assert_eq!(four(parts.geometry_box + 0x10), [x, 2.0, 0.0, x]);
    assert_eq!(four(parts.root + 0x30), [0.0; 4]);
    assert_eq!(four(parts.root + 0x40), [x, 2.0, 0.0, 0.0]);
    let wide = f64::from(x);
    let radius = ([wide, 2.0, 0.0]
        .map(|d: f64| d.powi(2))
        .iter()
        .sum::<f64>()
        .sqrt()
        / 2.0) as f32;
    assert_eq!(
        four(parts.root + 0x20),
        [(wide / 2.0) as f32, 1.0, 0.0, radius]
    );
    assert!(spheres_hold(&index_data(&written, &parts), read));
}

#[test]
fn a_new_vertex_count_lays_its_streams_out_again_and_rebuilds_its_meshlets() {
    let (sections, parts) = quantised(&TRIANGLE_AT, false);
    let quad = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let edit = GeometryEdit {
        positions: quad.clone(),
        normals: vec![UP; 4],
        tangents: vec![ALONG_X; 4],
        uvs: vec![(28, uvs.clone())],
        colours: vec![(24, vec![[1.0, 0.5, 0.0, 1.0]; 4])],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        ..GeometryEdit::default()
    };
    let mut written = sections.clone();
    let report =
        write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit).expect("written");
    assert!(report.triangles_rebuilt && report.carried.is_empty());
    assert_eq!(
        report.physical_grown,
        written.physical.len() - sections.physical.len()
    );

    let g = geometry_in(&written);
    assert_eq!(g.vertex_count, 4);
    assert_eq!(g.triangles, edit.triangles);
    assert!(near(g.positions.as_ref().expect("positions"), &quad, 1e-6));
    assert_eq!(g.vertices.uvs, [(28, uvs)]);
    let colour = [0xFF, 0x80, 0x00, 0xFF].map(|b: u8| f32::from(b) / 255.0);
    assert_eq!(g.vertices.colours, [(24, vec![colour; 4])]);
    assert!(near(&g.vertices.normals, &[UP; 4], 0.01));

    // Ascending channel order from 0x80, each stream 4-aligned after the one before.
    let v = &written.virtual_section;
    let d = parts.declaration;
    let mut next = 0x80;
    for (channel, size) in [(0, 8), (4, 4), (24, 4), (28, 8)] {
        assert_eq!(
            u32_at(v, d + 4 * channel) as usize,
            next,
            "channel {channel}"
        );
        next = (next + size * 4usize).next_multiple_of(4);
    }
    // Channel 1 is unused: it takes channel 0's offset.
    assert_eq!(u32_at(v, d + 4), 0x80);
    assert_eq!(u64_at(v, d + 0x138), 1 | (8 + 4 + 4 + 8) << 2 | 4 << 10);
    assert_eq!(
        u32_at(v, parts.vertex_buffer + 0x08) as usize,
        next.div_ceil(4)
    );

    // One meshlet of two triangles over a list of four vertices: roundup4(3 + 4 + 2 + 2) words.
    let words = index_data(&written, &parts);
    assert_eq!(words.len(), 12);
    assert_eq!(words[..3], [0x8000_0001, 2, 4]);
    assert_eq!(words[3] & 0xFFFF, 3 | 1 << 7);
    assert_eq!(words[4], 0);
    assert_eq!(words[7..9], [1 << 7 | 2 << 14, 2 << 7 | 3 << 14]);
    assert_eq!(words[9..12], [1 << 16, 2 | 3 << 16, 0]);
    assert!(spheres_hold(&words, &quad));
    let record = parts.geometry_box + 32;
    assert_eq!(target(v, record), parts.index_buffer);
    assert_eq!((v[record + 8], v[record + 0x0A]), (1, 2));
    assert_eq!(u32_at(v, record + 0x0C), 4 << 8);

    // Saved as a loose resource and read back, it reads as written.
    let file = write_rpf9_resource(&written).expect("written");
    let (find, bytes) = read_rpf9_body(Cursor::new(&file), 0, u64::MAX).expect("it reads");
    let back = Rpf9Sections::of(&find, &bytes).expect("a resource");
    let again = geometry_in(&back);
    assert_eq!(again.triangles, edit.triangles);
    assert_eq!(again.positions.as_ref().ok(), g.positions.as_ref().ok());
    assert_eq!(write_rpf9_resource(&back).expect("again"), file);
}

#[test]
fn a_channel_no_edit_gives_refuses_a_new_vertex_count_and_changes_nothing() {
    let (sections, _) = quantised(&TRIANGLE_AT, true);
    let mut edit = edit_of(&geometry_in(&sections));
    edit.positions.push([1.0, 1.0, 0.0]);
    edit.normals.push(UP);
    edit.tangents.push(ALONG_X);
    edit.uvs[0].1.push([0.5, 0.5]);
    edit.colours[0].1.push([1.0; 4]);
    edit.triangles.push([1, 3, 2]);
    let mut written = sections.clone();
    let refused = write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit);
    assert!(
        matches!(
            refused,
            Err(GeometryWriteError::Channel { channel: 16, .. })
        ),
        "{refused:?}"
    );
    assert_eq!(written, sections);
}

#[test]
fn new_triangles_over_per_meshlet_words_and_a_vertex_past_the_count_are_refused_by_name() {
    let (mut sections, parts) = quantised(&TRIANGLE_AT, false);
    let mut edit = edit_of(&geometry_in(&sections));
    edit.triangles = vec![[0, 2, 1]];
    // Bit 30 of the first word, which the record states again in its bit 31.
    let at = parts.index_data;
    let first = u32_at(&sections.physical, at) | 1 << 30;
    sections.physical[at..at + 4].copy_from_slice(&first.to_le_bytes());
    let record = parts.geometry_box + 32;
    let counts = u32_at(&sections.virtual_section, record + 0x0C) | 1 << 31;
    sections.virtual_section[record + 0x0C..record + 0x10].copy_from_slice(&counts.to_le_bytes());
    geometry_in(&sections);
    let mut written = sections.clone();
    let refused = write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit)
        .expect_err("refused")
        .to_string();
    assert!(refused.contains("per-meshlet words"), "{refused}");
    assert_eq!(written, sections);

    edit.triangles = vec![[0, 1, 3]];
    let refused = write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit)
        .expect_err("refused")
        .to_string();
    assert!(refused.contains("past the vertex count"), "{refused}");
}

#[test]
fn a_mesh_past_64_kib_takes_meshlets_of_128_and_grows_the_physical_section_by_whole_blocks() {
    let (sections, parts) = quantised(&TRIANGLE_AT, false);
    let (columns, rows) = (60u32, 50u32);
    let positions: Vec<[f32; 3]> = (0..rows * columns)
        .map(|i| [(i % columns) as f32 * 0.5, (i / columns) as f32 * 0.5, 0.0])
        .collect();
    let mut triangles = Vec::new();
    for r in 0..rows - 1 {
        for c in 0..columns - 1 {
            let at = r * columns + c;
            triangles.push([at, at + 1, at + columns + 1]);
            triangles.push([at, at + columns + 1, at + columns]);
        }
    }
    let n = positions.len();
    let edit = GeometryEdit {
        positions: positions.clone(),
        normals: vec![UP; n],
        tangents: vec![ALONG_X; n],
        uvs: vec![(28, vec![[0.5, 0.5]; n])],
        colours: vec![(24, vec![[1.0; 4]; n])],
        triangles: triangles.clone(),
        ..GeometryEdit::default()
    };
    let mut written = sections.clone();
    write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit).expect("written");
    assert_eq!(
        written.physical.len() % 0x1_0000,
        0,
        "a whole number of blocks"
    );
    let map = target(&written.virtual_section, 0x08);
    assert_eq!(written.virtual_section[map + 9], 1, "one physical block");
    let g = geometry_in(&written);
    assert_eq!((g.vertex_count as usize, &g.triangles), (n, &triangles));
    let words = index_data(&written, &parts);
    assert_eq!((words[0] & 0xFFFF) as usize, meshlets(&triangles).len());
    assert!(spheres_hold(
        &words,
        g.positions.as_ref().expect("positions")
    ));

    let file = write_rpf9_resource(&written).expect("written");
    let (find, bytes) = read_rpf9_body(Cursor::new(&file), 0, u64::MAX).expect("it reads");
    let back = Rpf9Sections::of(&find, &bytes).expect("a resource");
    assert_eq!(geometry_in(&back).triangles, triangles);
}

#[test]
fn meshlets_close_at_128_vertices_or_128_triangles_and_list_vertices_by_first_use() {
    // Disjoint triangles: 42 take 126 vertices, and a 43rd would make 129.
    let disjoint: Vec<[u32; 3]> = (0..200).map(|k| [3 * k, 3 * k + 1, 3 * k + 2]).collect();
    let counts =
        |cut: &[Meshlet]| -> Vec<usize> { cut.iter().map(|k| k.triangles.len()).collect() };
    assert_eq!(counts(&meshlets(&disjoint)), [42, 42, 42, 42, 32]);
    // A fan adds a vertex a triangle: 126 triangles hold 128 vertices.
    let fan: Vec<[u32; 3]> = (0..300).map(|k| [0, k + 1, k + 2]).collect();
    let cut = meshlets(&fan);
    assert_eq!(counts(&cut), [126, 126, 48]);
    assert_eq!(cut[1].vertices[..3], [0, 127, 128]);
    assert_eq!(cut[1].triangles[0], [0, 1, 2]);
    // One triangle again and again: the triangle limit closes each.
    assert_eq!(counts(&meshlets(&[[0, 1, 2]; 300])), [128, 128, 44]);
    // A corner named twice is listed once.
    assert_eq!(meshlets(&[[4, 4, 9]])[0].vertices, [4, 9]);
}

#[test]
fn a_meshlet_sphere_holds_its_points_and_coarsens_its_grid_past_128_m() {
    let cube: Vec<[f64; 3]> = (0..8)
        .map(|k| [0, 1, 2].map(|a| if k >> a & 1 == 1 { 0.5 } else { -0.5 }))
        .collect();
    let (radius, centre, z) = meshlet_bounds(&cube).expect("bounds");
    assert_eq!((centre, z), (2048 | 2048 << 12, 2048));
    assert!(f64::from(half_to_f32((radius >> 16) as u16)) >= 0.75f64.sqrt());
    // At 300 m a 1/16 m field passes 4,095, and a 1/4 m one does not.
    let far = [[300.0, 0.0, 0.0], [301.0, 1.0, 1.0]];
    let (radius, centre, z) = meshlet_bounds(&far).expect("bounds");
    assert_eq!(centre >> 28, 2);
    let field = |f: u32| (f64::from(f & 0xFFF) - 2048.0) * 0.25;
    let stored = [field(centre), field(centre >> 12), field(z)];
    let held = f64::from(half_to_f32((radius >> 16) as u16));
    assert!(far.iter().all(|p| {
        let d: f64 = (0..3).map(|a| (p[a] - stored[a]).powi(2)).sum();
        d.sqrt() <= held
    }));
    assert!(meshlet_bounds(&[[1e9, 0.0, 0.0]]).is_err());
}

#[test]
fn the_quantisation_block_puts_lane_c_on_the_smallest_f32_extent_and_steps_by_it() {
    let block = Quantisation::of_box([0.0; 3], [2.0, 1.0, 0.5]);
    let unit = |bits: u32| (1.0 / ((1u64 << bits) - 1) as f64) as f32;
    assert_eq!(block.axis, 2);
    assert_eq!(block.origin, [0.0; 3]);
    assert_eq!(block.step, [2.0 * unit(24), unit(24), 0.5 * unit(16)]);
    // Equal f32 extents: the lowest axis.
    let tie = Quantisation::of_box([1.0, 0.5, 0.0], [1.25, 0.75, 1.0]);
    assert_eq!((tie.axis, tie.origin), (0, [0.0, 0.5, 1.0]));
    // A zero extent steps by 1e-12 of a lane.
    let flat = Quantisation::of_box([0.0; 3], [1.0, 1.0, 0.0]);
    assert_eq!(flat.step[2], 1e-12 * unit(16));
    // Lanes pack as the reader unpacks them, and the box's corners take the lanes' ends.
    for q in [
        [0, 0, 0],
        [0x12_3456, 0xAB_CDEF, 0x7FFF],
        [0xFF_FFFF, 0xFF_FFFF, 0xFFFF],
    ] {
        assert_eq!(lanes(&packed(q)), q);
        assert_eq!(packed(q), pack_lanes(q).to_le_bytes());
    }
    assert_eq!(block.quanta([0.0; 3]), ([0, 0, 0], true));
    // The f32 step of a 24-bit lane is a ulp off 1/(2^24 - 1), a quantum at the far corner.
    let (corner, held) = block.quanta([2.0, 1.0, 0.5]);
    assert!(held);
    for (q, end) in corner.iter().zip([0xFF_FFFF, 0xFF_FFFF, 0xFFFF]) {
        assert!(end - q <= 1, "{corner:?}");
    }
    assert_eq!(corner[2], 0xFFFF);
    assert!(!block.quanta([3.0, 1.0, 0.5]).1, "past the block");
}

#[test]
fn an_angle_form_frame_word_re_encodes_from_what_it_decodes_to() {
    for (x, y) in [(600u32, 400u32), (511, 512), (300, 650)] {
        for angle in [0u32, 100, 511, 1000] {
            for sign in [0u32, 1] {
                let word = 1 << 31 | sign << 30 | angle << 20 | y << 10 | x;
                let (n, t) = frame(word);
                assert_eq!(frame_word(n, t), word, "{word:#010X}");
            }
        }
    }
    // A new frame is measured about the normal its word decodes to.
    let (n, t) = frame(frame_word([0.6, 0.0, 0.8], [0.8, 0.0, -0.6, -1.0]));
    assert!(
        (n[0] - 0.6).abs() < 0.01 && (n[2] - 0.8).abs() < 0.01,
        "{n:?}"
    );
    assert!(
        (t[0] - 0.8).abs() < 0.02 && (t[2] + 0.6).abs() < 0.02,
        "{t:?}"
    );
    assert_eq!(t[3], -1.0);
    assert_eq!(s16_pair([4.0, 0.0]), None);
    assert_eq!(s16_pair([-4.0, 0.5]), Some([0x00, 0x80, 0x00, 0x10]));
}

#[test]
fn slot_10_states_channel_12_and_slot_16_texcoord_4() {
    let (mut img, mut phys, parts) = drawable([1.0, 1.0, 0.0]);
    let streams = vec![
        float_positions(),
        stream(12, 0x18, &[[0u8; 4]; 3]),
        stream(32, 0x26, &[[0x00, 0x20, 0x00, 0xE0]; 3]),
    ];
    put_vertex_block(&mut img, &mut phys, &parts, 3, streams, None);
    let mut d = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    let mut models = d.levels[0].take().expect("level 0");
    let g = models
        .remove(0)
        .geometries
        .remove(0)
        .expect("the geometry reads");
    assert_eq!(g.vertices.uvs, [(32, vec![[1.0, -1.0]; 3])]);
    assert!(g.vertices.skipped.iter().any(|(c, _)| *c == 12));
}

#[test]
fn a_skinned_geometry_takes_a_new_vertex_count_from_the_edit_s_weights_and_indices() {
    let (sections, _) = quantised(&TRIANGLE_AT, true);
    let mut edit = edit_of(&geometry_in(&sections));
    edit.positions.push([1.0, 1.0, 0.0]);
    edit.normals.push(UP);
    edit.tangents.push(ALONG_X);
    edit.uvs[0].1.push([0.5, 0.5]);
    edit.colours[0].1.push([1.0; 4]);
    edit.triangles.push([1, 3, 2]);
    edit.weights = vec![(16, vec![[0.6, 0.4, 0.0, 0.0]; 4])];
    edit.joints = vec![(20, vec![[2, 7, 0, 0]; 4])];
    let mut written = sections.clone();
    let report =
        write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit).expect("written");
    assert!(report.carried.is_empty(), "{report:?}");
    let g = geometry_in(&written);
    assert_eq!(g.vertex_count, 4);
    // 0.6 and 0.4 of 255, each rounded to its exact share and summing to 255.
    let weights = [153u8, 102, 0, 0].map(|b| f32::from(b) / 255.0);
    assert_eq!(g.vertices.weights, [(16, vec![weights; 4])]);
    assert_eq!(g.vertices.joints, [(20, vec![[2, 7, 0, 0]; 4])]);

    edit.joints = vec![(20, vec![[256, 0, 0, 0]; 4])];
    let mut again = sections.clone();
    let refused = write_geometry(&mut again, Rpf9Role::Drawable, None, AT, &edit);
    assert!(
        matches!(
            refused,
            Err(GeometryWriteError::Channel { channel: 20, .. })
        ),
        "a byte form holds no index past 255: {refused:?}"
    );
    assert_eq!(again, sections);
}

#[test]
fn a_0x8c_index_keeps_its_high_bits_at_its_count_and_refuses_another_count() {
    let (mut img, mut phys, parts) = drawable([1.0, 1.0, 0.0]);
    const HIGH: u32 = 0xABCDE;
    let lanes: Vec<[u8; 16]> = (0..3u32)
        .map(|i| {
            let mut e = [0u8; 16];
            e[..4].copy_from_slice(&(HIGH << 12 | i).to_le_bytes());
            e
        })
        .collect();
    let streams = vec![float_positions(), stream(20, 0x03, &lanes)];
    put_vertex_block(&mut img, &mut phys, &parts, 3, streams, None);
    let sections = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: img.bytes,
        physical: phys,
    };
    let g = geometry_in(&sections);
    assert_eq!(
        g.vertices.joints,
        [(20, vec![[0, 0, 0, 0], [1, 0, 0, 0], [2, 0, 0, 0]])]
    );
    let mut edit = edit_of(&g);
    edit.joints = vec![(20, vec![[5, 0, 0, 0]; 3])];
    let mut written = sections.clone();
    write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit).expect("written");
    let back = geometry_in(&written);
    assert_eq!(back.vertices.joints, [(20, vec![[5, 0, 0, 0]; 3])]);
    let offset = (back.channels.iter().find(|c| c.index == 20))
        .expect("channel 20")
        .offset as usize;
    let data = target(&written.virtual_section, parts.vertex_buffer + 0x18);
    for i in 0..3 {
        let word = u32_at(&written.physical, data + offset + 16 * i);
        assert_eq!(word, HIGH << 12 | 5, "vertex {i}");
    }

    edit.positions.push([1.0, 1.0, 0.0]);
    edit.joints = vec![(20, vec![[5, 0, 0, 0]; 4])];
    edit.triangles.push([1, 3, 2]);
    let mut again = sections.clone();
    let refused = write_geometry(&mut again, Rpf9Role::Drawable, None, AT, &edit)
        .expect_err("refused")
        .to_string();
    assert!(refused.contains("0x8C"), "{refused}");
    assert_eq!(again, sections);
}

/// [`quantised`]'s skinned triangle, whose every vertex weights palette entry 0, its model's
/// palette the list `words` stated with `count` entries; where its model and geometry lie.
fn with_palette(words: &[u32], count: usize) -> (Rpf9Sections, usize, usize) {
    let (mut s, parts) = quantised(&TRIANGLE_AT, true);
    let v = &mut s.virtual_section;
    let model = target(v, target(v, target(v, parts.root + 0x50)));
    let geometry = target(v, target(v, model + 0x08));
    let list = v.len().next_multiple_of(16);
    v.resize(list + (4 * words.len()).next_multiple_of(16), 0);
    for (k, w) in words.iter().enumerate() {
        v[list + 4 * k..list + 4 * k + 4].copy_from_slice(&w.to_le_bytes());
    }
    v[model + 0x28..model + 0x30].copy_from_slice(&(0x5000_0000 + list as u64).to_le_bytes());
    v[model + 0x30] = count as u8;
    v[geometry + 0x2C..geometry + 0x30].copy_from_slice(&(count as u32).to_le_bytes());
    (s, model, geometry)
}

fn model_in(s: &Rpf9Sections) -> Model {
    let mut d = read_drawable(&s.virtual_section, Some(&s.physical)).expect("the drawable reads");
    d.levels[0].take().expect("level 0").remove(0)
}

#[test]
fn a_bone_hash_the_palette_lacks_goes_in_its_order_and_every_index_follows_its_bone() {
    let (b, c) = (0x2000_0000u32, 0x3000_0000u32);
    let (sections, _, geometry) = with_palette(&[b, c], 2);
    let mut written = sections.clone();
    let add = PaletteAdd {
        hashes: vec![0x1000_0000, c],
        ..PaletteAdd::default()
    };
    write_palette(&mut written, Rpf9Role::Drawable, None, 0, 0, &add).expect("written");
    let model = model_in(&written);
    let wanted = Palette::Hashes(vec![0x1000_0000, b, c]);
    assert_eq!(model.palette.ok(), Some(wanted));
    let g = model
        .geometries
        .into_iter()
        .next()
        .expect("a geometry")
        .expect("it reads");
    assert_eq!(
        g.vertices.joints,
        [(20, vec![[1; 4]; 3])],
        "entry 0 moved to 1"
    );
    assert_eq!(g.vertices.weights, geometry_in(&sections).vertices.weights);
    assert_eq!(u32_at(&written.virtual_section, geometry + 0x2C), 3);
    assert!(written.virtual_section.len() > sections.virtual_section.len());

    // A hash the list holds adds nothing, and a hash list takes no bone by its number.
    let mut again = written.clone();
    let held = PaletteAdd {
        hashes: vec![b],
        ..PaletteAdd::default()
    };
    write_palette(&mut again, Rpf9Role::Drawable, None, 0, 0, &held).expect("nothing to add");
    assert_eq!(again, written);
    let bone = PaletteAdd {
        bones: vec![1],
        ..PaletteAdd::default()
    };
    let refused = write_palette(&mut again, Rpf9Role::Drawable, None, 0, 0, &bone)
        .expect_err("refused")
        .to_string();
    assert!(refused.contains("by its number"), "{refused}");
    assert_eq!(again, written);
}

#[test]
fn a_bone_mask_takes_a_bone_s_bit_in_place_and_refuses_one_past_its_count() {
    let (sections, model, _) = with_palette(&[1, 0], 40);
    assert_eq!(model_in(&sections).palette.ok(), Some(Palette::Own));
    let mut written = sections.clone();
    let add = PaletteAdd {
        bones: vec![33, 2],
        ..PaletteAdd::default()
    };
    write_palette(&mut written, Rpf9Role::Drawable, None, 0, 0, &add).expect("set");
    let v = &written.virtual_section;
    let list = target(v, model + 0x28);
    assert_eq!((u32_at(v, list), u32_at(v, list + 4)), (1 | 1 << 2, 1 << 1));
    assert_eq!(v.len(), sections.virtual_section.len(), "in place");

    let past = PaletteAdd {
        bones: vec![40],
        ..PaletteAdd::default()
    };
    let mut again = written.clone();
    let refused = write_palette(&mut again, Rpf9Role::Drawable, None, 0, 0, &past)
        .expect_err("refused")
        .to_string();
    assert!(refused.contains("past its mask's count"), "{refused}");
    assert_eq!(again, written);
}

/// [`drawable`]'s triangle as floats with channel 33 `lanes`, its block map an allocation of its own.
fn with_reference(lanes: &[[u8; 8]]) -> Rpf9Sections {
    let (mut img, mut phys, parts) = drawable([1.0, 1.0, 0.0]);
    let map = img.alloc(0x20);
    (img.bytes[map + 8], img.bytes[map + 9]) = (1, 1);
    img.ptr(0x08, map);
    let streams = vec![float_positions(), stream(33, 0x10, lanes)];
    put_vertex_block(&mut img, &mut phys, &parts, 3, streams, None);
    Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: img.bytes,
        physical: phys,
    }
}

#[test]
fn channel_33_s_reference_follows_a_new_vertex_where_every_vertex_sets_bit_15() {
    const OFFSET: [f32; 3] = [0.5, -0.25, 1.0];
    let element = |p: &[f32; 3]| -> [u8; 8] {
        let q = |a: usize| (f64::from(p[a] + OFFSET[a]) * 8192.0 + 32768.0).round() as u16;
        let mut e = [0u8; 8];
        for (k, x) in [0x8000, q(0), q(1), q(2)].iter().enumerate() {
            e[2 * k..2 * k + 2].copy_from_slice(&x.to_le_bytes());
        }
        e
    };
    let lanes: Vec<[u8; 8]> = TRIANGLE_AT.iter().map(element).collect();
    let sections = with_reference(&lanes);
    let g = geometry_in(&sections);
    let moved =
        |p: &[f32; 3]| -> Option<[f32; 3]> { Some(std::array::from_fn(|a| p[a] + OFFSET[a])) };
    let held: Vec<Option<[f32; 3]>> = TRIANGLE_AT.iter().map(moved).collect();
    assert_eq!(g.vertices.reference, held);

    let mut edit = edit_of(&g);
    edit.positions.push([1.0, 1.0, 0.0]);
    edit.triangles.push([1, 3, 2]);
    let mut written = sections.clone();
    write_geometry(&mut written, Rpf9Role::Drawable, None, AT, &edit).expect("written");
    let back = geometry_in(&written);
    assert_eq!(back.vertex_count, 4);
    assert_eq!(back.vertices.reference[..3], held[..]);
    assert_eq!(back.vertices.reference[3], moved(&[1.0, 1.0, 0.0]));

    // A vertex whose lane 0 clears bit 15 holds lanes no writer derives.
    let mut clear = lanes.clone();
    clear[0][1] = 0;
    let sections = with_reference(&clear);
    let mut again = sections.clone();
    let refused = write_geometry(&mut again, Rpf9Role::Drawable, None, AT, &edit);
    assert!(
        matches!(
            refused,
            Err(GeometryWriteError::Channel { channel: 33, .. })
        ),
        "{refused:?}"
    );
    assert_eq!(again, sections);
}
