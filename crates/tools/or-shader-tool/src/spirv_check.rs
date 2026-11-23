//! How far naga gets with a translated SPIR-V module once it is legalized: the measure of a front end's output.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use naga::valid::{Capabilities, ValidationFlags, Validator};
use serde_json::{json, Value};

use crate::defs::walk;
use crate::inventory::spirv_module;
use crate::legalize::{legalize, Lowerings};

/// The Metal language version modules are written for: the oldest that has every attribute a translated shader needs.
const MSL_LANGUAGE: (u8, u8) = (3, 0);

/// The last step a module passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reached {
    Refused,
    Parsed,
    Validated,
    MslWritten,
    /// A pipeline made from it on a real device, which ran the platform's own shader compiler.
    Compiled,
}

impl Reached {
    fn as_str(self) -> &'static str {
        match self {
            Reached::Refused => "refused by the reader",
            Reached::Parsed => "parsed",
            Reached::Validated => "validated",
            Reached::MslWritten => "written as MSL",
            Reached::Compiled => "compiled on the device",
        }
    }
}

/// Something that takes a validated module the rest of the way: a device that builds a pipeline from it.
pub trait ModuleCompiler: Sync {
    fn compile(&self, module: &naga::Module) -> Result<(), String>;

    /// A render pipeline made of `vertex` and, when given, `fragment`: the stages of one pass built together, so what one writes meets what the other reads.
    fn compile_pipeline(
        &self,
        _vertex: &naga::Module,
        _fragment: Option<&naga::Module>,
    ) -> Result<(), String> {
        Err("this compiler builds one module at a time".into())
    }
}

/// How naga reads a legalized module: strict about nothing, since the legalizer has taken out what it would refuse.
fn reader_options() -> naga::front::spv::Options {
    naga::front::spv::Options {
        adjust_coordinate_space: false,
        strict_capabilities: false,
        block_ctx_dump_prefix: None,
    }
}

/// A translator's module, legalized and read by naga, as a device would be given it.
#[cfg(feature = "native-front")]
pub(crate) fn naga_module(raw: &[u8]) -> Result<naga::Module, String> {
    let (legal, _) = legalize(raw).map_err(|e| fold(&e.to_string()))?;
    naga::front::spv::parse_u8_slice(&legal, &reader_options()).map_err(|e| fold(&e.to_string()))
}

#[derive(Debug)]
pub struct Verdict {
    pub reached: Reached,
    /// Why it stopped, with digits folded so equal causes count together.
    pub cause: Option<String>,
    pub lowerings: Lowerings,
    pub stage: Option<u32>,
    /// Whether a compiler was offered the module.
    pub device: bool,
}

/// An error's text with each run of digits folded to `N`, cut to a line.
pub(crate) fn fold(text: &str) -> String {
    let mut out = String::new();
    let mut in_digits = false;
    for c in text.lines().next().unwrap_or("").chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('N');
            }
            in_digits = true;
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out.chars().take(120).collect()
}

/// The stage a SPIR-V execution model is: 0 vertex, 1 tessellation control, 2 tessellation evaluation, 3 geometry, 4 fragment, 5 compute.
pub fn spirv_stage_label(model: u32) -> &'static str {
    match model {
        0 => "VS",
        1 => "HS",
        2 => "DS",
        3 => "GS",
        4 => "PS",
        5 => "CS",
        _ => "?",
    }
}

fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(next) = source {
        text.push_str(" | ");
        text.push_str(&next.to_string());
        source = next.source();
    }
    text
}

/// Legalize, read, validate against every capability and write as MSL, and say where it stopped.
pub fn check(bytes: &[u8]) -> Verdict {
    check_with(bytes, None)
}

/// [`check`], then offer what survives to `compiler`.
pub fn check_with(bytes: &[u8], compiler: Option<&dyn ModuleCompiler>) -> Verdict {
    let stage = spirv_module(bytes, 0).and_then(|m| m.entry_stages.first().copied());
    match legalize(bytes) {
        Ok((legal, lowerings)) => check_legalized(&legal, lowerings, stage, compiler),
        Err(error) => Verdict {
            reached: Reached::Refused,
            cause: Some(fold(&error.to_string())),
            lowerings: Lowerings::default(),
            stage,
            device: false,
        },
    }
}

/// [`check_with`] for a module the legalizer has already rewritten, with the `lowerings` it reported and the `stage` of the original.
pub(crate) fn check_legalized(
    legal: &[u8],
    lowerings: Lowerings,
    stage: Option<u32>,
    compiler: Option<&dyn ModuleCompiler>,
) -> Verdict {
    let device = compiler.is_some();
    let stopped = |reached, cause: String, lowerings| Verdict {
        reached,
        cause: Some(fold(&cause)),
        lowerings,
        stage,
        device: false,
    };
    let module = match naga::front::spv::parse_u8_slice(legal, &reader_options()) {
        Ok(module) => module,
        Err(error) => return stopped(Reached::Refused, error.to_string(), lowerings),
    };
    let info = match Validator::new(ValidationFlags::all(), Capabilities::all()).validate(&module) {
        Ok(info) => info,
        Err(error) => return stopped(Reached::Parsed, chain(error.as_inner()), lowerings),
    };
    let msl = naga::back::msl::Options {
        lang_version: MSL_LANGUAGE,
        fake_missing_bindings: true,
        ..Default::default()
    };
    let written = catch_unwind(AssertUnwindSafe(|| {
        naga::back::msl::write_string(
            &module,
            &info,
            &msl,
            &naga::back::msl::PipelineOptions::default(),
        )
    }));
    match written {
        Ok(Ok(_)) => match compiler.map(|c| c.compile(&module)) {
            None => Verdict {
                reached: Reached::MslWritten,
                cause: None,
                lowerings,
                stage,
                device,
            },
            Some(Ok(())) => Verdict {
                reached: Reached::Compiled,
                cause: None,
                lowerings,
                stage,
                device,
            },
            Some(Err(message)) => Verdict {
                reached: Reached::MslWritten,
                cause: Some(fold(&message)),
                lowerings,
                stage,
                device,
            },
        },
        Ok(Err(error)) => stopped(Reached::Validated, error.to_string(), lowerings),
        Err(_) => stopped(
            Reached::Validated,
            "the MSL writer panicked".into(),
            lowerings,
        ),
    }
}

#[derive(Debug, Default)]
pub struct StageTally {
    pub modules: usize,
    pub parsed: usize,
    pub validated: usize,
    pub msl: usize,
    pub compiled: usize,
}

#[derive(Debug, Default)]
pub struct Tally {
    pub stages: BTreeMap<Option<u32>, StageTally>,
    /// Whether any module was offered to a device, so the report shows its column.
    pub device_used: bool,
    /// Modules that stopped at a step, by cause.
    pub causes: BTreeMap<(Reached, String), usize>,
    pub lowerings: Lowerings,
}

impl Tally {
    pub fn add(&mut self, verdict: &Verdict) {
        let stage = self.stages.entry(verdict.stage).or_default();
        stage.modules += 1;
        stage.parsed += usize::from(verdict.reached >= Reached::Parsed);
        stage.validated += usize::from(verdict.reached >= Reached::Validated);
        stage.msl += usize::from(verdict.reached >= Reached::MslWritten);
        stage.compiled += usize::from(verdict.reached == Reached::Compiled);
        self.device_used |= verdict.device;
        if let Some(cause) = &verdict.cause {
            *self
                .causes
                .entry((verdict.reached, cause.clone()))
                .or_default() += 1;
        }
        self.lowerings.add(&verdict.lowerings);
    }

    /// Add another tally's counts to this one.
    pub fn merge(&mut self, other: Tally) {
        for (stage, t) in other.stages {
            let mine = self.stages.entry(stage).or_default();
            mine.modules += t.modules;
            mine.parsed += t.parsed;
            mine.validated += t.validated;
            mine.msl += t.msl;
            mine.compiled += t.compiled;
        }
        self.device_used |= other.device_used;
        for (key, n) in other.causes {
            *self.causes.entry(key).or_default() += n;
        }
        self.lowerings.add(&other.lowerings);
    }

    pub fn modules(&self) -> usize {
        self.stages.values().map(|s| s.modules).sum()
    }

    fn total(&self, pick: impl Fn(&StageTally) -> usize) -> usize {
        self.stages.values().map(pick).sum()
    }

    pub fn report(&self) -> String {
        let mut out = format!(
            "{} modules: {} parsed, {} validated, {} written as MSL",
            self.modules(),
            self.total(|s| s.parsed),
            self.total(|s| s.validated),
            self.total(|s| s.msl),
        );
        if self.device_used {
            out.push_str(&format!(
                ", {} compiled on the device",
                self.total(|s| s.compiled)
            ));
        }
        out.push_str("\n\n  stage  modules  parsed  validated  msl");
        out.push_str(if self.device_used { "  device\n" } else { "\n" });
        for (stage, t) in &self.stages {
            out.push_str(&format!(
                "  {:<6} {:>7} {:>7} {:>10} {:>5}",
                stage.map_or("?", spirv_stage_label),
                t.modules,
                t.parsed,
                t.validated,
                t.msl,
            ));
            if self.device_used {
                out.push_str(&format!(" {:>7}", t.compiled));
            }
            out.push('\n');
        }
        out.push_str("\nwhere the rest stopped:\n");
        let mut causes: Vec<_> = self.causes.iter().collect();
        causes.sort_by(|a, b| b.1.cmp(a.1));
        for ((reached, cause), n) in causes.into_iter().take(20) {
            out.push_str(&format!(
                "  {n:>6}  after {:<14} {cause}\n",
                reached.as_str()
            ));
        }
        let l = &self.lowerings;
        out.push_str(&format!(
            "\nlowerings: {} capabilities and {} float modes dropped, {} LocalSizeId, {} demotes became a kill, {} demotes kept, {} interface locations merged, {} left shared, {} clip distances dropped\n",
            l.capabilities_dropped,
            l.float_modes_dropped,
            l.local_size_ids,
            l.demotes_to_kill,
            l.demotes_kept,
            l.interface_groups_merged,
            l.interface_groups_kept,
            l.clip_distances_dropped,
        ));
        out
    }

    pub fn to_json(&self) -> Value {
        let stages: Vec<Value> = self
            .stages
            .iter()
            .map(|(stage, t)| {
                json!({
                    "stage": stage.map_or("?", spirv_stage_label),
                    "modules": t.modules,
                    "parsed": t.parsed,
                    "validated": t.validated,
                    "msl": t.msl,
                    "compiled": t.compiled,
                })
            })
            .collect();
        let causes: Vec<Value> = self
            .causes
            .iter()
            .map(|((reached, cause), n)| json!({ "after": reached.as_str(), "cause": cause, "modules": n }))
            .collect();
        json!({ "modules": self.modules(), "stages": stages, "causes": causes })
    }
}

/// Check a file, or every file under a folder, and tally the verdicts.
pub fn check_path(path: &Path) -> std::io::Result<Tally> {
    check_path_with(path, None)
}

/// [`check_path`], offering every module that survives to `compiler`, on every core.
pub fn check_path_with(
    path: &Path,
    compiler: Option<&dyn ModuleCompiler>,
) -> std::io::Result<Tally> {
    let files = if path.is_dir() {
        walk(path)?
    } else {
        vec![path.to_path_buf()]
    };
    let mut contents = Vec::with_capacity(files.len());
    for file in files {
        contents.push(std::fs::read(file)?);
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    let mut tally = Tally::default();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut local = Tally::default();
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(bytes) = contents.get(index) else {
                            break;
                        };
                        local.add(&check_with(bytes, compiler));
                    }
                    local
                })
            })
            .collect();
        for handle in handles {
            if let Ok(local) = handle.join() {
                tally.merge(local);
            }
        }
    });
    Ok(tally)
}

#[cfg(test)]
mod tests;
