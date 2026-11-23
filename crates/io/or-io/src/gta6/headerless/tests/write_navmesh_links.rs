use super::*;

/// [`deletable`] as the body of the cell east of it: its edges name its own polygons, its links 0
/// and 1 run between polygons 2 and 1 of the cell west of it, link 2 from its polygon 7 into 1.
fn east() -> Image {
    let built = deletable();
    let mut img = built.img;
    img.u32(0xC8, OTHER);
    let edges = first_item(&img, built.adjacency);
    for j in 0..CORNERS.iter().map(|c| c.len()).sum::<usize>() {
        if u32_at(&img.bytes, edges + 12 * j) == CELL {
            img.u32(edges + 12 * j, OTHER);
        }
    }
    img
}

/// [`deletable`] with the edges `which` leading nowhere.
fn cut(which: &[usize]) -> Image {
    let built = deletable();
    let mut img = built.img;
    let edges = first_item(&img, built.adjacency);
    for j in which {
        img.u32(edges + 12 * j, NOWHERE);
        img.u16(edges + 12 * j + 4, 0xFFFF);
    }
    img
}

fn reads_back_loose(s: &Rpf9Sections) {
    let file = write_rpf9_resource(s).expect("written");
    let (find, bytes) = read_rpf9_body(Cursor::new(&file), 0, u64::MAX).expect("it reads");
    assert_eq!(&Rpf9Sections::of(&find, &bytes).expect("a resource"), s);
}

#[test]
fn a_neighbouring_body_s_links_are_renumbered_with_the_cell_and_written_in_place() {
    let m = read_navmesh(&deletable().img.bytes).expect("the cell reads");
    let mut s = sections_of(&east());
    let body = read_navmesh(&s.virtual_section).expect("the east body reads");
    let made = deletion(&m, 0, &[&body]).expect("deleted");
    let links = (relinked(&body, &m, &made).expect("resolved")).expect("renumbered");
    let ends: Vec<_> = (links.iter())
        .map(|l| (l.from_mesh, l.from_polygon, l.to_polygon))
        .collect();
    assert_eq!(ends, [(CELL, 1, 0), (CELL, 1, 0), (OTHER, 7, 0)]);

    let len = s.virtual_section.len();
    let read = write_links(&mut s, links.clone()).expect("written");
    assert_eq!(read.special_links, links);
    // Filed by number whichever mesh it names: links 0 and 2 under 0, link 0 under 1.
    assert_eq!(read.special_link_indices, [0, 2, 0]);
    let runs: Vec<_> = read.polygons.iter().map(|p| p.link_run()).collect();
    assert_eq!(runs, [0..2, 2..3, 3..3, 3..3]);
    assert_ne!(read.polygons[2].word_20 & (1 << 23), 0, "bit 23 as read");
    assert_eq!(read.vertices, body.vertices);
    assert_eq!(read.edges, body.edges);
    assert_eq!(s.virtual_section.len(), len, "nothing moves");
    reads_back_loose(&s);

    let kept = s.clone();
    write_links(&mut s, links).expect("the links it holds");
    assert_eq!(s, kept, "no byte changes");
    assert!(relinked(&m, &m, &made).expect("its own").is_none());
    let cut_only = disabling(&m, 1, &[]).expect("disabled");
    assert!(
        relinked(&body, &m, &cut_only)
            .expect("nothing renumbered")
            .is_none(),
        "a disable that removes no record"
    );
}

#[test]
fn a_neighbouring_body_s_link_into_a_deleted_polygon_goes_and_its_lists_shrink() {
    let m = read_navmesh(&deletable().img.bytes).expect("the cell reads");
    let mut img = east();
    // Link 2 now runs into polygon 0 of the cell west, which goes.
    let links = target(&img.bytes, 0xA8);
    img.u16(links + 64 + 0x0E, 0);
    let mut s = sections_of(&img);
    let body = read_navmesh(&s.virtual_section).expect("the patched body reads");
    let made = deletion(&m, 0, &[&body]).expect("deleted");
    let links = (relinked(&body, &m, &made).expect("resolved")).expect("changed");
    assert_eq!(links.len(), 2, "link 2 goes");
    let len = s.virtual_section.len();
    let read = write_links(&mut s, links).expect("written");
    assert_eq!(read.special_link_indices, [0, 0]);
    let runs: Vec<_> = read.polygons.iter().map(|p| p.link_run()).collect();
    assert_eq!(runs, [0..1, 1..2, 2..2, 2..2]);
    let v = &s.virtual_section;
    assert_eq!((u32_at(v, 0xD4), u32_at(v, 0xD8)), (2, 2));
    let total = 6 * read.vertices.len() + 14 * read.indices.len() + 44 * 4 + 32 * 2;
    assert_eq!(u32_at(v, 0xCC) as usize, total);
    assert_eq!(v.len(), len - 32, "one link's 32 bytes given back");
    reads_back_loose(&s);
}

#[test]
fn a_special_link_with_an_end_in_a_deleted_polygon_goes_with_its_copy() {
    // Polygon 1's edge into 2 cut: links 0 and 1 run from polygon 2, link 2 from the east into 1.
    let img = cut(&[4]);
    let m = read_navmesh(&img.bytes).expect("it reads");
    let mut s = sections_of(&img);
    let read = delete_polygon(&mut s, 2, &[]).expect("deleted");
    // The side record named polygon 2 alone and went with it.
    assert_eq!((read.polygons.len(), read.polygons_proper), (2, 2));
    assert_eq!(read.special_links, [m.special_links[2]]);
    assert_eq!(read.special_link_indices, [0]);
    let runs: Vec<_> = read.polygons.iter().map(|p| p.link_run()).collect();
    assert_eq!(runs, [0..0, 0..1]);
    let v = &s.virtual_section;
    assert_eq!(u32_at(v, 0xD4), 1);
    let total = 6 * read.vertices.len() + 14 * read.indices.len() + 44 * 2 + 32;
    assert_eq!(u32_at(v, 0xCC) as usize, total);
    reads_back_loose(&s);
}

#[test]
fn a_link_list_left_with_no_link_is_written_null_with_count_0() {
    // Polygon 2's edge into 1 cut as well: every link ends in polygon 1.
    let mut s = sections_of(&cut(&[4, 8]));
    let read = delete_polygon(&mut s, 1, &[]).expect("deleted");
    assert!(read.special_links.is_empty());
    assert!(read.special_link_indices.is_empty());
    assert!(read.polygons.iter().all(|p| p.link_run().is_empty()));
    let v = &s.virtual_section;
    assert_eq!((u64_at(v, 0xA8), u32_at(v, 0xD4)), (0, 0));
    assert_eq!((u64_at(v, 0xB0), u32_at(v, 0xD8)), (0, 0));
    reads_back_loose(&s);
}
