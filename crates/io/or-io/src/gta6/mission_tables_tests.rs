use super::*;

/// A table-form body: `types` as a hash, an element size and a flag byte each, and `blocks` as
/// a type's hash and its bytes each, laid out as a document's sections.
fn body(types: &[(u32, u32, u8)], blocks: &[(u32, Vec<u8>)]) -> Vec<u8> {
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
    for (class, size, flags) in types {
        let mut header = [0u8; 32];
        header[..4].copy_from_slice(&class.to_le_bytes());
        header[8] = *flags;
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

/// A state record whose run is `count` elements at `offset` of block number `block`, from 1.
fn record(key: u32, block: u64, offset: u64, count: u16) -> [u8; RECORD] {
    let mut out = [0u8; RECORD];
    out[..4].copy_from_slice(&key.to_le_bytes());
    out[8..12].copy_from_slice(&1u32.to_le_bytes());
    out[16..24].copy_from_slice(&(block | (offset << 20)).to_le_bytes());
    out[24..26].copy_from_slice(&count.to_le_bytes());
    out[26..28].copy_from_slice(&count.to_le_bytes());
    out
}

/// A placed element of `size` bytes; one of 0x70 holds a third vector before the key's copy.
fn placed(element: &PlacedElement, size: usize) -> Vec<u8> {
    let mut out = vec![0u8; size];
    out[..4].copy_from_slice(&element.archetype.to_le_bytes());
    let third = [7.5f32, 8.5, 9.5];
    let mut vectors = vec![(0x10, element.position), (0x20, element.rotation)];
    let tail = if size >= 0x70 {
        vectors.push((0x30, third));
        0x40
    } else {
        0x30
    };
    for (at, v) in vectors {
        for (k, c) in v.iter().chain(&v[..1]).enumerate() {
            out[at + 4 * k..at + 4 * k + 4].copy_from_slice(&c.to_le_bytes());
        }
    }
    out[tail..tail + 4].copy_from_slice(&element.archetype.to_le_bytes());
    out[tail + 4..tail + 8].copy_from_slice(&element.link.to_le_bytes());
    out
}

const ROOT: u32 = 0xABCD_0001;
const STATES: u32 = 0x101;
const GROUPS: u32 = 0x102;
const PEDS: u32 = 0x103;
const AREAS: u32 = 0x104;
const PAIRS: u32 = 0x105;

fn elements() -> [PlacedElement; 3] {
    let e = |archetype, x: f32, link| PlacedElement {
        archetype,
        position: [x, -20.25, 3.0],
        rotation: [0.0, 0.0, 1.5],
        link,
    };
    [
        e(0xA1A2_A3A4, 10.5, 0xC0DE),
        e(0xB1B2_B3B4, 11.5, 0xC0DE),
        e(0xC1C2_C3C4, 12.5, 0xF00D),
    ]
}

/// A body of two state tables, the first of whose states own records of the second, which own
/// two placed elements of 0x40 bytes, one of 0x70, and one pair of another shape.
fn mission() -> Vec<(u32, Vec<u8>)> {
    let [a, b, c] = elements();
    vec![
        // A structure of 32 bytes that reads as a record, and is no array's element.
        (ROOT, record(0x1111, 0, 0, 0).to_vec()),
        (
            STATES,
            [
                record(state_key("Approach"), 3, 0, 2),
                record(state_key("Leave"), 3, 0x40, 1),
                record(state_key("Idle"), 0, 0, 0),
            ]
            .concat(),
        ),
        (
            GROUPS,
            [
                record(0x51, 4, 0, 2),
                record(0x52, 5, 0, 1),
                record(0x53, 6, 0x28, 1),
            ]
            .concat(),
        ),
        (PEDS, [placed(&a, 0x40), placed(&b, 0x40)].concat()),
        (AREAS, placed(&c, 0x70)),
        (PAIRS, vec![0x5A; 0x50]),
        (7, vec![1, 0, 0, 0, 2, 0, 0, 0]),
    ]
}

const TYPES: [(u32, u32, u8); 6] = [
    (ROOT, 0x20, 0),
    (STATES, 0x20, 0x0C),
    (GROUPS, 0x20, 0x0C),
    (PEDS, 0x40, 0x0C),
    (AREAS, 0x70, 0x0C),
    (PAIRS, 0x28, 0x0C),
];

#[test]
fn a_body_s_blocks_are_placed_by_its_own_block_table() {
    let bytes = body(&TYPES, &mission());
    let read = Body::open(&bytes).expect("a table-form body");
    assert_eq!(read.blocks.len(), 7);
    // Seven rows end at 0x78, so the first block starts at 0x80 and each next on 16 bytes.
    let offsets: Vec<usize> = read.blocks.iter().map(|b| b.offset).collect();
    assert_eq!(offsets, [0x80, 0xA0, 0x100, 0x160, 0x1E0, 0x250, 0x2A0]);
    assert_eq!(read.blocks[1].element, Some((0x20, 0x0C)));
    assert_eq!(read.blocks[0].element, Some((0x20, 0)));
    assert_eq!(read.blocks[6].element, None);
    assert_eq!((read.blocks[6].class, read.blocks[6].size), (7, 8));
}

#[test]
fn a_table_is_a_block_declared_as_an_array_of_records_and_nothing_else() {
    let bytes = body(&TYPES, &mission());
    let read = Body::open(&bytes).expect("a table-form body");
    assert!(read.holds_tables());
    let tables = read.tables(&bytes).expect("two tables");
    // The root reads as a record and is left out: its type is no array's element.
    assert_eq!(
        tables
            .iter()
            .map(|t| (t.block, t.offset))
            .collect::<Vec<_>>(),
        [(1, 0xA0), (2, 0x100)]
    );
    assert_eq!(
        tables[0].records,
        [
            StateRecord {
                key: state_key("approach"),
                word_08: 1,
                run: Some((2, 0)),
                count: 2
            },
            StateRecord {
                key: state_key("LEAVE"),
                word_08: 1,
                run: Some((2, 0x40)),
                count: 1
            },
            StateRecord {
                key: state_key("idle"),
                word_08: 1,
                run: None,
                count: 0
            },
        ]
    );
    // Bytes that hold a run of records and no document are no body, so nothing is scanned out of them.
    let loose = [record(1, 0, 0, 0), record(2, 0, 0, 0)].concat();
    assert!(matches!(
        Body::open(&loose),
        Err(MissionTableError::NotABody { .. })
    ));
    let none = body(&TYPES[..1], &mission()[..1]);
    let read = Body::open(&none).expect("a table-form body");
    assert!(!read.holds_tables());
    assert_eq!(read.tables(&none), Ok(Vec::new()));
}

#[test]
fn a_state_s_run_is_another_table_placed_elements_or_another_shape() {
    let bytes = body(&TYPES, &mission());
    let read = Body::open(&bytes).expect("a table-form body");
    let tables = read.tables(&bytes).expect("two tables");
    let run = |table: usize, row: usize| read.run(&bytes, &tables, &tables[table].records[row]);
    assert_eq!(run(0, 0), Some(Run::Table(1)));
    assert_eq!(run(0, 2), None);
    let [a, b, c] = elements();
    assert_eq!(run(1, 0), Some(Run::Placed(vec![(0x160, a), (0x1A0, b)])));
    // The longer element holds a vector where the shorter repeats its key.
    assert_eq!(run(1, 1), Some(Run::Placed(vec![(0x1E0, c)])));
    assert_eq!(run(1, 2), Some(Run::Other { element: 0x28 }));
}

#[test]
fn a_row_that_is_no_record_refuses_the_table() {
    let whole = body(&TYPES, &mission());
    let first = 0xA0;
    let refused = |change: &dyn Fn(&mut Vec<u8>)| {
        let mut bytes = whole.clone();
        change(&mut bytes);
        let read = Body::open(&bytes).expect("a table-form body");
        match read.tables(&bytes) {
            Err(MissionTableError::NotARecord { at, why }) => (at, why),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(
        refused(&|b| b[first + 4] = 1),
        (first, "a word at +4, +0xC or +0x1C is not zero")
    );
    assert_eq!(
        refused(&|b| b[first + 26] = 9),
        (first, "the two counts at +0x18 differ")
    );
    assert_eq!(
        refused(&|b| b[first + 17] = 0x10),
        (first, "bits 12 to 19 of its pointer are not zero")
    );
    // Three records from the second pass the three the block holds.
    let past = "its run is not whole elements inside the block its pointer names";
    assert_eq!(
        refused(&|b| b[first + RECORD..first + 2 * RECORD].copy_from_slice(&record(9, 3, 0x20, 3))),
        (first + RECORD, past)
    );
    // A run that starts inside an element, and one in a block of no declared type.
    assert_eq!(
        refused(&|b| b[first..first + RECORD].copy_from_slice(&record(9, 3, 0x10, 1))),
        (first, past)
    );
    assert_eq!(
        refused(&|b| b[first..first + RECORD].copy_from_slice(&record(9, 7, 0, 1))),
        (first, past)
    );
    assert_eq!(
        refused(&|b| b[first..first + RECORD].copy_from_slice(&record(9, 8, 0, 1))),
        (first, past)
    );
    assert_eq!(
        refused(&|b| b[first..first + RECORD].copy_from_slice(&record(9, 3, 0, 0))),
        (
            first,
            "its pointer and its count do not agree on an empty run"
        )
    );
}

#[test]
fn bytes_that_are_no_table_form_document_are_refused() {
    let bytes = body(&TYPES, &mission());
    let why = |bytes: &[u8]| match Body::open(bytes) {
        Err(MissionTableError::NotABody { why }) => why,
        other => panic!("{other:?}"),
    };
    assert_eq!(why(&bytes[..4]), "a section's frame is cut short");
    assert_eq!(
        why(&bytes[..bytes.len() - 1]),
        "a section states a length outside the body"
    );
    let mut renamed = bytes.clone();
    renamed[..4].copy_from_slice(b"PMAP");
    assert_eq!(
        why(&renamed),
        "its sections are not PSIN LSCH LENM LSNS LGID LRSC CHKS"
    );
    // One block more than PSIN holds.
    let counts = bytes.len() - 20 - 0x40 + 0x3C;
    let mut more = bytes.clone();
    more[counts..counts + 2].copy_from_slice(&0x400u16.to_le_bytes());
    assert_eq!(
        why(&more),
        "PSIN is shorter than the block table LRSC counts"
    );
}

#[test]
fn a_state_key_folds_case() {
    assert_eq!(state_key("HOPOUT"), state_key("hopout"));
    assert_ne!(
        state_key("HOPOUT"),
        crate::hash::literal_string_hash(b"HOPOUT")
    );
}

#[test]
fn a_placed_element_is_refused_by_what_it_does_not_repeat() {
    let [a, _, c] = elements();
    let bytes = placed(&a, 0x40);
    assert_eq!(read_element(&bytes, 0, 0x40), Ok(a));
    assert_eq!(read_element(&placed(&c, 0x70), 0, 0x70), Ok(c));
    let why = |bytes: &[u8], size: usize| match read_element(bytes, 0, size) {
        Err(MissionTableError::NotAnElement { at: 0, why }) => why,
        other => panic!("{other:?}"),
    };
    // A position stated twice in full, as a scan once took it, closes neither vector.
    let mut doubled = bytes.clone();
    doubled.copy_within(0x10..0x1C, 0x1C);
    assert_eq!(
        why(&doubled, 0x40),
        "a vector is not closed by a copy of its first component"
    );
    let mut unkeyed = bytes.clone();
    unkeyed[0x30] ^= 1;
    assert_eq!(
        why(&unkeyed, 0x40),
        "its key is repeated at neither +0x30 nor +0x40"
    );
    let mut owned = bytes.clone();
    owned[4] = 1;
    assert_eq!(why(&owned, 0x40), "a word at +4, +8 or +0xC is not zero");
    assert_eq!(why(&bytes, 0x30), "it is shorter than a placed element");
    assert_eq!(
        read_element(&bytes[..0x2C], 0, 0x40),
        Err(MissionTableError::Truncated {
            what: "placed element",
            at: 0
        })
    );
}
