//! Every archive generation reads and writes through the same calls RPF7 does.
//!
//! A round trip passes when the reader and the writer are wrong the same way, so each version also
//! has a small archive assembled here byte by byte from its published layout: the reader must
//! open it, and the writer must produce it.

use std::io::Cursor;

use or_io::container::rpf::{BodyForm, Defrag};
use or_io::container::{
    Compressor, RpfArchive, RpfBuilder, RpfEntry, RpfResourceEntry, RpfVersion,
};
use or_io::error::IoError;
use or_io::hash::string_hash;

const SECTOR: usize = 2048;

fn open(bytes: Vec<u8>) -> RpfArchive {
    RpfArchive::from_bytes(bytes).expect("opens")
}

fn index_of(archive: &RpfArchive, path: &str) -> u32 {
    archive.find(path).unwrap_or_else(|| {
        let paths: Vec<_> = archive.nodes.iter().map(|n| n.path()).collect();
        panic!("{path} is not in {paths:?}")
    })
}

/// A name as `version` keeps it, which is the text, a hash of it, or only the file's own name.
fn kept(version: RpfVersion, path: &str) -> String {
    match version {
        RpfVersion::Rpf3 => path
            .split('/')
            .map(|c| format!("hash_{:08X}", string_hash(c.as_bytes())))
            .collect::<Vec<_>>()
            .join("/"),
        RpfVersion::Img1 | RpfVersion::Img2 | RpfVersion::Img3 => {
            path.rsplit('/').next().unwrap_or(path).to_string()
        }
        _ => path.to_string(),
    }
}

fn payload(seed: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(seed)).collect()
}

fn files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("data/levels/map.txt", b"the same words again ".repeat(300)),
        ("data/small.bin", b"tiny".to_vec()),
        ("audio/one.dat", payload(7, 5000)),
        ("zeta.cfg", payload(3, 70)),
    ]
}

fn build(version: RpfVersion, files: &[(&str, Vec<u8>)]) -> RpfBuilder {
    let mut builder = RpfBuilder::new(version);
    for (path, bytes) in files {
        builder
            .insert_with(path, bytes.clone(), true)
            .expect("adds");
    }
    builder
}

fn built(version: RpfVersion, files: &[(&str, Vec<u8>)]) -> RpfArchive {
    build(version, files).build().expect("saves and opens")
}

const SINGLE_FILE_VERSIONS: [RpfVersion; 9] = [
    RpfVersion::Rpf0,
    RpfVersion::Rpf2,
    RpfVersion::Rpf3,
    RpfVersion::Rpf4,
    RpfVersion::Rpf6,
    RpfVersion::Rpf7,
    RpfVersion::Rpf8,
    RpfVersion::Img2,
    RpfVersion::Img3,
];

#[test]
fn every_version_reads_back_the_files_it_was_given() {
    for version in SINGLE_FILE_VERSIONS {
        let given = files();
        let archive = built(version, &given);
        assert_eq!(archive.version, version, "{version}");
        assert_eq!(archive.file_count(), given.len(), "{version}");

        for (path, bytes) in &given {
            let index = index_of(&archive, &kept(version, path));
            let got = archive.read_entry(index).expect("extracts");
            // An image keeps sectors, not lengths: the file comes back with its padding.
            if matches!(version, RpfVersion::Img2) {
                assert_eq!(got.len(), bytes.len().next_multiple_of(SECTOR), "{version}");
                assert!(got[bytes.len()..].iter().all(|&b| b == 0), "{version}");
                assert_eq!(&got[..bytes.len()], bytes.as_slice(), "{version} {path}");
            } else {
                assert_eq!(&got, bytes, "{version} {path}");
            }
        }
    }
}

#[test]
fn an_img1_is_a_directory_and_an_image() {
    let given = files();
    let builder = build(RpfVersion::Img1, &given);
    let (mut dir, mut img) = (Vec::new(), Cursor::new(Vec::new()));
    builder.write_img1(&mut dir, &mut img).expect("saves");
    assert!(builder.write_to(&mut Cursor::new(Vec::new())).is_err());

    let archive = RpfArchive::from_img1(dir, img.into_inner()).expect("opens");
    assert_eq!(archive.version, RpfVersion::Img1);
    for (path, bytes) in &given {
        let got = archive
            .read_entry(index_of(&archive, &kept(RpfVersion::Img1, path)))
            .expect("extracts");
        assert_eq!(&got[..bytes.len()], bytes.as_slice(), "{path}");
    }
    assert!(RpfBuilder::new(RpfVersion::Rpf7)
        .write_img1(&mut Vec::new(), &mut Cursor::new(Vec::new()))
        .is_err());
}

#[test]
fn the_preview_is_the_size_the_save_writes_and_a_second_save_is_the_same_bytes() {
    for version in SINGLE_FILE_VERSIONS {
        let builder = build(version, &files());
        let first = builder.to_bytes().expect("saves");
        assert_eq!(
            builder.planned_size().expect("planned"),
            first.len() as u64,
            "{version}"
        );
        assert_eq!(
            first,
            builder.to_bytes().expect("saves"),
            "{version} is not deterministic"
        );
    }
}

#[test]
fn every_body_sits_at_the_alignment_its_version_keeps() {
    for version in SINGLE_FILE_VERSIONS {
        let archive = built(version, &files());
        for entry in &archive.entries {
            let offset = match entry {
                RpfEntry::Binary(b) => b.file_offset,
                RpfEntry::Resource(r) => r.file_offset,
                RpfEntry::Directory(_) => continue,
            };
            assert_eq!(
                offset % version.alignment(),
                0,
                "{version} body at {offset:#x}"
            );
        }
    }
}

#[test]
fn a_compressible_file_is_deflated_where_the_version_deflates_and_kept_whole_in_an_image() {
    for version in SINGLE_FILE_VERSIONS {
        let archive = built(version, &files());
        let index = index_of(&archive, &kept(version, "data/levels/map.txt"));
        let entry = archive.binary_entry(index).expect("a binary");
        let image = matches!(version, RpfVersion::Img2 | RpfVersion::Img3);
        assert_eq!(entry.is_compressed(), !image, "{version}");
        // RPF8 compresses with its own Oodle, whose stream the game reads by the type it names.
        assert_eq!(
            entry.compressor,
            match version {
                RpfVersion::Img2 | RpfVersion::Img3 => Compressor::None,
                RpfVersion::Rpf8 => Compressor::Oodle,
                _ => Compressor::Deflate,
            },
            "{version}"
        );
    }
}

#[test]
fn a_directory_tree_is_kept_where_the_version_has_one() {
    for version in [
        RpfVersion::Rpf0,
        RpfVersion::Rpf2,
        RpfVersion::Rpf4,
        RpfVersion::Rpf6,
    ] {
        let archive = built(version, &files());
        assert_eq!(
            archive.dir_count(),
            1 + 3,
            "{version}: a root, data, levels and audio"
        );
        assert!(
            archive
                .nodes
                .iter()
                .any(|n| n.is_dir() && n.path() == "data/levels"),
            "{version}"
        );
    }
    for version in [RpfVersion::Rpf8, RpfVersion::Img2, RpfVersion::Img3] {
        assert_eq!(
            built(version, &files()).dir_count(),
            1,
            "{version}: only a root"
        );
    }
}

#[test]
fn a_hashed_version_names_a_file_by_the_hash_the_engine_gives_it() {
    let archive = built(RpfVersion::Rpf3, &files());
    let name = format!("hash_{:08X}", string_hash(b"zeta.cfg"));
    assert!(archive.nodes.iter().any(|n| n.path() == name), "{name}");
    assert_eq!(
        string_hash(b"ZETA.CFG"),
        string_hash(b"zeta.cfg"),
        "the hash folds case"
    );
}

/// The smallest archive of each version, assembled by hand: one file, `hello`, stored.
fn by_hand(version: RpfVersion) -> (Vec<u8>, &'static str, usize) {
    let put32 = |b: &mut Vec<u8>, at: usize, words: &[u32], big: bool| {
        for (i, w) in words.iter().enumerate() {
            let bytes = if big {
                w.to_be_bytes()
            } else {
                w.to_le_bytes()
            };
            b[at + 4 * i..at + 4 * i + 4].copy_from_slice(&bytes);
        }
    };
    let magic = version.magic().expect("a magic");
    let mut b: Vec<u8>;
    match version {
        RpfVersion::Rpf0 => {
            b = vec![0; 0x840];
            put32(&mut b, 0, &[magic, 35, 2], false);
            put32(
                &mut b,
                0x800,
                &[0x8000_0000, 1, 1, 1, 1, 0x830, 5, 5],
                false,
            );
            b[0x820..0x823].copy_from_slice(b"\0a\0");
            b[0x830..0x835].copy_from_slice(b"hello");
            (b, "a", 5)
        }
        RpfVersion::Rpf2 | RpfVersion::Rpf3 | RpfVersion::Rpf4 => {
            let (root, file) = if version == RpfVersion::Rpf3 {
                (string_hash(b""), string_hash(b"a"))
            } else {
                (0, 1)
            };
            let (heap, shift, at) = match version {
                RpfVersion::Rpf3 => (0, 0, 0x900),
                RpfVersion::Rpf2 => (3, 0, 0x900),
                _ => (3, 3, 0x1000),
            };
            b = vec![0; at + if shift == 3 { 2048 } else { 256 }];
            put32(&mut b, 0, &[magic, 32 + heap, 2, 0, 0, 0], false);
            put32(&mut b, 0x800, &[root, 0, 0x8000_0001, 1], false);
            put32(&mut b, 0x810, &[file, 5, (at >> shift) as u32, 5], false);
            if heap != 0 {
                b[0x820..0x823].copy_from_slice(b"\0a\0");
            }
            b[at..at + 5].copy_from_slice(b"hello");
            (
                b,
                if version == RpfVersion::Rpf3 {
                    "hash"
                } else {
                    "a"
                },
                5,
            )
        }
        RpfVersion::Rpf6 => {
            b = vec![0; 0x1013];
            b[..4].copy_from_slice(b"RPF6");
            put32(&mut b, 4, &[2, 0x200, 0], true);
            put32(&mut b, 16, &[string_hash(b""), 0, 0x8000_0001, 1, 0], true);
            put32(&mut b, 36, &[string_hash(b"a"), 5, 0x800 >> 3, 0, 0], true);
            b[0x800..0x805].copy_from_slice(b"hello");
            put32(&mut b, 0x1000, &[0, 0, 1, 0], true);
            b[0x1010..0x1013].copy_from_slice(b"\0a\0");
            (b, "a", 5)
        }
        RpfVersion::Rpf8 => {
            b = vec![0; 0x14A];
            put32(&mut b, 0, &[magic, 1, 10], false);
            b[12..16].copy_from_slice(&[0xFF, 0, b'y', 0]);
            let hash = u64::from(string_hash(b"a.txt")) | 0xFF << 40 | 0xFF << 48;
            b[272..280].copy_from_slice(&hash.to_le_bytes());
            b[280..288].copy_from_slice(&(1u64 | 19 << 28).to_le_bytes());
            b[288..296].copy_from_slice(&5u64.to_le_bytes());
            b[0x130..0x135].copy_from_slice(b"hello");
            b[0x140..0x145].copy_from_slice(b"a.txt");
            (b, "a.txt", 5)
        }
        RpfVersion::Img2 => {
            b = vec![0; 2 * SECTOR];
            put32(&mut b, 0, &[magic, 1, 1], false);
            b[12..14].copy_from_slice(&1u16.to_le_bytes());
            b[16..21].copy_from_slice(b"a.txt");
            b[SECTOR..SECTOR + 5].copy_from_slice(b"hello");
            (b, "a.txt", SECTOR)
        }
        RpfVersion::Img3 => {
            b = vec![0; 2 * SECTOR];
            put32(&mut b, 0, &[magic, 3, 1, 22], false);
            b[16..18].copy_from_slice(&16u16.to_le_bytes());
            put32(&mut b, 0x14, &[5, 1, 1], false);
            b[0x20..0x22].copy_from_slice(&1u16.to_le_bytes());
            b[0x22..0x24].copy_from_slice(&((SECTOR - 5) as u16).to_le_bytes());
            b[0x24..0x2A].copy_from_slice(b"a.txt\0");
            b[SECTOR..SECTOR + 5].copy_from_slice(b"hello");
            (b, "a.txt", 5)
        }
        _ => unreachable!("{version} has its own test"),
    }
}

#[test]
fn an_archive_assembled_by_hand_opens_and_is_what_the_writer_writes() {
    for version in SINGLE_FILE_VERSIONS
        .into_iter()
        .filter(|v| *v != RpfVersion::Rpf7)
    {
        let (bytes, name, len) = by_hand(version);
        let archive = open(bytes.clone());
        assert_eq!(archive.version, version, "{version}");
        let file = archive
            .nodes
            .iter()
            .find(|n| !n.is_dir())
            .expect("one file");
        assert!(file.path().starts_with(name), "{version}: {}", file.path());
        let got = archive.read_entry(file.entry_index()).expect("extracts");
        assert_eq!(got.len(), len, "{version}");
        assert_eq!(&got[..5], b"hello", "{version}");

        let named = if matches!(
            version,
            RpfVersion::Rpf8 | RpfVersion::Img2 | RpfVersion::Img3
        ) {
            "a.txt"
        } else {
            "a"
        };
        assert_eq!(
            RpfBuilder::new(version)
                .insert_with(named, b"hello".to_vec(), false)
                .expect("adds")
                .to_bytes()
                .expect("saves"),
            bytes,
            "{version}: the writer is not the layout assembled by hand"
        );
    }
}

#[test]
fn an_img1_assembled_by_hand_opens() {
    let mut dir = vec![0u8; 32];
    dir[4..8].copy_from_slice(&1u32.to_le_bytes());
    dir[8..13].copy_from_slice(b"a.txt");
    let mut img = vec![0u8; SECTOR];
    img[..5].copy_from_slice(b"hello");

    let archive = RpfArchive::from_img1(dir.clone(), img.clone()).expect("opens");
    assert_eq!(&archive.read("a.txt").expect("extracts")[..5], b"hello");

    let mut builder = RpfBuilder::new(RpfVersion::Img1);
    builder
        .insert_with("a.txt", b"hello".to_vec(), false)
        .expect("adds");
    let (mut written_dir, mut written_img) = (Vec::new(), Cursor::new(Vec::new()));
    builder
        .write_img1(&mut written_dir, &mut written_img)
        .expect("saves");
    assert_eq!((written_dir, written_img.into_inner()), (dir, img));
}

/// An entry of `version` that holds `words`, to be carried as it is.
fn stored_resource(system_info: u32, graphics_info: u32, tag: u32, len: usize) -> RpfEntry {
    RpfEntry::Resource(RpfResourceEntry {
        name_offset: 0,
        consumed_size: len as u32,
        file_offset: 0,
        system_info,
        graphics_info,
        compressor: Compressor::None,
        tag,
    })
}

#[test]
fn a_resource_before_rpf7_is_carried_as_stored_with_every_bit_of_its_row() {
    let cases = [
        (RpfVersion::Rpf2, 0x8000_1234, 0, 0x6E),
        (RpfVersion::Rpf3, 0x8000_00FF, 0, 0x08),
        (RpfVersion::Rpf4, 0xC123_4567, 0, 0x6E),
        (RpfVersion::Rpf6, 0xC000_00FF, 0xDEAD_BEEF, 0x09 | 1 << 8),
        (RpfVersion::Img3, 0x1234_5678, 0x20, 0x2000),
    ];
    for (version, system_info, graphics_info, tag) in cases {
        let body = payload(11, 44);
        let entry = stored_resource(system_info, graphics_info, tag, body.len());

        let archive = RpfBuilder::new(version)
            .insert_stored("models/car.xyz", entry, body.clone())
            .expect("carried")
            .build()
            .expect("saves and opens");
        let index = index_of(&archive, &kept(version, "models/car.xyz"));

        let RpfEntry::Resource(r) = &archive.entries[index as usize] else {
            panic!("{version}: not a resource");
        };
        assert_eq!(
            (r.consumed_size, r.system_info, r.graphics_info, r.tag),
            (44, system_info, graphics_info, tag),
            "{version}"
        );
        assert_eq!(
            archive.read_entry(index).expect("extracts"),
            body,
            "{version}"
        );
        assert_eq!(
            archive
                .read_form(index, BodyForm::AsStored)
                .expect("stored"),
            body,
            "{version}"
        );
    }
}

#[test]
fn a_resource_cannot_be_written_from_its_file_where_its_header_is_not_known() {
    for (version, magic) in [
        (RpfVersion::Rpf2, 0x0543_5352u32),
        (RpfVersion::Rpf4, 0x0543_5352),
        (RpfVersion::Rpf6, 0x0643_5352),
        (RpfVersion::Img3, 0x0543_5352),
    ] {
        let mut file = magic.to_le_bytes().to_vec();
        file.extend_from_slice(&payload(5, 40));
        let refused = RpfBuilder::new(version)
            .insert_with("a.res", file, false)
            .expect("adds")
            .to_bytes();
        assert!(
            matches!(refused, Err(IoError::Invalid { .. })),
            "{version}: {refused:?}"
        );
    }
}

/// An RSC8 file: the header a retail tool writes, then a payload that is not decoded here.
fn rsc8(system_info: u32, graphics_info: u32, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut file = 0x3843_5352u32.to_le_bytes().to_vec();
    for word in [flags, system_info, graphics_info] {
        file.extend_from_slice(&word.to_le_bytes());
    }
    file.extend_from_slice(payload);
    file
}

#[test]
fn an_rpf8_resource_is_written_from_its_header_and_extracts_as_the_same_file() {
    // Resource id 0x65 from the low nibbles of the page flags, Oodle, no signature, no key, no config.
    let file = rsc8(0x1234_5676, 0x0000_0005, 0x165, &payload(9, 100));
    let archive = RpfBuilder::new(RpfVersion::Rpf8)
        .insert_with("x/y.ydr", file.clone(), true)
        .expect("adds")
        .build()
        .expect("saves and opens");

    let index = index_of(&archive, "x/y.ydr");
    let RpfEntry::Resource(r) = &archive.entries[index as usize] else {
        panic!("not a resource");
    };
    assert_eq!((r.system_info, r.graphics_info), (0x1234_5676, 5));
    assert_eq!(r.compressor, Compressor::Oodle);
    assert_eq!(r.tag & 0xFF, 0xFF, "no key");
    // A row counts a resource in sixteen-byte units, so the file comes back with its zero padding.
    let got = archive.read_entry(index).expect("extracts");
    assert_eq!(got.len(), file.len().next_multiple_of(16));
    assert_eq!(&got[..file.len()], file.as_slice());
    assert!(got[file.len()..].iter().all(|&b| b == 0));
}

#[test]
fn a_body_that_is_not_an_oodle_stream_is_refused_and_still_carried() {
    let entry = RpfEntry::Binary(or_io::container::RpfBinaryEntry {
        name_offset: 0,
        consumed_size: 64,
        file_offset: 0,
        uncompressed_size: 200,
        encrypted: false,
        encrypted_raw: 0,
        compressor: Compressor::Oodle,
        tag: 0xFF,
    });
    let archive = RpfBuilder::new(RpfVersion::Rpf8)
        .insert_stored("a/b.dat", entry, payload(2, 64))
        .expect("carried")
        .build()
        .expect("saves and opens");
    let index = index_of(&archive, "a/b.dat");

    assert!(matches!(
        archive.read_entry(index),
        Err(IoError::DecompressionFailed(_))
    ));
    assert_eq!(
        archive
            .read_form(index, BodyForm::AsStored)
            .expect("stored"),
        payload(2, 64)
    );
}

#[test]
fn a_body_the_title_encrypts_is_refused_not_returned_as_noise() {
    let mut entry = stored_resource(0x10, 0x10, 0x3A, 32);
    if let RpfEntry::Resource(r) = &mut entry {
        r.tag = 0x3A;
        r.compressor = Compressor::Oodle;
    }
    let archive = RpfBuilder::new(RpfVersion::Rpf8)
        .insert_stored("a.ydr", entry, vec![1; 32])
        .expect("carried")
        .build()
        .expect("saves and opens");
    let refused = archive.read_entry(index_of(&archive, "a.ydr"));
    assert!(
        matches!(
            refused,
            Err(IoError::RpfEncryptionUnsupported { key_id: 0x3A })
        ),
        "{refused:?}"
    );
}

#[test]
fn defrag_carries_every_body_as_stored_in_every_version_but_rpf7() {
    for version in SINGLE_FILE_VERSIONS {
        let archive = built(version, &files());
        let defrag = Defrag::plan(&archive, |_, _| {}).expect("plans");
        assert_eq!(defrag.files, archive.file_count(), "{version}");

        let out = defrag.builder.to_bytes().expect("saves");
        assert_eq!(out.len() as u64, defrag.planned, "{version}");

        let again = open(out);
        assert_eq!(again.version, version);
        for node in archive.nodes.iter().filter(|n| !n.is_dir()) {
            let mine = archive.read_entry(node.entry_index()).expect("extracts");
            let theirs = again
                .read_entry(index_of(&again, node.path()))
                .expect("extracts");
            assert_eq!(mine, theirs, "{version} {}", node.path());
        }
    }
}

#[test]
fn an_archive_is_found_by_its_magic_when_it_is_opened_from_bytes_and_from_a_path() {
    let bytes = build(RpfVersion::Rpf4, &files()).to_bytes().expect("saves");
    let archive = RpfArchive::from_bytes(bytes.clone()).expect("loads");
    assert_eq!(archive.version, RpfVersion::Rpf4);

    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("max_payne.rpf");
    std::fs::write(&path, &bytes).expect("writes");
    assert_eq!(
        RpfArchive::open(&path).expect("opens").version,
        RpfVersion::Rpf4
    );
}

#[test]
fn a_table_that_asks_for_more_than_the_file_holds_is_refused_before_anything_is_allocated() {
    let heads: [(RpfVersion, Vec<(usize, u32)>); 5] = [
        (RpfVersion::Rpf0, vec![(4, u32::MAX), (8, u32::MAX)]),
        (RpfVersion::Rpf2, vec![(4, 100), (8, 2)]),
        (RpfVersion::Rpf6, vec![(4, u32::MAX)]),
        (RpfVersion::Rpf8, vec![(4, u32::MAX)]),
        (RpfVersion::Img2, vec![(4, u32::MAX)]),
    ];
    for (version, words) in heads {
        let mut bytes = vec![0u8; 64];
        bytes[..4].copy_from_slice(&version.magic().expect("magic").to_le_bytes());
        for (at, word) in words {
            let word = if version == RpfVersion::Rpf6 {
                word.to_be_bytes()
            } else {
                word.to_le_bytes()
            };
            bytes[at..at + 4].copy_from_slice(&word);
        }
        let outcome = RpfArchive::from_bytes(bytes);
        assert!(outcome.is_err(), "{version} opened a table it cannot hold");
    }
}

#[test]
fn a_directory_that_lists_itself_is_refused_in_a_classic_table() {
    let mut bytes = vec![0u8; 0x900];
    let put = |b: &mut Vec<u8>, at: usize, words: [u32; 4]| {
        for (i, w) in words.iter().enumerate() {
            b[at + 4 * i..at + 4 * i + 4].copy_from_slice(&w.to_le_bytes());
        }
    };
    put(
        &mut bytes,
        0,
        [RpfVersion::Rpf2.magic().expect("magic"), 33, 2, 0],
    );
    put(&mut bytes, 0x800, [0, 0, 0x8000_0001, 1]);
    put(&mut bytes, 0x810, [0, 0, 0x8000_0001, 1]);
    assert!(RpfArchive::from_bytes(bytes).is_err());
}

#[test]
fn a_table_the_title_encrypts_is_refused_with_its_tag() {
    let mut rpf2 = by_hand(RpfVersion::Rpf2).0;
    rpf2[16..20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    let mut rpf8 = by_hand(RpfVersion::Rpf8).0;
    rpf8[12] = 0x0A;
    for (bytes, tag) in [(rpf2, 0xFFFF_FFFF), (rpf8, 0x0A)] {
        let refused = RpfArchive::from_bytes(bytes);
        assert!(
            matches!(refused, Err(IoError::RpfEncryptionUnsupported { key_id }) if key_id == tag),
            "{refused:?}"
        );
    }
}

#[test]
fn an_rpf6_body_may_be_zstd() {
    // A zstd frame holding `hello` as one raw block.
    let frame = [
        0x28, 0xB5, 0x2F, 0xFD, 0x20, 0x05, 0x29, 0x00, 0x00, b'h', b'e', b'l', b'l', b'o',
    ];
    let mut bytes = by_hand(RpfVersion::Rpf6).0;
    // Row 1: on disk the frame, 5 bytes once decoded, flagged compressed.
    bytes[36 + 4..36 + 8].copy_from_slice(&(frame.len() as u32).to_be_bytes());
    bytes[36 + 12..36 + 16].copy_from_slice(&(0x4000_0005u32).to_be_bytes());
    bytes[0x800..0x800 + frame.len()].copy_from_slice(&frame);

    let archive = open(bytes);
    assert_eq!(archive.read("a").expect("decodes"), b"hello");
}

#[test]
fn rpf7_is_still_rpf7() {
    let builder = RpfBuilder::default();
    assert_eq!(builder.version(), RpfVersion::Rpf7);
    let archive = built(RpfVersion::Rpf7, &files());
    assert_eq!(archive.header.entry_count as usize, archive.entries.len());
}
