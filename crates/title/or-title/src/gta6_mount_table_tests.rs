//! A table's entries by name, over tables made up here.

use super::*;
use or_io::gta6::hash::name_hash32;

/// An entry to lay out: its path, kind, offset and stored length in slots, stated length, and
/// whether it states a 32-bit name, a cipher and a chunk count.
#[derive(Clone, Copy)]
struct Made {
    path: &'static str,
    kind: u8,
    at: u64,
    slots: u64,
    stated: u64,
    named: bool,
    ciphered: bool,
    chunks: Option<u16>,
}

const FILE: Made = Made {
    path: "",
    kind: KIND_FILE,
    at: 0,
    slots: 1,
    stated: 10,
    named: true,
    ciphered: false,
    chunks: Some(1),
};

/// A decrypted table of `made`, with indices `width` bytes wide: five columns in the main region
/// and one external, each entry with a row of its own in the columns it states.
fn table_of(made: &[Made], width: usize) -> Rpf9Toc {
    let columns: [(u64, usize, bool); 6] = [
        (CIPHER, 64, false),
        (PLACE, 24, false),
        (WORD, 8, false),
        (NAME, 4, false),
        (CHUNKS, 8, false),
        (0xB6F5_BDB2_C66D_A2E9, 8, true),
    ];
    let row = |m: &Made, column: u64| -> Option<Vec<u8>> {
        match column {
            PLACE => {
                let mut row = (m.at | u64::from(m.kind) << 40).to_le_bytes().to_vec();
                row.extend(m.slots.to_le_bytes());
                row.extend(m.stated.to_le_bytes());
                Some(row)
            }
            WORD => Some(name_hash64(m.path.as_bytes()).to_le_bytes().to_vec()),
            NAME => (m.named).then(|| name_hash32(m.path.as_bytes()).to_le_bytes().to_vec()),
            CIPHER => m.ciphered.then(|| vec![0x11; 64]),
            CHUNKS => m.chunks.map(|n| {
                let mut row = vec![0xAA; 4];
                row.extend(n.to_le_bytes());
                row.extend([8, 3]);
                row
            }),
            _ => Some(vec![0x55; 8]),
        }
    };
    let none: u64 = if width == 2 { 0xFFFF } else { 0xFF };
    let put = |out: &mut Vec<u8>, index: u64| out.extend(&index.to_le_bytes()[..width]);
    let mut bytes = vec![0u8; 0x40 + 24 * columns.len()];
    bytes[0x2C] = width as u8;
    let mut groups = Vec::new();
    for external in [false, true] {
        if external {
            let split = bytes.len() as u32;
            bytes[0x08..0x0C].copy_from_slice(&split.to_le_bytes());
        }
        let mine: Vec<_> = columns.iter().filter(|c| c.2 == external).collect();
        // One row of indices an entry, a column after another.
        let mut counts = vec![0u64; mine.len()];
        for m in made {
            for (k, (hash, ..)) in mine.iter().enumerate() {
                match row(m, *hash) {
                    Some(_) => {
                        put(&mut bytes, counts[k]);
                        counts[k] += 1;
                    }
                    None => put(&mut bytes, none),
                }
            }
        }
        for (hash, row_width, _) in mine {
            let from = bytes.len();
            let rows: Vec<Vec<u8>> = made.iter().filter_map(|m| row(m, *hash)).collect();
            for r in &rows {
                assert_eq!(r.len(), *row_width);
                bytes.extend(r);
            }
            groups.push(Rpf9Group {
                name_hash: *hash,
                count: rows.len() as u32,
                width: *row_width as u16,
                external,
                data: from..bytes.len(),
            });
        }
    }
    Rpf9Toc {
        bytes,
        key_id: 0,
        registry_id: 0,
        entry_count: made.len() as u32,
        groups,
    }
}

fn made_up() -> Vec<Made> {
    vec![
        Made {
            path: "town/docks_metadata",
            kind: KIND_ARCHIVE,
            at: 0x1000,
            slots: 0x200,
            stated: 0x2000,
            named: false,
            chunks: None,
            ..FILE
        },
        Made {
            path: "changesets/ccs_town.meta",
            at: 0x20,
            slots: 3,
            stated: 100,
            ..FILE
        },
        Made {
            path: "changesets/ccs_closed.meta",
            at: 0x30,
            ciphered: true,
            ..FILE
        },
        Made {
            path: "town/ai/carparks",
            kind: KIND_RESOURCE,
            at: 0x40,
            stated: 200_000,
            chunks: Some(4),
            ..FILE
        },
    ]
}

#[test]
fn an_entry_is_found_by_the_hash_of_its_path_with_every_column_it_states() {
    for width in [1, 2] {
        let table = Table::of(&table_of(&made_up(), width)).expect("a table");
        assert_eq!(table.entries().len(), 4);
        let archive = table.find("TOWN/Docks_Metadata").expect("the archive");
        assert_eq!(
            *archive,
            Entry {
                word: name_hash64(b"town/docks_metadata"),
                name: None,
                offset: 0x10000,
                stored: 0x2000,
                stated: 0x2000,
                kind: KIND_ARCHIVE,
                ciphered: false,
                chunks: 0,
            }
        );
        let set = table.find("changesets/ccs_town.meta").expect("the file");
        assert_eq!(
            (set.offset, set.stored, set.stated, set.kind),
            (0x200, 48, 100, KIND_FILE)
        );
        assert_eq!(set.name, Some(name_hash32(b"changesets/ccs_town.meta")));
        assert!(!set.ciphered);
        let closed = table.find("changesets/ccs_closed.meta").expect("the file");
        assert!(closed.ciphered);
        let resource = table.find("town/ai/carparks").expect("the resource");
        assert_eq!((resource.kind, resource.chunks), (KIND_RESOURCE, 4));
        assert!(table.find("town/docks_metadata.rpf").is_none());
        assert_eq!(table.by_word(0), None);
    }
}

#[test]
fn a_registered_file_names_its_entry_with_no_device_and_no_extension_first() {
    let table = Table::of(&table_of(&made_up(), 1)).expect("a table");
    let at = |name: &str| table.find_registered(name).map(|e| e.offset);
    assert_eq!(at("platform:/town/docks_metadata.rpf"), Some(0x10000));
    assert_eq!(at("$(target)/town/DOCKS_METADATA.rpf"), Some(0x10000));
    // A plain file keeps its extension, and a resource is registered with none.
    assert_eq!(at("$(root)/changesets/ccs_town.meta"), Some(0x200));
    assert_eq!(at("platform:/town/ai/carparks"), Some(0x400));
    assert_eq!(at("platform:/town/missing.rpf"), None);
    assert_eq!(entry_path("common:/data/a.meta"), "data/a.meta");
    assert_eq!(entry_path("$(common)/data/a.meta"), "data/a.meta");
    assert_eq!(entry_path("data/a.meta"), "data/a.meta");
    assert_eq!(split_extension("a.b/c_d.rpf"), ("a.b/c_d", "rpf"));
    assert_eq!(split_extension("a.b/c_d"), ("a.b/c_d", ""));
}

#[test]
fn an_entry_with_no_place_is_left_out() {
    let mut toc = table_of(&made_up(), 1);
    // The second entry's index into the place column, the second of the five main columns.
    let index = 0x40 + 24 * 6 + 5 + 1;
    toc.bytes[index] = 0xFF;
    let table = Table::of(&toc).expect("a table");
    assert_eq!(table.entries().len(), 3);
    assert!(table.find("changesets/ccs_town.meta").is_none());
}

#[test]
fn a_table_that_breaks_its_layout_is_refused() {
    let whole = table_of(&made_up(), 1);
    let without = |hash: u64| {
        let mut toc = whole.clone();
        toc.groups.retain(|g| g.name_hash != hash);
        Table::of(&toc)
    };
    assert_eq!(without(WORD), Err(TableError::NoColumn(WORD)));
    let mut plain = whole.clone();
    plain.groups.iter_mut().for_each(|g| {
        if g.name_hash == PLACE {
            g.width = 16;
        }
    });
    assert_eq!(
        Table::of(&plain),
        Err(TableError::Width {
            column: PLACE,
            found: 16,
            expected: 24
        })
    );
    // An index past the rows of its column names no row.
    let mut wild = whole.clone();
    let index = 0x40 + 24 * 6 + 2;
    wild.bytes[index] = 9;
    assert_eq!(
        Table::of(&wild),
        Err(TableError::Row {
            entry: 0,
            column: WORD,
            row: 9,
            rows: 4
        })
    );
    let mut cut = whole.clone();
    cut.bytes.truncate(0x40);
    assert!(matches!(Table::of(&cut), Err(TableError::IndexPastEnd(_))));
}
