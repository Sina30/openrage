use std::collections::BTreeMap;

use or_io::gta5::shaders::spirv_pack::{
    Fidelity, NagaReach, Production, Program, Reach, Reflection,
};

use super::*;

/// The SPIR-V naga writes for `source`, standing in for a legalized module.
fn spirv_of(source: &str) -> Vec<u8> {
    let module = naga::front::wgsl::parse_str(source).expect("the WGSL parses");
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .expect("it validates");
    naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
        .expect("naga writes it")
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect()
}

const PLAIN: &str =
    "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(1.0, 0.5, 0.0, 1.0); }";

/// A module that needs 16-bit floats, which a device offers only as an option.
const HALF: &str = "enable f16;
@fragment fn main() -> @location(0) vec4<f32> {
    let h: f16 = f16(0.5);
    return vec4<f32>(f32(h));
}";

#[test]
fn a_plain_module_is_written_by_every_writer() {
    let results = check_module(&spirv_of(PLAIN));
    assert_eq!(results.len(), Backend::ALL.len());
    for backend in Backend::ALL {
        assert_eq!(results[&backend], Ok(()), "{backend:?}");
    }
}

#[test]
fn a_module_that_needs_an_optional_capability_is_written_only_where_it_is_allowed() {
    let results = check_module(&spirv_of(HALF));
    assert_eq!(results[&Backend::SpirvAny], Ok(()));
    assert!(
        results[&Backend::SpirvBase].is_err(),
        "16-bit floats are not among the capabilities every device has: {:?}",
        results[&Backend::SpirvBase]
    );
}

#[test]
fn a_module_naga_cannot_read_stops_every_writer_with_the_same_cause() {
    let results = check_module(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let causes: std::collections::BTreeSet<&String> =
        results.values().filter_map(|r| r.as_ref().err()).collect();
    assert_eq!(results.len(), Backend::ALL.len());
    assert_eq!(causes.len(), 1, "{causes:?}");
    assert!(causes
        .iter()
        .next()
        .is_some_and(|c| c.contains("reader refused")));
}

#[test]
fn a_tally_counts_each_program_by_stage_and_each_writers_refusals_by_cause_with_digits_folded() {
    let mut tally = BackendTally::default();
    let wrote = |backends: &[Backend]| -> BTreeMap<Backend, Result<(), String>> {
        Backend::ALL
            .iter()
            .map(|b| {
                (
                    *b,
                    if backends.contains(b) {
                        Ok(())
                    } else {
                        Err("it wanted 12 of something".to_string())
                    },
                )
            })
            .collect()
    };
    tally.add(ShaderStageKind::Vertex, &wrote(&Backend::ALL));
    tally.add(ShaderStageKind::Pixel, &wrote(&[Backend::Msl]));
    tally.add(
        ShaderStageKind::Pixel,
        &wrote(&[Backend::Msl, Backend::Hlsl51]),
    );
    assert_eq!(tally.total(), 3);
    assert_eq!(tally.written_by(Backend::Msl), 3);
    assert_eq!(tally.written_by(Backend::Hlsl51), 2);
    assert_eq!(tally.written_by(Backend::SpirvAny), 1);
    assert_eq!(
        tally.causes[&(Backend::SpirvAny, "it wanted 12 of something".to_string())],
        2
    );
    let report = tally.report();
    assert!(report.contains("3 translated programs"), "{report}");
    assert!(
        report.contains("Metal (MSL): 3 written (100.0%)"),
        "{report}"
    );
    assert!(
        report.contains("Direct3D 12 (HLSL 5.1): 2 written (66.7%)"),
        "{report}"
    );
    assert!(report.contains("Pixel"), "{report}");
    let json = tally.to_json();
    assert_eq!(json["modules"]["Pixel"], 2);
    assert_eq!(json["writers"].as_array().map(Vec::len), Some(4));
}

fn program(stage: ShaderStageKind, outcome: Outcome) -> Program {
    Program {
        stage,
        source_bytes: 1,
        outcome,
        reflection: Reflection::default(),
    }
}

#[test]
fn a_pack_is_read_from_its_manifest_and_only_the_translated_programs_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let mut pack = SpirvPack::new(Production {
        translator: "t".into(),
        legalizer: "l".into(),
    });
    let translated = Outcome::Translated {
        spirv_bytes: 1,
        fidelity: Fidelity::Equivalent,
        lowerings: BTreeMap::new(),
        naga: NagaReach {
            reached: Reach::MslWritten,
            cause: None,
        },
    };
    pack.programs.insert(
        "aa11".repeat(10),
        program(ShaderStageKind::Pixel, translated.clone()),
    );
    pack.programs.insert(
        "bb22".repeat(10),
        program(ShaderStageKind::Vertex, translated),
    );
    pack.programs.insert(
        "cc33".repeat(10),
        program(
            ShaderStageKind::Pixel,
            Outcome::Refused {
                reason: "no".into(),
            },
        ),
    );
    for (key, source) in [("aa11".repeat(10), PLAIN), ("bb22".repeat(10), HALF)] {
        let path = dir.path().join(module_path(&key));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, spirv_of(source)).unwrap();
    }
    std::fs::write(
        dir.path().join(MANIFEST_FILE),
        serde_json::to_vec(&pack).unwrap(),
    )
    .unwrap();
    let tally = check_pack(dir.path()).unwrap();
    assert_eq!(tally.total(), 2, "the refused program is not a module");
    assert_eq!(tally.modules["Pixel"], 1);
    assert_eq!(tally.modules["Vertex"], 1);
    assert_eq!(tally.written_by(Backend::SpirvAny), 2);
    assert_eq!(tally.written_by(Backend::SpirvBase), 1);
}

#[test]
fn a_missing_manifest_or_a_missing_module_is_said_and_not_counted_as_written() {
    let dir = tempfile::tempdir().unwrap();
    assert!(check_pack(dir.path()).is_err());
    let mut pack = SpirvPack::new(Production {
        translator: "t".into(),
        legalizer: "l".into(),
    });
    pack.programs.insert(
        "dd44".repeat(10),
        program(
            ShaderStageKind::Pixel,
            Outcome::Translated {
                spirv_bytes: 1,
                fidelity: Fidelity::Equivalent,
                lowerings: BTreeMap::new(),
                naga: NagaReach {
                    reached: Reach::MslWritten,
                    cause: None,
                },
            },
        ),
    );
    std::fs::write(
        dir.path().join(MANIFEST_FILE),
        serde_json::to_vec(&pack).unwrap(),
    )
    .unwrap();
    let tally = check_pack(dir.path()).unwrap();
    assert_eq!(tally.total(), 1);
    for backend in Backend::ALL {
        assert_eq!(tally.written_by(backend), 0, "{backend:?}");
    }
    assert!(tally
        .causes
        .keys()
        .any(|(_, c)| c.contains("cannot be read")));
}

/// A vertex stage that reads the first instance of the draw, as a Vulkan shader may and a Direct3D one has no register for.
fn first_instance_vertex() -> Vec<u8> {
    let mut module = naga::front::wgsl::parse_str(
        "@vertex fn main(@builtin(instance_index) first: u32) -> @builtin(position) vec4<f32> {
            return vec4<f32>(f32(first));
        }",
    )
    .expect("the WGSL parses");
    module.entry_points[0].function.arguments[0].binding =
        Some(naga::Binding::BuiltIn(naga::BuiltIn::BaseInstance));
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .expect("it validates");
    naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
        .expect("naga writes it")
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect()
}

#[test]
fn a_first_instance_is_written_for_direct3d_once_the_legalizer_has_zeroed_it_and_not_before() {
    let raw = first_instance_vertex();
    assert_eq!(
        check_module(&raw)[&Backend::Hlsl51],
        Err("builtin BaseInstance".to_string()),
        "the writer has no register for it"
    );
    let (legal, _) = crate::legalize::legalize(&raw).expect("the legalizer takes it");
    let results = check_module(&legal);
    for backend in Backend::ALL {
        assert_eq!(results[&backend], Ok(()), "{backend:?}");
    }
}

/// A stand-in for `spirv-val` that says what the module's length was and refuses a module of a given first word.
#[cfg(unix)]
fn fake_validator(dir: &Path, script: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join("spirv-val");
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn a_validators_refusal_is_the_cause_with_its_place_in_the_module_dropped_and_its_digits_folded() {
    let dir = tempfile::tempdir().unwrap();
    let tool = fake_validator(
        dir.path(),
        "cat >/dev/null; echo 'error: line 4182: ID 77 has not been defined' >&2; exit 1",
    );
    let results = check_module_with(&spirv_of(PLAIN), Some(&SpirvVal::new(tool)));
    assert_eq!(
        results[&Backend::SpirvChecked],
        Err("ID N has not been defined".to_string())
    );
    for backend in Backend::ALL {
        assert_eq!(results[&backend], Ok(()), "{backend:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_validator_is_given_the_vulkan_rules_and_the_module_bytes_the_writer_made() {
    let dir = tempfile::tempdir().unwrap();
    let seen = dir.path().join("seen");
    let tool = fake_validator(
        dir.path(),
        &format!(
            "echo \"$@\" > {seen}; cat > {seen}.bin",
            seen = seen.display()
        ),
    );
    let results = check_module_with(&spirv_of(PLAIN), Some(&SpirvVal::new(tool)));
    assert_eq!(results[&Backend::SpirvChecked], Ok(()));
    assert_eq!(
        std::fs::read_to_string(&seen).unwrap().trim(),
        "--target-env vulkan1.1 -"
    );
    let given = std::fs::read(format!("{}.bin", seen.display())).unwrap();
    assert_eq!(
        &given[..4],
        &0x0723_0203_u32.to_le_bytes(),
        "a SPIR-V module, by its magic"
    );
    assert_eq!(given.len() % 4, 0);
}

#[cfg(unix)]
#[test]
fn a_validator_judges_by_the_vulkan_version_it_is_told() {
    let dir = tempfile::tempdir().unwrap();
    let seen = dir.path().join("seen");
    let tool = fake_validator(
        dir.path(),
        &format!("echo \"$@\" > {}; cat >/dev/null", seen.display()),
    );
    let mut validator = SpirvVal::new(tool);
    assert_eq!(validator.target_env, "vulkan1.1");
    validator.target_env = "vulkan1.3".to_string();
    let results = check_module_with(&spirv_of(PLAIN), Some(&validator));
    assert_eq!(results[&Backend::SpirvChecked], Ok(()));
    assert_eq!(
        std::fs::read_to_string(&seen).unwrap().trim(),
        "--target-env vulkan1.3 -"
    );
}

#[test]
fn without_a_validator_the_tally_reports_naga_alone_and_with_one_the_validators_verdict_too() {
    let mut alone = BackendTally::default();
    alone.add(
        ShaderStageKind::Pixel,
        &check_module_with(&spirv_of(PLAIN), None),
    );
    assert!(!alone.report().contains("validator"), "{}", alone.report());
    assert_eq!(alone.to_json()["writers"].as_array().map(Vec::len), Some(4));

    let mut judged = BackendTally::default();
    judged.add(
        ShaderStageKind::Pixel,
        &BTreeMap::from([(Backend::SpirvChecked, Err("nope".to_string()))]),
    );
    assert!(judged.report().contains("validator"), "{}", judged.report());
    assert_eq!(
        judged.to_json()["writers"].as_array().map(Vec::len),
        Some(5)
    );
}

#[test]
fn a_validator_that_is_not_there_is_said_for_every_module_and_not_counted_as_accepting() {
    let results = check_module_with(&spirv_of(PLAIN), Some(&SpirvVal::new("/no/such/spirv-val")));
    assert!(results[&Backend::SpirvChecked]
        .as_ref()
        .err()
        .is_some_and(|c| c.contains("cannot start")));
}

#[test]
fn a_module_the_common_capabilities_cannot_carry_has_no_spirv_to_validate() {
    let results = check_module_with(&spirv_of(HALF), Some(&SpirvVal::new("/no/such/spirv-val")));
    assert_eq!(
        results[&Backend::SpirvChecked],
        Err("no SPIR-V was written to validate".to_string())
    );
}

/// The reference validator when the machine has one.
fn reference_validator() -> Option<std::path::PathBuf> {
    std::env::var_os("OPENRAGE_SPIRV_VAL")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|d| d.join("spirv-val"))
                .find(|p| p.is_file())
        })
}

#[test]
#[ignore = "needs SPIRV-Tools' spirv-val on PATH or in OPENRAGE_SPIRV_VAL: cargo test -- --ignored"]
fn the_reference_validator_accepts_what_naga_writes_for_a_plain_module_and_refuses_a_broken_one() {
    let tool = reference_validator().expect("spirv-val is on PATH or in OPENRAGE_SPIRV_VAL");
    let words: Vec<u32> = spirv_of(PLAIN)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let validator = SpirvVal::new(tool);
    assert_eq!(validate_vulkan(&validator, &words), Ok(()));
    // The id bound, the fifth word of the header, below an id the module uses
    let mut broken = words;
    broken[3] = 2;
    assert!(validate_vulkan(&validator, &broken).is_err());
}
