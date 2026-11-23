//! What an owned value holds on the heap: each run's allocation and what its
//! elements hold, each pointee and each string, for a value built from scratch.

use std::mem::size_of;

use or_io::decl::Heap;

use super::{Catalogue, Holder, Point};

#[test]
fn a_value_built_from_scratch_counts_its_allocations() {
    let catalogue = Catalogue {
        name: String::from("catalogue"),
        points: vec![Point::default(); 3],
        primary: Some(Box::default()),
        held: Some(Box::new(Holder {
            items: vec![Some(Box::default())],
            ..Default::default()
        })),
        ..Default::default()
    };
    let expected = catalogue.name.capacity()
        + catalogue.points.capacity() * size_of::<Point>()
        + size_of::<Point>()
        + size_of::<Holder<Point>>()
        + size_of::<Option<Box<Point>>>()
        + size_of::<Point>();
    assert_eq!(catalogue.heap_bytes(), expected);
    assert_eq!(
        Catalogue::default().heap_bytes(),
        0,
        "an empty value allocates nothing"
    );
}

#[test]
fn a_value_whose_run_grows_counts_more() {
    let mut catalogue = Catalogue::default();
    catalogue.points.reserve_exact(64);
    let reserved = catalogue.heap_bytes();
    assert_eq!(reserved, catalogue.points.capacity() * size_of::<Point>());
    catalogue
        .tags
        .extend((0..32).map(or_io::decl::Hash32::from_raw));
    assert!(catalogue.heap_bytes() >= reserved + 32 * size_of::<or_io::decl::Hash32>());
}
