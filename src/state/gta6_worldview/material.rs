//! What a mesh's material is on the CPU: its family, the maps of each layer and those beside them,
//! and the constants its arithmetic reads, each found by the record hash its drawable states.
//!
//! ⚠ A drawable's record states the effect record's annotation hash, not its variable's: `BumpTex`,
//! never `bumpTexture`, in a one-layer effect. A record's key is its slot in the drawable's own
//! order, never the effect's.

use std::hash::{Hash, Hasher};

use or_io::gta6::hash::name_hash32;
use or_io::gta6::headerless::{ParameterValue, Shader};

/// Which arithmetic a material's colour and surface words take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Family {
    Standard,
    TwoLayer,
    FourLayer,
    Decal,
    Glass,
    /// A one-layer material whose emissive map is bound.
    Emissive,
    Grass,
    TerrainLod,
    /// A surface that marks water for the game's own water pass: its mesh is never drawn.
    WaterMask,
    #[default]
    Other,
}

/// The maps of one layer, as texture-pack keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Layer {
    /// `None` too for a material that states a normal map alone: a normal map is no colour.
    pub colour: Option<u64>,
    pub normal: Option<u64>,
    pub material_a: Option<u64>,
    pub material_b: Option<u64>,
}

/// What an emissive map adds to a material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emissive {
    pub map: u64,
    pub multiplier: f32,
    /// The strength at night; below 0 the multiplier holds all day.
    pub night: f32,
    pub scale: [f32; 3],
    pub power: [f32; 3],
    /// The power of `N.V` the glow is scaled by where above 0.
    pub facing: f32,
    /// How far the glow takes the colour map's tint.
    pub diffuse: f32,
}

/// The constants a material's arithmetic reads: its own, or where it states none the default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constants {
    pub fresnel: [f32; 4],
    pub bumpiness: [f32; 4],
    /// The palette tint's strength per layer: a two-layer material's own, 1 for every other.
    pub palette_tint: [f32; 4],
    pub translucency_as_tint_mask: f32,
    /// The palette row.
    pub tint_palette_selector: f32,
    pub hbb_range_scale: [f32; 4],
    pub hbb_range_bias: [f32; 4],
    /// Per layer of a four-layer material; a two-layer one's is the first.
    pub blend_softness: [f32; 4],
    pub tint_intensity: [f32; 4],
    pub tint_blend_amount: [f32; 4],
    pub vertex_tint_scale: [f32; 4],
    pub vertex_tint_amount: [f32; 4],
    pub vertex_tint: bool,
    /// A layer's mask is its material map's alpha, else one less it.
    pub mask_is_alpha: bool,
    pub control_filter: [f32; 4],
    pub inv_control: bool,
    /// Added to a two-layer material's second layer's weight.
    pub layer_weight_bias: f32,
    pub dirt_intensity: f32,
    pub hard_light: bool,
    /// A decal's opacity into colour, normal, smoothness and specular, and ambient.
    pub decal_blend_opacity: [f32; 4],
    pub gtintensity: f32,
    /// A four-layer material's scale and offset of the first and of the second texture coordinate.
    pub uv0_transform: [f32; 4],
    pub uv1_transform: [f32; 4],
    /// By layer, the coordinate set its maps are sampled by: 1 the first, 2 the second.
    pub uv_sets: [f32; 4],
    /// The set the lookup or control map is sampled by: 1, 2 or 3.
    pub lookup_uv_set: f32,
    /// A two-layer material's scale of its second layer's coordinate.
    pub layer1_uv_scale: [f32; 2],
}

impl Default for Constants {
    fn default() -> Self {
        Self {
            fresnel: [0.0; 4],
            bumpiness: [1.0; 4],
            palette_tint: [1.0; 4],
            translucency_as_tint_mask: 0.0,
            tint_palette_selector: 0.0,
            hbb_range_scale: [1.0; 4],
            hbb_range_bias: [0.0; 4],
            blend_softness: [0.5; 4],
            tint_intensity: [1.0; 4],
            tint_blend_amount: [0.0; 4],
            vertex_tint_scale: [1.0; 4],
            vertex_tint_amount: [0.0; 4],
            vertex_tint: false,
            mask_is_alpha: true,
            control_filter: [0.0, 0.0, 1.0, 0.0],
            inv_control: false,
            layer_weight_bias: 0.0,
            dirt_intensity: 1.0,
            hard_light: false,
            decal_blend_opacity: [1.0; 4],
            gtintensity: 0.0,
            uv0_transform: [1.0, 1.0, 0.0, 0.0],
            uv1_transform: [1.0, 1.0, 0.0, 0.0],
            uv_sets: [1.0; 4],
            lookup_uv_set: 1.0,
            layer1_uv_scale: [1.0; 2],
        }
    }
}

/// A mesh's material: its family, the maps of each layer and those beside them, and its constants.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CpuMaterial {
    pub family: Family,
    /// One for a one-layer family, two or four for a layered one.
    pub layers: Vec<Layer>,
    /// A four-layer material's layer weights or a two-layer one's control map.
    pub lookup: Option<u64>,
    /// The palette the mesh stage reads a texel of.
    pub palette: Option<u64>,
    /// A four-layer material's tint by UV.
    pub tint: Option<u64>,
    pub tint_normal: Option<u64>,
    /// A colour over the layers by its alpha.
    pub dirt: Option<u64>,
    pub emissive: Option<Emissive>,
    pub constants: Constants,
}

// ponytail: equal by value, hashed by its Debug text: a 0.0 and a -0.0, or a NaN, only make two materials of one.
impl Eq for CpuMaterial {}

impl Hash for CpuMaterial {
    fn hash<H: Hasher>(&self, state: &mut H) {
        format!("{self:?}").hash(state);
    }
}

impl CpuMaterial {
    /// Every texture key it names, each once, the first layer's colour first.
    pub fn keys(&self) -> Vec<u64> {
        let layers =
            (self.layers.iter()).flat_map(|l| [l.colour, l.normal, l.material_a, l.material_b]);
        let beside = [
            self.lookup,
            self.palette,
            self.tint,
            self.tint_normal,
            self.dirt,
        ];
        let emissive = self.emissive.map(|e| e.map);
        let mut keys = Vec::new();
        for key in layers.chain(beside).chain([emissive]).flatten() {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }
}

/// A layer's records: colour, normal, material A, material B; each the first of its hashes stated.
type Slots = [&'static [u32]; 4];

/// The one layer of a one-layer material; the second hash of each is a terrain LOD's.
const ONE: Slots = [
    &[name_hash32(b"DiffuseTex")],
    &[name_hash32(b"BumpTex"), name_hash32(b"bumpTexture")],
    &[name_hash32(b"SpecularTex"), name_hash32(b"SpecularTexture")],
    &[
        name_hash32(b"SpecularTex2"),
        name_hash32(b"MaterialBTexture"),
    ],
];
/// The records only a terrain LOD states.
const TERRAIN: [u32; 2] = [
    name_hash32(b"SpecularTexture"),
    name_hash32(b"MaterialBTexture"),
];
const TWO: [Slots; 2] = [
    [
        &[name_hash32(b"lyr0DiffuseTex")],
        &[name_hash32(b"lyr0NormalTex")],
        &[name_hash32(b"lyr0MaterialATex")],
        &[name_hash32(b"lyr0MaterialBTex")],
    ],
    [
        &[name_hash32(b"lyr1DiffuseTex")],
        &[name_hash32(b"lyr1NormalTex")],
        &[name_hash32(b"lyr1MaterialATex")],
        &[name_hash32(b"lyr1MaterialBTex")],
    ],
];
const FOUR: [Slots; 4] = [
    [
        &[name_hash32(b"DiffuseTexture_layer0")],
        &[name_hash32(b"BumpTexture_layer0")],
        &[name_hash32(b"MaterialMapATexture_layer0")],
        &[],
    ],
    [
        &[name_hash32(b"DiffuseTexture_layer1")],
        &[name_hash32(b"BumpTexture_layer1")],
        &[name_hash32(b"MaterialMapATexture_layer1")],
        &[],
    ],
    [
        &[name_hash32(b"DiffuseTexture_layer2")],
        &[name_hash32(b"BumpTexture_layer2")],
        &[name_hash32(b"MaterialMapATexture_layer2")],
        &[],
    ],
    [
        &[name_hash32(b"DiffuseTexture_layer3")],
        &[name_hash32(b"BumpTexture_layer3")],
        &[name_hash32(b"MaterialMapATexture_layer3")],
        &[],
    ],
];
const LOOKUP: [u32; 2] = [
    name_hash32(b"lookupTexture"),
    name_hash32(b"controlTextureTex"),
];
const PALETTE: u32 = name_hash32(b"TintPaletteTex");
const TINT: u32 = name_hash32(b"tintTexture");
const TINT_NORMAL: u32 = name_hash32(b"tintBumpTexture");
const DIRT: [u32; 2] = [name_hash32(b"DiffuseTex2"), name_hash32(b"dirtDiffuseTex")];
const EMISSIVE_MAP: u32 = name_hash32(b"EmissiveTex");

/// The hash of the constant a material that glows scales its glow by.
pub(super) const GLOW: u32 = name_hash32(b"emissivemultiplier");
const NIGHT: u32 = 0x823D_5749;
/// The two-layer interior effect's annotation of the night strength, which its drawables state instead.
const NIGHT_INTERIOR: u32 = 0x605D_899E;
const GLOW_SCALE: u32 = 0x7AB0_FD74;
const GLOW_POWER: u32 = 0x5FCB_8723;
const GLOW_FACING: u32 = 0x7FBC_292C;
const GLOW_DIFFUSE: u32 = 0xC0FE_E36D;

const FRESNEL: u32 = name_hash32(b"Fresnel");
const BUMPINESS: u32 = name_hash32(b"Bumpiness");
const LAYER_FRESNEL: [u32; 2] = [name_hash32(b"lyr0Fresnel"), name_hash32(b"lyr1Fresnel")];
const LAYER_BUMPINESS: [u32; 2] = [name_hash32(b"lyr0Bumpiness"), name_hash32(b"lyr1Bumpiness")];
const LAYER_TINT: [u32; 2] = [name_hash32(b"lyr0Tint"), name_hash32(b"lyr1Tint")];
const TINT_MASK: u32 = name_hash32(b"translucencyAsTintMask");
const PALETTE_SELECTOR: u32 = name_hash32(b"tintPaletteSelector");
const HBB_SCALE: u32 = name_hash32(b"HBBRangeScale1234");
const HBB_BIAS: u32 = name_hash32(b"HBBRangeBias1234");
const SOFTNESS: u32 = name_hash32(b"blendSoftness");
const TINT_INTENSITY: u32 = name_hash32(b"tintIntensity");
const TINT_BLEND: u32 = name_hash32(b"TintBlendAmount");
const VERTEX_TINT_SCALE: u32 = 0x81BC_5BCA;
const VERTEX_TINT_AMOUNT: u32 = 0x896D_1E47;
const VERTEX_TINT: u32 = 0x3FB2_B8F7;
const MASK_IS_ALPHA: u32 = 0x7A09_0C2A;
const CONTROL_FILTER: u32 = name_hash32(b"controlFilter");
const INV_CONTROL: u32 = name_hash32(b"invControl");
const LAYER_WEIGHT_BIAS: u32 = 0x2926_5D05;
const DIRT_INTENSITY: u32 = name_hash32(b"dirtIntensity");
const HARD_LIGHT: u32 = name_hash32(b"bUseHardLight");
const DECAL_OPACITY: [u32; 4] = [
    name_hash32(b"DecalBlendOpacityGBuffer0"),
    name_hash32(b"DecalBlendOpacityGBuffer1"),
    name_hash32(b"DecalBlendOpacityGBuffer2"),
    name_hash32(b"DecalBlendOpacityGBuffer3"),
];
const GRASS_TINT: u32 = name_hash32(b"gtintensity");
const UV0_TRANSFORM: u32 = 0x4344_D2CC;
const UV1_TRANSFORM: u32 = 0x9ECA_AA1E;
const UV_SETS: u32 = name_hash32(b"uvSet");
const LAYER_UV_SETS: [u32; 2] = [name_hash32(b"lyr0UVSet"), name_hash32(b"lyr1UVSet")];
const LOOKUP_UV_SET: [u32; 2] = [name_hash32(b"lookupUVSet"), name_hash32(b"controlUVSet")];
const LAYER1_UV_SCALE: u32 = name_hash32(b"lyr1UVScale");

/// A shader's records and constants, by the hash its drawable states for each.
struct Records<'a>(&'a Shader);

impl Records<'_> {
    /// Whether the shader states a texture record of `hash`, bound or not.
    fn states(&self, hash: u32) -> bool {
        (self.0.parameters.iter())
            .any(|p| p.hash == hash && matches!(p.value, ParameterValue::Texture { .. }))
    }

    /// The key bound to the texture record of `hash`.
    fn key(&self, hash: u32) -> Option<u64> {
        self.0.parameters.iter().find_map(|p| match p.value {
            ParameterValue::Texture { slot } if p.hash == hash => self
                .0
                .texture_keys
                .get(usize::from(slot))
                .copied()
                .flatten(),
            _ => None,
        })
    }

    fn first_key(&self, hashes: &[u32]) -> Option<u64> {
        hashes.iter().find_map(|h| self.key(*h))
    }

    fn layer(&self, records: &Slots) -> Layer {
        Layer {
            colour: self.first_key(records[0]),
            normal: self.first_key(records[1]),
            material_a: self.first_key(records[2]),
            material_b: self.first_key(records[3]),
        }
    }

    fn states_layer(&self, records: &Slots) -> bool {
        records
            .iter()
            .any(|hashes| hashes.iter().any(|h| self.states(*h)))
    }

    /// The words of the constant of `hash`.
    fn words(&self, hash: u32) -> Option<&[u32]> {
        self.0.parameters.iter().find_map(|p| match &p.value {
            ParameterValue::Constant { words, .. } if p.hash == hash => Some(&words[..]),
            _ => None,
        })
    }

    /// The constant of `hash` as floats over `default`: a word it does not state keeps the default's.
    fn floats<const N: usize>(&self, hash: u32, default: [f32; N]) -> [f32; N] {
        let mut out = default;
        for (slot, word) in out.iter_mut().zip(self.words(hash).unwrap_or_default()) {
            *slot = f32::from_bits(*word);
        }
        out
    }

    fn float(&self, hash: u32, default: f32) -> f32 {
        self.floats(hash, [default])[0]
    }

    /// The coordinate sets of the constant of `hash` over `default`, each stated as an integer or a float.
    fn sets<const N: usize>(&self, hash: u32, default: [f32; N]) -> [f32; N] {
        let mut out = default;
        for (slot, word) in out.iter_mut().zip(self.words(hash).unwrap_or_default()) {
            *slot = match *word {
                small @ 0..=0xFF => small as f32,
                bits => f32::from_bits(bits),
            };
        }
        out
    }

    fn flag(&self, hash: u32, default: bool) -> bool {
        self.words(hash)
            .and_then(|words| words.first())
            .map_or(default, |w| f32::from_bits(*w) != 0.0)
    }
}

/// `shader`'s constants.
fn constants_of(r: &Records) -> Constants {
    let d = Constants::default();
    let mut c = Constants {
        fresnel: r.floats(FRESNEL, d.fresnel),
        bumpiness: r.floats(BUMPINESS, d.bumpiness),
        palette_tint: d.palette_tint,
        translucency_as_tint_mask: r.float(TINT_MASK, d.translucency_as_tint_mask),
        tint_palette_selector: r.float(PALETTE_SELECTOR, d.tint_palette_selector),
        hbb_range_scale: r.floats(HBB_SCALE, d.hbb_range_scale),
        hbb_range_bias: r.floats(HBB_BIAS, d.hbb_range_bias),
        blend_softness: r.floats(SOFTNESS, d.blend_softness),
        tint_intensity: r.floats(TINT_INTENSITY, d.tint_intensity),
        tint_blend_amount: r.floats(TINT_BLEND, d.tint_blend_amount),
        vertex_tint_scale: r.floats(VERTEX_TINT_SCALE, d.vertex_tint_scale),
        vertex_tint_amount: r.floats(VERTEX_TINT_AMOUNT, d.vertex_tint_amount),
        vertex_tint: r.flag(VERTEX_TINT, d.vertex_tint),
        mask_is_alpha: r.flag(MASK_IS_ALPHA, d.mask_is_alpha),
        control_filter: r.floats(CONTROL_FILTER, d.control_filter),
        inv_control: r.flag(INV_CONTROL, d.inv_control),
        layer_weight_bias: r.float(LAYER_WEIGHT_BIAS, d.layer_weight_bias),
        dirt_intensity: r.float(DIRT_INTENSITY, d.dirt_intensity),
        hard_light: r.flag(HARD_LIGHT, d.hard_light),
        decal_blend_opacity: DECAL_OPACITY.map(|hash| r.float(hash, 1.0)),
        gtintensity: r.float(GRASS_TINT, d.gtintensity),
        uv0_transform: r.floats(UV0_TRANSFORM, d.uv0_transform),
        uv1_transform: r.floats(UV1_TRANSFORM, d.uv1_transform),
        uv_sets: r.sets(UV_SETS, d.uv_sets),
        lookup_uv_set: r.sets(
            LOOKUP_UV_SET[1],
            r.sets(LOOKUP_UV_SET[0], [d.lookup_uv_set]),
        )[0],
        layer1_uv_scale: r.floats(LAYER1_UV_SCALE, d.layer1_uv_scale),
    };
    for (set, hash) in c.uv_sets.iter_mut().zip(LAYER_UV_SETS) {
        *set = r.sets(hash, [*set])[0];
    }
    // A two-layer material states each layer's own under a name of its own.
    let per_layer = [
        (&mut c.fresnel, LAYER_FRESNEL),
        (&mut c.bumpiness, LAYER_BUMPINESS),
        (&mut c.palette_tint, LAYER_TINT),
    ];
    for (values, hashes) in per_layer {
        for (value, hash) in values.iter_mut().zip(hashes) {
            *value = r.float(hash, *value);
        }
    }
    c
}

fn emissive_of(r: &Records) -> Option<Emissive> {
    Some(Emissive {
        map: r.key(EMISSIVE_MAP)?,
        multiplier: r.float(GLOW, 1.0),
        night: r.float(NIGHT, r.float(NIGHT_INTERIOR, -1.0)),
        scale: r.floats(GLOW_SCALE, [1.0; 3]),
        power: r.floats(GLOW_POWER, [1.0; 3]),
        facing: r.float(GLOW_FACING, 0.0),
        diffuse: r.float(GLOW_DIFFUSE, 1.0),
    })
}

/// The material `shader` states, drawn in the draw bucket of `byte`; for a colour map its records
/// do not name, the first key `srgb` holds, else the first it knows nothing of.
///
/// ⚠ A key a pack states as linear is a data or normal map: drawn as a colour it tints the whole
/// mesh green and blue.
pub(super) fn material_of(
    shader: &Shader,
    byte: u8,
    srgb: &dyn Fn(u64) -> Option<bool>,
) -> CpuMaterial {
    let r = Records(shader);
    let (shape, layers): (Family, Vec<Layer>) = if FOUR.iter().any(|l| r.states_layer(l)) {
        (Family::FourLayer, FOUR.iter().map(|l| r.layer(l)).collect())
    } else if TWO.iter().any(|l| r.states_layer(l)) {
        (Family::TwoLayer, TWO.iter().map(|l| r.layer(l)).collect())
    } else if TERRAIN.iter().any(|h| r.states(*h)) {
        (Family::TerrainLod, vec![r.layer(&ONE)])
    } else {
        (Family::Standard, vec![r.layer(&ONE)])
    };
    let mut material = CpuMaterial {
        family: shape,
        layers,
        lookup: r.first_key(&LOOKUP),
        palette: r.key(PALETTE),
        tint: r.key(TINT),
        tint_normal: r.key(TINT_NORMAL),
        dirt: r.first_key(&DIRT),
        emissive: emissive_of(&r),
        constants: constants_of(&r),
    };
    if material.layers[0].colour.is_none() {
        // A key a record names is that record's map, never the colour.
        let named_keys = material.keys();
        let named = &named_keys;
        let others = || {
            let keys = shader.texture_keys.iter().flatten().copied();
            keys.filter(move |k| !named.contains(k))
        };
        material.layers[0].colour = (others().find(|k| srgb(*k) == Some(true)))
            .or_else(|| others().find(|k| srgb(*k).is_none()));
    }
    let any_map = (material.layers.iter()).any(|l| {
        [l.colour, l.normal, l.material_a, l.material_b]
            .iter()
            .any(Option::is_some)
    });
    material.family = match byte & 0x7F {
        6 => Family::WaterMask,
        2 => Family::Decal,
        1 => Family::Glass,
        _ if matches!(shape, Family::FourLayer | Family::TwoLayer) => shape,
        _ if r.words(GRASS_TINT).is_some() => Family::Grass,
        _ if material.emissive.is_some() => Family::Emissive,
        _ if shape == Family::TerrainLod => shape,
        _ if any_map => Family::Standard,
        _ => Family::Other,
    };
    material
}

#[cfg(test)]
#[path = "material_tests.rs"]
mod tests;
