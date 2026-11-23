use super::*;

fn program(stage: ShaderStageKind) -> Program {
    Program {
        stage,
        source_bytes: 612,
        outcome: Outcome::Translated {
            spirv_bytes: 1_204,
            fidelity: Fidelity::Approximated,
            lowerings: BTreeMap::from([("clip_distances_dropped".to_string(), 1)]),
            naga: NagaReach {
                reached: Reach::Validated,
                cause: Some("the MSL writer panicked".into()),
            },
        },
        reflection: Reflection {
            bindings: vec![Binding {
                name: "Globals".into(),
                kind: "cbuffer".into(),
                class: BindingClass::Cbv,
                register: 3,
                set: 0,
                binding: 3,
            }],
            constant_buffers: vec!["cafe".into()],
            inputs: vec![SignatureElement {
                semantic: "POSITION".into(),
                index: 0,
                system_value: 0,
                component_type: 3,
                register: 0,
                mask: 7,
            }],
            outputs: Vec::new(),
        },
    }
}

fn pack() -> SpirvPack {
    let mut pack = SpirvPack::new(Production {
        translator: "front".into(),
        legalizer: "legal".into(),
    });
    pack.source_build_hash = "build".into();
    pack.constant_buffers.insert(
        "cafe".into(),
        ConstantBuffer {
            name: "Globals".into(),
            size: 64,
            variables: vec![Variable {
                name: "gWorld".into(),
                offset: 0,
                size: 64,
                used: true,
                scalar: "float".into(),
                rows: 4,
                columns: 4,
                elements: 0,
            }],
        },
    );
    pack.programs
        .insert("ab12cd".into(), program(ShaderStageKind::Vertex));
    pack.programs.insert(
        "ef34".into(),
        Program {
            stage: ShaderStageKind::Pixel,
            source_bytes: 90,
            outcome: Outcome::Refused {
                reason: "no position".into(),
            },
            reflection: Reflection::default(),
        },
    );
    pack.effects.push(Effect {
        name: "gta_default".into(),
        source_path: "win32_40_final/gta_default.fxc".into(),
        vertex_type: 4,
        defaults: Default::default(),
        techniques: vec![Technique {
            name: "draw".into(),
            passes: vec![Pass {
                stages: BTreeMap::from([
                    (ShaderStageKind::Vertex, "ab12cd".to_string()),
                    (ShaderStageKind::Pixel, "ef34".to_string()),
                ]),
            }],
        }],
    });
    pack
}

#[test]
fn a_pack_round_trips_through_json() {
    let pack = pack();
    let text = serde_json::to_string(&pack).unwrap();
    assert_eq!(serde_json::from_str::<SpirvPack>(&text).unwrap(), pack);
}

#[test]
fn an_outcome_names_itself_in_the_json_and_a_stage_is_a_lowercase_key() {
    let value = serde_json::to_value(pack()).unwrap();
    assert_eq!(value["schema_version"], PACK_VERSION);
    assert_eq!(
        value["programs"]["ab12cd"]["outcome"]["outcome"],
        "translated"
    );
    assert_eq!(value["programs"]["ef34"]["outcome"]["outcome"], "refused");
    assert_eq!(
        value["programs"]["ab12cd"]["outcome"]["fidelity"],
        "approximated"
    );
    assert_eq!(
        value["effects"][0]["techniques"][0]["passes"][0]["stages"]["pixel"],
        "ef34"
    );
}

#[test]
fn a_pass_finds_the_program_it_runs_at_each_stage() {
    let pack = pack();
    let pass = &pack.effects[0].techniques[0].passes[0];
    assert_eq!(
        pack.program_of(pass, ShaderStageKind::Vertex)
            .unwrap()
            .source_bytes,
        612
    );
    assert!(pack.program_of(pass, ShaderStageKind::Geometry).is_none());
    assert_eq!(pack.translated(), 1);
    let vertex = pack.program_of(pass, ShaderStageKind::Vertex).unwrap();
    let buffers = pack.constant_buffers_of(vertex);
    assert_eq!(buffers.len(), 1);
    assert_eq!(buffers[0].variables[0].name, "gWorld");
    assert!(pack
        .constant_buffers_of(pack.program_of(pass, ShaderStageKind::Pixel).unwrap())
        .is_empty());
}

#[test]
fn a_module_lives_under_the_first_two_characters_of_its_key() {
    assert_eq!(module_path("ab12cd"), "spirv/ab/ab12cd.spv");
    assert_eq!(module_path("x"), "spirv/00/x.spv");
}
