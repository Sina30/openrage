use serde::{Deserialize, Serialize};

use crate::domain::Vec3;
use crate::evolution::Colour;
use crate::keyframe::KeyframePropData;
use or_io::gta5::particle::{
    self as ptx, AlignmentMode, AnimTexLoopMode, KeyframeMode, KfpMeta, ReferenceSpace,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Behaviour {
    Acceleration(AccelerationBehaviour),

    Velocity(VelocityBehaviour),

    Rotation(RotationBehaviour),

    RotationSpeed(RotationSpeedBehaviour),

    Colour(ColourBehaviour),

    Size(SizeBehaviour),

    AnimateTexture(AnimateTextureBehaviour),

    FadeIn(FadeInBehaviour),

    FadeOut(FadeOutBehaviour),

    Noise(NoiseBehaviour),

    Wind(WindBehaviour),

    Dampening(DampeningBehaviour),

    Gravity(GravityBehaviour),

    Collision(CollisionBehaviour),

    Attractor(AttractorBehaviour),

    DrawSprite(DrawSpriteBehaviour),

    DrawTrail(DrawTrailBehaviour),

    DrawModel(DrawModelBehaviour),

    Decal(DecalBehaviour),

    FogVolume(FogVolumeBehaviour),

    Light(LightBehaviour),

    Liquid(LiquidBehaviour),

    DecalPool(DecalPoolBehaviour),

    River(RiverBehaviour),

    Age(AgeBehaviour),

    Pause(PauseBehaviour),

    ZCulling(ZCullingBehaviour),

    MatrixWeight(MatrixWeightBehaviour),

    Unknown { type_id: u32, data: Vec<u8> },
}

macro_rules! declare_behaviours {
    ($($hash:ident, $variant:ident, $kfps:ident;)+) => {
        impl Behaviour {
            pub fn kfp_property_ids(&self) -> &[u32] {
                match self {
                    $( Self::$variant(_) => ptx::$kfps, )+
                    Self::Unknown { .. } => &[],
                }
            }
            pub fn type_id(&self) -> u32 {
                match self {
                    $( Self::$variant(_) => ptx::$hash, )+
                    Self::Unknown { type_id, .. } => *type_id,
                }
            }
        }
    };
}

declare_behaviours! {
    BEHAVIOUR_HASH_AGE, Age, KFP_AGE;
    BEHAVIOUR_HASH_ACCELERATION, Acceleration, KFP_ACCELERATION;
    BEHAVIOUR_HASH_VELOCITY, Velocity, KFP_VELOCITY;
    BEHAVIOUR_HASH_ROTATION, Rotation, KFP_ROTATION;
    BEHAVIOUR_HASH_ROTATION_SPEED, RotationSpeed, KFP_ROTATION_SPEED;
    BEHAVIOUR_HASH_SIZE, Size, KFP_SIZE;
    BEHAVIOUR_HASH_COLOUR, Colour, KFP_COLOUR;
    BEHAVIOUR_HASH_ANIMATE_TEXTURE, AnimateTexture, KFP_ANIMATE_TEXTURE;
    BEHAVIOUR_HASH_DAMPENING, Dampening, KFP_DAMPENING;
    BEHAVIOUR_HASH_MATRIX_WEIGHT, MatrixWeight, KFP_MATRIX_WEIGHT;
    BEHAVIOUR_HASH_COLLISION, Collision, KFP_COLLISION;
    BEHAVIOUR_HASH_WIND, Wind, KFP_WIND;
    BEHAVIOUR_HASH_NOISE, Noise, KFP_NOISE;
    BEHAVIOUR_HASH_ATTRACTOR, Attractor, KFP_ATTRACTOR;
    BEHAVIOUR_HASH_PAUSE, Pause, KFP_PAUSE;
    BEHAVIOUR_HASH_FADE_IN, FadeIn, KFP_FADE_IN;
    BEHAVIOUR_HASH_FADE_OUT, FadeOut, KFP_FADE_OUT;
    BEHAVIOUR_HASH_GRAVITY, Gravity, KFP_GRAVITY;
    BEHAVIOUR_HASH_SPRITE, DrawSprite, KFP_SPRITE;
    BEHAVIOUR_HASH_TRAIL, DrawTrail, KFP_TRAIL;
    BEHAVIOUR_HASH_MODEL, DrawModel, KFP_MODEL;
    BEHAVIOUR_HASH_LIGHT, Light, KFP_LIGHT;
    BEHAVIOUR_HASH_DECAL, Decal, KFP_DECAL;
    BEHAVIOUR_HASH_ZCULL, ZCulling, KFP_ZCULL;
    BEHAVIOUR_HASH_FOG_VOLUME, FogVolume, KFP_FOG_VOLUME;
    BEHAVIOUR_HASH_RIVER, River, KFP_RIVER;
    BEHAVIOUR_HASH_DECAL_POOL, DecalPool, KFP_DECAL_POOL;
    BEHAVIOUR_HASH_LIQUID, Liquid, KFP_LIQUID;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccelerationBehaviour {
    pub xyz_min: Vec3,
    pub xyz_max: Vec3,
    pub xyz_min_keyframes: Option<KeyframePropData>,
    pub xyz_max_keyframes: Option<KeyframePropData>,
    pub reference_space: ReferenceSpace,
    pub is_affected_by_zoom: bool,
    pub enable_gravity: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VelocityBehaviour {
    pub xyz_min: Vec3,
    pub xyz_max: Vec3,
    pub xyz_min_keyframes: Option<KeyframePropData>,
    pub xyz_max_keyframes: Option<KeyframePropData>,
    pub reference_space: ReferenceSpace,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RotationBehaviour {
    pub initial_angle_min_keyframes: Option<KeyframePropData>,
    pub initial_angle_max_keyframes: Option<KeyframePropData>,
    pub angle_min_keyframes: Option<KeyframePropData>,
    pub angle_max_keyframes: Option<KeyframePropData>,
    pub init_rotation_mode: u32,
    pub update_rotation_mode: u32,
    pub accumulate_angle: bool,
    pub rotate_angle_axes: bool,
    pub rotate_init_angle_axes: bool,
    pub speed_fade_thresh: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RotationSpeedBehaviour {
    pub speed_min: f32,
    pub speed_max: f32,
    pub axis: Vec3,
    pub keyframes: Option<KeyframePropData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ColourBehaviour {
    pub rgba_min: Colour,
    pub rgba_max: Colour,
    pub rgba_min_keyframes: Option<KeyframePropData>,
    pub rgba_max_keyframes: Option<KeyframePropData>,
    pub emissive_intensity_min: f32,
    pub emissive_intensity_max: f32,
    pub emissive_intensity_keyframes: Option<KeyframePropData>,
    pub keyframe_mode: KeyframeMode,
    pub rgba_max_enable: bool,
    pub rgba_proportional: bool,
    pub rgb_can_tint: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SizeBehaviour {
    pub whd_min_keyframes: Option<KeyframePropData>,
    pub whd_max_keyframes: Option<KeyframePropData>,
    pub tblr_scalar_keyframes: Option<KeyframePropData>,
    pub tblr_vel_scalar_keyframes: Option<KeyframePropData>,
    pub keyframe_mode: KeyframeMode,
    pub is_proportional: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnimateTextureBehaviour {
    pub anim_rate_min: f32,
    pub anim_rate_max: f32,
    pub anim_rate_keyframes: Option<KeyframePropData>,
    pub keyframe_mode: KeyframeMode,
    pub last_frame_id: u32,
    pub loop_mode: AnimTexLoopMode,
    pub is_randomised: bool,
    pub is_scaled_over_particle_life: bool,
    pub is_held_on_last_frame: bool,
    pub do_frame_blending: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_scroll_rate_u_keyframes: Option<KeyframePropData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_scroll_rate_v_keyframes: Option<KeyframePropData>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FadeInBehaviour {
    pub duration: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FadeOutBehaviour {
    pub duration: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NoiseBehaviour {
    pub pos_noise_min_keyframes: Option<KeyframePropData>,
    pub pos_noise_max_keyframes: Option<KeyframePropData>,
    pub vel_noise_min_keyframes: Option<KeyframePropData>,
    pub vel_noise_max_keyframes: Option<KeyframePropData>,
    pub reference_space: ReferenceSpace,
    pub keep_constant_speed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WindBehaviour {
    pub influence_keyframes: Option<KeyframePropData>,
    pub high_lod_range: f32,
    pub low_lod_range: f32,
    pub high_lod_disturbance_mode: u32,
    pub low_lod_disturbance_mode: u32,
    pub ignore_mtx_weight: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DampeningBehaviour {
    pub xyz_min: Vec3,
    pub xyz_max: Vec3,
    pub xyz_min_keyframes: Option<KeyframePropData>,
    pub xyz_max_keyframes: Option<KeyframePropData>,
    pub reference_space: ReferenceSpace,
    pub enable_air_resistance: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GravityBehaviour {
    pub strength: f32,
    pub direction: Vec3,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollisionBehaviour {
    pub bounciness_keyframes: Option<KeyframePropData>,
    pub bounce_dir_var_keyframes: Option<KeyframePropData>,
    pub radius_mult: f32,
    pub rest_speed: f32,
    pub collision_chance: u32,
    pub kill_chance: u32,
    pub debug_draw: bool,
    pub override_min_radius: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttractorBehaviour {
    pub strength_min: f32,
    pub strength_max: f32,
    pub strength_keyframes: Option<KeyframePropData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DrawSpriteBehaviour {
    pub align_axis: Vec3,
    pub alignment_mode: AlignmentMode,
    pub flip_chance_u: f32,
    pub flip_chance_v: f32,
    pub near_clip_dist: f32,
    pub far_clip_dist: f32,
    pub projection_depth: f32,
    pub shadow_cast_intensity: f32,
    pub is_screen_space: bool,
    pub is_hi_res: bool,
    pub near_clip: bool,
    pub far_clip: bool,
    pub uv_clip: bool,
    pub disable_draw: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DrawTrailBehaviour {
    pub tex_info_keyframes: Option<KeyframePropData>,
    pub align_axis: Vec3,
    pub alignment_mode: AlignmentMode,
    pub tessellation_u: u32,
    pub tessellation_v: u32,
    pub smoothness_x: f32,
    pub smoothness_y: f32,
    pub projection_depth: f32,
    pub shadow_cast_intensity: f32,
    pub flip_u: bool,
    pub flip_v: bool,
    pub wrap_texture_over_particle_life: bool,
    pub disable_draw: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DrawModelBehaviour {
    pub colour_control_shader_id: u32,
    pub camera_shrink: f32,
    pub shadow_cast_intensity: f32,
    pub disable_draw: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecalBehaviour {
    pub dimensions_keyframes: Option<KeyframePropData>,
    pub alpha_keyframes: Option<KeyframePropData>,
    pub decal_id: u32,
    pub velocity_thresh: f32,
    pub total_life: f32,
    pub fade_in_time: f32,
    pub uv_mult_start: f32,
    pub uv_mult_end: f32,
    pub uv_mult_time: f32,
    pub duplicate_reject_dist: f32,
    pub flip_u: bool,
    pub flip_v: bool,
    pub proportional_size: bool,
    pub use_complex_coln: bool,
    pub projection_depth: f32,
    pub distance_scale: f32,
    pub is_directional: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FogVolumeBehaviour {
    pub rgb_tint_min_keyframes: Option<KeyframePropData>,
    pub rgb_tint_max_keyframes: Option<KeyframePropData>,
    pub density_range_keyframes: Option<KeyframePropData>,
    pub scale_min_keyframes: Option<KeyframePropData>,
    pub scale_max_keyframes: Option<KeyframePropData>,
    pub rotation_min_keyframes: Option<KeyframePropData>,
    pub rotation_max_keyframes: Option<KeyframePropData>,
    pub falloff: f32,
    pub hdr_mult: f32,
    pub lighting_type: u32,
    pub colour_tint_from_particle: bool,
    pub sort_with_particles: bool,
    pub use_ground_fog_colour: bool,
    pub use_effect_evo_values: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LightBehaviour {
    pub rgb_min_keyframes: Option<KeyframePropData>,
    pub rgb_max_keyframes: Option<KeyframePropData>,
    pub intensity_keyframes: Option<KeyframePropData>,
    pub range_keyframes: Option<KeyframePropData>,
    pub corona_rgb_min_keyframes: Option<KeyframePropData>,
    pub corona_rgb_max_keyframes: Option<KeyframePropData>,
    pub corona_intensity_keyframes: Option<KeyframePropData>,
    pub corona_size_keyframes: Option<KeyframePropData>,
    pub corona_flare_keyframes: Option<KeyframePropData>,
    pub corona_z_bias: f32,
    pub corona_use_light_col: bool,
    pub colour_from_particle: bool,
    pub colour_per_frame: bool,
    pub intensity_per_frame: bool,
    pub range_per_frame: bool,
    pub casts_shadows: bool,
    pub corona_not_in_reflection: bool,
    pub corona_only_in_reflection: bool,
    pub light_type: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LiquidBehaviour {
    pub velocity_thresh: f32,
    pub liquid_type: u32,
    pub pool_start_size: f32,
    pub pool_end_size: f32,
    pub pool_growth_rate: f32,
    pub trail_width_min: f32,
    pub trail_width_max: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecalPoolBehaviour {
    pub velocity_thresh: f32,
    pub liquid_type: u32,
    pub decal_id: u32,
    pub start_size: f32,
    pub end_size: f32,
    pub growth_rate: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RiverBehaviour {
    pub velocity_mult: f32,
    pub influence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgeBehaviour {
    pub age_rate: f32,
}

impl Default for AgeBehaviour {
    fn default() -> Self {
        Self { age_rate: 1.0 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PauseBehaviour {
    pub pause_life_ratio: f32,
    pub unpause_time: f32,
    pub unpause_effect_dist: f32,
    pub unpause_cam_dist: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ZCullingBehaviour {
    pub height_keyframes: Option<KeyframePropData>,
    pub fade_dist_keyframes: Option<KeyframePropData>,
    pub mode: u32,
    pub reference_space: ReferenceSpace,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatrixWeightBehaviour {
    pub weight_keyframes: Option<KeyframePropData>,
    pub reference_space: ReferenceSpace,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kfp_meta: Vec<KfpMeta>,
}
