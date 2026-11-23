use super::*;

/// The adjacency record at index `j` of a built mesh.
fn edge_at(img: &Image, parts: &MeshParts, j: usize) -> usize {
    first_item(img, parts.adjacency) + 12 * j
}

#[test]
fn a_polygon_s_link_run_reads_from_0x20_and_runs_that_do_not_tile_the_list_are_refused() {
    let (mut img, parts) = navmesh();
    let link = img.alloc(32);
    img.ptr(0xA8, link);
    img.u32(0xD4, 1);
    img.u32(0xCC, 6 * 3 + 14 * 5 + 44 * 2 + 32);
    let list = img.alloc(2);
    img.ptr(0xB0, list);
    img.u32(0xD8, 1);
    let polygons = first_item(&img, parts.polygons);
    // A run of one from 0, and bit 23, which is not known.
    img.u32(polygons + 0x20, 1 | 0x0080_0000);
    let mesh = read_navmesh(&img.bytes).expect("the runs tile the list");
    assert_eq!(mesh.polygons[0].link_run(), 0..1);
    assert_eq!(mesh.polygons[0].word_20, 1 | 0x0080_0000);
    assert!(mesh.polygons[1].link_run().is_empty());

    // The side record's run starts at 0 again, where the first ends at 1.
    img.u32(polygons + 44 + 0x20, 1);
    assert!(matches!(
        read_navmesh(&img.bytes),
        Err(HeaderlessError::Disagree {
            first: 0,
            second: 1,
            ..
        })
    ));
    // It starts at 1, but the runs then name two indices of a list of one.
    img.u32(polygons + 44 + 0x20, 1 | (1 << 6));
    assert!(matches!(
        read_navmesh(&img.bytes),
        Err(HeaderlessError::Disagree {
            first: 2,
            second: 1,
            ..
        })
    ));
}

/// The built mesh with the triangle's corner-0 edge into the side record, and the side record's
/// corner-1 edge back into the triangle.
fn joined() -> (Image, MeshParts) {
    let (mut img, parts) = navmesh();
    let out = edge_at(&img, &parts, 0);
    img.u32(out, CELL);
    img.u16(out + 4, 1);
    img.u16(out + 6, 0x2A07);
    img.u32(out + 8, 0x10);
    let back = edge_at(&img, &parts, 4);
    img.u32(back, CELL);
    img.u16(back + 4, 0);
    img.u16(back + 6, 0xFFFF);
    (img, parts)
}

#[test]
fn a_disable_cuts_edges_to_the_shipped_form_and_keeps_a_side_record_s_corner_1_edge() {
    let (mut img, parts) = joined();
    let read = read_navmesh(&img.bytes).expect("it reads");
    let cut = polygon_cut(&read, 0).expect("a polygon proper");
    assert_eq!(
        cut,
        PolygonCut {
            edges: vec![0],
            side_records: vec![1],
            links: Vec::new(),
        }
    );
    assert!(matches!(
        polygon_cut(&read, 1),
        Err(HeaderlessError::NotInPlace(_))
    ));

    let mut want = img.bytes.clone();
    let at = edge_at(&img, &parts, 0);
    want[at..at + 4].copy_from_slice(&NOWHERE.to_le_bytes());
    want[at + 4..at + 6].copy_from_slice(&0xFFFFu16.to_le_bytes());
    want[at + 6..at + 8].copy_from_slice(&0x2A00u16.to_le_bytes());
    cut_edges(&mut img.bytes, &cut.edges).expect("written in place");
    assert_eq!(
        img.bytes, want,
        "only the edge's mesh, polygon and +0x06 low byte"
    );
    let again = read_navmesh(&img.bytes).expect("it reads back");
    assert_eq!(
        again.edges[0],
        NavEdge {
            mesh: NOWHERE,
            polygon: 0xFFFF,
            word_06: 0x2A00,
            flags: 0x10,
        }
    );
    assert_eq!(
        again.neighbour(4),
        Some(0),
        "the side record still names it"
    );
    assert_eq!(
        polygon_cut(&again, 0).expect("cut").edges,
        Vec::<usize>::new()
    );

    let before = img.bytes.clone();
    assert!(matches!(
        cut_edges(&mut img.bytes, &[4]),
        Err(HeaderlessError::NotInPlace(_))
    ));
    assert!(matches!(
        cut_edges(&mut img.bytes, &[5]),
        Err(HeaderlessError::IndexPastCount { index: 5, .. })
    ));
    assert_eq!(img.bytes, before);
}

#[test]
fn a_cut_takes_an_edge_into_the_polygon_and_names_each_link_at_it_once() {
    // Polygon 1 named by a side record's corner 1, which stays, and by its corner 2, which goes.
    let mut m = cell(
        CELL,
        [0.0, 0.0],
        &[(1.0, &[[128.0, 0.0], [128.0, 4.0], [128.0, 8.0]], &[1, 1])],
    );
    let link = SpecialLink {
        from: [0; 3],
        to: [0; 3],
        from_polygon: 2,
        to_polygon: 1,
        from_mesh: CELL,
        to_mesh: CELL,
        word_18: 0,
        heading: 0x40,
    };
    // From polygon 1 of the cell east, to polygon 2 of this one.
    let elsewhere = SpecialLink {
        from_polygon: 1,
        from_mesh: CELL + 1,
        to_polygon: 2,
        ..link
    };
    m.special_links = vec![elsewhere, link, link];
    let cut = polygon_cut(&m, 1).expect("a polygon proper");
    assert_eq!(cut.edges, [2], "the side record's corner 2");
    assert_eq!(cut.side_records, [3]);
    assert_eq!(
        cut.links,
        [1],
        "the link once, its copy and the one at polygon 2 not"
    );
    assert_eq!(
        links_into(&m, CELL + 1, 1),
        [0],
        "a neighbour's link names it by mesh and number"
    );
    assert!(links_into(&m, CELL + 1, 2).is_empty());
}
