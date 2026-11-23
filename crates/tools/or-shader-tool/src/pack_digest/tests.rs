use std::collections::BTreeMap;

use or_io::gta5::shaders::spirv_pack::ShaderStageKind;
use or_io::gta5::shaders::spirv_pack::{
    Fidelity, NagaReach, Outcome, Production, Program, Reach, Reflection,
};

use super::*;

fn translated() -> Program {
    Program {
        stage: ShaderStageKind::Pixel,
        source_bytes: 1,
        outcome: Outcome::Translated {
            spirv_bytes: 4,
            fidelity: Fidelity::Equivalent,
            lowerings: BTreeMap::new(),
            naga: NagaReach {
                reached: Reach::MslWritten,
                cause: None,
            },
        },
        reflection: Reflection::default(),
    }
}

/// A pack folder holding two modules whose bytes are `first` and `second`, baked from `build`.
fn pack(dir: &Path, build: &str, first: &[u8], second: &[u8]) {
    let mut pack = SpirvPack::new(Production {
        translator: "t".into(),
        legalizer: "l".into(),
    });
    pack.source_build_hash = build.into();
    for (key, bytes) in [("aa11".repeat(10), first), ("bb22".repeat(10), second)] {
        pack.programs.insert(key.clone(), translated());
        let path = dir.join(module_path(&key));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    pack.programs.insert(
        "cc33".repeat(10),
        Program {
            outcome: Outcome::Refused {
                reason: "no".into(),
            },
            ..translated()
        },
    );
    std::fs::write(dir.join(MANIFEST_FILE), serde_json::to_vec(&pack).unwrap()).unwrap();
}

fn digest_of(build: &str, first: &[u8], second: &[u8]) -> (usize, String) {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path(), build, first, second);
    pack_digest(dir.path()).unwrap()
}

#[test]
fn a_digest_counts_the_translated_modules_and_is_forty_hex_digits() {
    let (count, digest) = digest_of("one", b"abcd", b"efgh");
    assert_eq!(count, 2, "the refused program has no module");
    assert_eq!(digest.len(), 40);
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn the_same_modules_baked_from_another_executable_have_the_same_digest() {
    assert_eq!(
        digest_of("one", b"abcd", b"efgh"),
        digest_of("another", b"abcd", b"efgh")
    );
}

#[test]
fn one_byte_of_one_module_changes_the_digest() {
    let base = digest_of("one", b"abcd", b"efgh");
    assert_ne!(base, digest_of("one", b"abcX", b"efgh"));
    assert_ne!(base, digest_of("one", b"abcd", b"efgX"));
}

#[test]
fn the_same_bytes_in_the_other_modules_are_not_the_same_pack() {
    assert_ne!(
        digest_of("one", b"abcd", b"efgh"),
        digest_of("one", b"efgh", b"abcd")
    );
}

#[test]
fn a_changed_manifest_changes_the_digest() {
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path(), "one", b"abcd", b"efgh");
    let before = pack_digest(dir.path()).unwrap();
    let path = dir.path().join(MANIFEST_FILE);
    let mut manifest: SpirvPack = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest.produced_by.legalizer = "m".into();
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert_ne!(pack_digest(dir.path()).unwrap(), before);
}

#[test]
fn a_missing_manifest_or_module_is_an_error_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(pack_digest(dir.path()).is_err());
    pack(dir.path(), "one", b"abcd", b"efgh");
    std::fs::remove_file(dir.path().join(module_path(&"aa11".repeat(10)))).unwrap();
    let error = pack_digest(dir.path()).unwrap_err();
    assert!(error.contains("aa11"), "{error}");
}
