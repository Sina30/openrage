use std::path::PathBuf;
use std::sync::Arc;

use or_io::container::{
    extract_keys, fingerprint_executable, Gta5Keys, Gta5RetailCryptoProvider, KeysCache,
    RpfArchive, RpfBuilder, RpfCryptoProvider, RpfEncryption,
};

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
fn aes_archive_roundtrips_through_retail_provider() {
    let keys = Arc::new(synthetic_keys());
    let provider: Arc<dyn RpfCryptoProvider> =
        Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&keys)));

    let bytes = RpfBuilder::default()
        .encrypted(RpfEncryption::Aes, Arc::clone(&provider))
        .insert_with(
            "readme.txt",
            b"hello, retail RPF! ".repeat(64).to_vec(),
            true,
        )
        .unwrap()
        .insert_with(
            "data/inner.bin",
            b"binary-payload-x".repeat(32).to_vec(),
            true,
        )
        .unwrap()
        .to_bytes()
        .expect("save aes");

    // Header magic intact, encryption tag is the AES key id.
    assert_eq!(
        u32::from_le_bytes(bytes[..4].try_into().unwrap()),
        or_io::container::rpf::RPF7_MAGIC
    );
    assert_eq!(
        u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        or_io::container::rpf::RPF_AES_MAGIC
    );

    let archive =
        RpfArchive::from_bytes_with(Some("synthetic.rpf"), bytes.clone(), Arc::clone(&provider))
            .expect("read with retail provider");
    assert_eq!(archive.file_count(), 2);

    let cases: &[(&str, &[u8])] = &[
        ("readme.txt", &b"hello, retail RPF! ".repeat(64)),
        ("data/inner.bin", &b"binary-payload-x".repeat(32)),
    ];
    for (path, expected) in cases {
        let bytes = archive.read(path).expect("extract");
        assert_eq!(
            bytes.as_slice(),
            *expected,
            "AES roundtrip mismatch for {}",
            path
        );
    }
}

// Body decryption picks an NG sub-key by hashing the **filename**; the full
// path (`data/action/branches.meta`) selects a different NG sub-key.
#[test]
fn nested_path_uses_filename_for_per_entry_crypto() {
    let keys = Arc::new(synthetic_keys());
    let payload = b"<?xml version=\"1.0\"?>".repeat(64);
    let provider: Arc<dyn RpfCryptoProvider> =
        Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&keys)));
    let bytes = RpfBuilder::default()
        .encrypted(RpfEncryption::Aes, Arc::clone(&provider))
        .insert_with("data/action/branches.meta", payload.clone(), true)
        .unwrap()
        .to_bytes()
        .expect("save");

    let archive =
        RpfArchive::from_bytes_with(Some("synthetic.rpf"), bytes, provider).expect("TOC parse");

    let entry_idx = archive
        .find("data/action/branches.meta")
        .expect("entry not found");

    // The bare name, not the path, is what NG key derivation hashes.
    assert_eq!(
        archive.entry_name(entry_idx).as_deref(),
        Some("branches.meta"),
        "entry_name must strip the parent path"
    );

    let extracted = archive
        .read_entry(entry_idx)
        .expect("extract nested-path entry");
    assert_eq!(extracted, payload, "round-trip body must match");
}

#[test]
fn aes_archive_must_have_provider_to_decrypt() {
    let keys = Arc::new(synthetic_keys());
    let bytes = RpfBuilder::default()
        .encrypted(
            RpfEncryption::Aes,
            Arc::new(Gta5RetailCryptoProvider::new(keys)),
        )
        .insert_with("hello.txt", b"world".to_vec(), true)
        .unwrap()
        .to_bytes()
        .unwrap();

    let err = RpfArchive::from_bytes(bytes).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("encryption") || msg.contains("not supported"),
        "unexpected error: {msg}"
    );
}

#[test]
fn open_archive_still_loads_with_retail_provider() {
    let keys = Arc::new(synthetic_keys());
    let provider = Gta5RetailCryptoProvider::new(keys);
    let bytes = RpfBuilder::default()
        .insert_with("a.txt", b"plain-bytes".to_vec(), true)
        .unwrap()
        .to_bytes()
        .unwrap();

    let archive = RpfArchive::from_bytes_with(Some("open.rpf"), bytes, Arc::new(provider))
        .expect("retail provider on Open");
    assert_eq!(archive.file_count(), 1);
}

#[test]
fn cache_roundtrip_through_disk() {
    let keys = synthetic_keys();
    let scratch = tempfile::tempdir().unwrap();
    let cache_path = scratch.path().join("cache_roundtrip.bin");

    let cache = KeysCache::build(
        PathBuf::from("/games/gta5.exe"),
        or_io::container::rpf::crypto::ExeFingerprint {
            sha1_hex: "deadbeef".repeat(5),
            size: 1024,
        },
        &keys,
    );
    cache.write_to(&cache_path).expect("write cache");

    let loaded = KeysCache::read_from(&cache_path)
        .expect("read cache")
        .expect("cache present");
    assert_eq!(loaded.metadata.gta5_size, 1024);
    let restored = loaded.into_keys().expect("decode keys");
    assert_eq!(restored.aes_key, keys.aes_key);
    assert_eq!(restored.lut, keys.lut);
    assert_eq!(restored.ng_keys.len(), keys.ng_keys.len());
}

#[test]
fn fingerprint_changes_with_file_content() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("fp_a.bin");
    let b = dir.path().join("fp_b.bin");
    std::fs::write(&a, b"alpha").unwrap();
    std::fs::write(&b, b"beta").unwrap();
    let fa = fingerprint_executable(&a).unwrap();
    let fb = fingerprint_executable(&b).unwrap();
    assert_ne!(fa.sha1_hex, fb.sha1_hex);
}

#[test]
fn aes_save_is_deterministic_for_same_keys_and_input() {
    let keys = Arc::new(synthetic_keys());
    let provider1 = Gta5RetailCryptoProvider::new(Arc::clone(&keys));
    let provider2 = Gta5RetailCryptoProvider::new(Arc::clone(&keys));

    let build = || -> Vec<u8> {
        let mut b = RpfBuilder::default().encrypted(
            RpfEncryption::Aes,
            Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&keys))),
        );
        b.insert_with("a/b.txt", b"deterministic-payload".to_vec(), true)
            .unwrap();
        b.to_bytes().unwrap()
    };
    let a = build();
    let b = build();
    assert_eq!(&a[..16 + 16 * 2], &b[..16 + 16 * 2]);
    let _ = (provider1, provider2);
}

#[test]
fn extractor_rejects_files_missing_aes_signature() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no_aes.exe");
    let payload = vec![0xCCu8; 1 << 20];
    std::fs::write(&path, &payload).unwrap();

    let progress = |_: or_io::container::ExtractionProgress| {};
    let err = extract_keys(&path, &progress).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("no AES key in"), "unexpected error: {msg}");
    // A refusal that does not say which of the four ways this file is wrong
    // sends the user back to the same guess they arrived with.
    assert!(
        msg.contains("not a Windows executable"),
        "the refusal named no cause: {msg}"
    );
}
