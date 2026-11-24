use super::*;

const CELL: u32 = 160 + 512 * 179;

/// Where a built navigation mesh's parts lie.
struct MeshParts {
    polygons: usize,
    adjacency: usize,
    tree: usize,
}

/// A split array of `items`, each `stride` bytes, in chunks of `16384 / stride` items.
fn split_into(img: &mut Image, items: &[u8], stride: usize) -> usize {
    let header = img.alloc(0x30);
    let chunks: Vec<&[u8]> = items.chunks(16384 / stride * stride).collect();
    let table = img.alloc(16 * chunks.len());
    let starts = img.alloc(4 * chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let data = img.alloc(chunk.len());
        img.put(data, chunk);
        img.ptr(table + 16 * i, data);
        img.u64(table + 16 * i + 8, (chunk.len() / stride) as u64);
        img.u32(starts + 4 * i, (i * (16384 / stride)) as u32);
    }
    img.u64(header + 0x08, (items.len() / stride) as u64);
    img.ptr(header + 0x10, table);
    img.ptr(header + 0x18, starts);
    img.u64(header + 0x20, chunks.len() as u64);
    header
}

/// A cell of one triangle and one two-corner side record, in a quadtree of one leaf.
fn navmesh() -> (Image, MeshParts) {
    let mut img = Image::default();
    let root = img.alloc(0xF0);
    let map = img.alloc(16);
    img.bytes[map + 8] = 1;
    img.ptr(root + 0x08, map);
    img.u32(root + 0x10, 9);
    img.u32(root + 0x14, 0x0001_0015);
    for row in 0..3 {
        img.put(root + 0x20 + 20 * row, &1.0f32.to_le_bytes());
    }
    let vertices: Vec<u8> = [[0u16, 0, 0], [65535, 0, 0], [0, 65535, 0]]
        .iter()
        .flatten()
        .flat_map(|q| q.to_le_bytes())
        .collect();
    let corners = [0u16, 1, 2, 0, 1];
    let indices: Vec<u8> = corners.iter().flat_map(|i| i.to_le_bytes()).collect();
    let mut adjacency = Vec::new();
    for _ in corners {
        adjacency.extend(0xFFFF_FFFFu32.to_le_bytes());
        adjacency.extend([0u8; 8]);
    }
    let mut polygons = vec![0u8; 88];
    polygons[4..8].copy_from_slice(&3u32.to_le_bytes());
    polygons[44 + 4..44 + 8].copy_from_slice(&(3u32 << 4 | 2).to_le_bytes());
    polygons[44 + 0x18..44 + 0x1C].copy_from_slice(&0x8800u32.to_le_bytes());
    polygons[0x1C] = 0x55;
    let list = split_into(&mut img, &vertices, 6);
    img.ptr(root + 0x70, list);
    let list = split_into(&mut img, &indices, 2);
    img.ptr(root + 0x80, list);
    let list = split_into(&mut img, &adjacency, 12);
    img.ptr(root + 0x88, list);
    let adjacency = list;
    let list = split_into(&mut img, &polygons, 44);
    img.ptr(root + 0x98, list);
    let polygons = list;
    img.u32(root + 0x90, 5);
    let tree = img.alloc(0x60);
    img.vec3(tree, [-12288.0, -9856.0, 0.0]);
    img.vec3(tree + 0x10, [-12160.0, -9728.0, 4.0]);
    let leaf = img.alloc(0x20);
    let ids = img.alloc(2);
    img.ptr(leaf + 0x08, ids);
    img.u16(leaf + 0x18, 1);
    img.ptr(tree + 0x2C, leaf);
    img.ptr(root + 0xA0, tree);
    img.u32(root + 0xC0, 3);
    img.u32(root + 0xC4, 2);
    img.u32(root + 0xC8, CELL);
    img.u32(root + 0xCC, 6 * 3 + 14 * 5 + 44 * 2);
    img.u32(root + 0xE8, 0x0102_0304);
    (
        img,
        MeshParts {
            polygons,
            adjacency,
            tree,
        },
    )
}

/// The first item of the split array whose header is at `header`.
fn first_item(img: &Image, header: usize) -> usize {
    let table = u64_at(&img.bytes, header + 0x10) as usize & 0x0FFF_FFFF;
    u64_at(&img.bytes, table) as usize & 0x0FFF_FFFF
}

#[test]
fn a_navigation_mesh_reads_through_its_role_to_its_vertices_polygons_and_leaves() {
    let (img, _) = navmesh();
    let signature = Rpf9Signature::of(&img.bytes, img.bytes.len() as u64).expect("a signature");
    assert_eq!(signature.role(), Some(Rpf9Role::NavMesh), "{signature}");
    let Ok(Resource::NavMesh(mesh)) = read(signature, &img.bytes, Some(&[])) else {
        panic!("a navigation mesh");
    };
    assert_eq!(
        (mesh.cell, mesh.flags, mesh.build_id),
        (CELL, 9, 0x0102_0304)
    );
    assert_eq!(mesh.vertices[1], [65535, 0, 0]);
    assert_eq!(mesh.indices, [0, 1, 2, 0, 1]);
    assert_eq!(mesh.edges[0].mesh, NOWHERE);
    assert_eq!(mesh.polygons.len(), 2);
    assert_eq!((mesh.polygons[1].first, mesh.polygons[1].corners), (3, 2));
    assert_eq!(mesh.polygons[0].centroid, [0x55, 0]);
    assert_eq!(mesh.polygons_proper, 1);
    assert_eq!((mesh.nodes, mesh.leaves), (1, 1));
    assert_eq!(mesh.tree.polygons.as_deref(), Some(&[0][..]));
    assert_eq!(mesh.tree.box_max, [-12160.0, -9728.0, 4.0]);
}

#[test]
fn a_memory_total_that_is_not_the_sum_of_the_arrays_is_refused() {
    let (mut img, _) = navmesh();
    img.u32(0xCC, 0);
    assert!(matches!(
        read_navmesh(&img.bytes),
        Err(HeaderlessError::Disagree { second: 0, .. })
    ));
}

#[test]
fn a_polygon_whose_first_index_is_not_the_corners_before_it_is_refused() {
    let (mut img, parts) = navmesh();
    let at = first_item(&img, parts.polygons);
    img.u32(at + 44 + 4, 4 << 4 | 2);
    assert!(matches!(
        read_navmesh(&img.bytes),
        Err(HeaderlessError::Disagree {
            first: 3,
            second: 4,
            ..
        })
    ));
}

#[test]
fn an_edge_into_another_cell_is_refused() {
    let (mut img, parts) = navmesh();
    let at = first_item(&img, parts.adjacency);
    img.u32(at, CELL + 1);
    assert!(matches!(
        read_navmesh(&img.bytes),
        Err(HeaderlessError::Disagree { .. })
    ));
}

#[test]
fn a_record_without_the_side_bits_is_a_polygon_proper_and_refused_with_two_corners() {
    let (mut img, parts) = navmesh();
    let at = first_item(&img, parts.polygons);
    img.u32(at + 44 + 0x18, 0x0800);
    assert!(matches!(
        read_navmesh(&img.bytes),
        Err(HeaderlessError::Constant { found: 2, .. })
    ));
}

/// A cell of `proper` triangles, then one side record. Polygon 0 runs over vertices 0, 1, 2 and
/// its first edge names number `id`; polygon `holder` runs over 1, 0, 5 and every other over 3,
/// 4, 5. The leaf names polygon numbers `leaf`.
fn wide_cell(proper: usize, id: u16, holder: usize, leaf: &[u16]) -> Vec<u8> {
    let mut img = Image::default();
    let root = img.alloc(0xF0);
    // Room for both blocks a section past 64 KiB is cut into, as a written body's map holds.
    let map = img.alloc(0x20);
    img.ptr(root + 0x08, map);
    img.u32(root + 0x10, 9);
    img.u32(root + 0x14, 0x0001_0015);
    let vertices: Vec<u8> = (0..6u16)
        .flat_map(|i| [i * 100, i * 50, 0])
        .flat_map(|q| q.to_le_bytes())
        .collect();
    let (mut indices, mut adjacency, mut polygons) = (Vec::new(), Vec::new(), Vec::new());
    let mut first = 0u32;
    for p in 0..=proper {
        let corners: &[u16] = match p {
            0 => &[0, 1, 2],
            _ if p == holder => &[1, 0, 5],
            _ if p == proper => &[3, 4],
            _ => &[3, 4, 5],
        };
        for (k, c) in corners.iter().enumerate() {
            indices.extend(c.to_le_bytes());
            let named = p == 0 && k == 0;
            adjacency.extend(if named { CELL } else { NOWHERE }.to_le_bytes());
            adjacency.extend(if named { id } else { 0 }.to_le_bytes());
            adjacency.extend([0u8; 6]);
        }
        let mut record = [0u8; 44];
        record[4..8].copy_from_slice(&(first << 4 | corners.len() as u32).to_le_bytes());
        for (k, m) in [-12288i16, -12160, -9856, -9728, 0, 4].iter().enumerate() {
            record[0xC + 2 * k..0xE + 2 * k].copy_from_slice(&m.to_le_bytes());
        }
        if p == proper {
            record[0x18..0x1C].copy_from_slice(&SIDE_RECORD.to_le_bytes());
        }
        polygons.extend(record);
        first += corners.len() as u32;
    }
    for (slot, items, stride) in [
        (0x70, &vertices, 6),
        (0x80, &indices, 2),
        (0x88, &adjacency, 12),
        (0x98, &polygons, 44),
    ] {
        let list = split_into(&mut img, items, stride);
        img.ptr(root + slot, list);
    }
    img.u32(root + 0x90, first);
    let tree = img.alloc(0x60);
    img.vec3(tree, [-12288.0, -9856.0, 0.0]);
    img.vec3(tree + 0x10, [-12160.0, -9728.0, 4.0]);
    let node = img.alloc(0x20);
    let ids = img.alloc(2 * leaf.len());
    for (i, id) in leaf.iter().enumerate() {
        img.u16(ids + 2 * i, *id);
    }
    img.ptr(node + 0x08, ids);
    img.u16(node + 0x18, leaf.len() as u16);
    img.ptr(tree + 0x2C, node);
    img.ptr(root + 0xA0, tree);
    img.u32(root + 0xC0, 6);
    img.u32(root + 0xC4, proper as u32 + 1);
    img.u32(root + 0xC8, CELL);
    img.u32(
        root + 0xCC,
        (6 * 6 + 14 * first as usize + 44 * (proper + 1)) as u32,
    );
    // A section is stored in 16-byte steps, its whole 64 KiB pieces one block, the rest another.
    let len = img.bytes.len().next_multiple_of(16);
    img.bytes.resize(len, 0);
    img.bytes[map + 8] = u8::from(len >= 0x1_0000) + u8::from(len % 0x1_0000 != 0);
    img.bytes
}

#[test]
fn a_mesh_of_more_than_65_536_polygons_ends_them_at_its_first_side_record_and_resolves_numbers() {
    let proper = 65_536 + 10;
    // Number 4 names polygons 4 and 65,540; the second holds both vertices of polygon 0's edge.
    let mesh = read_navmesh(&wide_cell(proper, 4, 65_540, &[0, 4])).expect("it reads");
    assert_eq!(mesh.polygons_proper, proper);
    assert_eq!(mesh.polygons.len(), proper + 1);
    assert_eq!(mesh.edges[0].polygon, 4);
    assert_eq!(mesh.neighbour(0), Some(65_540));
    assert_eq!(mesh.neighbour(1), None, "an edge that leads nowhere");
    let leaf = mesh.tree.polygons.as_deref();
    assert_eq!(leaf, Some(&[0, 4][..]));
    assert_eq!(mesh.leaf_polygons(&mesh.tree), [0, 4, 65_536, 65_540]);
    // A number that names no polygon holding the edge's vertices resolves to none.
    let none = read_navmesh(&wide_cell(proper, 7, 3, &[0])).expect("it reads");
    assert_eq!(
        none.neighbour(0),
        None,
        "neither 7 nor 65,543 holds the edge"
    );
    let both = read_navmesh(&wide_cell(proper, 4, 4, &[0])).expect("it reads");
    assert_eq!(both.neighbour(0), Some(4));
    // Below 65,537 polygons a number names one polygon, whatever its vertices.
    let small = read_navmesh(&wide_cell(20, 4, 9, &[0])).expect("it reads");
    assert_eq!(small.neighbour(0), Some(4));
    // A leaf's number past the polygons proper is refused.
    assert!(read_navmesh(&wide_cell(20, 4, 9, &[20])).is_err());
}

#[test]
fn a_link_s_heading_a_footprint_and_the_open_words_read_raw_where_they_lie() {
    let (mut img, parts) = navmesh();
    let link = img.alloc(32);
    img.u32(link + 0x18, 0x0072_5B03);
    img.u32(link + 0x1C, 0x5B);
    img.ptr(0xA8, link);
    img.u32(0xD4, 1);
    img.u32(0xCC, 6 * 3 + 14 * 5 + 44 * 2 + 32);
    let footprint = img.alloc(96);
    img.vec3(footprint + 8, [10.0, -20.0, 3.0]);
    img.u16(footprint + 0x14, 0x0408);
    for (k, [dx, dy]) in [
        [512i16, 256],
        [-1024, 128],
        [-256, -512],
        [100, -50],
        [7, 9],
    ]
    .iter()
    .enumerate()
    {
        img.u16(footprint + 0x16 + 4 * k, *dx as u16);
        img.u16(footprint + 0x18 + 4 * k, *dy as u16);
    }
    img.ptr(0xB8, footprint);
    img.u32(0xDC, 1);
    let edges = first_item(&img, parts.adjacency);
    img.u16(edges + 6, 0xBEEF);
    img.u32(edges + 8, 0x2C61_9E47);
    let polygons = first_item(&img, parts.polygons);
    img.bytes[polygons + 0x25] = 49;
    let mesh = read_navmesh(&img.bytes).expect("the mesh reads");
    let [link] = mesh.special_links.as_slice() else {
        panic!("one link");
    };
    assert_eq!((link.word_18, link.heading), (0x0072_5B03, 0x5B));
    assert_eq!(link.heading_degrees(), 91.0 * 360.0 / 256.0);
    assert_eq!(
        (mesh.edges[0].word_06, mesh.edges[0].flags),
        (0xBEEF, 0x2C61_9E47)
    );
    assert_eq!(mesh.polygons[0].steepness, 49);
    let [footprint] = mesh.footprints.as_slice() else {
        panic!("one footprint");
    };
    let unit = FOOTPRINT_UNIT;
    let corners = |outline: &[[f32; 2]]| -> Vec<[f32; 3]> {
        (outline.iter())
            .map(|[dx, dy]| [10.0 + dx * unit, -20.0 + dy * unit, 3.0])
            .collect()
    };
    let four = [
        [512.0, 256.0],
        [-1024.0, 128.0],
        [-256.0, -512.0],
        [100.0, -50.0],
    ];
    assert_eq!(footprint.corners(), corners(&four));
    assert_eq!(footprint.corners()[0], [11.0, -19.5, 3.0], "1/512 m a unit");
    let mut eight = *footprint;
    eight.kind = EIGHT_CORNERS;
    let mut all = four.to_vec();
    all.push([7.0, 9.0]);
    all.extend([[0.0; 2]; 3]);
    assert_eq!(eight.corners(), corners(&all));
}

/// The box of the cell whose south-west corner is `min`, 128 m a side.
fn cell_box(min: [f32; 2]) -> NavNode {
    NavNode {
        box_min: [min[0], min[1], 0.0],
        box_max: [min[0] + 128.0, min[1] + 128.0, 8.0],
        polygons: None,
        children: Default::default(),
    }
}

/// A side record's height, its corners as x and y, and the polygon proper each chain edge names.
type Record<'a> = (f32, &'a [[f32; 2]], &'a [u16]);

/// A cell of three polygons proper, only named, and the side records `records`.
fn cell(cell: u32, min: [f32; 2], records: &[Record]) -> NavMesh {
    let tree = cell_box(min);
    let quantise = |p: [f32; 3]| -> [u16; 3] {
        std::array::from_fn(|a| {
            let size = tree.box_max[a] - tree.box_min[a];
            ((p[a] - tree.box_min[a]) / size * 65535.0).round() as u16
        })
    };
    let polygon = |first: u32, corners: u8, side_flags: u32| NavPolygon {
        first,
        corners,
        flags: 0,
        min_max: [0; 6],
        centroid: [0; 2],
        side_flags,
        steepness: 0,
        word_20: 0,
    };
    let (mut vertices, mut edges) = (Vec::new(), Vec::new());
    let mut polygons = vec![polygon(0, 0, 0); 3];
    for (z, corners, names) in records {
        polygons.push(polygon(
            vertices.len() as u32,
            corners.len() as u8,
            SIDE_RECORD,
        ));
        for (k, [x, y]) in corners.iter().enumerate() {
            vertices.push(quantise([*x, *y, *z]));
            let name = k.checked_sub(1).map(|j| names[j]);
            edges.push(NavEdge {
                mesh: name.map_or(NOWHERE, |_| cell),
                polygon: name.unwrap_or(0),
                word_06: 0,
                flags: 0,
            });
        }
    }
    NavMesh {
        flags: 9,
        cell,
        build_id: 0,
        tree,
        indices: (0..vertices.len() as u16).collect(),
        vertices,
        edges,
        polygons,
        polygons_proper: 3,
        nodes: 1,
        depth: 0,
        leaves: 1,
        special_links: Vec::new(),
        special_link_indices: Vec::new(),
        footprints: Vec::new(),
    }
}

#[test]
fn a_join_across_two_cells_names_the_polygon_on_the_other_side_at_its_own_height() {
    let (west, east) = ([-12288.0, -9856.0], [-12160.0, -9856.0]);
    let line = east[0];
    // Polygons 0 and 1 meet the border at height 1, polygon 2 over polygon 0 at height 5.
    let a = cell(
        CELL,
        west,
        &[
            (1.0, &[[line - 0.06, -9856.0], [line, -9852.0]], &[0]),
            (1.0, &[[line, -9852.0], [line, -9848.0]], &[1]),
            (5.0, &[[line - 0.06, -9856.0], [line, -9852.0]], &[2]),
        ],
    );
    // The cell east of it cuts its first slot at -9853, between its polygons 0 and 1.
    let b = cell(
        CELL + 1,
        east,
        &[
            (
                1.0,
                &[[line + 0.06, -9856.0], [line, -9852.0], [line, -9853.0]],
                &[1, 0],
            ),
            (1.0, &[[line, -9852.0], [line, -9848.0]], &[1]),
            (5.0, &[[line + 0.06, -9856.0], [line, -9852.0]], &[2]),
        ],
    );
    let named: Vec<_> = (a.border_edges().iter())
        .map(|e| (e.border, e.polygon))
        .collect();
    assert_eq!(
        named,
        [
            (Border::East, Some(0)),
            (Border::East, Some(1)),
            (Border::East, Some(2))
        ]
    );
    let join = |polygon, other| Join {
        border: Border::East,
        polygon,
        other,
    };
    assert_eq!(
        joins(&a, &b),
        [join(0, 0), join(0, 1), join(1, 1), join(2, 2)]
    );
    let back: Vec<_> = joins(&b, &a)
        .iter()
        .map(|j| (j.border, j.polygon, j.other))
        .collect();
    assert_eq!(
        back,
        [
            (Border::West, 0, 0),
            (Border::West, 1, 0),
            (Border::West, 1, 1),
            (Border::West, 2, 2)
        ]
    );
    let mut north = b;
    north.cell = CELL + 512;
    assert!(joins(&a, &north).is_empty(), "no shared border");
}

#[test]
fn a_quadtree_that_loops_is_refused() {
    let (mut img, parts) = navmesh();
    img.ptr(parts.tree + 0x34, parts.tree);
    assert!(matches!(
        read_navmesh(&img.bytes),
        Err(HeaderlessError::TreeTooLarge { .. })
    ));
}

#[test]
fn a_polygon_s_flags_write_in_place_but_for_the_bits_kept_and_another_count_is_refused() {
    let (mut img, parts) = navmesh();
    let first = first_item(&img, parts.polygons);
    img.u32(first + 8, 1 << 21);
    let original = img.bytes.clone();
    let mut bytes = original.clone();
    write_polygon_flags(&mut bytes, &[1 << 21, 0], 1 << 21).expect("nothing edited writes");
    assert_eq!(bytes, original);
    write_polygon_flags(&mut bytes, &[WATER | SMALL, 0], 1 << 21).expect("the flags write");
    let read = read_navmesh(&bytes).expect("it reads back");
    let flags: Vec<u32> = read.polygons.iter().map(|p| p.flags).collect();
    assert_eq!(flags, [WATER | SMALL | 1 << 21, 0]);
    assert_eq!(
        u32_at(&bytes, first + 44 + 8),
        0,
        "the side record keeps its flags"
    );
    assert!(matches!(
        write_polygon_flags(&mut bytes, &[0], 0),
        Err(HeaderlessError::NotInPlace(_))
    ));
}

#[test]
fn a_cell_s_links_end_across_its_four_borders_and_at_its_four_corners_inside_the_grid() {
    let c = 10 + 512 * 20;
    let (e, w, n, s) = (c + 1, c - 1, c + 512, c - 512);
    let corners = [e + 512, e - 512, w + 512, w - 512];
    assert_eq!(Border::around(c), [[e, w, n, s], corners].concat());
    assert_eq!(Border::around(0), [1, 512, 513], "the grid's first cell");
    let last = 511 + 512 * 511;
    assert_eq!(
        Border::around(last),
        [last - 1, last - 512, last - 513],
        "the grid's last cell"
    );
}

#[path = "write_navmesh.rs"]
mod write_navmesh;

#[path = "write_navmesh_delete.rs"]
mod write_navmesh_delete;
