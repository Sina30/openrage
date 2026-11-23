use or_io::gta5::shaders::fxc::{GrcEffectPass, GrcEffectProgram, GrcEffectTechnique};

use super::*;
use crate::dxbc_testing::{
    container, discarding_pixel_shader, dwords, plain_vertex_shader, rdef_with_buffers,
    red_pixel_shader, textured_pixel_shader, vertex_shader_without_a_position,
};

/// A shader whose constant buffer `Globals` holds `gTint` and `gFade`, sampling `Diffuse` from `texture_register` through s2.
fn textured_in(texture_register: u32) -> Vec<u8> {
    let reflection = rdef_with_buffers(
        &[
            ("Globals", 0, 3),
            ("Diffuse", 2, texture_register),
            ("DiffuseSampler", 3, 2),
        ],
        &[("Globals", 32, &[("gTint", 0, 16), ("gFade", 16, 16)])],
    );
    textured_pixel_shader(3, texture_register, 2, Some(reflection))
}

fn textured() -> Vec<u8> {
    textured_in(5)
}

fn sha1_of(bytes: &[u8]) -> String {
    format!("{:x}", Sha1::digest(bytes))
}

/// An effect whose programs are `blobs` laid end to end, each in the stage given.
fn source(
    name: &str,
    blobs: &[(GrcProgramStage, Vec<u8>)],
    techniques: &[(&str, &[[u8; 6]])],
) -> SourceEffect {
    let mut bytes = Vec::new();
    let mut programs = Vec::new();
    for (stage, blob) in blobs {
        programs.push(GrcEffectProgram {
            stage: *stage,
            name: format!("program{}", programs.len()),
            variables: Vec::new(),
            cbuffers: Vec::new(),
            bytecode_offset: bytes.len(),
            bytecode_size: blob.len(),
            shader_model: None,
        });
        bytes.extend_from_slice(blob);
    }
    SourceEffect {
        name: name.to_string(),
        source_path: format!("{name}.fxc"),
        effect: GrcEffect {
            vertex_type: 7,
            programs,
            techniques: techniques
                .iter()
                .map(|(technique, passes)| GrcEffectTechnique {
                    name: technique.to_string(),
                    passes: passes
                        .iter()
                        .map(|programs| GrcEffectPass {
                            programs: *programs,
                            render_states: Vec::new(),
                        })
                        .collect(),
                })
                .collect(),
            ..GrcEffect::default()
        },
        bytes,
    }
}

/// A vertex and a pixel shader, drawn by one pass of `draw` and of `again`.
fn drawing_effect() -> SourceEffect {
    source(
        "gta_default",
        &[
            (GrcProgramStage::Vertex, plain_vertex_shader()),
            (GrcProgramStage::Pixel, red_pixel_shader()),
        ],
        &[
            ("draw", &[[1, 1, 0, 0, 0, 0]]),
            ("again", &[[1, 1, 0, 0, 0, 0]]),
        ],
    )
}

#[test]
fn a_pixel_shader_is_translated_described_and_kept_as_a_module() {
    let Baked {
        program,
        module,
        buffers,
    } = translate_one(&textured(), GrcProgramStage::Pixel);
    assert_eq!(program.stage, ShaderStageKind::Pixel);
    let Outcome::Translated {
        spirv_bytes,
        fidelity,
        lowerings,
        naga,
    } = &program.outcome
    else {
        panic!("{:?}", program.outcome);
    };
    assert_eq!(*fidelity, Fidelity::Equivalent);
    assert_eq!(lowerings.get("memory_model"), Some(&1));
    assert_eq!(naga.reached, Reach::MslWritten, "{:?}", naga.cause);
    let module = module.expect("a translated program has a module");
    assert_eq!(module.len(), *spirv_bytes as usize);
    assert_eq!(module[..4], 0x0723_0203_u32.to_le_bytes());

    let described: Vec<_> = program
        .reflection
        .bindings
        .iter()
        .map(|b| (b.name.as_str(), b.class, b.register, b.set, b.binding))
        .collect();
    assert_eq!(
        described,
        vec![
            ("Globals", BindingClass::Cbv, 3, 1, 3),
            ("Diffuse", BindingClass::Srv, 5, 1, 37),
            ("DiffuseSampler", BindingClass::Sampler, 2, 1, 18),
        ]
    );
    assert_eq!(program.reflection.inputs[0].semantic, "TEXCOORD");
    assert_eq!(program.reflection.outputs[0].semantic, "SV_Target");

    assert_eq!(buffers.len(), 1);
    let (key, layout) = &buffers[0];
    assert_eq!(program.reflection.constant_buffers, vec![key.clone()]);
    assert_eq!((layout.name.as_str(), layout.size), ("Globals", 32));
    let members: Vec<_> = layout
        .variables
        .iter()
        .map(|v| {
            (
                v.name.as_str(),
                v.offset,
                v.size,
                v.used,
                v.scalar.as_str(),
                v.rows,
                v.columns,
            )
        })
        .collect();
    assert_eq!(
        members,
        vec![
            ("gTint", 0, 16, true, "float", 1, 4),
            ("gFade", 16, 16, true, "float", 1, 4),
        ]
    );
}

#[test]
fn a_vertex_programs_bindings_are_in_the_set_before_the_pixel_stages() {
    let reflection = rdef_with_buffers(&[("Globals", 0, 3)], &[]);
    let blob = crate::dxbc_testing::vertex_shader_with_a_buffer(3, Some(reflection));
    let baked = translate_one(&blob, GrcProgramStage::Vertex);
    let described: Vec<_> = baked
        .program
        .reflection
        .bindings
        .iter()
        .map(|b| (b.name.as_str(), b.set, b.binding))
        .collect();
    assert_eq!(described, vec![("Globals", 0, 3)]);
}

#[test]
fn a_program_that_is_not_translated_says_why_and_has_no_module() {
    let dxil = container(&[(
        *b"DXIL",
        dwords(&[0x0001_0066, 6, 0x4C49_5844, 0x102, 16, 0]),
    )]);
    let none = container(&[(*b"RDEF", vec![0; 8])]);
    let skipped = |blob: &[u8]| match translate_one(blob, GrcProgramStage::Pixel) {
        Baked {
            program:
                Program {
                    outcome: Outcome::Skipped { reason },
                    ..
                },
            module: None,
            ..
        } => reason,
        other => panic!("{other:?}"),
    };
    assert_eq!(skipped(&dxil), "DXIL");
    assert_eq!(skipped(&none), "no program part");

    // A vertex shader in a pixel slot is refused before it is translated
    let baked = translate_one(&plain_vertex_shader(), GrcProgramStage::Pixel);
    let Outcome::Refused { reason } = baked.program.outcome else {
        panic!("a vertex shader in a pixel slot was translated");
    };
    assert!(reason.contains("stage differs"), "{reason}");
    assert!(baked.module.is_none() && baked.buffers.is_empty());
}

#[test]
fn a_shader_that_discards_is_labelled_approximated_for_the_kill_that_stands_in_for_the_demote() {
    let baked = translate_one(&discarding_pixel_shader(), GrcProgramStage::Pixel);
    let Outcome::Translated {
        fidelity,
        lowerings,
        ..
    } = baked.program.outcome
    else {
        panic!("{:?}", baked.program.outcome);
    };
    assert_eq!(lowerings.get("demotes_to_kill"), Some(&1), "{lowerings:?}");
    assert_eq!(fidelity, Fidelity::Approximated);
}

#[test]
fn how_far_naga_got_is_recorded_with_what_stopped_it() {
    let baked = translate_one(&vertex_shader_without_a_position(), GrcProgramStage::Vertex);
    let Outcome::Translated { naga, .. } = baked.program.outcome else {
        panic!("{:?}", baked.program.outcome);
    };
    assert_eq!(naga.reached, Reach::Parsed);
    assert!(naga.cause.unwrap_or_default().contains("position"));
    assert!(
        baked.module.is_some(),
        "the module is kept for the Vulkan path"
    );
}

#[test]
fn every_stage_a_pass_can_bind_has_its_own_kind() {
    let kinds: Vec<_> = GrcProgramStage::ALL
        .iter()
        .map(|s| stage_kind(*s))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ShaderStageKind::Vertex,
            ShaderStageKind::Pixel,
            ShaderStageKind::Compute,
            ShaderStageKind::Domain,
            ShaderStageKind::Geometry,
            ShaderStageKind::Hull,
        ]
    );
}

#[test]
fn programs_that_are_refused_or_set_aside_are_counted_kept_in_the_manifest_and_reused() {
    let tmp = tempfile::tempdir().unwrap();
    let dxil = container(&[(
        *b"DXIL",
        dwords(&[0x0001_0066, 6, 0x4C49_5844, 0x102, 16, 0]),
    )]);
    let effect = || {
        source(
            "odd",
            &[
                (GrcProgramStage::Pixel, dxil.clone()),
                // a vertex shader in a pixel slot
                (GrcProgramStage::Pixel, plain_vertex_shader()),
                (GrcProgramStage::Pixel, red_pixel_shader()),
            ],
            &[(
                "draw",
                &[[0, 1, 0, 0, 0, 0], [0, 2, 0, 0, 0, 0], [0, 3, 0, 0, 0, 0]],
            )],
        )
    };
    let production = production();
    let (pack, report) = bake_effects(&[effect()], tmp.path(), &production, "").unwrap();
    assert_eq!(
        (
            report.programs,
            report.translated,
            report.refused,
            report.skipped
        ),
        (3, 1, 1, 1)
    );
    assert_eq!(pack.programs.len(), 3);
    let (again, report) = bake_effects(&[effect()], tmp.path(), &production, "").unwrap();
    assert_eq!((report.reused, report.baked), (3, 0));
    assert_eq!(again, pack);
}

#[test]
fn a_bake_leaves_no_temporary_file_behind() {
    let tmp = tempfile::tempdir().unwrap();
    bake_effects(&[drawing_effect()], tmp.path(), &production(), "").unwrap();
    let mut stack = vec![tmp.path().to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let extension = path.extension().unwrap().to_string_lossy().into_owned();
                assert!(["spv", "json"].contains(&extension.as_str()), "{path:?}");
            }
        }
    }
}

#[test]
fn a_demote_or_a_dropped_clip_distance_makes_a_module_approximated_and_nothing_else_does() {
    use crate::legalize::Lowerings;
    assert!(!Lowerings::default().approximates());
    let only = |set: fn(&mut Lowerings)| {
        let mut lowerings = Lowerings::default();
        set(&mut lowerings);
        lowerings
    };
    assert!(only(|l| l.demotes_to_kill = 1).approximates());
    assert!(only(|l| l.clip_distances_dropped = 1).approximates());
    assert!(!only(|l| l.memory_model = true).approximates());
    assert!(!only(|l| l.interface_groups_merged = 3).approximates());
    assert!(!only(|l| l.interface_locations_widened = 4).approximates());
    assert_eq!(
        only(|l| l.interface_locations_widened = 4).counts()["interface_locations_widened"],
        4
    );
    let counts = only(|l| {
        l.demotes_to_kill = 2;
        l.memory_model = true;
    })
    .counts();
    assert_eq!(counts["demotes_to_kill"], 2);
    assert_eq!(counts["memory_model"], 1);
    assert!(!counts.contains_key("local_size_ids"));
}

#[test]
fn a_bake_stores_each_distinct_program_once_and_says_which_pass_runs_which() {
    let tmp = tempfile::tempdir().unwrap();
    let (pack, report) =
        bake_effects(&[drawing_effect()], tmp.path(), &production(), "build-1").unwrap();
    assert_eq!(pack.programs.len(), 2);
    assert_eq!(
        (report.programs, report.translated, report.baked),
        (2, 2, 2)
    );
    assert_eq!(pack.source_build_hash, "build-1");

    let effect = &pack.effects[0];
    assert_eq!(
        (effect.name.as_str(), effect.vertex_type),
        ("gta_default", 7)
    );
    assert_eq!(effect.techniques.len(), 2);
    for technique in &effect.techniques {
        let stages = &technique.passes[0].stages;
        assert_eq!(
            stages[&ShaderStageKind::Vertex],
            sha1_of(&plain_vertex_shader())
        );
        assert_eq!(
            stages[&ShaderStageKind::Pixel],
            sha1_of(&red_pixel_shader())
        );
    }
    for key in pack.programs.keys() {
        assert!(tmp.path().join(module_path(key)).is_file(), "{key}");
    }
    let on_disk: SpirvPack =
        serde_json::from_slice(&std::fs::read(tmp.path().join(MANIFEST_FILE)).unwrap()).unwrap();
    assert_eq!(on_disk, pack);
}

#[test]
fn a_constant_buffer_two_programs_name_is_one_layout_in_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let effect = source(
        "pair",
        &[
            (GrcProgramStage::Pixel, textured_in(5)),
            (GrcProgramStage::Pixel, textured_in(6)),
        ],
        &[("a", &[[0, 1, 0, 0, 0, 0]]), ("b", &[[0, 2, 0, 0, 0, 0]])],
    );
    let (pack, _) = bake_effects(&[effect], tmp.path(), &production(), "").unwrap();
    assert_eq!(pack.programs.len(), 2);
    assert_eq!(pack.constant_buffers.len(), 1);
    for program in pack.programs.values() {
        let layouts = pack.constant_buffers_of(program);
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].name, "Globals");
    }
}

#[test]
fn a_second_bake_reuses_what_the_first_wrote_and_a_missing_module_is_made_again() {
    let tmp = tempfile::tempdir().unwrap();
    let production = production();
    let (first, _) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();

    let (second, report) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    assert_eq!((report.reused, report.baked), (2, 0));
    assert_eq!(second, first);

    let key = sha1_of(&red_pixel_shader());
    std::fs::remove_file(tmp.path().join(module_path(&key))).unwrap();
    let (_, report) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    assert_eq!((report.reused, report.baked), (1, 1));
    assert!(tmp.path().join(module_path(&key)).is_file());
}

#[test]
fn a_reused_program_keeps_the_constant_buffer_layouts_it_names() {
    let tmp = tempfile::tempdir().unwrap();
    let production = production();
    let effect = || {
        source(
            "reuse",
            &[(GrcProgramStage::Pixel, textured())],
            &[("draw", &[[0, 1, 0, 0, 0, 0]])],
        )
    };
    let (first, _) = bake_effects(&[effect()], tmp.path(), &production, "").unwrap();
    assert_eq!(first.constant_buffers.len(), 1);
    let (second, report) = bake_effects(&[effect()], tmp.path(), &production, "").unwrap();
    assert_eq!((report.reused, report.baked), (1, 0));
    assert_eq!(second, first);
}

#[test]
fn a_manifest_of_another_schema_is_not_reused_even_when_the_same_tools_wrote_it() {
    let tmp = tempfile::tempdir().unwrap();
    let production = production();
    bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    let path = tmp.path().join(MANIFEST_FILE);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["schema_version"] = (PACK_VERSION + 1).into();
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let (_, report) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    assert_eq!((report.reused, report.baked), (0, 2));
}

#[test]
fn a_module_cut_short_on_disk_is_not_reused() {
    let tmp = tempfile::tempdir().unwrap();
    let production = production();
    bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    let key = sha1_of(&plain_vertex_shader());
    let path = tmp.path().join(module_path(&key));
    let whole = std::fs::read(&path).unwrap();
    std::fs::write(&path, &whole[..whole.len() - 4]).unwrap();
    let (_, report) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    assert_eq!((report.reused, report.baked), (1, 1));
    assert_eq!(std::fs::read(&path).unwrap(), whole);
}

#[test]
fn a_pack_made_by_another_translator_or_legalizer_is_not_reused() {
    let tmp = tempfile::tempdir().unwrap();
    let mut production = production();
    bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    production.legalizer = "another".into();
    let (pack, report) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    assert_eq!((report.reused, report.baked), (0, 2));
    assert_eq!(pack.produced_by.legalizer, "another");
}

#[test]
fn a_program_no_pass_runs_is_left_out_and_one_shared_by_two_effects_is_one_program() {
    let tmp = tempfile::tempdir().unwrap();
    let unused = source(
        "unused",
        &[
            (GrcProgramStage::Pixel, red_pixel_shader()),
            (GrcProgramStage::Pixel, textured()),
        ],
        &[("draw", &[[0, 1, 0, 0, 0, 0]])],
    );
    let again = source(
        "again",
        &[(GrcProgramStage::Pixel, red_pixel_shader())],
        &[("draw", &[[0, 1, 0, 0, 0, 0]])],
    );
    let (pack, report) = bake_effects(&[unused, again], tmp.path(), &production(), "").unwrap();
    assert_eq!(pack.programs.len(), 1, "{:?}", pack.programs.keys());
    assert_eq!(report.programs, 1);
    assert_eq!(pack.effects.len(), 2);
}

#[test]
fn a_pass_that_names_a_program_the_effect_lacks_runs_nothing_at_that_stage() {
    let tmp = tempfile::tempdir().unwrap();
    let effect = source(
        "short",
        &[(GrcProgramStage::Pixel, red_pixel_shader())],
        &[("draw", &[[3, 1, 0, 0, 0, 0]])],
    );
    let (pack, _) = bake_effects(&[effect], tmp.path(), &production(), "").unwrap();
    let stages = &pack.effects[0].techniques[0].passes[0].stages;
    assert_eq!(
        stages.keys().collect::<Vec<_>>(),
        vec![&ShaderStageKind::Pixel]
    );
}

#[test]
fn a_folder_that_cannot_be_written_is_an_error_and_not_a_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join("blocker");
    std::fs::write(&blocker, b"a file where a folder was wanted").unwrap();
    let result = bake_effects(
        &[drawing_effect()],
        &blocker.join("pack"),
        &production(),
        "",
    );
    assert!(matches!(result, Err(PackError::Write { .. })), "{result:?}");
}

#[test]
fn a_folder_of_loose_effects_is_read_and_one_that_is_not_an_effect_is_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let mut empty = b"rgxe".to_vec();
    empty.extend_from_slice(&0x59u32.to_le_bytes());
    empty.push(0);
    for _ in 0..6 {
        empty.extend([1u8, 5]);
        empty.extend_from_slice(b"NULL\0");
        empty.extend_from_slice(&[0; 6]);
    }
    empty.extend_from_slice(&[0, 0, 0, 0, 0]);
    let fxc = tmp.path().join("fxc");
    std::fs::create_dir(&fxc).unwrap();
    std::fs::write(fxc.join("empty.fxc"), empty).unwrap();
    std::fs::write(fxc.join("bad.fxc"), b"not an effect").unwrap();
    let out = tmp.path().join("pack");
    let (pack, report) = bake(&fxc, "shaders/win32_40_final", &out, "build").unwrap();
    assert_eq!((report.effects, report.unreadable), (1, 1));
    assert_eq!(pack.effects[0].name, "empty");
    assert_eq!(pack.effects[0].source_path, "empty.fxc");
    assert!(out.join(MANIFEST_FILE).is_file());
}

#[test]
fn what_writes_the_modules_is_named_the_same_each_time() {
    let one = production();
    assert_eq!(one, production());
    assert!(one.translator.starts_with("dxbc-spirv "));
    assert_eq!(one.legalizer.len(), 16);
}

#[test]
fn progress_counts_the_programs_translated_from_none_to_all_and_a_reused_pack_has_none_to_count() {
    let tmp = tempfile::tempdir().unwrap();
    let production = production();
    let calls: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
    let note = |done, total| calls.lock().unwrap().push((done, total));
    let (_, report) =
        bake_effects_with(&[drawing_effect()], tmp.path(), &production, "", &note).unwrap();
    let mut seen = calls.lock().unwrap().clone();
    assert!(report.baked > 0);
    assert_eq!(
        seen.len(),
        report.baked + 1,
        "once for the start and once for each program"
    );
    assert_eq!(seen[0], (0, report.baked));
    seen.sort();
    assert_eq!(
        seen,
        (0..=report.baked)
            .map(|done| (done, report.baked))
            .collect::<Vec<_>>(),
        "each count from none to all is told once"
    );

    calls.lock().unwrap().clear();
    let (_, again) =
        bake_effects_with(&[drawing_effect()], tmp.path(), &production, "", &note).unwrap();
    assert_eq!(again.baked, 0);
    assert_eq!(*calls.lock().unwrap(), [(0, 0)]);
}

fn variable(
    name: &str,
    var_type: or_io::gta5::shaders::fxc::GrcVariableType,
    floats: &[f32],
    uints: &[u32],
) -> or_io::gta5::shaders::fxc::GrcEffectVariable {
    or_io::gta5::shaders::fxc::GrcEffectVariable {
        var_type,
        count: 0,
        slot: 0,
        group: 0,
        name: name.into(),
        semantic: format!("{name}Semantic"),
        offset: 0,
        variant: 0,
        cbuffer: 0,
        annotations: Vec::new(),
        default_floats: floats.to_vec(),
        default_uints: uints.to_vec(),
    }
}

#[test]
fn what_an_effect_declares_for_its_constants_is_kept_as_words_by_the_name_the_shader_reads() {
    use or_io::gta5::shaders::fxc::GrcVariableType as T;
    let effect = GrcEffect {
        variables: vec![
            variable("matTint", T::Float4, &[0.5, 1.0, 0.0, 2.0], &[]),
            variable("gNumLights", T::Int, &[], &[8]),
            variable("noDefault", T::Float4, &[], &[]),
            // A sampler's states are a run of integers, and the sampler is not a constant
            variable("DiffuseSampler", T::Texture, &[], &[0, 3, 1, 3]),
            // The first of two declarations of a name is the one kept
            variable("matTint", T::Float4, &[9.0], &[]),
        ],
        ..GrcEffect::default()
    };
    let defaults = defaults_of(&effect);
    assert_eq!(
        defaults.keys().map(String::as_str).collect::<Vec<_>>(),
        ["gNumLights", "matTint"]
    );
    assert_eq!(
        defaults["matTint"],
        [0.5f32, 1.0, 0.0, 2.0].map(f32::to_bits)
    );
    assert_eq!(defaults["gNumLights"], [8]);
}

#[test]
fn a_baked_effect_carries_its_defaults_and_a_pack_without_any_still_reads() {
    use or_io::gta5::shaders::fxc::GrcVariableType as T;
    let tmp = tempfile::tempdir().unwrap();
    let mut source = drawing_effect();
    source.effect.variables = vec![variable("matTint", T::Float4, &[0.25], &[])];
    let (pack, _) = bake_effects(&[source], tmp.path(), &production(), "").unwrap();
    assert_eq!(pack.effects[0].defaults["matTint"], [0.25f32.to_bits()]);
    let on_disk: SpirvPack =
        serde_json::from_slice(&std::fs::read(tmp.path().join(MANIFEST_FILE)).unwrap()).unwrap();
    assert_eq!(on_disk, pack);
    let (bare, _) = bake_effects(&[drawing_effect()], tmp.path(), &production(), "").unwrap();
    assert!(bare.effects[0].defaults.is_empty());
    let text = serde_json::to_string(&bare).unwrap();
    assert!(
        !text.contains("defaults"),
        "an effect with none says none: {text}"
    );
}

#[test]
fn a_bake_removes_the_modules_no_program_names_and_what_a_killed_run_left_and_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let production = production();
    let (pack, report) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    assert_eq!(report.swept, 0, "a first bake has nothing to remove");
    let kept: Vec<String> = pack
        .programs
        .iter()
        .filter(|(_, p)| matches!(p.outcome, Outcome::Translated { .. }))
        .map(|(key, _)| key.clone())
        .collect();
    assert!(!kept.is_empty());
    let shard = |key: &str| tmp.path().join("spirv").join(&key[..2]);
    // A module of a program the pack no longer has, a temporary file of a killed run, and two things that are not a module's
    let stray = tmp
        .path()
        .join(module_path(&format!("ff{}", "0".repeat(38))));
    std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
    std::fs::write(&stray, b"old").unwrap();
    let temporary = shard(&kept[0]).join("abc.tmp99999");
    std::fs::write(&temporary, b"half").unwrap();
    let note = shard(&kept[0]).join("notes.txt");
    std::fs::write(&note, b"mine").unwrap();
    let outside = tmp.path().join("other.spv");
    std::fs::write(&outside, b"not in spirv").unwrap();

    let (_, again) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    assert_eq!(
        (again.reused, again.baked, again.swept),
        (kept.len() + (pack.programs.len() - kept.len()), 0, 2)
    );
    assert!(!stray.exists() && !temporary.exists());
    assert!(
        !stray.parent().unwrap().exists(),
        "its folder was left empty and goes"
    );
    assert!(
        note.exists() && outside.exists(),
        "only a module's or a temporary file is touched"
    );
    for key in &kept {
        assert!(
            tmp.path().join(module_path(key)).is_file(),
            "{key} was kept"
        );
    }
    assert!(tmp.path().join(MANIFEST_FILE).is_file());
    // Nothing is left to remove the next time
    let (_, last) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    assert_eq!(last.swept, 0);
}

#[test]
fn a_module_of_an_effect_taken_out_of_the_sources_is_removed_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let production = production();
    // An effect whose one program no other has, as a textured pixel shader is
    let textured_effect = || {
        source(
            "textured_only",
            &[(GrcProgramStage::Pixel, textured())],
            &[("draw", &[[0, 1, 0, 0, 0, 0]])],
        )
    };
    let (full, _) = bake_effects(
        &[drawing_effect(), textured_effect()],
        tmp.path(),
        &production,
        "",
    )
    .unwrap();
    let (fewer, report) = bake_effects(&[drawing_effect()], tmp.path(), &production, "").unwrap();
    let gone: Vec<&String> = full
        .programs
        .iter()
        .filter(|(key, p)| {
            matches!(p.outcome, Outcome::Translated { .. }) && !fewer.programs.contains_key(*key)
        })
        .map(|(key, _)| key)
        .collect();
    assert!(
        !gone.is_empty(),
        "the effect taken out had a program of its own"
    );
    assert_eq!(report.swept, gone.len());
    for key in gone {
        assert!(!tmp.path().join(module_path(key)).exists());
    }
}

#[test]
fn a_module_left_for_a_program_that_is_now_refused_or_set_aside_is_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let dxil = container(&[(
        *b"DXIL",
        dwords(&[0x0001_0066, 6, 0x4C49_5844, 0x102, 16, 0]),
    )]);
    let effect = || {
        source(
            "odd",
            &[
                (GrcProgramStage::Pixel, dxil.clone()),
                (GrcProgramStage::Pixel, plain_vertex_shader()),
                (GrcProgramStage::Pixel, red_pixel_shader()),
            ],
            &[(
                "draw",
                &[[0, 1, 0, 0, 0, 0], [0, 2, 0, 0, 0, 0], [0, 3, 0, 0, 0, 0]],
            )],
        )
    };
    let production = production();
    let (pack, _) = bake_effects(&[effect()], tmp.path(), &production, "").unwrap();
    let not_translated: Vec<&String> = pack
        .programs
        .iter()
        .filter(|(_, p)| !matches!(p.outcome, Outcome::Translated { .. }))
        .map(|(key, _)| key)
        .collect();
    assert_eq!(not_translated.len(), 2);
    for key in &not_translated {
        let path = tmp.path().join(module_path(key));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"from a translator that took it").unwrap();
    }
    let (_, report) = bake_effects(&[effect()], tmp.path(), &production, "").unwrap();
    assert_eq!(report.swept, 2);
    for key in not_translated {
        assert!(!tmp.path().join(module_path(key)).exists());
    }
}
