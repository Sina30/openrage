//! The source of a `.ytd`: a folder of every texture's exact DDS and the rules that compile it back.
//!
//! A texture's source is, in order, the `pack` or the `source` its entry in `textures.json` names, an
//! image of its name beside the DDS, then the DDS, which is copied block for block. An image
//! made to the size of its DDS keeps every block it did not change.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use or_io::authoring::texture_import::{Depth, Levels};
use or_io::authoring::{
    automatic_format, encode_texture_with_options, texture_import, EncodeQuality,
    TextureBuildOptions,
};
use or_io::codec::texture::{
    decode_texture_data, format_of_dds, max_mip_levels, to_dds, AlphaRule, GrcTextureFormat,
    MipFilter, RgbaImage,
};
use or_io::container::ResourceEnvelope;
use or_io::dds::Dds;
use or_io::decl::{File, Open, OpenEnum};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::gen9::{
    GrcTextureG9, PixelFormat, ShaderResourceViewG9, TextureDimension, TileMode,
};
use or_io::gta5::texture_dict::gen8::{PAYLOAD_SIZE_MASK, PROCESSED};
use or_io::gta5::PgDictionary;
use or_io::{string_hash_str, Generation};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::asset::{folder_name, sha256_hex, unsafe_file_name, AssetManifest, How, Origin, Source};
use crate::header::{EnhancedHeader, LegacyHeader};
use crate::json::canonical;
use crate::reencode;
use crate::Problem;

/// The `kind` of a textures source.
pub const KIND: &str = "textures";
/// The file of per-texture rules, which a textures source always holds.
pub const RULES: &str = "textures.json";
/// The folder of DDS files and images inside a textures source.
pub const DIR: &str = "textures";
/// The longest side a texture states: its width and height are 16 bits.
const MAX_SIDE: u32 = u16::MAX as u32;
/// Whether `extension` is an image a texture compiles from, which is every format an import reads but a DDS.
fn is_image_extension(extension: &str) -> bool {
    texture_import::SourceFormat::from_extension(extension)
        .is_some_and(|format| format != texture_import::SourceFormat::Dds)
}

/// The extensions a texture compiles from besides `dds`, as a message lists them.
fn image_extensions() -> String {
    texture_import::SourceFormat::EXTENSIONS
        .into_iter()
        .filter(|e| is_image_extension(e))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `textures.json`: what a DDS cannot say and how an image becomes one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "textures.json")]
pub struct Rules {
    pub textures: Vec<Entry>,
}

/// One texture's rules; a texture with none still compiles from its files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    /// A file in `textures/`, named when the source is not the image or DDS of the texture's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Images whose channels make the texture, where the pixels are not one image or the DDS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<Pack>,
    /// The header words of the texture that a DDS cannot state, under the engine's names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "header_schema")]
    pub header: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode: Option<Encode>,
}

/// The image each channel of a texture is taken from, as a specular map is made of three measures
/// that artists author apart. A channel left out is 0, and an alpha left out is opaque.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r: Option<Taken>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub g: Option<Taken>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Taken>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Taken>,
}

impl Pack {
    /// Red, green, blue and alpha, each with its letter for a message.
    fn channels(&self) -> [(&'static str, Option<&Taken>); 4] {
        [
            ("r", self.r.as_ref()),
            ("g", self.g.as_ref()),
            ("b", self.b.as_ref()),
            ("a", self.a.as_ref()),
        ]
    }
}

/// One channel of one image in `textures/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Taken {
    pub file: String,
    pub channel: Channel,
}

/// A channel of an image, by the letter its tools use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    R,
    G,
    B,
    A,
}

/// How an image becomes a DDS; what is left out follows the DDS the image replaces.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Encode {
    /// A RAGE or D3D spelling, such as `DXT5` or `BC5`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mips: Option<Mips>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<Quality>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour_space: Option<ColourSpace>,
    /// Inverts the green channel of the image, for a normal map authored with green pointing up.
    /// Shipped maps point it down the image, so an image made from a DDS needs no flip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_green: Option<bool>,
    /// The part of a 16-bit image's range, or of floating-point light, that is kept as `[min, max]`: it is stretched over the
    /// 8 bits the game's formats hold and cut outside. A 16-bit value is a fraction of its range and light is itself. With
    /// none stated a 16-bit image is scaled over its full range and floating-point light (EXR, HDR) is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clamp: Option<[f32; 2]>,
    /// How floating-point light comes down to 8 bits in place of a clamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tonemap: Option<Tonemap>,
    /// Stops of exposure before the tone map: the light is multiplied by two to this power. Needs a `tonemap`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f32>,
    /// Whether the colour of the image is straight or already multiplied by its alpha; what is left out is straight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha: Option<AlphaMode>,
    /// The alpha a cutout is tested against, above 0 and at most 1: every mip keeps the share of texels that reach it at the top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_coverage: Option<f32>,
}

/// How the colour of an image relates to its alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AlphaMode {
    /// The colour is independent of the alpha, which is what an image is unless it says otherwise.
    Straight,
    /// The colour is multiplied by the alpha, as an exporter that premultiplies writes it, and every mip is stored so.
    Premultiplied,
}

/// The words of a Legacy header or of an Enhanced one, by the generation the source compiles to.
fn header_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "anyOf": [
            generator.subschema_for::<LegacyHeader>(),
            generator.subschema_for::<EnhancedHeader>(),
        ],
    })
}

/// A tone map for floating-point light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Tonemap {
    /// `x / (1 + x)`, which keeps what is brighter than 1 in the range.
    Reinhard,
}

impl Encode {
    /// The first rule stated that only means something to an image of more than 8 bits a channel.
    fn bit_depth_rule(&self) -> Option<&'static str> {
        [
            ("clamp", self.clamp.is_some()),
            ("tonemap", self.tonemap.is_some()),
            ("exposure", self.exposure.is_some()),
        ]
        .into_iter()
        .find_map(|(rule, stated)| stated.then_some(rule))
    }
}

/// Why a texture's pixels could not be made: a fault in the rules, or in the file they come from.
enum Fault {
    Rule(String),
    Input(String),
}

impl From<String> for Fault {
    fn from(why: String) -> Self {
        Self::Input(why)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Mips {
    /// Until the shorter side is four texels, where shipped textures stop.
    #[serde(rename = "to-4x4")]
    To4x4,
    #[serde(rename = "to-1x1")]
    To1x1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    Box,
    Lanczos3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Fast,
    Normal,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ColourSpace {
    Srgb,
    Linear,
}

/// What an extraction records about where its `.ytd` came from.
pub struct Extraction<'a> {
    pub from_game: bool,
    pub virtual_path: Option<&'a str>,
}

/// A compiled `.ytd`, named for the platform's extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

struct Part {
    name: String,
    dds: Vec<u8>,
    header: Value,
}

/// Writes `ytd` into `parent` as `<name>.textures.src` and returns the folder; nothing is left behind on a refusal.
pub fn extract(
    ytd: &[u8],
    name: &str,
    parent: &Path,
    how: &Extraction<'_>,
) -> Result<PathBuf, Vec<Problem>> {
    let target = target_of(ytd)?;
    let parts = if target == Generation::Gen9 {
        enhanced_parts(ytd)?
    } else {
        legacy_parts(ytd)?
    };
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    let mut problems = Vec::new();
    for part in &parts {
        if let Some(bad) = unsafe_file_name(&part.name) {
            problems.push(Problem::new(bad).of_texture(&part.name));
        }
        if let Some(first) = seen.insert(part.name.to_lowercase(), &part.name) {
            problems.push(
                Problem::new(format!(
                    "is named like {first:?} but for its case; a folder cannot hold both"
                ))
                .of_texture(&part.name),
            );
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }

    let origin = Origin {
        how: How::Extracted,
        from_game: how.from_game,
        virtual_path: how.virtual_path.map(str::to_string),
        game_build: None,
        sha256: Some(sha256_hex(ytd)),
        cooked_sha256: None,
        extra: BTreeMap::new(),
    };
    let manifest = AssetManifest::new(KIND, name, target.slug(), origin).map_err(|p| vec![p])?;
    let mut entries: Vec<Entry> = parts
        .iter()
        .map(|part| Entry {
            name: part.name.clone(),
            source: None,
            pack: None,
            header: Some(part.header.clone()),
            encode: None,
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    let rules = canonical(&Rules { textures: entries })
        .map_err(|e| vec![Problem::new(e.to_string()).in_file(RULES)])?;

    let dir = parent.join(folder_name(name, KIND));
    if dir.exists() {
        return Err(vec![Problem::new(format!(
            "{} already exists",
            dir.display()
        ))]);
    }
    let written = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(dir.join(DIR))?;
        std::fs::write(dir.join(crate::asset::MANIFEST), manifest.to_text())?;
        std::fs::write(dir.join(RULES), rules)?;
        for part in &parts {
            std::fs::write(dir.join(DIR).join(format!("{}.dds", part.name)), &part.dds)?;
        }
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(vec![Problem::new(format!(
            "cannot write {}: {e}",
            dir.display()
        ))]);
    }
    Ok(dir)
}

/// A texture by its name, its exact DDS and its header words.
pub(crate) type TexturePart = (String, Vec<u8>, Value);

/// The name, exact DDS and header words of each texture of `ytd`: what a source of it holds, for a caller that compares two dictionaries by it.
pub(crate) fn parts_of(ytd: &[u8]) -> Result<Vec<TexturePart>, Vec<Problem>> {
    let parts = if target_of(ytd)? == Generation::Gen9 {
        enhanced_parts(ytd)?
    } else {
        legacy_parts(ytd)?
    };
    Ok(parts
        .into_iter()
        .map(|part| (part.name, part.dds, part.header))
        .collect())
}

fn target_of(ytd: &[u8]) -> Result<Generation, Vec<Problem>> {
    let refused = |why: String| {
        vec![Problem::new(format!(
            "not a texture dictionary this build reads: {why}"
        ))]
    };
    let version = or_io::container::resource::Rsc7::peek(ytd)
        .map_err(|e| refused(e.to_string()))?
        .version();
    or_io::gta5::formats::format("ytd")
        .ok_or_else(|| refused("no .ytd format is declared".to_string()))?
        .target_of_version(version)
        .map_err(|e| refused(e.to_string()))
}

fn legacy_parts(ytd: &[u8]) -> Result<Vec<Part>, Vec<Problem>> {
    let dictionary = PgDictionary::<GrcTexture>::from_bytes(ytd)
        .map_err(|e| vec![Problem::new(e.to_string())])?;
    let mut parts = Vec::new();
    let mut problems = Vec::new();
    for (code, entry) in dictionary.codes.iter().zip(&dictionary.entries) {
        let Some(t) = entry else { continue };
        let mut refuse = |why: String| problems.push(Problem::new(why).of_texture(&t.name));
        if string_hash_str(&t.name) != *code {
            refuse("is filed under a hash that is not its name's".to_string());
            continue;
        }
        // A DDS holds one 2D surface and every mip of it.
        for (word, value, plain) in [
            ("depth", u32::from(t.depth), 1),
            ("layer_count", t.layer_count.into(), 0),
            ("image_type", t.image_type.into(), 0),
            ("cut_mip_levels", t.cut_mip_levels.into(), 0),
        ] {
            if value != plain {
                refuse(format!(
                    "is not one whole 2D surface ({word} is {value}), which a source holds"
                ));
            }
        }
        match to_dds(&**t).and_then(|dds| dds.to_bytes()) {
            Ok(dds) => parts.push(Part {
                name: t.name.clone(),
                dds,
                header: header_value(&LegacyHeader {
                    resource_type_and_conversion_flags: t.resource_type_and_conversion_flags,
                    template_type: t.template_type(),
                    extra_flags: t.extra_flags,
                    handle_index: t.handle_index,
                    info_bits: t.info_bits & !1,
                    stereo_rt_mode: t.stereo_rt_mode,
                }),
            }),
            Err(e) => refuse(format!("cannot be written as a DDS: {e}")),
        }
    }
    if problems.is_empty() {
        Ok(parts)
    } else {
        Err(problems)
    }
}

fn enhanced_parts(ytd: &[u8]) -> Result<Vec<Part>, Vec<Problem>> {
    let dictionary = PgDictionary::<GrcTextureG9>::from_bytes(ytd)
        .map_err(|e| vec![Problem::new(e.to_string())])?;
    let mut parts = Vec::new();
    let mut problems = Vec::new();
    for (code, entry) in dictionary.codes.iter().zip(&dictionary.entries) {
        let Some(t) = entry else { continue };
        let mut refuse = |why: String| problems.push(Problem::new(why).of_texture(&t.name));
        if string_hash_str(&t.name) != *code {
            refuse("is filed under a hash that is not its name's".to_string());
            continue;
        }
        if t.dimension != TextureDimension::Texture2D || t.depth != 1 {
            refuse(format!(
                "is not one 2D surface (dimension {:?}, depth {}), which a source holds",
                t.dimension, t.depth
            ));
            continue;
        }
        match to_dds(&**t).and_then(|dds| dds.to_bytes()) {
            Ok(dds) => parts.push(Part {
                name: t.name.clone(),
                dds,
                header: header_value(&EnhancedHeader {
                    flags: t.flags,
                    usage_data: t.usage_data,
                    anti_alias_type: t.anti_alias_type,
                    tile_mode: t.tile_mode.raw(),
                }),
            }),
            Err(e) => refuse(format!("cannot be written as a DDS: {e}")),
        }
    }
    if problems.is_empty() {
        Ok(parts)
    } else {
        Err(problems)
    }
}

fn header_value(header: &impl Serialize) -> Value {
    // Plain integers: serialising them cannot fail.
    serde_json::to_value(header).unwrap_or(Value::Null)
}

/// Everything a compile reads, so a recook can tell nothing changed before it encodes anything.
pub struct Inputs {
    manifest: AssetManifest,
    rules: Rules,
    /// Every DDS and image in `textures/`, by file name.
    files: BTreeMap<String, Vec<u8>>,
}

impl Inputs {
    pub fn read(source: &Source) -> Result<Self, Vec<Problem>> {
        Self::read_with(source, true)
    }

    /// The names of the files and the rules, with no file read: enough to say which file each texture compiles from.
    fn read_names(source: &Source) -> Result<Self, Vec<Problem>> {
        Self::read_with(source, false)
    }

    fn read_with(source: &Source, bytes: bool) -> Result<Self, Vec<Problem>> {
        let text = std::fs::read_to_string(source.dir.join(RULES))
            .map_err(|e| vec![Problem::new(format!("cannot read: {e}")).in_file(RULES)])?;
        let rules = serde_json::from_str::<Rules>(&text)
            .map_err(|e| vec![Problem::new(e.to_string()).in_file(RULES)])?;

        let mut files = BTreeMap::new();
        let mut problems = Vec::new();
        // A folder that cannot be listed is not a source of no textures: compiled as one it would
        // replace the last good output with an empty dictionary.
        let cannot_list =
            |e: std::io::Error| Problem::new(format!("cannot list {DIR}/: {e}")).in_file(DIR);
        let listing = std::fs::read_dir(source.dir.join(DIR)).map_err(|e| vec![cannot_list(e)])?;
        for entry in listing {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    problems.push(cannot_list(e));
                    continue;
                }
            };
            let file = entry.file_name().to_string_lossy().into_owned();
            if !is_texture_file(&file) || !entry.path().is_file() {
                continue;
            }
            let held = if bytes {
                std::fs::read(entry.path())
            } else {
                Ok(Vec::new())
            };
            match held {
                Ok(bytes) => {
                    files.insert(file, bytes);
                }
                Err(e) => problems.push(
                    Problem::new(format!("cannot read: {e}")).in_file(format!("{DIR}/{file}")),
                ),
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Self {
            manifest: source.manifest.clone(),
            rules,
            files,
        })
    }

    /// A digest of every input, whatever its formatting or its place.
    pub fn digest(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(self.manifest.digest_text());
        hash.update(canonical(&self.rules).unwrap_or_default());
        for (file, bytes) in &self.files {
            hash.update(file.as_bytes());
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        hash.finalize().into()
    }

    /// The generation the manifest compiles to.
    fn generation(&self) -> Result<Generation, Vec<Problem>> {
        let manifest = &self.manifest;
        if manifest.platform != "win64" {
            return Err(vec![Problem::new(format!(
                "platform {:?} is not one this build compiles for; only win64 is",
                manifest.platform
            ))
            .in_file(crate::asset::MANIFEST)]);
        }
        [Generation::Gen8, Generation::Gen9]
            .into_iter()
            .find(|g| g.slug() == manifest.target)
            .ok_or_else(|| {
                vec![Problem::new(format!(
                    "target {:?} is neither Legacy nor Enhanced",
                    manifest.target
                ))
                .in_file(crate::asset::MANIFEST)]
            })
    }

    /// The top mip of the texture `name` as the compile would store it, decoded to the pixels a person sees.
    pub(crate) fn pixels_of(&self, name: &str) -> Result<RgbaImage, String> {
        let join = |problems: Vec<Problem>| {
            problems
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        };
        let items = self.plan(self.generation().map_err(join)?).map_err(join)?;
        let item = items
            .iter()
            .find(|i| i.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} is not a texture of the source"))?;
        let surface = self.surface(item).map_err(|fault| match fault {
            Fault::Rule(why) | Fault::Input(why) => why,
        })?;
        let top = surface
            .format
            .mip_level_range(surface.width, surface.height, 0);
        let mut image = decode_texture_data(
            surface.format,
            surface.width,
            surface.height,
            surface
                .payload
                .get(top)
                .ok_or("the top mip is not in the file")?,
        )
        .map_err(|e| e.to_string())?;
        image.is_srgb = surface.srgb;
        Ok(image)
    }

    /// The `.ytd` the inputs compile to, reloaded through the readers before it is returned.
    pub fn compile(&self) -> Result<Compiled, Vec<Problem>> {
        let manifest = &self.manifest;
        let target = self.generation()?;

        let items = self.plan(target)?;
        let mut problems = Vec::new();
        let mut built = Vec::new();
        for item in &items {
            match self
                .surface(item)
                .and_then(|s| item.assemble(s).map_err(Fault::Input))
            {
                Ok(texture) => {
                    built.push((string_hash_str(&item.name), item.name.as_str(), texture))
                }
                Err(fault) => problems.push(item.problem(fault)),
            }
        }
        built.sort_by_key(|(code, ..)| *code);
        for pair in built.windows(2) {
            if pair[0].0 == pair[1].0 {
                problems.push(
                    Problem::new(format!(
                        "{:?} and {:?} are the same hash",
                        pair[0].1, pair[1].1
                    ))
                    .of_texture(pair[1].1),
                );
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }

        let count = built.len();
        let bytes = if target == Generation::Gen9 {
            let (codes, textures) = split(built, |t| match t {
                Texture::Enhanced(t) => Some(t),
                Texture::Legacy(_) => None,
            });
            let bytes = PgDictionary {
                codes,
                entries: textures,
                ..Default::default()
            }
            .to_bytes();
            reload(bytes, count, |b| {
                Ok(PgDictionary::<GrcTextureG9>::from_bytes(b)?
                    .entries
                    .iter()
                    .flatten()
                    .count())
            })?
        } else {
            let (codes, textures) = split(built, |t| match t {
                Texture::Legacy(t) => Some(t),
                Texture::Enhanced(_) => None,
            });
            let bytes = PgDictionary {
                codes,
                entries: textures,
                ..Default::default()
            }
            .to_bytes();
            reload(bytes, count, |b| {
                Ok(PgDictionary::<GrcTexture>::from_bytes(b)?
                    .entries
                    .iter()
                    .flatten()
                    .count())
            })?
        };
        Ok(Compiled {
            file_name: output_name(manifest),
            bytes,
        })
    }

    /// The textures of the source by lower-cased name, each with the entry that rules it and the files of its name, and the entries that are listed twice.
    fn slots(&self) -> (BTreeMap<String, Slot<'_>>, Vec<Problem>) {
        let mut slots: BTreeMap<String, Slot<'_>> = BTreeMap::new();
        let mut problems = Vec::new();
        for entry in &self.rules.textures {
            let slot = slots.entry(entry.name.to_lowercase()).or_default();
            if slot.entry.replace(entry).is_some() {
                problems.push(
                    Problem::new("is listed twice")
                        .in_file(RULES)
                        .of_texture(&entry.name),
                );
            }
            slot.name = entry.name.clone();
        }
        // A file an entry names, as its source or as a channel of its pack, is that entry's input.
        // With no texture of its own name it is not a texture itself.
        let claimed: BTreeSet<&str> = self
            .rules
            .textures
            .iter()
            .flat_map(|entry| {
                let packed = entry.pack.iter().flat_map(|pack| {
                    pack.channels()
                        .into_iter()
                        .filter_map(|(_, taken)| taken.map(|t| t.file.as_str()))
                });
                entry.source.as_deref().into_iter().chain(packed)
            })
            .collect();
        for file in self.files.keys() {
            let path = Path::new(file);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(file);
            if claimed.contains(file.as_str()) && !slots.contains_key(&stem.to_lowercase()) {
                continue;
            }
            let slot = slots.entry(stem.to_lowercase()).or_default();
            if slot.name.is_empty() {
                slot.name = stem.to_string();
            }
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
            {
                slot.dds.push(file);
            } else {
                slot.images.push(file);
            }
        }
        (slots, problems)
    }

    /// The file `slot` compiles from, or why it cannot be said.
    fn pick<'a>(&'a self, slot: &Slot<'a>) -> Result<Pick<'a>, Unpicked> {
        let name = &slot.name;
        let in_rules = |why: String| Unpicked {
            why,
            in_rules: true,
        };
        let plain = |why: String| Unpicked {
            why,
            in_rules: false,
        };
        if let Some(pack) = slot.entry.and_then(|e| e.pack.as_ref()) {
            if slot.entry.is_some_and(|e| e.source.is_some()) {
                return Err(in_rules(
                    "names a source and a pack, and a texture is made of one".to_string(),
                ));
            }
            self.check_pack(pack).map_err(in_rules)?;
            return Ok(Pick::Pack(pack));
        }
        if let Some(named) = slot.entry.and_then(|e| e.source.as_deref()) {
            if !is_plain_input(named) {
                return Err(in_rules(format!(
                    "source {named:?} is not a file name in {DIR}/"
                )));
            }
            return match self.files.get_key_value(named) {
                Some((file, _)) => Ok(Pick::of(file)),
                None => Err(in_rules(format!(
                    "source {named:?} is not a DDS or an image ({}) in {DIR}/",
                    image_extensions()
                ))),
            };
        }
        if let [image] = slot.images.as_slice() {
            return Ok(Pick::Image(image));
        }
        if slot.images.len() > 1 {
            return Err(plain(format!(
                "has {} images of its name ({}); keep one, or name the one to use as its source in {RULES}",
                slot.images.len(),
                slot.images.join(", ")
            )));
        }
        if let [dds] = slot.dds.as_slice() {
            return Ok(Pick::Dds(dds));
        }
        if slot.dds.len() > 1 {
            return Err(plain(format!(
                "has {} DDS files that differ only in case",
                slot.dds.len()
            )));
        }
        Err(plain(format!(
            "has no source: there is no {DIR}/{name}.dds and no image of that name"
        )))
    }

    /// Which file each texture compiles from, or why it has none.
    fn plan(&self, target: Generation) -> Result<Vec<Item<'_>>, Vec<Problem>> {
        let (slots, mut problems) = self.slots();
        let mut items = Vec::new();
        for slot in slots.into_values() {
            let name = slot.name.clone();
            let fail = |why: String| Problem::new(why).of_texture(&name);
            if let Some(bad) = unsafe_file_name(&name) {
                problems.push(fail(bad));
                continue;
            }
            let header = match Header::of(slot.entry, target) {
                Ok(header) => header,
                Err(why) => {
                    problems.push(fail(why).in_file(RULES));
                    continue;
                }
            };
            let format = slot
                .entry
                .and_then(|e| e.encode.as_ref())
                .and_then(|e| e.format.as_deref());
            if let Some(named) = format.filter(|n| GrcTextureFormat::from_name(n).is_none()) {
                problems.push(
                    fail(format!(
                        "{named:?} is not a texture format this build knows"
                    ))
                    .in_file(RULES),
                );
                continue;
            }
            let pick = match self.pick(&slot) {
                Ok(pick) => pick,
                Err(Unpicked { why, in_rules }) => {
                    let problem = fail(why);
                    problems.push(if in_rules {
                        problem.in_file(RULES)
                    } else {
                        problem
                    });
                    continue;
                }
            };
            let base = slot.dds.first().copied();
            items.push(Item {
                name,
                entry: slot.entry,
                pick,
                base,
                header,
            });
        }
        if problems.is_empty() {
            Ok(items)
        } else {
            Err(problems)
        }
    }

    fn surface(&self, item: &Item<'_>) -> Result<Surface, Fault> {
        let rules = item.entry.and_then(|e| e.encode.as_ref());
        if let Pick::Dds(file) = item.pick {
            return held_by_a_dds(rules, surface_of_dds(file, &self.files[file])?);
        }
        let base = item
            .base
            .map(|dds| surface_of_dds(dds, &self.files[dds]))
            .transpose()?;
        let packed = matches!(item.pick, Pick::Pack(_));
        let levels = levels_of(rules, colour_space(rules, base.as_ref(), packed))?;
        let (image, depth) = match item.pick {
            Pick::Image(file) => decode_image(file, &self.files[file], &levels)?,
            Pick::Pack(pack) => self.packed(pack, &levels)?,
            Pick::Dds(_) => unreachable!("a DDS returned above"),
        };
        if depth == Depth::Eight {
            if let Some(rule) = rules.and_then(Encode::bit_depth_rule) {
                return Err(Fault::Rule(format!(
                    "{rule} brings an image of more than 8 bits down, and the image this texture is made from has 8 bits a channel"
                )));
            }
        }
        if depth == Depth::Sixteen && rules.is_none_or(|r| r.clamp.is_none()) {
            let file = match item.pick {
                Pick::Image(file) => format!("{DIR}/{file}"),
                _ => "an image of the pack this texture is made from".to_string(),
            };
            return Err(Fault::Rule(format!(
                "{file} has 16 bits a channel, and the game's formats hold 8, so how it comes down is stated and not guessed: add `\"clamp\": [0, 1]` to this texture's encode in {RULES}, which keeps the whole range"
            )));
        }
        encode(rules, base.as_ref(), image, packed)
    }

    /// Why `pack` cannot be built from the files in `textures/`, or `Ok` where each channel names an image.
    fn check_pack(&self, pack: &Pack) -> Result<(), String> {
        let mut named = false;
        for (letter, taken) in pack.channels() {
            let Some(taken) = taken else { continue };
            named = true;
            let file = taken.file.as_str();
            if !is_plain_input(file) {
                return Err(format!(
                    "pack {letter}: {file:?} is not a file name in {DIR}/"
                ));
            }
            if !self.files.contains_key(file) || is_dds(file) {
                let kind = if is_dds(file) {
                    "a DDS, and a channel is taken from"
                } else {
                    "not"
                };
                return Err(format!(
                    "pack {letter}: {file:?} is {kind} an image ({}) in {DIR}/",
                    image_extensions()
                ));
            }
        }
        if named {
            Ok(())
        } else {
            Err("has a pack that names no channel".to_string())
        }
    }

    /// The image `pack` makes: each channel is one channel of one of the images, all of one size.
    fn packed(&self, pack: &Pack, levels: &Levels) -> Result<(RgbaImage, Depth), String> {
        let mut decoded: BTreeMap<&str, RgbaImage> = BTreeMap::new();
        let mut deepest = Depth::Eight;
        let mut size: Option<(u32, u32, &str)> = None;
        for (letter, taken) in pack.channels() {
            let Some(taken) = taken else { continue };
            let file = taken.file.as_str();
            if !decoded.contains_key(file) {
                let (image, depth) = decode_image(file, &self.files[file], levels)?;
                deepest = deepest.max(depth);
                decoded.insert(file, image);
            }
            let image = &decoded[file];
            match size {
                None => size = Some((image.width, image.height, file)),
                Some((w, h, first)) if (w, h) != (image.width, image.height) => {
                    return Err(format!(
                        "pack channels are made from images of one size, and {first} is {w}x{h} \
                         while {file} (for {letter}) is {}x{}",
                        image.width, image.height
                    ));
                }
                Some(_) => {}
            }
        }
        let (width, height, _) = size.ok_or("has a pack that names no channel")?;
        let from: [Option<(&[u8], usize)>; 4] = pack.channels().map(|(_, taken)| {
            taken.map(|t| (decoded[t.file.as_str()].data.as_slice(), t.channel as usize))
        });
        let mut out = RgbaImage::new(width, height);
        for (i, texel) in out.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            for (c, put) in texel.iter_mut().enumerate() {
                *put = match from[c] {
                    Some((data, channel)) => data[i * 4 + channel],
                    None if c == 3 => 255,
                    None => 0,
                };
            }
        }
        Ok((out, deepest))
    }
}

enum Pick<'a> {
    Dds(&'a str),
    Image(&'a str),
    Pack(&'a Pack),
}

/// A texture of the source: the entry that rules it and the files of its name.
#[derive(Default)]
struct Slot<'a> {
    name: String,
    entry: Option<&'a Entry>,
    dds: Vec<&'a str>,
    images: Vec<&'a str>,
}

/// Why no file can be said to be a texture's source, and whether the fault is in `textures.json`.
struct Unpicked {
    why: String,
    in_rules: bool,
}

impl<'a> Pick<'a> {
    fn of(file: &'a str) -> Self {
        if is_dds(file) {
            Self::Dds(file)
        } else {
            Self::Image(file)
        }
    }

    /// Where a fault in this input is, relative to the source folder.
    fn location(&self) -> String {
        match self {
            Self::Dds(file) | Self::Image(file) => format!("{DIR}/{file}"),
            Self::Pack(_) => RULES.to_string(),
        }
    }
}

/// One texture's compile: its name, its entry and the file its pixels come from.
struct Item<'a> {
    name: String,
    entry: Option<&'a Entry>,
    pick: Pick<'a>,
    /// The texture's own DDS, which an image's format and colour space default to.
    base: Option<&'a str>,
    header: Header,
}

impl Item<'_> {
    fn problem(&self, fault: Fault) -> Problem {
        let (why, file) = match fault {
            Fault::Rule(why) => (why, RULES.to_string()),
            Fault::Input(why) => (why, self.pick.location()),
        };
        Problem::new(why).in_file(file).of_texture(&self.name)
    }

    fn assemble(&self, surface: Surface) -> Result<Texture, String> {
        match &self.header {
            Header::Enhanced(header) => {
                enhanced(&self.name, surface, header).map(Texture::Enhanced)
            }
            Header::Legacy(header) => legacy(&self.name, surface, header).map(Texture::Legacy),
        }
    }
}

/// A texture's header words, read for the generation the source compiles to.
enum Header {
    Legacy(LegacyHeader),
    Enhanced(EnhancedHeader),
}

impl Header {
    /// The words an entry states, or those of a texture made from files alone.
    fn of(entry: Option<&Entry>, target: Generation) -> Result<Self, String> {
        let stated = entry.and_then(|e| e.header.as_ref());
        let bad = |e: serde_json::Error| format!("header: {e}");
        Ok(if target == Generation::Gen9 {
            Self::Enhanced(match stated {
                Some(value) => serde_json::from_value(value.clone()).map_err(bad)?,
                None => EnhancedHeader {
                    tile_mode: TileMode::Auto.to_raw(),
                    ..Default::default()
                },
            })
        } else {
            Self::Legacy(match stated {
                Some(value) => serde_json::from_value(value.clone()).map_err(bad)?,
                None => LegacyHeader {
                    resource_type_and_conversion_flags: PROCESSED,
                    ..Default::default()
                },
            })
        })
    }
}

enum Texture {
    Legacy(GrcTexture),
    Enhanced(GrcTextureG9),
}

/// A texture's pixels and how they are stored, whichever generation holds them.
pub(crate) struct Surface {
    pub(crate) format: GrcTextureFormat,
    pub(crate) srgb: bool,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) mips: u32,
    /// Exactly the declared mips.
    pub(crate) payload: Vec<u8>,
    /// Every block is the DDS's own, so a payload no image could fill can still be written.
    pub(crate) exact: bool,
}

pub(crate) fn surface_of_dds(file: &str, bytes: &[u8]) -> Result<Surface, String> {
    let dds = Dds::from_bytes(bytes).map_err(|e| format!("{file}: {e}"))?;
    if !dds.is_one_surface() {
        return Err(format!(
            "{file} is a cube map, a volume or an array, and a texture is one 2D surface"
        ));
    }
    let (format, srgb) = format_of_dds(&dds).map_err(|e| format!("{file}: {e}"))?;
    let (width, height) = (dds.header.width.max(1), dds.header.height.max(1));
    let mips = dds.header.mip_map_count.max(1);
    // The header's words are the file's to choose: bound them before any size is computed from them.
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!(
            "{file} states {width}x{height}, and a texture's sides are at most {MAX_SIDE}"
        ));
    }
    let most = max_mip_levels(width, height);
    if mips > most {
        return Err(format!(
            "{file} states {mips} mips, and {width}x{height} has at most {most}"
        ));
    }
    let size = payload_size(format, width, height, mips);
    let Some(payload) = usize::try_from(size)
        .ok()
        .and_then(|size| dds.data.get(..size))
    else {
        return Err(format!(
            "{file} holds {} bytes and {width}x{height} {format:?} with {mips} mips needs {size}",
            dds.data.len()
        ));
    };
    Ok(Surface {
        format,
        srgb,
        width,
        height,
        mips,
        payload: payload.to_vec(),
        exact: true,
    })
}

/// The bytes of `mips` levels of a `width` by `height` surface, in 64 bits, which `total_size` is not.
fn payload_size(format: GrcTextureFormat, width: u32, height: u32, mips: u32) -> u64 {
    let block = u64::from(format.block_size());
    let bytes = u64::from(format.bytes_per_block());
    let (mut w, mut h) = (u64::from(width), u64::from(height));
    let mut total = 0;
    for _ in 0..mips {
        let (across, down) = if block > 1 {
            (w.div_ceil(block).max(1), h.div_ceil(block).max(1))
        } else {
            (w, h)
        };
        total += across * down * bytes;
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
    }
    total
}

fn decode_image(file: &str, bytes: &[u8], levels: &Levels) -> Result<(RgbaImage, Depth), String> {
    texture_import::decode_levels(Path::new(file), bytes, levels)
        .map(|decoded| (decoded.image, decoded.depth))
        .map_err(|e| format!("cannot decode {file}: {e}"))
}

/// The colour space a texture is stored in: the rule's, else the DDS's, else sRGB.
fn colour_space(rules: Option<&Encode>, base: Option<&Surface>, packed: bool) -> bool {
    // Packed channels are measures and not colour, so with no DDS to say otherwise they are linear.
    match rules.and_then(|r| r.colour_space) {
        Some(space) => space == ColourSpace::Srgb,
        None => base.map_or(!packed, |b| b.srgb),
    }
}

/// The alpha rule of a texture stored as `format`, or the rule of its entry that the format cannot keep.
fn alpha_rule(rules: &Encode, format: GrcTextureFormat) -> Result<AlphaRule, Fault> {
    let premultiplied = rules.alpha == Some(AlphaMode::Premultiplied);
    if let Some(cutoff) = rules.alpha_coverage {
        if !(cutoff > 0.0 && cutoff <= 1.0) {
            return Err(Fault::Rule(format!(
                "alpha_coverage {cutoff} is not a cutoff above 0 and at most 1"
            )));
        }
    }
    let alpha_bits = |format: GrcTextureFormat| match format {
        GrcTextureFormat::DXT1 => 1,
        GrcTextureFormat::DXT3
        | GrcTextureFormat::DXT5
        | GrcTextureFormat::BC7
        | GrcTextureFormat::A8R8G8B8
        | GrcTextureFormat::A8B8G8R8 => 8,
        _ => 0,
    };
    let held = alpha_bits(format);
    if premultiplied && held < 8 {
        return Err(Fault::Rule(format!(
            "alpha premultiplied cannot be honoured: {format:?} holds {}, and colour multiplied by alpha needs eight bits of it; state DXT5, BC7 or an uncompressed format",
            if held == 0 { "no alpha" } else { "one bit of alpha" }
        )));
    }
    if rules.alpha_coverage.is_some() && held == 0 {
        return Err(Fault::Rule(format!(
            "alpha_coverage cannot be honoured: {format:?} holds no alpha for a cutout to be tested on"
        )));
    }
    Ok(AlphaRule {
        premultiplied,
        coverage: rules.alpha_coverage,
    })
}

/// How an image of more than 8 bits comes down, from the rules alone.
fn levels_of(rules: Option<&Encode>, srgb: bool) -> Result<Levels, Fault> {
    let none = Encode::default();
    let rules = rules.unwrap_or(&none);
    let rule = |why: String| Fault::Rule(why);
    if rules.exposure.is_some() && rules.tonemap.is_none() {
        return Err(rule(
            "exposure is stated with no tonemap to apply it to".to_string(),
        ));
    }
    if rules.clamp.is_some() && rules.tonemap.is_some() {
        return Err(rule(
            "clamp and tonemap are both stated, and light is brought down by one".to_string(),
        ));
    }
    if let Some([lo, hi]) = rules.clamp {
        if !(lo.is_finite() && hi.is_finite() && lo < hi) {
            return Err(rule(format!("clamp {lo} to {hi} is not a range")));
        }
    }
    Ok(Levels {
        clamp: rules.clamp.map(|[lo, hi]| (lo, hi)),
        reinhard: rules
            .tonemap
            .map(|Tonemap::Reinhard| rules.exposure.unwrap_or(0.0)),
        srgb,
    })
}

/// How many mips `mips` gives an image of this size.
pub(crate) fn mip_levels(mips: Mips, width: u32, height: u32) -> u32 {
    match mips {
        // Until the shorter side is four texels, where shipped textures stop.
        Mips::To4x4 => width.min(height).max(1).ilog2().saturating_sub(1).max(1),
        Mips::To1x1 => max_mip_levels(width, height),
    }
}

/// A DDS with no image beside it is copied block for block, so a rule that would change a block is refused and never skipped.
fn held_by_a_dds(rules: Option<&Encode>, mut surface: Surface) -> Result<Surface, Fault> {
    let Some(rules) = rules else {
        return Ok(surface);
    };
    let refuse = |rule: &str, why: String| {
        Fault::Rule(format!(
            "{rule} cannot be honoured: {why}; put an image of the texture beside the DDS, or edit the DDS"
        ))
    };
    if let Some(format) = rules
        .format
        .as_deref()
        .and_then(GrcTextureFormat::from_name)
    {
        if format != surface.format {
            return Err(refuse(
                "format",
                format!("the DDS is {:?} and is copied as it is", surface.format),
            ));
        }
    }
    if let Some(mips) = rules.mips {
        let wanted = mip_levels(mips, surface.width, surface.height);
        if wanted != surface.mips {
            return Err(refuse(
                "mips",
                format!("the DDS holds {} and the rule gives {wanted}", surface.mips),
            ));
        }
    }
    if rules.flip_green == Some(true) {
        return Err(refuse(
            "flip_green",
            "it would change every block".to_string(),
        ));
    }
    if rules.alpha_coverage.is_some() {
        return Err(refuse(
            "alpha_coverage",
            "it would change the alpha of every mip".to_string(),
        ));
    }
    if let Some(rule) = rules.bit_depth_rule() {
        return Err(refuse(
            rule,
            "it brings an image of more than 8 bits down, and this texture is a DDS".to_string(),
        ));
    }
    // The colour space is a flag of the header, which moves no block.
    if let Some(space) = rules.colour_space {
        surface.srgb = space == ColourSpace::Srgb;
    }
    Ok(surface)
}

/// The pixels of `image` as a surface: the blocks it leaves as the DDS has them where it is made to
/// that DDS, and a full encode with the rules otherwise. What the rules leave out follows the DDS.
fn encode(
    rules: Option<&Encode>,
    base: Option<&Surface>,
    mut image: RgbaImage,
    packed: bool,
) -> Result<Surface, Fault> {
    let rules = rules.cloned().unwrap_or_default();
    if rules.flip_green == Some(true) {
        for texel in image.data.as_chunks_mut::<4>().0 {
            texel[1] = 255 - texel[1];
        }
    }
    let format = match &rules.format {
        Some(name) => GrcTextureFormat::from_name(name)
            .ok_or_else(|| format!("{name:?} is not a texture format this build knows"))?,
        None => base.map_or_else(|| automatic_format(&image), |b| b.format),
    };
    let alpha = alpha_rule(&rules, format)?;
    let srgb = colour_space(Some(&rules), base, packed);
    let filter = match rules.filter {
        Some(Filter::Lanczos3) => MipFilter::Lanczos3,
        _ => MipFilter::Box,
    };
    let quality = match rules.quality {
        Some(Quality::Fast) => EncodeQuality::Fast,
        Some(Quality::High) => EncodeQuality::High,
        _ => EncodeQuality::Normal,
    };
    let levels = |mips: Mips| mip_levels(mips, image.width, image.height);

    if let Some(base) = base {
        // An image made to the DDS's own format and mip count keeps every block it did not change.
        let same_mips = rules.mips.is_none_or(|mips| levels(mips) == base.mips);
        if format == base.format && same_mips && alpha.is_default() {
            let settings = reencode::Settings {
                filter,
                quality,
                srgb,
            };
            if let Some(kept) = reencode::changed_blocks(base, &image, &settings)? {
                return Ok(kept);
            }
        }
    }

    let options = TextureBuildOptions {
        mip_levels: Some(levels(rules.mips.unwrap_or(Mips::To4x4))),
        filter,
        srgb,
        quality,
        allow_npot: false,
        alpha,
    };
    let encoded = encode_texture_with_options(format, image.width, image.height, &image, &options)
        .map_err(|e| e.to_string())?;
    Ok(Surface {
        format,
        srgb,
        width: image.width,
        height: image.height,
        mips: u32::from(encoded.mip_count),
        payload: encoded.data,
        exact: false,
    })
}

fn legacy(name: &str, surface: Surface, header: &LegacyHeader) -> Result<GrcTexture, String> {
    let sides = (u16::try_from(surface.width), u16::try_from(surface.height));
    let (Ok(width), Ok(height)) = sides else {
        return Err(format!(
            "{}x{} does not fit a texture's 16-bit sides",
            surface.width, surface.height
        ));
    };
    let mip_count = u8::try_from(surface.mips)
        .map_err(|_| format!("{} mips is more than a texture holds", surface.mips))?;
    let stride = surface.format.legacy_mip_stride(surface.width);
    let mip_stride =
        u16::try_from(stride).map_err(|_| format!("a {stride}-byte row does not fit 16 bits"))?;
    let mut backing_store = surface.payload;
    // The masked word states only whole multiples of the mask's lowest bit.
    let unit = (PAYLOAD_SIZE_MASK & PAYLOAD_SIZE_MASK.wrapping_neg()) as usize;
    backing_store.resize(backing_store.len().next_multiple_of(unit), 0);
    let mut texture = GrcTexture {
        name: name.to_string(),
        width,
        height,
        depth: 1,
        mip_stride,
        format: surface.format.to_raw_format(),
        mip_count,
        info_bits: header.info_bits & !1 | u8::from(surface.srgb),
        backing_store,
        resource_type_and_conversion_flags: header.resource_type_and_conversion_flags,
        extra_flags: header.extra_flags,
        handle_index: header.handle_index,
        stereo_rt_mode: header.stereo_rt_mode,
        ..Default::default()
    };
    texture.set_template_type(header.template_type);
    Ok(texture)
}

fn enhanced(name: &str, surface: Surface, header: &EnhancedHeader) -> Result<GrcTextureG9, String> {
    let sides = (u16::try_from(surface.width), u16::try_from(surface.height));
    let (Ok(width), Ok(height)) = sides else {
        return Err(format!(
            "{}x{} does not fit a texture's 16-bit sides",
            surface.width, surface.height
        ));
    };
    let mip_levels = u8::try_from(surface.mips)
        .map_err(|_| format!("{} mips is more than a texture holds", surface.mips))?;
    let dxgi = surface.format.to_dxgi(surface.srgb) as u32;
    let format = u8::try_from(dxgi)
        .map_err(|_| format!("{:?} has no Enhanced pixel format", surface.format))?;
    // Only Auto and Linear store a payload row by row; an image has no swizzle to be written in.
    if !surface.exact
        && !matches!(
            <TileMode as OpenEnum>::from_raw(header.tile_mode),
            TileMode::Auto | TileMode::Linear
        )
    {
        return Err(format!(
            "has tile mode {}, whose swizzled payload only a DDS copied from the file can fill",
            header.tile_mode
        ));
    }
    let mut texture = GrcTextureG9 {
        name: name.to_string(),
        width,
        height,
        depth: 1,
        dimension: Open::new(TextureDimension::Texture2D),
        format: Open::new(<PixelFormat as OpenEnum>::from_raw(format)),
        tile_mode: Open::new(<TileMode as OpenEnum>::from_raw(header.tile_mode)),
        anti_alias_type: header.anti_alias_type,
        mip_levels,
        pixels: surface.payload,
        flags: header.flags,
        usage_data: header.usage_data,
        view: ShaderResourceViewG9 {
            view_dimension: 0x41,
            ..Default::default()
        },
        ..Default::default()
    };
    texture.set_block_stride(surface.format.bytes_per_block());
    Ok(texture)
}

/// The codes and the boxed textures of `built`, which is already in code order.
fn split<T>(
    built: Vec<(u32, &str, Texture)>,
    take: impl Fn(Texture) -> Option<T>,
) -> (Vec<u32>, Vec<Option<Box<T>>>) {
    let mut codes = Vec::new();
    let mut textures = Vec::new();
    for (code, _, texture) in built {
        codes.push(code);
        textures.push(take(texture).map(Box::new));
    }
    (codes, textures)
}

/// `bytes` once the readers have taken them back with the texture count they were written from.
fn reload(
    bytes: or_io::Result<Vec<u8>>,
    count: usize,
    read: impl Fn(&[u8]) -> or_io::Result<usize>,
) -> Result<Vec<u8>, Vec<Problem>> {
    let bytes = bytes.map_err(|e| vec![Problem::new(format!("cannot write the .ytd: {e}"))])?;
    match read(&bytes) {
        Ok(back) if back == count => Ok(bytes),
        Ok(back) => Err(vec![Problem::new(format!(
            "the .ytd read back with {back} textures, not the {count} it was written from"
        ))]),
        Err(e) => Err(vec![Problem::new(format!(
            "the .ytd does not read back: {e}"
        ))]),
    }
}

/// The name of the `.ytd` a source compiles to.
pub fn output_name(manifest: &AssetManifest) -> String {
    format!("{}.ytd", manifest.name)
}

/// Whether a file in `textures/` is a DDS or an image a texture compiles from.
pub fn is_texture_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dds") || is_image_extension(e))
}

/// Whether `name` is a DDS by its extension.
fn is_dds(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
}

/// Whether `name` is one file name inside `textures/`, as a `source` or a pack channel must be.
fn is_plain_input(name: &str) -> bool {
    !(name.is_empty() || name.contains(['/', '\\']) || name.contains(".."))
}

/// `ytd` with one texture made from `image`, every other texture and every block the image leaves alone as it was.
///
/// The texture is the one whose name is the image's, else the first of the dictionary, which is the one
/// a variant is drawn with and the only one of a variant that holds a single texture. The work is a textures source made in a folder of its own
/// under `scratch` and removed again, so the compile is the one a saved source gets.
pub fn replace_image(ytd: &[u8], image: &Path, scratch: &Path) -> Result<Vec<u8>, Vec<Problem>> {
    static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let extension = image.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !is_image_extension(extension) {
        return Err(vec![Problem::new(format!(
            "{} is not an image this build reads ({})",
            image.display(),
            image_extensions()
        ))]);
    }
    let pixels = std::fs::read(image).map_err(|e| {
        vec![Problem::new(format!(
            "cannot read {}: {e}",
            image.display()
        ))]
    })?;
    let work = scratch.join(format!(
        "replace-{}-{}",
        std::process::id(),
        MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let cannot = |what: &Path, e: std::io::Error| {
        vec![Problem::new(format!(
            "cannot write {}: {e}",
            what.display()
        ))]
    };
    std::fs::create_dir_all(&work).map_err(|e| cannot(&work, e))?;
    let made = (|| {
        let how = Extraction {
            from_game: false,
            virtual_path: None,
        };
        let dir = extract(ytd, "variant", &work, &how)?;
        let folder = dir.join(DIR);
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .map_err(|e| cannot(&folder, e))?
            .flatten()
            .filter(|e| is_dds(&e.file_name().to_string_lossy()))
            .filter_map(|e| Some(e.path().file_stem()?.to_string_lossy().into_owned()))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        let stem = image
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase());
        let first = first_texture_name(ytd).and_then(|first| names.iter().find(|n| **n == first));
        let texture = names
            .iter()
            .find(|n| Some(n.to_lowercase()) == stem)
            .or(first)
            .or(names.first())
            .ok_or_else(|| vec![Problem::new("holds no texture to put an image into")])?;
        let put = folder.join(format!("{texture}.{extension}"));
        std::fs::write(&put, pixels).map_err(|e| cannot(&put, e))?;
        // A picture of 16 bits a channel compiles only with a rule that says how it comes down, so it is given its whole range.
        state_depth_rules(&dir, &[format!("{texture}.{extension}")])
            .map_err(|why| vec![Problem::new(why)])?;
        let source = Source::open(&dir).map_err(|p| vec![p])?;
        compile(&source).map(|compiled| compiled.bytes)
    })();
    let _ = std::fs::remove_dir_all(&work);
    made
}

/// The name of the first texture of the dictionary `ytd`, in the order it stores them.
fn first_texture_name(ytd: &[u8]) -> Option<String> {
    match target_of(ytd).ok()? {
        Generation::Gen9 => PgDictionary::<GrcTextureG9>::from_bytes(ytd)
            .ok()?
            .entries
            .iter()
            .flatten()
            .next()
            .map(|t| t.name.clone()),
        _ => PgDictionary::<GrcTexture>::from_bytes(ytd)
            .ok()?
            .entries
            .iter()
            .flatten()
            .next()
            .map(|t| t.name.clone()),
    }
}

/// What the folder `source` names compiles to.
pub fn compile(source: &Source) -> Result<Compiled, Vec<Problem>> {
    Inputs::read(source)?.compile()
}

/// What one texture of a source is made from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Input {
    pub texture: String,
    pub kind: InputKind,
    /// The files its pixels come from: one, or the images whose channels are packed.
    pub from: Vec<String>,
    /// Files of its name that the compile does not use.
    pub unused: Vec<String>,
    /// What the entry of `textures.json` states for it, in words.
    pub rules: Vec<String>,
    /// For an `Unclear` texture, why no file can be said to be its source.
    pub why: Option<String>,
    /// Its entry in `textures.json` names the file as its `source`.
    pub by_entry: bool,
}

/// What a texture is made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// A DDS, copied block for block.
    Dds,
    /// An image, which wins over a DDS of its name.
    Image,
    /// The channels of images.
    Pack,
    /// More than one file could be it, or none: the compile refuses the source.
    Unclear,
}

impl Input {
    /// The input as one line an inspector shows, saying which file is used and what it wins over.
    pub fn sentence(&self) -> String {
        let named = |kind: &str, file: &str| format!("{} ({kind})", file);
        let mut line = match (self.kind, self.from.first()) {
            (InputKind::Unclear, _) => {
                let files = (!self.from.is_empty()).then(|| format!(" ({})", self.from.join(", ")));
                format!(
                    "{}: unclear{}: {}",
                    self.texture,
                    files.unwrap_or_default(),
                    self.why
                        .as_deref()
                        .unwrap_or("no file can be said to be its source")
                )
            }
            // The texture's name once: a DDS of the same name is the texture's own file.
            (InputKind::Dds, Some(file)) if file.trim_end_matches(".dds") == self.texture => {
                format!("{}, copied as it is", named("DDS", file))
            }
            (InputKind::Dds, Some(file)) => {
                format!("{}: {}, copied as it is", self.texture, named("DDS", file))
            }
            (InputKind::Pack, _) => format!(
                "{}: the channels of {} (a pack of {RULES})",
                self.texture,
                self.from.join(", ")
            ),
            (_, Some(file)) if self.by_entry => format!(
                "{}: {}, because its entry in {RULES} names it as the source",
                self.texture,
                named(if is_dds(file) { "DDS" } else { "image" }, file)
            ),
            (_, Some(file)) if self.unused.is_empty() => {
                format!("{}: {}", self.texture, named("image", file))
            }
            (_, Some(file)) => format!("{}: {} wins", self.texture, named("image", file)),
            (_, None) => self.texture.clone(),
        };
        for file in &self.unused {
            line.push_str(&if is_dds(file) {
                format!("; {file} beside it only says what the rules leave out")
            } else {
                format!("; {file} is not used")
            });
        }
        if !self.rules.is_empty() {
            line.push_str(&format!(" [{}]", self.rules.join(", ")));
        }
        line
    }
}

/// Which file each texture of `source` compiles from, with no file read and no pixel decoded: a texture whose source is unclear says so.
pub fn inputs_of(source: &Source) -> Result<Vec<Input>, Vec<Problem>> {
    let inputs = Inputs::read_names(source)?;
    let (slots, _) = inputs.slots();
    let mut out = Vec::new();
    for slot in slots.into_values() {
        let rules = slot
            .entry
            .and_then(|e| e.encode.as_ref())
            .map_or_else(Vec::new, Encode::words);
        let all: Vec<String> = slot
            .dds
            .iter()
            .chain(&slot.images)
            .map(ToString::to_string)
            .collect();
        let (kind, from, why, by_entry) = match inputs.pick(&slot) {
            Ok(Pick::Dds(file)) => (InputKind::Dds, vec![file.to_string()], None, false),
            Ok(Pick::Image(file)) => (InputKind::Image, vec![file.to_string()], None, false),
            Ok(Pick::Pack(pack)) => {
                let mut files: Vec<String> = pack
                    .channels()
                    .into_iter()
                    .filter_map(|(_, taken)| taken.map(|t| t.file.clone()))
                    .collect();
                files.dedup();
                (InputKind::Pack, files, None, false)
            }
            Err(Unpicked { why, .. }) => (InputKind::Unclear, all.clone(), Some(why), false),
        };
        let by_entry = by_entry
            || slot
                .entry
                .is_some_and(|e| e.source.is_some() && kind != InputKind::Unclear);
        let unused = all.into_iter().filter(|f| !from.contains(f)).collect();
        out.push(Input {
            texture: slot.name,
            kind,
            from,
            unused,
            rules,
            why,
            by_entry,
        });
    }
    Ok(out)
}

/// The window a 16-bit picture is given when it is put into a source: its whole range.
pub const DEPTH_RULE: [f32; 2] = [0.0, 1.0];

/// States [`DEPTH_RULE`] in `textures.json` for each of `files` in `dir`/`textures` that is a picture of 16 bits a channel and has no
/// window stated, so that the source compiles and the rule is where a person reads it; the sentence for each picture changed.
///
/// A picture of 8 bits takes back that exact window from the texture of its name, which an 8-bit picture is refused with. A picture
/// whose header cannot be read is left for the compile to refuse with its reason.
pub fn state_depth_rules(dir: &Path, files: &[String]) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(dir.join(RULES)).map_err(|e| format!("{RULES}: {e}"))?;
    let mut rules = serde_json::from_str::<Rules>(&text).map_err(|e| format!("{RULES}: {e}"))?;
    let mut said = Vec::new();
    for file in files {
        let path = Path::new(file);
        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !is_image_extension(extension) {
            continue;
        }
        let Ok(bytes) = std::fs::read(dir.join(DIR).join(file)) else {
            continue;
        };
        let depth = texture_import::depth_of(path, &bytes).ok();
        let name = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        if depth == Some(Depth::Eight) {
            if take_back_depth_rule(&mut rules, &name) {
                said.push(format!(
                    "{file} has 8 bits a channel: {RULES} no longer states clamp {} to {} for {name}",
                    DEPTH_RULE[0], DEPTH_RULE[1]
                ));
            }
            continue;
        }
        if depth != Some(Depth::Sixteen) {
            continue;
        }
        let at = match rules
            .textures
            .iter()
            .position(|e| e.name.eq_ignore_ascii_case(&name))
        {
            Some(at) => at,
            None => {
                rules.textures.push(Entry {
                    name: name.clone(),
                    source: None,
                    pack: None,
                    header: None,
                    encode: None,
                });
                rules.textures.len() - 1
            }
        };
        let encode = rules.textures[at]
            .encode
            .get_or_insert_with(Encode::default);
        if encode.clamp.is_none() {
            encode.clamp = Some(DEPTH_RULE);
            said.push(format!(
                "{file} has 16 bits a channel: {RULES} states clamp {} to {}, its whole range, for {name}",
                DEPTH_RULE[0], DEPTH_RULE[1]
            ));
        }
    }
    if !said.is_empty() {
        let text = canonical(&rules).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(RULES), text).map_err(|e| format!("{RULES}: {e}"))?;
    }
    Ok(said)
}

/// Takes [`DEPTH_RULE`] out of the entry named `name` when it is the whole of what the entry says about the clamp and the entry
/// makes the texture from the file of its name; the entry goes with it when nothing else is left in it.
fn take_back_depth_rule(rules: &mut Rules, name: &str) -> bool {
    let Some(at) = (rules.textures.iter()).position(|e| e.name.eq_ignore_ascii_case(name)) else {
        return false;
    };
    let entry = &mut rules.textures[at];
    let stated = entry
        .encode
        .as_ref()
        .is_some_and(|e| e.clamp == Some(DEPTH_RULE));
    if !stated || entry.source.is_some() || entry.pack.is_some() {
        return false;
    }
    if let Some(encode) = entry.encode.as_mut() {
        encode.clamp = None;
        if *encode == Encode::default() {
            entry.encode = None;
        }
    }
    if entry.encode.is_none() && entry.header.is_none() {
        rules.textures.remove(at);
    }
    true
}

impl Encode {
    /// What the entry states, in words an inspector shows.
    pub fn words(&self) -> Vec<String> {
        let mut words = Vec::new();
        let mut say = |on: bool, text: String| {
            if on {
                words.push(text);
            }
        };
        say(
            self.format.is_some(),
            format!("format {}", self.format.as_deref().unwrap_or_default()),
        );
        say(
            self.mips.is_some(),
            format!(
                "mips {}",
                match self.mips {
                    Some(Mips::To1x1) => "to 1x1",
                    _ => "to 4x4",
                }
            ),
        );
        say(
            self.filter.is_some(),
            format!(
                "filter {}",
                match self.filter {
                    Some(Filter::Lanczos3) => "lanczos3",
                    _ => "box",
                }
            ),
        );
        say(
            self.quality.is_some(),
            format!(
                "quality {}",
                match self.quality {
                    Some(Quality::Fast) => "fast",
                    Some(Quality::High) => "high",
                    _ => "normal",
                }
            ),
        );
        say(
            self.colour_space.is_some(),
            format!(
                "colour space {}",
                match self.colour_space {
                    Some(ColourSpace::Linear) => "linear",
                    _ => "sRGB",
                }
            ),
        );
        say(self.flip_green == Some(true), "green flipped".to_string());
        if let Some([lo, hi]) = self.clamp {
            say(true, format!("clamp {lo} to {hi}"));
        }
        say(self.tonemap.is_some(), "tone map reinhard".to_string());
        if let Some(stops) = self.exposure {
            say(true, format!("exposure {stops} stops"));
        }
        if let Some(mode) = self.alpha {
            say(
                true,
                format!(
                    "alpha {}",
                    match mode {
                        AlphaMode::Straight => "straight",
                        AlphaMode::Premultiplied => "premultiplied",
                    }
                ),
            );
        }
        if let Some(cutoff) = self.alpha_coverage {
            say(true, format!("alpha coverage kept at {cutoff}"));
        }
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_in_64_bits_is_the_size_the_format_states() {
        let formats = [
            GrcTextureFormat::DXT1,
            GrcTextureFormat::DXT5,
            GrcTextureFormat::BC7,
            GrcTextureFormat::A8R8G8B8,
            GrcTextureFormat::L8,
            GrcTextureFormat::A32B32G32R32F,
        ];
        for format in formats {
            for (width, height) in [(1, 1), (4, 4), (64, 32), (100, 60), (2048, 2048)] {
                for mips in 1..=max_mip_levels(width, height) {
                    assert_eq!(
                        payload_size(format, width, height, mips),
                        u64::from(format.total_size(width, height, mips)),
                        "{format:?} {width}x{height} with {mips} mips"
                    );
                }
            }
        }
    }
}
