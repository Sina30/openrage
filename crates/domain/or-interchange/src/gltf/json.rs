//! The glTF 2.0 document, as the structs its JSON serializes to.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod component {
    pub const UNSIGNED_SHORT: u32 = 5123;
    pub const UNSIGNED_INT: u32 = 5125;
    pub const FLOAT: u32 = 5126;
}

pub mod target {
    pub const ARRAY_BUFFER: u32 = 34962;
    pub const ELEMENT_ARRAY_BUFFER: u32 = 34963;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
}

impl Default for Asset {
    fn default() -> Self {
        Self {
            version: "2.0".to_string(),
            generator: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Buffer {
    pub byte_length: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferView {
    pub buffer: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub byte_offset: usize,
    pub byte_length: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_stride: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accessor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_view: Option<usize>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub byte_offset: usize,
    pub component_type: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub normalized: bool,
    pub count: usize,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Vec<f32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Primitive {
    pub attributes: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indices: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mesh {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub primitives: Vec<Primitive>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<[f32; 3]>,
    /// xyzw, which is glTF's order and `crBoneData`'s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f32; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub nodes: Vec<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inverse_bind_matrices: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skeleton: Option<usize>,
    pub joints: Vec<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureRef {
    pub index: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tex_coord: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PbrMetallicRoughness {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color_texture: Option<TextureRef>,
    pub metallic_factor: f32,
    pub roughness_factor: f32,
}

impl Default for PbrMetallicRoughness {
    /// ⚠ Not glTF's own defaults. A RAGE material states no metallic and no
    /// roughness, and glTF's defaults are a fully metallic, fully rough
    /// surface — which renders every exported mesh as dark metal.
    fn default() -> Self {
        Self {
            base_color_texture: None,
            metallic_factor: 0.0,
            roughness_factor: 1.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Material {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub pbr_metallic_roughness: PbrMetallicRoughness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_texture: Option<TextureRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_cutoff: Option<f32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub double_sided: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Texture {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_view: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sampler {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mag_filter: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_filter: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap_t: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub asset: Asset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<SceneEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meshes: Vec<Mesh>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skins: Vec<Skin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<Material>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub textures: Vec<Texture>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Image>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samplers: Vec<Sampler>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accessors: Vec<Accessor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buffer_views: Vec<BufferView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buffers: Vec<Buffer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}
