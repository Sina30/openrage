//! A [`Scene`] written as glTF 2.0, in either container the format defines.

pub mod json;
pub mod read;

use std::collections::BTreeMap;

use glam::{Mat4, Quat, Vec3};
use serde_json::{json, Value};

use crate::scene::{AlphaMode, Bone, LodSlot, Object, Primitive, Scene};
use json::{component, target, Document};

/// Z-up to Y-up: a quarter turn about X, as xyzw.
///
/// ⚠ **Carried on one root node rather than applied to the data.** Every
/// position, normal, tangent and bind matrix stays in the drawable's own
/// frame, so an export changes no number and a later import has nothing to
/// undo. Rotating a bone hierarchy in place is the alternative, and it is
/// wrong below the root: `local' = R·L` ignores that the parent's frame
/// turned too, which shows as a skeleton correct in bind pose and sheared
/// the moment it moves.
pub const BASIS_ROTATION: [f32; 4] = [
    -std::f32::consts::FRAC_1_SQRT_2,
    0.0,
    0.0,
    std::f32::consts::FRAC_1_SQRT_2,
];

const GLB_MAGIC: u32 = 0x4654_6C67;
const GLB_JSON: u32 = 0x4E4F_534A;
const GLB_BIN: u32 = 0x004E_4942;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// One `.glb`, with the buffer and every image inside it.
    Glb,
    /// A `.gltf` beside a `.bin` and one `.png` per texture.
    Separate,
}

/// What an export produced: the file asked for, and anything it refers to.
#[derive(Debug, Clone, Default)]
pub struct Files {
    pub primary: Vec<u8>,
    /// File name relative to the primary file's directory.
    pub sidecars: Vec<(String, Vec<u8>)>,
}

pub fn write(scene: &Scene, stem: &str, container: Container) -> Files {
    Writer::new(scene, stem, container).run()
}

struct Writer<'a> {
    scene: &'a Scene,
    stem: String,
    container: Container,
    doc: Document,
    bin: Vec<u8>,
    sidecars: Vec<(String, Vec<u8>)>,
}

impl<'a> Writer<'a> {
    fn new(scene: &'a Scene, stem: &str, container: Container) -> Self {
        Self {
            scene,
            stem: sanitize(stem),
            container,
            doc: Document {
                asset: json::Asset {
                    version: "2.0".to_string(),
                    generator: Some("OpenRage or-interchange".to_string()),
                },
                ..Document::default()
            },
            bin: Vec::new(),
            sidecars: Vec::new(),
        }
    }

    fn run(mut self) -> Files {
        self.write_images();
        self.write_materials();

        let mut nodes: Vec<json::Node> = vec![json::Node::default()];
        let mut root_children = Vec::new();
        let mut loose = Vec::new();
        for object in &self.scene.objects {
            let at = self.write_object(object, &mut nodes, &mut loose);
            root_children.push(at);
        }
        nodes[0] = json::Node {
            name: Some(self.scene.name.clone()),
            children: root_children,
            rotation: Some(BASIS_ROTATION),
            ..json::Node::default()
        };
        self.doc.nodes = nodes;
        // ⚠ A skinned mesh node sits at the scene root rather than under the
        // basis, because glTF ignores its transform outright: its vertices
        // are placed by the joints, which *are* under the basis.
        let mut roots = vec![0];
        roots.extend(loose);
        self.doc.scenes = vec![json::SceneEntry {
            name: Some(self.scene.name.clone()),
            nodes: roots,
        }];
        self.doc.scene = Some(0);
        self.doc.extras = Some(report_extras(self.scene));

        self.finish()
    }

    fn write_images(&mut self) {
        if self.scene.images.is_empty() {
            return;
        }
        self.doc.samplers = vec![json::Sampler {
            mag_filter: Some(9729),
            min_filter: Some(9987),
            wrap_s: Some(10497),
            wrap_t: Some(10497),
        }];
        for image in &self.scene.images {
            let entry = match self.container {
                Container::Glb => {
                    let view = self.push_view(&image.png.clone(), None);
                    json::Image {
                        name: Some(image.name.clone()),
                        buffer_view: Some(view),
                        mime_type: Some("image/png".to_string()),
                        ..json::Image::default()
                    }
                }
                Container::Separate => {
                    let file = format!("{}_{}.png", self.stem, sanitize(&image.name));
                    self.sidecars.push((file.clone(), image.png.clone()));
                    json::Image {
                        name: Some(image.name.clone()),
                        uri: Some(file),
                        ..json::Image::default()
                    }
                }
            };
            let source = self.doc.images.len();
            self.doc.images.push(entry);
            self.doc.textures.push(json::Texture {
                sampler: Some(0),
                source: Some(source),
                name: Some(self.scene.images[source].name.clone()),
            });
        }
    }

    fn write_materials(&mut self) {
        for material in &self.scene.materials {
            let (mode, cutoff) = match material.alpha_mode {
                AlphaMode::Opaque => (None, None),
                AlphaMode::Mask => (Some("MASK".to_string()), Some(0.5)),
                AlphaMode::Blend => (Some("BLEND".to_string()), None),
            };
            self.doc.materials.push(json::Material {
                name: Some(material.name.clone()),
                pbr_metallic_roughness: json::PbrMetallicRoughness {
                    base_color_texture: material.base_colour.map(texture_ref),
                    ..json::PbrMetallicRoughness::default()
                },
                normal_texture: material.normal.map(texture_ref),
                alpha_mode: mode,
                alpha_cutoff: cutoff,
                double_sided: false,
                extras: Some(material_extras(material)),
            });
        }
    }

    fn write_object(
        &mut self,
        object: &Object,
        nodes: &mut Vec<json::Node>,
        loose: &mut Vec<usize>,
    ) -> usize {
        let object_at = nodes.len();
        nodes.push(json::Node::default());
        let mut children = Vec::new();

        let bone_base = nodes.len();
        for bone in &object.bones {
            nodes.push(json::Node {
                name: Some(bone.name.clone()),
                translation: Some(bone.translation),
                rotation: Some(bone.rotation),
                scale: Some(fixed_scale(bone.scale).to_array()),
                extras: Some(json!({ "rage": { "boneTag": bone.tag } })),
                ..json::Node::default()
            });
        }
        let mut roots = Vec::new();
        for (index, bone) in object.bones.iter().enumerate() {
            match bone.parent {
                // ⚠ A bone naming itself, or a parent this skeleton does not
                // have, would make the node graph cyclic — which glTF forbids
                // and which a reader is entitled to recurse into forever.
                Some(parent) if parent != index && parent < object.bones.len() => {
                    nodes[bone_base + parent].children.push(bone_base + index)
                }
                _ => roots.push(bone_base + index),
            }
        }
        children.extend(roots.iter().copied());

        let skin = if object.is_skinned() {
            let matrices = self.push_inverse_bind(&object.bones);
            let at = self.doc.skins.len();
            self.doc.skins.push(json::Skin {
                name: Some(format!("{}_skin", object.name)),
                inverse_bind_matrices: Some(matrices),
                skeleton: roots.first().copied(),
                joints: (0..object.bones.len()).map(|i| bone_base + i).collect(),
            });
            Some(at)
        } else {
            None
        };

        for mesh in &object.meshes {
            let skinned = skin.is_some() && mesh.is_skinned();
            let primitives: Vec<json::Primitive> = mesh
                .primitives
                .iter()
                .map(|p| self.write_primitive(p, skinned))
                .collect();
            if primitives.is_empty() {
                continue;
            }
            let mesh_at = self.doc.meshes.len();
            self.doc.meshes.push(json::Mesh {
                name: Some(format!("{}_{}", object.name, mesh.name)),
                primitives,
            });
            if skinned {
                loose.push(nodes.len());
            } else {
                children.push(nodes.len());
            }
            nodes.push(json::Node {
                name: Some(format!("{}_{}", object.name, mesh.name)),
                mesh: Some(mesh_at),
                skin: if skinned { skin } else { None },
                extras: Some(json!({ "rage": { "lod": mesh.lod.label() } })),
                ..json::Node::default()
            });
        }

        nodes[object_at] = json::Node {
            name: Some(object.name.clone()),
            children,
            ..json::Node::default()
        };
        object_at
    }

    fn write_primitive(&mut self, primitive: &Primitive, skinned: bool) -> json::Primitive {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "POSITION".to_string(),
            self.push_vec3(&primitive.positions, true),
        );
        if primitive.normals.len() == primitive.positions.len() {
            attributes.insert(
                "NORMAL".to_string(),
                self.push_vec3(&primitive.normals, false),
            );
        }
        if primitive.tangents.len() == primitive.positions.len() {
            attributes.insert("TANGENT".to_string(), self.push_vec4(&primitive.tangents));
        }
        for (slot, uv) in primitive.uvs.iter().enumerate() {
            if uv.len() != primitive.positions.len() {
                continue;
            }
            attributes.insert(format!("TEXCOORD_{slot}"), self.push_vec2(uv));
        }
        for (slot, colour) in primitive.colours.iter().enumerate() {
            if colour.len() != primitive.positions.len() {
                continue;
            }
            attributes.insert(format!("COLOR_{slot}"), self.push_vec4(colour));
        }
        // ⚠ Skinning goes out only when a skeleton went with it. A `.ydd` of
        // ped components states `matrix_palette` indices into a skeleton that
        // lives in another file, and glTF has no way to say that: the
        // attributes without a `skin` are attributes every reader discards,
        // and Blender's importer drops them rather than making vertex groups.
        // The interchange model keeps them either way.
        if skinned && primitive.joints.len() == primitive.positions.len() {
            attributes.insert("JOINTS_0".to_string(), self.push_u16x4(&primitive.joints));
            attributes.insert(
                "WEIGHTS_0".to_string(),
                self.push_vec4_from(&primitive.weights),
            );
        }

        json::Primitive {
            attributes,
            indices: Some(self.push_indices(&primitive.indices, primitive.positions.len())),
            material: primitive.material,
            mode: None,
            extras: Some(primitive_extras(primitive)),
        }
    }

    fn push_inverse_bind(&mut self, bones: &[Bone]) -> usize {
        let world = bind_world(bones);
        let mut bytes = Vec::with_capacity(bones.len() * 64);
        for matrix in &world {
            let mut columns = matrix.inverse().to_cols_array();
            // ⚠ The bottom row is pinned rather than inverted. A bone's
            // transform is affine, so the exact answer here is (0, 0, 0, 1) —
            // but a 4x4 inversion reaches it through the determinant and
            // lands on 1.0000001, which the Khronos validator rejects as
            // ACCESSOR_INVALID_IBM. Measured on `head_010_r.ydd`: 17 of its
            // 128 bones failed, every one of them in this element alone.
            columns[3] = 0.0;
            columns[7] = 0.0;
            columns[11] = 0.0;
            columns[15] = 1.0;
            for value in columns {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let view = self.push_view(&bytes, None);
        self.push_accessor(json::Accessor {
            buffer_view: Some(view),
            component_type: component::FLOAT,
            count: bones.len(),
            kind: "MAT4".to_string(),
            ..json::Accessor::default()
        })
    }

    fn push_vec3(&mut self, values: &[[f32; 3]], with_bounds: bool) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 12);
        for value in values {
            for axis in value {
                bytes.extend_from_slice(&axis.to_le_bytes());
            }
        }
        let view = self.push_view(&bytes, Some(target::ARRAY_BUFFER));
        // ⚠ `min`/`max` are required on `POSITION` and must be finite: a
        // non-finite bound serializes as JSON `null`, which is a document no
        // reader accepts. The vertex data itself is left as the file has it.
        let (min, max) = if with_bounds {
            let bounds = finite_bounds(values);
            (Some(bounds.0.to_vec()), Some(bounds.1.to_vec()))
        } else {
            (None, None)
        };
        self.push_accessor(json::Accessor {
            buffer_view: Some(view),
            component_type: component::FLOAT,
            count: values.len(),
            kind: "VEC3".to_string(),
            min,
            max,
            ..json::Accessor::default()
        })
    }

    fn push_vec2(&mut self, values: &[[f32; 2]]) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 8);
        for value in values {
            for axis in value {
                bytes.extend_from_slice(&axis.to_le_bytes());
            }
        }
        let view = self.push_view(&bytes, Some(target::ARRAY_BUFFER));
        self.push_accessor(json::Accessor {
            buffer_view: Some(view),
            component_type: component::FLOAT,
            count: values.len(),
            kind: "VEC2".to_string(),
            ..json::Accessor::default()
        })
    }

    fn push_vec4(&mut self, values: &[[f32; 4]]) -> usize {
        self.push_vec4_from(values)
    }

    fn push_vec4_from(&mut self, values: &[[f32; 4]]) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 16);
        for value in values {
            for axis in value {
                bytes.extend_from_slice(&axis.to_le_bytes());
            }
        }
        let view = self.push_view(&bytes, Some(target::ARRAY_BUFFER));
        self.push_accessor(json::Accessor {
            buffer_view: Some(view),
            component_type: component::FLOAT,
            count: values.len(),
            kind: "VEC4".to_string(),
            ..json::Accessor::default()
        })
    }

    fn push_u16x4(&mut self, values: &[[u16; 4]]) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 8);
        for value in values {
            for lane in value {
                bytes.extend_from_slice(&lane.to_le_bytes());
            }
        }
        let view = self.push_view(&bytes, Some(target::ARRAY_BUFFER));
        self.push_accessor(json::Accessor {
            buffer_view: Some(view),
            component_type: component::UNSIGNED_SHORT,
            count: values.len(),
            kind: "VEC4".to_string(),
            ..json::Accessor::default()
        })
    }

    fn push_indices(&mut self, indices: &[u32], vertices: usize) -> usize {
        let narrow = vertices <= usize::from(u16::MAX);
        let mut bytes = Vec::with_capacity(indices.len() * if narrow { 2 } else { 4 });
        for index in indices {
            if narrow {
                bytes.extend_from_slice(&(*index as u16).to_le_bytes());
            } else {
                bytes.extend_from_slice(&index.to_le_bytes());
            }
        }
        let view = self.push_view(&bytes, Some(target::ELEMENT_ARRAY_BUFFER));
        self.push_accessor(json::Accessor {
            buffer_view: Some(view),
            component_type: if narrow {
                component::UNSIGNED_SHORT
            } else {
                component::UNSIGNED_INT
            },
            count: indices.len(),
            kind: "SCALAR".to_string(),
            ..json::Accessor::default()
        })
    }

    fn push_accessor(&mut self, accessor: json::Accessor) -> usize {
        self.doc.accessors.push(accessor);
        self.doc.accessors.len() - 1
    }

    /// ⚠ Every view starts on a four-byte boundary. glTF requires an
    /// accessor's offset to be a multiple of its component size, and a
    /// `bufferView` carrying floats after an odd-length PNG otherwise lands
    /// one byte off for the whole run.
    fn push_view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.doc.buffer_views.push(json::BufferView {
            buffer: 0,
            byte_offset: offset,
            byte_length: bytes.len(),
            byte_stride: None,
            target,
        });
        self.doc.buffer_views.len() - 1
    }

    fn finish(mut self) -> Files {
        match self.container {
            Container::Separate => {
                let bin_name = format!("{}.bin", self.stem);
                if !self.bin.is_empty() {
                    self.doc.buffers.push(json::Buffer {
                        byte_length: self.bin.len(),
                        uri: Some(bin_name.clone()),
                    });
                    self.sidecars
                        .push((bin_name, std::mem::take(&mut self.bin)));
                }
                let text =
                    serde_json::to_string_pretty(&self.doc).unwrap_or_else(|_| "{}".to_string());
                Files {
                    primary: text.into_bytes(),
                    sidecars: self.sidecars,
                }
            }
            Container::Glb => {
                if !self.bin.is_empty() {
                    self.doc.buffers.push(json::Buffer {
                        byte_length: self.bin.len(),
                        uri: None,
                    });
                }
                let text = serde_json::to_string(&self.doc).unwrap_or_else(|_| "{}".to_string());
                Files {
                    primary: glb(text.as_bytes(), &self.bin),
                    sidecars: self.sidecars,
                }
            }
        }
    }
}

/// The 12-byte header and two padded chunks a `.glb` is.
fn glb(json: &[u8], bin: &[u8]) -> Vec<u8> {
    // ⚠ The JSON chunk pads with spaces and the binary chunk with zeroes, and
    // both are part of the declared chunk length. A reader that trusts the
    // length reads the padding as content if it is the wrong byte.
    let json_pad = (4 - json.len() % 4) % 4;
    let bin_pad = (4 - bin.len() % 4) % 4;
    let json_len = json.len() + json_pad;
    let bin_len = bin.len() + bin_pad;
    let total = 12 + 8 + json_len + if bin.is_empty() { 0 } else { 8 + bin_len };

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());

    out.extend_from_slice(&(json_len as u32).to_le_bytes());
    out.extend_from_slice(&GLB_JSON.to_le_bytes());
    out.extend_from_slice(json);
    out.extend(std::iter::repeat_n(b' ', json_pad));

    if !bin.is_empty() {
        out.extend_from_slice(&(bin_len as u32).to_le_bytes());
        out.extend_from_slice(&GLB_BIN.to_le_bytes());
        out.extend_from_slice(bin);
        out.extend(std::iter::repeat_n(0u8, bin_pad));
    }
    out
}

fn texture_ref(index: usize) -> json::TextureRef {
    json::TextureRef {
        index,
        tex_coord: 0,
    }
}

fn material_extras(material: &crate::scene::Material) -> Value {
    let textures: Vec<Value> = material
        .textures
        .iter()
        .map(|binding| {
            json!({
                "parameter": binding.parameter,
                "texture": binding.texture,
                "carried": binding.image.is_some(),
            })
        })
        .collect();
    let parameters: BTreeMap<&str, &Vec<[f32; 4]>> = material
        .vectors
        .iter()
        .map(|(name, values)| (name.as_str(), values))
        .collect();
    json!({
        "rage": {
            "shader": material.shader,
            "drawBucket": material.draw_bucket,
            "textures": textures,
            "parameters": parameters,
        }
    })
}

fn primitive_extras(primitive: &Primitive) -> Value {
    json!({
        "rage": {
            "lod": primitive.lod.label(),
            "model": primitive.model,
            "geometry": primitive.geometry,
            "uvChannels": primitive.uv_channels,
            "colourChannels": primitive.colour_channels,
        }
    })
}

fn report_extras(scene: &Scene) -> Value {
    let report = &scene.report;
    json!({
        "rage": {
            "source": scene.name,
            "upAxis": "Z",
            "geometries": report.geometries,
            "dropped": {
                "geometriesWithoutPositions": report.geometries_without_positions,
                "geometriesNotTriangles": report.geometries_not_triangles,
                "geometriesWithPartialTriangles": report.geometries_with_partial_triangles,
                "indicesOutOfRange": report.indices_out_of_range,
                "influencesOutsideThePalette": report.influences_outside_the_palette,
                "shaderIndicesOutOfRange": report.shader_indices_out_of_range,
                "texturesNotCarried": report.textures_not_carried,
                "texturesNotDecoded": report.textures_not_decoded,
            }
        }
    })
}

/// A scale no inversion can divide by.
fn fixed_scale(scale: [f32; 3]) -> Vec3 {
    Vec3::new(axis(scale[0]), axis(scale[1]), axis(scale[2]))
}

fn axis(value: f32) -> f32 {
    if value.is_finite() && value != 0.0 {
        value
    } else {
        1.0
    }
}

fn local_of(bone: &Bone) -> Mat4 {
    let rotation = Quat::from_xyzw(
        bone.rotation[0],
        bone.rotation[1],
        bone.rotation[2],
        bone.rotation[3],
    );
    let rotation = if rotation.length_squared() > 1e-6 {
        rotation.normalize()
    } else {
        Quat::IDENTITY
    };
    Mat4::from_scale_rotation_translation(
        fixed_scale(bone.scale),
        rotation,
        Vec3::from(bone.translation),
    )
}

/// Each bone's bind transform in the drawable's frame.
///
/// ⚠ Walks up rather than accumulating forwards: `crSkeletonData` stores
/// bones in file order and nothing guarantees a parent precedes its child.
fn bind_world(bones: &[Bone]) -> Vec<Mat4> {
    let locals: Vec<Mat4> = bones.iter().map(local_of).collect();
    (0..bones.len())
        .map(|index| {
            let mut matrix = locals[index];
            let mut at = bones[index].parent;
            let mut guard = bones.len();
            while let Some(parent) = at {
                if parent >= bones.len() || parent == index || guard == 0 {
                    break;
                }
                matrix = locals[parent] * matrix;
                at = bones[parent].parent;
                guard -= 1;
            }
            matrix
        })
        .collect()
}

fn finite_bounds(values: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut any = false;
    for value in values {
        if !value.iter().all(|v| v.is_finite()) {
            continue;
        }
        for axis in 0..3 {
            min[axis] = min[axis].min(value[axis]);
            max[axis] = max[axis].max(value[axis]);
        }
        any = true;
    }
    if any {
        (min, max)
    } else {
        ([0.0; 3], [0.0; 3])
    }
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "mesh".to_string()
    } else {
        cleaned
    }
}

/// Whether a reader that shows one LOD should show this one.
pub fn default_visible(lod: LodSlot) -> bool {
    lod.is_default_visible()
}
