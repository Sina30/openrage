use crate::behaviour::Behaviour;
use or_io::gta5::particle::ShaderVar;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ParticleSortType {
    #[default]
    ClosestFirst = 0,
    FarthestFirst = 1,
    NewestFirst = 2,
    OldestFirst = 3,
    Random = 4,
}

impl ParticleSortType {
    pub fn from_u8(b: u8) -> Self {
        match b {
            0 => Self::ClosestFirst,
            1 => Self::FarthestFirst,
            2 => Self::NewestFirst,
            3 => Self::OldestFirst,
            4 => Self::Random,
            _ => Self::ClosestFirst,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn requires_distance_sort(self) -> bool {
        matches!(self, Self::ClosestFirst | Self::FarthestFirst)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SpawnedEffectScalars {
    pub duration_scalar: f32,
    pub playback_rate_scalar: f32,
    pub colour_tint_scalar: u32,
    pub zoom_scalar: f32,
    pub flags: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EffectSpawner {
    pub effect_rule_name: String,
    pub trigger_info: f32,
    pub scalars_min: SpawnedEffectScalars,
    pub scalars_max: SpawnedEffectScalars,
    pub inherits_point_life: bool,
    pub tracks_point_pos: bool,
    pub tracks_point_dir: bool,
    pub tracks_point_neg_dir: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BiasLink {
    pub name: String,
    pub keyframe_prop_ids: Vec<u32>,
    pub rand_index: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleRule {
    pub name: String,
    pub draw_type: DrawType,
    pub blend_mode: BlendMode,
    pub cull_mode: CullMode,
    pub depth_write: bool,
    pub depth_test: bool,
    pub depth_bias: f32,
    pub lighting_enabled: bool,
    pub ambient_factor: f32,
    pub directional_factor: f32,
    pub soft_particle: bool,
    pub soft_particle_distance: f32,
    pub shader_template_name: String,
    pub shader_technique_name: String,
    pub texture_dictionary: String,
    pub texture_name: String,
    pub uv_anim_rows: u32,
    pub uv_anim_cols: u32,
    pub uv_anim_frame_rate: f32,
    pub drawable_refs: Vec<DrawableRef>,
    pub tex_frame_id_min: i32,
    pub tex_frame_id_max: i32,
    pub flags: u8,
    pub sort_type: u8,
    pub runtime_flags: u8,
    pub file_version: f32,
    pub diffuse_mode: u32,
    pub proj_mode: u32,
    pub is_lit: bool,
    pub is_soft: bool,
    pub is_screen_space: bool,
    #[serde(default)]
    pub is_refract: bool,
    #[serde(default)]
    pub is_normal_spec: bool,
    pub shader_vars: Vec<ShaderVar>,
    pub effect_spawner_at_ratio: Option<EffectSpawner>,
    pub effect_spawner_on_collision: Option<EffectSpawner>,
    pub behaviours: Vec<Behaviour>,
    pub bias_links: Vec<BiasLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawableRef {
    pub name: String,
    pub lod_level: u8,
    pub scale: f32,
}

impl ParticleRule {
    pub fn new(name: String) -> Self {
        Self {
            name,
            draw_type: DrawType::Sprite,
            blend_mode: BlendMode::Additive,
            cull_mode: CullMode::None,
            depth_write: false,
            depth_test: true,
            depth_bias: 0.0,
            lighting_enabled: false,
            ambient_factor: 1.0,
            directional_factor: 0.0,
            soft_particle: true,
            soft_particle_distance: 0.1,
            shader_template_name: String::new(),
            shader_technique_name: String::new(),
            texture_dictionary: String::new(),
            texture_name: String::new(),
            uv_anim_rows: 1,
            uv_anim_cols: 1,
            uv_anim_frame_rate: 0.0,
            drawable_refs: Vec::new(),
            tex_frame_id_min: 0,
            tex_frame_id_max: 0,
            flags: 0,
            sort_type: 0,
            runtime_flags: 0,
            file_version: 4.0,
            diffuse_mode: 0,
            proj_mode: 0,
            is_lit: false,
            is_soft: true,
            is_screen_space: false,
            is_refract: false,
            is_normal_spec: false,
            shader_vars: Vec::new(),
            effect_spawner_at_ratio: None,
            effect_spawner_on_collision: None,
            behaviours: Vec::new(),
            bias_links: Vec::new(),
        }
    }
}

impl DrawableRef {
    pub fn new(name: String) -> Self {
        Self {
            name,
            lod_level: 0,
            scale: 1.0,
        }
    }

    pub fn with_scale(name: String, scale: f32) -> Self {
        Self {
            name,
            lod_level: 0,
            scale,
        }
    }
}

impl Default for DrawableRef {
    fn default() -> Self {
        Self {
            name: String::new(),
            lod_level: 0,
            scale: 1.0,
        }
    }
}

impl Default for ParticleRule {
    fn default() -> Self {
        Self::new(String::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum DrawType {
    #[default]
    Sprite = 0,
    Model = 1,
    Trail = 2,
}

impl DrawType {
    pub fn from_id(id: u32) -> Self {
        match id {
            0 => Self::Sprite,
            1 => Self::Model,
            2 => Self::Trail,
            _ => Self::Sprite,
        }
    }

    pub fn to_id(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum BlendMode {
    Alpha = 0,
    #[default]
    Additive = 1,
    Subtract = 2,
    Lightmap = 3,
    Matte = 4,
    Overwrite = 5,
    Dest = 6,
    AlphaAdd = 7,
    ReverseSubtract = 8,
    Min = 9,
    Max = 10,
    AlphaSubtract = 11,
    MultiplySrcDest = 12,
    CompositeAlpha = 13,
    CompositeAlphaSubtract = 14,
}

impl BlendMode {
    pub fn from_id(id: u32) -> Self {
        match id {
            0 => Self::Alpha,
            1 => Self::Additive,
            2 => Self::Subtract,
            3 => Self::Lightmap,
            4 => Self::Matte,
            5 => Self::Overwrite,
            6 => Self::Dest,
            7 => Self::AlphaAdd,
            8 => Self::ReverseSubtract,
            9 => Self::Min,
            10 => Self::Max,
            11 => Self::AlphaSubtract,
            12 => Self::MultiplySrcDest,
            13 => Self::CompositeAlpha,
            14 => Self::CompositeAlphaSubtract,
            _ => Self::Additive,
        }
    }

    pub fn to_id(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum CullMode {
    #[default]
    None = 0,
    Back = 1,
    Front = 2,
}

impl CullMode {
    pub fn from_id(id: u32) -> Self {
        match id {
            0 => Self::None,
            1 => Self::Back,
            2 => Self::Front,
            _ => Self::None,
        }
    }

    pub fn to_id(self) -> u32 {
        self as u32
    }
}
