use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::container::{RpfArchive, RpfBuilder, RpfEncryption, RpfEntry};

fn make_fake_rsc7(system_info: u32, graphics_info: u32, body: &[u8]) -> Vec<u8> {
    let seal = Rsc7::header_from_entry_flags(system_info, graphics_info);
    let mut blob = Vec::with_capacity(Rsc7::HEADER_LEN + body.len());
    blob.extend_from_slice(&seal.header_bytes());
    blob.extend_from_slice(body);
    blob
}

#[test]
fn synthetic_open_archive_roundtrips_bytes_for_every_entry() {
    let mut builder = RpfBuilder::default();

    let txt_payload = b"Hello, RPF! ".repeat(1024);
    let bik_payload = b"\x00\x01\x02\x03BIK!".repeat(64);
    let rsc7_payload = make_fake_rsc7(0xA000_4000, 0x5000_0000, &b"FAKE-RESOURCE-DATA".repeat(200));

    builder
        .insert_with("zeta/last.txt", txt_payload.clone(), true)
        .unwrap()
        .insert_with("alpha/first.txt", b"first".to_vec(), true)
        .unwrap()
        .insert_with("alpha/second.bik", bik_payload.clone(), false)
        .unwrap()
        .insert_with("alpha/sub/leaf.ydr", rsc7_payload.clone(), true)
        .unwrap()
        .insert_with("loose.bin", b"loose-bytes".to_vec(), true)
        .unwrap();

    let archive_bytes = builder.to_bytes().expect("save");

    assert!(archive_bytes.len() >= 2048);

    let archive = RpfArchive::from_bytes(archive_bytes).expect("load");

    assert_eq!(archive.header.encryption, RpfEncryption::Open);
    assert_eq!(archive.header.entry_count as usize, 1 + 3 + 5);
    assert_eq!(archive.file_count(), 5);
    assert_eq!(archive.dir_count(), 4);

    let cases: &[(&str, &[u8])] = &[
        ("alpha/first.txt", b"first"),
        ("alpha/second.bik", &bik_payload),
        ("alpha/sub/leaf.ydr", &rsc7_payload),
        ("zeta/last.txt", &txt_payload),
        ("loose.bin", b"loose-bytes"),
    ];
    for (path, expected) in cases {
        let entry_index = archive
            .find(path)
            .unwrap_or_else(|| panic!("entry {} not present in archive", path));
        let actual = archive.read_entry(entry_index).expect("read entry");
        assert_eq!(
            actual.as_slice(),
            *expected,
            "mismatch for entry {} (kind = {:?})",
            path,
            archive.entries[entry_index as usize].kind()
        );
    }
}

#[test]
fn extension_blacklist_skips_compression() {
    let payload = b"\x00\x01\x02\x03".repeat(2048);
    let archive = RpfBuilder::default()
        .insert_with("audio.bik", payload.clone(), false)
        .unwrap()
        .insert_with("text.txt", payload.clone(), true)
        .unwrap()
        .build()
        .unwrap();

    let bik_entry = archive.find("audio.bik").unwrap();
    let txt_entry = archive.find("text.txt").unwrap();

    if let RpfEntry::Binary(b) = &archive.entries[bik_entry as usize] {
        assert!(!b.is_compressed(), ".bik must not be compressed");
        assert_eq!(b.uncompressed_size as usize, payload.len());
    } else {
        panic!("audio.bik missing or wrong kind");
    }
    if let RpfEntry::Binary(b) = &archive.entries[txt_entry as usize] {
        assert!(b.is_compressed(), ".txt should compress (very repetitive)");
        assert!(b.consumed_size < b.uncompressed_size);
    } else {
        panic!("text.txt missing or wrong kind");
    }
}

#[test]
fn directory_traversal_uses_case_insensitive_sort() {
    let archive = RpfBuilder::default()
        .insert_with("Z_last.bin", b"z".to_vec(), true)
        .unwrap()
        .insert_with("a_first.bin", b"a".to_vec(), true)
        .unwrap()
        .insert_with("M_middle.bin", b"m".to_vec(), true)
        .unwrap()
        .build()
        .unwrap();

    let root_dir = match &archive.entries[0] {
        RpfEntry::Directory(d) => d,
        _ => panic!("entry 0 not directory"),
    };
    let mut names: Vec<String> = Vec::new();
    for i in 0..root_dir.entry_count {
        let idx = root_dir.first_entry_index + i;
        names.push(archive.entry_name(idx).unwrap());
    }
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "siblings must be alphabetically sorted");
}

#[test]
fn rebuild_after_mutation_preserves_expected_paths_and_bytes() {
    let original = RpfBuilder::default()
        .insert_with("a.txt", b"alpha".to_vec(), true)
        .expect("insert a")
        .insert_with("folder/b.txt", b"bravo".to_vec(), true)
        .expect("insert b")
        .build()
        .expect("build original");

    let mut editor = original.edit().expect("edit");
    editor.rename("a.txt", "renamed/a.txt").expect("rename a");
    editor.remove("folder/b.txt");
    editor
        .insert_with("new/c.txt", b"charlie".to_vec(), true)
        .expect("insert c");

    let reparsed = editor.build().expect("build rebuilt");
    assert_eq!(reparsed.read("renamed/a.txt").unwrap(), b"alpha");
    assert_eq!(reparsed.read("new/c.txt").unwrap(), b"charlie");
    assert!(reparsed.find("a.txt").is_none());
    assert!(reparsed.find("folder/b.txt").is_none());
}
