//! The manifest of a SPIR-V shader pack, version 3: content-addressed modules, what each binds, and which program each pass of each technique runs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The stage a shader program runs at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShaderStageKind {
    Vertex,
    Pixel,
    Geometry,
    Hull,
    Domain,
    Compute,
    Unknown,
}

#[cfg(feature = "fxc")]
impl From<super::fxc::ShaderStage> for ShaderStageKind {
    fn from(s: super::fxc::ShaderStage) -> Self {
        match s {
            super::fxc::ShaderStage::Vertex => Self::Vertex,
            super::fxc::ShaderStage::Pixel => Self::Pixel,
            super::fxc::ShaderStage::Geometry => Self::Geometry,
            super::fxc::ShaderStage::Hull => Self::Hull,
            super::fxc::ShaderStage::Domain => Self::Domain,
            super::fxc::ShaderStage::Compute => Self::Compute,
            super::fxc::ShaderStage::Unknown => Self::Unknown,
        }
    }
}

impl ShaderStageKind {
    pub fn short_code(self) -> &'static str {
        match self {
            Self::Vertex => "vs",
            Self::Pixel => "ps",
            Self::Geometry => "gs",
            Self::Hull => "hs",
            Self::Domain => "ds",
            Self::Compute => "cs",
            Self::Unknown => "xx",
        }
    }
}

pub const PACK_VERSION: u32 = 4;

/// The file name of the manifest inside a pack folder.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Where the module of the program whose source container hashes to `key` lives, relative to the pack folder.
pub fn module_path(key: &str) -> String {
    let shard = key.get(..2).unwrap_or("00");
    format!("spirv/{shard}/{key}.spv")
}

/// What wrote the modules: a pack made by a different translator or legalizer is not reused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Production {
    pub translator: String,
    pub legalizer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpirvPack {
    pub schema_version: u32,
    pub produced_by: Production,
    /// The title's build the sources came from.
    #[serde(default)]
    pub source_build_hash: String,
    /// By the SHA-1 of the source container, so a program shared by many effects is stored once.
    pub programs: BTreeMap<String, Program>,
    /// The constant buffer layouts the programs reflect, by a hash of the layout: a retail install has a thousand where its programs name twenty thousand.
    #[serde(default)]
    pub constant_buffers: BTreeMap<String, ConstantBuffer>,
    pub effects: Vec<Effect>,
}

impl SpirvPack {
    pub fn new(produced_by: Production) -> Self {
        Self {
            schema_version: PACK_VERSION,
            produced_by,
            source_build_hash: String::new(),
            programs: BTreeMap::new(),
            constant_buffers: BTreeMap::new(),
            effects: Vec::new(),
        }
    }

    /// The program a pass runs at `stage`, if the pass binds one.
    pub fn program_of(&self, pass: &Pass, stage: ShaderStageKind) -> Option<&Program> {
        self.programs.get(pass.stages.get(&stage)?)
    }

    /// The constant buffers a program reflects.
    pub fn constant_buffers_of(&self, program: &Program) -> Vec<&ConstantBuffer> {
        program
            .reflection
            .constant_buffers
            .iter()
            .filter_map(|key| self.constant_buffers.get(key))
            .collect()
    }

    pub fn translated(&self) -> usize {
        self.programs
            .values()
            .filter(|p| matches!(p.outcome, Outcome::Translated { .. }))
            .count()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub name: String,
    pub source_path: String,
    pub vertex_type: u32,
    pub techniques: Vec<Technique>,
    /// The value the effect declares for each constant, by its variable's name, as 32-bit words (a float's bits): what a constant holds until the engine or a material sets it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<String, Vec<u32>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Technique {
    pub name: String,
    pub passes: Vec<Pass>,
}

/// The programs one pass runs, as keys of [`SpirvPack::programs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pass {
    pub stages: BTreeMap<ShaderStageKind, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    pub stage: ShaderStageKind,
    pub source_bytes: u32,
    pub outcome: Outcome,
    /// What the source container's own reflection says, copied unchanged.
    #[serde(default)]
    pub reflection: Reflection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// The module is at [`module_path`] of the program's key.
    Translated {
        spirv_bytes: u32,
        fidelity: Fidelity,
        /// The legalizer's rewrites that changed the module, by name and count.
        lowerings: BTreeMap<String, u32>,
        naga: NagaReach,
    },
    /// The translator or the legalizer refused the source.
    Refused { reason: String },
    /// Set aside before translation, such as DXIL.
    Skipped { reason: String },
}

/// How a module compares with its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    /// A compile of the source's instructions, equal up to what a GPU does differently for the game itself.
    Equivalent,
    /// A lowering changed what the module does.
    Approximated,
}

/// The last step naga's reader, validator and Metal writer got the module through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NagaReach {
    pub reached: Reach,
    /// Why it stopped, where it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    Refused,
    Parsed,
    Validated,
    MslWritten,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reflection {
    pub bindings: Vec<Binding>,
    /// Keys of [`SpirvPack::constant_buffers`].
    pub constant_buffers: Vec<String>,
    pub inputs: Vec<SignatureElement>,
    pub outputs: Vec<SignatureElement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingClass {
    Cbv,
    Sampler,
    Srv,
    Uav,
    UavCounter,
}

/// One descriptor of the module, and the register it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub name: String,
    /// The reflection's own word for the kind, such as `texture` or `cbuffer`.
    pub kind: String,
    pub class: BindingClass,
    pub register: u32,
    pub set: u32,
    pub binding: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstantBuffer {
    pub name: String,
    pub size: u32,
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub used: bool,
    /// The scalar the variable is made of, as the reflection names it.
    pub scalar: String,
    pub rows: u16,
    pub columns: u16,
    pub elements: u16,
}

/// One element of an input or output signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureElement {
    pub semantic: String,
    pub index: u32,
    pub system_value: u32,
    pub component_type: u32,
    pub register: u32,
    pub mask: u8,
}

#[cfg(test)]
mod tests;
