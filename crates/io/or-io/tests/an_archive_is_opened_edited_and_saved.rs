//! An archive on disk is opened, edited as a builder and saved, which is how the editor and the CLI
//! change one: `RpfArchive::open`, `edit`, `insert`, `remove`, `rename` and `save`.

use std::path::Path;
use std::sync::Arc;

use or_io::container::rpf::{
    BodyForm, RpfArchive, RpfBuilder, RpfCryptoProvider, RpfEncryption, RpfVersion, RPF_AES_MAGIC,
};
use or_io::container::{Gta5Keys, Gta5RetailCryptoProvider};

fn text() -> Vec<u8> {
    b"the same words again ".repeat(200)
}

fn blob() -> Vec<u8> {
    (0..3000u32).map(|i| (i * 7) as u8).collect()
}

/// The files every archive here starts with: one compressible, one that is not, and one loose.
fn starting_files(builder: &mut RpfBuilder) {
    builder
        .insert_with("data/a.txt", text(), true)
        .unwrap()
        .insert_with("data/b.bin", blob(), false)
        .unwrap()
        .insert_with("c.dat", b"loose".to_vec(), true)
        .unwrap();
}

fn written(version: RpfVersion, path: &Path) {
    let mut builder = RpfBuilder::new(version);
    starting_files(&mut builder);
    builder.save(path).unwrap();
}

#[test]
fn an_rpf7_is_opened_edited_and_saved_to_another_path() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from.rpf"), dir.path().join("to.rpf"));
    written(RpfVersion::Rpf7, &from);
    let before = std::fs::read(&from).unwrap();

    let mut edit = RpfArchive::open(&from).unwrap().edit().unwrap();
    edit.insert("Data/New.txt", b"new".to_vec()).unwrap();
    assert_eq!(edit.remove("c.dat"), 1);
    assert_eq!(edit.rename("data/b.bin", "data/renamed.bin").unwrap(), 1);
    edit.save(&to).unwrap();

    assert_eq!(
        std::fs::read(&from).unwrap(),
        before,
        "the source is as it was"
    );
    let saved = RpfArchive::open(&to).unwrap();
    assert_eq!(saved.file_count(), 3);
    assert_eq!(saved.read("data/a.txt").unwrap(), text());
    assert_eq!(saved.read("data/renamed.bin").unwrap(), blob());
    assert_eq!(saved.read("data/new.txt").unwrap(), b"new");
    assert!(saved.find("c.dat").is_none());
    assert!(saved.find("data/b.bin").is_none());
}

#[test]
fn a_save_replaces_the_file_the_archive_was_opened_from() {
    for version in [RpfVersion::Rpf7, RpfVersion::Rpf8, RpfVersion::Rpf2] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack.rpf");
        written(version, &path);

        // ⚠ Taken before the save: an opened archive reads its file when asked, and the file is replaced.
        let stored = |archive: &RpfArchive| {
            ["data/a.txt", "data/b.bin"].map(|name| {
                let index = archive.find(name).unwrap();
                archive.read_form(index, BodyForm::AsStored).unwrap()
            })
        };
        let opened = RpfArchive::open(&path).unwrap();
        let stored_before = stored(&opened);
        let mut edit = opened.edit().unwrap();
        edit.insert("data/new.txt", b"new".to_vec()).unwrap();
        assert_eq!(edit.remove("c.dat"), 1);
        edit.save(&path).unwrap();

        let saved = RpfArchive::open(&path).unwrap();
        assert_eq!(saved.version, version);
        assert_eq!(saved.file_count(), 3, "{version}");
        assert_eq!(saved.read("data/a.txt").unwrap(), text(), "{version}");
        assert_eq!(saved.read("data/b.bin").unwrap(), blob(), "{version}");
        assert_eq!(saved.read("data/new.txt").unwrap(), b"new", "{version}");
        assert!(saved.find("c.dat").is_none(), "{version}");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "{version}: the temporary file is gone"
        );

        // Every version but RPF7 carries a body as it is stored, deflated and all.
        if version != RpfVersion::Rpf7 {
            assert_eq!(stored_before, stored(&saved), "{version}");
        }
    }
}

fn synthetic_keys() -> Gta5Keys {
    let mut k = Gta5Keys::empty();
    k.aes_key = [0xA5u8; 32];
    for (i, b) in k.lut.iter_mut().enumerate() {
        *b = i as u8;
    }
    k.ng_keys = vec![[0u8; 0x110]; 101];
    k.ng_decrypt_tables = vec![[[0u32; 256]; 16]; 17];
    k.provenance = or_io::container::rpf::crypto::KeyProvenance::all_from_executable();
    k
}

#[test]
fn an_edit_of_an_encrypted_rpf7_keeps_its_encryption() {
    let keys: Arc<dyn RpfCryptoProvider> =
        Arc::new(Gta5RetailCryptoProvider::new(Arc::new(synthetic_keys())));
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from.rpf"), dir.path().join("to.rpf"));
    let mut builder = RpfBuilder::default().encrypted(RpfEncryption::Aes, Arc::clone(&keys));
    starting_files(&mut builder);
    builder.save(&from).unwrap();

    let opened = RpfArchive::open_with(&from, Arc::clone(&keys)).unwrap();
    assert_eq!(opened.header.encryption, RpfEncryption::Aes);
    let mut edit = opened.edit().unwrap();
    edit.insert("data/new.txt", text()).unwrap();
    edit.save(&to).unwrap();

    let head = std::fs::read(&to).unwrap();
    assert_eq!(
        u32::from_le_bytes(head[12..16].try_into().unwrap()),
        RPF_AES_MAGIC
    );
    assert!(
        RpfArchive::open(&to).is_err(),
        "an edit that dropped the encryption would open without keys"
    );
    let saved = RpfArchive::open_with(&to, keys).unwrap();
    assert_eq!(saved.header.encryption, RpfEncryption::Aes);
    for path in ["data/a.txt", "data/new.txt"] {
        let entry = saved.binary_entry(saved.find(path).unwrap()).unwrap();
        assert!(entry.encrypted, "{path} is stored encrypted");
    }
    assert_eq!(saved.read("data/a.txt").unwrap(), text());
    assert_eq!(saved.read("data/new.txt").unwrap(), text());
    assert_eq!(saved.read("data/b.bin").unwrap(), blob());
}

#[test]
fn a_path_is_found_without_regard_to_case_or_slash() {
    let mut builder = RpfBuilder::default();
    builder
        .insert_with("Data/Levels/Map.txt", text(), true)
        .unwrap()
        .insert_with("Other.bin", blob(), false)
        .unwrap();
    let archive = builder.build().unwrap();

    let index = archive
        .find("data/levels/map.txt")
        .expect("found as stored");
    for spelling in [
        "DATA\\LEVELS\\MAP.TXT",
        "Data/Levels/Map.txt",
        "/data/levels/map.txt/",
        "data//levels\\map.txt",
    ] {
        assert_eq!(archive.find(spelling), Some(index), "{spelling}");
    }
    assert_eq!(archive.entry_path(index).unwrap(), "data/levels/map.txt");
    assert_eq!(archive.read("DATA/levels/MAP.txt").unwrap(), text());
    let directory = archive
        .find("Data\\Levels")
        .expect("a directory is found too");
    assert!(archive.node_for_entry(directory).unwrap().is_dir());
    assert!(archive.find("data/levels/missing.txt").is_none());
    assert!(archive.read("data/levels/missing.txt").is_err());
}

#[test]
fn an_img1_opens_by_its_img_and_by_its_dir() {
    let dir = tempfile::tempdir().unwrap();
    let img = dir.path().join("pack.img");
    RpfBuilder::new(RpfVersion::Img1)
        .insert_with("a.txt", text(), false)
        .unwrap()
        .insert_with("b.bin", blob(), false)
        .unwrap()
        .save(&img)
        .unwrap();
    assert!(
        img.with_extension("dir").is_file(),
        "the directory is saved beside the image"
    );

    for opened in [img.clone(), img.with_extension("dir")] {
        let archive = RpfArchive::open(&opened).unwrap();
        assert_eq!(archive.version, RpfVersion::Img1, "{}", opened.display());
        assert_eq!(archive.file_count(), 2);
        // An image keeps sectors, not lengths: the file comes back with its padding.
        assert!(archive.read("a.txt").unwrap().starts_with(&text()));
        assert!(archive.read("b.bin").unwrap().starts_with(&blob()));
    }

    let mut edit = RpfArchive::open(img.with_extension("dir"))
        .unwrap()
        .edit()
        .unwrap();
    assert_eq!(edit.remove("a.txt"), 1);
    edit.insert_with("c.bin", b"new".to_vec(), false).unwrap();
    edit.save(&img).unwrap();
    let saved = RpfArchive::open(img.with_extension("dir")).unwrap();
    assert_eq!(saved.file_count(), 2);
    assert!(saved.find("a.txt").is_none());
    assert!(saved.read("c.bin").unwrap().starts_with(b"new"));
    assert!(saved.read("b.bin").unwrap().starts_with(&blob()));
}
