//! A `rmcDrawable` of either target read into the interchange model.
//!
//! ⚠ Each target is read as itself, through [`Extracted`]: routing an
//! Enhanced drawable through `gen9::convert` would report a Gen8 fact about
//! an Enhanced file.

use std::collections::BTreeMap;

use or_io::codec::mesh::{GrcDataSize, GrcFvfChannel, VertexReader};
use or_io::codec::texture::{decode_level, encode_png};
use or_io::decl::{Gen8, Gen9, Shared};
use or_io::gta5::drawable_codecs::gen8::EntryData;
use or_io::gta5::{
    gen8, gen9, CrSkeletonData, DrawableTarget, GrmGeometry, GrmModel, GrmShaderGroup, RmcDrawable,
    RmcLod,
};

use crate::scene::{
    AlphaMode, Bone, ExtractReport, Image, LodSlot, Material, Mesh, Object, Primitive, Scene,
    TextureBinding,
};

/// One material parameter as an extraction reads it.
pub enum Parameter<'a> {
    /// The name of the texture it binds; empty where unbound.
    Texture(&'a str),
    /// Its constant, one row of four per register.
    Vectors(Vec<[f32; 4]>),
}

/// What an extraction reads of a target's materials and textures.
pub trait Extracted: DrawableTarget + Sized {
    /// The hash of the effect a material draws with.
    fn effect_hash(shader: &Self::InstanceData) -> u32;
    fn draw_bucket(shader: &Self::InstanceData) -> u8;
    /// Each texture and constant parameter, by its name hash, in the material's order.
    fn parameters<'a>(
        shader: &'a Self::InstanceData,
        group: &'a GrmShaderGroup<Self>,
    ) -> Vec<(u32, Parameter<'a>)>;
    fn texture_name(texture: &Self::Texture) -> &str;
    /// The texture's first level as a PNG; `None` where it does not decode.
    fn png(texture: &Self::Texture) -> Option<Vec<u8>>;
}

impl Extracted for Gen8 {
    fn effect_hash(shader: &gen8::GrcInstanceData) -> u32 {
        shader.basis_hash.get()
    }

    fn draw_bucket(shader: &gen8::GrcInstanceData) -> u8 {
        shader.draw_bucket
    }

    fn parameters<'a>(
        shader: &'a gen8::GrcInstanceData,
        group: &'a GrmShaderGroup<Gen8>,
    ) -> Vec<(u32, Parameter<'a>)> {
        (shader.entries.iter())
            .map(|entry| {
                let parameter = match &entry.data {
                    EntryData::Texture(slot) => {
                        Parameter::Texture(group.slot_name(slot).unwrap_or(""))
                    }
                    EntryData::Float(rows) => {
                        Parameter::Vectors(rows.iter().map(|v| [v.x, v.y, v.z, v.w]).collect())
                    }
                };
                (entry.name_hash.get(), parameter)
            })
            .collect()
    }

    fn texture_name(texture: &gen8::GrcTexture) -> &str {
        &texture.name
    }

    fn png(texture: &gen8::GrcTexture) -> Option<Vec<u8>> {
        decode_level(texture, 0)
            .and_then(|rgba| encode_png(&rgba))
            .ok()
    }
}

impl Extracted for Gen9 {
    fn effect_hash(shader: &gen9::GrcInstanceDataG9) -> u32 {
        shader.name.get()
    }

    fn draw_bucket(shader: &gen9::GrcInstanceDataG9) -> u8 {
        shader.draw_bucket
    }

    /// Samplers and the kind-1 parameters are left out: neither is a value glTF carries.
    fn parameters<'a>(
        shader: &'a gen9::GrcInstanceDataG9,
        group: &'a GrmShaderGroup<Gen9>,
    ) -> Vec<(u32, Parameter<'a>)> {
        (shader.param_infos.params.iter())
            .filter_map(|info| {
                let parameter = if let Some(slot) = info.texture() {
                    let slot = shader.params.textures.get(slot)?;
                    Parameter::Texture(group.slot_name(slot).unwrap_or(""))
                } else {
                    let (buffer, range) = info.constant()?;
                    let bytes = shader.params.buffers.get(buffer)?.get(range)?;
                    let floats: Vec<f32> = (bytes.as_chunks::<4>().0.iter())
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect();
                    Parameter::Vectors(
                        (floats.chunks(4))
                            .map(|r| std::array::from_fn(|i| r.get(i).copied().unwrap_or(0.0)))
                            .collect(),
                    )
                };
                Some((info.name.get(), parameter))
            })
            .collect()
    }

    fn texture_name(texture: &gen9::GrcTextureG9) -> &str {
        &texture.name
    }

    /// ⚠ Only a payload stored row by row decodes: every other tile mode swizzles it.
    fn png(texture: &gen9::GrcTextureG9) -> Option<Vec<u8>> {
        if !matches!(
            texture.tile_mode.get(),
            gen9::TileMode::Auto | gen9::TileMode::Linear
        ) {
            return None;
        }
        decode_level(texture, 0)
            .and_then(|rgba| encode_png(&rgba))
            .ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LodSelection {
    #[default]
    Highest,
    All,
}

#[derive(Debug, Clone)]
pub struct ExtractOptions {
    pub lods: LodSelection,
    /// Decode the drawable's own texture dictionary to PNG.
    pub textures: bool,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            lods: LodSelection::default(),
            textures: true,
        }
    }
}

/// Which glTF PBR slot, if any, a RAGE sampler parameter fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextureRole {
    BaseColour,
    Normal,
    Other,
}

/// The sampler names this maps into glTF's two slots.
///
/// ⚠ **Only the two glTF states in the same terms.** A RAGE specular map
/// packs intensity, falloff and fresnel into three channels and a
/// metallic-roughness map does not, so binding one to the other renders a
/// plausible lie. Every other sampler is carried by name in `extras`.
const BASE_COLOUR_SAMPLERS: &[&str] = &[
    "DiffuseSampler",
    "DiffuseTex",
    "TextureSampler",
    "TextureSamplerDiffPal",
    "DiffuseSamplerP",
];

const NORMAL_SAMPLERS: &[&str] = &["BumpSampler", "NormalSampler", "BumpSamplerP"];

/// Sampler names worth spelling in `extras` when nothing else names them.
const KNOWN_SAMPLERS: &[&str] = &[
    "SpecSampler",
    "SpecularSampler",
    "DetailSampler",
    "TintPaletteSampler",
    "EnvironmentSampler",
    "DirtSampler",
    "SnowSampler",
    "HeightSampler",
    "distanceMapSampler",
    "FlowSampler",
    "FogSampler",
    "DamageSampler",
    "StippleTexture",
    "DiffuseSampler2",
    "DiffuseSampler3",
    "BumpSampler2",
    "BumpSampler3",
    "plateBgSampler",
    "plateBgBumpSampler",
];

fn role_of(name: &str) -> TextureRole {
    if BASE_COLOUR_SAMPLERS
        .iter()
        .any(|n| n.eq_ignore_ascii_case(name))
    {
        TextureRole::BaseColour
    } else if NORMAL_SAMPLERS.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        TextureRole::Normal
    } else {
        TextureRole::Other
    }
}

/// The parameter name behind a hash, from the shader database when one is
/// staged and from the sampler table otherwise.
pub fn parameter_name(effect_hash: u32, name_hash: u32) -> String {
    if let Some(def) = or_io::gta5::shaders::shader_defs::describe(effect_hash, name_hash) {
        return def.name;
    }
    for candidate in BASE_COLOUR_SAMPLERS
        .iter()
        .chain(NORMAL_SAMPLERS)
        .chain(KNOWN_SAMPLERS)
    {
        if or_io::hash::string_hash_str(candidate) == name_hash {
            return (*candidate).to_string();
        }
    }
    format!("0x{name_hash:08X}")
}

/// The draw buckets a material blends in: alpha, decal and displacement alpha.
const BLENDED_BUCKETS: [u8; 3] = [1, 2, 7];

/// The draw bucket a material cuts out in.
const CUTOUT_BUCKET: u8 = 3;

fn alpha_mode_of(draw_bucket: u8) -> AlphaMode {
    if BLENDED_BUCKETS.contains(&draw_bucket) {
        AlphaMode::Blend
    } else if draw_bucket == CUTOUT_BUCKET {
        AlphaMode::Mask
    } else {
        AlphaMode::Opaque
    }
}

/// The effect's name as the shader table knows it, else its hash.
pub fn effect_name(hash: u32) -> String {
    match or_io::gta5::shaders::shader_names::lookup(hash) {
        Some(name) => name.to_string(),
        None => format!("shader_{hash:08X}"),
    }
}

/// Builds one interchange scene out of one or more drawables.
#[derive(Debug, Default)]
pub struct SceneBuilder {
    scene: Scene,
    image_by_name: BTreeMap<String, usize>,
    reference: Vec<Bone>,
}

impl SceneBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            scene: Scene {
                name: name.into(),
                ..Scene::default()
            },
            image_by_name: BTreeMap::new(),
            reference: Vec::new(),
        }
    }

    /// The skeleton a skinned drawable that carries none of its own is read against.
    ///
    /// ⚠ A ped component skins against the ped's skeleton, which lives in another file, so its
    /// weights reach a glTF only beside that skeleton. A drawable with a skeleton of its own
    /// keeps it, and an unskinned one takes none.
    pub fn with_reference_skeleton(mut self, bones: Vec<Bone>) -> Self {
        self.reference = bones;
        self
    }

    pub fn push<T: Extracted>(
        &mut self,
        name: &str,
        drawable: &RmcDrawable<T>,
        options: &ExtractOptions,
    ) {
        let material_base = self.scene.materials.len();
        self.push_materials(drawable, options);
        self.push_object(name, drawable, material_base, options);
    }

    /// How many materials the scene holds: where the materials of the next drawable pushed begin.
    pub fn material_count(&self) -> usize {
        self.scene.materials.len()
    }

    /// A drawable that carries no shader group of its own, as a fragment's extra drawable does not: its shader indices name the materials from `base` on, which an earlier drawable of the same file pushed.
    pub fn push_sharing<T: Extracted>(
        &mut self,
        name: &str,
        drawable: &RmcDrawable<T>,
        base: usize,
        options: &ExtractOptions,
    ) {
        self.push_object(name, drawable, base, options);
    }

    fn push_object<T: Extracted>(
        &mut self,
        name: &str,
        drawable: &RmcDrawable<T>,
        material_base: usize,
        options: &ExtractOptions,
    ) {
        let mut object = Object {
            name: name.to_string(),
            bones: drawable
                .skeleton
                .as_deref()
                .map(bones_of)
                .unwrap_or_default(),
            meshes: Vec::new(),
        };

        for (slot, lod) in lods_of(drawable, options.lods) {
            for (index, model) in lod.models.iter().enumerate() {
                let mesh = self.read_model(model, slot, index, material_base);
                if !mesh.primitives.is_empty() {
                    object.meshes.push(mesh);
                }
            }
        }

        if object.bones.is_empty() && object.meshes.iter().any(Mesh::is_skinned) {
            if self.reference.is_empty() {
                self.scene.report.skinned_without_a_skeleton += 1;
            } else {
                object.bones = self.reference.clone();
            }
        }
        self.scene.objects.push(object);
    }

    pub fn finish(self) -> Scene {
        self.scene
    }

    /// The materials of a drawable's shader group, with no object: those a drawable that carries none of its own indexes from the count before.
    pub fn push_materials<T: Extracted>(
        &mut self,
        drawable: &RmcDrawable<T>,
        options: &ExtractOptions,
    ) {
        let Some(group) = drawable.shader_group.as_deref() else {
            return;
        };

        if options.textures {
            let dictionary = group.texture_dictionary.as_deref();
            for texture in dictionary.iter().flat_map(|d| d.entries.iter().flatten()) {
                self.push_image::<T>(texture);
            }
        }

        for shader in &group.shaders {
            let material = self.read_shader::<T>(shader, group);
            self.scene.materials.push(material);
        }
    }

    fn push_image<T: Extracted>(&mut self, texture: &T::Texture) {
        let name = T::texture_name(texture);
        if name.is_empty() || self.image_by_name.contains_key(name) {
            return;
        }
        match T::png(texture) {
            Some(png) => {
                self.image_by_name
                    .insert(name.to_string(), self.scene.images.len());
                self.scene.images.push(Image {
                    name: name.to_string(),
                    png,
                });
            }
            None => self
                .scene
                .report
                .textures_not_decoded
                .push(name.to_string()),
        }
    }

    fn read_shader<T: Extracted>(
        &mut self,
        shader: &T::InstanceData,
        group: &GrmShaderGroup<T>,
    ) -> Material {
        let effect_hash = T::effect_hash(shader);
        let draw_bucket = T::draw_bucket(shader);
        let name = effect_name(effect_hash);
        let mut material = Material {
            name: name.clone(),
            shader: name,
            draw_bucket,
            alpha_mode: alpha_mode_of(draw_bucket),
            ..Material::default()
        };

        for (name_hash, value) in T::parameters(shader, group) {
            let parameter = parameter_name(effect_hash, name_hash);
            match value {
                Parameter::Texture(name) => {
                    if name.is_empty() {
                        continue;
                    }
                    let image = self.image_by_name.get(name).copied();
                    if image.is_none() && group.texture_dictionary.is_some() {
                        let missing = &mut self.scene.report.textures_not_carried;
                        if !missing.iter().any(|n| n == name) {
                            missing.push(name.to_string());
                        }
                    }
                    match role_of(&parameter) {
                        TextureRole::BaseColour if material.base_colour.is_none() => {
                            material.base_colour = image
                        }
                        TextureRole::Normal if material.normal.is_none() => material.normal = image,
                        _ => {}
                    }
                    material.textures.push(TextureBinding {
                        parameter,
                        texture: name.to_string(),
                        image,
                    });
                }
                Parameter::Vectors(values) => material.vectors.push((parameter, values)),
            }
        }

        material
    }

    fn read_model<T: DrawableTarget>(
        &mut self,
        model: &GrmModel<T>,
        lod: LodSlot,
        index: usize,
        material_base: usize,
    ) -> Mesh {
        let mut mesh = Mesh {
            name: format!("{}_model{index}", lod.label()),
            lod,
            primitives: Vec::new(),
        };

        for (slot, geometry) in model.geometries.iter().enumerate() {
            self.scene.report.geometries += 1;
            let Some(mut primitive) = self.read_geometry(geometry) else {
                continue;
            };
            primitive.lod = lod;
            primitive.model = index;
            primitive.geometry = slot;
            primitive.material = match model.shader_indices.get(slot) {
                Some(&shader) => {
                    let at = material_base + shader as usize;
                    if at < self.scene.materials.len() {
                        Some(at)
                    } else {
                        self.scene.report.shader_indices_out_of_range += 1;
                        None
                    }
                }
                None => None,
            };
            mesh.primitives.push(primitive);
        }

        mesh
    }

    fn read_geometry<T: DrawableTarget>(&mut self, geometry: &GrmGeometry<T>) -> Option<Primitive> {
        let reader = VertexReader::new(&*geometry.vertex_buffer)?;
        let layout = *reader.layout();
        if !layout.has(GrcFvfChannel::Position) {
            self.scene.report.geometries_without_positions += 1;
            return None;
        }
        let count = reader.count();
        if count == 0 {
            return None;
        }

        let mut primitive = Primitive {
            positions: (0..count)
                .map(|v| {
                    let p = reader.get(GrcFvfChannel::Position, v).unwrap_or_default();
                    [p[0], p[1], p[2]]
                })
                .collect(),
            ..Primitive::default()
        };

        // ⚠ Normalised on the way out, and it is a requirement rather than
        // tidiness: glTF states that `NORMAL` and `TANGENT` are unit vectors
        // and a validator rejects a file whose are not. A `PackedNormal`
        // channel quantises each axis to ten bits, so almost none of them
        // arrive at exactly one.
        if layout.has(GrcFvfChannel::Normal) {
            primitive.normals = (0..count)
                .map(|v| {
                    let n = reader.get(GrcFvfChannel::Normal, v).unwrap_or_default();
                    match unit([n[0], n[1], n[2]]) {
                        Some(unit) => unit,
                        None => {
                            self.scene.report.normals_substituted += 1;
                            [0.0, 0.0, 1.0]
                        }
                    }
                })
                .collect();
        }

        // ⚠ glTF requires the tangent's fourth component to be exactly ±1 and
        // requires a normal beside it. RAGE writes a bitangent sign there and
        // a `PackedNormal` tangent has no fourth component at all, so it
        // decodes to 1.0 — which is the same convention, not a coincidence.
        if layout.has(GrcFvfChannel::Tangent0) && !primitive.normals.is_empty() {
            primitive.tangents = (0..count)
                .map(|v| {
                    let t = reader.get(GrcFvfChannel::Tangent0, v).unwrap_or_default();
                    let sign = if t[3] < 0.0 { -1.0 } else { 1.0 };
                    match unit([t[0], t[1], t[2]]) {
                        Some(u) => [u[0], u[1], u[2], sign],
                        None => {
                            self.scene.report.tangents_substituted += 1;
                            [1.0, 0.0, 0.0, sign]
                        }
                    }
                })
                .collect();
        }

        for channel in TEX_CHANNELS {
            if !layout.has(channel) {
                continue;
            }
            primitive.uv_channels.push(channel as u8);
            primitive.uvs.push(
                (0..count)
                    .map(|v| {
                        let uv = reader.get(channel, v).unwrap_or_default();
                        [uv[0], uv[1]]
                    })
                    .collect(),
            );
        }

        for channel in [GrcFvfChannel::Diffuse, GrcFvfChannel::Specular] {
            if !layout.has(channel) {
                continue;
            }
            primitive.colour_channels.push(channel as u8);
            primitive.colours.push(
                (0..count)
                    .map(|v| reader.get(channel, v).unwrap_or([0.0; 4]))
                    .collect(),
            );
        }

        self.read_skinning(geometry, &reader, count, &mut primitive);
        self.read_indices(geometry, count, &mut primitive);

        if primitive.indices.is_empty() {
            return None;
        }
        Some(primitive)
    }

    fn read_skinning<T: DrawableTarget>(
        &mut self,
        geometry: &GrmGeometry<T>,
        reader: &VertexReader<'_>,
        count: usize,
        primitive: &mut Primitive,
    ) {
        let layout = reader.layout();
        if geometry.matrix_palette.is_empty()
            || !layout.has(GrcFvfChannel::Weight)
            || !layout.has(GrcFvfChannel::Binding)
        {
            return;
        }
        let palette = &geometry.matrix_palette;

        primitive.joints.reserve(count);
        primitive.weights.reserve(count);
        for vertex in 0..count {
            let raw_weights = channel_lanes(reader, GrcFvfChannel::Weight, vertex);
            let raw_slots = channel_lanes(reader, GrcFvfChannel::Binding, vertex);

            let mut joints = [0u16; 4];
            let mut weights = [0.0f32; 4];
            let mut lane_out = 0;
            let mut total = 0.0f32;
            for lane in 0..4 {
                if raw_weights[lane] == 0 {
                    continue;
                }
                let Some(&bone) = palette.get(raw_slots[lane] as usize) else {
                    self.scene.report.influences_outside_the_palette += 1;
                    continue;
                };
                joints[lane_out] = bone;
                weights[lane_out] = f32::from(raw_weights[lane]);
                total += f32::from(raw_weights[lane]);
                lane_out += 1;
            }

            // ⚠ A lane with no weight must name joint 0, and the four must sum
            // to one: a glTF reader is entitled to treat either as an error,
            // and RAGE states the weights as eight-bit fractions that do not.
            if lane_out == 0 {
                if let Some(&bone) = palette.get(raw_slots[0] as usize) {
                    joints[0] = bone;
                    weights[0] = 1.0;
                } else {
                    weights[0] = 1.0;
                }
            } else {
                let scale = 1.0 / total;
                for weight in weights.iter_mut().take(lane_out) {
                    *weight *= scale;
                }
            }

            primitive.joints.push(joints);
            primitive.weights.push(weights);
        }
    }

    fn read_indices<T: DrawableTarget>(
        &mut self,
        geometry: &GrmGeometry<T>,
        vertices: usize,
        primitive: &mut Primitive,
    ) {
        let indices = T::indices(&geometry.index_buffer);
        if !is_triangle_list(geometry) {
            self.scene.report.geometries_not_triangles += 1;
            return;
        }
        if !indices.len().is_multiple_of(3) {
            self.scene.report.geometries_with_partial_triangles += 1;
        }

        primitive.indices.reserve(indices.len());
        for triangle in indices.as_chunks::<3>().0 {
            if triangle.iter().any(|&i| usize::from(i) >= vertices) {
                self.scene.report.indices_out_of_range += 1;
                continue;
            }
            primitive
                .indices
                .extend(triangle.iter().map(|&i| u32::from(i)));
        }
    }
}

/// Whether a geometry's index run groups into triangles, by its own counts.
///
/// ⚠ Not `primitive_type`: `GrmPrimitiveType` was once numbered one out and
/// read every shipped geometry as `LineStrip`, and a reader that trusted it
/// exported nothing. `openrage-cli mesh census --dir` over the staged corpus:
/// 1,590 of 1,590 geometries state 3 and have an index run exactly
/// `primitive_count * 3` long.
pub fn is_triangle_list<T: DrawableTarget>(geometry: &GrmGeometry<T>) -> bool {
    let indices = T::indices(&geometry.index_buffer).len();
    let primitives = geometry.primitive_count() as usize;
    if primitives > 0 {
        return indices == primitives * 3;
    }
    indices.is_multiple_of(3)
}

/// A unit vector, or `None` when there is no direction to recover.
fn unit(value: [f32; 3]) -> Option<[f32; 3]> {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if !length.is_finite() || length < 1e-9 {
        return None;
    }
    Some([value[0] / length, value[1] / length, value[2] / length])
}

const TEX_CHANNELS: [GrcFvfChannel; 8] = [
    GrcFvfChannel::Tex0,
    GrcFvfChannel::Tex1,
    GrcFvfChannel::Tex2,
    GrcFvfChannel::Tex3,
    GrcFvfChannel::Tex4,
    GrcFvfChannel::Tex5,
    GrcFvfChannel::Tex6,
    GrcFvfChannel::Tex7,
];

/// The four bytes of a skinning channel, whatever the channel says it holds.
///
/// ⚠ `Weight` and `Binding` are four lanes of payload whatever width they are
/// spelled at: a bone index read through `Color`'s normalising decoder comes
/// back as a fraction.
pub fn channel_lanes(reader: &VertexReader<'_>, channel: GrcFvfChannel, vertex: usize) -> [u8; 4] {
    let Some(slot) = reader.layout().slot(channel) else {
        return [0; 4];
    };
    let Some(bytes) = reader.raw(channel, vertex) else {
        return [0; 4];
    };
    let mut out = [0u8; 4];
    match slot.size {
        GrcDataSize::UByte4 | GrcDataSize::Color => {
            for (lane, value) in out.iter_mut().zip(bytes) {
                *lane = *value;
            }
        }
        GrcDataSize::Short4 => {
            for (lane, pair) in out.iter_mut().zip(bytes.as_chunks::<2>().0.iter()) {
                *lane = (u16::from_le_bytes([pair[0], pair[1]]) >> 8) as u8;
            }
        }
        _ => {}
    }
    out
}

/// ⚠ A null level reads as an empty one, and no corpus level is empty and
/// non-null (OD-TWIN-LODS), so an empty level is skipped as a null one.
pub(crate) fn lods_of<T: DrawableTarget>(
    drawable: &RmcDrawable<T>,
    selection: LodSelection,
) -> Vec<(LodSlot, &RmcLod<T>)> {
    let mut out = Vec::new();
    for (level, lod) in drawable.lod_group.lods.iter().enumerate() {
        if lod.models.is_empty() {
            continue;
        }
        out.push((LodSlot::Level(level as u8), &**lod));
        if selection == LodSelection::Highest {
            return out;
        }
    }
    // ⚠ A container that shares the first level is that level, not a second
    // mesh, so only one the file holds apart is walked.
    if selection == LodSelection::All {
        if let Shared::Own(Some(container)) = &drawable.container {
            out.push((LodSlot::Container, &**container));
        }
    }
    out
}

/// A skeleton's bones in the order a skinned drawable's palette indexes them.
pub fn bones_of(skeleton: &CrSkeletonData) -> Vec<Bone> {
    skeleton
        .bones
        .iter()
        .map(|bone| Bone {
            name: if bone.name.is_empty() {
                format!("bone_{}", bone.bone_id)
            } else {
                bone.name.clone()
            },
            tag: bone.bone_id,
            parent: usize::try_from(bone.parent_index).ok(),
            translation: [bone.translation.x, bone.translation.y, bone.translation.z],
            rotation: [
                bone.rotation.x,
                bone.rotation.y,
                bone.rotation.z,
                bone.rotation.w,
            ],
            scale: [bone.scale.x, bone.scale.y, bone.scale.z],
        })
        .collect()
}

/// One drawable, as a scene of one object.
pub fn drawable_scene<T: Extracted>(
    name: &str,
    drawable: &RmcDrawable<T>,
    options: &ExtractOptions,
) -> Scene {
    let mut builder = SceneBuilder::new(name);
    builder.push(name, drawable, options);
    builder.finish()
}

/// What an extraction reported, as lines a command can print.
pub fn report_lines(report: &ExtractReport) -> Vec<String> {
    let mut out = Vec::new();
    let mut note = |condition: bool, line: String| {
        if condition {
            out.push(line);
        }
    };
    note(
        report.geometries_without_positions > 0,
        format!(
            "{} geometries declare no position channel",
            report.geometries_without_positions
        ),
    );
    note(
        report.geometries_with_partial_triangles > 0,
        format!(
            "{} geometries end mid-triangle",
            report.geometries_with_partial_triangles
        ),
    );
    note(
        report.indices_out_of_range > 0,
        format!(
            "{} triangles index past the vertex run",
            report.indices_out_of_range
        ),
    );
    note(
        report.influences_outside_the_palette > 0,
        format!(
            "{} influences name a palette slot that is not there",
            report.influences_outside_the_palette
        ),
    );
    note(
        report.shader_indices_out_of_range > 0,
        format!(
            "{} geometries name a material the shader group does not have",
            report.shader_indices_out_of_range
        ),
    );
    note(
        report.normals_substituted + report.tangents_substituted > 0,
        format!(
            "{} normals and {} tangents had no direction and were replaced",
            report.normals_substituted, report.tangents_substituted
        ),
    );
    note(
        report.skinned_without_a_skeleton > 0,
        format!(
            "{} drawables skin against a skeleton they do not carry, so glTF gets \
             the mesh without its weights",
            report.skinned_without_a_skeleton
        ),
    );
    note(
        !report.textures_not_carried.is_empty(),
        format!(
            "{} textures live in another dictionary: {}",
            report.textures_not_carried.len(),
            report.textures_not_carried.join(", ")
        ),
    );
    note(
        !report.textures_not_decoded.is_empty(),
        format!(
            "{} textures would not decode: {}",
            report.textures_not_decoded.len(),
            report.textures_not_decoded.join(", ")
        ),
    );
    out
}
