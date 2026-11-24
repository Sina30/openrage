use super::*;

/// A table-form body: `types` as a hash and an element size each, and `blocks` as a type's hash and
/// its bytes each, laid out as a document's sections.
fn body(types: &[(u32, u32)], blocks: &[(u32, Vec<u8>)]) -> Vec<u8> {
    fn frame(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(tag);
        out.extend_from_slice(&(data.len() as u32 + 8).to_be_bytes());
        out.extend_from_slice(data);
    }
    // The block table, then each block on 16 bytes counted from the frame.
    let mut psin = Vec::new();
    for (class, data) in blocks {
        psin.extend_from_slice(&class.to_le_bytes());
        psin.extend_from_slice(&(data.len() as u32).to_le_bytes());
        psin.extend_from_slice(&[0xAA; 8]);
    }
    for (_, data) in blocks {
        psin.resize((psin.len() + 8).next_multiple_of(16) - 8, 0x70);
        psin.extend_from_slice(data);
    }
    let mut lsch = Vec::new();
    for (class, size) in types {
        let mut header = [0u8; 32];
        header[..4].copy_from_slice(&class.to_le_bytes());
        header[24..28].copy_from_slice(&size.to_le_bytes());
        lsch.extend_from_slice(&header);
    }
    let mut lrsc = vec![0u8; 0x40];
    lrsc[0x38..0x3A].copy_from_slice(&(types.len() as u16).to_le_bytes());
    lrsc[0x3C..0x3E].copy_from_slice(&(blocks.len() as u16).to_le_bytes());
    let mut out = Vec::new();
    frame(&mut out, b"PSIN", &psin);
    frame(&mut out, b"LSCH", &lsch);
    for tag in [b"LENM", b"LSNS", b"LGID"] {
        frame(&mut out, tag, &[]);
    }
    frame(&mut out, b"LRSC", &lrsc);
    frame(&mut out, b"CHKS", &[0; 12]);
    out
}

/// A list's root: `count` regions of room `capacity` at `offset` of block number `block`, from 1.
fn root(count: u32, capacity: u32, block: u64, offset: u64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&capacity.to_le_bytes());
    out.extend_from_slice(&(block | (offset << 20)).to_le_bytes());
    out
}

fn header(region: &Region) -> [u8; HEADER] {
    let mut out = [0u8; HEADER];
    out[..4].copy_from_slice(&region.x.to_le_bytes());
    out[4..8].copy_from_slice(&region.y.to_le_bytes());
    out[8..12].copy_from_slice(&region.version.to_le_bytes());
    out[12..16].copy_from_slice(&region.file_name.to_le_bytes());
    out[16..20].copy_from_slice(&region.index.to_le_bytes());
    out[24..32].copy_from_slice(&region.map_word.to_le_bytes());
    out
}

const TYPES: &[(u32, u32)] = &[(REGION_LIST, 0x10), (REGION_HEADER, 0x20)];

fn regions() -> Vec<Region> {
    vec![
        Region {
            x: -7,
            y: 12,
            version: 0,
            file_name: 0x1111_2222,
            index: 5,
            map_word: 0,
        },
        Region {
            x: 3,
            y: -8,
            version: 1,
            file_name: 0x3333_4444,
            index: 0,
            map_word: 0x0102_0304_0506_0708,
        },
    ]
}

fn list_of(regions: &[Region]) -> Vec<u8> {
    let rows: Vec<u8> = regions.iter().flat_map(header).collect();
    let count = regions.len() as u32;
    body(
        TYPES,
        &[
            (REGION_LIST, root(count, count, 2, 0)),
            (REGION_HEADER, rows),
        ],
    )
}

#[test]
fn a_list_reads_as_the_regions_it_states_in_their_order() {
    assert_eq!(read(&list_of(&regions())), Ok(regions()));
    // A list of none points nowhere.
    let empty = body(TYPES, &[(REGION_LIST, root(0, 0, 0, 0))]);
    assert_eq!(read(&empty), Ok(Vec::new()));
}

#[test]
fn a_list_may_start_its_regions_inside_their_block() {
    let rows: Vec<u8> = regions().iter().flat_map(header).collect();
    let later = body(
        TYPES,
        &[(REGION_LIST, root(1, 1, 2, 0x20)), (REGION_HEADER, rows)],
    );
    assert_eq!(read(&later), Ok(vec![regions()[1]]));
}

#[test]
fn bytes_that_are_no_region_list_are_refused() {
    let not_a_list = |bytes: &[u8]| matches!(read(bytes), Err(MapGridError::NotAList(_)));
    let rows: Vec<u8> = regions().iter().flat_map(header).collect();
    // No block of the list's type, and two of them.
    assert!(not_a_list(&body(TYPES, &[(REGION_HEADER, rows.clone())])));
    let twice = [
        (REGION_LIST, root(0, 0, 0, 0)),
        (REGION_LIST, root(0, 0, 0, 0)),
    ];
    assert!(not_a_list(&body(TYPES, &twice)));
    // A root shorter than its array.
    assert!(not_a_list(&body(TYPES, &[(REGION_LIST, vec![0; 8])])));
    // More regions than the block holds, and more than the array has room for.
    let over = [
        (REGION_LIST, root(3, 3, 2, 0)),
        (REGION_HEADER, rows.clone()),
    ];
    assert!(not_a_list(&body(TYPES, &over)));
    let crowded = [
        (REGION_LIST, root(2, 1, 2, 0)),
        (REGION_HEADER, rows.clone()),
    ];
    assert!(not_a_list(&body(TYPES, &crowded)));
    // Regions that start between two headers.
    let askew = [
        (REGION_LIST, root(1, 1, 2, 8)),
        (REGION_HEADER, rows.clone()),
    ];
    assert!(not_a_list(&body(TYPES, &askew)));
    // A pointer at a block of another type, at no block, and at one past the last.
    let other = [(REGION_LIST, root(2, 2, 1, 0)), (0x1234, rows.clone())];
    assert!(not_a_list(&body(TYPES, &other)));
    let nowhere = [
        (REGION_LIST, root(2, 2, 0, 0)),
        (REGION_HEADER, rows.clone()),
    ];
    assert!(not_a_list(&body(TYPES, &nowhere)));
    let past = [
        (REGION_LIST, root(2, 2, 3, 0)),
        (REGION_HEADER, rows.clone()),
    ];
    assert!(not_a_list(&body(TYPES, &past)));
    // Headers the type table gives another size.
    let wide: &[(u32, u32)] = &[(REGION_LIST, 0x10), (REGION_HEADER, 0x28)];
    let sized = [(REGION_LIST, root(2, 2, 2, 0)), (REGION_HEADER, rows)];
    assert!(not_a_list(&body(wide, &sized)));
    // And bytes that are no table-form body at all.
    assert!(matches!(
        read(b"PSIN\0\0\0\x08"),
        Err(MapGridError::Body(_))
    ));
}

#[test]
fn every_changed_byte_is_read_or_refused_and_never_a_panic() {
    crate::gta6::mutated_tests::every_change(&list_of(&regions()), |bytes| {
        let _ = read(bytes);
    });
}
