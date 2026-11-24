use super::*;

/// [`deletable`] as a body of no cell: mesh 0 wherever it named its own cell; link 0 from polygon 2
/// to 1, link 1 from polygon 2 to none, link 2 from polygon 1 to 2, each filed under its from
/// number alone with an empty run at the running total.
fn no_cell() -> Image {
    let built = deletable();
    let mut img = built.img;
    img.u32(0xC8, 0);
    let edges = first_item(&img, built.adjacency);
    for j in 0..CORNERS.iter().map(|c| c.len()).sum::<usize>() {
        if u32_at(&img.bytes, edges + 12 * j) == CELL {
            img.u32(edges + 12 * j, 0);
        }
    }
    let links = target(&img.bytes, 0xA8);
    for (k, (from, to_mesh, to)) in [(2, 0, 1), (2, NOWHERE, 0xFFFF), (1, 0, 2)]
        .into_iter()
        .enumerate()
    {
        let l = links + 32 * k;
        img.u16(l + 0x0C, from);
        img.u16(l + 0x0E, to);
        img.u32(l + 0x10, 0);
        img.u32(l + 0x14, to_mesh);
    }
    // The end that names nowhere still holds a position.
    img.u16(links + 32 + 0x06, 0x1234);
    for (k, i) in [2u16, 0, 1].iter().enumerate() {
        img.u16(built.link_list + 2 * k, *i);
    }
    let records = first_item(&img, built.polygons);
    for (q, run) in [0, 1, (1 << 23) | (1 << 6) | 2, 3 << 6].iter().enumerate() {
        img.u32(records + 44 * q + 0x20, *run);
    }
    read_navmesh(&img.bytes).expect("the body of no cell reads");
    img
}

#[test]
fn a_body_of_no_cell_deletes_a_polygon_and_renumbers_its_own_links_alone() {
    let mut s = sections_of(&no_cell());
    let read = delete_polygon(&mut s, 0, &[]).expect("deleted");
    assert_eq!(read.cell, 0);
    assert_eq!((read.polygons.len(), read.polygons_proper), (3, 2));
    let named: Vec<Option<usize>> = (0..read.edges.len()).map(|j| read.neighbour(j)).collect();
    let mut want = Vec::new();
    for (q, corners) in CORNERS.iter().enumerate().skip(1) {
        for k in 0..corners.len() {
            want.push(leads(q, k).map(|t| usize::from(t) - 1));
        }
    }
    assert_eq!(named, want);
    // Each own end one lower for the record removed below it; the end that names nowhere as read.
    let ends: Vec<_> = (read.special_links.iter())
        .map(|l| (l.from_mesh, l.from_polygon, l.to_mesh, l.to_polygon))
        .collect();
    assert_eq!(ends, [(0, 1, 0, 0), (0, 1, NOWHERE, 0xFFFF), (0, 0, 0, 1)]);
    assert_eq!(read.special_links[1].to, [0x1234, 0, 0]);
    assert_eq!(read.special_link_indices, [2, 0, 1]);
    let runs: Vec<_> = read.polygons.iter().map(|p| p.link_run()).collect();
    assert_eq!(runs, [0..1, 1..3, 3..3]);
    assert_ne!(read.polygons[1].word_20 & (1 << 23), 0, "bit 23 as read");

    let file = write_rpf9_resource(&s).expect("written");
    let (find, bytes) = read_rpf9_body(Cursor::new(&file), 0, u64::MAX).expect("it reads");
    assert_eq!(Rpf9Sections::of(&find, &bytes).expect("a resource"), s);
}

#[test]
fn a_deletion_in_a_body_of_no_cell_renumbers_no_link_of_a_cell_body() {
    let m = read_navmesh(&no_cell().bytes).expect("it reads");
    let cell = read_navmesh(&deletable().img.bytes).expect("a cell body reads");
    let made = deletion(&m, 0, &[&cell]).expect("deleted");
    assert!(matches!(relinked(&cell, &m, &made), Ok(None)));
}
