//! A drawable, a texture dictionary, a drawable dictionary or a fragment
//! moved between Gen8 and Gen9: the spine is carried as it is, and the four
//! types Enhanced changed are rebuilt, with a [`LossReport`] naming each value
//! the other target has no field for.
//!
//! ⚠ **Nothing is invented.** A value the target cannot state is dropped and
//! named; a word only the target states (bind flags, buffer views, sampler
//! states, usage data, the preset tag, the per-thread copies) is left at its
//! declaration's default. A texture the other target cannot describe, one a
//! tile mode swizzles or one that is not a plain 2D texture, refuses the whole
//! file rather than being carried as bytes that mean something else.
//!
//! ⚠ **An Enhanced material's layout is OpenRage's, not the shader's.** A
//! Legacy entry states its value and not where the Enhanced shader reads it,
//! so the textures take slots in entry order and the constants whole registers
//! of one buffer in entry order. Every value comes back, and the game's own
//! Enhanced layout would place them elsewhere.

use std::fmt;
use std::mem::offset_of;

use super::drawable::gen8::{GrcFvf, GrcIndexBuffer, GrcInstanceData, GrcTextureReference};
use super::drawable::gen9::{
    GrcIndexBufferG9, GrcInstanceDataG9, GrcTextureReferenceG9, GrcVertexBufferG9, ShaderParamInfo,
    ShaderParamInfos, VertexDeclaration,
};
use super::drawable::{
    gen8, DrawableTarget, GrmGeometry, GrmModel, GrmShaderGroup, GtaDrawable, RmcDrawable, RmcLod,
    RmcLodGroup,
};
use super::drawable_codecs::gen8::{EntryData, GrcInstanceDataEntry};
use super::drawable_codecs::gen9::Params;
use super::drawable_codecs::TextureSlot;
use super::fragment::{
    EnvironmentCloth, FragDrawable, FragPhysicsLOD, FragPhysicsLODGroup, FragType, FragTypeChild,
    GtaFragType,
};
use super::shaders::shader_defs::ParamRemap;
use super::shared::PgDictionary;
use super::texture_dict::gen8::{GrcTexture, PAYLOAD_SIZE_MASK};
use super::texture_dict::gen9::{
    GrcTextureG9, PixelFormat, ShaderResourceViewG9, TextureDimension, TileMode,
};
use crate::codec::mesh::{
    elements_g9, GrcDataSize, GrcFvfChannel, VertexElementFormatG9, VertexReader, VertexSource,
    G9_SLOT_TO_GEN8_SEMANTIC,
};
use crate::codec::texture::{DxgiFormat, GrcTextureFormat, TextureSource};
use crate::decl::{
    Gen8, Gen9, HasOrigin, Hash32, Keyed, NullIsEmpty, Open, OpenEnum, Owned, Raw, Record, Shared,
    Vec4,
};
use crate::{Error, Generation};

/// A value the other target has no field for, and where it was.
///
/// ⚠ Its `Display` is what `openrage-cli convert retarget` reports, and the
/// app words the same value for a modder, so a new kind of loss is a variant
/// both match on rather than a sentence only one of them can read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub at: Place,
    pub lost: Lost,
}

impl fmt::Display for Dropped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.lost)
    }
}

/// Where a value was dropped: `shader 2`, `level0/model1/geometry0` or `texture name`, under the drawable holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The drawable of a dictionary or a fragment it is in; `None` in a file of one drawable or of textures.
    pub drawable: Option<Part>,
    pub spot: Spot,
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.drawable {
            Some(part) => write!(f, "{part}/{}", self.spot),
            None => write!(f, "{}", self.spot),
        }
    }
}

/// A shader, a geometry or a texture of one drawable or texture dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spot {
    /// The shader group's shader at this index.
    Shader(usize),
    /// A model's geometry at a level of detail; `None` for the copy of the first level a drawable keeps apart.
    Geometry {
        lod: Option<usize>,
        model: usize,
        geometry: usize,
    },
    /// A texture, by its name.
    Texture(String),
}

impl fmt::Display for Spot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shader(index) => write!(f, "shader {index}"),
            Self::Geometry {
                lod,
                model,
                geometry,
            } => {
                match lod {
                    Some(lod) => write!(f, "level{lod}")?,
                    None => f.write_str("container")?,
                }
                write!(f, "/model{model}/geometry{geometry}")
            }
            Self::Texture(name) => write!(f, "texture {name}"),
        }
    }
}

/// One drawable of a drawable dictionary or a fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// A drawable dictionary's entry, by its hash.
    Entry(u32),
    /// A fragment's own drawable.
    Common,
    /// A fragment's cloth drawable.
    Cloth,
    /// A fragment's extra drawable, by its index.
    Extra(usize),
    /// The drawable the environment cloth at this index refers to.
    EnvironmentCloth(usize),
    /// A physics child's drawable at a physics level, whole or damaged.
    Child {
        level: usize,
        child: usize,
        damaged: bool,
    },
}

impl fmt::Display for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Entry(hash) => write!(f, "drawable {hash:#010x}"),
            Self::Common => f.write_str("drawable"),
            Self::Cloth => f.write_str("cloth drawable"),
            Self::Extra(index) => write!(f, "extra drawable {index}"),
            Self::EnvironmentCloth(index) => write!(f, "environment cloth {index}"),
            Self::Child {
                level,
                child,
                damaged,
            } => {
                write!(f, "physics level {level}, child {child}")?;
                if *damaged {
                    f.write_str(" damaged")?;
                }
                Ok(())
            }
        }
    }
}

/// What was dropped, with the parameter hash, channel or slot where one applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lost {
    /// A Legacy entry, a texture or a value, the shader database does not spell for Enhanced, by its name's hash.
    Entry { hash: u32, texture: bool },
    /// An Enhanced texture parameter the shader database does not spell for Legacy.
    TextureParameter(u32),
    /// An Enhanced constant the shader database does not spell for Legacy.
    Constant(u32),
    /// An Enhanced constant of this many bytes, which are not whole floats.
    Uneven(u32, usize),
    /// An Enhanced sampler state, by its sampler's hash.
    Sampler(u32),
    /// An Enhanced parameter that is neither a texture, a constant nor a sampler.
    Other(u32),
    /// An Enhanced texture slot no parameter Legacy spells binds.
    Slot(usize),
    /// A Legacy vertex channel Enhanced has no slot for, and its encoding.
    Channel(GrcFvfChannel, GrcDataSize),
    /// An Enhanced vertex element Legacy has no channel for, by its slot, and its format.
    Element(usize, VertexElementFormatG9),
    /// A word of the texture a material refers to by this name.
    Reference(String, Word),
    /// A word of a geometry, a buffer, a material or a texture.
    Word(Word),
}

impl From<Word> for Lost {
    fn from(word: Word) -> Self {
        Self::Word(word)
    }
}

impl fmt::Display for Lost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unspelled = "which the shader database does not spell for";
        match self {
            Self::Entry { hash, .. } => write!(f, "entry {hash:#010x}, {unspelled} Enhanced"),
            Self::TextureParameter(hash) => {
                write!(f, "texture parameter {hash:#010x}, {unspelled} Legacy")
            }
            Self::Constant(hash) => write!(f, "constant {hash:#010x}, {unspelled} Legacy"),
            Self::Uneven(hash, bytes) => {
                write!(
                    f,
                    "constant {hash:#010x}, {bytes} bytes and not whole floats"
                )
            }
            Self::Sampler(hash) => write!(f, "sampler {hash:#010x}"),
            Self::Other(hash) => write!(f, "other parameter {hash:#010x}"),
            Self::Slot(slot) => {
                write!(
                    f,
                    "texture slot {slot}, which no parameter Legacy spells binds"
                )
            }
            Self::Channel(channel, size) => {
                write!(f, "vertex channel {} ({size:?})", channel.name())
            }
            Self::Element(slot, format) => write!(f, "vertex element {slot} ({format:?})"),
            Self::Reference(name, word) => write!(f, "reference {name}: {word}"),
            Self::Word(word) => write!(f, "{word}"),
        }
    }
}

/// A word only the source's declaration states, displayed as that declaration names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    VertexData,
    VertexBufferFlags,
    LockPtr,
    FvfPastSixteen,
    FvfFlags,
    DynamicOrder,
    ChannelSizes,
    IndexCountTopByte,
    Registers,
    MaterialHash,
    BasisHashCode,
    MaterialHashCode,
    PhysMtlDeprecated,
    Flags,
    IsInstanced,
    UserFlags,
    SortKeyDeprecated,
    Texture,
    ResourceTypeAndConversionFlags,
    LayerCount,
    PhysicalSizeAndTemplateType,
    HandleIndex,
    ExtraFlags,
    TemplateType,
    InfoBitsPastSrgb,
    StereoRtMode,
    MipStride,
    PayloadPastMipChain,
    VertexBindFlags,
    VertexSrv,
    InfoData,
    IndexSize,
    IndexBindFlags,
    IndexSrv,
    Preset,
    Samplers,
    Others,
    BufferCopies,
    Width,
    Height,
    Depth,
    Dimension,
    Format,
    TileMode,
    AntiAliasType,
    MipLevels,
    UsageData,
    AfterMipLevels,
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::VertexData => "vertex_data, a copy of the vertices apart from the buffer's",
            Self::VertexBufferFlags => "vertex_buffer.flags",
            Self::LockPtr => "vertex_buffer.lock_ptr, a copy of the vertices apart from data",
            Self::FvfPastSixteen => "vertex_buffer.fvf.fvf past the sixteen channels",
            Self::FvfFlags => "vertex_buffer.fvf.flags",
            Self::DynamicOrder => "vertex_buffer.fvf.dynamic_order",
            Self::ChannelSizes => {
                "vertex_buffer.fvf.channel_sizes of the channels it does not carry"
            }
            Self::IndexCountTopByte => "index_buffer.index_count's top byte",
            Self::Registers => "the entries' registers",
            Self::MaterialHash => "material_hash",
            Self::BasisHashCode => "basis_hash_code",
            Self::MaterialHashCode => "material_hash_code",
            Self::PhysMtlDeprecated => "phys_mtl_deprecated",
            Self::Flags => "flags",
            Self::IsInstanced => "is_instanced",
            Self::UserFlags => "user_flags",
            Self::SortKeyDeprecated => "sort_key_deprecated",
            Self::Texture => "texture",
            Self::ResourceTypeAndConversionFlags => "resource_type_and_conversion_flags",
            Self::LayerCount => "layer_count",
            Self::PhysicalSizeAndTemplateType => "physical_size_and_template_type",
            Self::HandleIndex => "handle_index",
            Self::ExtraFlags => "extra_flags",
            Self::TemplateType => "the template type",
            Self::InfoBitsPastSrgb => "info_bits past sRGB",
            Self::StereoRtMode => "stereo_rt_mode",
            Self::MipStride => "mip_stride",
            Self::PayloadPastMipChain => "payload bytes past the mip chain",
            Self::VertexBindFlags => "vertex_buffer.bind_flags",
            Self::VertexSrv => "vertex_buffer.srv",
            Self::InfoData => "vertex_buffer.info.data beside the vertex size",
            Self::IndexSize => "index_buffer.index_size",
            Self::IndexBindFlags => "index_buffer.bind_flags",
            Self::IndexSrv => "index_buffer.srv",
            Self::Preset => "preset",
            Self::Samplers => "params.samplers, each a sampler state",
            Self::Others => "params.others",
            Self::BufferCopies => "param_infos.buffer_copies",
            Self::Width => "width",
            Self::Height => "height",
            Self::Depth => "depth",
            Self::Dimension => "dimension",
            Self::Format => "format",
            Self::TileMode => "tile_mode",
            Self::AntiAliasType => "anti_alias_type",
            Self::MipLevels => "mip_levels",
            Self::UsageData => "usage_data",
            Self::AfterMipLevels => "after_mip_levels",
        })
    }
}

/// Every value a retarget dropped, in the order the drawable was walked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LossReport(pub Vec<Dropped>);

impl LossReport {
    /// Whether an entry at `at` names a field containing `field`, as the report prints them.
    pub fn names(&self, at: &str, field: &str) -> bool {
        (self.0.iter()).any(|d| d.at.to_string() == at && d.lost.to_string().contains(field))
    }

    fn drop(&mut self, at: &Spot, lost: impl Into<Lost>) {
        self.0.push(Dropped {
            at: Place {
                drawable: None,
                spot: at.clone(),
            },
            lost: lost.into(),
        });
    }

    fn drop_if(&mut self, at: &Spot, lost: impl Into<Lost>, dropped: bool) {
        if dropped {
            self.drop(at, lost);
        }
    }
}

impl fmt::Display for LossReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<String> = self.0.iter().map(Dropped::to_string).collect();
        f.write_str(&lines.join("; "))
    }
}

/// `drawable` as Enhanced lays it out, and what Enhanced cannot carry; `remap` spells each parameter.
pub fn to_gen9(
    drawable: &GtaDrawable<Gen8>,
    remap: &ParamRemap,
) -> Result<(GtaDrawable<Gen9>, LossReport), Error> {
    walked(ToGen9::new(remap), |to| retarget(drawable, to))
}

/// `drawable` as Legacy lays it out, and what Legacy cannot carry; `remap` spells each parameter.
pub fn to_gen8(
    drawable: &GtaDrawable<Gen9>,
    remap: &ParamRemap,
) -> Result<(GtaDrawable<Gen8>, LossReport), Error> {
    walked(ToGen8::new(remap), |to| retarget(drawable, to))
}

/// A `.ytd` as Enhanced lays it out, and what Enhanced cannot carry.
pub fn texture_dictionary_to_gen9(
    dict: &PgDictionary<GrcTexture>,
) -> Result<(PgDictionary<GrcTextureG9>, LossReport), Error> {
    walked(ToGen9::new(&ParamRemap::default()), |to| {
        root(dictionary(dict, |_, t| to.texture(t))?)
    })
}

/// A `.ytd` as Legacy lays it out, and what Legacy cannot carry.
pub fn texture_dictionary_to_gen8(
    dict: &PgDictionary<GrcTextureG9>,
) -> Result<(PgDictionary<GrcTexture>, LossReport), Error> {
    walked(ToGen8::new(&ParamRemap::default()), |to| {
        root(dictionary(dict, |_, t| to.texture(t))?)
    })
}

/// A `.ydd` as Enhanced lays it out, each drawable named by its hash in the report; `remap` spells each parameter.
pub fn drawable_dictionary_to_gen9(
    dict: &PgDictionary<GtaDrawable<Gen8>>,
    remap: &ParamRemap,
) -> Result<(PgDictionary<GtaDrawable<Gen9>>, LossReport), Error> {
    walked(ToGen9::new(remap), |to| drawable_dictionary(dict, to))
}

/// A `.ydd` as Legacy lays it out, each drawable named by its hash in the report; `remap` spells each parameter.
pub fn drawable_dictionary_to_gen8(
    dict: &PgDictionary<GtaDrawable<Gen9>>,
    remap: &ParamRemap,
) -> Result<(PgDictionary<GtaDrawable<Gen8>>, LossReport), Error> {
    walked(ToGen8::new(remap), |to| drawable_dictionary(dict, to))
}

/// A `.yft` as Enhanced lays it out, each drawable named by its place in the report; `remap` spells each parameter.
pub fn fragment_to_gen9(
    fragment: &GtaFragType<Gen8>,
    remap: &ParamRemap,
) -> Result<(GtaFragType<Gen9>, LossReport), Error> {
    walked(ToGen9::new(remap), |to| self::fragment(fragment, to))
}

/// A `.yft` as Legacy lays it out, each drawable named by its place in the report; `remap` spells each parameter.
pub fn fragment_to_gen8(
    fragment: &GtaFragType<Gen9>,
    remap: &ParamRemap,
) -> Result<(GtaFragType<Gen8>, LossReport), Error> {
    walked(ToGen8::new(remap), |to| self::fragment(fragment, to))
}

/// The extensions a file retargets by.
pub const CONVERTED: [&str; 4] = ["ydr", "ytd", "ydd", "yft"];

/// A file as the other target than its own RSC7 version names.
pub struct Retargeted {
    pub from: Generation,
    pub to: Generation,
    pub bytes: Vec<u8>,
    pub report: LossReport,
}

/// `bytes`, a file called `name`, converted to the other target, and what that dropped; `Err` says why it is refused.
///
/// ⚠ A file's target is its RSC7 version, never the folder it sits in; a version neither target
/// declares is refused rather than read as Legacy.
pub fn retarget_file(name: &str, bytes: &[u8], remap: &ParamRemap) -> Result<Retargeted, String> {
    let ext = extension_of(name);
    if !CONVERTED.contains(&ext.as_str()) {
        return Err("not a drawable, texture dictionary, drawable dictionary or fragment".into());
    }
    let from = layout_of(&ext, bytes)?;
    let (bytes, report) = converted(&ext, from, bytes, remap).map_err(|e| e.to_string())?;
    let to = if from.is_gen9() {
        Generation::Gen8
    } else {
        Generation::Gen9
    };
    Ok(Retargeted {
        from,
        to,
        bytes,
        report,
    })
}

/// The target a file of `ext` is laid out for, by its own RSC7 version.
pub fn layout_of(ext: &str, bytes: &[u8]) -> Result<Generation, String> {
    use crate::container::resource::{ResourceEnvelope, Rsc7};
    let version = Rsc7::peek(bytes).map_err(|e| e.to_string())?.version();
    (crate::gta5::formats::format(ext).ok_or("not a declared format")?)
        .target_of_version(version)
        .map_err(|e| e.to_string())
}

/// The lower-case extension of `name`, empty when it has none.
fn extension_of(name: &str) -> String {
    std::path::Path::new(name)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// `bytes`, a `from` file of `ext`, saved as the other target, and what that dropped.
fn converted(
    ext: &str,
    from: Generation,
    bytes: &[u8],
    remap: &ParamRemap,
) -> Result<(Vec<u8>, LossReport), Error> {
    use crate::decl::File;
    fn saved<T: File>((value, report): (T, LossReport)) -> Result<(Vec<u8>, LossReport), Error> {
        Ok((value.to_bytes()?, report))
    }
    match (ext, from.is_gen9()) {
        ("ydr", false) => saved(to_gen9(&GtaDrawable::<Gen8>::from_bytes(bytes)?, remap)?),
        ("ydr", true) => saved(to_gen8(&GtaDrawable::<Gen9>::from_bytes(bytes)?, remap)?),
        ("ytd", false) => saved(texture_dictionary_to_gen9(
            &PgDictionary::<GrcTexture>::from_bytes(bytes)?,
        )?),
        ("ytd", true) => saved(texture_dictionary_to_gen8(
            &PgDictionary::<GrcTextureG9>::from_bytes(bytes)?,
        )?),
        ("ydd", false) => saved(drawable_dictionary_to_gen9(
            &PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes)?,
            remap,
        )?),
        ("ydd", true) => saved(drawable_dictionary_to_gen8(
            &PgDictionary::<GtaDrawable<Gen9>>::from_bytes(bytes)?,
            remap,
        )?),
        ("yft", false) => saved(fragment_to_gen9(
            &GtaFragType::<Gen8>::from_bytes(bytes)?,
            remap,
        )?),
        ("yft", true) => saved(fragment_to_gen8(
            &GtaFragType::<Gen9>::from_bytes(bytes)?,
            remap,
        )?),
        _ => unreachable!("retarget_file admits only the formats this converts"),
    }
}

fn walked<A, B, L, T>(
    mut to: L,
    walk: impl FnOnce(&mut L) -> Result<T, Error>,
) -> Result<(T, LossReport), Error>
where
    A: DrawableTarget,
    B: DrawableTarget,
    L: Leaves<A, B>,
{
    let out = walk(&mut to)?;
    Ok((out, std::mem::take(to.report())))
}

/// `walk`, with what it drops and what it refuses named as within `at`.
fn within<A, B, L, T>(
    at: Part,
    to: &mut L,
    walk: impl FnOnce(&mut L) -> Result<T, Error>,
) -> Result<T, Error>
where
    A: DrawableTarget,
    B: DrawableTarget,
    L: Leaves<A, B>,
{
    let from = to.report().0.len();
    let out = walk(to).map_err(|e| match e {
        Error::Invalid { what, why } => refused(format!("{at}/{what}"), why),
        e => e,
    })?;
    for d in &mut to.report().0[from..] {
        d.at.drawable = Some(at);
    }
    Ok(out)
}

/// A file's root, with no path: the source's would make `save()` write the other target over it.
fn root<T: HasOrigin>(mut out: T) -> Result<T, Error> {
    *out.origin_mut() = Default::default();
    Ok(out)
}

/// `d` with each entry rebuilt by `each`, given its index; the codes are carried, so each entry keeps its key.
fn dictionary<T: Owned, U: Owned>(
    d: &PgDictionary<T>,
    mut each: impl FnMut(usize, &T) -> Result<U, Error>,
) -> Result<PgDictionary<U>, Error>
where
    Raw<T>: Record,
    Raw<U>: Record,
{
    let mut entries = Vec::with_capacity(d.entries.len());
    for (i, t) in d.entries.iter().enumerate() {
        entries.push(match t.as_deref() {
            Some(t) => Some(Box::new(each(i, t)?)),
            None => None,
        });
    }
    Ok(PgDictionary {
        base: d.base.clone(),
        codes: d.codes.clone(),
        entries,
        __carry: d.__carry.clone(),
    })
}

fn drawable_dictionary<A: DrawableTarget, B: DrawableTarget>(
    d: &PgDictionary<GtaDrawable<A>>,
    to: &mut impl Leaves<A, B>,
) -> Result<PgDictionary<GtaDrawable<B>>, Error> {
    root(dictionary(d, |i, drawable| {
        let hash = d.codes.get(i).copied().unwrap_or_default();
        within(Part::Entry(hash), to, |to| retarget(drawable, to))
    })?)
}

/// The four types one target lays out differently, rebuilt as the other's.
trait Leaves<A: DrawableTarget, B: DrawableTarget> {
    fn vertex_buffer(&mut self, vb: &A::VertexBuffer, at: &Spot) -> Result<B::VertexBuffer, Error>;
    fn index_buffer(&mut self, ib: &A::IndexBuffer, at: &Spot) -> B::IndexBuffer;
    fn shader(&mut self, s: &A::InstanceData, at: &Spot) -> Result<B::InstanceData, Error>;
    fn texture(&mut self, t: &A::Texture) -> Result<B::Texture, Error>;
    fn report(&mut self) -> &mut LossReport;
}

fn refused(what: impl Into<String>, why: impl Into<String>) -> Error {
    Error::Invalid {
        what: what.into(),
        why: why.into(),
    }
}

fn retarget<A: DrawableTarget, B: DrawableTarget>(
    d: &GtaDrawable<A>,
    to: &mut impl Leaves<A, B>,
) -> Result<GtaDrawable<B>, Error> {
    let mut out = GtaDrawable {
        base: drawable(&d.base, to)?,
        lights: d.lights.clone(),
        bound: d.bound.clone(),
        __carry: d.__carry.clone(),
    };
    // ⚠ The source's path would make `save()` write the other target over it.
    *out.__carry.origin_mut() = Default::default();
    Ok(out)
}

fn drawable<A: DrawableTarget, B: DrawableTarget>(
    d: &RmcDrawable<A>,
    to: &mut impl Leaves<A, B>,
) -> Result<RmcDrawable<B>, Error> {
    let shader_group = match d.shader_group.as_deref() {
        Some(g) => Some(Box::new(shader_group(g, to)?)),
        None => None,
    };
    let mut lods = Vec::with_capacity(4);
    for (i, lod) in d.lod_group.lods.iter().enumerate() {
        lods.push(NullIsEmpty::new(level(lod, Some(i), to)?));
    }
    let lods: [NullIsEmpty<RmcLod<B>>; 4] = lods
        .try_into()
        .map_err(|_| refused("RmcLodGroup.lods", "not four levels"))?;
    let container = match &d.container {
        Shared::Same => Shared::Same,
        Shared::Own(None) => Shared::Own(None),
        Shared::Own(Some(lod)) => Shared::Own(Some(Box::new(level(lod, None, to)?))),
    };
    let g = &d.lod_group;
    Ok(RmcDrawable {
        base: d.base.clone(),
        shader_group,
        skeleton: d.skeleton.clone(),
        lod_group: RmcLodGroup {
            bounds_sphere: g.bounds_sphere.clone(),
            bounds_box: g.bounds_box.clone(),
            lods,
            lod_distances: g.lod_distances,
            bucket_masks: g.bucket_masks,
            __carry: g.__carry.clone(),
        },
        joint_data: d.joint_data.clone(),
        handle_index: d.handle_index,
        container_size_qw: d.container_size_qw,
        container,
        name: d.name.clone(),
        __carry: d.__carry.clone(),
    })
}

/// ⚠ Only the drawables differ between the targets; everything else a fragment states is carried as it is.
fn fragment<A: DrawableTarget, B: DrawableTarget>(
    f: &GtaFragType<A>,
    to: &mut impl Leaves<A, B>,
) -> Result<GtaFragType<B>, Error> {
    let t = &f.base;
    let mut extra_drawables = Vec::with_capacity(t.extra_drawables.len());
    for (i, d) in t.extra_drawables.iter().enumerate() {
        extra_drawables.push(Box::new(frag_drawable(d, Part::Extra(i), to)?));
    }
    let mut env_cloth = Vec::with_capacity(t.env_cloth.len());
    for (i, c) in t.env_cloth.iter().enumerate() {
        let at = Part::EnvironmentCloth(i);
        env_cloth.push(Box::new(EnvironmentCloth {
            base: c.base.clone(),
            tuning: c.tuning.clone(),
            referenced_drawable: frag_drawable_opt(c.referenced_drawable.as_deref(), at, to)?,
            cloth_controller: c.cloth_controller.clone(),
            initial_position: c.initial_position.clone(),
            force: c.force.clone(),
            user_data: c.user_data.clone(),
            flags: c.flags,
            __carry: c.__carry.clone(),
        }));
    }
    let common_drawable = frag_drawable_opt(t.common_drawable.as_deref(), Part::Common, to)?;
    let common_cloth_drawable =
        frag_drawable_opt(t.common_cloth_drawable.as_deref(), Part::Cloth, to)?;
    let physics_lod_group = match t.physics_lod_group.as_deref() {
        None => None,
        Some(g) => {
            let mut lods = Vec::with_capacity(3);
            for (l, lod) in [&g.high_lod, &g.medium_lod, &g.low_lod]
                .into_iter()
                .enumerate()
            {
                lods.push(match lod.as_deref() {
                    Some(lod) => Some(Box::new(physics_lod(lod, l, to)?)),
                    None => None,
                });
            }
            let [high_lod, medium_lod, low_lod]: [_; 3] = lods
                .try_into()
                .map_err(|_| refused("FragPhysicsLODGroup", "not three levels"))?;
            Some(Box::new(FragPhysicsLODGroup {
                base: g.base.clone(),
                high_lod,
                medium_lod,
                low_lod,
                __carry: g.__carry.clone(),
            }))
        }
    };
    root(GtaFragType {
        base: FragType {
            base: t.base.clone(),
            bounding_sphere: t.bounding_sphere.clone(),
            common_drawable,
            extra_drawables,
            extra_drawable_names: t.extra_drawable_names.clone(),
            damaged_drawable: t.damaged_drawable,
            tune_name: t.tune_name.clone(),
            env_cloth,
            collision_eventset: t.collision_eventset.clone(),
            shared_matrix_set: t.shared_matrix_set.clone(),
            estimated_cache_size: t.estimated_cache_size,
            estimated_articulated_cache_size: t.estimated_articulated_cache_size,
            entity_class: t.entity_class,
            art_asset_id: t.art_asset_id,
            attach_bottom_end: t.attach_bottom_end,
            flags: t.flags,
            client_class_id: t.client_class_id,
            unbroken_elasticity: t.unbroken_elasticity,
            gravity_factor: t.gravity_factor,
            buoyancy_factor: t.buoyancy_factor,
            glass_attachment_bone: t.glass_attachment_bone,
            glass_pane_model_infos: t.glass_pane_model_infos.clone(),
            physics_lod_group,
            common_cloth_drawable,
            __carry: t.__carry.clone(),
        },
        lights: f.lights.clone(),
        vehicle_window_data: f.vehicle_window_data.clone(),
        __carry: f.__carry.clone(),
    })
}

fn physics_lod<A: DrawableTarget, B: DrawableTarget>(
    lod: &FragPhysicsLOD<A>,
    l: usize,
    to: &mut impl Leaves<A, B>,
) -> Result<FragPhysicsLOD<B>, Error> {
    let mut children = Vec::with_capacity(lod.children.len());
    for (c, child) in lod.children.iter().enumerate() {
        let at = |damaged| Part::Child {
            level: l,
            child: c,
            damaged,
        };
        let undamaged_entity = frag_drawable_opt(child.undamaged_entity.as_deref(), at(false), to)?;
        let damaged_entity = frag_drawable_opt(child.damaged_entity.as_deref(), at(true), to)?;
        children.push(Box::new(FragTypeChild {
            base: child.base.clone(),
            undamaged_mass: child.undamaged_mass,
            damaged_mass: child.damaged_mass,
            owner_group_pointer_index: child.owner_group_pointer_index,
            flags: child.flags,
            bone_id: child.bone_id,
            undamaged_entity,
            damaged_entity,
            continuous_eventset: child.continuous_eventset.clone(),
            collision_eventset: child.collision_eventset.clone(),
            break_eventset: child.break_eventset.clone(),
            break_from_root_eventset: child.break_from_root_eventset.clone(),
            __carry: child.__carry.clone(),
        }));
    }
    Ok(FragPhysicsLOD {
        base: lod.base.clone(),
        smallest_ang_inertia: lod.smallest_ang_inertia,
        largest_ang_inertia: lod.largest_ang_inertia,
        min_move_force: lod.min_move_force,
        body_type: lod.body_type.clone(),
        min_breaking_impulses: lod.min_breaking_impulses.clone(),
        root_cg_offset: lod.root_cg_offset.clone(),
        original_root_cg_offset: lod.original_root_cg_offset.clone(),
        unbroken_cg_offset: lod.unbroken_cg_offset.clone(),
        damping_constant: lod.damping_constant.clone(),
        group_names: lod.group_names.clone(),
        groups: lod.groups.clone(),
        children,
        phys_damp_undamaged: lod.phys_damp_undamaged.clone(),
        phys_damp_damaged: lod.phys_damp_damaged.clone(),
        composite_bounds: lod.composite_bounds.clone(),
        undamaged_ang_inertia: lod.undamaged_ang_inertia.clone(),
        damaged_ang_inertia: lod.damaged_ang_inertia.clone(),
        link_attachments: lod.link_attachments.clone(),
        self_collision_a: lod.self_collision_a.clone(),
        self_collision_b: lod.self_collision_b.clone(),
        root_group_count: lod.root_group_count,
        num_root_damage_regions: lod.num_root_damage_regions,
        num_bony_children: lod.num_bony_children,
        __carry: lod.__carry.clone(),
    })
}

fn frag_drawable_opt<A: DrawableTarget, B: DrawableTarget>(
    d: Option<&FragDrawable<A>>,
    at: Part,
    to: &mut impl Leaves<A, B>,
) -> Result<Option<Box<FragDrawable<B>>>, Error> {
    Ok(match d {
        Some(d) => Some(Box::new(frag_drawable(d, at, to)?)),
        None => None,
    })
}

fn frag_drawable<A: DrawableTarget, B: DrawableTarget>(
    d: &FragDrawable<A>,
    at: Part,
    to: &mut impl Leaves<A, B>,
) -> Result<FragDrawable<B>, Error> {
    Ok(FragDrawable {
        base: within(at, to, |to| drawable(&d.base, to))?,
        bound_matrix: d.bound_matrix.clone(),
        bound: d.bound.clone(),
        extra_bounds: d.extra_bounds.clone(),
        extra_bounds_matrices: d.extra_bounds_matrices.clone(),
        num_extra_bounds: d.num_extra_bounds,
        load_skeleton: d.load_skeleton,
        skeleton_type_name: d.skeleton_type_name.clone(),
        __carry: d.__carry.clone(),
    })
}

fn shader_group<A: DrawableTarget, B: DrawableTarget>(
    g: &GrmShaderGroup<A>,
    to: &mut impl Leaves<A, B>,
) -> Result<GrmShaderGroup<B>, Error> {
    let texture_dictionary = match g.texture_dictionary.as_deref() {
        None => None,
        Some(d) => Some(Box::new(dictionary(d, |_, t| to.texture(t))?)),
    };
    let mut shaders = Vec::with_capacity(g.shaders.len());
    for (i, s) in g.shaders.iter().enumerate() {
        shaders.push(Box::new(to.shader(s, &Spot::Shader(i))?));
    }
    Ok(GrmShaderGroup {
        base: g.base.clone(),
        texture_dictionary,
        shaders,
        container_size_qw: g.container_size_qw,
        has_instanced: g.has_instanced,
        __carry: g.__carry.clone(),
    })
}

/// `lod`, the level of detail at index `at`, or the copy of the first a drawable keeps apart where `None`.
fn level<A: DrawableTarget, B: DrawableTarget>(
    lod: &RmcLod<A>,
    at: Option<usize>,
    to: &mut impl Leaves<A, B>,
) -> Result<RmcLod<B>, Error> {
    let mut models = Vec::with_capacity(lod.models.len());
    for (m, model) in lod.models.iter().enumerate() {
        let mut geometries = Vec::with_capacity(model.geometries.len());
        for (g, geometry) in model.geometries.iter().enumerate() {
            let at = Spot::Geometry {
                lod: at,
                model: m,
                geometry: g,
            };
            geometries.push(Box::new(self::geometry(geometry, &at, to)?));
        }
        models.push(Box::new(GrmModel {
            base: model.base.clone(),
            geometries,
            aabbs: model.aabbs.clone(),
            shader_indices: model.shader_indices.clone(),
            matrix_count: model.matrix_count,
            flags: model.flags,
            geom_type: model.geom_type,
            matrix_index: model.matrix_index,
            mask: model.mask,
            skin_flag_and_tess: model.skin_flag_and_tess,
            __carry: model.__carry.clone(),
        }));
    }
    Ok(RmcLod {
        models,
        __carry: lod.__carry.clone(),
    })
}

fn geometry<A: DrawableTarget, B: DrawableTarget>(
    g: &GrmGeometry<A>,
    at: &Spot,
    to: &mut impl Leaves<A, B>,
) -> Result<GrmGeometry<B>, Error> {
    if matches!(g.vertex_data, Shared::Own(_)) {
        to.report().drop(at, Word::VertexData);
    }
    let vertex_buffer = to.vertex_buffer(&g.vertex_buffer, at)?;
    let stride = vertex_buffer.vertex_stride();
    let mut out = GrmGeometry {
        base: g.base.clone(),
        geom_type: g.geom_type,
        vertex_buffer: Box::new(vertex_buffer),
        index_buffer: Box::new(to.index_buffer(&g.index_buffer, at)),
        primitive_type: g.primitive_type,
        double_buffered: g.double_buffered,
        matrix_palette: g.matrix_palette.clone(),
        extra_vertices: g.extra_vertices,
        vertex_data: Shared::Same,
        index_offset: g.index_offset,
        __carry: g.__carry.clone(),
    };
    // ⚠ A dropped channel narrows the vertex, and the carried stride would contradict it at save.
    if stride != g.vertex_buffer.vertex_stride() {
        let stride = u8::try_from(stride)
            .map_err(|_| refused(format!("{at}: vertex_stride"), format!("{stride} bytes")))?;
        (out.__carry).hide(offset_of!(Raw<GrmGeometry<B>>, vertex_stride), stride);
    }
    Ok(out)
}

/// Where a Legacy channel sits in an Enhanced declaration, and the format that reads its bytes as Legacy does.
fn gen9_element(
    channel: GrcFvfChannel,
    size: GrcDataSize,
) -> Option<(usize, VertexElementFormatG9)> {
    use VertexElementFormatG9 as F;
    let slot = (G9_SLOT_TO_GEN8_SEMANTIC.iter()).position(|s| *s == Some(channel as u8))?;
    let format = [
        F::R32G32B32A32Float,
        F::R32G32B32Float,
        F::R16G16B16A16Float,
        F::R32G32Float,
        F::R10G10B10A2,
        F::R8G8B8A8Unorm,
        F::R8G8B8A8Uint,
        F::R16G16Float,
    ]
    .into_iter()
    .find(|f| f.gen8_data_size() == Some(size))?;
    Some((slot, format))
}

/// Every vertex `reader` addresses, each the concatenation of `channels` in order.
fn interleave(
    reader: &VertexReader<'_>,
    channels: &[GrcFvfChannel],
    stride: usize,
    at: &Spot,
) -> Result<Vec<u8>, Error> {
    let mut data = Vec::with_capacity(stride * reader.count());
    for v in 0..reader.count() {
        for &c in channels {
            let raw = reader.raw(c, v).ok_or_else(|| {
                refused(
                    format!("{at}: vertex_buffer"),
                    format!("vertex {v}'s {} lies past the buffer", c.name()),
                )
            })?;
            data.extend_from_slice(raw);
        }
    }
    Ok(data)
}

fn keyed<T, U>(k: &Keyed<T>) -> Keyed<U> {
    k.key.map_or_else(Keyed::default, Keyed::to)
}

/// A shader parameter's kind, in the low two bits of [`ShaderParamInfo::data`].
const TEXTURE: u32 = 0;
const CONSTANT: u32 = 3;
/// The widest offset or length a constant's info states, in twelve bits.
const CONSTANT_SPAN: usize = 0xFFF;
/// The view dimension every Enhanced 2D texture states.
const VIEW_TEXTURE_2D: u16 = 0x41;

fn info(name: u32, data: u32) -> ShaderParamInfo {
    ShaderParamInfo {
        name: Hash32::from_raw(name),
        data,
        ..Default::default()
    }
}

/// The payload's mip chain; bytes past it that are not zero are named.
fn chain(
    at: &Spot,
    format: GrcTextureFormat,
    (width, height, levels): (u16, u16, u8),
    payload: &[u8],
    report: &mut LossReport,
) -> Result<Vec<u8>, Error> {
    let size = format.total_size(
        u32::from(width),
        u32::from(height),
        u32::from(levels.max(1)),
    );
    let size = size as usize;
    let Some(pixels) = payload.get(..size) else {
        return Err(refused(
            at.to_string(),
            format!(
                "{} payload bytes, and its mip chain takes {size}",
                payload.len()
            ),
        ));
    };
    report.drop_if(
        at,
        Word::PayloadPastMipChain,
        payload[size..].iter().any(|b| *b != 0),
    );
    Ok(pixels.to_vec())
}

struct ToGen9<'a> {
    remap: &'a ParamRemap,
    report: LossReport,
}

impl<'a> ToGen9<'a> {
    fn new(remap: &'a ParamRemap) -> Self {
        Self {
            remap,
            report: LossReport::default(),
        }
    }

    fn slot(
        &mut self,
        slot: &TextureSlot<GrcTexture, GrcTextureReference>,
        at: &Spot,
    ) -> TextureSlot<GrcTextureG9, GrcTextureReferenceG9> {
        let r = match slot {
            TextureSlot::Unbound => return TextureSlot::Unbound,
            TextureSlot::Embedded(k) => return TextureSlot::Embedded(keyed(k)),
            TextureSlot::Reference(r) => r,
        };
        let field = |word| Lost::Reference(r.name.clone(), word);
        let report = &mut self.report;
        report.drop_if(at, field(Word::Texture), r.texture != Default::default());
        report.drop_if(
            at,
            field(Word::ResourceTypeAndConversionFlags),
            r.resource_type_and_conversion_flags != gen8::REFERENCE,
        );
        report.drop_if(at, field(Word::LayerCount), r.layer_count != 0);
        report.drop_if(
            at,
            field(Word::PhysicalSizeAndTemplateType),
            r.physical_size_and_template_type != 0,
        );
        report.drop_if(at, field(Word::HandleIndex), r.handle_index != 0);
        report.drop_if(at, field(Word::ExtraFlags), r.extra_flags != 0);
        TextureSlot::Reference(Box::new(GrcTextureReferenceG9 {
            name: r.name.clone(),
            ..Default::default()
        }))
    }
}

impl Leaves<Gen8, Gen9> for ToGen9<'_> {
    fn vertex_buffer(
        &mut self,
        vb: &gen8::GrcVertexBuffer,
        at: &Spot,
    ) -> Result<GrcVertexBufferG9, Error> {
        let (Some(fvf), Some(reader)) = (vb.fvf.as_deref(), VertexReader::new(vb)) else {
            return Err(refused(
                format!("{at}: vertex_buffer"),
                "it states no FVF, so its bytes have no channels to lay out",
            ));
        };
        let mut elements = Vec::new();
        for (channel, slot) in reader.layout().present() {
            match gen9_element(channel, slot.size) {
                Some((s, format)) => elements.push((s, format, channel)),
                None => self.report.drop(at, Lost::Channel(channel, slot.size)),
            }
        }
        // Enhanced interleaves a vertex in slot order, which is not Legacy's channel order.
        elements.sort_by_key(|e| e.0);
        let mut info = VertexDeclaration::default();
        let mut stride = 0usize;
        for &(s, format, _) in &elements {
            info.offsets[s] = stride as u32;
            info.types[s] = format.to_raw();
            stride += format.byte_size().unwrap_or(0);
        }
        let step = u8::try_from(stride).map_err(|_| {
            refused(
                format!("{at}: vertex_buffer"),
                format!("a {stride}-byte vertex, and a declaration states a byte"),
            )
        })?;
        for &(s, ..) in &elements {
            info.sizes[s] = step;
        }
        // Bits 2 to 9 are the vertex size; the flags and the count beside it are zero in every buffer shipped.
        info.data = u64::from(step) << 2;
        let channels: Vec<GrcFvfChannel> = elements.iter().map(|e| e.2).collect();
        let data = interleave(&reader, &channels, stride, at)?;

        let absent = (GrcFvfChannel::ALL.iter())
            .filter(|c| fvf.fvf & c.mask() == 0)
            .fold(0u64, |m, &c| m | 0xF << (c as u64 * 4));
        let report = &mut self.report;
        report.drop_if(at, Word::VertexBufferFlags, vb.flags != 0);
        report.drop_if(at, Word::LockPtr, matches!(vb.lock_ptr, Shared::Own(_)));
        report.drop_if(
            at,
            Word::FvfPastSixteen,
            fvf.fvf >> GrcFvfChannel::COUNT != 0,
        );
        report.drop_if(at, Word::FvfFlags, fvf.flags != 0);
        report.drop_if(at, Word::DynamicOrder, fvf.dynamic_order != 0);
        report.drop_if(at, Word::ChannelSizes, fvf.channel_sizes & absent != 0);
        Ok(GrcVertexBufferG9 {
            data,
            info: Some(Box::new(info)),
            ..Default::default()
        })
    }

    fn index_buffer(&mut self, ib: &GrcIndexBuffer, at: &Spot) -> GrcIndexBufferG9 {
        let stated = (ib.__carry)
            .hidden::<u32>(offset_of!(Raw<GrcIndexBuffer>, index_count))
            .copied()
            .unwrap_or(0);
        self.report
            .drop_if(at, Word::IndexCountTopByte, stated >> 24 != 0);
        GrcIndexBufferG9 {
            indices: ib.indices.clone(),
            index_size: std::mem::size_of::<u16>() as u16,
            ..Default::default()
        }
    }

    fn shader(&mut self, s: &GrcInstanceData, at: &Spot) -> Result<GrcInstanceDataG9, Error> {
        let shader = s.basis_hash.get();
        let mut params = Params::default();
        let (mut infos, mut buffer) = (Vec::new(), Vec::new());
        for e in &s.entries {
            let name = e.name_hash.get();
            let Some(hash) =
                (self.remap).translate(shader, name, Generation::Gen8, Generation::Gen9)
            else {
                let texture = matches!(e.data, EntryData::Texture(_));
                self.report.drop(
                    at,
                    Lost::Entry {
                        hash: name,
                        texture,
                    },
                );
                continue;
            };
            match &e.data {
                EntryData::Texture(slot) => {
                    let data = (params.textures.len() as u32) << 2 | TEXTURE;
                    params.textures.push(self.slot(slot, at));
                    infos.push(info(hash, data));
                }
                EntryData::Float(rows) => {
                    let offset = buffer.len();
                    buffer.extend(
                        (rows.iter())
                            .flat_map(|r| [r.x, r.y, r.z, r.w])
                            .flat_map(f32::to_le_bytes),
                    );
                    if buffer.len() > CONSTANT_SPAN {
                        return Err(refused(
                            format!("{at}: entry {name:#010x}"),
                            format!(
                                "the constants end at byte {}, past what twelve bits address",
                                buffer.len()
                            ),
                        ));
                    }
                    let len = buffer.len() - offset;
                    infos.push(info(
                        hash,
                        CONSTANT | (offset as u32) << 8 | (len as u32) << 20,
                    ));
                }
            }
        }
        if !buffer.is_empty() {
            params.buffers.push(buffer);
        }

        let report = &mut self.report;
        report.drop_if(at, Word::Registers, !s.entries.is_empty());
        report.drop_if(at, Word::MaterialHash, s.material_hash.get() != 0);
        report.drop_if(at, Word::BasisHashCode, s.basis_hash_code != 0);
        report.drop_if(at, Word::MaterialHashCode, s.material_hash_code != 0);
        report.drop_if(at, Word::PhysMtlDeprecated, s.phys_mtl_deprecated != 0);
        report.drop_if(at, Word::Flags, s.flags != 0);
        report.drop_if(at, Word::IsInstanced, s.is_instanced != 0);
        report.drop_if(at, Word::UserFlags, s.user_flags != 0);
        report.drop_if(at, Word::SortKeyDeprecated, s.sort_key_deprecated != 0);
        Ok(GrcInstanceDataG9 {
            name: s.basis_hash,
            params,
            param_infos: Box::new(ShaderParamInfos {
                params: infos,
                ..Default::default()
            }),
            draw_bucket: s.draw_bucket,
            draw_bucket_mask: s.draw_bucket_mask,
            ..Default::default()
        })
    }

    fn texture(&mut self, t: &GrcTexture) -> Result<GrcTextureG9, Error> {
        let at = Spot::Texture(t.name.clone());
        if t.image_type != 0 || t.depth > 1 || t.layer_count != 0 || t.cut_mip_levels != 0 {
            return Err(refused(
                at.to_string(),
                format!(
                    "only a plain 2D texture retargets, and this states image type {}, depth {}, \
                     {} layers and {} cut mip levels",
                    t.image_type, t.depth, t.layer_count, t.cut_mip_levels
                ),
            ));
        }
        let format = t.pixel_format();
        let dxgi = format.to_dxgi(t.is_srgb());
        let Some(raw) = u8::try_from(dxgi.to_raw())
            .ok()
            .filter(|_| dxgi != DxgiFormat::Unknown)
        else {
            return Err(refused(
                at.to_string(),
                format!("{format:?} has no Enhanced pixel format"),
            ));
        };
        let size = (t.width, t.height, t.mip_count);
        let pixels = chain(&at, format, size, &t.backing_store, &mut self.report)?;

        let report = &mut self.report;
        report.drop_if(&at, Word::Texture, t.texture != Default::default());
        report.drop_if(
            &at,
            Word::ResourceTypeAndConversionFlags,
            t.resource_type_and_conversion_flags != 0,
        );
        report.drop_if(&at, Word::HandleIndex, t.handle_index != 0);
        report.drop_if(&at, Word::ExtraFlags, t.extra_flags != 0);
        report.drop_if(&at, Word::TemplateType, t.template_type() != 0);
        report.drop_if(&at, Word::InfoBitsPastSrgb, t.info_bits & !1 != 0);
        report.drop_if(&at, Word::StereoRtMode, t.stereo_rt_mode != 0);
        report.drop_if(
            &at,
            Word::MipStride,
            u32::from(t.mip_stride) != format.legacy_mip_stride(u32::from(t.width)),
        );
        // A 2D untiled texture's words, as `GrcTextureG9::from_png` states them.
        let mut out = GrcTextureG9 {
            name: t.name.clone(),
            width: t.width,
            height: t.height,
            depth: 1,
            dimension: Open::new(TextureDimension::Texture2D),
            format: Open::new(<PixelFormat as OpenEnum>::from_raw(raw)),
            tile_mode: Open::new(TileMode::Auto),
            mip_levels: t.mip_count,
            pixels,
            view: ShaderResourceViewG9 {
                view_dimension: VIEW_TEXTURE_2D,
                ..Default::default()
            },
            ..Default::default()
        };
        out.set_block_stride(format.bytes_per_block());
        Ok(out)
    }

    fn report(&mut self) -> &mut LossReport {
        &mut self.report
    }
}

struct ToGen8<'a> {
    remap: &'a ParamRemap,
    report: LossReport,
}

impl<'a> ToGen8<'a> {
    fn new(remap: &'a ParamRemap) -> Self {
        Self {
            remap,
            report: LossReport::default(),
        }
    }

    fn slot(
        &mut self,
        slot: &TextureSlot<GrcTextureG9, GrcTextureReferenceG9>,
        at: &Spot,
    ) -> TextureSlot<GrcTexture, GrcTextureReference> {
        let r = match slot {
            TextureSlot::Unbound => return TextureSlot::Unbound,
            TextureSlot::Embedded(k) => return TextureSlot::Embedded(keyed(k)),
            TextureSlot::Reference(r) => r,
        };
        let field = |word| Lost::Reference(r.name.clone(), word);
        let report = &mut self.report;
        report.drop_if(at, field(Word::Flags), r.flags != 0);
        report.drop_if(at, field(Word::Width), r.width != 0);
        report.drop_if(at, field(Word::Height), r.height != 0);
        report.drop_if(at, field(Word::Depth), r.depth != 0);
        report.drop_if(at, field(Word::Dimension), r.dimension.raw() != 0);
        report.drop_if(at, field(Word::Format), r.format.raw() != 0);
        report.drop_if(at, field(Word::TileMode), r.tile_mode.raw() != 0);
        report.drop_if(at, field(Word::AntiAliasType), r.anti_alias_type != 0);
        report.drop_if(at, field(Word::MipLevels), r.mip_levels != 0);
        report.drop_if(at, field(Word::UsageData), r.usage_data != 0);
        TextureSlot::Reference(Box::new(GrcTextureReference::named(r.name.clone())))
    }
}

impl Leaves<Gen9, Gen8> for ToGen8<'_> {
    fn vertex_buffer(
        &mut self,
        vb: &GrcVertexBufferG9,
        at: &Spot,
    ) -> Result<gen8::GrcVertexBuffer, Error> {
        let (Some(declaration), Some(reader)) = (vb.info.as_deref(), VertexReader::new(vb)) else {
            return Err(refused(
                format!("{at}: vertex_buffer"),
                "it states no declaration, so its bytes have no channels to lay out",
            ));
        };
        let d = declaration;
        for e in elements_g9(&d.offsets, &d.sizes, &d.types) {
            let carried = (e.gen8_semantic().and_then(GrcFvfChannel::from_slot)).is_some()
                && e.format.gen8_data_size().is_some();
            self.report
                .drop_if(at, Lost::Element(e.slot, e.format), !carried);
        }
        let (mut fvf, mut sizes, mut stride, mut channels) = (0u32, 0u64, 0usize, Vec::new());
        for (channel, slot) in reader.layout().present() {
            fvf |= channel.mask();
            sizes |= (slot.size as u64) << (channel as u64 * 4);
            stride += slot.size.byte_size();
            channels.push(channel);
        }
        let fvf_size = u8::try_from(stride).map_err(|_| {
            refused(
                format!("{at}: vertex_buffer"),
                format!("a {stride}-byte vertex, and an FVF states a byte"),
            )
        })?;
        let data = interleave(&reader, &channels, stride, at)?;

        let report = &mut self.report;
        report.drop_if(at, Word::VertexBindFlags, vb.bind_flags != 0);
        report.drop_if(at, Word::VertexSrv, vb.srv.is_some());
        report.drop_if(at, Word::InfoData, d.data & !(0xFF << 2) != 0);
        Ok(gen8::GrcVertexBuffer {
            lock_ptr: Shared::Same,
            data,
            fvf: Some(Box::new(GrcFvf {
                fvf,
                fvf_size,
                channel_count: channels.len() as u8,
                channel_sizes: sizes,
                ..Default::default()
            })),
            ..Default::default()
        })
    }

    fn index_buffer(&mut self, ib: &GrcIndexBufferG9, at: &Spot) -> GrcIndexBuffer {
        let report = &mut self.report;
        report.drop_if(
            at,
            Word::IndexSize,
            usize::from(ib.index_size) != std::mem::size_of::<u16>(),
        );
        report.drop_if(at, Word::IndexBindFlags, ib.bind_flags != 0);
        report.drop_if(at, Word::IndexSrv, ib.srv.is_some());
        GrcIndexBuffer {
            indices: ib.indices.clone(),
            ..Default::default()
        }
    }

    fn shader(&mut self, s: &GrcInstanceDataG9, at: &Spot) -> Result<GrcInstanceData, Error> {
        type Entry = GrcInstanceDataEntry<TextureSlot<GrcTexture, GrcTextureReference>>;
        let shader = s.name.get();
        let (mut textures, mut constants): (Vec<Entry>, Vec<Entry>) = (Vec::new(), Vec::new());
        let mut named = vec![false; s.params.textures.len()];
        for p in &s.param_infos.params {
            let name = p.name.get();
            let def = self.remap.resolve(shader, name, Generation::Gen9);
            if let Some(slot) = p.texture() {
                let (Some(def), Some(bound)) =
                    (def.filter(|d| d.is_binding()), s.params.textures.get(slot))
                else {
                    self.report.drop(at, Lost::TextureParameter(name));
                    continue;
                };
                named[slot] = true;
                let mut entry = Entry::declared(def);
                entry.data = EntryData::Texture(self.slot(bound, at));
                textures.push(entry);
            } else if let Some((buffer, range)) = p.constant() {
                let bytes = (s.params.buffers.get(buffer)).and_then(|b| b.get(range));
                let (Some(def), Some(bytes)) = (def.filter(|d| !d.is_binding()), bytes) else {
                    self.report.drop(at, Lost::Constant(name));
                    continue;
                };
                if bytes.len() % 4 != 0 {
                    self.report.drop(at, Lost::Uneven(name, bytes.len()));
                    continue;
                }
                let floats: Vec<f32> = (bytes.as_chunks::<4>().0.iter())
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                // A register's lanes past the parameter's width read zero.
                let rows = (floats.chunks(4))
                    .map(|r| {
                        let lane = |i: usize| r.get(i).copied().unwrap_or(0.0);
                        Vec4 {
                            x: lane(0),
                            y: lane(1),
                            z: lane(2),
                            w: lane(3),
                        }
                    })
                    .collect();
                let mut entry = Entry::declared(def);
                entry.data = EntryData::Float(rows);
                constants.push(entry);
            } else if p.sampler().is_some() {
                self.report.drop(at, Lost::Sampler(name));
            } else {
                self.report.drop(at, Lost::Other(name));
            }
        }
        for (slot, t) in s.params.textures.iter().enumerate() {
            self.report.drop_if(
                at,
                Lost::Slot(slot),
                !named[slot] && !matches!(t, TextureSlot::Unbound),
            );
        }

        let report = &mut self.report;
        report.drop_if(at, Word::Preset, s.preset.get() != 0);
        report.drop_if(at, Word::Samplers, !s.params.samplers.is_empty());
        report.drop_if(at, Word::Others, s.params.others != 0);
        report.drop_if(at, Word::BufferCopies, s.param_infos.buffer_copies != 0);
        // The engine reads a Legacy material's textures as a leading run.
        textures.extend(constants);
        Ok(GrcInstanceData {
            entries: textures,
            basis_hash: s.name,
            draw_bucket: s.draw_bucket,
            draw_bucket_mask: s.draw_bucket_mask,
            ..Default::default()
        })
    }

    fn texture(&mut self, t: &GrcTextureG9) -> Result<GrcTexture, Error> {
        let at = Spot::Texture(t.name.clone());
        let untiled = matches!(t.tile_mode.get(), TileMode::Auto | TileMode::Linear);
        if !untiled || t.dimension.get() != TextureDimension::Texture2D || t.depth > 1 {
            return Err(refused(
                at.to_string(),
                format!(
                    "only an untiled 2D texture retargets, and this states {:?}, {:?} and depth {}",
                    t.dimension, t.tile_mode, t.depth
                ),
            ));
        }
        let (format, srgb) =
            GrcTextureFormat::from_dxgi(DxgiFormat::from_raw(u32::from(t.format.raw())));
        if format == GrcTextureFormat::None {
            return Err(refused(
                at.to_string(),
                format!("{:?} has no Legacy format", t.format),
            ));
        }
        let size = (t.width, t.height, t.mip_levels);
        let mut backing_store = chain(&at, format, size, &t.pixels, &mut self.report)?;
        // The masked word states only whole multiples of the mask's lowest bit.
        let unit = (PAYLOAD_SIZE_MASK & PAYLOAD_SIZE_MASK.wrapping_neg()) as usize;
        backing_store.resize(backing_store.len().next_multiple_of(unit), 0);
        let mip_stride =
            u16::try_from(format.legacy_mip_stride(u32::from(t.width))).map_err(|_| {
                refused(
                    at.to_string(),
                    format!("a {}-texel row does not fit a 16-bit pitch", t.width),
                )
            })?;
        let after_mip_levels = (t.__carry)
            .hidden::<u8>(offset_of!(Raw<GrcTextureG9>, after_mip_levels))
            .copied()
            .unwrap_or(0);

        let report = &mut self.report;
        report.drop_if(&at, Word::Flags, t.flags != 0);
        report.drop_if(&at, Word::AntiAliasType, t.anti_alias_type != 0);
        report.drop_if(&at, Word::AfterMipLevels, after_mip_levels != 0);
        report.drop_if(&at, Word::UsageData, t.usage_data != 0);
        Ok(GrcTexture {
            name: t.name.clone(),
            width: t.width,
            height: t.height,
            depth: 1,
            mip_stride,
            format: format.to_raw_format(),
            mip_count: t.mip_levels,
            info_bits: u8::from(srgb),
            backing_store,
            ..Default::default()
        })
    }

    fn report(&mut self) -> &mut LossReport {
        &mut self.report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl::File;
    use crate::gta5::shaders::shader_defs::{
        hash_name, ShaderDef, ShaderDefinitions, ShaderParamDef, ShaderParamKind,
    };

    fn param(name: &str, semantic: &str, kind: ShaderParamKind) -> ShaderParamDef {
        ShaderParamDef {
            name: name.into(),
            name_hash: hash_name(name),
            kind,
            array_len: 1,
            register: 0,
            cbuffer: None,
            drawable_place: None,
            semantic: Some(semantic.into()),
            semantic_hash: Some(hash_name(semantic)),
            ui_name: None,
            default: None,
            preset_default: None,
        }
    }

    fn remap() -> ParamRemap {
        let mut defs = ShaderDefinitions::new("gta5", "gen8");
        defs.shaders.push(ShaderDef {
            name: "default".into(),
            name_hash: hash_name("default"),
            effect: "default".into(),
            draw_bucket: None,
            vertex_type: 0,
            traits: Vec::new(),
            parameters: vec![
                param("DiffuseSampler", "DiffuseTex", ShaderParamKind::Texture),
                param(
                    "specularIntensityMult",
                    "SpecularColor",
                    ShaderParamKind::Float,
                ),
            ],
        });
        ParamRemap::of(&defs)
    }

    /// A triangle whose vertices carry a position and a seventh texture coordinate, which Enhanced has no slot for.
    fn legacy() -> GtaDrawable<Gen8> {
        let (position, tex7) = (GrcFvfChannel::Position, GrcFvfChannel::Tex7);
        let fvf = GrcFvf {
            fvf: position.mask() | tex7.mask(),
            fvf_size: 20,
            channel_count: 2,
            channel_sizes: (GrcDataSize::Float3 as u64) << (position as u64 * 4)
                | (GrcDataSize::Float2 as u64) << (tex7 as u64 * 4),
            ..Default::default()
        };
        let data = (0..15u8).flat_map(|i| f32::from(i).to_le_bytes()).collect();
        let mut geometry = GrmGeometry::<Gen8> {
            vertex_buffer: Box::new(gen8::GrcVertexBuffer {
                lock_ptr: Shared::Same,
                data,
                fvf: Some(Box::new(fvf)),
                ..Default::default()
            }),
            index_buffer: Box::new(GrcIndexBuffer {
                indices: vec![0, 1, 2],
                ..Default::default()
            }),
            primitive_type: 3,
            vertex_data: Shared::Same,
            ..Default::default()
        };
        // As an opened file states its stride, which the dropped channel narrows.
        let stride = offset_of!(Raw<GrmGeometry<Gen8>>, vertex_stride);
        geometry.__carry.hide(stride, 20u8);
        geometry.__carry.__keep_derived(stride, Some(20u8));
        let shader = GrcInstanceData {
            entries: vec![
                GrcInstanceDataEntry {
                    register: 0,
                    name_hash: Hash32::of("DiffuseSampler"),
                    data: EntryData::Texture(TextureSlot::Reference(Box::new(
                        GrcTextureReference::named("shared_diffuse"),
                    ))),
                },
                GrcInstanceDataEntry {
                    register: 1,
                    name_hash: Hash32::of("specularIntensityMult"),
                    data: EntryData::Float(vec![Vec4 {
                        x: 0.5,
                        ..Default::default()
                    }]),
                },
            ],
            basis_hash: Hash32::of("default"),
            material_hash: Hash32::of("default.sps"),
            ..Default::default()
        };
        let mut base = RmcDrawable::<Gen8> {
            shader_group: Some(Box::new(GrmShaderGroup {
                shaders: vec![Box::new(shader)],
                ..Default::default()
            })),
            ..Default::default()
        };
        base.lod_group.lods[0] = NullIsEmpty::new(RmcLod {
            models: vec![Box::new(GrmModel {
                geometries: vec![Box::new(geometry)],
                aabbs: vec![Default::default()],
                shader_indices: vec![0],
                ..Default::default()
            })],
            ..Default::default()
        });
        GtaDrawable {
            base,
            ..Default::default()
        }
    }

    fn one<T: Owned>(t: T) -> PgDictionary<T>
    where
        Raw<T>: Record,
    {
        PgDictionary {
            codes: vec![0x1234],
            entries: vec![Some(Box::new(t))],
            ..Default::default()
        }
    }

    #[test]
    fn a_dropped_value_prints_as_the_command_reports_it() {
        let at = |drawable, spot| Place { drawable, spot };
        let geometry = |lod| Spot::Geometry {
            lod,
            model: 1,
            geometry: 2,
        };
        for (dropped, printed) in [
            (
                Dropped {
                    at: at(Some(Part::Entry(0x1234)), Spot::Shader(2)),
                    lost: Lost::Entry {
                        hash: 0x1f8a49b7,
                        texture: true,
                    },
                },
                "drawable 0x00001234/shader 2: entry 0x1f8a49b7, \
                 which the shader database does not spell for Enhanced",
            ),
            (
                Dropped {
                    at: at(
                        Some(Part::Child {
                            level: 0,
                            child: 3,
                            damaged: true,
                        }),
                        geometry(Some(0)),
                    ),
                    lost: Word::ChannelSizes.into(),
                },
                "physics level 0, child 3 damaged/level0/model1/geometry2: \
                 vertex_buffer.fvf.channel_sizes of the channels it does not carry",
            ),
            (
                Dropped {
                    at: at(None, geometry(None)),
                    lost: Lost::Channel(GrcFvfChannel::Tex1, GrcDataSize::Half2),
                },
                "container/model1/geometry2: vertex channel tex1 (Half2)",
            ),
            (
                Dropped {
                    at: at(Some(Part::Extra(1)), Spot::Shader(0)),
                    lost: Lost::Reference("diffuse".into(), Word::Flags),
                },
                "extra drawable 1/shader 0: reference diffuse: flags",
            ),
            (
                Dropped {
                    at: at(None, Spot::Texture("road".into())),
                    lost: Word::PayloadPastMipChain.into(),
                },
                "texture road: payload bytes past the mip chain",
            ),
        ] {
            assert_eq!(dropped.to_string(), printed);
        }
    }

    #[test]
    fn a_texture_the_other_target_cannot_describe_refuses_the_whole_file_by_name() {
        let array = GrcTexture {
            name: "lights".into(),
            width: 4,
            height: 4,
            mip_count: 1,
            layer_count: 5,
            ..Default::default()
        };
        let e = texture_dictionary_to_gen9(&one(array.clone())).unwrap_err();
        assert!(e.to_string().starts_with("texture lights: "), "{e}");

        let mut drawable = legacy();
        let group = drawable.base.shader_group.as_deref_mut().unwrap();
        group.texture_dictionary = Some(Box::new(one(array)));
        let e = drawable_dictionary_to_gen9(&one(drawable), &remap()).unwrap_err();
        assert!(
            e.to_string()
                .starts_with("drawable 0x00001234/texture lights: "),
            "{e}"
        );

        let tiled = GrcTextureG9 {
            name: "road".into(),
            width: 4,
            height: 4,
            depth: 1,
            dimension: Open::new(TextureDimension::Texture2D),
            tile_mode: Open::new(TileMode::Standard),
            ..Default::default()
        };
        let e = texture_dictionary_to_gen8(&one(tiled)).unwrap_err();
        assert!(e.to_string().starts_with("texture road: "), "{e}");
    }

    /// ⚠ A Legacy `mip_stride` is a row of texels' bytes, a quarter of a compressed format's row of blocks.
    #[test]
    fn a_texture_keeps_the_mip_stride_every_shipped_one_states_through_both_targets() {
        let format = GrcTextureFormat::DXT1;
        let stride = format.legacy_mip_stride(16);
        assert_ne!(
            stride,
            format.row_pitch(16),
            "a compressed row of blocks would not tell the two readings apart"
        );
        let legacy = GrcTexture {
            name: "road".into(),
            width: 16,
            height: 16,
            depth: 1,
            mip_count: 1,
            mip_stride: u16::try_from(stride).unwrap(),
            format: format.to_raw_format(),
            backing_store: vec![0; format.total_size(16, 16, 1) as usize],
            ..Default::default()
        };

        let (enhanced, lost) = texture_dictionary_to_gen9(&one(legacy.clone())).unwrap();
        assert!(
            !lost.names("texture road", "mip_stride"),
            "a stride every shipped texture states is not a loss: {lost}"
        );
        let (back, _) = texture_dictionary_to_gen8(&enhanced).unwrap();
        let back = back.entries[0].as_deref().unwrap();
        assert_eq!(back.mip_stride, legacy.mip_stride);
    }

    #[test]
    fn what_enhanced_cannot_carry_is_named_and_dropped_and_the_rest_comes_back() {
        let remap = remap();
        let (enhanced, lost) = to_gen9(&legacy(), &remap).unwrap();
        assert!(lost.names("shader 0", "material_hash"), "{lost}");
        assert!(lost.names("level0/model0/geometry0", "tex7"), "{lost}");
        let enhanced =
            <GtaDrawable<Gen9> as File>::from_bytes(&enhanced.to_bytes().unwrap()).unwrap();

        let geometry = &enhanced.lod_group.lods[0].models[0].geometries[0];
        let reader = VertexReader::new(&*geometry.vertex_buffer).unwrap();
        assert!(!reader.layout().has(GrcFvfChannel::Tex7));
        let positions: Vec<[f32; 3]> = reader.positions().collect();
        assert_eq!(
            positions[1],
            [5.0, 6.0, 7.0],
            "the second vertex, past the dropped channel"
        );

        let shader = &enhanced.shader_group.as_deref().unwrap().shaders[0];
        let (buffer, range) = (shader.param(hash_name("SpecularColor")))
            .and_then(ShaderParamInfo::constant)
            .expect("the constant, under its Enhanced spelling");
        assert_eq!(
            shader.params.buffers[buffer][range][..4],
            0.5f32.to_le_bytes()
        );
        assert!(shader
            .param(hash_name("DiffuseTex"))
            .is_some_and(|p| p.texture().is_some()));

        let (back, _) = to_gen8(&enhanced, &remap).unwrap();
        let shader = &back.shader_group.as_deref().unwrap().shaders[0];
        assert_eq!(shader.material_hash.get(), 0, "dropped, never invented");
        let diffuse = shader.entry(hash_name("DiffuseSampler")).unwrap();
        assert!(matches!(
            &diffuse.data,
            EntryData::Texture(TextureSlot::Reference(r)) if r.name == "shared_diffuse"
        ));
        let specular = shader.entry(hash_name("specularIntensityMult")).unwrap();
        assert!(matches!(&specular.data, EntryData::Float(rows) if rows[0].x == 0.5));
    }
}
