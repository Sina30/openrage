//! What a material's parameters *are*: the shader definition database, and the loader for one.

pub mod coverage;
pub mod diff;
pub mod materials;
pub mod remap;

#[cfg(feature = "fxc")]
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

pub use coverage::{Coverage, Residue};
pub use diff::{Change, DiffCounts, ParamDelta, ShaderDefsDiff, ShaderDelta};
pub use materials::{MaterialEntry, MaterialRef};
pub use remap::{ParamRemap, Spelling};

#[cfg(feature = "fxc")]
use crate::gta5::shaders::fxc::{
    GrcAnnotationValue, GrcEffect, GrcEffectVariable, GrcVariableType,
};
#[cfg(feature = "fxc")]
use crate::gta5::shaders::sga::{SgaEffect, SgaVariable, SgaVariableType};
#[cfg(feature = "fxc")]
use crate::gta5::shaders::sps::{SpsPreset, DRAW_BUCKET_PRESET};

pub const SHADER_DEFS_SCHEMA: u32 = 2;

/// What kind of thing a declared parameter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShaderParamKind {
    Float,
    Float2,
    Float3,
    Float4,
    Float3x4,
    Float4x4,
    Int,
    Int4,
    Bool,
    Texture,
    Buffer,
    UavBuffer,
    UavTexture,
    /// An Enhanced sampler, which a Legacy effect declares as a texture.
    Sampler,
    /// A type code no release has explained, kept so a value is never renamed
    /// into the nearest variant that happens to exist.
    Unknown(u8),
}

impl ShaderParamKind {
    #[cfg(feature = "fxc")]
    fn of(t: GrcVariableType) -> Self {
        match t {
            GrcVariableType::Float => Self::Float,
            GrcVariableType::Float2 => Self::Float2,
            GrcVariableType::Float3 => Self::Float3,
            GrcVariableType::Float4 => Self::Float4,
            GrcVariableType::Float3x4 => Self::Float3x4,
            GrcVariableType::Float4x4 => Self::Float4x4,
            GrcVariableType::Int => Self::Int,
            GrcVariableType::Int4 => Self::Int4,
            GrcVariableType::Bool => Self::Bool,
            GrcVariableType::Texture => Self::Texture,
            GrcVariableType::Buffer => Self::Buffer,
            GrcVariableType::UavBuffer => Self::UavBuffer,
            GrcVariableType::UavTexture => Self::UavTexture,
            GrcVariableType::Unknown(c) => Self::Unknown(c),
        }
    }

    /// A binding rather than a constant-buffer member, which is the same line
    /// `GrmShader` draws when it reads `data_type == 0`.
    pub fn is_binding(self) -> bool {
        matches!(
            self,
            Self::Texture | Self::Buffer | Self::UavBuffer | Self::UavTexture | Self::Sampler
        )
    }

    /// Float components one value occupies, or `None` for a binding.
    ///
    /// ⚠ **Type codes 17–20 are not spare and must not be folded into 2–5.**
    /// CodeWalker names them `Unused1`–`Unused4`; measured over the 27,916
    /// parameters a retail Legacy install declares, 1,734 of them use one,
    /// and every single one carries a default run exactly as wide as the code
    /// fifteen below it — 17 holds 0 or 1 float, 18 holds 2, 19 holds 3, 20
    /// holds 0 or 4. The width is measured; what the distinction *means* is
    /// not, so the code is kept rather than renamed into the nearest variant.
    pub fn components(self) -> Option<usize> {
        Some(match self {
            Self::Float => 1,
            Self::Float2 => 2,
            Self::Float3 => 3,
            Self::Float4 | Self::Int4 => 4,
            Self::Int | Self::Bool => 1,
            Self::Float3x4 => 12,
            Self::Float4x4 => 16,
            Self::Unknown(17) => 1,
            Self::Unknown(18) => 2,
            Self::Unknown(19) => 3,
            Self::Unknown(20) => 4,
            Self::Unknown(_) | Self::Texture | Self::Buffer => return None,
            Self::UavBuffer | Self::UavTexture | Self::Sampler => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Float => "float",
            Self::Float2 => "float2",
            Self::Float3 => "float3",
            Self::Float4 => "float4",
            Self::Float3x4 => "float3x4",
            Self::Float4x4 => "float4x4",
            Self::Int => "int",
            Self::Int4 => "int4",
            Self::Bool => "bool",
            Self::Texture => "texture",
            Self::Buffer => "buffer",
            Self::UavBuffer => "rwbuffer",
            Self::UavTexture => "rwtexture",
            Self::Sampler => "sampler",
            Self::Unknown(_) => "unknown",
        }
    }
}

/// A value the file states for a parameter.
///
/// ⚠ **Tagged, and it has to be.** The two arms serialize to the same JSON
/// when the integers happen to be small, so an untagged enum reads
/// `[0, 3, 1, 3]` — the sampler-state run behind `FogRaySampler` — back as
/// floats, and a database that survived one save/load cycle disagreed with
/// the one that built it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShaderParamValue {
    Floats(Vec<f32>),
    Uints(Vec<u32>),
}

impl ShaderParamValue {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Floats(v) => v.is_empty(),
            Self::Uints(v) => v.is_empty(),
        }
    }

    /// The value as the float4 registers a material's entry data actually is,
    /// or `None` where the translation is not known.
    ///
    /// ⚠ **An integer default has no known register encoding and must not be
    /// guessed.** A constant register is four bytes and the database does not
    /// say whether the shader reads them with `asint`; writing `3.0` where the
    /// shader wants the bit pattern `3` is a silently wrong value. 1,049 of a
    /// retail Legacy install's 24,840 non-binding parameters declare an
    /// integer default — every one of kind `int`, `int4` or `bool` — so this
    /// refuses for them rather than inventing a conversion.
    pub fn as_float_registers(&self) -> Option<Vec<[f32; 4]>> {
        let Self::Floats(floats) = self else {
            return None;
        };
        if floats.is_empty() {
            return None;
        }
        Some(
            floats
                .chunks(4)
                .map(|c| {
                    let mut reg = [0.0f32; 4];
                    reg[..c.len()].copy_from_slice(c);
                    reg
                })
                .collect(),
        )
    }
}

/// One declared parameter of one shader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderParamDef {
    pub name: String,
    /// `atStringHash(name)` — the key a `GrmShader` entry carries instead of the name.
    pub name_hash: u32,
    pub kind: ShaderParamKind,
    /// Array length in elements; 1 for a scalar.
    #[serde(default = "one")]
    pub array_len: u16,
    pub register: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cbuffer: Option<String>,
    /// Where a GTA VI drawable's record places this constant: its buffer's rank among the effect's buffers a drawable holds, and its byte offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawable_place: Option<(u8, u16)>,
    /// The shader-side spelling, recorded only where it differs from `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<String>,
    /// `atStringHash(semantic)`, and **the key a file is more likely to carry
    /// than [`Self::name_hash`]**.
    ///
    /// ⚠ A `.ypt` binds `WrapLightingTerm`; `ptfx_sprite` declares the variable
    /// as `wrapLigthtingTerm`, typo included, and states `WrapLightingTerm` as
    /// its semantic. Measured over the staged corpus: of 26 parameter hashes a
    /// file bound that a name-keyed lookup could not resolve, 20 were semantics
    /// this database already held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_hash: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_name: Option<String>,
    /// What the effect declares, before any preset touches it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<ShaderParamValue>,
    /// What the preset pins over that. ⚠ Distinct from `default` on purpose:
    /// "reset to the effect's value" and "reset to this material's preset" are
    /// different commands and a database that merged them could offer only one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_default: Option<ShaderParamValue>,
}

fn one() -> u16 {
    1
}

impl ShaderParamDef {
    pub fn is_binding(&self) -> bool {
        self.kind.is_binding()
    }

    /// The register count an entry for this parameter occupies.
    ///
    /// A scalar still costs a whole float4 register, which is why this is a
    /// ceiling rather than a component count.
    pub fn register_span(&self) -> u8 {
        if self.is_binding() {
            return 0;
        }
        let components = self.kind.components().unwrap_or(4) * self.array_len.max(1) as usize;
        components.div_ceil(4).clamp(1, u8::MAX as usize) as u8
    }

    /// The registers a fresh entry starts with: the preset's pin, else the
    /// effect's declaration, else zeros.
    ///
    /// ⚠ **Zeros where the declared default is an integer**, for the reason
    /// [`ShaderParamValue::as_float_registers`] states.
    pub fn initial_registers(&self) -> Vec<[f32; 4]> {
        let span = self.register_span() as usize;
        let mut regs = self
            .preset_default
            .as_ref()
            .and_then(ShaderParamValue::as_float_registers)
            .or_else(|| {
                self.default
                    .as_ref()
                    .and_then(ShaderParamValue::as_float_registers)
            })
            .unwrap_or_default();
        regs.resize(span, [0.0; 4]);
        regs
    }
}

/// A named constant the preset system pins before binding: a trait flag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderTrait {
    pub name: String,
    pub value: u32,
}

/// One shader as a material names it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderDef {
    /// The preset's name — what a `.ydr` stores and hashes.
    pub name: String,
    pub name_hash: u32,
    /// The effect the preset binds, which is often spelled differently.
    pub effect: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_bucket: Option<u32>,
    pub vertex_type: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<ShaderTrait>,
    pub parameters: Vec<ShaderParamDef>,
}

impl ShaderDef {
    /// The parameter a file's entry hash names, by declared name or by semantic.
    ///
    /// ⚠ **The name is tried first and that ordering is load-bearing.** In 385
    /// of a retail Legacy install's shaders one parameter's semantic is
    /// another parameter's name — `gCSMShadowTextureSamp` states the semantic
    /// `gCSMShadowTexture`, and `gCSMShadowTexture` is also a variable — so a
    /// semantic-first lookup would answer with the sampler. No shader declares
    /// two parameters with the same semantic, so the fallback is unambiguous.
    pub fn parameter(&self, name_hash: u32) -> Option<&ShaderParamDef> {
        self.parameters
            .iter()
            .find(|p| p.name_hash == name_hash)
            .or_else(|| {
                self.parameters
                    .iter()
                    .find(|p| p.semantic_hash == Some(name_hash))
            })
    }

    pub fn bindings(&self) -> impl Iterator<Item = &ShaderParamDef> {
        self.parameters.iter().filter(|p| p.is_binding())
    }
}

/// Every shader one title's one target declares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderDefinitions {
    pub schema_version: u32,
    /// The title these came from, spelled as its `GameId` is.
    pub title: String,
    /// `gen8` / `gen9`.
    pub target: String,
    #[serde(default)]
    pub source_build: String,
    pub shaders: Vec<ShaderDef>,
}

impl ShaderDefinitions {
    pub fn new(title: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            schema_version: SHADER_DEFS_SCHEMA,
            title: title.into(),
            target: target.into(),
            source_build: String::new(),
            shaders: Vec::new(),
        }
    }

    pub fn shader(&self, name_hash: u32) -> Option<&ShaderDef> {
        self.shaders.iter().find(|s| s.name_hash == name_hash)
    }

    pub fn shader_named(&self, name: &str) -> Option<&ShaderDef> {
        let hash = crate::string_hash(name.as_bytes());
        self.shader(hash)
    }

    pub fn parameter_count(&self) -> usize {
        self.shaders.iter().map(|s| s.parameters.len()).sum()
    }

    /// Name a material's entries — the question the whole database exists to answer.
    pub fn name_material(&self, shader_hash: u32, entries: &[u32]) -> MaterialNaming {
        let Some(shader) = self.shader(shader_hash) else {
            return MaterialNaming {
                shader: None,
                named: Vec::new(),
                unnamed: entries.to_vec(),
            };
        };
        let mut named = Vec::new();
        let mut unnamed = Vec::new();
        for hash in entries {
            match shader.parameter(*hash) {
                Some(def) => named.push((*hash, def.clone())),
                None => unnamed.push(*hash),
            }
        }
        MaterialNaming {
            shader: Some(shader.name.clone()),
            named,
            unnamed,
        }
    }

    /// ⚠ **A database from an older schema is refused rather than read.**
    /// Schema 2 added `semantic_hash`, and a schema-1 file deserializes
    /// cleanly with every one of them absent — which does not fail, it just
    /// silently stops naming the 9,712 parameters a file addresses by
    /// semantic.
    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        let parsed: Self = serde_json::from_str(text)?;
        if parsed.schema_version != SHADER_DEFS_SCHEMA {
            return Err(serde::de::Error::custom(format!(
                "shader definition database is schema {} and this build reads {}; \
                 rebuild it with `or-shader-tool defs build`",
                parsed.schema_version, SHADER_DEFS_SCHEMA
            )));
        }
        Ok(parsed)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Assemble from the two files a title states this in: an effect per
    /// `.fxc`, and a preset per `.sps` naming one.
    ///
    /// ⚠ **A preset with no effect is dropped rather than carried empty.** An
    /// entry whose parameter list is silently empty reads to a caller as "this
    /// shader has no parameters", which is the one answer the database must
    /// never give when it simply did not find the effect.
    #[cfg(feature = "fxc")]
    pub fn assemble(
        title: impl Into<String>,
        target: impl Into<String>,
        effects: &[(String, GrcEffect)],
        presets: &[SpsPreset],
    ) -> (Self, Vec<UnresolvedPreset>) {
        let effects: Vec<(String, Declared)> = (effects.iter())
            .map(|(name, effect)| (name.clone(), declared(effect)))
            .collect();
        Self::assemble_declared(title, target, &effects, presets)
    }

    /// Assemble from Enhanced's shader container and the `.sps` presets it ships.
    #[cfg(feature = "fxc")]
    pub fn assemble_sga(
        title: impl Into<String>,
        target: impl Into<String>,
        effects: &[SgaEffect],
        presets: &[SpsPreset],
    ) -> (Self, Vec<UnresolvedPreset>) {
        let effects: Vec<(String, Declared)> = (effects.iter())
            .map(|effect| (effect.name.clone(), declared_sga(effect)))
            .collect();
        Self::assemble_declared(title, target, &effects, presets)
    }

    #[cfg(feature = "fxc")]
    fn assemble_declared(
        title: impl Into<String>,
        target: impl Into<String>,
        effects: &[(String, Declared)],
        presets: &[SpsPreset],
    ) -> (Self, Vec<UnresolvedPreset>) {
        let by_name: HashMap<&str, &Declared> =
            effects.iter().map(|(n, e)| (n.as_str(), e)).collect();

        let mut out = Self::new(title, target);
        let mut unresolved = Vec::new();

        for preset in presets {
            let Some(effect) = by_name.get(preset.effect.as_str()) else {
                unresolved.push(UnresolvedPreset {
                    preset: preset.name.clone(),
                    effect: preset.effect.clone(),
                });
                continue;
            };
            out.shaders.push(define(preset, effect));
        }

        // An effect no preset names is still a name a file can carry, so it is
        // defined against a preset that pins nothing.
        for (name, effect) in effects {
            if out.shader_named(name).is_some() {
                continue;
            }
            out.shaders.push(define(
                &SpsPreset {
                    name: name.clone(),
                    effect: name.clone(),
                    overrides: Vec::new(),
                },
                effect,
            ));
        }

        out.shaders.sort_by(|a, b| a.name.cmp(&b.name));
        (out, unresolved)
    }
}

/// What the database could and could not say about one material.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialNaming {
    /// `None` when the database does not describe this shader at all, which is
    /// a different answer from describing it and not recognising a parameter.
    pub shader: Option<String>,
    pub named: Vec<(u32, ShaderParamDef)>,
    pub unnamed: Vec<u32>,
}

impl MaterialNaming {
    pub fn is_complete(&self) -> bool {
        self.shader.is_some() && self.unnamed.is_empty()
    }
}

/// A preset naming an effect the database was not given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedPreset {
    pub preset: String,
    pub effect: String,
}

/// One effect's declarations, before a preset pins anything over them.
#[cfg(feature = "fxc")]
struct Declared {
    vertex_type: u32,
    draw_bucket: Option<u32>,
    traits: Vec<ShaderTrait>,
    parameters: Vec<ShaderParamDef>,
}

#[cfg(feature = "fxc")]
fn define(preset: &SpsPreset, effect: &Declared) -> ShaderDef {
    let mut traits = effect.traits.clone();
    for over in &preset.overrides {
        if over.name.eq_ignore_ascii_case(DRAW_BUCKET_PRESET) {
            continue;
        }
        if effect.parameters.iter().any(|p| p.name == over.name) {
            continue;
        }
        if let Some(value) = over.values.first().and_then(|v| v.as_u32()) {
            traits.push(ShaderTrait {
                name: over.name.clone(),
                value,
            });
        }
    }

    let parameters = (effect.parameters.iter())
        .map(|p| ShaderParamDef {
            preset_default: preset.get(&p.name).map(|o| {
                if matches!(p.kind, ShaderParamKind::Int | ShaderParamKind::Bool) {
                    ShaderParamValue::Uints(o.values.iter().filter_map(|v| v.as_u32()).collect())
                } else {
                    ShaderParamValue::Floats(o.values.iter().map(|x| x.as_f32()).collect())
                }
            }),
            ..p.clone()
        })
        .collect();

    ShaderDef {
        name_hash: crate::string_hash(preset.name.as_bytes()),
        name: preset.name.clone(),
        effect: preset.effect.clone(),
        draw_bucket: preset.draw_bucket().or(effect.draw_bucket),
        vertex_type: effect.vertex_type,
        traits,
        parameters,
    }
}

#[cfg(feature = "fxc")]
fn declared(effect: &GrcEffect) -> Declared {
    Declared {
        vertex_type: effect.vertex_type,
        draw_bucket: effect.draw_bucket(),
        traits: (effect.presets.iter())
            .filter(|p| !p.name.eq_ignore_ascii_case(DRAW_BUCKET_PRESET))
            .map(|p| ShaderTrait {
                name: p.name.clone(),
                value: p.value,
            })
            .collect(),
        parameters: (effect.variables.iter())
            .map(|v| parameter(v, effect))
            .collect(),
    }
}

#[cfg(feature = "fxc")]
fn parameter(v: &GrcEffectVariable, effect: &GrcEffect) -> ShaderParamDef {
    let kind = ShaderParamKind::of(v.var_type);

    let default = if !v.default_floats.is_empty() {
        Some(ShaderParamValue::Floats(v.default_floats.clone()))
    } else if !v.default_uints.is_empty() {
        Some(ShaderParamValue::Uints(v.default_uints.clone()))
    } else {
        None
    };

    let cbuffer = effect
        .cbuffers
        .iter()
        .find(|c| crate::string_hash(c.name.as_bytes()) == v.cbuffer)
        .map(|c| c.name.clone());

    let ui_name = v.annotations.iter().find_map(|a| match &a.value {
        GrcAnnotationValue::String(s) if a.name.eq_ignore_ascii_case("UIName") => Some(s.clone()),
        _ => None,
    });

    let semantic = (!v.semantic.is_empty() && v.semantic != v.name).then(|| v.semantic.clone());

    ShaderParamDef {
        name_hash: crate::string_hash(v.name.as_bytes()),
        name: v.name.clone(),
        kind,
        array_len: v.count.max(1).into(),
        register: v.slot,
        cbuffer,
        drawable_place: None,
        semantic_hash: semantic
            .as_deref()
            .map(|s| crate::string_hash(s.as_bytes())),
        semantic,
        ui_name,
        default,
        preset_default: None,
    }
}

/// ⚠ The container states no vertex type and no draw bucket for an effect,
/// and keys its annotations by a hash this tree cannot compute, so an Enhanced
/// entry carries vertex type 0, a bucket only from its preset, and no `UIName`.
#[cfg(feature = "fxc")]
fn declared_sga(effect: &SgaEffect) -> Declared {
    Declared {
        vertex_type: 0,
        draw_bucket: None,
        traits: Vec::new(),
        parameters: effect.variables.iter().map(sga_parameter).collect(),
    }
}

#[cfg(feature = "fxc")]
pub(crate) fn sga_parameter(v: &SgaVariable) -> ShaderParamDef {
    let kind = match v.var_type {
        SgaVariableType::Bool => ShaderParamKind::Bool,
        SgaVariableType::Int => ShaderParamKind::Int,
        SgaVariableType::Float => ShaderParamKind::Float,
        SgaVariableType::Float2 => ShaderParamKind::Float2,
        SgaVariableType::Float3 => ShaderParamKind::Float3,
        SgaVariableType::Float4 => ShaderParamKind::Float4,
        SgaVariableType::Float3x4 => ShaderParamKind::Float3x4,
        SgaVariableType::Float4x4 => ShaderParamKind::Float4x4,
        SgaVariableType::Texture => ShaderParamKind::Texture,
        SgaVariableType::UavTexture => ShaderParamKind::UavTexture,
        SgaVariableType::Buffer => ShaderParamKind::Buffer,
        SgaVariableType::UavBuffer => ShaderParamKind::UavBuffer,
        SgaVariableType::Sampler => ShaderParamKind::Sampler,
        SgaVariableType::OtherConstant(c) | SgaVariableType::OtherResource(c) => {
            ShaderParamKind::Unknown(c)
        }
    };
    let default = (!v.default.is_empty()).then(|| {
        if matches!(kind, ShaderParamKind::Int | ShaderParamKind::Bool) {
            ShaderParamValue::Uints(v.default.clone())
        } else {
            ShaderParamValue::Floats(v.default.iter().map(|w| f32::from_bits(*w)).collect())
        }
    });
    // A constant's register is the 16-byte row its byte offset falls in.
    let register = if v.var_type.is_resource() {
        v.slot
    } else {
        v.slot / 16
    };
    let semantic = v.semantic.clone().filter(|s| *s != v.name);
    ShaderParamDef {
        name_hash: crate::string_hash(v.name.as_bytes()),
        name: v.name.clone(),
        kind,
        array_len: v.count.max(1),
        register: u8::try_from(register).unwrap_or(u8::MAX),
        cbuffer: v.cbuffer.clone(),
        drawable_place: None,
        semantic_hash: semantic
            .as_deref()
            .map(|s| crate::string_hash(s.as_bytes())),
        semantic,
        ui_name: None,
        default,
        preset_default: None,
    }
}

/// The hash a shader or a parameter is keyed by, which is `atStringHash` and
/// therefore case-insensitive.
pub fn hash_name(name: &str) -> u32 {
    crate::string_hash(name.as_bytes())
}

/// Where a built database is staged. ⚠ **It is never committed**: it is
/// derived from a game install, so it is the same kind of thing a corpus is.
pub const SHADER_DEFS_ENV: &str = "OPENRAGE_SHADER_DEFS";

pub fn load_from(path: &std::path::Path) -> std::io::Result<ShaderDefinitions> {
    let text = std::fs::read_to_string(path)?;
    ShaderDefinitions::from_json(&text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// The database [`SHADER_DEFS_ENV`] names, if it names a readable one.
pub fn staged() -> Option<ShaderDefinitions> {
    let raw = std::env::var_os(SHADER_DEFS_ENV)?;
    if raw.is_empty() {
        return None;
    }
    load_from(std::path::Path::new(&raw)).ok()
}

type Installed = Arc<Option<ShaderDefinitions>>;

fn slot() -> &'static RwLock<Installed> {
    static SLOT: OnceLock<RwLock<Installed>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(Arc::new(None)))
}

/// Make a database the one this process answers from.
pub fn install(defs: ShaderDefinitions) {
    if let Ok(mut s) = slot().write() {
        *s = Arc::new(Some(defs));
    }
}

pub fn clear() {
    if let Ok(mut s) = slot().write() {
        *s = Arc::new(None);
    }
}

/// Whether this process has a database at all.
///
/// ⚠ **Distinct from [`describe`] returning `None`**, and a caller that cannot
/// tell them apart reports "nothing knows this parameter" when the truth is
/// "nothing was asked". The particle preview's warning depends on the
/// difference.
pub fn is_installed() -> bool {
    slot().read().map(|s| s.as_ref().is_some()).unwrap_or(false)
}

/// Whether the installed database describes this shader.
pub fn describes_shader(shader_hash: u32) -> bool {
    slot()
        .read()
        .ok()
        .and_then(|s| s.as_ref().as_ref().map(|d| d.shader(shader_hash).is_some()))
        .unwrap_or(false)
}

/// Whether the installed database says this shader declares this parameter.
///
/// The predicate rather than [`describe`] where only the answer is wanted:
/// this allocates nothing, and the particle preview asks it per bound
/// variable.
pub fn declares(shader_hash: u32, param_hash: u32) -> bool {
    slot()
        .read()
        .ok()
        .and_then(|s| {
            s.as_ref().as_ref().map(
                |d| matches!(d.shader(shader_hash), Some(sh) if sh.parameter(param_hash).is_some()),
            )
        })
        .unwrap_or(false)
}

/// What a material's entry is, resolved through the installed database.
pub fn describe(shader_hash: u32, param_hash: u32) -> Option<ShaderParamDef> {
    let snapshot = slot().read().ok()?.clone();
    let defs = snapshot.as_ref().as_ref()?;
    defs.shader(shader_hash)?.parameter(param_hash).cloned()
}

/// `f` of the installed database; `None` with none installed.
pub fn with_installed<R>(f: impl FnOnce(&ShaderDefinitions) -> R) -> Option<R> {
    let snapshot = slot().read().ok()?.clone();
    snapshot.as_ref().as_ref().map(f)
}

/// Every parameter the installed database declares for a shader, in file order.
pub fn parameters_of(shader_hash: u32) -> Option<Vec<ShaderParamDef>> {
    let snapshot = slot().read().ok()?.clone();
    let defs = snapshot.as_ref().as_ref()?;
    Some(defs.shader(shader_hash)?.parameters.clone())
}

#[cfg(all(test, feature = "fxc"))]
mod tests {
    use super::*;
    use crate::gta5::shaders::fxc::{GrcEffectPreset, GrcVariableAnnotation};

    /// ⚠ The installed database is process-wide, so two tests that install
    /// and clear it cannot run at the same time.
    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
        GUARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn variable(name: &str, t: GrcVariableType) -> GrcEffectVariable {
        GrcEffectVariable {
            var_type: t,
            count: 1,
            slot: 3,
            group: 0,
            name: name.to_string(),
            semantic: name.to_string(),
            offset: 0,
            variant: 0,
            cbuffer: 0,
            annotations: Vec::new(),
            default_floats: Vec::new(),
            default_uints: Vec::new(),
        }
    }

    fn effect(vars: Vec<GrcEffectVariable>) -> GrcEffect {
        GrcEffect {
            vertex_type: 0x59,
            presets: Vec::new(),
            programs: Vec::new(),
            cbuffers: Vec::new(),
            variables: vars,
            techniques: Vec::new(),
        }
    }

    #[test]
    fn a_parameter_is_keyed_by_the_hash_a_material_carries() {
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[(
                "default".into(),
                effect(vec![variable("Specular", GrcVariableType::Float)]),
            )],
            &[SpsPreset::parse("default", "shader default")],
        );
        let shader = defs.shader_named("default").expect("a default shader");
        let hash = crate::string_hash(b"Specular");
        assert_eq!(
            shader.parameter(hash).map(|p| p.name.as_str()),
            Some("Specular")
        );
        assert_eq!(
            shader.parameter(hash).map(|p| p.kind),
            Some(ShaderParamKind::Float)
        );
    }

    /// ⚠ A preset and the effect it binds are different names, and the
    /// database is keyed by the preset because that is what the file stores.
    #[test]
    fn two_presets_over_one_effect_are_two_shaders() {
        let effects = vec![(
            "default".into(),
            effect(vec![variable("DiffuseSampler", GrcVariableType::Texture)]),
        )];
        let presets = vec![
            SpsPreset::parse("default", "shader default"),
            SpsPreset::parse(
                "cutout",
                "shader default\n__rage_drawbucket {\n\tint 3\n}\n",
            ),
        ];
        let (defs, unresolved) = ShaderDefinitions::assemble("gta5", "gen8", &effects, &presets);

        assert!(unresolved.is_empty());
        assert_eq!(defs.shaders.len(), 2);
        assert_eq!(defs.shader_named("default").unwrap().draw_bucket, None);
        assert_eq!(defs.shader_named("cutout").unwrap().draw_bucket, Some(3));
        assert_eq!(
            defs.shader_named("cutout").unwrap().bindings().count(),
            1,
            "a preset inherits the effect's parameters"
        );
    }

    #[test]
    fn a_preset_naming_an_effect_we_were_not_given_is_reported_not_emptied() {
        let (defs, unresolved) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[],
            &[SpsPreset::parse("cutout", "shader default")],
        );
        assert!(defs.shaders.is_empty());
        assert_eq!(
            unresolved,
            vec![UnresolvedPreset {
                preset: "cutout".into(),
                effect: "default".into()
            }]
        );
    }

    #[test]
    fn an_effect_no_preset_names_is_still_a_shader() {
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[(
                "orphan".into(),
                effect(vec![variable("X", GrcVariableType::Float4)]),
            )],
            &[],
        );
        assert_eq!(defs.shaders.len(), 1);
        assert_eq!(defs.shaders[0].name, "orphan");
    }

    /// ⚠ Resetting to the effect's value and resetting to the preset's need two
    /// values, so the preset's override must not overwrite the effect's
    /// declaration.
    #[test]
    fn a_preset_override_sits_beside_the_effect_default_rather_than_replacing_it() {
        let mut v = variable("Specular", GrcVariableType::Float);
        v.default_floats = vec![40.0];
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[("default".into(), effect(vec![v]))],
            &[SpsPreset::parse(
                "shiny",
                "shader default\nSpecular {\n\tfloat 180\n}\n",
            )],
        );
        let p = &defs.shader_named("shiny").unwrap().parameters[0];
        assert_eq!(p.default, Some(ShaderParamValue::Floats(vec![40.0])));
        assert_eq!(
            p.preset_default,
            Some(ShaderParamValue::Floats(vec![180.0]))
        );
    }

    #[test]
    fn a_trait_flag_is_a_preset_constant_that_is_not_a_parameter() {
        let mut e = effect(vec![variable("Specular", GrcVariableType::Float)]);
        e.presets.push(GrcEffectPreset {
            name: "ExposeAlphaMap".into(),
            value: 1,
        });
        e.presets.push(GrcEffectPreset {
            name: DRAW_BUCKET_PRESET.into(),
            value: 2,
        });
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[("x".into(), e)],
            &[SpsPreset::parse("x", "shader x")],
        );
        let s = defs.shader_named("x").unwrap();
        assert_eq!(s.draw_bucket, Some(2), "the bucket is not a trait");
        assert_eq!(s.traits.len(), 1);
        assert_eq!(s.traits[0].name, "ExposeAlphaMap");
    }

    #[test]
    fn an_enhanced_sampler_binds_and_a_constant_sits_in_its_buffers_row() {
        use crate::gta5::shaders::sga::{SgaEffect, SgaVariable, SgaVariableType};
        let var = |name: &str, var_type, slot, cbuffer: Option<&str>| SgaVariable {
            name: name.into(),
            semantic: None,
            var_type,
            count: 0,
            slot,
            cbuffer: cbuffer.map(Into::into),
            default: Vec::new(),
        };
        let effect = SgaEffect {
            name: "normal_tnt".into(),
            programs: Default::default(),
            variables: vec![
                var("DiffuseSampler", SgaVariableType::Sampler, 2, None),
                var("bumpiness", SgaVariableType::Float, 0x34, Some("locals")),
            ],
        };
        let preset = SpsPreset::parse(
            "normal_tnt_alpha",
            "shader normal_tnt\n__rage_drawbucket {\n\tint 1\n}",
        );
        let (defs, unresolved) =
            ShaderDefinitions::assemble_sga("gta5", "gen9", &[effect], &[preset]);
        assert!(unresolved.is_empty());
        let s = defs.shader_named("normal_tnt_alpha").unwrap();
        assert_eq!(s.draw_bucket, Some(1));
        let sampler = s.parameter(hash_name("DiffuseSampler")).unwrap();
        assert_eq!(
            (sampler.kind, sampler.is_binding()),
            (ShaderParamKind::Sampler, true)
        );
        let bumpiness = s.parameter(hash_name("bumpiness")).unwrap();
        assert_eq!(bumpiness.register, 3);
        assert_eq!(bumpiness.cbuffer.as_deref(), Some("locals"));
        assert!(
            defs.shader_named("normal_tnt").is_some(),
            "an effect is still a shader under its own name"
        );
    }

    #[test]
    fn a_ui_name_annotation_is_carried_and_others_are_not() {
        let mut v = variable("gSpec", GrcVariableType::Float);
        v.annotations = vec![
            GrcVariableAnnotation {
                name: "UIName".into(),
                value: GrcAnnotationValue::String("Specular".into()),
            },
            GrcVariableAnnotation {
                name: "UIMin".into(),
                value: GrcAnnotationValue::Float(0.0),
            },
        ];
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[("x".into(), effect(vec![v]))],
            &[SpsPreset::parse("x", "shader x")],
        );
        assert_eq!(
            defs.shader_named("x").unwrap().parameters[0]
                .ui_name
                .as_deref(),
            Some("Specular")
        );
    }

    /// ⚠ A file may address a parameter by the effect's *semantic* rather than
    /// the variable's name: `ptfx_sprite` spells the variable
    /// `wrapLigthtingTerm`, typo included, and states `WrapLightingTerm` as its
    /// semantic, which is what every `.ypt` binding it carries.
    #[test]
    fn a_parameter_answers_to_its_semantic_as_well_as_its_name() {
        let mut v = variable("wrapLigthtingTerm", GrcVariableType::Float);
        v.semantic = "WrapLightingTerm".into();
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[("ptfx_sprite".into(), effect(vec![v]))],
            &[SpsPreset::parse("ptfx_sprite", "shader ptfx_sprite")],
        );
        let shader = defs.shader_named("ptfx_sprite").expect("the shader");
        for spelling in ["wrapLigthtingTerm", "WrapLightingTerm"] {
            assert_eq!(
                shader
                    .parameter(crate::string_hash(spelling.as_bytes()))
                    .map(|p| p.name.as_str()),
                Some("wrapLigthtingTerm"),
                "{spelling}"
            );
        }
    }

    /// ⚠ One parameter's semantic is another parameter's name in 385 of a
    /// retail Legacy install's shaders, so a semantic-first lookup answers
    /// with the wrong parameter. The name wins.
    #[test]
    fn a_name_outranks_another_parameters_semantic() {
        let mut sampler = variable("gCSMShadowTextureSamp", GrcVariableType::Texture);
        sampler.semantic = "gCSMShadowTexture".into();
        let texture = variable("gCSMShadowTexture", GrcVariableType::Texture);

        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[("alpha".into(), effect(vec![sampler, texture]))],
            &[SpsPreset::parse("alpha", "shader alpha")],
        );
        assert_eq!(
            defs.shader_named("alpha")
                .unwrap()
                .parameter(crate::string_hash(b"gCSMShadowTexture"))
                .map(|p| p.name.as_str()),
            Some("gCSMShadowTexture")
        );
    }

    /// ⚠ A schema-1 database deserializes without error and simply has no
    /// semantics, which reads downstream as "this install declares fewer
    /// parameters" rather than as a stale file.
    #[test]
    fn a_database_from_an_older_schema_is_refused_rather_than_read_short() {
        let mut defs = ShaderDefinitions::new("gta5", "gen8");
        defs.schema_version = 1;
        let json = serde_json::to_string(&defs).unwrap();
        let err = ShaderDefinitions::from_json(&json).expect_err("should refuse");
        assert!(err.to_string().contains("schema 1"), "{err}");
    }

    #[test]
    fn a_database_round_trips_through_json() {
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[(
                "default".into(),
                effect(vec![variable("Specular", GrcVariableType::Float)]),
            )],
            &[SpsPreset::parse("default", "shader default")],
        );
        let json = defs.to_json().unwrap();
        let back = ShaderDefinitions::from_json(&json).unwrap();
        assert_eq!(back, defs);
        assert_eq!(back.schema_version, SHADER_DEFS_SCHEMA);
    }

    /// ⚠ A sampler's default is a run of small integers, which is also what a
    /// run of small floats looks like once it is written out. Reading it back
    /// as the wrong arm is silent.
    #[test]
    fn an_integer_default_does_not_come_back_as_floats() {
        let mut v = variable("FogRaySampler", GrcVariableType::Texture);
        v.default_uints = vec![0, 3, 1, 3, 2, 3];
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[("x".into(), effect(vec![v]))],
            &[SpsPreset::parse("x", "shader x")],
        );
        let json = defs.to_json().unwrap();
        let back = ShaderDefinitions::from_json(&json).unwrap();
        assert_eq!(
            back.shader_named("x").unwrap().parameters[0].default,
            Some(ShaderParamValue::Uints(vec![0, 3, 1, 3, 2, 3]))
        );
        assert_eq!(back, defs);
    }

    #[test]
    fn the_four_unexplained_type_codes_keep_the_width_they_were_measured_with() {
        for (code, expected) in [(17, 1), (18, 2), (19, 3), (20, 4)] {
            assert_eq!(
                ShaderParamKind::Unknown(code).components(),
                Some(expected),
                "code {code}"
            );
            assert_eq!(
                ShaderParamKind::Unknown(code).as_str(),
                "unknown",
                "a measured width is not a name; code {code} stays unnamed"
            );
        }
        assert_eq!(ShaderParamKind::Unknown(200).components(), None);
        assert_eq!(ShaderParamKind::Texture.components(), None);
    }

    /// ⚠ "I do not know this shader" and "I know it and not this parameter"
    /// are different answers, and a caller that cannot tell them apart will
    /// report a missing database as a material with no parameters.
    #[test]
    fn an_unknown_shader_and_an_unknown_parameter_are_different_answers() {
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[(
                "default".into(),
                effect(vec![variable("Specular", GrcVariableType::Float)]),
            )],
            &[SpsPreset::parse("default", "shader default")],
        );
        let known = crate::string_hash(b"Specular");
        let stranger = crate::string_hash(b"NotAParameter");

        let missing = defs.name_material(crate::string_hash(b"nope"), &[known]);
        assert_eq!(missing.shader, None);
        assert_eq!(missing.unnamed, vec![known]);
        assert!(!missing.is_complete());

        let partial = defs.name_material(crate::string_hash(b"default"), &[known, stranger]);
        assert_eq!(partial.shader.as_deref(), Some("default"));
        assert_eq!(partial.named.len(), 1);
        assert_eq!(partial.unnamed, vec![stranger]);

        let whole = defs.name_material(crate::string_hash(b"default"), &[known]);
        assert!(whole.is_complete());
    }

    /// ⚠ "No database" and "a database that does not know this shader" are
    /// different answers, and the particle preview's warning is wrong for one
    /// of them: with no database installed nothing was asked, so nothing can
    /// be called unrecognised.
    #[test]
    fn an_absent_database_and_an_unknown_shader_are_different_answers() {
        let _lock = exclusive();
        clear();
        assert!(!is_installed());
        assert!(!describes_shader(crate::string_hash(b"default")));

        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[(
                "default".into(),
                effect(vec![variable("Specular", GrcVariableType::Float)]),
            )],
            &[SpsPreset::parse("default", "shader default")],
        );
        install(defs);
        assert!(is_installed());
        assert!(describes_shader(crate::string_hash(b"default")));
        assert!(!describes_shader(crate::string_hash(b"stranger")));
        assert!(declares(
            crate::string_hash(b"default"),
            crate::string_hash(b"Specular")
        ));
        assert!(!declares(
            crate::string_hash(b"default"),
            crate::string_hash(b"Nope")
        ));
        clear();
        assert!(!is_installed());
    }

    #[test]
    fn the_installed_database_answers_by_hash_and_nothing_answers_without_one() {
        let _lock = exclusive();
        clear();
        let hash = crate::string_hash(b"default");
        let param = crate::string_hash(b"Specular");
        assert!(describe(hash, param).is_none());
        assert!(parameters_of(hash).is_none());

        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[(
                "default".into(),
                effect(vec![variable("Specular", GrcVariableType::Float)]),
            )],
            &[SpsPreset::parse("default", "shader default")],
        );
        install(defs);
        assert_eq!(
            describe(hash, param).map(|p| p.name),
            Some("Specular".into())
        );
        assert_eq!(parameters_of(hash).map(|p| p.len()), Some(1));
        clear();
        assert!(describe(hash, param).is_none());
    }
}
