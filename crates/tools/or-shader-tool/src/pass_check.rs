//! The effects of an install run through the translator, the legalizer, naga and optionally a device, judged a pass at a time.

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use or_io::gta5::shaders::fxc::{DxbcBlob, GrcEffect, GrcProgramStage};
use or_renderer::game_shaders::link::match_interpolation;

use sha1::{Digest, Sha1};

use crate::inventory::{facts, for_each_source, InventoryError, Target};
use crate::passes::{program_index, Outcome, PassTally, PipelineTally};
use crate::signature::{link, signatures};
use crate::spirv_check::{check_with, fold, naga_module, ModuleCompiler, Reached};

/// The program-type code a container carries for each stage a pass can bind.
pub(crate) fn stage_code(stage: GrcProgramStage) -> u32 {
    match stage {
        GrcProgramStage::Pixel => 0,
        GrcProgramStage::Vertex => 1,
        GrcProgramStage::Geometry => 2,
        GrcProgramStage::Hull => 3,
        GrcProgramStage::Domain => 4,
        GrcProgramStage::Compute => 5,
    }
}

/// Translate one program and take it as far as `compiler` (or naga's Metal writer) will.
fn judge(blob: &[u8], stage: GrcProgramStage, compiler: Option<&dyn ModuleCompiler>) -> Outcome {
    let own = facts(&DxbcBlob {
        offset: 0,
        size: blob.len(),
        bytes: blob,
    });
    if own.stage != Some(stage_code(stage)) {
        return Outcome::Failed("a program's own stage differs from the slot it fills".into());
    }
    match or_shader_front::dxbc_to_spirv(blob) {
        Err(error) => Outcome::Failed(fold(&format!("the translator refused it: {error}"))),
        Ok(spirv) => {
            let verdict = check_with(&spirv, compiler);
            let built = match compiler {
                Some(_) => verdict.reached == Reached::Compiled,
                None => verdict.reached >= Reached::MslWritten,
            };
            if built {
                Outcome::Built
            } else {
                Outcome::Failed(verdict.cause.unwrap_or_else(|| "it did not build".into()))
            }
        }
    }
}

/// The `.fxc` files of `folder` in an install, or every `.fxc` under a folder of loose files, as `(name, bytes)`.
pub(crate) fn read_effect_files(
    path: &Path,
    folder: &str,
) -> Result<Vec<(String, Vec<u8>)>, InventoryError> {
    let install = path.join("common.rpf").exists();
    let prefix = format!("{folder}/");
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for_each_source(path, Target::Gen8, &mut |name, bytes| {
        let wanted = name.ends_with(".fxc") && (!install || name.starts_with(&prefix));
        if wanted {
            files.push((name.to_string(), bytes.to_vec()));
        }
    })?;
    Ok(files)
}

/// Every `.fxc` of `folder` in an install, or every `.fxc` under a folder of loose files, judged a pass at a time.
pub fn pass_check(
    path: &Path,
    folder: &str,
    compiler: Option<&dyn ModuleCompiler>,
) -> Result<PassTally, InventoryError> {
    let files = read_effect_files(path, folder)?;
    let mut unreadable = 0;
    let mut effects: Vec<(GrcEffect, &[u8])> = Vec::new();
    for (_, bytes) in &files {
        match GrcEffect::parse(bytes) {
            Ok(effect) => effects.push((effect, bytes)),
            Err(_) => unreadable += 1,
        }
    }
    let mut tally = check_effects(&effects, compiler);
    tally.unreadable = unreadable;
    Ok(tally)
}

/// The bytes of program `program` of effect `effect`.
fn blob_of<'a>(effects: &[(GrcEffect, &'a [u8])], effect: usize, program: usize) -> &'a [u8] {
    let (parsed, bytes) = &effects[effect];
    let program = &parsed.programs[program];
    bytes
        .get(program.bytecode_offset..program.bytecode_offset + program.bytecode_size)
        .unwrap_or(&[])
}

fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A pass that binds a vertex stage and at most a pixel stage, whose programs all built: where its two programs are, and a key for the pair.
struct PassPair {
    vertex: (usize, usize),
    pixel: Option<(usize, usize)>,
    key: (String, Option<String>),
}

fn pass_pairs(effects: &[(GrcEffect, &[u8])], outcomes: &[Vec<Option<Outcome>>]) -> Vec<PassPair> {
    let built = |e: usize, p: usize| outcomes[e][p] == Some(Outcome::Built);
    let mut pairs = Vec::new();
    for (e, (effect, _)) in effects.iter().enumerate() {
        for pass in effect.techniques.iter().flat_map(|t| &t.passes) {
            let position = |stage: GrcProgramStage| {
                let slot = GrcProgramStage::ALL.iter().position(|s| *s == stage)?;
                let at = usize::from(pass.programs[slot]);
                (at > 0).then_some(at)
            };
            let others = GrcProgramStage::ALL
                .iter()
                .filter(|s| !matches!(s, GrcProgramStage::Vertex | GrcProgramStage::Pixel))
                .any(|s| position(*s).is_some());
            let Some(vertex) = position(GrcProgramStage::Vertex)
                .and_then(|at| program_index(effect, GrcProgramStage::Vertex, at))
            else {
                continue;
            };
            let pixel = match position(GrcProgramStage::Pixel) {
                None => None,
                Some(at) => match program_index(effect, GrcProgramStage::Pixel, at) {
                    Some(index) => Some(index),
                    None => continue,
                },
            };
            if others || !built(e, vertex) || pixel.is_some_and(|p| !built(e, p)) {
                continue;
            }
            pairs.push(PassPair {
                vertex: (e, vertex),
                pixel: pixel.map(|p| (e, p)),
                key: (
                    sha1_hex(blob_of(effects, e, vertex)),
                    pixel.map(|p| sha1_hex(blob_of(effects, e, p))),
                ),
            });
        }
    }
    pairs
}

/// Link the two stages of a pass and build them as one pipeline.
fn compile_linked(
    compiler: &dyn ModuleCompiler,
    mut vertex: naga::Module,
    pixel: Option<&naga::Module>,
) -> Result<(), String> {
    if let Some(pixel) = pixel {
        match_interpolation(&mut vertex, pixel);
    }
    compiler
        .compile_pipeline(&vertex, pixel)
        .map_err(|cause| fold(&cause))
}

/// Build each distinct pair as one pipeline, on every core.
fn build_pipelines(
    effects: &[(GrcEffect, &[u8])],
    pairs: &[PassPair],
    compiler: &dyn ModuleCompiler,
) -> HashMap<(String, Option<String>), Result<(), String>> {
    let mut distinct: Vec<&PassPair> = Vec::new();
    let mut seen = HashSet::new();
    for pair in pairs {
        if seen.insert(pair.key.clone()) {
            distinct.push(pair);
        }
    }
    let next = AtomicUsize::new(0);
    let results = Mutex::new(HashMap::new());
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(pair) = distinct.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let module = |(e, p): (usize, usize)| -> Result<naga::Module, String> {
                        let raw = or_shader_front::dxbc_to_spirv(blob_of(effects, e, p)).map_err(
                            |error| fold(&format!("the translator refused it: {error}")),
                        )?;
                        naga_module(&raw)
                    };
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        let vertex = module(pair.vertex)?;
                        let pixel = pair.pixel.map(module).transpose()?;
                        compile_linked(compiler, vertex, pixel.as_ref())
                    }))
                    .unwrap_or_else(|_| Err("building the pipeline panicked".into()));
                    if let Ok(mut all) = results.lock() {
                        all.insert(pair.key.clone(), result);
                    }
                }
            });
        }
    });
    results.into_inner().unwrap_or_default()
}

/// Judge every program of `effects`, then each pass, and with a `compiler` build the passes of a vertex and a pixel stage as pipelines.
pub(crate) fn check_effects(
    effects: &[(GrcEffect, &[u8])],
    compiler: Option<&dyn ModuleCompiler>,
) -> PassTally {
    let mut tally = PassTally::default();
    let jobs: Vec<(usize, usize)> = effects
        .iter()
        .enumerate()
        .flat_map(|(e, (effect, _))| (0..effect.programs.len()).map(move |p| (e, p)))
        .collect();
    let next = AtomicUsize::new(0);
    let outcomes: Mutex<Vec<Vec<Option<Outcome>>>> = Mutex::new(
        effects
            .iter()
            .map(|(effect, _)| vec![None; effect.programs.len()])
            .collect(),
    );
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(&(e, p)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let program = &effects[e].0.programs[p];
                    let outcome = judge(blob_of(effects, e, p), program.stage, compiler);
                    if let Ok(mut all) = outcomes.lock() {
                        all[e][p] = Some(outcome);
                    }
                }
            });
        }
    });
    let outcomes = outcomes.into_inner().unwrap_or_default();
    for (e, (effect, _)) in effects.iter().enumerate() {
        tally.add_effect(effect, &|p| outcomes[e][p].clone());
    }
    let pairs = pass_pairs(effects, &outcomes);
    let mut classified = HashSet::new();
    for pair in pairs.iter().filter(|p| p.pixel.is_some()) {
        if !classified.insert(pair.key.clone()) {
            continue;
        }
        let signatures_of = |(e, p): (usize, usize)| {
            let blob = blob_of(effects, e, p);
            signatures(&DxbcBlob {
                offset: 0,
                size: blob.len(),
                bytes: blob,
            })
        };
        let (_, outputs) = signatures_of(pair.vertex);
        let (inputs, _) = signatures_of(pair.pixel.unwrap_or(pair.vertex));
        *tally
            .linkage
            .pairs
            .entry(link(&outputs, &inputs))
            .or_default() += 1;
    }
    if let Some(compiler) = compiler {
        let results = build_pipelines(effects, &pairs, compiler);
        let mut pipelines = PipelineTally {
            distinct: results.len(),
            ..PipelineTally::default()
        };
        for pair in &pairs {
            if let Some(result) = results.get(&pair.key) {
                pipelines.add(result);
            }
        }
        tally.pipelines = Some(pipelines);
    }
    tally
}

#[cfg(test)]
mod tests;
