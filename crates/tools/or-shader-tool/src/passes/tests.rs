use or_io::gta5::shaders::fxc::{GrcEffectPass, GrcEffectProgram, GrcEffectTechnique};

use super::*;

fn program(stage: GrcProgramStage, name: &str) -> GrcEffectProgram {
    GrcEffectProgram {
        stage,
        name: name.to_string(),
        variables: Vec::new(),
        cbuffers: Vec::new(),
        bytecode_offset: 0,
        bytecode_size: 0,
        shader_model: None,
    }
}

/// A pass binding the one-based positions `[vertex, pixel, compute, domain, geometry, hull]`.
fn pass(programs: [u8; 6]) -> GrcEffectPass {
    GrcEffectPass {
        programs,
        render_states: Vec::new(),
    }
}

fn technique(name: &str, passes: Vec<GrcEffectPass>) -> GrcEffectTechnique {
    GrcEffectTechnique {
        name: name.to_string(),
        passes,
    }
}

/// Programs in effect order: two vertex shaders, one pixel shader, one geometry shader.
fn effect() -> GrcEffect {
    GrcEffect {
        programs: vec![
            program(GrcProgramStage::Vertex, "vs_a"),
            program(GrcProgramStage::Pixel, "ps_a"),
            program(GrcProgramStage::Vertex, "vs_b"),
            program(GrcProgramStage::Geometry, "gs_a"),
        ],
        techniques: vec![
            technique("draw", vec![pass([1, 1, 0, 0, 0, 0])]),
            technique("shadow", vec![pass([2, 0, 0, 0, 1, 0])]),
            technique("depth", vec![pass([2, 0, 0, 0, 0, 0])]),
        ],
        ..GrcEffect::default()
    }
}

fn judge(failing: &'static [usize]) -> impl Fn(usize) -> Option<Outcome> {
    move |index| {
        Some(if failing.contains(&index) {
            Outcome::Failed(format!("program {index} failed"))
        } else {
            Outcome::Built
        })
    }
}

#[test]
fn a_pass_builds_only_when_every_stage_it_binds_does() {
    let mut tally = PassTally::default();
    tally.add_effect(&effect(), &judge(&[3]));
    assert_eq!(
        tally.classes["VS+PS"],
        ClassTally {
            passes: 1,
            built: 1
        }
    );
    assert_eq!(
        tally.classes["VS+GS"],
        ClassTally {
            passes: 1,
            built: 0
        }
    );
    assert_eq!(
        tally.classes["VS"],
        ClassTally {
            passes: 1,
            built: 1
        }
    );
    assert_eq!(
        tally.techniques["draw"],
        TechniqueTally {
            effects: 1,
            built: 1
        }
    );
    assert_eq!(
        tally.techniques["shadow"],
        TechniqueTally {
            effects: 1,
            built: 0
        }
    );
}

#[test]
fn a_pass_names_a_program_by_its_one_based_position_among_its_stages_programs() {
    // `depth` binds the second vertex program, `vs_b`, which is the third program of the effect.
    let mut tally = PassTally::default();
    tally.add_effect(&effect(), &judge(&[2]));
    assert_eq!(
        tally.techniques["depth"].built, 0,
        "vs_b failed, so the pass failed"
    );
    assert_eq!(tally.techniques["draw"].built, 1, "vs_a did not");
    assert_eq!(
        tally.causes["program 2 failed"], 2,
        "counted once for each pass that uses it"
    );
}

#[test]
fn programs_are_counted_whether_or_not_a_pass_uses_them() {
    let mut tally = PassTally::default();
    tally.add_effect(&effect(), &judge(&[0, 1]));
    assert_eq!((tally.programs, tally.programs_built), (4, 2));
    assert_eq!(tally.effects, 1);
}

#[test]
fn a_pass_naming_a_program_the_effect_lacks_does_not_build_and_says_so() {
    let mut bad = effect();
    bad.techniques = vec![technique("draw", vec![pass([1, 5, 0, 0, 0, 0])])];
    let mut tally = PassTally::default();
    tally.add_effect(&bad, &judge(&[]));
    assert_eq!(tally.techniques["draw"].built, 0);
    assert_eq!(tally.causes["a pass names a program the effect lacks"], 1);
}

#[test]
fn a_technique_counts_an_effect_built_only_when_all_of_its_passes_are() {
    let mut two = effect();
    two.techniques = vec![technique(
        "draw",
        vec![pass([1, 1, 0, 0, 0, 0]), pass([2, 1, 0, 0, 0, 0])],
    )];
    let mut tally = PassTally::default();
    tally.add_effect(&two, &judge(&[2]));
    assert_eq!(
        tally.classes["VS+PS"],
        ClassTally {
            passes: 2,
            built: 1
        }
    );
    assert_eq!(
        tally.techniques["draw"],
        TechniqueTally {
            effects: 1,
            built: 0
        }
    );
}

#[test]
fn a_pass_that_binds_nothing_is_its_own_class() {
    let mut empty = effect();
    empty.techniques = vec![technique("clear", vec![pass([0; 6])])];
    let mut tally = PassTally::default();
    tally.add_effect(&empty, &judge(&[]));
    assert_eq!(
        tally.classes["(binds nothing)"],
        ClassTally {
            passes: 1,
            built: 1
        }
    );
}

#[test]
fn the_report_and_the_json_carry_the_same_counts() {
    let mut tally = PassTally::default();
    tally.add_effect(&effect(), &judge(&[3]));
    let report = tally.report();
    assert!(
        report.starts_with("1 effects (0 unreadable), 4 programs, 3 built"),
        "{report}"
    );
    assert!(report.contains("VS+PS"), "{report}");
    let json = tally.to_json();
    assert_eq!(json["programs_built"], 3);
    assert_eq!(json["classes"].as_array().unwrap().len(), 3);
}

#[test]
fn a_position_names_the_nth_program_of_its_stage_and_zero_names_none() {
    let effect = effect();
    // The vertex programs are at 0 and 2, the pixel program at 1
    assert_eq!(program_index(&effect, GrcProgramStage::Vertex, 1), Some(0));
    assert_eq!(program_index(&effect, GrcProgramStage::Vertex, 2), Some(2));
    assert_eq!(program_index(&effect, GrcProgramStage::Vertex, 3), None);
    assert_eq!(program_index(&effect, GrcProgramStage::Vertex, 0), None);
    assert_eq!(program_index(&effect, GrcProgramStage::Pixel, 1), Some(1));
    assert_eq!(program_index(&effect, GrcProgramStage::Hull, 1), None);
}

#[test]
fn a_pipeline_tally_counts_the_built_the_failed_and_why() {
    let mut tally = PipelineTally::default();
    tally.add(&Ok(()));
    tally.add(&Err("stages do not meet".into()));
    tally.add(&Err("stages do not meet".into()));
    tally.add(&Err("no layout".into()));
    assert_eq!((tally.passes, tally.built), (4, 1));
    assert_eq!(tally.causes["stages do not meet"], 2);
    assert_eq!(tally.causes["no layout"], 1);
}

#[test]
fn the_report_and_the_json_carry_the_pipelines_only_when_a_device_built_some() {
    let mut tally = PassTally::default();
    tally.add_effect(&effect(), &judge(&[]));
    assert!(!tally.report().contains("pipelines"));
    assert!(tally.to_json()["pipelines"].is_null());
    tally.pipelines = Some(PipelineTally {
        passes: 5,
        built: 2,
        distinct: 3,
        causes: BTreeMap::from([("stages do not meet".to_string(), 3)]),
    });
    let report = tally.report();
    assert!(
        report.contains("2 of 5 passes, from 3 distinct pairs"),
        "{report}"
    );
    assert!(report.contains("stages do not meet"));
    let json = tally.to_json();
    assert_eq!(json["pipelines"]["built"], 2);
    assert_eq!(json["pipelines"]["distinct"], 3);
    assert_eq!(json["pipelines"]["causes"]["stages do not meet"], 3);
}
