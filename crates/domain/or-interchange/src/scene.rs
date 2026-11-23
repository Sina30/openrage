//! A drawable's mesh, materials and skeleton, in terms an external tool has.
//!
//! ⚠ The vertices stay in the drawable's own Z-up space and the axes are
//! never rewritten here; the writer carries the basis on one root node
//! ([`crate::gltf::BASIS_ROTATION`] says why).

/// Which LOD of the drawable a mesh was read out of.
///
/// ⚠ Ordered so that an import can group by it. `Container` sorts after
/// every level, which is the order the drawable walks them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LodSlot {
    Level(u8),
    /// `RmcDrawable::container`, the extra LOD outside the group's four.
    Container,
}

impl LodSlot {
    pub fn label(self) -> String {
        match self {
            Self::Level(n) => format!("lod{n}"),
            Self::Container => "container".to_string(),
        }
    }

    /// Whether a viewer that shows one LOD should show this one.
    pub fn is_default_visible(self) -> bool {
        matches!(self, Self::Level(0))
    }
}

/// How a material's coverage is meant to be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaMode {
    #[default]
    Opaque,
    Mask,
    Blend,
}

/// One material's binding of a RAGE shader parameter to a texture.
#[derive(Debug, Clone)]
pub struct TextureBinding {
    /// The RAGE parameter name, or `0x…` when nothing in the build names it.
    pub parameter: String,
    pub texture: String,
    /// `None` when no dictionary travelling with the drawable holds it.
    pub image: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct Material {
    pub name: String,
    pub shader: String,
    pub draw_bucket: u8,
    pub alpha_mode: AlphaMode,
    /// The diffuse binding, as an index into [`Scene::images`].
    pub base_colour: Option<usize>,
    pub normal: Option<usize>,
    /// Every texture binding, including the two named above.
    pub textures: Vec<TextureBinding>,
    pub vectors: Vec<(String, Vec<[f32; 4]>)>,
}

/// A decoded texture, ready to be written beside or inside an interchange file.
#[derive(Debug, Clone)]
pub struct Image {
    pub name: String,
    pub png: Vec<u8>,
}

/// One `grmGeometry`: a vertex run, an index run and the material they draw with.
#[derive(Debug, Clone, Default)]
pub struct Primitive {
    pub material: Option<usize>,
    /// The name of the material a file gave this primitive, which an import pairs a geometry by.
    pub material_name: Option<String>,
    pub positions: Vec<[f32; 3]>,
    /// Empty when the geometry declares no normal channel.
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    /// `Tex0` upward, in channel order, with no gaps.
    pub uvs: Vec<Vec<[f32; 2]>>,
    /// Which `GrcFvfChannel` each of `uvs` came from. ⚠ A drawable may skip
    /// `Tex0`, and glTF's `TEXCOORD_n` must start at nought and be
    /// contiguous, so the two numbering schemes are not the same one.
    pub uv_channels: Vec<u8>,
    pub colours: Vec<Vec<[f32; 4]>>,
    pub colour_channels: Vec<u8>,
    /// Skeleton bone indices, already resolved through the matrix palette.
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub lod: LodSlot,
    /// Where this came from in the drawable, so a later import can find it again.
    pub model: usize,
    pub geometry: usize,
    /// The `(model, geometry)` an interchange file *stated*, as against the
    /// position it arrived at.
    ///
    /// ⚠ `None` and `Some((0, 0))` are different answers: the first is a
    /// file that never said, and the second is one that said the first
    /// geometry. An import pairs positionally and uses this to refuse a file
    /// whose meshes were reordered after it left.
    pub stated_address: Option<(usize, usize)>,
}

impl Default for LodSlot {
    fn default() -> Self {
        Self::Level(0)
    }
}

impl Primitive {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_skinned(&self) -> bool {
        !self.joints.is_empty()
    }
}

/// One `grmModel`.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub name: String,
    pub lod: LodSlot,
    pub primitives: Vec<Primitive>,
}

impl Mesh {
    pub fn is_skinned(&self) -> bool {
        self.primitives.iter().any(Primitive::is_skinned)
    }
}

/// A bone, with its transform in its parent's frame.
#[derive(Debug, Clone, Default)]
pub struct Bone {
    pub name: String,
    pub tag: u16,
    pub parent: Option<usize>,
    pub translation: [f32; 3],
    /// xyzw, matching both `crBoneData` and glTF.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

/// One drawable: its meshes and the skeleton they are bound to.
#[derive(Debug, Clone, Default)]
pub struct Object {
    pub name: String,
    pub meshes: Vec<Mesh>,
    /// Empty when the drawable ships no skeleton.
    pub bones: Vec<Bone>,
}

impl Object {
    pub fn is_skinned(&self) -> bool {
        !self.bones.is_empty() && self.meshes.iter().any(Mesh::is_skinned)
    }
}

/// What the extraction could not carry, counted rather than logged and forgotten.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractReport {
    pub geometries: usize,
    /// A geometry whose vertex buffer declares no position channel.
    pub geometries_without_positions: usize,
    /// A geometry whose index run is not a whole number of triangles.
    pub geometries_with_partial_triangles: usize,
    /// An index past the end of the vertex run, dropped with its triangle.
    pub indices_out_of_range: usize,
    /// A `Binding` lane naming a palette slot the geometry does not have.
    pub influences_outside_the_palette: usize,
    /// A geometry whose primitive type is not a triangle list.
    pub geometries_not_triangles: usize,
    /// A normal with no direction in it, replaced so the file stays valid.
    pub normals_substituted: usize,
    pub tangents_substituted: usize,
    /// A drawable whose geometry is skinned against a skeleton it does not
    /// carry — every ped component `.ydd` is one.
    pub skinned_without_a_skeleton: usize,
    /// A texture a material names that no dictionary here holds.
    pub textures_not_carried: Vec<String>,
    /// A texture that is here and would not decode.
    pub textures_not_decoded: Vec<String>,
    /// A primitive whose shader index is past the end of the shader group.
    pub shader_indices_out_of_range: usize,
}

impl ExtractReport {
    pub fn is_clean(&self) -> bool {
        self.geometries_without_positions == 0
            && self.geometries_with_partial_triangles == 0
            && self.indices_out_of_range == 0
            && self.influences_outside_the_palette == 0
            && self.geometries_not_triangles == 0
            && self.textures_not_carried.is_empty()
            && self.textures_not_decoded.is_empty()
            && self.shader_indices_out_of_range == 0
    }
}

/// Something a file held that a drawable has no place for, and how much of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub what: String,
    pub count: usize,
}

impl Dropped {
    pub fn new(what: impl Into<String>, count: usize) -> Self {
        Self {
            what: what.into(),
            count,
        }
    }
}

impl std::fmt::Display for Dropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.count, self.what)
    }
}

/// Everything one interchange file carries.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub name: String,
    pub objects: Vec<Object>,
    /// Pooled across objects, because a dictionary's entries share materials.
    pub materials: Vec<Material>,
    pub images: Vec<Image>,
    pub report: ExtractReport,
    /// What a reader left out of the file, which an edit of a drawable has no place for.
    pub dropped: Vec<Dropped>,
}

impl Scene {
    pub fn primitives(&self) -> impl Iterator<Item = &Primitive> {
        self.objects
            .iter()
            .flat_map(|o| o.meshes.iter())
            .flat_map(|m| m.primitives.iter())
    }

    pub fn vertex_count(&self) -> usize {
        self.primitives().map(Primitive::vertex_count).sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.primitives().map(Primitive::triangle_count).sum()
    }
}
