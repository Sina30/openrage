//! Bakes effects into a SPIR-V pack: one legalized module per distinct program, and the manifest that says which pass of which technique runs which.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use or_io::gta5::shaders::fxc::{DxbcBlob, GrcEffect, GrcProgramStage};
use or_io::gta5::shaders::spirv_pack::ShaderStageKind;
use or_io::gta5::shaders::spirv_pack::{
    module_path, Binding, BindingClass, ConstantBuffer, Effect, Fidelity, NagaReach, Outcome, Pass,
    Production, Program, Reach, Reflection, SpirvPack, Technique, Variable, MANIFEST_FILE,
    PACK_VERSION,
};
use or_shader_front::{BindingLayout, ResourceClass};
use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::bindings::class_of;
use crate::inventory::{facts, InventoryError, Kind};
use crate::legalize::legalize;
use crate::pass_check::{read_effect_files, stage_code};
use crate::passes::program_index;
use crate::signature::signatures;
use crate::spirv_check::{check_legalized, fold, Reached};

#[derive(Debug, Error)]
pub enum PackError {
    #[error(transparent)]
    Source(#[from] InventoryError),
    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot write the manifest: {0}")]
    Manifest(#[from] serde_json::Error),
}

/// An effect read from a game file, with the bytes its programs index into.
pub struct SourceEffect {
    pub name: String,
    pub source_path: String,
    pub effect: GrcEffect,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BakeReport {
    pub effects: usize,
    /// Files that did not parse as an effect.
    pub unreadable: usize,
    /// Distinct programs some pass runs.
    pub programs: usize,
    pub translated: usize,
    pub refused: usize,
    pub skipped: usize,
    /// Programs taken from the pack already in the folder.
    pub reused: usize,
    /// Programs translated by this run.
    pub baked: usize,
    /// Files of the folder's `spirv` that no program of the pack names: modules of a game or a translator gone by, and what a killed run left half written.
    pub swept: usize,
}

impl BakeReport {
    pub fn report(&self) -> String {
        format!(
            "{} effects ({} unreadable), {} distinct programs: {} translated, {} refused, {} set aside; {} reused from the pack in the folder and {} translated now; {} stale files removed\n",
            self.effects,
            self.unreadable,
            self.programs,
            self.translated,
            self.refused,
            self.skipped,
            self.reused,
            self.baked,
            self.swept,
        )
    }
}

/// What writes the modules now: the translator build and a hash of the legalizer's sources.
pub fn production() -> Production {
    let mut hash = Sha1::new();
    hash.update(include_bytes!("legalize.rs"));
    hash.update(include_bytes!("legalize/interface.rs"));
    let legalizer = hash.finalize();
    Production {
        translator: or_shader_front::identity(),
        legalizer: legalizer
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect(),
    }
}

fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn stage_kind(stage: GrcProgramStage) -> ShaderStageKind {
    match stage {
        GrcProgramStage::Vertex => ShaderStageKind::Vertex,
        GrcProgramStage::Pixel => ShaderStageKind::Pixel,
        GrcProgramStage::Compute => ShaderStageKind::Compute,
        GrcProgramStage::Domain => ShaderStageKind::Domain,
        GrcProgramStage::Geometry => ShaderStageKind::Geometry,
        GrcProgramStage::Hull => ShaderStageKind::Hull,
    }
}

fn reach_of(reached: Reached) -> Reach {
    match reached {
        Reached::Refused => Reach::Refused,
        Reached::Parsed => Reach::Parsed,
        Reached::Validated => Reach::Validated,
        Reached::MslWritten | Reached::Compiled => Reach::MslWritten,
    }
}

fn binding_class(class: ResourceClass) -> BindingClass {
    match class {
        ResourceClass::Cbv => BindingClass::Cbv,
        ResourceClass::Sampler => BindingClass::Sampler,
        ResourceClass::Srv => BindingClass::Srv,
        ResourceClass::Uav => BindingClass::Uav,
        ResourceClass::UavCounter => BindingClass::UavCounter,
    }
}

/// The key of a constant buffer layout in the pack's table: the first eight bytes of the SHA-1 of its JSON.
fn buffer_key(buffer: &ConstantBuffer) -> String {
    let text = serde_json::to_vec(buffer).unwrap_or_default();
    Sha1::digest(&text)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What the container's own reflection and signatures say, with each register's binding under the layout, and the constant buffer layouts it names by key.
fn reflection_of(blob: &DxbcBlob<'_>) -> (Reflection, Vec<(String, ConstantBuffer)>) {
    let (inputs, outputs) = signatures(blob);
    let mut reflection = Reflection {
        inputs,
        outputs,
        ..Reflection::default()
    };
    let mut buffers = Vec::new();
    let Some(Ok(rdef)) = blob.reflect() else {
        return (reflection, buffers);
    };
    let layout = BindingLayout::OPENRAGE
        .for_program_type(or_shader_front::program_type(blob.bytes).unwrap_or(1));
    for bound in &rdef.bindings {
        let Some(class) = class_of(bound.kind) else {
            continue;
        };
        let counter = crate::bindings::has_counter(bound.kind).then_some(ResourceClass::UavCounter);
        for class in std::iter::once(class).chain(counter) {
            if let Some(at) = layout.binding(class, 0, bound.register) {
                reflection.bindings.push(Binding {
                    name: bound.name.clone(),
                    kind: bound.kind.as_str().to_string(),
                    class: binding_class(class),
                    register: bound.register,
                    set: at.set,
                    binding: at.binding,
                });
            }
        }
    }
    for buffer in &rdef.constant_buffers {
        let layout = ConstantBuffer {
            name: buffer.name.clone(),
            size: buffer.size,
            variables: buffer
                .variables
                .iter()
                .map(|v| Variable {
                    name: v.name.clone(),
                    offset: v.offset,
                    size: v.size,
                    used: v.used,
                    scalar: v.shape.map_or("", |s| s.scalar.as_str()).to_string(),
                    rows: v.shape.map_or(0, |s| s.rows),
                    columns: v.shape.map_or(0, |s| s.columns),
                    elements: v.shape.map_or(0, |s| s.elements),
                })
                .collect(),
        };
        let key = buffer_key(&layout);
        reflection.constant_buffers.push(key.clone());
        buffers.push((key, layout));
    }
    (reflection, buffers)
}

/// One program as baked: what describes it, the module to store if it has one, and the constant buffer layouts its description names.
#[derive(Debug)]
pub struct Baked {
    pub program: Program,
    pub module: Option<Vec<u8>>,
    pub buffers: Vec<(String, ConstantBuffer)>,
}

/// Translate and legalize one program, and describe it.
pub fn translate_one(blob: &[u8], stage: GrcProgramStage) -> Baked {
    let container = DxbcBlob {
        offset: 0,
        size: blob.len(),
        bytes: blob,
    };
    let own = facts(&container);
    let program = |outcome| Program {
        stage: stage_kind(stage),
        source_bytes: u32::try_from(blob.len()).unwrap_or(u32::MAX),
        outcome,
        reflection: Reflection::default(),
    };
    let without_module = |outcome| Baked {
        program: program(outcome),
        module: None,
        buffers: Vec::new(),
    };
    match own.kind {
        Kind::Dxil => {
            return without_module(Outcome::Skipped {
                reason: "DXIL".into(),
            })
        }
        Kind::Neither => {
            return without_module(Outcome::Skipped {
                reason: "no program part".into(),
            })
        }
        Kind::Dxbc => {}
    }
    if own.stage != Some(stage_code(stage)) {
        return without_module(Outcome::Refused {
            reason: "a program's own stage differs from the slot it fills".into(),
        });
    }
    let refused = |reason: String| {
        without_module(Outcome::Refused {
            reason: fold(&reason),
        })
    };
    let raw = match or_shader_front::dxbc_to_spirv(blob) {
        Ok(raw) => raw,
        Err(error) => return refused(format!("the translator refused it: {error}")),
    };
    let (legal, lowerings) = match legalize(&raw) {
        Ok(done) => done,
        Err(error) => return refused(format!("the legalizer refused it: {error}")),
    };
    let model =
        crate::inventory::spirv_module(&raw, 0).and_then(|m| m.entry_stages.first().copied());
    let verdict = check_legalized(&legal, lowerings.clone(), model, None);
    let fidelity = if lowerings.approximates() {
        Fidelity::Approximated
    } else {
        Fidelity::Equivalent
    };
    let mut described = program(Outcome::Translated {
        spirv_bytes: u32::try_from(legal.len()).unwrap_or(u32::MAX),
        fidelity,
        lowerings: lowerings
            .counts()
            .into_iter()
            .map(|(name, n)| (name.to_string(), u32::try_from(n).unwrap_or(u32::MAX)))
            .collect(),
        naga: NagaReach {
            reached: reach_of(verdict.reached),
            cause: verdict.cause,
        },
    });
    let (reflection, buffers) = reflection_of(&container);
    described.reflection = reflection;
    Baked {
        program: described,
        module: Some(legal),
        buffers,
    }
}

/// Write `bytes` to `path` through a temporary file, so a run that is killed leaves no half-written module.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), PackError> {
    let fail = |source| PackError::Write {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(fail)?;
    }
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&temporary, bytes).map_err(fail)?;
    std::fs::rename(&temporary, path).map_err(fail)
}

/// Remove the modules under `out/spirv` that no translated program of `pack` names, and the temporary files a killed run left, then the shard folders that are empty; how many files went.
///
/// Only a file named `<key>.spv` or with a temporary extension is touched, and only inside `spirv`.
pub fn sweep(out: &Path, pack: &SpirvPack) -> usize {
    let named: std::collections::BTreeSet<&str> = pack
        .programs
        .iter()
        .filter(|(_, p)| matches!(p.outcome, Outcome::Translated { .. }))
        .map(|(key, _)| key.as_str())
        .collect();
    let mut removed = 0;
    let Ok(shards) = std::fs::read_dir(out.join("spirv")) else {
        return 0;
    };
    for shard in shards.flatten().filter(|e| e.path().is_dir()) {
        let Ok(files) = std::fs::read_dir(shard.path()) else {
            continue;
        };
        for file in files.flatten().filter(|e| e.path().is_file()) {
            let path = file.path();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let stale = match extension {
                "spv" => !named.contains(stem),
                other => other.starts_with("tmp"),
            };
            if stale && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        // Only an empty folder goes
        let _ = std::fs::remove_dir(shard.path());
    }
    removed
}

/// The pack already in `out`, when it was made by `production` and so its programs can be reused.
fn previous(out: &Path, production: &Production) -> Option<SpirvPack> {
    let text = std::fs::read_to_string(out.join(MANIFEST_FILE)).ok()?;
    let pack: SpirvPack = serde_json::from_str(&text).ok()?;
    (pack.schema_version == PACK_VERSION && &pack.produced_by == production).then_some(pack)
}

/// Whether `old` can stand in for a fresh translation: a module is there with the size it says, and the other outcomes are not on disk at all.
fn reusable(old: &Program, out: &Path, key: &str) -> bool {
    match &old.outcome {
        Outcome::Translated { spirv_bytes, .. } => std::fs::metadata(out.join(module_path(key)))
            .is_ok_and(|m| m.len() == u64::from(*spirv_bytes)),
        Outcome::Refused { .. } | Outcome::Skipped { .. } => true,
    }
}

/// The bytes of the program at `index` of the effect, if the effect's bytes hold them.
fn blob_of(source: &SourceEffect, index: usize) -> Option<&[u8]> {
    let program = source.effect.programs.get(index)?;
    let end = program.bytecode_offset.checked_add(program.bytecode_size)?;
    source.bytes.get(program.bytecode_offset..end)
}

/// The value `effect` declares for each constant that has one, as 32-bit words: the file says the type, and the type says whether it is floats or integers.
pub fn defaults_of(effect: &GrcEffect) -> BTreeMap<String, Vec<u32>> {
    let mut defaults = BTreeMap::new();
    for variable in effect
        .variables
        .iter()
        .filter(|v| !v.var_type.is_resource())
    {
        let words: Vec<u32> = if variable.default_floats.is_empty() {
            variable.default_uints.clone()
        } else {
            variable
                .default_floats
                .iter()
                .map(|f| f.to_bits())
                .collect()
        };
        if !words.is_empty() {
            defaults.entry(variable.name.clone()).or_insert(words);
        }
    }
    defaults
}

/// Bake `sources` into `out`, reusing what a pack made by the same `production` already holds there.
pub fn bake_effects(
    sources: &[SourceEffect],
    out: &Path,
    production: &Production,
    source_build_hash: &str,
) -> Result<(SpirvPack, BakeReport), PackError> {
    bake_effects_with(sources, out, production, source_build_hash, &|_, _| {})
}

/// [`bake_effects`], telling `progress` how many of the programs to translate are done, from none to all, as each is.
pub fn bake_effects_with(
    sources: &[SourceEffect],
    out: &Path,
    production: &Production,
    source_build_hash: &str,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<(SpirvPack, BakeReport), PackError> {
    // Every program some pass runs, once, by the hash of its source.
    let mut wanted: BTreeMap<String, (GrcProgramStage, &[u8])> = BTreeMap::new();
    let mut effects = Vec::new();
    for source in sources {
        let mut techniques = Vec::new();
        for technique in &source.effect.techniques {
            let mut passes = Vec::new();
            for pass in &technique.passes {
                let mut stages = BTreeMap::new();
                for (slot, stage) in GrcProgramStage::ALL.iter().enumerate() {
                    let position = usize::from(pass.programs[slot]);
                    let Some(index) = (position > 0)
                        .then(|| program_index(&source.effect, *stage, position))
                        .flatten()
                    else {
                        continue;
                    };
                    let Some(blob) = blob_of(source, index) else {
                        continue;
                    };
                    let key = sha1_hex(blob);
                    wanted.entry(key.clone()).or_insert((*stage, blob));
                    stages.insert(stage_kind(*stage), key);
                }
                passes.push(Pass { stages });
            }
            techniques.push(Technique {
                name: technique.name.clone(),
                passes,
            });
        }
        effects.push(Effect {
            name: source.name.clone(),
            source_path: source.source_path.clone(),
            vertex_type: source.effect.vertex_type,
            techniques,
            defaults: defaults_of(&source.effect),
        });
    }

    let before = previous(out, production);
    let mut pack = SpirvPack::new(production.clone());
    pack.source_build_hash = source_build_hash.to_string();
    pack.effects = effects;
    let mut report = BakeReport {
        effects: sources.len(),
        programs: wanted.len(),
        ..BakeReport::default()
    };

    let mut to_translate: Vec<(&String, GrcProgramStage, &[u8])> = Vec::new();
    for (key, (stage, blob)) in &wanted {
        match before.as_ref().and_then(|b| b.programs.get(key)) {
            Some(old) if reusable(old, out, key) => {
                let table = &before.as_ref().map(|b| &b.constant_buffers);
                for buffer_key in &old.reflection.constant_buffers {
                    if let Some(buffer) = table.and_then(|t| t.get(buffer_key)) {
                        pack.constant_buffers
                            .insert(buffer_key.clone(), buffer.clone());
                    }
                }
                pack.programs.insert(key.clone(), old.clone());
                report.reused += 1;
            }
            _ => to_translate.push((key, *stage, blob)),
        }
    }

    let next = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let total = to_translate.len();
    progress(0, total);
    let done: Mutex<Vec<(String, Baked)>> = Mutex::new(Vec::new());
    let failure: Mutex<Option<PackError>> = Mutex::new(None);
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some((key, stage, blob)) =
                    to_translate.get(next.fetch_add(1, Ordering::Relaxed))
                {
                    let mut baked = translate_one(blob, *stage);
                    if let Some(module) = baked.module.take() {
                        if let Err(error) = write_atomically(&out.join(module_path(key)), &module) {
                            if let Ok(mut slot) = failure.lock() {
                                slot.get_or_insert(error);
                            }
                            continue;
                        }
                    }
                    if let Ok(mut all) = done.lock() {
                        all.push(((*key).clone(), baked));
                    }
                    progress(finished.fetch_add(1, Ordering::Relaxed) + 1, total);
                }
            });
        }
    });
    if let Some(error) = failure.into_inner().ok().flatten() {
        return Err(error);
    }
    for (key, baked) in done.into_inner().unwrap_or_default() {
        report.baked += 1;
        pack.constant_buffers.extend(baked.buffers);
        pack.programs.insert(key, baked.program);
    }

    for program in pack.programs.values() {
        match program.outcome {
            Outcome::Translated { .. } => report.translated += 1,
            Outcome::Refused { .. } => report.refused += 1,
            Outcome::Skipped { .. } => report.skipped += 1,
        }
    }
    write_atomically(&out.join(MANIFEST_FILE), &serde_json::to_vec(&pack)?)?;
    // After the manifest, so a run that stops before it has swept nothing the old manifest names
    report.swept = sweep(out, &pack);
    Ok((pack, report))
}

/// Bake the effects of `folder` in an install, or every `.fxc` under a folder of loose files, into `out`.
pub fn bake(
    path: &Path,
    folder: &str,
    out: &Path,
    source_build_hash: &str,
) -> Result<(SpirvPack, BakeReport), PackError> {
    bake_with(path, folder, out, source_build_hash, &|_, _| {})
}

/// [`bake`], telling `progress` how many programs are translated, as [`bake_effects_with`] does.
pub fn bake_with(
    path: &Path,
    folder: &str,
    out: &Path,
    source_build_hash: &str,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<(SpirvPack, BakeReport), PackError> {
    let mut sources = Vec::new();
    let mut unreadable = 0;
    for (name, bytes) in read_effect_files(path, folder)? {
        match GrcEffect::parse(&bytes) {
            Ok(effect) => sources.push(SourceEffect {
                name: Path::new(&name)
                    .file_stem()
                    .map_or_else(|| name.clone(), |s| s.to_string_lossy().into_owned()),
                source_path: name,
                effect,
                bytes,
            }),
            Err(_) => unreadable += 1,
        }
    }
    let (pack, mut report) =
        bake_effects_with(&sources, out, &production(), source_build_hash, progress)?;
    report.unreadable = unreadable;
    Ok((pack, report))
}

#[cfg(test)]
mod tests;
