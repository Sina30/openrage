//! What a game's compiled shaders are, counted from their bytes, so a translator is chosen from a measurement.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::gta5::shaders::fxc::DxbcBlob;
use or_title::install::shader_defs::extract_sga_containers;
use or_title::install::shaders::{extract_entries_from_install, DEFAULT_SHADER_DIR};
use or_title::install::GtaInstall;
use serde_json::{json, Map, Value};

use crate::defs::{install_crypto, walk, DefsError};

#[cfg(test)]
mod tests;

const DXBC_MAGIC: &[u8; 4] = b"DXBC";
/// Bytes before a container's part offsets: the tag, a 16-byte digest, a version, the size and the part count.
const CONTAINER_HEADER: usize = 32;
const SIZE_OFFSET: usize = 24;
const PART_COUNT_OFFSET: usize = 28;
const MAX_PARTS: u32 = 256;

const DXIL_MAGIC: u32 = 0x4C49_5844;

/// The other shader folders a Legacy update ships beside `DEFAULT_SHADER_DIR`: a low-quality set and a stereo-3D set.
const LEGACY_VARIANT_DIRS: [&str; 2] =
    ["shaders/win32_40_lq_final", "shaders/win32_nvstereo_final"];

/// A DXBC instruction token: the opcode in the low 11 bits, the length in dwords in bits 24 to 30.
const OPCODE_MASK: u32 = 0x7ff;
const LENGTH_SHIFT: u32 = 24;
const LENGTH_MASK: u32 = 0x7f;
/// Its length is the dword after the token, not the token's own field.
const CUSTOM_DATA: u32 = 53;

const SPIRV_MAGIC: [u8; 4] = [0x03, 0x02, 0x23, 0x07];
const SPIRV_MAX_BOUND: u32 = 0x0040_0000;
const OP_EXTENSION: u32 = 10;
const OP_EXT_INST_IMPORT: u32 = 11;
const OP_MEMORY_MODEL: u32 = 14;
const OP_ENTRY_POINT: u32 = 15;
const OP_EXECUTION_MODE: u32 = 16;
const OP_CAPABILITY: u32 = 17;
const OP_EXECUTION_MODE_ID: u32 = 331;

/// Bit index to name for the feature flags a container's `SFI0` part holds; a bit not named here prints as `bit_N`.
const FEATURES: [(u32, &str); 33] = [
    (0, "Doubles"),
    (1, "ComputeShadersPlusRawAndStructuredBuffersViaShader4X"),
    (2, "UAVsAtEveryStage"),
    (3, "64UAVs"),
    (4, "MinimumPrecision"),
    (5, "11_1_DoubleExtensions"),
    (6, "11_1_ShaderExtensions"),
    (7, "LEVEL9ComparisonFiltering"),
    (8, "TiledResources"),
    (9, "StencilRef"),
    (10, "InnerCoverage"),
    (11, "TypedUAVLoadAdditionalFormats"),
    (12, "ROVs"),
    (13, "ViewportAndRTArrayIndexFromAnyShaderFeedingRasterizer"),
    (14, "WaveOps"),
    (15, "Int64Ops"),
    (16, "ViewID"),
    (17, "Barycentrics"),
    (18, "NativeLowPrecision"),
    (19, "ShadingRate"),
    (20, "Raytracing_Tier_1_1"),
    (21, "SamplerFeedback"),
    (22, "AtomicInt64OnTypedResource"),
    (23, "AtomicInt64OnGroupShared"),
    (24, "DerivativesInMeshAndAmpShaders"),
    (25, "ResourceDescriptorHeapIndexing"),
    (26, "SamplerDescriptorHeapIndexing"),
    (28, "AtomicInt64OnHeapResource"),
    (29, "AdvancedTextureOps"),
    (30, "WriteableMSAATextures"),
    (31, "SampleCmpGradientOrBias"),
    (32, "ExtendedCommandInfo"),
    (33, "LinearAlgebra"),
];

#[derive(Debug, thiserror::Error)]
pub enum InventoryError {
    #[error("input not found: {0}")]
    MissingInput(PathBuf),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Defs(#[from] DefsError),
    #[error("this install yielded no shader container: {0}")]
    NoContainers(String),
}

/// Which edition's shader containers to read out of an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Auto,
    Gen8,
    Gen9,
}

impl Target {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "auto" => Some(Self::Auto),
            "gen8" => Some(Self::Gen8),
            "gen9" => Some(Self::Gen9),
            _ => None,
        }
    }
}

/// The bytecode a container holds, told by its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Dxbc,
    Dxil,
    /// A valid container with neither program part.
    Neither,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Dxbc => "dxbc",
            Kind::Dxil => "dxil",
            Kind::Neither => "neither",
        }
    }
}

/// What one container says about itself.
#[derive(Debug, Clone)]
pub struct BlobFacts {
    pub kind: Kind,
    /// The program-type code, which both bytecodes number alike: 0 pixel, 1 vertex, 2 geometry, 3 hull, 4 domain, 5 compute.
    pub stage: Option<u32>,
    pub model: Option<(u8, u8)>,
    pub bytes: usize,
    pub parts: BTreeSet<[u8; 4]>,
    pub features: u64,
    /// DXBC only: how often each opcode appears, declarations included.
    pub opcodes: BTreeMap<u16, u32>,
    /// A DXBC program whose instruction walk did not end where it says it does, or a DXIL one whose header is wrong.
    pub malformed: bool,
}

/// A SPIR-V module's header region: what it needs of the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpirvFacts {
    pub version: (u8, u8),
    pub capabilities: Vec<u32>,
    pub extensions: Vec<String>,
    pub imports: Vec<String>,
    pub memory_model: Option<(u32, u32)>,
    pub entry_stages: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Profile {
    pub kind: Kind,
    pub stage: Option<u32>,
    pub model: Option<(u8, u8)>,
}

#[derive(Debug, Default)]
pub struct Tally {
    pub shaders: usize,
    pub bytes: usize,
    pub malformed: usize,
    /// Shaders carrying each part.
    pub parts: BTreeMap<[u8; 4], usize>,
    /// Shaders setting each feature bit.
    pub features: BTreeMap<u32, usize>,
    pub opcode_shaders: BTreeMap<u16, usize>,
    pub opcode_instructions: BTreeMap<u16, u64>,
}

#[derive(Debug, Default)]
pub struct SpirvTally {
    pub modules: usize,
    pub versions: BTreeMap<(u8, u8), usize>,
    pub capabilities: BTreeMap<u32, usize>,
    pub extensions: BTreeMap<String, usize>,
    pub memory_models: BTreeMap<(u32, u32), usize>,
    pub entry_stages: BTreeMap<u32, usize>,
}

#[derive(Debug)]
pub struct Source {
    pub name: String,
    pub bytes: usize,
    pub containers: usize,
    pub spirv_modules: usize,
}

#[derive(Debug, Default)]
pub struct Inventory {
    pub sources: Vec<Source>,
    pub profiles: BTreeMap<Profile, Tally>,
    pub spirv: SpirvTally,
    /// Places the bytes read `DXBC` and no valid container followed.
    pub rejected_candidates: usize,
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

/// The size of the container starting at `at`, when its header, part count and part offsets all hold together.
fn container_len(bytes: &[u8], at: usize) -> Option<usize> {
    let word = |offset: usize| read_u32(bytes, at.checked_add(offset)?);
    let size = word(SIZE_OFFSET)? as usize;
    let parts = word(PART_COUNT_OFFSET)?;
    if parts == 0 || parts > MAX_PARTS {
        return None;
    }
    let table_end = CONTAINER_HEADER + parts as usize * 4;
    if size < table_end || at.checked_add(size)? > bytes.len() {
        return None;
    }
    for part in 0..parts as usize {
        let offset = word(CONTAINER_HEADER + part * 4)? as usize;
        if offset < table_end || offset.checked_add(8)? > size {
            return None;
        }
    }
    Some(size)
}

/// A program-type, major and minor from the version token both bytecodes begin with.
fn split_version(token: u32) -> (u32, (u8, u8)) {
    (
        token >> 16,
        (((token >> 4) & 0xf) as u8, (token & 0xf) as u8),
    )
}

/// How often each opcode appears in a `SHEX` or `SHDR` part, and whether the walk ended on the stated length.
fn walk_instructions(data: &[u8]) -> (BTreeMap<u16, u32>, bool) {
    let mut opcodes = BTreeMap::new();
    let words: Vec<u32> = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let end = match words.get(1) {
        Some(&stated) if (2..=words.len() as u32).contains(&stated) => stated as usize,
        _ => return (opcodes, false),
    };
    let mut at = 2;
    while at < end {
        let token = words[at];
        let opcode = token & OPCODE_MASK;
        let length = if opcode == CUSTOM_DATA {
            match words.get(at + 1) {
                Some(&length) => length as usize,
                None => return (opcodes, false),
            }
        } else {
            ((token >> LENGTH_SHIFT) & LENGTH_MASK) as usize
        };
        if length == 0 || at + length > end {
            return (opcodes, false);
        }
        *opcodes.entry(opcode as u16).or_insert(0) += 1;
        at += length;
    }
    (opcodes, true)
}

/// What one container says about itself.
pub fn facts(blob: &DxbcBlob<'_>) -> BlobFacts {
    let parts = blob.parts();
    let part = |tag: &[u8; 4]| parts.iter().find(|p| &p.tag == tag);
    let features = part(b"SFI0")
        .and_then(|p| p.data.get(..8))
        .map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])));

    let mut out = BlobFacts {
        kind: Kind::Neither,
        stage: None,
        model: None,
        bytes: blob.size,
        parts: parts.iter().map(|p| p.tag).collect(),
        features,
        opcodes: BTreeMap::new(),
        malformed: false,
    };
    if let Some(dxil) = part(b"DXIL") {
        out.kind = Kind::Dxil;
        match (
            read_u32(dxil.data, 0),
            read_u32(dxil.data, 8).filter(|&m| m == DXIL_MAGIC),
        ) {
            (Some(token), Some(_)) => {
                let (stage, model) = split_version(token);
                out.stage = Some(stage);
                out.model = Some(model);
            }
            _ => out.malformed = true,
        }
    } else if let Some(program) = part(b"SHEX").or_else(|| part(b"SHDR")) {
        out.kind = Kind::Dxbc;
        match read_u32(program.data, 0) {
            Some(token) => {
                let (stage, model) = split_version(token);
                out.stage = Some(stage);
                out.model = Some(model);
                let (opcodes, whole) = walk_instructions(program.data);
                out.opcodes = opcodes;
                out.malformed = !whole;
            }
            None => out.malformed = true,
        }
    }
    out
}

/// The literal string packed from `words` of the module starting at `at`.
fn spirv_literal(bytes: &[u8], at: usize, first: usize, count: usize) -> String {
    let mut text = Vec::new();
    'words: for index in first..first + count {
        let Some(word) = read_u32(bytes, at + index * 4) else {
            break;
        };
        for byte in word.to_le_bytes() {
            if byte == 0 {
                break 'words;
            }
            text.push(byte);
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}

/// The module whose header starts at `at`, when it opens as a SPIR-V module does: a capability first.
pub fn spirv_module(bytes: &[u8], at: usize) -> Option<SpirvFacts> {
    let word = |index: usize| read_u32(bytes, at.checked_add(index.checked_mul(4)?)?);
    if bytes.get(at..at + 4)? != SPIRV_MAGIC {
        return None;
    }
    let version = word(1)?;
    let (major, minor) = ((version >> 16) as u8, (version >> 8) as u8);
    let bound = word(3)?;
    if version & 0xff00_00ff != 0
        || major != 1
        || minor > 6
        || word(4)? != 0
        || !(1..=SPIRV_MAX_BOUND).contains(&bound)
    {
        return None;
    }
    let mut out = SpirvFacts {
        version: (major, minor),
        capabilities: Vec::new(),
        extensions: Vec::new(),
        imports: Vec::new(),
        memory_model: None,
        entry_stages: Vec::new(),
    };
    let mut index = 5;
    while let Some(head) = word(index) {
        let (count, opcode) = ((head >> 16) as usize, head & 0xffff);
        if count == 0 || word(index + count - 1).is_none() {
            break;
        }
        if out.capabilities.is_empty() && opcode != OP_CAPABILITY {
            return None;
        }
        match opcode {
            OP_CAPABILITY => out.capabilities.push(word(index + 1)?),
            OP_EXTENSION => out
                .extensions
                .push(spirv_literal(bytes, at, index + 1, count - 1)),
            OP_EXT_INST_IMPORT => out
                .imports
                .push(spirv_literal(bytes, at, index + 2, count - 2)),
            OP_MEMORY_MODEL => out.memory_model = Some((word(index + 1)?, word(index + 2)?)),
            OP_ENTRY_POINT => out.entry_stages.push(word(index + 1)?),
            OP_EXECUTION_MODE | OP_EXECUTION_MODE_ID => {}
            _ => break,
        }
        index += count;
    }
    Some(out)
}

pub fn stage_label(code: u32) -> &'static str {
    match code {
        0 => "PS",
        1 => "VS",
        2 => "GS",
        3 => "HS",
        4 => "DS",
        5 => "CS",
        6 => "LIB",
        7 => "RGEN",
        8 => "INT",
        9 => "AHIT",
        10 => "CHIT",
        11 => "MISS",
        12 => "CALL",
        13 => "MS",
        14 => "AS",
        15 => "NODE",
        _ => "?",
    }
}

fn feature_name(bit: u32) -> String {
    FEATURES
        .iter()
        .find(|(b, _)| *b == bit)
        .map_or_else(|| format!("bit_{bit}"), |(_, name)| (*name).to_string())
}

impl Profile {
    fn stage_text(&self) -> &'static str {
        self.stage.map_or("-", stage_label)
    }

    fn model_text(&self) -> String {
        self.model
            .map_or_else(|| "-".into(), |(major, minor)| format!("{major}.{minor}"))
    }
}

impl Inventory {
    /// Count every container and every SPIR-V module in `bytes`, found wherever they sit.
    pub fn scan(&mut self, name: &str, bytes: &[u8]) {
        let mut source = Source {
            name: name.to_string(),
            bytes: bytes.len(),
            containers: 0,
            spirv_modules: 0,
        };
        let mut at = 0;
        while at + 4 <= bytes.len() {
            match bytes[at] {
                b'D' if bytes[at..].starts_with(DXBC_MAGIC) => {
                    if let Some(size) = container_len(bytes, at) {
                        let blob = DxbcBlob {
                            offset: at,
                            size,
                            bytes: &bytes[at..at + size],
                        };
                        self.record(&facts(&blob));
                        source.containers += 1;
                        at += size;
                        continue;
                    }
                    self.rejected_candidates += 1;
                }
                0x03 if bytes[at..at + 4] == SPIRV_MAGIC => {
                    if let Some(module) = spirv_module(bytes, at) {
                        self.record_spirv(&module);
                        source.spirv_modules += 1;
                        at += 4;
                        continue;
                    }
                }
                _ => {}
            }
            at += 1;
        }
        self.sources.push(source);
    }

    fn record(&mut self, facts: &BlobFacts) {
        let profile = Profile {
            kind: facts.kind,
            stage: facts.stage,
            model: facts.model,
        };
        let tally = self.profiles.entry(profile).or_default();
        tally.shaders += 1;
        tally.bytes += facts.bytes;
        tally.malformed += usize::from(facts.malformed);
        for tag in &facts.parts {
            *tally.parts.entry(*tag).or_default() += 1;
        }
        for bit in (0..64).filter(|bit| facts.features >> bit & 1 == 1) {
            *tally.features.entry(bit).or_default() += 1;
        }
        for (opcode, count) in &facts.opcodes {
            *tally.opcode_shaders.entry(*opcode).or_default() += 1;
            *tally.opcode_instructions.entry(*opcode).or_default() += u64::from(*count);
        }
    }

    fn record_spirv(&mut self, module: &SpirvFacts) {
        let tally = &mut self.spirv;
        tally.modules += 1;
        *tally.versions.entry(module.version).or_default() += 1;
        for capability in module.capabilities.iter().collect::<BTreeSet<_>>() {
            *tally.capabilities.entry(*capability).or_default() += 1;
        }
        for extension in module.extensions.iter().collect::<BTreeSet<_>>() {
            *tally.extensions.entry(extension.clone()).or_default() += 1;
        }
        if let Some(model) = module.memory_model {
            *tally.memory_models.entry(model).or_default() += 1;
        }
        for stage in module.entry_stages.iter().collect::<BTreeSet<_>>() {
            *tally.entry_stages.entry(*stage).or_default() += 1;
        }
    }

    pub fn shaders(&self) -> usize {
        self.profiles.values().map(|t| t.shaders).sum()
    }

    /// Every number the scan took, for the owner to hand back.
    pub fn to_json(&self) -> Value {
        let sources: Vec<Value> = self
            .sources
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "bytes": s.bytes,
                    "containers": s.containers,
                    "spirv_modules": s.spirv_modules,
                })
            })
            .collect();
        let profiles: Vec<Value> = self
            .profiles
            .iter()
            .map(|(profile, tally)| {
                let parts: Map<String, Value> = tally
                    .parts
                    .iter()
                    .map(|(tag, n)| (String::from_utf8_lossy(tag).into_owned(), json!(n)))
                    .collect();
                let features: Map<String, Value> = tally
                    .features
                    .iter()
                    .map(|(bit, n)| (feature_name(*bit), json!(n)))
                    .collect();
                let opcodes: Map<String, Value> = tally
                    .opcode_shaders
                    .iter()
                    .map(|(op, shaders)| {
                        (
                            format!("0x{op:02x}"),
                            json!({
                                "shaders": shaders,
                                "instructions": tally.opcode_instructions.get(op),
                            }),
                        )
                    })
                    .collect();
                json!({
                    "kind": profile.kind.as_str(),
                    "stage": profile.stage_text(),
                    "stage_code": profile.stage,
                    "model": profile.model_text(),
                    "shaders": tally.shaders,
                    "bytes": tally.bytes,
                    "malformed": tally.malformed,
                    "parts": parts,
                    "features": features,
                    "opcodes": opcodes,
                })
            })
            .collect();
        let spirv = &self.spirv;
        json!({
            "sources": sources,
            "rejected_candidates": self.rejected_candidates,
            "profiles": profiles,
            "spirv": {
                "modules": spirv.modules,
                "versions": spirv.versions.iter()
                    .map(|((major, minor), n)| (format!("{major}.{minor}"), json!(n)))
                    .collect::<Map<_, _>>(),
                "capabilities": spirv.capabilities.iter()
                    .map(|(c, n)| (c.to_string(), json!(n)))
                    .collect::<Map<_, _>>(),
                "extensions": spirv.extensions.iter()
                    .map(|(e, n)| (e.clone(), json!(n)))
                    .collect::<Map<_, _>>(),
                "memory_models": spirv.memory_models.iter()
                    .map(|((a, m), n)| (format!("addressing {a}, memory {m}"), json!(n)))
                    .collect::<Map<_, _>>(),
                "entry_stages": spirv.entry_stages.iter()
                    .map(|(s, n)| (s.to_string(), json!(n)))
                    .collect::<Map<_, _>>(),
            },
        })
    }

    /// The summary a person reads; the JSON holds the rest.
    pub fn report(&self) -> String {
        let mut out = String::new();
        let bytes: usize = self.sources.iter().map(|s| s.bytes).sum();
        out.push_str(&format!(
            "{} files, {:.1} MiB; {} shader containers, {} SPIR-V modules, {} `DXBC` candidates refused\n\n",
            self.sources.len(),
            bytes as f64 / (1024.0 * 1024.0),
            self.shaders(),
            self.spirv.modules,
            self.rejected_candidates,
        ));
        out.push_str("  kind     stage  model  shaders   MiB   malformed\n");
        for (profile, tally) in &self.profiles {
            out.push_str(&format!(
                "  {:<8} {:<6} {:<6} {:>7}  {:>5.1}  {:>5}\n",
                profile.kind.as_str(),
                profile.stage_text(),
                profile.model_text(),
                tally.shaders,
                tally.bytes as f64 / (1024.0 * 1024.0),
                tally.malformed,
            ));
        }
        let features: BTreeMap<u32, usize> =
            self.profiles
                .values()
                .fold(BTreeMap::new(), |mut all, tally| {
                    for (bit, n) in &tally.features {
                        *all.entry(*bit).or_default() += n;
                    }
                    all
                });
        if !features.is_empty() {
            out.push_str("\nfeature flags set (shaders):\n");
            for (bit, n) in features {
                out.push_str(&format!("  {:<52} {n}\n", feature_name(bit)));
            }
        }
        if self.spirv.modules > 0 {
            out.push_str("\nSPIR-V:\n");
            for ((major, minor), n) in &self.spirv.versions {
                out.push_str(&format!("  version {major}.{minor}: {n} modules\n"));
            }
            for (capability, n) in &self.spirv.capabilities {
                out.push_str(&format!("  capability {capability}: {n}\n"));
            }
            for (extension, n) in &self.spirv.extensions {
                out.push_str(&format!("  extension {extension}: {n}\n"));
            }
        }
        out
    }
}

/// Hand `each` every file of a file, a folder of loose files, or a game install, whichever `path` is, as its name and bytes.
pub fn for_each_source(
    path: &Path,
    target: Target,
    each: &mut dyn FnMut(&str, &[u8]),
) -> Result<(), InventoryError> {
    if !path.exists() {
        return Err(InventoryError::MissingInput(path.to_path_buf()));
    }
    if path.is_file() {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        each(&name, &std::fs::read(path)?);
    } else if path.join("common.rpf").exists() {
        scan_install(path, target, each)?;
    } else {
        for file in walk(path)? {
            let bytes = std::fs::read(&file)?;
            let name = file
                .strip_prefix(path)
                .unwrap_or(&file)
                .display()
                .to_string();
            each(&name, &bytes);
        }
    }
    Ok(())
}

/// Scan a file, a directory of loose files, or a game install, whichever `path` is.
pub fn scan_path(path: &Path, target: Target) -> Result<Inventory, InventoryError> {
    let mut inventory = Inventory::default();
    for_each_source(path, target, &mut |name, bytes| inventory.scan(name, bytes))?;
    Ok(inventory)
}

/// Every container in `bytes`, found wherever it sits and skipped whole.
pub fn containers(bytes: &[u8]) -> Vec<&[u8]> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 4 <= bytes.len() {
        if bytes[at] == b'D' && bytes[at..].starts_with(DXBC_MAGIC) {
            if let Some(size) = container_len(bytes, at) {
                found.push(&bytes[at..at + size]);
                at += size;
                continue;
            }
        }
        at += 1;
    }
    found
}

fn scan_install(
    root: &Path,
    target: Target,
    each: &mut dyn FnMut(&str, &[u8]),
) -> Result<(), InventoryError> {
    let crypto = install_crypto(root)?;
    let install = GtaInstall {
        root: root.to_path_buf(),
    };
    let mut failures = Vec::new();
    let mut found = false;
    if target != Target::Gen9 {
        match extract_entries_from_install(&install, Arc::clone(&crypto), DEFAULT_SHADER_DIR, "fxc")
        {
            Ok(entries) => {
                for (stem, bytes) in entries {
                    each(&format!("{DEFAULT_SHADER_DIR}/{stem}.fxc"), &bytes);
                }
                found = true;
            }
            Err(error) => failures.push(error.to_string()),
        }
        for folder in LEGACY_VARIANT_DIRS {
            if let Ok(entries) =
                extract_entries_from_install(&install, Arc::clone(&crypto), folder, "fxc")
            {
                for (stem, bytes) in entries {
                    each(&format!("{folder}/{stem}.fxc"), &bytes);
                }
            }
        }
    }
    if target != Target::Gen8 {
        match extract_sga_containers(&install, crypto) {
            Ok(files) => {
                for (name, bytes) in files {
                    each(&name, &bytes);
                }
                found = true;
            }
            Err(error) => failures.push(error.to_string()),
        }
    }
    if found {
        Ok(())
    } else {
        Err(InventoryError::NoContainers(failures.join("; ")))
    }
}
