//! How far each of naga's shader writers gets with the modules of a baked pack: Metal's, and the two a Windows or Linux machine uses, Vulkan's SPIR-V and Direct3D 12's HLSL.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use naga::valid::{Capabilities, ValidationFlags, Validator};
use or_io::gta5::shaders::spirv_pack::ShaderStageKind;
use or_io::gta5::shaders::spirv_pack::{module_path, Outcome, SpirvPack, MANIFEST_FILE};
use serde_json::{json, Value};

use crate::spirv_check::fold;

/// A writer of naga's, and so a platform's route to the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Backend {
    /// Metal: macOS.
    Msl,
    /// Vulkan, with every capability a module asks for allowed.
    SpirvAny,
    /// Vulkan, with only the capabilities every device offers and the few the renderer asks for.
    SpirvBase,
    /// Direct3D 12 compiled by FXC, shader model 5.1.
    Hlsl51,
    /// The SPIR-V written for Vulkan with the common capabilities, which SPIRV-Tools' own validator accepts: what a driver is given, judged by the reference.
    SpirvChecked,
}

impl Backend {
    pub const ALL: [Backend; 4] = [
        Backend::Msl,
        Backend::SpirvAny,
        Backend::SpirvBase,
        Backend::Hlsl51,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Backend::Msl => "Metal (MSL)",
            Backend::SpirvAny => "Vulkan (SPIR-V, any capability)",
            Backend::SpirvBase => "Vulkan (SPIR-V, the common capabilities)",
            Backend::Hlsl51 => "Direct3D 12 (HLSL 5.1)",
            Backend::SpirvChecked => "Vulkan (SPIR-V accepted by the SPIRV-Tools validator)",
        }
    }
}

/// What wgpu allows a Vulkan device before a feature adds to it, and the few a game's shaders ask for that the renderer enables.
fn common_capabilities() -> naga::FastHashSet<naga::back::spv::Capability> {
    use naga::back::spv::Capability as C;
    [
        C::Shader,
        C::Matrix,
        C::Sampled1D,
        C::Image1D,
        C::ImageQuery,
        C::DerivativeControl,
        C::StorageImageExtendedFormats,
        C::SampledCubeArray,
        C::SampleRateShading,
        C::ClipDistance,
        C::StorageImageWriteWithoutFormat,
    ]
    .into_iter()
    .collect()
}

fn spirv_words(
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    capabilities: Option<naga::FastHashSet<naga::back::spv::Capability>>,
) -> Result<Vec<u32>, String> {
    let options = naga::back::spv::Options {
        lang_version: (1, 3),
        capabilities,
        ..Default::default()
    };
    naga::back::spv::write_vec(module, info, &options, None).map_err(|e| e.to_string())
}

fn write_spirv(
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    capabilities: Option<naga::FastHashSet<naga::back::spv::Capability>>,
) -> Result<(), String> {
    spirv_words(module, info, capabilities).map(|_| ())
}

/// SPIRV-Tools' `spirv-val`, and the Vulkan version whose rules it judges a module by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpirvVal {
    pub tool: PathBuf,
    pub target_env: String,
}

impl SpirvVal {
    /// The validator at `tool`, judging as Vulkan 1.1, the version wgpu asks a device for at least.
    pub fn new(tool: impl Into<PathBuf>) -> Self {
        Self {
            tool: tool.into(),
            target_env: "vulkan1.1".to_string(),
        }
    }
}

/// `words` judged by `validator`, and its first complaint if it refuses them.
pub fn validate_vulkan(validator: &SpirvVal, words: &[u32]) -> Result<(), String> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let tool = &validator.tool;
    let mut child = Command::new(tool)
        .args(["--target-env", &validator.target_env, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{} cannot start: {e}", tool.display()))?;
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    if let Some(mut stdin) = child.stdin.take() {
        // The validator reads all of its input before it speaks, so a write that fails is read from what it then says
        let _ = stdin.write_all(&bytes);
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("{} did not finish: {e}", tool.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&output.stderr);
    let complaint = text.lines().next().unwrap_or("it gave no reason");
    // "error: line 123: " is where in the module, which says nothing of why
    let why = complaint
        .strip_prefix("error: ")
        .map(|rest| rest.split_once(": ").map_or(rest, |(_, why)| why))
        .unwrap_or(complaint);
    Err(fold(why))
}

fn write_hlsl(module: &naga::Module, info: &naga::valid::ModuleInfo) -> Result<(), String> {
    let options = naga::back::hlsl::Options {
        shader_model: naga::back::hlsl::ShaderModel::V5_1,
        fake_missing_bindings: true,
        // wgpu's Direct3D 12 backend gives a first vertex and a first instance this way, which a game's shader reads
        special_constants_binding: Some(naga::back::hlsl::BindTarget {
            space: 0,
            register: 0,
            binding_array_size: None,
            dynamic_storage_buffer_offsets_index: None,
            restrict_indexing: false,
        }),
        ..Default::default()
    };
    let pipeline = naga::back::hlsl::PipelineOptions::default();
    let mut text = String::new();
    naga::back::hlsl::Writer::new(&mut text, &options, &pipeline)
        .write(module, info, None)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn write_msl(module: &naga::Module, info: &naga::valid::ModuleInfo) -> Result<(), String> {
    let options = naga::back::msl::Options {
        lang_version: (3, 0),
        fake_missing_bindings: true,
        ..Default::default()
    };
    naga::back::msl::write_string(
        module,
        info,
        &options,
        &naga::back::msl::PipelineOptions::default(),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// How naga reads a legalized module and judges it, or why it will not.
fn read_and_validate(legal: &[u8]) -> Result<(naga::Module, naga::valid::ModuleInfo), String> {
    let options = naga::front::spv::Options {
        adjust_coordinate_space: false,
        strict_capabilities: false,
        block_ctx_dump_prefix: None,
    };
    let module = naga::front::spv::parse_u8_slice(legal, &options)
        .map_err(|e| format!("the reader refused it: {e}"))?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|e| format!("the validator refused it: {e}"))?;
    Ok((module, info))
}

/// What each writer made of one module: `Ok` where it wrote it, else why not. A module naga cannot read or validate stops every writer at the same place.
pub fn check_module(legal: &[u8]) -> BTreeMap<Backend, Result<(), String>> {
    check_module_with(legal, None)
}

/// [`check_module`], and with a `validator` the SPIR-V written for the common capabilities judged by it too.
pub fn check_module_with(
    legal: &[u8],
    validator: Option<&SpirvVal>,
) -> BTreeMap<Backend, Result<(), String>> {
    let (module, info) = match read_and_validate(legal) {
        Ok(read) => read,
        Err(why) => {
            return Backend::ALL
                .iter()
                .copied()
                .chain(validator.map(|_| Backend::SpirvChecked))
                .map(|b| (b, Err(fold(&why))))
                .collect();
        }
    };
    let guarded = |writer: &dyn Fn() -> Result<(), String>, name: &str| {
        catch_unwind(AssertUnwindSafe(writer))
            .unwrap_or_else(|_| Err(format!("the {name} writer panicked")))
            .map_err(|e| fold(&e))
    };
    let mut results = BTreeMap::from([
        (Backend::Msl, guarded(&|| write_msl(&module, &info), "MSL")),
        (
            Backend::SpirvAny,
            guarded(&|| write_spirv(&module, &info, None), "SPIR-V"),
        ),
        (
            Backend::SpirvBase,
            guarded(
                &|| write_spirv(&module, &info, Some(common_capabilities())),
                "SPIR-V",
            ),
        ),
        (
            Backend::Hlsl51,
            guarded(&|| write_hlsl(&module, &info), "HLSL"),
        ),
    ]);
    if let Some(validator) = validator {
        let checked = match catch_unwind(AssertUnwindSafe(|| {
            spirv_words(&module, &info, Some(common_capabilities()))
        })) {
            Ok(Ok(words)) => validate_vulkan(validator, &words),
            _ => Err("no SPIR-V was written to validate".to_string()),
        };
        results.insert(Backend::SpirvChecked, checked);
    }
    results
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BackendTally {
    /// Translated programs checked, by stage.
    pub modules: BTreeMap<String, usize>,
    /// Programs a writer wrote, by writer and stage.
    pub written: BTreeMap<(Backend, String), usize>,
    /// Why a writer did not, by writer and cause.
    pub causes: BTreeMap<(Backend, String), usize>,
}

fn stage_name(stage: ShaderStageKind) -> String {
    format!("{stage:?}")
}

impl BackendTally {
    pub fn add(&mut self, stage: ShaderStageKind, results: &BTreeMap<Backend, Result<(), String>>) {
        let stage = stage_name(stage);
        *self.modules.entry(stage.clone()).or_default() += 1;
        for (backend, result) in results {
            match result {
                Ok(()) => *self.written.entry((*backend, stage.clone())).or_default() += 1,
                Err(cause) => *self.causes.entry((*backend, cause.clone())).or_default() += 1,
            }
        }
    }

    pub fn total(&self) -> usize {
        self.modules.values().sum()
    }

    pub fn written_by(&self, backend: Backend) -> usize {
        self.written
            .iter()
            .filter(|((b, _), _)| *b == backend)
            .map(|(_, n)| n)
            .sum()
    }

    /// The writers the programs were checked with: all of naga's, and the validator's verdict when one was asked.
    fn backends(&self) -> Vec<Backend> {
        let judged = self
            .written
            .keys()
            .any(|(b, _)| *b == Backend::SpirvChecked)
            || self.causes.keys().any(|(b, _)| *b == Backend::SpirvChecked);
        Backend::ALL
            .into_iter()
            .chain(judged.then_some(Backend::SpirvChecked))
            .collect()
    }

    pub fn report(&self) -> String {
        let total = self.total();
        let mut out = format!("{total} translated programs\n");
        for backend in self.backends() {
            let written = self.written_by(backend);
            out.push_str(&format!(
                "\n{}: {written} written ({:.1}%)\n",
                backend.label(),
                100.0 * written as f64 / total.max(1) as f64
            ));
            for (stage, of) in &self.modules {
                let done = self
                    .written
                    .get(&(backend, stage.clone()))
                    .copied()
                    .unwrap_or(0);
                out.push_str(&format!("  {stage:<10} {done:>6} of {of:>6}\n"));
            }
            let mut causes: Vec<_> = self
                .causes
                .iter()
                .filter(|((b, _), _)| *b == backend)
                .collect();
            causes.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            for ((_, cause), n) in causes.into_iter().take(8) {
                out.push_str(&format!("  {n:>6}  {cause}\n"));
            }
        }
        out
    }

    pub fn to_json(&self) -> Value {
        json!({
            "modules": self.modules,
            "writers": self.backends().iter().map(|b| json!({
                "writer": b.label(),
                "written": self.written_by(*b),
                "by_stage": self.written.iter()
                    .filter(|((w, _), _)| w == b)
                    .map(|((_, s), n)| (s.clone(), *n))
                    .collect::<BTreeMap<_, _>>(),
                "causes": self.causes.iter()
                    .filter(|((w, _), _)| w == b)
                    .map(|((_, c), n)| (c.clone(), *n))
                    .collect::<BTreeMap<_, _>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// Run every writer over every translated module of the pack at `root`, on every core.
pub fn check_pack(root: &Path) -> Result<BackendTally, String> {
    check_pack_with(root, None)
}

/// [`check_pack`], with the SPIR-V judged by `validator` too.
pub fn check_pack_with(root: &Path, validator: Option<&SpirvVal>) -> Result<BackendTally, String> {
    let manifest = root.join(MANIFEST_FILE);
    let pack: SpirvPack = serde_json::from_slice(
        &std::fs::read(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?,
    )
    .map_err(|e| format!("{}: {e}", manifest.display()))?;
    let translated: Vec<(&String, ShaderStageKind)> = pack
        .programs
        .iter()
        .filter(|(_, p)| matches!(p.outcome, Outcome::Translated { .. }))
        .map(|(key, p)| (key, p.stage))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let tally = std::sync::Mutex::new(BackendTally::default());
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some((key, stage)) =
                    translated.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                {
                    let results = match std::fs::read(root.join(module_path(key))) {
                        Ok(bytes) => check_module_with(&bytes, validator),
                        Err(e) => Backend::ALL
                            .iter()
                            .copied()
                            .chain(validator.map(|_| Backend::SpirvChecked))
                            .map(|b| (b, Err(format!("the module cannot be read: {e}"))))
                            .collect(),
                    };
                    if let Ok(mut all) = tally.lock() {
                        all.add(*stage, &results);
                    }
                }
            });
        }
    });
    tally
        .into_inner()
        .map_err(|_| "a worker panicked".to_string())
}

#[cfg(test)]
mod tests;
