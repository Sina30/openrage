//! What a pack's files state of themselves that a rule holds to a limit.
//!
//! A scan counts it as the bytes of each file are read, so a rule is a function of what the files say and reads
//! none again. The size, mips and format of each texture of a dictionary, the levels of detail, shaders and
//! embedded textures of each drawable and the pages the game streams for the whole are counted from the
//! owned readers of `or-io`; a file they do not open has no count, and a rule that needs one is left out for it.

use std::path::Path;

use or_interchange::gltf::json::Document;
use or_interchange::gltf::read::lod_of;
use or_interchange::LodSlot;
use or_io::codec::mesh::VertexSource;
use or_io::codec::texture::{GrcTextureFormat, TextureSource};
use or_io::container::resource::Rsc7;
use or_io::container::ResourceEnvelope;
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::drawable::{DrawableTarget, GrmShaderGroup, GtaDrawable};
use or_io::gta5::gen8::{EntryData, GrcInstanceData, GrcTexture};
use or_io::gta5::gen9::{GrcInstanceDataG9, GrcTextureG9};
use or_io::gta5::{PgDictionary, TextureSlot};
use or_project::content::facts::{Fact, Measured};
use serde::{Deserialize, Serialize};

/// What one file holds, by the counts a rule reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measure {
    /// The pages the game allocates for the file, which its RSC7 header states.
    pub streaming: u64,
    /// Whether the lists below were read out of the file: false for a file that is no texture or model of the pack, or that the owned readers do not open.
    pub detail: bool,
    /// Whether the file is laid out for Enhanced.
    pub enhanced: bool,
    pub textures: Vec<Texture>,
    pub drawables: Vec<Drawable>,
}

/// One texture of a dictionary, as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Texture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    /// The pixel format by the engine's name, `DXT1`, `DXT5`, `BC7`, `A8R8G8B8`.
    pub format: String,
}

/// One drawable of a model file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drawable {
    pub name: String,
    /// High, medium, low and very low; none where the drawable has no model at that level.
    pub lods: [Option<Lod>; 4],
    /// How many shaders it draws with, and which of them, by index, bind no diffuse texture.
    pub shaders: u32,
    pub without_diffuse: Vec<u32>,
    /// The textures it carries inside itself that no shader of it binds.
    pub unused_embedded: Vec<String>,
    /// For each shader, by index, the constants its material lacks that the shader database declares for it; none where the database was not loaded when the file was counted, and for a model of Enhanced.
    #[serde(default)]
    pub missing_parameters: Vec<Missing>,
}

/// The parameters one material lacks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Missing {
    pub shader: u32,
    pub names: Vec<String>,
}

/// What one level of detail draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lod {
    pub vertices: u64,
    pub triangles: u64,
}

impl Measure {
    /// What `measured` says of a file, or `None` for a file nobody counted in.
    pub fn of(measured: &Measured) -> Option<Self> {
        serde_json::from_value(measured.clone()).ok()
    }

    /// What the facts say of the file `path`.
    pub fn of_fact(fact: Option<&Fact>) -> Option<Self> {
        match fact? {
            Fact::File {
                measured: Some(measured),
                ..
            } => Self::of(measured),
            _ => None,
        }
    }
}

/// The measurer a pack's scan counts each file with: a resource's pages, and, for a texture dictionary or a model, what it holds.
pub fn measure(name: &str, bytes: &[u8]) -> Option<Measured> {
    let seal = Rsc7::peek(bytes).ok()?;
    let mut counted = Measure {
        streaming: u64::from(seal.total_size()),
        ..Measure::default()
    };
    let ext = name
        .rsplit_once('.')
        .map_or("", |(_, e)| e)
        .to_ascii_lowercase();
    if let Some(format) = or_io::gta5::formats::format(&ext) {
        if let Ok(target) = format.target_of_version(seal.version()) {
            counted.enhanced = target.is_gen9();
            counted.detail = match (ext.as_str(), counted.enhanced) {
                ("ytd", false) => PgDictionary::<GrcTexture>::from_bytes(bytes)
                    .map(|d| counted.textures = textures(d.entries.iter().flatten().map(|b| &**b)))
                    .is_ok(),
                ("ytd", true) => PgDictionary::<GrcTextureG9>::from_bytes(bytes)
                    .map(|d| counted.textures = textures(d.entries.iter().flatten().map(|b| &**b)))
                    .is_ok(),
                ("ydd", false) => PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes)
                    .map(|d| {
                        counted.drawables = drawables(d.entries.iter().flatten().map(|b| &**b))
                    })
                    .is_ok(),
                ("ydd", true) => PgDictionary::<GtaDrawable<Gen9>>::from_bytes(bytes)
                    .map(|d| {
                        counted.drawables = drawables(d.entries.iter().flatten().map(|b| &**b))
                    })
                    .is_ok(),
                _ => false,
            };
        }
    }
    serde_json::to_value(counted).ok()
}

/// What a drawables source holds, counted from the glb of each drawable without cooking it, as a rule reads a compiled model.
///
/// A glb is a header, a chunk of JSON that says what each mesh is and how many vertices and indices it has, and the buffers; only the JSON is read.
/// A mesh belongs to the level the cook gives it (`lod_of`: its node's extras, else its name, else the first), its vertices are those of its positions and its indices are three to a triangle.
/// What a glb cannot state (shaders, embedded textures) is left empty, so only the rules of the levels and the triangles read it.
pub fn source_drawables(dir: &Path) -> Option<Vec<Drawable>> {
    let mut drawables: Vec<Drawable> = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir.join(or_source::models::DIR))
        .ok()?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let Some(doc) = glb_document(&entry.path().join(or_source::models::MODEL)) else {
            continue;
        };
        let counted = |accessor: Option<usize>| Some(doc.accessors.get(accessor?)?.count as u64);
        let mut lods: [Option<Lod>; 4] = [None; 4];
        for node in &doc.nodes {
            let Some(mesh) = node.mesh.and_then(|at| doc.meshes.get(at)) else {
                continue;
            };
            let LodSlot::Level(level) = lod_of(node, mesh.name.as_deref().unwrap_or_default())
            else {
                continue;
            };
            let Some(total) = lods.get_mut(usize::from(level)) else {
                continue;
            };
            for primitive in &mesh.primitives {
                let positions = counted(primitive.attributes.get("POSITION").copied());
                let indices = counted(primitive.indices).or(positions);
                if let Some(indices) = indices {
                    let held = total.get_or_insert(Lod {
                        vertices: 0,
                        triangles: 0,
                    });
                    held.vertices += positions.unwrap_or(0);
                    held.triangles += indices / 3;
                }
            }
        }
        drawables.push(Drawable {
            name: entry.file_name().to_string_lossy().into_owned(),
            lods,
            ..Drawable::default()
        });
    }
    (!drawables.is_empty()).then_some(drawables)
}

/// The glTF document in the JSON chunk of the glb at `file`, none where it is not one or does not read.
fn glb_document(file: &Path) -> Option<Document> {
    use std::io::Read;
    /// A glb's JSON chunk is the description of its meshes, kilobytes; a file that claims more is not read.
    const LARGEST: usize = 16 << 20;
    let mut open = std::fs::File::open(file).ok()?;
    let mut head = [0u8; 20];
    open.read_exact(&mut head).ok()?;
    let (magic, kind) = (&head[0..4], &head[16..20]);
    let length = usize::try_from(u32::from_le_bytes(head[12..16].try_into().ok()?)).ok()?;
    if magic != b"glTF" || kind != b"JSON" || length > LARGEST {
        return None;
    }
    let mut text = vec![0u8; length];
    open.read_exact(&mut text).ok()?;
    serde_json::from_slice(&text).ok()
}

fn textures<'a, T: TextureSource + Named + 'a>(
    entries: impl Iterator<Item = &'a T>,
) -> Vec<Texture> {
    entries
        .map(|texture| {
            let (width, height) = texture.stored_size();
            Texture {
                name: texture.name().to_string(),
                width,
                height,
                mips: texture.stored_levels(),
                format: format_name(texture.pixel_format()),
            }
        })
        .collect()
}

/// The name the engine gives a pixel format, which is how a rule and a person say it.
pub fn format_name(format: GrcTextureFormat) -> String {
    format!("{format:?}")
}

/// What a target's texture and shader let a count read.
trait Named {
    fn name(&self) -> &str;
}

impl Named for GrcTexture {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Named for GrcTextureG9 {
    fn name(&self) -> &str {
        &self.name
    }
}

/// What a target's drawable lets a count read that the trait of the target does not.
trait Counted: DrawableTarget {
    /// Whether the shader binds a texture to the parameter named by `hash`; `None` where it has no such parameter.
    fn binds(shader: &Self::InstanceData, hash: u32) -> Option<bool>;
    fn texture_name(texture: &Self::Texture) -> &str;
    /// The constants the shader database declares for the shader that its material does not carry.
    fn missing_parameters(shader: &Self::InstanceData) -> Vec<String>;
}

impl Counted for Gen8 {
    fn binds(shader: &GrcInstanceData, hash: u32) -> Option<bool> {
        match &shader.entry(hash)?.data {
            EntryData::Texture(slot) => Some(!matches!(slot, TextureSlot::Unbound)),
            EntryData::Float(_) => None,
        }
    }

    fn texture_name(texture: &GrcTexture) -> &str {
        &texture.name
    }

    fn missing_parameters(shader: &GrcInstanceData) -> Vec<String> {
        or_io::gta5::shaders::shader_defs::parameters_of(shader.basis_hash.get())
            .into_iter()
            .flatten()
            .filter(|d| !d.is_binding() && shader.entry(d.name_hash).is_none())
            .map(|d| d.name)
            .collect()
    }
}

impl Counted for Gen9 {
    fn binds(shader: &GrcInstanceDataG9, hash: u32) -> Option<bool> {
        let slot = shader.params.textures.get(shader.param(hash)?.texture()?)?;
        Some(!matches!(slot, TextureSlot::Unbound))
    }

    fn texture_name(texture: &GrcTextureG9) -> &str {
        &texture.name
    }

    fn missing_parameters(_: &GrcInstanceDataG9) -> Vec<String> {
        Vec::new()
    }
}

fn drawables<'a, T: Counted + 'a>(
    entries: impl Iterator<Item = &'a GtaDrawable<T>>,
) -> Vec<Drawable> {
    let diffuse = or_io::hash::string_hash_str("DiffuseSampler");
    entries
        .map(|drawable| {
            let mut counted = Drawable {
                name: drawable.name.clone(),
                ..Drawable::default()
            };
            for (level, lod) in drawable.lod_group.lods.iter().enumerate() {
                let geometries = || lod.models.iter().flat_map(|m| m.geometries.iter());
                if lod.models.is_empty() {
                    continue;
                }
                counted.lods[level] = Some(Lod {
                    vertices: geometries()
                        .map(|g| g.vertex_buffer.vertex_declared() as u64)
                        .sum(),
                    triangles: geometries().map(|g| u64::from(g.primitive_count())).sum(),
                });
            }
            if let Some(group) = &drawable.shader_group {
                counted.shaders = group.shaders.len() as u32;
                counted.without_diffuse = (group.shaders.iter().enumerate())
                    .filter(|(_, shader)| T::binds(shader, diffuse) == Some(false))
                    .map(|(i, _)| i as u32)
                    .collect();
                counted.unused_embedded = unused(group);
                counted.missing_parameters = (group.shaders.iter().enumerate())
                    .map(|(i, shader)| Missing {
                        shader: i as u32,
                        names: T::missing_parameters(shader),
                    })
                    .filter(|m| !m.names.is_empty())
                    .collect();
            }
            counted
        })
        .collect()
}

/// The textures a shader group carries in its own dictionary that none of its shaders binds.
fn unused<T: Counted>(group: &GrmShaderGroup<T>) -> Vec<String> {
    let Some(dictionary) = group.texture_dictionary.as_deref() else {
        return Vec::new();
    };
    let bound: Vec<u32> = group.shaders.iter().flat_map(|s| T::embedded(s)).collect();
    dictionary
        .codes
        .iter()
        .zip(&dictionary.entries)
        .filter(|(code, _)| !bound.contains(code))
        .map(|(code, texture)| match texture {
            Some(texture) => T::texture_name(texture).to_string(),
            None => format!("{code:#010x}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_no_resource_has_no_count() {
        assert_eq!(measure("shop.meta", b"<xml/>"), None);
        assert_eq!(measure("a.ytd", b""), None);
    }
}
