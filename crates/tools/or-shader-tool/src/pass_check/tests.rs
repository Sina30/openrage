use super::*;
use std::sync::atomic::AtomicUsize;

use or_io::gta5::shaders::fxc::{GrcEffectPass, GrcEffectProgram, GrcEffectTechnique};

use crate::dxbc_testing::{container, plain_vertex_shader, red_pixel_shader};

#[test]
fn a_program_that_translates_and_builds_is_built() {
    assert_eq!(
        judge(&red_pixel_shader(), GrcProgramStage::Pixel, None),
        Outcome::Built
    );
}

#[test]
fn a_program_whose_own_stage_is_not_the_slot_it_fills_fails_before_it_is_translated() {
    let outcome = judge(&red_pixel_shader(), GrcProgramStage::Vertex, None);
    let Outcome::Failed(cause) = outcome else {
        panic!("a pixel shader in a vertex slot built");
    };
    assert!(cause.contains("stage differs"), "{cause}");
}

#[test]
fn a_program_the_translator_refuses_fails_with_its_message() {
    let broken = container(&[(*b"SHDR", vec![0; 4])]);
    let Outcome::Failed(cause) = judge(&broken, GrcProgramStage::Pixel, None) else {
        panic!("a program with no signatures built");
    };
    assert!(!cause.is_empty());
    let Outcome::Failed(cause) = judge(b"nonsense", GrcProgramStage::Pixel, None) else {
        panic!("nonsense built");
    };
    assert!(
        cause.contains("stage differs") || cause.contains("refused"),
        "{cause}"
    );
}

/// A `.fxc` with the header and six empty banks and nothing else.
fn skeleton() -> Vec<u8> {
    let empty_bank = || {
        let mut out = vec![1u8, 5];
        out.extend_from_slice(b"NULL\0");
        out.extend_from_slice(&[0; 6]);
        out
    };
    let mut out = b"rgxe".to_vec();
    out.extend_from_slice(&0x59u32.to_le_bytes());
    out.push(0);
    for _ in 0..6 {
        out.extend(empty_bank());
    }
    out.extend_from_slice(&[0, 0, 0, 0, 0]);
    out
}

#[test]
fn a_folder_of_effects_is_read_and_an_unreadable_one_is_counted() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("ok.fxc"), skeleton()).unwrap();
    std::fs::write(tmp.path().join("bad.fxc"), b"not an effect").unwrap();
    std::fs::write(tmp.path().join("other.txt"), skeleton()).unwrap();
    let tally = pass_check(tmp.path(), "shaders/win32_40_final", None).unwrap();
    assert_eq!((tally.effects, tally.unreadable), (1, 1));
    assert_eq!(tally.passes(), 0);
}

/// Programs laid end to end, the techniques naming them by one-based position: `(technique, [(passes of programs per slot)])`.
fn effect(
    blobs: &[(GrcProgramStage, Vec<u8>)],
    techniques: &[(&str, &[[u8; 6]])],
) -> (GrcEffect, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut programs = Vec::new();
    for (stage, blob) in blobs {
        programs.push(GrcEffectProgram {
            stage: *stage,
            name: String::new(),
            variables: Vec::new(),
            cbuffers: Vec::new(),
            bytecode_offset: bytes.len(),
            bytecode_size: blob.len(),
            shader_model: None,
        });
        bytes.extend_from_slice(blob);
    }
    let techniques = techniques
        .iter()
        .map(|(name, passes)| GrcEffectTechnique {
            name: name.to_string(),
            passes: passes
                .iter()
                .map(|programs| GrcEffectPass {
                    programs: *programs,
                    render_states: Vec::new(),
                })
                .collect(),
        })
        .collect();
    (
        GrcEffect {
            programs,
            techniques,
            ..GrcEffect::default()
        },
        bytes,
    )
}

/// A compiler that builds every module and pipeline, or refuses every pipeline, and counts the pipelines it was asked for.
struct Counting {
    refuse: bool,
    pipelines: AtomicUsize,
    with_a_pixel_stage: AtomicUsize,
}

impl Counting {
    fn new(refuse: bool) -> Self {
        Self {
            refuse,
            pipelines: AtomicUsize::new(0),
            with_a_pixel_stage: AtomicUsize::new(0),
        }
    }
}

impl ModuleCompiler for Counting {
    fn compile(&self, _: &naga::Module) -> Result<(), String> {
        Ok(())
    }

    fn compile_pipeline(
        &self,
        _: &naga::Module,
        pixel: Option<&naga::Module>,
    ) -> Result<(), String> {
        self.pipelines.fetch_add(1, Ordering::SeqCst);
        self.with_a_pixel_stage
            .fetch_add(usize::from(pixel.is_some()), Ordering::SeqCst);
        if self.refuse {
            Err("the stages do not meet".into())
        } else {
            Ok(())
        }
    }
}

fn drawing() -> (GrcEffect, Vec<u8>) {
    effect(
        &[
            (GrcProgramStage::Vertex, plain_vertex_shader()),
            (GrcProgramStage::Pixel, red_pixel_shader()),
        ],
        &[
            ("draw", &[[1, 1, 0, 0, 0, 0]]),
            ("again", &[[1, 1, 0, 0, 0, 0]]),
            ("depth", &[[1, 0, 0, 0, 0, 0]]),
        ],
    )
}

#[test]
fn a_pair_two_passes_share_is_built_once_and_counted_for_both() {
    let (parsed, bytes) = drawing();
    let compiler = Counting::new(false);
    let tally = check_effects(&[(parsed, &bytes)], Some(&compiler));
    let pipelines = tally.pipelines.expect("a device was offered");
    // Two passes make the vertex-and-pixel pair and one the vertex stage alone
    assert_eq!(
        (pipelines.passes, pipelines.built, pipelines.distinct),
        (3, 3, 2)
    );
    assert_eq!(compiler.pipelines.load(Ordering::SeqCst), 2);
    assert_eq!(compiler.with_a_pixel_stage.load(Ordering::SeqCst), 1);
}

#[test]
fn a_pipeline_the_device_refuses_is_counted_with_its_cause_for_each_pass() {
    let (parsed, bytes) = drawing();
    let tally = check_effects(&[(parsed, &bytes)], Some(&Counting::new(true)));
    let pipelines = tally.pipelines.unwrap();
    assert_eq!((pipelines.passes, pipelines.built), (3, 0));
    assert_eq!(pipelines.causes["the stages do not meet"], 3);
}

#[test]
fn without_a_compiler_no_pipeline_is_attempted() {
    let (parsed, bytes) = drawing();
    let tally = check_effects(&[(parsed, &bytes)], None);
    assert!(tally.pipelines.is_none());
}

#[test]
fn a_pass_whose_program_did_not_build_makes_no_pipeline() {
    // A vertex shader in the pixel slot fails its program check, so the pass that binds it is not attempted
    let (parsed, bytes) = effect(
        &[
            (GrcProgramStage::Vertex, plain_vertex_shader()),
            (GrcProgramStage::Pixel, plain_vertex_shader()),
        ],
        &[("draw", &[[1, 1, 0, 0, 0, 0]])],
    );
    let compiler = Counting::new(false);
    let tally = check_effects(&[(parsed, &bytes)], Some(&compiler));
    assert_eq!(tally.pipelines.unwrap().passes, 0);
    assert_eq!(compiler.pipelines.load(Ordering::SeqCst), 0);
}

#[test]
fn nor_does_a_pass_whose_vertex_program_did_not_build() {
    let (parsed, bytes) = effect(
        &[
            (GrcProgramStage::Vertex, red_pixel_shader()),
            (GrcProgramStage::Pixel, red_pixel_shader()),
        ],
        &[("draw", &[[1, 1, 0, 0, 0, 0]])],
    );
    let compiler = Counting::new(false);
    let tally = check_effects(&[(parsed, &bytes)], Some(&compiler));
    assert_eq!(tally.pipelines.unwrap().passes, 0);
    assert_eq!(compiler.pipelines.load(Ordering::SeqCst), 0);
}

/// Programs whose stages are what a pass names and whose bytes are empty, all taken as built.
fn all_built(effect: &GrcEffect) -> Vec<Vec<Option<Outcome>>> {
    vec![vec![Some(Outcome::Built); effect.programs.len()]]
}

#[test]
fn only_a_pass_of_a_vertex_stage_and_at_most_a_pixel_stage_is_paired() {
    use GrcProgramStage::{Compute, Geometry, Pixel, Vertex};
    let stages: Vec<_> = [Vertex, Pixel, Geometry, Compute]
        .into_iter()
        .map(|s| (s, Vec::new()))
        .collect();
    let (parsed, bytes) = effect(
        &stages,
        &[(
            "all",
            &[
                // vertex and pixel
                [1, 1, 0, 0, 0, 0],
                // vertex alone
                [1, 0, 0, 0, 0, 0],
                // pixel alone
                [0, 1, 0, 0, 0, 0],
                // vertex, pixel and geometry
                [1, 1, 0, 0, 1, 0],
                // compute
                [0, 0, 1, 0, 0, 0],
                // a pixel program the effect lacks
                [1, 2, 0, 0, 0, 0],
                // nothing
                [0; 6],
            ],
        )],
    );
    let outcomes = all_built(&parsed);
    let pairs = pass_pairs(&[(parsed, &bytes)], &outcomes);
    let shapes: Vec<_> = pairs
        .iter()
        .map(|p| (p.vertex.1, p.pixel.map(|(_, i)| i)))
        .collect();
    assert_eq!(shapes, vec![(0, Some(1)), (0, None)]);
}

/// A compiler whose pipeline build panics, as a backend writer can.
struct Panicking;

impl ModuleCompiler for Panicking {
    fn compile(&self, _: &naga::Module) -> Result<(), String> {
        Ok(())
    }

    fn compile_pipeline(&self, _: &naga::Module, _: Option<&naga::Module>) -> Result<(), String> {
        panic!("a writer gave up")
    }
}

#[test]
fn a_pipeline_build_that_panics_is_a_cause_and_not_a_crash() {
    let (parsed, bytes) = drawing();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let tally = check_effects(&[(parsed, &bytes)], Some(&Panicking));
    std::panic::set_hook(previous);
    let pipelines = tally.pipelines.unwrap();
    assert_eq!((pipelines.passes, pipelines.built), (3, 0));
    assert_eq!(pipelines.causes["building the pipeline panicked"], 3);
}

#[test]
fn the_pairs_are_classified_once_by_how_their_signatures_meet() {
    use crate::signature::Linkage;
    let (parsed, bytes) = drawing();
    let tally = check_effects(&[(parsed, &bytes)], None);
    // The hand-built stages pass only a system value, so the one distinct pair links exactly; the vertex-alone pass is not a pair
    assert_eq!(tally.linkage.pairs.len(), 1);
    assert_eq!(tally.linkage.pairs[&Linkage::Exact], 1);
    assert_eq!(tally.linkage.total(), 1);
    assert!(tally.report().contains("1 distinct vertex-and-pixel pairs"));
}

/// A compiler that keeps the vertex stage it was asked to build.
struct Recording(std::sync::Mutex<Option<naga::Module>>);

impl ModuleCompiler for Recording {
    fn compile(&self, _: &naga::Module) -> Result<(), String> {
        Ok(())
    }

    fn compile_pipeline(
        &self,
        vertex: &naga::Module,
        _: Option<&naga::Module>,
    ) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(vertex.clone());
        Ok(())
    }
}

#[test]
fn a_pair_is_linked_before_it_is_built_so_the_vertex_stage_carries_the_pixel_stages_interpolation()
{
    let vertex = naga::front::wgsl::parse_str(
        "@vertex fn main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }",
    )
    .unwrap();
    let pixel = naga::front::wgsl::parse_str(
        "@fragment fn main(@location(0) @interpolate(flat) a: vec4<f32>) -> @location(0) vec4<f32> { return a; }",
    )
    .unwrap();
    let compiler = Recording(Default::default());
    compile_linked(&compiler, vertex, Some(&pixel)).unwrap();
    let built = compiler.0.lock().unwrap().take().unwrap();
    let Some(naga::Binding::Location { interpolation, .. }) = built.entry_points[0]
        .function
        .result
        .as_ref()
        .unwrap()
        .binding
    else {
        panic!("a location binding");
    };
    assert_eq!(interpolation, Some(naga::Interpolation::Flat));

    // A vertex stage alone is built as it is
    let alone = naga::front::wgsl::parse_str(
        "@vertex fn main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }",
    )
    .unwrap();
    compile_linked(&compiler, alone, None).unwrap();
    assert!(compiler.0.lock().unwrap().is_some());
}

/// A compiler that refuses every pipeline in the words a device might, over two lines and with numbers.
struct Refusing;

impl ModuleCompiler for Refusing {
    fn compile(&self, _: &naga::Module) -> Result<(), String> {
        Ok(())
    }

    fn compile_pipeline(&self, _: &naga::Module, _: Option<&naga::Module>) -> Result<(), String> {
        Err("Location[3] Float32x4 does not match\nthe second line".into())
    }
}

#[test]
fn a_cause_the_compiler_gives_is_folded_to_one_line_with_its_numbers_counted_together() {
    let vertex = naga::front::wgsl::parse_str(
        "@vertex fn main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }",
    )
    .unwrap();
    assert_eq!(
        compile_linked(&Refusing, vertex, None),
        Err("Location[N] FloatNxN does not match".to_string())
    );
}
