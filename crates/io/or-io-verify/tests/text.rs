//! GXT2 through its declarations: the header and directory against the file's
//! own words, the parse against the table written, and open, edit and save as
//! a fixed point.
//!
//! ⚠ **No GXT2 is staged**: every one the game ships is inside an archive, and
//! every retail archive's table is NG-encrypted, so reading them needs a mount
//! and a key snapshot. The tables here are the ones the writer produces, and
//! the shipped half is skipped with that reason.

use std::mem::{offset_of, size_of};

use or_io::decl::{describe, root, File, HasOrigin, Plain, Raw};
use or_io::gta5::text::*;
use or_io::Asset;
use or_test_support::skip_install;

const HEADER: usize = size_of::<Raw<CGxt2Header>>();
const ENTRY: usize = size_of::<Raw<CGxt2Entry>>();

fn tables() -> Vec<(String, Gxt2)> {
    let mut out = vec![("empty".to_string(), Gxt2::new())];
    for n in [1usize, 7, 300, 5000] {
        let mut t = Gxt2::new();
        for i in 0..n {
            let value = match i % 4 {
                0 => String::new(),
                1 => format!("Line {i}"),
                2 => "Peña — café ¡Hola! ".repeat(i % 5 + 1),
                _ => "x".repeat(i % 200),
            };
            t.insert_key(&format!("KEY_{i:05}"), value);
        }
        out.push((format!("{n} strings"), t));
    }
    out
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

#[test]
fn every_gxt2_table_reads_typed_as_the_parser_reads() {
    let (mut tables_n, mut strings) = (0usize, 0usize);
    for (name, table) in tables() {
        let bytes = table.to_bytes().unwrap();
        let header = Raw::<CGxt2Header>::read_from(&bytes).unwrap();
        assert_eq!(
            [header.magic, header.count],
            [word(&bytes, 0), word(&bytes, 4)]
        );
        assert_eq!(header.magic, GXT2_MAGIC, "{name}");
        let parsed = Gxt2::from_bytes(&bytes).unwrap();
        assert_eq!(parsed, table, "{name}");
        assert_eq!(header.count as usize, parsed.len(), "{name}");
        for (i, e) in parsed.entries.iter().enumerate() {
            let at = HEADER + i * ENTRY;
            let typed = Raw::<CGxt2Entry>::read_from(&bytes[at..]).unwrap();
            assert_eq!(
                [typed.hash.get(), typed.offset],
                [word(&bytes, at), word(&bytes, at + 4)]
            );
            assert_eq!(typed.hash.get(), e.hash, "{name} entry {i}");
            let start = typed.offset as usize;
            let end = start + bytes[start..].iter().position(|b| *b == 0).unwrap();
            assert_eq!(&bytes[start..end], e.value.as_bytes(), "{name} entry {i}");
            strings += 1;
        }
        let at = HEADER + parsed.len() * ENTRY;
        let last = Raw::<CGxt2Entry>::read_from(&bytes[at..]).unwrap();
        assert_eq!(last.hash.get(), GXT2_MAGIC, "{name}: the last entry's hash");
        assert_eq!(
            last.offset as usize,
            bytes.len(),
            "{name}: the last entry's offset is where the strings end"
        );
        tables_n += 1;
    }
    println!("{tables_n} tables, {strings} strings: every word reads typed as the parser reads it");
}

#[test]
fn every_gxt2_table_writes_to_a_fixed_point() {
    let mut n = 0;
    for (name, table) in tables() {
        let once = table.to_bytes().unwrap();
        let twice = Gxt2::from_bytes(&once).unwrap().to_bytes().unwrap();
        assert_eq!(twice, once, "{name}");
        n += 1;
    }
    println!("{n} of {n} tables write back to their own bytes");
}

#[test]
fn a_gxt2_opens_takes_a_key_and_saves_to_a_fixed_point() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("global.gxt2");
    let (_, written) = tables().swap_remove(2);
    std::fs::write(&path, written.to_bytes().unwrap()).unwrap();

    let mut table = Gxt2::open(&path).unwrap();
    assert_eq!(table.origin().path(), Some(path.as_path()));
    assert_eq!(table.insert_key("PM_PAUSE_MAP", "Map"), None);
    assert_eq!(
        table.insert_key("KEY_00001", "Replaced").as_deref(),
        Some("Line 1")
    );
    table.save().unwrap();
    let saved = std::fs::read(&path).unwrap();

    let reopened = Gxt2::open(&path).unwrap();
    assert_eq!(reopened, table);
    assert_eq!(reopened.get_by_key("PM_PAUSE_MAP"), Some("Map"));
    assert_eq!(reopened.get_by_key("KEY_00001"), Some("Replaced"));
    assert_eq!(reopened.len(), written.len() + 1);
    reopened.save().unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        saved,
        "a second save changes nothing"
    );

    let Asset::TextTable(opened) = or_io::open(&path).unwrap() else {
        panic!("a .gxt2 opens as a text table");
    };
    assert_eq!(opened, table);
    let Asset::TextTable(from_bytes) = or_io::from_bytes(&saved).unwrap() else {
        panic!("GXT2's magic names a text table");
    };
    assert_eq!(from_bytes, table);
}

#[test]
fn a_table_the_game_would_misread_is_refused() {
    let mut t = Gxt2::new();
    t.insert_key("A", "alpha");
    t.insert_key("B", "bravo");
    let good = t.to_bytes().unwrap();

    let mut past = good.clone();
    let at = HEADER + offset_of!(Raw<CGxt2Entry>, offset);
    past[at..at + 4].copy_from_slice(&(good.len() as u32 + 1).to_le_bytes());
    let past = Gxt2::from_bytes(&past)
        .expect_err("a string past the end")
        .to_string();

    let short = Gxt2::from_bytes(&good[..HEADER + ENTRY / 2])
        .expect_err("a directory cut short")
        .to_string();

    let mut magic = good.clone();
    magic[0] = b'X';
    let magic = Gxt2::from_bytes(&magic).expect_err("no magic").to_string();

    let mut nul = t.clone();
    nul.insert_key("A", "al\0pha");
    let nul = nul
        .to_bytes()
        .expect_err("a NUL inside a string")
        .to_string();

    let mut twice = t.clone();
    twice.entries.push(twice.entries[0].clone());
    let twice = twice.to_bytes().expect_err("one hash twice").to_string();
    println!("{past}\n{short}\n{magic}\n{nul}\n{twice}");
}

#[test]
fn the_declarations_describe_themselves_at_the_offsets_they_are_read_at() {
    let pack = describe(
        "gta5",
        "gen8",
        &[root::<Raw<CGxt2Header>>, root::<Raw<CGxt2Entry>>],
    )
    .unwrap();
    for (ty, size, expected) in [
        (
            "CGxt2Header",
            HEADER,
            [
                ("magic", offset_of!(Raw<CGxt2Header>, magic)),
                ("count", offset_of!(Raw<CGxt2Header>, count)),
            ],
        ),
        (
            "CGxt2Entry",
            ENTRY,
            [
                ("hash", offset_of!(Raw<CGxt2Entry>, hash)),
                ("offset", offset_of!(Raw<CGxt2Entry>, offset)),
            ],
        ),
    ] {
        let t = pack.types.iter().find(|t| t.name == ty).unwrap();
        let got: Vec<(&str, usize)> = t
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.offset))
            .collect();
        assert_eq!(t.size, size, "{ty}");
        assert_eq!(got, expected, "{ty}");
    }
}

#[test]
fn every_shipped_gxt2_reads_typed() {
    skip_install(
        "the shipped GXT2 tables",
        "every one is inside an NG-encrypted archive, so reading them needs a mount, and this \
         suite does not mount an install yet",
    );
}
