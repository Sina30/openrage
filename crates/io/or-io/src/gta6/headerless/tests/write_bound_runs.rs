use super::*;
use crate::gta6::headerless::write_bound_runs::runs_encoded;

/// Two keys whose low halves ascend.
const KEYS: [u64; 2] = [0x3F80_0000_0000_0010, 0x4000_0000_0000_0020];

fn u16_at(v: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([v[at], v[at + 1]])
}

/// `bytes` laid out past the end of `s`'s virtual section, 16-aligned and zero to the next 16.
fn appended(s: &mut Rpf9Sections, bytes: &[u8]) -> usize {
    let v = &mut s.virtual_section;
    let at = v.len().next_multiple_of(16);
    v.resize(at + bytes.len().next_multiple_of(16), 0);
    v[at..at + bytes.len()].copy_from_slice(bytes);
    at
}

/// [`two_triangles`] whose child holds `keys` and the runs `pairs`; the sections, where the child
/// lies and where its runs lie.
fn keyed(keys: &[u64], pairs: &[(u16, u16)]) -> (Rpf9Sections, usize, usize) {
    let (mut s, child, _) = two_triangles();
    let table: Vec<u8> = keys.iter().flat_map(|k| k.to_le_bytes()).collect();
    let runs: Vec<u8> = (pairs.iter())
        .flat_map(|(k, n)| [*k, *n])
        .flat_map(u16::to_le_bytes)
        .collect();
    let t = appended(&mut s, &table);
    let r = appended(&mut s, &runs);
    let v = &mut s.virtual_section;
    v[child + 0x138..child + 0x140].copy_from_slice(&(0x5000_0000 + t as u64).to_le_bytes());
    v[child + 0x140..child + 0x142].copy_from_slice(&(keys.len() as u16).to_le_bytes());
    v[child + 0x144..child + 0x148].copy_from_slice(&(2 * pairs.len() as u32).to_le_bytes());
    v[child + 0x148..child + 0x150].copy_from_slice(&(0x5000_0000 + r as u64).to_le_bytes());
    (s, child, r)
}

/// The (key index, run) pairs the geometry at `child` holds.
fn pairs_of(s: &Rpf9Sections, child: usize) -> Vec<(u16, u16)> {
    let v = &s.virtual_section;
    let words = u32_at(v, child + 0x144) as usize;
    let at = target(v, child + 0x148);
    (0..words / 2)
        .map(|k| (u16_at(v, at + 4 * k), u16_at(v, at + 4 * k + 2)))
        .collect()
}

/// The keys the geometry at `child` holds.
fn keys_of(s: &Rpf9Sections, child: usize) -> Vec<u64> {
    let v = &s.virtual_section;
    let count = usize::from(u16_at(v, child + 0x140));
    let at = target(v, child + 0x138);
    (0..count).map(|k| u64_at(v, at + 8 * k)).collect()
}

#[test]
fn runs_are_maximal_and_a_run_past_a_pair_s_count_is_split() {
    let pairs = |b: Vec<u8>| -> Vec<(u16, u16)> {
        (b.as_chunks::<4>().0.iter())
            .map(|p| {
                (
                    u16::from_le_bytes([p[0], p[1]]),
                    u16::from_le_bytes([p[2], p[3]]),
                )
            })
            .collect()
    };
    assert_eq!(pairs(runs_encoded(&[1, 1, 2, 1])), [(1, 2), (2, 1), (1, 1)]);
    let long: Vec<u16> = std::iter::repeat_n(7, 0x1_0001).collect();
    assert_eq!(pairs(runs_encoded(&long)), [(7, 0xFFFF), (7, 2)]);
    assert!(runs_encoded(&[]).is_empty());
}

#[test]
fn a_deleted_primitive_takes_the_key_no_other_names_and_the_indices_past_it_follow() {
    let (mut s, child, runs) = keyed(&KEYS, &[(0, 1), (1, 1)]);
    let table = target(&s.virtual_section, child + 0x138);
    let op = BoundReshape::DeletePrimitive {
        child: 0,
        primitive: 0,
    };
    reshape_bound_body(&mut s, BoundHolder::Bound, &op).expect("taken out where it stands");
    assert_eq!(
        pairs_of(&s, child),
        [(0, 1)],
        "the second key is now the first"
    );
    assert_eq!(keys_of(&s, child), [KEYS[1]]);
    let v = &s.virtual_section;
    assert_eq!(target(v, child + 0x148), runs, "the runs shrink in place");
    assert_eq!(u64_at(v, table + 8), 0, "the freed entry is zeroed");
    assert_eq!(u32_at(v, runs + 4), 0, "the freed pair is zeroed");
    only_geometry(v).expect("it reads back");

    // A key another primitive names stays.
    let (mut s, child, _) = keyed(&KEYS[..1], &[(0, 2)]);
    let op = BoundReshape::DeletePrimitive {
        child: 0,
        primitive: 1,
    };
    reshape_bound_body(&mut s, BoundHolder::Bound, &op).expect("taken out where it stands");
    assert_eq!(pairs_of(&s, child), [(0, 1)]);
    assert_eq!(keys_of(&s, child), [KEYS[0]]);
}

#[test]
fn an_added_primitive_takes_a_linked_triangle_s_key_else_the_last_one_s() {
    let (mut s, child, runs) = keyed(&KEYS, &[(0, 1), (1, 1)]);
    // Linked across its first edge to triangle 0, whose key is not the last primitive's.
    let record = Primitive::Triangle {
        vertices: [0, 2, 3],
        neighbours: [0, OPEN, OPEN],
        edges: [9, 0xFF, 0xFF],
    };
    let op = BoundReshape::AddPrimitive {
        child: 0,
        record,
        material: 0,
    };
    reshape_bound_body(&mut s, BoundHolder::Bound, &op).expect("laid out past the end");
    assert_eq!(pairs_of(&s, child), [(0, 1), (1, 1), (0, 1)]);
    assert_eq!(keys_of(&s, child), KEYS, "no key is added");
    assert_eq!(
        target(&s.virtual_section, child + 0x148),
        runs,
        "a pair more fits the runs' 16 bytes"
    );
    let sphere = Primitive::Sphere {
        centre: 0,
        radius: 0.5,
    };
    let op = BoundReshape::AddPrimitive {
        child: 0,
        record: sphere,
        material: 0,
    };
    reshape_bound_body(&mut s, BoundHolder::Bound, &op).expect("laid out past the end");
    assert_eq!(
        pairs_of(&s, child),
        [(0, 1), (1, 1), (0, 2)],
        "the last one's"
    );
    only_geometry(&s.virtual_section).expect("it reads back");

    // Runs whose next 16 bytes hold something are laid out anew.
    let (mut s, child, runs) = keyed(&KEYS, &[(0, 1), (1, 1)]);
    s.virtual_section[runs + 8] = 0xAB;
    let op = BoundReshape::AddPrimitive {
        child: 0,
        record,
        material: 0,
    };
    reshape_bound_body(&mut s, BoundHolder::Bound, &op).expect("laid out past the end");
    assert_ne!(target(&s.virtual_section, child + 0x148), runs);
    assert_eq!(pairs_of(&s, child), [(0, 1), (1, 1), (0, 1)]);
    assert_eq!(s.virtual_section[runs + 8], 0xAB, "what lay there is kept");
}

#[test]
fn runs_that_do_not_give_each_primitive_one_key_refuse_a_primitive_edit() {
    let refused = |s: &mut Rpf9Sections, op: &BoundReshape| {
        let kept = s.clone();
        let err = reshape_bound_body(s, BoundHolder::Bound, op).expect_err("refused");
        assert_eq!(*s, kept, "{err}");
        err.to_string()
    };
    let delete = BoundReshape::DeletePrimitive {
        child: 0,
        primitive: 1,
    };
    let add = BoundReshape::AddPrimitive {
        child: 0,
        record: Primitive::Sphere {
            centre: 0,
            radius: 0.5,
        },
        material: 0,
    };
    let (mut s, ..) = keyed(&KEYS, &[(0, 2), (1, 1)]);
    assert!(refused(&mut s, &delete).contains("more primitives"));
    assert!(refused(&mut s, &add).contains("more primitives"));
    let (mut s, ..) = keyed(&KEYS, &[(0, 1)]);
    assert!(refused(&mut s, &add).contains("fewer primitives"));
    let (mut s, child, _) = keyed(&KEYS, &[(0, 1), (1, 1)]);
    s.virtual_section[child + 0x148..child + 0x150].fill(0);
    assert!(refused(&mut s, &delete).contains("both null"));
    // A vertex edit leaves the runs.
    let (mut s, child, _) = keyed(&KEYS, &[(0, 1), (1, 1)]);
    let op = BoundReshape::AddVertex { child: 0 };
    reshape_bound_body(&mut s, BoundHolder::Bound, &op).expect("laid out past the end");
    assert_eq!(pairs_of(&s, child), [(0, 1), (1, 1)]);
}

#[test]
fn a_copied_geometry_child_holds_arrays_of_its_own() {
    let (mut s, child, _) = keyed(&KEYS, &[(0, 1), (1, 1)]);
    let mut edited = read_bound(&s.virtual_section).expect("the bound reads");
    assert_eq!(
        copy_bound_child(&mut edited, 0).expect("a geometry with a BVH"),
        1
    );
    let op = BoundReshape::CopyChild { child: 0 };
    reshape_bound_body(&mut s, BoundHolder::Bound, &op).expect("laid out past the end");
    let read = read_bound(&s.virtual_section).expect("it reads back");
    assert_eq!(read.children.len(), 2);
    let geometry = |i: usize| match &read.children[i].bound {
        Some(Ok(ChildBound::Geometry(g))) => (g.vertices.clone(), g.primitives.clone()),
        other => panic!("a geometry child, not {other:?}"),
    };
    assert_eq!(geometry(1), geometry(0));
    let v = &s.virtual_section;
    let copy = target(v, target(v, 0x70) + 8);
    assert_ne!(copy, child);
    for (word, len) in [(0x88, 32), (0xB0, 24), (0x138, 16), (0x148, 8)] {
        let (a, b) = (target(v, child + word), target(v, copy + word));
        assert_ne!(a, b, "+{word:#X} names an array of the copy's own");
        assert_eq!(
            v[a..a + len],
            v[b..b + len],
            "+{word:#X} holds the same bytes"
        );
    }
    let (a, b) = (target(v, child + 0x130), target(v, copy + 0x130));
    assert_ne!(a, b);
    let (na, nb) = (target(v, a + 0x50), target(v, b + 0x50));
    assert_ne!(na, nb);
    assert_eq!(v[na..na + 48], v[nb..nb + 48], "the BVH's three nodes");

    // An edit of the copy leaves its source.
    let op = BoundReshape::DeletePrimitive {
        child: 1,
        primitive: 0,
    };
    reshape_bound_body(&mut s, BoundHolder::Bound, &op).expect("taken out where it stands");
    let v = &s.virtual_section;
    let copy = target(v, target(v, 0x70) + 8);
    assert_eq!((u32_at(v, child + 0xD4), u32_at(v, copy + 0xD4)), (2, 1));
    assert_eq!(pairs_of(&s, child), [(0, 1), (1, 1)]);
    assert_eq!(keys_of(&s, child), KEYS);
    assert_eq!(pairs_of(&s, copy), [(0, 1)]);
    assert_eq!(keys_of(&s, copy), [KEYS[1]]);
}

#[test]
fn a_geometry_child_naming_bytes_no_reader_lays_out_is_not_copied() {
    let (mut s, child, runs) = keyed(&KEYS, &[(0, 1), (1, 1)]);
    s.virtual_section[child + 0xF8..child + 0x100]
        .copy_from_slice(&(0x5000_0000 + runs as u64).to_le_bytes());
    let kept = s.clone();
    let op = BoundReshape::CopyChild { child: 0 };
    let err = reshape_bound_body(&mut s, BoundHolder::Bound, &op).expect_err("refused");
    assert!(err.to_string().contains("no reader lays out"), "{err}");
    assert_eq!(s, kept);
}

/// A vehicle glass window of `child` whose map is `rows` blank rows.
fn pane(child: u16, rows: u16) -> GlassWindow {
    let mut header = [0u8; 0x70];
    header[0x40..0x44].copy_from_slice(b"CWGV");
    header[0x44..0x46].copy_from_slice(&child.to_le_bytes());
    header[0x48..0x4A].copy_from_slice(&1u16.to_le_bytes());
    header[0x4A..0x4C].copy_from_slice(&rows.to_le_bytes());
    // Each row's offset, then its one byte.
    header[0x4C..0x4E].copy_from_slice(&(3 * rows).to_le_bytes());
    GlassWindow {
        header,
        rows: vec![GlassRow::Blank; usize::from(rows)],
    }
}

#[test]
fn vehicle_glass_windows_of_other_lengths_move_whole_when_renumbered() {
    let starts = |g: &VehicleGlass| -> Vec<(u32, u32)> {
        let b = g.encode();
        (0..g.windows.len())
            .map(|k| (u32_at(&b, 12 + 8 * k), u32_at(&b, 16 + 8 * k)))
            .collect()
    };
    // The first window follows a table of an even number of slots and one word; each is its
    // header and map to the next 16.
    let first = 12 + 8 * 2 + 4;
    let len = |rows: u32| (0x70 + 3 * rows).next_multiple_of(16);
    assert_ne!(len(1), len(20), "no 0x70 grid");
    let glass = VehicleGlass {
        stride: 0x70,
        after_offsets: 0,
        windows: vec![pane(0, 1), pane(2, 20)],
    };
    assert_eq!(starts(&glass), [(0, first), (2, first + len(1))]);
    let total = first + len(1) + len(20);
    assert_eq!(u32_at(&glass.encode(), 8), total);

    // Child 2 renumbered 1 goes first, child 0 renumbered 2 after it.
    let mut renumbered = glass.clone();
    renumbered.windows.swap(0, 1);
    for (w, id) in renumbered.windows.iter_mut().zip([1u16, 2]) {
        w.header[0x44..0x46].copy_from_slice(&id.to_le_bytes());
    }
    assert_eq!(starts(&renumbered), [(1, first), (2, first + len(20))]);
    assert_eq!(
        u32_at(&renumbered.encode(), 8),
        total,
        "the block keeps its length"
    );
    assert_eq!(renumbered.encode().len(), glass.encode().len());
}
