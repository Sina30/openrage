//! What a material of the world is shaded by: its family, its maps by slot, and the constants its arithmetic reads.

use bytemuck::{Pod, Zeroable};

use super::WorldBucket;
use crate::viewport::MeshVertex;

/// How many maps a material binds.
pub const WORLD_MAPS: usize = 15;

/// Which map each slot of [`WorldSurface::maps`] holds; the four-layer family takes slots 9 to 11 for its fourth layer, every other family for material B and the emissive map.
pub mod slot {
    pub const fn colour(layer: usize) -> usize {
        layer * 3
    }

    pub const fn normal(layer: usize) -> usize {
        layer * 3 + 1
    }

    pub const fn material_a(layer: usize) -> usize {
        layer * 3 + 2
    }

    /// Material B of the first and the second layer.
    pub const MATERIAL_B: [usize; 2] = [9, 10];
    pub const EMISSIVE: usize = 11;
    /// The four-layer lookup map, or the two-layer control map.
    pub const WEIGHTS: usize = 12;
    /// The palette, or the four-layer tint map, which is read by the texture coordinate.
    pub const TINT: usize = 13;
    /// The four-layer tint normal map, or the dirt or second colour map.
    pub const SECOND: usize = 14;
}

/// What each slot samples where the material names no map: white, a flat normal, no metal and fully rough at mid height, no detail at mid cavity, white, black, the first layer alone, white, and a flat normal with no dirt.
pub(super) const STAND_INS: [[f32; 4]; WORLD_MAPS] = {
    let (colour, normal, a) = ([1.0; 4], [0.5, 0.5, 1.0, 0.0], [0.0, 1.0, 0.5, 0.0]);
    [
        colour,
        normal,
        a,
        colour,
        normal,
        a,
        colour,
        normal,
        a,
        // Material B of the first layer and of the second; the four-layer family's fourth layer takes the first layer's stand-ins in slots 9 to 11 instead
        [0.0, 0.0, 0.5, 1.0],
        [0.0, 0.0, 0.5, 1.0],
        // The emissive map
        [0.0; 4],
        [1.0, 0.0, 0.0, 0.0],
        [1.0; 4],
        [0.5, 0.5, 1.0, 0.0],
    ]
};

/// The arithmetic a material of the world is shaded by: the families a model's materials are read as, and terrain LOD.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum WorldFamily {
    #[default]
    Standard = 0,
    TwoLayer = 1,
    FourLayer = 2,
    /// The standard arithmetic; the bucket makes it a decal.
    Decal = 3,
    /// The standard arithmetic, in the bucket that blends.
    Glass = 4,
    /// The standard arithmetic; in the bucket that blends, the forward emissive.
    Emissive = 5,
    Grass = 6,
    Other = 7,
    TerrainLod = 8,
    /// The albedo is the vertex's first colour, for meshes the viewer builds itself.
    VertexColoured = 9,
}

/// The constants a material's arithmetic reads, by the names its effect gives them; `Default` is what most effects state.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct WorldConstants {
    pub fresnel: [f32; 4],
    /// By layer, the one- and two-layer families' too.
    pub bumpiness: [f32; 4],
    pub height_scale: [f32; 4],
    pub height_bias: [f32; 4],
    pub blend_softness: [f32; 4],
    pub tint_intensity: [f32; 4],
    pub tint_blend: [f32; 4],
    pub vertex_tint_scale: [f32; 4],
    pub vertex_tint_amount: [f32; 4],
    pub control_filter: [f32; 4],
    /// A decal's opacity into each of the four words; the first is the forward emissive's opacity.
    pub opacity: [f32; 4],
    pub emissive_scale: [f32; 4],
    pub emissive_power: [f32; 4],
    /// The scale and offset a layer's texture coordinate takes from the first and from the second set.
    pub uv0_transform: [f32; 4],
    pub uv1_transform: [f32; 4],
    /// By layer, the set its maps are sampled by: 1 the first, 2 the second.
    pub uv_sets: [f32; 4],
    /// The set the weights map is sampled by (1, 2 or 3), nought, and the second layer's scale of a two-layer material.
    pub uv_extra: [f32; 4],
    pub flags: u32,
    /// The row of the palette, which the instance's own tint states where it states one.
    pub palette_row: f32,
    pub translucency_as_tint_mask: f32,
    pub specular_fresnel: f32,
    pub layer0_tint: f32,
    pub layer1_tint: f32,
    pub control_softness: f32,
    pub control_offset: f32,
    /// The share of the second layer the game's own per-entity byte allows, which no file states.
    pub control_entity: f32,
    pub dirt_intensity: f32,
    pub grass_tint: f32,
    pub emissive_multiplier: f32,
    /// The strength at night, under nought for none.
    pub emissive_night: f32,
    pub emissive_view_power: f32,
    pub emissive_diffuse_tint: f32,
}

impl WorldConstants {
    /// The four-layer colours take the vertex's second colour.
    pub const VERTEX_TINT: u32 = 1;
    /// A layer's tint mask is its material A's alpha, else one less it.
    pub const MASK_FROM_ALPHA: u32 = 2;
    /// The two-layer control is taken from one.
    pub const INVERT_CONTROL: u32 = 4;
    /// The dirt is laid over as hard light.
    pub const HARD_LIGHT: u32 = 8;
    /// The meshes are a model's, their vertices packed by [`coloured`].
    pub const MODEL_VERTEX: u32 = 16;
    /// The albedo is the instance's tint, as a region-tint prop list's.
    pub const INSTANCE_COLOUR: u32 = 32;
    /// A ground decal: it shows where its material A's blue, between the first height bias and scale, outweighs the vertex's share, and takes its tint as its tint blend says.
    pub const GROUND_BLEND: u32 = 64;
    /// A ground decal's tint multiplies its colour rather than taking its luminance.
    pub const TINT_MULTIPLY: u32 = 128;
    /// A tree's leaves and bark: no metal, and the lighting's translucent material, which lights the side turned from the sun.
    pub const FOLIAGE: u32 = 256;
    /// With [`Self::INSTANCE_COLOUR`], the vertex's first colour lies over the instance's tint as far as its alpha, which is then no coverage.
    pub const VERTEX_OVER_TINT: u32 = 512;
}

impl Default for WorldConstants {
    fn default() -> Self {
        Self {
            fresnel: [0.0; 4],
            bumpiness: [1.0; 4],
            height_scale: [1.0; 4],
            height_bias: [0.0; 4],
            blend_softness: [0.4; 4],
            tint_intensity: [0.0; 4],
            tint_blend: [0.0; 4],
            vertex_tint_scale: [1.0; 4],
            vertex_tint_amount: [0.0; 4],
            control_filter: [0.0, 0.0, 1.0, 0.0],
            opacity: [1.0; 4],
            emissive_scale: [1.0; 4],
            emissive_power: [1.0; 4],
            uv0_transform: [1.0, 1.0, 0.0, 0.0],
            uv1_transform: [1.0, 1.0, 0.0, 0.0],
            uv_sets: [1.0; 4],
            uv_extra: [1.0, 0.0, 1.0, 1.0],
            flags: Self::VERTEX_TINT | Self::MASK_FROM_ALPHA,
            palette_row: 0.0,
            translucency_as_tint_mask: 0.0,
            specular_fresnel: 0.0,
            layer0_tint: 0.0,
            layer1_tint: 0.0,
            control_softness: 1.0,
            control_offset: 0.0,
            control_entity: 1.0,
            dirt_intensity: 0.0,
            grass_tint: 0.0,
            emissive_multiplier: 1.0,
            emissive_night: -1.0,
            emissive_view_power: 0.0,
            emissive_diffuse_tint: 1.0,
        }
    }
}

/// A material as the app hands it over: its arithmetic, its bucket, its maps by [`slot`], and its constants.
pub struct WorldSurface<'a> {
    pub family: WorldFamily,
    pub bucket: WorldBucket,
    pub maps: [Option<&'a wgpu::TextureView>; WORLD_MAPS],
    pub constants: WorldConstants,
}

/// `rgba` as the bits of a float of a vertex, which the world's shader reads as a colour.
pub fn colour1(rgba: [u8; 4]) -> f32 {
    f32::from_bits(u32::from_le_bytes(rgba))
}

/// A mesh's colours of one channel, one a vertex, if it has them.
type Colours<'a> = Option<&'a [[f32; 4]]>;
/// A mesh's texture coordinates of one set, one a vertex, if it has them.
type Coordinates<'a> = Option<&'a [[f32; 2]]>;

/// Give each of `vertices` of a model its two colours, each white where the mesh has none, and its second and third texture coordinates, each its first where the mesh has none.
pub fn coloured(
    vertices: &mut [MeshVertex],
    (first, second): (Colours, Colours),
    (uv1, uv2): (Coordinates, Coordinates),
) {
    // ⚠ A model's vertex keeps the second coordinate in the first two floats of its colour, the first colour in the bits of the third, the third coordinate as two halves in the bits of the fourth, and the second colour in the bits of its depth bias, as a material of [`WorldConstants::MODEL_VERTEX`] reads it.
    let at = |colours: Option<&[[f32; 4]]>, index: usize| {
        let colour = colours.and_then(|colours| colours.get(index)).copied();
        colour
            .unwrap_or([1.0; 4])
            .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    for (index, vertex) in vertices.iter_mut().enumerate() {
        let uv = |set: Option<&[[f32; 2]]>| set.and_then(|s| s.get(index)).copied();
        let [u1, v1] = uv(uv1).unwrap_or(vertex.uv);
        let [u2, v2] = uv(uv2).unwrap_or(vertex.uv);
        let halves =
            u32::from(or_io::half::f32_to_half(u2)) | u32::from(or_io::half::f32_to_half(v2)) << 16;
        vertex.colour = [u1, v1, colour1(at(first, index)), f32::from_bits(halves)];
        vertex.depth_bias = colour1(at(second, index));
    }
}

/// The bytes of the uniform a material's constants are read from: the family in front, the whole a multiple of sixteen as a uniform struct is.
pub(super) fn uniform_bytes(family: WorldFamily, constants: &WorldConstants) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + size_of::<WorldConstants>() + 16);
    bytes.extend_from_slice(bytemuck::bytes_of(&[family as u32, 0, 0, 0]));
    bytes.extend_from_slice(bytemuck::bytes_of(constants));
    bytes.resize(bytes.len().next_multiple_of(16), 0);
    bytes
}
