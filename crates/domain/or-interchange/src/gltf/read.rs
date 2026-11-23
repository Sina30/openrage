//! A glTF 2.0 document read back into a [`Scene`].
//!
//! ⚠ **Anything this cannot map is refused, never approximated**: an importer
//! is where a wrong guess becomes bytes the game reads. Every unsupported
//! component type, accessor shape and buffer reference is an error naming what
//! it found.

use std::collections::BTreeMap;
use std::path::Path;

use glam::{Mat4, Quat, Vec3};
use serde::Deserialize;
use serde_json::Value;

use super::json::{component, Document};
use super::BASIS_ROTATION;
use crate::scene::{Bone, Dropped, LodSlot, Mesh, Object, Primitive, Scene};

/// Why a glTF document could not be read as a mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError(pub String);

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReadError {}

type Read<T> = std::result::Result<T, ReadError>;

fn fail<T>(message: impl Into<String>) -> Read<T> {
    Err(ReadError(message.into()))
}

const GLB_MAGIC: u32 = 0x4654_6C67;
const GLB_JSON: u32 = 0x4E4F_534A;
const GLB_BIN: u32 = 0x004E_4942;

/// The JSON and the binary chunk of whichever container this is.
struct Loaded {
    document: Document,
    buffers: Vec<Vec<u8>>,
    beyond: Beyond,
}

/// What a document states that [`Document`] does not model, read once so the report can count it.
#[derive(Default, Deserialize)]
struct Beyond {
    #[serde(default)]
    animations: Vec<Value>,
    #[serde(default)]
    cameras: Vec<Value>,
    #[serde(default)]
    extensions: Option<Value>,
    #[serde(default)]
    meshes: Vec<BeyondMesh>,
}

#[derive(Default, Deserialize)]
struct BeyondMesh {
    #[serde(default)]
    primitives: Vec<BeyondPrimitive>,
}

#[derive(Default, Deserialize)]
struct BeyondPrimitive {
    #[serde(default)]
    targets: Vec<Value>,
    #[serde(default)]
    attributes: BTreeMap<String, Value>,
}

/// `text` as [`Beyond`]; a document that parsed as a [`Document`] has no reason not to.
fn beyond_of(text: &str) -> Beyond {
    serde_json::from_str(text).unwrap_or_default()
}

/// The attributes of a primitive that the reader makes no channel of, other than a fifth influence.
///
/// ⚠ `TEXCOORD_n` and `COLOR_n` are read from nought upward until one is missing, so one past a
/// gap is not read, and neither is any past the eighth and the second.
fn unread(attributes: &BTreeMap<String, Value>) -> impl Iterator<Item = &str> {
    let run = |prefix: &str, most: usize| {
        (0..most)
            .take_while(|n| attributes.contains_key(&format!("{prefix}{n}")))
            .count()
    };
    let (uvs, colours) = (run("TEXCOORD_", 8), run("COLOR_", 2));
    attributes.keys().map(String::as_str).filter(move |a| {
        let ordinal = |prefix: &str| a.strip_prefix(prefix).and_then(|n| n.parse::<usize>().ok());
        match *a {
            "POSITION" | "NORMAL" | "TANGENT" => false,
            _ if a.starts_with("JOINTS_") || a.starts_with("WEIGHTS_") => false,
            _ => match (ordinal("TEXCOORD_"), ordinal("COLOR_")) {
                (Some(n), _) => n >= uvs,
                (_, Some(n)) => n >= colours,
                _ => true,
            },
        }
    })
}

/// What the file holds that an edit of a drawable has no place for.
///
/// ⚠ Materials and textures are always among it: an edit replaces a geometry's vertices and
/// keeps the drawable's own shaders and textures.
fn dropped_of(loaded: &Loaded) -> Vec<Dropped> {
    let (doc, beyond) = (&loaded.document, &loaded.beyond);
    let primitives = || beyond.meshes.iter().flat_map(|m| m.primitives.iter());
    let lights = beyond
        .extensions
        .as_ref()
        .and_then(|e| e.get("KHR_lights_punctual")?.get("lights")?.as_array())
        .map_or(0, Vec::len);
    let mut other: BTreeMap<&str, usize> = BTreeMap::new();
    for attribute in primitives().flat_map(|p| unread(&p.attributes)) {
        *other.entry(attribute).or_default() += 1;
    }

    let mut out = Vec::new();
    let mut note = |what: &str, count: usize| {
        if count > 0 {
            out.push(Dropped::new(what, count));
        }
    };
    note("materials", doc.materials.len());
    note("textures", doc.textures.len().max(doc.images.len()));
    note("animations", beyond.animations.len());
    note("cameras", beyond.cameras.len());
    note("lights", lights);
    note("skins after the first", doc.skins.len().saturating_sub(1));
    note(
        "primitives with morph targets",
        primitives().filter(|p| !p.targets.is_empty()).count(),
    );
    note(
        "primitives with more than four influences",
        primitives()
            .filter(|p| p.attributes.contains_key("JOINTS_1"))
            .count(),
    );
    for (attribute, count) in other {
        note(&format!("primitives with the attribute {attribute}"), count);
    }
    out
}

fn u32_at(bytes: &[u8], at: usize) -> Read<u32> {
    let end = at + 4;
    if end > bytes.len() {
        return fail(format!(
            "the file ends at {} and a word was read at {at}",
            bytes.len()
        ));
    }
    Ok(u32::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
    ]))
}

/// Read a `.glb` or a `.gltf`, resolving a `.bin` beside the latter.
pub fn load(path: &Path, bytes: &[u8]) -> Read<Scene> {
    let loaded = if bytes.len() >= 12 && u32_at(bytes, 0)? == GLB_MAGIC {
        load_glb(bytes)?
    } else {
        load_gltf(path, bytes)?
    };
    scene_of(&loaded)
}

fn load_glb(bytes: &[u8]) -> Read<Loaded> {
    let version = u32_at(bytes, 4)?;
    if version != 2 {
        return fail(format!("glTF binary version {version}; only 2 is defined"));
    }
    let total = u32_at(bytes, 8)? as usize;
    if total > bytes.len() {
        return fail(format!(
            "the header declares {total} bytes and the file holds {}",
            bytes.len()
        ));
    }

    let mut at = 12usize;
    let mut json: Option<&[u8]> = None;
    let mut bin: Option<&[u8]> = None;
    while at + 8 <= total {
        let length = u32_at(bytes, at)? as usize;
        let kind = u32_at(bytes, at + 4)?;
        let start = at + 8;
        let end = start
            .checked_add(length)
            .filter(|e| *e <= bytes.len())
            .ok_or_else(|| {
                ReadError(format!(
                    "a chunk at {at} declares {length} bytes and runs past the file"
                ))
            })?;
        match kind {
            GLB_JSON => json = Some(&bytes[start..end]),
            GLB_BIN => bin = Some(&bytes[start..end]),
            // An unknown chunk is skipped, which the specification requires.
            _ => {}
        }
        at = end;
    }

    let Some(json) = json else {
        return fail("the glTF binary carries no JSON chunk");
    };
    let text = std::str::from_utf8(json)
        .map_err(|_| ReadError("the JSON chunk is not UTF-8".to_string()))?;
    let text = text.trim_end_matches(['\0', ' ']);
    let document: Document = serde_json::from_str(text)
        .map_err(|e| ReadError(format!("the JSON chunk does not parse: {e}")))?;

    Ok(Loaded {
        document,
        buffers: vec![bin.map(<[u8]>::to_vec).unwrap_or_default()],
        beyond: beyond_of(text),
    })
}

fn load_gltf(path: &Path, bytes: &[u8]) -> Read<Loaded> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| ReadError("the .gltf is not UTF-8".to_string()))?;
    let document: Document = serde_json::from_str(text)
        .map_err(|e| ReadError(format!("the .gltf does not parse: {e}")))?;

    let directory = path.parent().unwrap_or(Path::new("."));
    let mut buffers = Vec::with_capacity(document.buffers.len());
    for (index, buffer) in document.buffers.iter().enumerate() {
        match buffer.uri.as_deref() {
            None => {
                return fail(format!(
                    "buffer {index} states no uri, which only a .glb may do"
                ))
            }
            // Refused: a base64 payload is a decode path this has never read.
            Some(uri) if uri.starts_with("data:") => {
                return fail(format!(
                    "buffer {index} is an embedded data uri, which this reader does not \
                     decode; export as .glb or as .gltf with a separate .bin"
                ))
            }
            Some(uri) => {
                let at = directory.join(uri);
                let held = std::fs::read(&at).map_err(|e| {
                    ReadError(format!(
                        "buffer {index} names `{uri}`, which did not read: {e}"
                    ))
                })?;
                if held.len() < buffer.byte_length {
                    return fail(format!(
                        "buffer {index} declares {} bytes and `{uri}` holds {}",
                        buffer.byte_length,
                        held.len()
                    ));
                }
                buffers.push(held);
            }
        }
    }
    Ok(Loaded {
        document,
        buffers,
        beyond: beyond_of(text),
    })
}

/// One accessor's values, as `[f32; 4]` per element with D3D padding.
fn floats(loaded: &Loaded, accessor: usize, want: usize) -> Read<Vec<[f32; 4]>> {
    let doc = &loaded.document;
    let Some(acc) = doc.accessors.get(accessor) else {
        return fail(format!("accessor {accessor} is past the end"));
    };
    let components = match acc.kind.as_str() {
        "SCALAR" => 1usize,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT4" => 16,
        other => {
            return fail(format!(
                "accessor {accessor} is a {other}, which a mesh channel is not"
            ))
        }
    };
    if components != want {
        return fail(format!(
            "accessor {accessor} is a {} where a {}-component channel was expected",
            acc.kind, want
        ));
    }
    // ⚠ A `MAT4` comes out as four `[f32; 4]` columns per element, so its
    // caller reads four entries per matrix rather than one.
    let lanes = components.min(4);
    let Some(view_index) = acc.buffer_view else {
        // ⚠ A view-less accessor is defined to read as zeroes, which for a
        // position channel is a mesh collapsed to the origin. Refusing says
        // so; reading zeroes would import it silently.
        return fail(format!(
            "accessor {accessor} names no bufferView, so every element would read as zero"
        ));
    };
    let Some(view) = doc.buffer_views.get(view_index) else {
        return fail(format!("bufferView {view_index} is past the end"));
    };
    let Some(buffer) = loaded.buffers.get(view.buffer) else {
        return fail(format!("buffer {} is past the end", view.buffer));
    };

    let (width, decode): (usize, fn(&[u8]) -> f32) = match acc.component_type {
        component::FLOAT => (4, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        component::UNSIGNED_SHORT => (2, |b| f32::from(u16::from_le_bytes([b[0], b[1]]))),
        component::UNSIGNED_INT => (4, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32),
        5121 => (1, |b| f32::from(b[0])),
        5120 => (1, |b| f32::from(b[0] as i8)),
        5122 => (2, |b| f32::from(i16::from_le_bytes([b[0], b[1]]))),
        other => {
            return fail(format!(
                "accessor {accessor} has component type {other}, which is not one glTF defines"
            ))
        }
    };
    // ⚠ A normalised integer is a fraction and a plain one is a count, and
    // the same bytes mean both. Taking the wrong one renumbers every bone or
    // washes out every colour.
    let scale = if acc.normalized {
        match acc.component_type {
            component::UNSIGNED_SHORT => 1.0 / 65535.0,
            5121 => 1.0 / 255.0,
            5122 => 1.0 / 32767.0,
            5120 => 1.0 / 127.0,
            _ => 1.0,
        }
    } else {
        1.0
    };

    let element = width * components;
    let stride = view.byte_stride.unwrap_or(element);
    if stride < element {
        return fail(format!(
            "bufferView {view_index} states a {stride}-byte stride for {element}-byte elements"
        ));
    }
    let base = view.byte_offset + acc.byte_offset;

    let mut out = Vec::with_capacity(acc.count * (components / lanes));
    for index in 0..acc.count {
        let at = base + index * stride;
        if at + element > buffer.len() {
            return fail(format!(
                "accessor {accessor} element {index} runs past its buffer"
            ));
        }
        for column in 0..components / lanes {
            let mut value = [0.0, 0.0, 0.0, if lanes == 4 { 0.0 } else { 1.0 }];
            for (lane, slot) in value.iter_mut().take(lanes).enumerate() {
                let start = at + (column * lanes + lane) * width;
                *slot = decode(&buffer[start..start + width]) * scale;
            }
            out.push(value);
        }
    }
    Ok(out)
}

/// Each node's world transform, by walking down from the scene roots.
fn world_transforms(doc: &Document) -> Vec<Mat4> {
    let mut world = vec![Mat4::IDENTITY; doc.nodes.len()];
    let mut seen = vec![false; doc.nodes.len()];
    let roots: Vec<usize> = doc
        .scenes
        .first()
        .map(|s| s.nodes.clone())
        .unwrap_or_else(|| (0..doc.nodes.len()).collect());

    let mut stack: Vec<(usize, Mat4)> = roots
        .into_iter()
        .filter(|n| *n < doc.nodes.len())
        .map(|n| (n, Mat4::IDENTITY))
        .collect();
    while let Some((index, parent)) = stack.pop() {
        // ⚠ glTF forbids a cycle in the node graph and a file can still
        // carry one, so a node already placed is not placed again.
        if seen[index] {
            continue;
        }
        seen[index] = true;
        let node = &doc.nodes[index];
        let local = Mat4::from_scale_rotation_translation(
            Vec3::from(node.scale.unwrap_or([1.0; 3])),
            node.rotation
                .map(|r| Quat::from_xyzw(r[0], r[1], r[2], r[3]))
                .filter(|q| q.length_squared() > 1e-6)
                .map(Quat::normalize)
                .unwrap_or(Quat::IDENTITY),
            Vec3::from(node.translation.unwrap_or([0.0; 3])),
        );
        let here = parent * local;
        world[index] = here;
        for child in &node.children {
            if *child < doc.nodes.len() {
                stack.push((*child, here));
            }
        }
    }
    world
}

/// How far from identity a composed transform may be and still be identity.
///
/// ⚠ A quarter turn about X composed with its own inverse is identity in
/// arithmetic and 1.2e-7 away from it in `f32`, because both go through a
/// quaternion. Left alone, re-importing a file straight out of the exporter
/// moves every vertex by that much and nothing round-trips. This is four
/// orders of magnitude below anything a mesh means — a 1e-6 radian turn moves
/// a 100-metre model by a tenth of a millimetre — so a transform inside it
/// cannot be one somebody authored.
const IDENTICAL: f32 = 1e-6;

/// A transform indistinguishable from identity, as identity.
fn settled(transform: Mat4) -> Mat4 {
    let difference = transform.to_cols_array();
    let identity = Mat4::IDENTITY.to_cols_array();
    if difference
        .iter()
        .zip(identity.iter())
        .all(|(a, b)| (a - b).abs() <= IDENTICAL)
    {
        return Mat4::IDENTITY;
    }
    transform
}

/// How far two joints' idea of the mesh space may differ.
///
/// Every `joint_world · IBM` is the same matrix in a well-formed skin. A
/// tolerance rather than equality because both sides are accumulated products
/// of `f32` transforms over a bone chain.
const ONE_SPACE: f32 = 1e-3;

/// The map from a skin's mesh space to the world, derived from the skin.
fn skin_space(loaded: &Loaded, skin_index: usize, world: &[Mat4]) -> Read<Mat4> {
    let doc = &loaded.document;
    let Some(skin) = doc.skins.get(skin_index) else {
        return fail(format!("skin {skin_index} is past the end"));
    };
    let Some(accessor) = skin.inverse_bind_matrices else {
        // With no matrices stated they are identity, so the mesh space is
        // whatever the joints' own transforms make it.
        return Ok(skin
            .joints
            .first()
            .and_then(|j| world.get(*j))
            .copied()
            .unwrap_or(Mat4::IDENTITY));
    };
    // ⚠ `MAT4`, not `VEC4`. An inverse bind matrix accessor states sixteen
    // components per element and a channel reader that asked for four read
    // the first column of each and called it a matrix.
    let values = floats(loaded, accessor, 16)?;
    if values.len() != skin.joints.len() * 4 {
        return fail(format!(
            "skin {skin_index} has {} joints and {} matrix columns",
            skin.joints.len(),
            values.len()
        ));
    }

    let mut agreed: Option<Mat4> = None;
    for (slot, joint) in skin.joints.iter().enumerate() {
        let Some(joint_world) = world.get(*joint) else {
            return fail(format!(
                "skin {skin_index} joint {slot} names node {joint}, which is past the end"
            ));
        };
        let inverse_bind = Mat4::from_cols_array(&[
            values[slot * 4][0],
            values[slot * 4][1],
            values[slot * 4][2],
            values[slot * 4][3],
            values[slot * 4 + 1][0],
            values[slot * 4 + 1][1],
            values[slot * 4 + 1][2],
            values[slot * 4 + 1][3],
            values[slot * 4 + 2][0],
            values[slot * 4 + 2][1],
            values[slot * 4 + 2][2],
            values[slot * 4 + 2][3],
            values[slot * 4 + 3][0],
            values[slot * 4 + 3][1],
            values[slot * 4 + 3][2],
            values[slot * 4 + 3][3],
        ]);
        let here = *joint_world * inverse_bind;
        match agreed {
            None => agreed = Some(here),
            Some(first) => {
                let scale = first
                    .to_cols_array()
                    .iter()
                    .fold(1.0f32, |acc, v| acc.max(v.abs()));
                let apart = first
                    .to_cols_array()
                    .iter()
                    .zip(here.to_cols_array().iter())
                    .fold(0.0f32, |acc, (a, b)| acc.max((a - b).abs()));
                if apart > ONE_SPACE * scale {
                    // ⚠ Refused rather than taking the first joint's answer.
                    // If the joints disagree there is no one mesh space, and
                    // picking one silently places the mesh by whichever bone
                    // happened to be first in the file.
                    return fail(format!(
                        "skin {skin_index}: joint {slot} and joint 0 disagree about where the \
                         mesh sits by {apart}, so the skin states no single mesh space"
                    ));
                }
            }
        }
    }
    Ok(agreed.unwrap_or(Mat4::IDENTITY))
}

/// The rotation the exporter puts on its root, inverted.
fn basis_inverse() -> Mat4 {
    Mat4::from_quat(
        Quat::from_xyzw(
            BASIS_ROTATION[0],
            BASIS_ROTATION[1],
            BASIS_ROTATION[2],
            BASIS_ROTATION[3],
        )
        .inverse(),
    )
}

fn scene_of(loaded: &Loaded) -> Read<Scene> {
    let doc = &loaded.document;
    let world = world_transforms(doc);
    let to_rage = basis_inverse();

    let mut scene = Scene {
        name: doc
            .scenes
            .first()
            .and_then(|s| s.name.clone())
            .unwrap_or_else(|| "imported".to_string()),
        ..Scene::default()
    };

    // One object, holding every mesh the document places. ⚠ The drawable a
    // mesh goes back into is chosen by the caller; splitting the import by
    // glTF node would invent an addressing the file does not state.
    let mut object = Object {
        name: scene.name.clone(),
        ..Object::default()
    };

    for (node_index, node) in doc.nodes.iter().enumerate() {
        let Some(mesh_index) = node.mesh else {
            continue;
        };
        let Some(mesh) = doc.meshes.get(mesh_index) else {
            return fail(format!(
                "node {node_index} names mesh {mesh_index}, which is past the end"
            ));
        };

        // ⚠ **A skinned primitive's space comes from its skin, not from its
        // node.** glTF ignores a skinned node's own transform, and the
        // vertices are not in world space either: at bind pose a skinned
        // vertex lands at `joint_world · IBM · v`, one constant matrix however
        // many joints there are. This exporter's is the basis and a Blender
        // file's is identity; assuming either one turns the other a quarter
        // circle about X.
        let placement = settled(match node.skin {
            Some(skin) => to_rage * skin_space(loaded, skin, &world)?,
            None => to_rage * world[node_index],
        });
        let normal_matrix = placement.inverse().transpose();
        // ⚠ A mirroring transform reverses which side of a triangle its
        // winding describes, so the indices have to be reversed with it or
        // every face comes back inside out.
        let mirrored = placement.determinant() < 0.0;

        let name = mesh
            .name
            .clone()
            .unwrap_or_else(|| format!("mesh{mesh_index}"));
        let mut out = Mesh {
            lod: lod_of(node, &name),
            name,
            primitives: Vec::with_capacity(mesh.primitives.len()),
        };
        for (slot, primitive) in mesh.primitives.iter().enumerate() {
            out.primitives.push(read_primitive(
                loaded,
                primitive,
                slot,
                &placement,
                &normal_matrix,
                mirrored,
                out.lod,
            )?);
        }
        object.meshes.push(out);
    }

    if object.meshes.is_empty() {
        return fail("the document places no mesh on any node");
    }

    for (index, skin) in doc.skins.iter().enumerate() {
        if index > 0 {
            break;
        }
        object.bones = bones_of(doc, skin);
    }

    scene.objects.push(object);
    scene.dropped = dropped_of(loaded);
    Ok(scene)
}

/// `GrcFvfChannel::Tex0` and `::Diffuse`, without a dependency on the enum
/// here: this module knows glTF and the scene model, and nothing else.
const TEX0: u8 = 6;
const DIFFUSE: u8 = 4;

fn rage_extra(primitive: &super::json::Primitive, key: &str) -> Vec<u8> {
    primitive
        .extras
        .as_ref()
        .and_then(|e| e.get("rage"))
        .and_then(|r| r.get(key))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_u64())
                .map(|v| v as u8)
                .collect()
        })
        .unwrap_or_default()
}

/// Where an export said this primitive came from, if it said.
fn rage_pair(primitive: &super::json::Primitive) -> Option<(usize, usize)> {
    let rage = primitive.extras.as_ref()?.get("rage")?;
    let model = rage.get("model")?.as_u64()? as usize;
    let geometry = rage.get("geometry")?.as_u64()? as usize;
    Some((model, geometry))
}

/// The level a node's mesh belongs to: what its extras state, else what its mesh's name says, else the first.
///
/// ⚠ A modelling tool drops the extras and keeps the names, and a mesh named `…_lod2_model0` is
/// the third level whether or not a tool forgot to say so.
pub fn lod_of(node: &super::json::Node, mesh_name: &str) -> LodSlot {
    let label = node
        .extras
        .as_ref()
        .and_then(|e| e.get("rage"))
        .and_then(|r| r.get("lod"))
        .and_then(|l| l.as_str());
    match label {
        Some(label) => lod_named(label).unwrap_or(LodSlot::Level(0)),
        None => lod_in_name(mesh_name).unwrap_or(LodSlot::Level(0)),
    }
}

fn lod_named(label: &str) -> Option<LodSlot> {
    if label == "container" {
        return Some(LodSlot::Container);
    }
    label
        .strip_prefix("lod")
        .and_then(|n| n.parse::<u8>().ok())
        .map(LodSlot::Level)
}

/// The level and model a name ends in, such as `jbib_lod1_model0` or `prop_container_model2`.
pub fn lod_and_model_in_name(name: &str) -> Option<(LodSlot, usize)> {
    let name = strip_copy_suffix(name);
    let (head, model) = name.rsplit_once("_model")?;
    let model = model.parse::<usize>().ok()?;
    let lod = head.rsplit('_').next().and_then(lod_named)?;
    Some((lod, model))
}

fn lod_in_name(name: &str) -> Option<LodSlot> {
    lod_and_model_in_name(name).map(|(lod, _)| lod)
}

/// A name without the `.001` a modelling tool adds to keep two of one name apart.
pub fn strip_copy_suffix(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((head, digits))
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => name,
    }
}

#[allow(clippy::too_many_arguments)]
fn read_primitive(
    loaded: &Loaded,
    primitive: &super::json::Primitive,
    slot: usize,
    placement: &Mat4,
    normal_matrix: &Mat4,
    mirrored: bool,
    lod: LodSlot,
) -> Read<Primitive> {
    // ⚠ Only a triangle list. Mode 4 is glTF's default and everything else
    // is a different topology that a `grmGeometry` would have to restate.
    if let Some(mode) = primitive.mode {
        if mode != 4 {
            return fail(format!(
                "primitive {slot} is mode {mode}; a drawable geometry is a triangle list"
            ));
        }
    }

    let Some(&position) = primitive.attributes.get("POSITION") else {
        return fail(format!("primitive {slot} has no POSITION"));
    };
    let positions = floats(loaded, position, 3)?;
    let count = positions.len();

    let mut out = Primitive {
        positions: positions
            .iter()
            .map(|p| {
                placement
                    .transform_point3(Vec3::new(p[0], p[1], p[2]))
                    .to_array()
            })
            .collect(),
        lod,
        material_name: primitive
            .material
            .and_then(|m| loaded.document.materials.get(m))
            .and_then(|m| m.name.clone()),
        ..Primitive::default()
    };

    if let Some(&normal) = primitive.attributes.get("NORMAL") {
        out.normals = floats(loaded, normal, 3)?
            .iter()
            .map(|n| {
                normal_matrix
                    .transform_vector3(Vec3::new(n[0], n[1], n[2]))
                    .normalize_or_zero()
                    .to_array()
            })
            .collect();
    }
    if let Some(&tangent) = primitive.attributes.get("TANGENT") {
        out.tangents = floats(loaded, tangent, 4)?
            .iter()
            .map(|t| {
                let v = placement
                    .transform_vector3(Vec3::new(t[0], t[1], t[2]))
                    .normalize_or_zero();
                // The handedness flips with the winding.
                let sign = if mirrored { -t[3] } else { t[3] };
                [v.x, v.y, v.z, if sign < 0.0 { -1.0 } else { 1.0 }]
            })
            .collect();
    }

    // ⚠ `uv_channels` means a `GrcFvfChannel` discriminant on both sides of
    // this crate, never glTF's `TEXCOORD_n` ordinal. A drawable may skip
    // `Tex0`, so the two numberings are not the same one, and `extras` is
    // where an export that knew the difference recorded it.
    let stated = rage_extra(primitive, "uvChannels");
    for ordinal in 0..8usize {
        let Some(&accessor) = primitive.attributes.get(&format!("TEXCOORD_{ordinal}")) else {
            break;
        };
        out.uv_channels
            .push(stated.get(ordinal).copied().unwrap_or(TEX0 + ordinal as u8));
        out.uvs.push(
            floats(loaded, accessor, 2)?
                .iter()
                .map(|uv| [uv[0], uv[1]])
                .collect(),
        );
    }
    let stated = rage_extra(primitive, "colourChannels");
    for ordinal in 0..2usize {
        let Some(&accessor) = primitive.attributes.get(&format!("COLOR_{ordinal}")) else {
            break;
        };
        out.colour_channels.push(
            stated
                .get(ordinal)
                .copied()
                .unwrap_or(DIFFUSE + ordinal as u8),
        );
        out.colours.push(floats(loaded, accessor, 4)?);
    }

    out.stated_address = rage_pair(primitive);

    if let Some(&joints) = primitive.attributes.get("JOINTS_0") {
        let Some(&weights) = primitive.attributes.get("WEIGHTS_0") else {
            return fail(format!("primitive {slot} states JOINTS_0 and no WEIGHTS_0"));
        };
        out.joints = floats(loaded, joints, 4)?
            .iter()
            .map(|j| [j[0] as u16, j[1] as u16, j[2] as u16, j[3] as u16])
            .collect();
        out.weights = floats(loaded, weights, 4)?;
    }

    let Some(indices) = primitive.indices else {
        // An index-less primitive is a defined glTF shape and a `grmGeometry`
        // always states indices; refused rather than synthesised.
        return fail(format!(
            "primitive {slot} states no indices, which a drawable geometry always does"
        ));
    };
    let read = floats(loaded, indices, 1)?;
    if read.len() % 3 != 0 {
        return fail(format!(
            "primitive {slot} holds {} indices, which is not a whole number of triangles",
            read.len()
        ));
    }
    out.indices.reserve(read.len());
    for triangle in read.as_chunks::<3>().0 {
        let corners = [
            triangle[0][0] as u32,
            triangle[1][0] as u32,
            triangle[2][0] as u32,
        ];
        if corners.iter().any(|c| *c as usize >= count) {
            return fail(format!(
                "primitive {slot} indexes vertex {} of {count}",
                corners.iter().max().copied().unwrap_or(0)
            ));
        }
        if mirrored {
            out.indices.extend([corners[0], corners[2], corners[1]]);
        } else {
            out.indices.extend(corners);
        }
    }

    for (name, values) in [
        ("NORMAL", out.normals.len()),
        ("TANGENT", out.tangents.len()),
        ("JOINTS_0", out.joints.len()),
        ("WEIGHTS_0", out.weights.len()),
    ] {
        if values != 0 && values != count {
            return fail(format!(
                "primitive {slot}: {name} holds {values} elements and POSITION holds {count}"
            ));
        }
    }
    for (index, uv) in out.uvs.iter().enumerate() {
        if uv.len() != count {
            return fail(format!(
                "primitive {slot}: TEXCOORD_{index} holds {} elements and POSITION holds {count}",
                uv.len()
            ));
        }
    }

    Ok(out)
}

fn bones_of(doc: &Document, skin: &super::json::Skin) -> Vec<Bone> {
    let mut slot_of: BTreeMap<usize, usize> = BTreeMap::new();
    for (slot, node) in skin.joints.iter().enumerate() {
        slot_of.insert(*node, slot);
    }
    let mut parent_of: BTreeMap<usize, usize> = BTreeMap::new();
    for (index, node) in doc.nodes.iter().enumerate() {
        for child in &node.children {
            parent_of.insert(*child, index);
        }
    }

    skin.joints
        .iter()
        .enumerate()
        .filter_map(|(slot, node_index)| {
            let node = doc.nodes.get(*node_index)?;
            Some(Bone {
                name: node.name.clone().unwrap_or_else(|| format!("bone{slot}")),
                tag: node
                    .extras
                    .as_ref()
                    .and_then(|e| e.get("rage"))
                    .and_then(|r| r.get("boneTag"))
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0) as u16,
                parent: parent_of
                    .get(node_index)
                    .and_then(|p| slot_of.get(p).copied()),
                translation: node.translation.unwrap_or([0.0; 3]),
                rotation: node.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]),
                scale: node.scale.unwrap_or([1.0; 3]),
            })
        })
        .collect()
}
