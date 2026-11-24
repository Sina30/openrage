//! A model as a worker read it: every level of detail its body holds, each mesh with the maps and
//! the bucket its material states.

use std::collections::HashMap;
use std::fs::File;

use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9ZlibRun};
use or_io::gta6::hash::name_hash32;
use or_io::gta6::headerless::{self, Drawable, Entry, ParameterValue, Resource, Shader};
use or_renderer::MeshVertex;

use super::index::{ModelRef, WorldIndex};
use super::texture::Places;

/// The bytes a model's body is read to.
const BODY_LIMIT: u64 = 512 << 20;

/// The hashes a material's record states for its colour map: a plain material's, then the first
/// layer's of a two-layer and of a four-layer one.
const COLOUR: [u32; 3] = [name_hash32(b"DiffuseTex"), 0x3463_94EA, 0xCE7B_968E];
/// The same for its normal map.
const NORMAL: [u32; 3] = [name_hash32(b"BumpTex"), 0x84C6_9350, 0x27DB_D2BE];
/// The hash a plain material's record states for its specular map.
// ponytail: a layered material states no map of this name, so it draws matte; its own material maps once their channels are measured.
const SPECULAR: u32 = name_hash32(b"SpecularTex");
/// The hash of the constant a material that glows scales its glow by.
const GLOW: u32 = name_hash32(b"EmissiveMultiplier");

/// A model on the CPU: its levels of detail, from the highest down.
pub struct CpuModel {
    pub lods: Vec<CpuLod>,
}

/// One level of a model, drawn while its instance is nearer than `until` metres; the last is drawn for ever.
pub struct CpuLod {
    pub until: f32,
    pub meshes: Vec<CpuMesh>,
}

/// One mesh of a level, before it is on the device.
pub struct CpuMesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub maps: Maps,
    pub bucket: Bucket,
}

/// The keys a material's maps are found by in the texture packs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Maps {
    /// `None` too for a material that states a normal map alone: a normal map is no colour.
    pub colour: Option<u64>,
    pub normal: Option<u64>,
    pub specular: Option<u64>,
}

/// How a mesh is drawn, as its material states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Bucket {
    #[default]
    Opaque,
    Cutout,
    Blend,
    Decal,
    Emissive,
}

impl Bucket {
    /// The bucket of a material whose draw bucket byte is `byte`, which glows where `glows`;
    /// `None` for one that draws no colour.
    fn of(byte: u8, glows: bool) -> Option<Self> {
        // ⚠ The high bit marks a material drawn in batches: it is no part of the bucket.
        match byte & 0x7F {
            0 if glows => Some(Self::Emissive),
            // 6 is a water surface.
            1 | 6 => Some(Self::Blend),
            2 => Some(Self::Decal),
            3 => Some(Self::Cutout),
            // Masks that keep water out of a hull.
            4 | 5 => None,
            _ => Some(Self::Opaque),
        }
    }
}

impl CpuModel {
    /// The bytes of its vertices and indices.
    pub fn bytes(&self) -> u64 {
        let meshes = self.lods.iter().flat_map(|lod| &lod.meshes);
        meshes
            .map(|m| (size_of_val(&*m.vertices) + size_of_val(&*m.indices)) as u64)
            .sum()
    }
}

/// Whether `shader` states a glow above none.
fn glows(shader: &Shader) -> bool {
    shader.parameters.iter().any(|p| match &p.value {
        ParameterValue::Constant { words, .. } if p.hash == GLOW => {
            words.first().is_some_and(|w| f32::from_bits(*w) > 0.0)
        }
        _ => false,
    })
}

/// The maps `shader` states: each from its own record, and for the colour of a material with no
/// such record the first key `srgb` holds, else the first it knows nothing of.
///
/// ⚠ A key a pack states as linear is a data or normal map: drawn as a colour it tints the whole
/// mesh green and blue.
fn maps_of(shader: &Shader, srgb: &dyn Fn(u64) -> Option<bool>) -> Maps {
    let key = |slot: u8| shader.texture_keys.get(usize::from(slot)).copied().flatten();
    let record = |hash: u32| {
        (shader.parameters.iter()).find_map(|p| match p.value {
            ParameterValue::Texture { slot } if p.hash == hash => key(slot),
            _ => None,
        })
    };
    let normal = NORMAL.into_iter().find_map(record);
    let specular = record(SPECULAR);
    let others = || {
        let keys = shader.texture_keys.iter().flatten().copied();
        keys.filter(move |k| Some(*k) != normal && Some(*k) != specular)
    };
    let colour = (COLOUR.into_iter().find_map(record))
        .or_else(|| others().find(|k| srgb(*k) == Some(true)))
        .or_else(|| others().find(|k| srgb(*k).is_none()));
    Maps {
        colour,
        normal,
        specular,
    }
}

/// `drawable` as a model: each level that holds a mesh, with the distance its body states, each
/// mesh's bucket from its shader's byte of `buckets`.
pub(super) fn model_of(
    drawable: &Drawable,
    buckets: &[u8],
    srgb: &dyn Fn(u64) -> Option<bool>,
) -> CpuModel {
    let materials: Vec<Option<(Maps, Bucket)>> = (drawable.shaders.iter().enumerate())
        .map(|(at, shader)| {
            let byte = buckets.get(at).copied().unwrap_or(0);
            Bucket::of(byte, glows(shader)).map(|bucket| (maps_of(shader, srgb), bucket))
        })
        .collect();
    let mut lods = Vec::new();
    for (level, models) in drawable.levels.iter().enumerate() {
        let Some(models) = models.as_ref().filter(|models| !models.is_empty()) else {
            continue;
        };
        let read = crate::render::gta6_drawable::meshes_at(drawable, level, None, None, &|_| None);
        // ⚠ One mesh comes back for each geometry whose positions read, in the models' order.
        let shaders = (models.iter())
            .flat_map(|model| model.geometries.iter().flatten())
            .filter(|geometry| geometry.positions.is_ok())
            .map(|geometry| geometry.shader);
        let meshes: Vec<CpuMesh> = (read.into_iter().zip(shaders))
            .filter(|(mesh, _)| !mesh.vertices.is_empty() && !mesh.indices.is_empty())
            .filter_map(|(mesh, shader)| {
                let (maps, bucket) = (*materials.get(usize::from(shader))?)?;
                Some(CpuMesh {
                    vertices: mesh.vertices,
                    indices: mesh.indices,
                    maps,
                    bucket,
                })
            })
            .collect();
        if !meshes.is_empty() {
            lods.push(CpuLod {
                until: drawable.lod_distances[level],
                meshes,
            });
        }
    }
    // What cuts a model off is its entity's own distance, never its last level's.
    match lods.last_mut() {
        Some(last) => last.until = f32::INFINITY,
        None => lods.push(CpuLod {
            until: f32::INFINITY,
            meshes: Vec::new(),
        }),
    }
    CpuModel { lods }
}

/// The model `model` names, read from its archive; `None` where it does not read.
///
/// A fragment is its one drawable: every part, its glass too, is a geometry of it at rest.
pub(super) fn read_model(
    files: &mut HashMap<u32, File>,
    index: &WorldIndex,
    places: Option<&Places>,
    model: ModelRef,
) -> Option<CpuModel> {
    let file = match files.entry(model.archive) {
        std::collections::hash_map::Entry::Occupied(open) => open.into_mut(),
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(File::open(index.archives.get(model.archive as usize)?).ok()?)
        }
    };
    let (find, bytes) = read_rpf9_body(&*file, model.offset, BODY_LIMIT).ok()?;
    let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
        resource: Some(resource),
        signature: Some(signature),
        ..
    }) = find.kind
    else {
        return None;
    };
    let (virtual_section, physical) = resource.sections(&bytes);
    let read = headerless::read(signature, virtual_section, Some(physical)).ok()?;
    let drawable = match (&read, model.entry) {
        (Resource::Drawable(d), None) => d,
        (Resource::DrawableWithBound(r), None) => &r.drawable,
        (Resource::Dictionary(dictionary), Some(code)) => {
            (dictionary.entries.iter()).find_map(|(c, e)| match e {
                Ok(Entry::Drawable(d)) if *c == code => Some(&**d),
                _ => None,
            })?
        }
        _ => return None,
    };
    // Bytes that do not read leave every mesh opaque.
    let buckets =
        headerless::shader_buckets(signature, virtual_section, model.entry).unwrap_or_default();
    // With the packs indexed a key is a colour map or it is not; with none nothing is known of it.
    let srgb = |key: u64| places.map(|places| places.is_colour(key));
    Some(model_of(drawable, &buckets, &srgb))
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
