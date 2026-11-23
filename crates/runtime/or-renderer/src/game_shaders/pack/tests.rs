use std::collections::BTreeMap;

use or_io::gta5::shaders::spirv_pack::ShaderStageKind;
use or_io::gta5::shaders::spirv_pack::{
    Effect, Fidelity, NagaReach, Outcome, Pass, Production, Program, Reach, Reflection, Technique,
};

use super::*;

/// A WGSL module written out as the SPIR-V a pack holds.
fn spirv_of(source: &str) -> Vec<u8> {
    let module = naga::front::wgsl::parse_str(source).expect("the WGSL parses");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("the module validates");
    let words =
        naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
            .expect("naga writes it");
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

const KEY: &str = "ab12cd34";

fn manifest() -> SpirvPack {
    let mut pack = SpirvPack::new(Production {
        translator: "test".into(),
        legalizer: "test".into(),
    });
    pack.programs.insert(
        KEY.into(),
        Program {
            stage: ShaderStageKind::Pixel,
            source_bytes: 10,
            outcome: Outcome::Translated {
                spirv_bytes: 0,
                fidelity: Fidelity::Equivalent,
                lowerings: BTreeMap::new(),
                naga: NagaReach {
                    reached: Reach::MslWritten,
                    cause: None,
                },
            },
            reflection: Reflection::default(),
        },
    );
    pack.effects.push(Effect {
        name: "e".into(),
        source_path: "e.fxc".into(),
        vertex_type: 0,
        defaults: Default::default(),
        techniques: vec![Technique {
            name: "draw".into(),
            passes: vec![Pass {
                stages: BTreeMap::from([(ShaderStageKind::Pixel, KEY.to_string())]),
            }],
        }],
    });
    pack
}

/// A pack folder holding `manifest` and, when given, one module for `KEY`.
fn pack_on_disk(manifest: &SpirvPack, module: Option<&[u8]>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(MANIFEST_FILE),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
    if let Some(module) = module {
        let path = dir.path().join(module_path(KEY));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, module).unwrap();
    }
    dir
}

#[test]
fn a_pack_opens_and_a_program_is_read_from_its_module() {
    let module =
        spirv_of("@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }");
    let dir = pack_on_disk(&manifest(), Some(&module));
    let pack = GamePack::open(dir.path()).unwrap();
    assert_eq!(pack.manifest().effects[0].name, "e");
    assert_eq!(pack.root(), Some(dir.path()));
    let read = pack.module(KEY).unwrap();
    assert_eq!(read.entry_points.len(), 1);
    assert_eq!(read.entry_points[0].stage, naga::ShaderStage::Fragment);
}

#[test]
fn a_folder_with_no_manifest_is_named_and_so_is_one_that_does_not_parse() {
    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(
        GamePack::open(empty.path()),
        Err(PackError::ManifestMissing(_))
    ));
    std::fs::write(empty.path().join(MANIFEST_FILE), b"{ not json").unwrap();
    assert!(matches!(
        GamePack::open(empty.path()),
        Err(PackError::ManifestParse { .. })
    ));
}

#[test]
fn a_pack_of_another_version_is_refused_with_both_versions() {
    let mut other = manifest();
    other.schema_version = PACK_VERSION + 1;
    let dir = pack_on_disk(&other, None);
    let Err(error) = GamePack::open(dir.path()) else {
        panic!("a pack of another version opened");
    };
    assert!(matches!(
        error,
        PackError::Version { found, expected } if found == PACK_VERSION + 1 && expected == PACK_VERSION
    ));
    assert!(error
        .to_string()
        .contains(&format!("version {}", PACK_VERSION + 1)));
}

#[test]
fn a_module_that_is_missing_or_not_spirv_is_an_error_naming_its_program() {
    let dir = pack_on_disk(&manifest(), None);
    let pack = GamePack::open(dir.path()).unwrap();
    let Err(PackError::ModuleRead { key, .. }) = pack.module(KEY) else {
        panic!("a missing module read");
    };
    assert_eq!(key, KEY);

    let dir = pack_on_disk(&manifest(), Some(b"not spirv at all, not at all"));
    let pack = GamePack::open(dir.path()).unwrap();
    let Err(PackError::ModuleParse { key, message }) = pack.module(KEY) else {
        panic!("garbage read as a module");
    };
    assert_eq!(key, KEY);
    assert!(!message.is_empty());
}

#[test]
fn a_pack_in_memory_gives_its_modules_and_names_why_a_refused_program_has_none() {
    let module = naga::front::wgsl::parse_str(
        "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(0.5); }",
    )
    .expect("the WGSL parses");
    let mut manifest = manifest();
    manifest.programs.insert(
        "refused".into(),
        Program {
            stage: ShaderStageKind::Vertex,
            source_bytes: 3,
            outcome: Outcome::Refused {
                reason: "no bitcode in it".into(),
            },
            reflection: Reflection::default(),
        },
    );
    let pack = GamePack::in_memory(
        manifest,
        HashMap::from([(KEY.to_string(), std::sync::Arc::new(module))]),
    );
    assert_eq!(pack.root(), None);
    assert_eq!(pack.manifest().effects[0].name, "e");
    let read = pack.module(KEY).unwrap();
    assert_eq!(read.entry_points[0].stage, naga::ShaderStage::Fragment);

    let Err(PackError::NotTranslated { key, reason }) = pack.module("refused") else {
        panic!("a refused program gave a module");
    };
    assert_eq!(
        (key.as_str(), reason.as_str()),
        ("refused", "no bitcode in it")
    );
    let Err(PackError::NotTranslated { reason, .. }) = pack.module("absent") else {
        panic!("a program the pack does not hold gave a module");
    };
    assert_eq!(reason, "the pack holds no module for it");
}
