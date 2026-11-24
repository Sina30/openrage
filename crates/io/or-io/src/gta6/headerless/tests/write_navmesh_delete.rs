use std::io::Cursor;

use crate::container::rpf::{read_rpf9_body, write_rpf9_resource, Rpf9Form, Rpf9Sections};

use super::*;

/// The cell east of [`CELL`].
const OTHER: u32 = CELL + 1;

/// Where a built cell's parts lie.
struct Built {
    img: Image,
    polygons: usize,
    adjacency: usize,
    link_list: usize,
}

/// Each corner run of the built cell: three triangles, then a side record.
const CORNERS: [&[u16]; 4] = [&[0, 1, 2], &[1, 3, 2], &[3, 4, 2], &[4, 5]];

/// Vertex `i` of the built cell.
fn vertex(i: u16) -> [u16; 3] {
    [i * 1000, i * 500, 7]
}

/// The polygon the built cell's record `q` leads to from corner `k`.
fn leads(q: usize, k: usize) -> Option<u16> {
    match (q, k) {
        (1, 1) | (3, 1) => Some(2),
        (2, 2) => Some(1),
        _ => None,
    }
}

/// A special link of the built cell, from polygon `from` of `from_mesh` to polygon `to` of it.
fn link(from: u16, from_mesh: u32, to: u16) -> Vec<u8> {
    let mut l = vec![0u8; 32];
    l[0x0C..0x0E].copy_from_slice(&from.to_le_bytes());
    l[0x0E..0x10].copy_from_slice(&to.to_le_bytes());
    l[0x10..0x14].copy_from_slice(&from_mesh.to_le_bytes());
    l[0x14..0x18].copy_from_slice(&CELL.to_le_bytes());
    l[0x1C..0x20].copy_from_slice(&0x40u32.to_le_bytes());
    l
}

/// [`CORNERS`] over six vertices: polygon 0 has every edge cut, polygons 1 and 2 lead into each
/// other and the side record's corner 1 names polygon 2. Leaf A names polygon 0, leaf B 0, 1 and 2.
/// Link 0 runs from polygon 2 to 1, link 1 is its copy, link 2 runs from polygon 7 of the cell east
/// to polygon 1; they are filed under both ends, an empty run starting at the running total.
fn deletable() -> Built {
    let mut img = Image::default();
    let root = img.alloc(0xF0);
    let map = img.alloc(24);
    img.bytes[map + 8] = 1;
    img.ptr(root + 0x08, map);
    img.u32(root + 0x10, 9);
    img.u32(root + 0x14, 0x0001_0015);
    let vertices: Vec<u8> = (0..6u16)
        .flat_map(vertex)
        .flat_map(|q| q.to_le_bytes())
        .collect();
    // Polygon 1's run of 2, polygon 2's of 1 after it and bit 23, which is kept as read.
    let runs: [u32; 4] = [0, 2, (1 << 23) | (2 << 6) | 1, 3 << 6];
    let (mut indices, mut adjacency, mut polygons) = (Vec::new(), Vec::new(), Vec::new());
    let mut first = 0u32;
    for (q, corners) in CORNERS.iter().enumerate() {
        for (k, c) in corners.iter().enumerate() {
            indices.extend(c.to_le_bytes());
            let to = leads(q, k);
            adjacency.extend(to.map_or(NOWHERE, |_| CELL).to_le_bytes());
            adjacency.extend(to.unwrap_or(0xFFFF).to_le_bytes());
            adjacency.extend((0x2A00 + k as u16).to_le_bytes());
            adjacency.extend(0x10u32.to_le_bytes());
        }
        let mut record = [0u8; 44];
        record[4..8].copy_from_slice(&((first << 4) | corners.len() as u32).to_le_bytes());
        record[8..12].copy_from_slice(&SMALL.to_le_bytes());
        for (k, m) in [-12288i16, -12160, -9856, -9728, 0, 4].iter().enumerate() {
            record[0xC + 2 * k..0xE + 2 * k].copy_from_slice(&m.to_le_bytes());
        }
        if q == 3 {
            record[0x18..0x1C].copy_from_slice(&SIDE_RECORD.to_le_bytes());
        }
        record[0x20..0x24].copy_from_slice(&runs[q].to_le_bytes());
        record[0x28..0x2C].copy_from_slice(&(0xA5A5_0000 + q as u32).to_le_bytes());
        polygons.extend(record);
        first += corners.len() as u32;
    }
    let mut lists = [0; 4];
    for (n, (slot, items, stride)) in [
        (0x70, &vertices, 6),
        (0x80, &indices, 2),
        (0x88, &adjacency, 12),
        (0x98, &polygons, 44),
    ]
    .into_iter()
    .enumerate()
    {
        lists[n] = split_into(&mut img, items, stride);
        img.ptr(root + slot, lists[n]);
    }
    img.u32(root + 0x90, first);
    let tree = img.alloc(0x60);
    img.vec3(tree, [-12288.0, -9856.0, 0.0]);
    img.vec3(tree + 0x10, [-12160.0, -9728.0, 4.0]);
    for (i, ids) in [&[0u16][..], &[0, 1, 2]].iter().enumerate() {
        let node = img.alloc(0x60);
        img.vec3(node, [-12288.0, -9856.0, 0.0]);
        img.vec3(node + 0x10, [-12160.0, -9728.0, 4.0]);
        let leaf = img.alloc(0x20);
        let at = img.alloc(2 * ids.len());
        for (k, id) in ids.iter().enumerate() {
            img.u16(at + 2 * k, *id);
        }
        img.ptr(leaf + 0x08, at);
        img.u16(leaf + 0x18, ids.len() as u16);
        img.ptr(node + 0x2C, leaf);
        img.ptr(tree + 0x34 + 8 * i, node);
    }
    img.ptr(root + 0xA0, tree);
    let links = img.alloc(96);
    img.put(
        links,
        &[link(2, CELL, 1), link(2, CELL, 1), link(7, OTHER, 1)].concat(),
    );
    img.ptr(root + 0xA8, links);
    img.u32(root + 0xD4, 3);
    let link_list = img.alloc(6);
    for (k, i) in [0u16, 2, 0].iter().enumerate() {
        img.u16(link_list + 2 * k, *i);
    }
    img.ptr(root + 0xB0, link_list);
    img.u32(root + 0xD8, 3);
    img.u32(root + 0xC0, 6);
    img.u32(root + 0xC4, 4);
    img.u32(root + 0xC8, CELL);
    let total = 6 * 6 + 14 * first as usize + 44 * 4 + 32 * 3;
    img.u32(root + 0xCC, total as u32);
    img.u32(root + 0xE8, 0x0102_0304);
    let end = img.bytes.len().next_multiple_of(16);
    img.bytes.resize(end, 0);
    Built {
        img,
        polygons: lists[3],
        adjacency: lists[2],
        link_list,
    }
}

fn sections_of(img: &Image) -> Rpf9Sections {
    Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: img.bytes.clone(),
        physical: Vec::new(),
    }
}

/// The offset the virtual pointer at `at` aims at.
fn target(v: &[u8], at: usize) -> usize {
    (u64_at(v, at) & 0x0FFF_FFFF) as usize
}

#[test]
fn a_deleted_polygon_s_records_go_the_rest_renumber_and_the_body_reads_back_loose() {
    let built = deletable();
    let mut s = sections_of(&built.img);
    let len = s.virtual_section.len();
    let returned = delete_polygon(&mut s, 0, &[]).expect("deleted");
    let read = read_navmesh(&s.virtual_section).expect("the body reads");
    assert_eq!(returned.polygons, read.polygons);
    assert_eq!((read.polygons.len(), read.polygons_proper), (3, 2));
    // Vertex 0 was polygon 0's alone.
    assert_eq!(read.vertices, (1..6).map(vertex).collect::<Vec<_>>());
    let renumbered: Vec<u16> = (CORNERS[1..].iter().copied().flatten())
        .map(|i| i - 1)
        .collect();
    assert_eq!(read.indices, renumbered);
    let named: Vec<Option<usize>> = (0..read.edges.len()).map(|j| read.neighbour(j)).collect();
    let mut want = Vec::new();
    for (q, corners) in CORNERS.iter().enumerate().skip(1) {
        for k in 0..corners.len() {
            want.push(leads(q, k).map(|t| usize::from(t) - 1));
        }
    }
    assert_eq!(named, want);
    assert_eq!(read.edges[1].word_06, 0x2A01, "+0x06 as read");
    let ends: Vec<_> = (read.special_links.iter())
        .map(|l| (l.from_mesh, l.from_polygon, l.to_polygon))
        .collect();
    assert_eq!(ends, [(CELL, 1, 0), (CELL, 1, 0), (OTHER, 7, 0)]);
    assert_eq!(read.special_link_indices, [0, 2, 0]);
    let runs: Vec<_> = read.polygons.iter().map(|p| p.link_run()).collect();
    assert_eq!(runs, [0..2, 2..3, 3..3]);
    assert_ne!(read.polygons[1].word_20 & (1 << 23), 0, "bit 23 as read");
    let leaf = |i: usize| (read.tree.children[i].as_deref()).and_then(|n| n.polygons.clone());
    assert_eq!((leaf(0), leaf(1)), (Some(vec![]), Some(vec![0, 1])));

    let v = &s.virtual_section;
    let list = Image { bytes: v.clone() };
    let header = target(v, 0x98);
    let record = first_item(&list, header) + 44;
    assert_eq!(
        u32_at(v, record + 0x28),
        0xA5A5_0002,
        "a word not known, kept"
    );
    let node = target(v, target(v, 0xA0) + 0x34);
    assert_eq!(
        u64_at(v, target(v, node + 0x2C) + 0x08),
        0,
        "an emptied leaf's null"
    );
    let total = 6 * 5 + 14 * 8 + 44 * 3 + 32 * 3;
    assert_eq!(u32_at(v, 0xCC), total);
    // Each array's 16-byte allocation shrinks to its new length: items before and after.
    let a16 = |n: usize| n.next_multiple_of(16);
    let shrunk: usize = [
        (6, 6, 5),
        (2, 11, 8),
        (12, 11, 8),
        (44, 4, 3),
        (2, 1, 0),
        (2, 3, 2),
    ]
    .iter()
    .map(|(stride, old, new)| a16(stride * old) - a16(stride * new))
    .sum();
    assert_eq!(v.len(), len - shrunk);

    let file = write_rpf9_resource(&s).expect("written");
    let (find, bytes) = read_rpf9_body(Cursor::new(&file), 0, u64::MAX).expect("it reads");
    let loose = Rpf9Sections::of(&find, &bytes).expect("a resource");
    assert_eq!(loose, s);
}

#[test]
fn an_index_list_rebuilt_by_its_rule_takes_an_entry_more_in_place() {
    let built = deletable();
    let mut img = built.img;
    // Link 2 runs from polygon 2 of the cell east to polygon 2: filed once under 2.
    let links = target(&img.bytes, 0xA8);
    img.u16(links + 64 + 0x0C, 2);
    img.u16(links + 64 + 0x0E, 2);
    for (k, i) in [0u16, 0, 2].iter().enumerate() {
        img.u16(built.link_list + 2 * k, *i);
    }
    let records = first_item(&img, built.polygons);
    for (q, run) in [0, 1, (1 << 23) | (1 << 6) | 2, 3 << 6].iter().enumerate() {
        img.u32(records + 44 * q + 0x20, *run);
    }
    read_navmesh(&img.bytes).expect("the patched cell reads");
    let mut s = sections_of(&img);
    let read = delete_polygon(&mut s, 0, &[]).expect("deleted");
    // Its end in this cell is now polygon 1, its other end still numbers 2.
    assert_eq!(read.special_link_indices, [0, 0, 2, 2]);
    let runs: Vec<_> = read.polygons.iter().map(|p| p.link_run()).collect();
    assert_eq!(runs, [0..1, 1..3, 3..4]);
    assert_eq!(u32_at(&s.virtual_section, 0xD8), 4);
}

#[test]
fn a_deletion_the_rules_do_not_cover_is_refused_by_name_and_changes_nothing() {
    let built = deletable();
    let stored = sections_of(&built.img);
    let refused = |img: &Image, polygon: usize, others: &[&NavMesh]| {
        let mut s = sections_of(img);
        let kept = s.clone();
        let e = delete_polygon(&mut s, polygon, others).expect_err("refused");
        assert_eq!(s, kept, "{e}");
        e
    };
    assert!(matches!(
        refused(&built.img, 3, &[]),
        NavDeleteError::NotProper {
            polygon: 3,
            proper: 3
        }
    ));
    assert!(matches!(
        refused(&built.img, 1, &[]),
        NavDeleteError::EdgeInto {
            from: 2,
            corner: 2,
            polygon: 1
        }
    ));
    let mut same_cell = read_navmesh(&stored.virtual_section).expect("it reads");
    same_cell.special_links.clear();
    let mut s = stored.clone();
    delete_polygon(&mut s, 0, &[&same_cell]).expect("another body of the cell itself");
}

#[test]
fn an_index_list_no_form_gives_is_filed_again_under_both_ends_of_each_link() {
    let built = deletable();
    let mut img = built.img;
    for (k, i) in [2u16, 0, 0].iter().enumerate() {
        img.u16(built.link_list + 2 * k, *i);
    }
    let mut s = sections_of(&img);
    let read = delete_polygon(&mut s, 0, &[]).expect("deleted");
    assert_eq!(read.special_link_indices, [0, 2, 0]);
}

#[test]
fn an_index_list_left_with_no_entry_is_written_null_with_count_0() {
    let built = deletable();
    let mut img = built.img;
    // Every link runs between polygons 3 of the cell east: one link, filed under record 3 alone.
    let links = target(&img.bytes, 0xA8);
    for k in 0..3 {
        img.u16(links + 32 * k + 0x0C, 3);
        img.u16(links + 32 * k + 0x0E, 3);
        img.u32(links + 32 * k + 0x10, OTHER);
        img.u32(links + 32 * k + 0x14, OTHER);
    }
    img.u16(built.link_list, 0);
    img.u32(0xD8, 1);
    let records = first_item(&img, built.polygons);
    for (q, run) in [0u32, 0, 1 << 23, 1].iter().enumerate() {
        img.u32(records + 44 * q + 0x20, *run);
    }
    read_navmesh(&img.bytes).expect("the patched cell reads");
    let mut s = sections_of(&img);
    // Three records stay, so number 3 files under none.
    let read = delete_polygon(&mut s, 0, &[]).expect("deleted");
    assert!(read.special_link_indices.is_empty());
    assert_eq!(read.special_links.len(), 3, "no link is deleted");
    assert!(read.polygons.iter().all(|p| p.link_run().is_empty()));
    assert_ne!(read.polygons[1].word_20 & (1 << 23), 0, "bit 23 as read");
    let v = &s.virtual_section;
    assert_eq!((u64_at(v, 0xB0), u32_at(v, 0xD8)), (0, 0));
    let file = write_rpf9_resource(&s).expect("written");
    let (find, bytes) = read_rpf9_body(Cursor::new(&file), 0, u64::MAX).expect("it reads");
    assert_eq!(Rpf9Sections::of(&find, &bytes).expect("a resource"), s);
}

/// [`deletable`] with polygon 1's edge into 2 cut and links 0 and 1 running from polygon 1.
fn side_only() -> Image {
    let built = deletable();
    let mut img = built.img;
    let edge = first_item(&img, built.adjacency) + 12 * 4;
    img.u32(edge, NOWHERE);
    img.u16(edge + 4, 0xFFFF);
    let links = target(&img.bytes, 0xA8);
    for k in 0..2 {
        img.u16(links + 32 * k + 0x0C, 1);
    }
    read_navmesh(&img.bytes).expect("the patched cell reads");
    img
}

#[test]
fn a_side_record_whose_last_named_piece_led_into_the_polygon_goes_with_it() {
    let mut s = sections_of(&side_only());
    let read = delete_polygon(&mut s, 2, &[]).expect("deleted");
    assert_eq!((read.polygons.len(), read.polygons_proper), (2, 2));
    // Vertices 4 and 5 were polygon 2's and the side record's alone.
    assert_eq!(read.vertices, (0..4).map(vertex).collect::<Vec<_>>());
    assert_eq!(read.indices, [0, 1, 2, 1, 3, 2]);
    // Links 0 and 1 are one link filed once under polygon 1, link 2 under its end there.
    assert_eq!(read.special_link_indices, [0, 2]);
    let runs: Vec<_> = read.polygons.iter().map(|p| p.link_run()).collect();
    assert_eq!(runs, [0..0, 0..2]);
    let leaf = |i: usize| (read.tree.children[i].as_deref()).and_then(|n| n.polygons.clone());
    assert_eq!((leaf(0), leaf(1)), (Some(vec![0]), Some(vec![0, 1])));
}

#[test]
fn a_disable_that_empties_a_side_record_writes_the_cell_without_it() {
    let built = deletable();
    let mut s = sections_of(&built.img);
    let read = disable_polygon(&mut s, 2, &[]).expect("disabled");
    assert_eq!((read.polygons.len(), read.polygons_proper), (3, 3));
    let reaches = |j: usize| read.edges[j].mesh != NOWHERE;
    assert!(!(0..read.edges.len()).any(reaches), "every edge cut");
    assert_eq!(read.edges[8].word_06, 0x2A00, "+0x06's high byte kept");
    assert_eq!(read.vertices, (0..5).map(vertex).collect::<Vec<_>>());
    assert_eq!(read.special_link_indices, [0, 2, 0]);
    assert_ne!(read.polygons[2].word_20 & (1 << 23), 0, "bit 23 as read");

    // Polygon 1 names no side record: its cut moves nothing.
    let m = read_navmesh(&built.img.bytes).expect("it reads");
    let cut = disabling(&m, 1, &[]).expect("disabled");
    assert_eq!(cut.mesh.indices, m.indices);
    assert_eq!(cut.records, [0, 1, 2, 3]);
    assert_eq!(cut.edges, (0..m.edges.len()).collect::<Vec<_>>());
}

#[test]
fn a_split_array_that_empties_its_last_chunk_loses_the_chunk_and_its_row() {
    // 372 polygons a chunk: the 373rd record alone in the second.
    let mut s = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: wide_cell(372, 5, 1, &[0]),
        physical: Vec::new(),
    };
    let read = delete_polygon(&mut s, 3, &[]).expect("deleted");
    assert_eq!((read.polygons.len(), read.polygons_proper), (372, 371));
    assert_eq!(read.neighbour(0), Some(4), "polygon 5 is now 4");
    let header = target(&s.virtual_section, 0x98);
    assert_eq!(u64_at(&s.virtual_section, header + 0x20), 1, "one chunk");
    let file = write_rpf9_resource(&s).expect("written");
    let (find, bytes) = read_rpf9_body(Cursor::new(&file), 0, u64::MAX).expect("it reads");
    assert_eq!(Rpf9Sections::of(&find, &bytes).expect("a resource"), s);
}

#[test]
fn a_cell_past_65_536_records_writes_each_number_modulo_65_536_and_it_resolves_again() {
    let proper = 65_536 + 10;
    // Number 4 names polygons 4 and 65,540, the second the one holding polygon 0's edge.
    let mut s = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: wide_cell(proper, 4, 65_540, &[10]),
        physical: Vec::new(),
    };
    let read = delete_polygon(&mut s, 65_539, &[]).expect("deleted");
    assert_eq!(read.polygons_proper, proper - 1);
    assert_eq!(read.edges[0].polygon, 3, "65,539 modulo 65,536");
    assert_eq!(read.neighbour(0), Some(65_539));
    assert_eq!(read.tree.polygons.as_deref(), Some(&[10][..]));

    // A leaf whose numbers name polygons past the one that goes would name others once renumbered.
    let m = read_navmesh(&wide_cell(proper, 4, 65_540, &[0, 4])).expect("it reads");
    assert!(matches!(
        polygon_deleted(&m, 65_539, &[]),
        Err(NavDeleteError::Ambiguous { index: 65_539, .. })
    ));
}

#[path = "write_navmesh_links.rs"]
mod write_navmesh_links;

#[path = "write_navmesh_no_cell.rs"]
mod write_navmesh_no_cell;
