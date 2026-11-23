//! `.ypt`: a particle effects list — effect rules that fire emitters, emitter
//! rules that say where and how fast, particle rules that say what each
//! particle does and how it is drawn, and the textures and drawables they use.
//!
//! ⚠ **Every keyframed value is a [`PtxKeyframeProp`] embedded in its owner,
//! and the owner lists them again as pointers at its own members.** The list
//! is a run of [`Member`], answered inside the record holding it; for a
//! behaviour that record is the variant, not the base declaring the list.
//!
//! ⚠ **A rule is found by name hash in its dictionary.** A spawner's effect
//! rule and an event's emitter and particle rules are held by key where
//! another pointer owns them; the first pointer the file reaches owns it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::drawable::{check_drawable, Aliasing, Broken as DrawableBroken, Buffers};
use super::drawable::{DrawableTarget, RmcDrawable};
use super::shared::{DatBase, PgBase, PgBaseRefCounted, PgDictionary, Vec3V, Vec4V};
use crate::decl::*;
use crate::Generation;

impl Resource for PtxFxList<Gen8> {
    const EXT: &'static str = "ypt";
    const TARGET: Generation = Generation::Gen8;
}

#[cfg(feature = "gen9")]
impl Resource for PtxFxList<Gen9> {
    const EXT: &'static str = "ypt";
    const TARGET: Generation = Generation::Gen9;
}

impl PtxFxList {
    /// The Legacy particle list at `path`; an Enhanced one is refused by name, pointing at `or_io::open`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, crate::Error> {
        <Self as File>::open(path)
    }
}

/// `ptxKeyframeEntry`: when, and what, four lanes each.
#[record(size = 0x20)]
pub struct PtxKeyframeEntry {
    pub time: Vec4V,
    pub value: Vec4V,
}

/// `ptxKeyframe`: a run of keys.
#[record(size = 0x20)]
pub struct PtxKeyframe {
    pub entries: Vec<PtxKeyframeEntry>,
    #[note("a runtime pointer, carried")]
    widget: u64,
    #[note("a runtime pointer, carried")]
    defn: u64,
}

/// `ptxKeyframeProp`: a value that varies over a particle's or an effect's life.
#[record(size = 0x90)]
pub struct PtxKeyframeProp {
    #[note("a runtime address, carried")]
    vft: u64,
    #[note("a runtime pointer per thread, carried")]
    evolved_keyframe_prop: [u64; 12],
    pub property_id: Hash32,
    pub invert_bias_link: Bool8,
    #[pad_after(2)]
    pub rand_index: i8,
    pub keyframe: PtxKeyframe,
}

/// `ptxEvolvedKeyframe`: a keyframe evolved by one evolution, and whether it is the LOD one.
#[record(size = 0x30)]
pub struct PtxEvolvedKeyframe {
    pub keyframe: PtxKeyframe,
    pub evolution_idx: i32,
    pub is_lod_evolution: Bool8,
    pad: [u8; 11],
}

/// `ptxEvolvedKeyframeProp`: every evolved keyframe of one keyframed value, and how they blend.
#[record(size = 0x18)]
pub struct PtxEvolvedKeyframeProp {
    pub evolved_keyframes: Vec<PtxEvolvedKeyframe>,
    pub property_id: Hash32,
    pub blend_mode: u8,
    pad: [u8; 3],
}

/// One pair of an [`AtStringMap`]: a keyframed value's property id and its evolved keyframes.
#[record(size = 0x10)]
pub struct AtBinaryMapDataPair {
    #[pad_after(4)]
    pub key: u32,
    /// ⚠ An element of the list's `evolved_keyframe_props`.
    pub data: Ref<Box<PtxEvolvedKeyframeProp>>,
}

/// `atStringMap<datRef<ptxEvolvedKeyframeProp>>`: pairs sorted by key, so a lookup is a binary search.
#[record(size = 0x18)]
pub struct AtStringMap {
    pub sorted: Bool8,
    #[pad_after(4)]
    pad: [u8; 3],
    pub data: Vec<AtBinaryMapDataPair>,
}

/// `ptxEvolution`: a named slider an effect's values vary with.
#[record(size = 0x18)]
pub struct PtxEvolution {
    pub name: String,
    pub name_hash: Hash32,
    pub override_value: f32,
    pub is_overriden: Bool8,
    #[pad_after(4)]
    pad: [u8; 3],
}

/// `ptxEvolutionList`: the evolutions an effect or an event has, and what each varies.
#[record(size = 0x40)]
pub struct PtxEvolutionList {
    pub evolutions: Vec<PtxEvolution>,
    pub evolved_keyframe_props: Vec<PtxEvolvedKeyframeProp>,
    /// ⚠ The engine's list ends here; a resource allocates it to 0x40.
    #[pad_after(8)]
    pub evolved_keyframe_prop_map: AtStringMap,
}

/// `ptxSpawnedEffectScalars`: what an effect a particle spawns is scaled by, at one end of a range.
#[record(size = 0x20)]
pub struct PtxSpawnedEffectScalars {
    pub duration_scalar: f32,
    pub playback_rate_scalar: f32,
    pub colour_tint_scalar: u32,
    pub zoom_scalar: f32,
    pub flags: u32,
    pad: [u8; 12],
}

/// `ptxEffectSpawner`: an effect a particle starts, at a point in its life or when it collides.
#[record(size = 0x70)]
pub struct PtxEffectSpawner<T: DrawableTarget> {
    pub base: PgBase,
    pad: [u8; 8],
    pub spawned_effect_scalars_min: PtxSpawnedEffectScalars,
    pub spawned_effect_scalars_max: PtxSpawnedEffectScalars,
    /// The rule `name` names, found by key in the list's effect rules.
    pub effect_rule: OwnedOrKeyed<PtxEffectRule<T>>,
    pub name: String,
    pub trigger_info: f32,
    pub inherits_point_life: Bool8,
    pub tracks_point_pos: Bool8,
    pub tracks_point_dir: Bool8,
    pub tracks_point_neg_dir: Bool8,
}

/// `ptxRenderState`: how a particle rule's sprites are blended and tested.
#[record(size = 0x14)]
pub struct PtxRenderState {
    pub cull_mode: i32,
    pub blend_set: i32,
    pub lighting_mode: i32,
    pub depth_write: Bool8,
    pub depth_test: Bool8,
    pub alpha_blend: Bool8,
    pad: [u8; 5],
}

/// `ptxBiasLink`: keyframed values that share one random draw, and a name for the group.
#[record(size = 0x58)]
pub struct PtxBiasLink {
    /// NUL-padded ASCII.
    pub name: [u8; 64],
    pub keyframe_prop_ids: Vec<Hash32>,
    pub rand_index: u8,
    #[pad_after(4)]
    pad: [u8; 3],
}

/// `ptxTechniqueDesc`: which technique of a particle shader draws a rule.
#[record(size = 0x18)]
pub struct PtxTechniqueDesc {
    #[note("a runtime address, carried")]
    vft: u64,
    pub diffuse_mode: u32,
    pub proj_mode: u32,
    pub is_lit: Bool8,
    pub is_soft: Bool8,
    pub is_screen_space: Bool8,
    pub is_refract: Bool8,
    pub is_normal_spec: Bool8,
    pad: [u8; 3],
}

/// `ptxShaderVar`: one shader variable a particle rule sets; `type` selects its layout.
#[record(
    size = 0x28,
    tag = "type",
    variant(0 => PtxShaderVarVector<T>),
    variant(1 => PtxShaderVarVector<T>),
    variant(2 => PtxShaderVarVector<T>),
    variant(3 => PtxShaderVarVector<T>),
    variant(4 => PtxShaderVarVector<T>),
    variant(5 => PtxShaderVarVector<T>),
    variant(6 => PtxShaderVarTexture<T>),
    variant(7 => PtxShaderVarKeyframe<T>)
)]
pub struct PtxShaderVar<T: DrawableTarget> {
    pub base: DatBase,
    #[note("a runtime pointer, carried")]
    info: u64,
    pub hash_name: Hash32,
    pub r#type: u32,
    pub id: u32,
    pub is_keyframeable: Bool8,
    pub owns_info: Bool8,
    pad: [u8; 10],
}

/// `ptxShaderVarVector`: a constant of up to four lanes.
#[record(size = 0x40)]
pub struct PtxShaderVarVector<T: DrawableTarget> {
    #[pad_after(8)]
    pub base: PtxShaderVar<T>,
    pub vector: Vec4V,
}

/// `ptxShaderVarTexture`: a texture of the list's own dictionary, and its name.
#[record(size = 0x40)]
pub struct PtxShaderVarTexture<T: DrawableTarget> {
    pub base: PtxShaderVar<T>,
    pub texture: Keyed<T::Texture>,
    pub texture_name: String,
    pub texture_hash_name: Hash32,
    pub external_reference: Bool8,
    pad: [u8; 3],
}

/// `ptxShaderVarKeyframe`: a value keyed over the particle's life.
#[record(size = 0x50)]
pub struct PtxShaderVarKeyframe<T: DrawableTarget> {
    pub base: PtxShaderVar<T>,
    #[pad_after(8)]
    pub keyframe: PtxKeyframe,
}

/// `ptxShaderInst`: the shader a particle rule is drawn with, and the variables it sets.
#[record(size = 0x60)]
pub struct PtxShaderInst<T: DrawableTarget> {
    #[note("a runtime address, carried")]
    vft: u64,
    pub shader_template_name: String,
    pub shader_template_technique_name: String,
    #[note("a runtime pointer, carried")]
    shader_template: u64,
    #[pad_after(4)]
    pub shader_template_technique_id: u32,
    pub technique_desc: PtxTechniqueDesc,
    pub inst_vars: Vec<Option<Box<PtxShaderVarAny<T>>>>,
    pub is_data_in_sync: Bool8,
    pad: [u8; 7],
    #[pad_after(4)]
    pub shader_template_hash_name: Hash32,
}

/// `ptxDrawable`: a model a particle rule draws, with the extent it is culled by.
#[record(size = 0x30)]
pub struct PtxDrawable<T: DrawableTarget> {
    /// Width, height, depth and the bounding sphere's radius.
    pub bound_info: Vec4V,
    pub name: String,
    /// An entry of the list's drawable dictionary.
    pub drawable: OwnedOrKeyed<RmcDrawable<T>>,
    pub hash_name: Hash32,
    #[pad_after(8)]
    pad: [u8; 4],
}

/// `ptxParticleRule`: what each particle does and how it is drawn.
///
/// ⚠ The six behaviour lists point at the same behaviours: `all_behaviours`
/// owns each, and the others name it there.
#[record(size = 0x240)]
pub struct PtxParticleRule<T: DrawableTarget> {
    pub base: PgBaseRefCounted,
    #[note("a runtime pointer, carried")]
    ui_data: u64,
    pub effect_spawner_at_ratio: PtxEffectSpawner<T>,
    pub effect_spawner_on_coln: PtxEffectSpawner<T>,
    pub render_state: PtxRenderState,
    pub file_version: f32,
    pub tex_frame_id_min: i32,
    pub tex_frame_id_max: i32,
    pub name: String,
    pub all_behaviours: Vec<Ref<Box<PtxBehaviourAny>>>,
    pub init_behaviours: Vec<Ref<Box<PtxBehaviourAny>>>,
    pub update_behaviours: Vec<Ref<Box<PtxBehaviourAny>>>,
    pub update_finalize_behaviours: Vec<Ref<Box<PtxBehaviourAny>>>,
    pub draw_behaviours: Vec<Ref<Box<PtxBehaviourAny>>>,
    pub release_behaviours: Vec<Ref<Box<PtxBehaviourAny>>>,
    pub bias_links: Vec<PtxBiasLink>,
    #[note("a runtime pointer, carried")]
    point_pool: u64,
    #[note("a run nothing follows, carried as its two words")]
    func_table_notused: [u64; 2],
    pub shader_inst: PtxShaderInst<T>,
    pub drawables: Vec<PtxDrawable<T>>,
    pub sort_type: u8,
    pub draw_type: u8,
    pub flags: u8,
    #[note("runtime flags, carried")]
    #[pad_after(4)]
    runtime_flags: u8,
    #[note("a runtime pointer, carried")]
    last_evo_list_notused: u64,
    #[note("a runtime pointer, carried")]
    wind_behaviour: u64,
    pad: [u8; 8],
}

/// `ptxDomain`: a region an emitter spawns in, aims at or attracts to; `shape` says which.
#[record(size = 0x280)]
pub struct PtxDomain {
    pub base: DatBase,
    /// Which of the emitter's three domains this is.
    pub r#type: i32,
    pub shape: i32,
    pub is_world_space: Bool8,
    pub is_point_relative: Bool8,
    pub is_creation_relative: Bool8,
    #[pad_after(4)]
    pub is_target_relative: Bool8,
    pub position_kfp: PtxKeyframeProp,
    pub rotation_kfp: PtxKeyframeProp,
    pub size_outer_kfp: PtxKeyframeProp,
    pub size_inner_kfp: PtxKeyframeProp,
    #[pad_after(4)]
    pub file_version: f32,
    pub keyframe_prop_list: Vec<Member<PtxKeyframeProp>>,
    #[pad_after(12)]
    pad: [u8; 4],
}

/// `ptxDomainObj`: an emitter rule's hold on one domain.
#[record(size = 0x10)]
pub struct PtxDomainObj {
    pub domain: Option<Box<PtxDomain>>,
    #[pad_after(4)]
    pub new_domain_shape: i32,
}

/// `ptxEmitterRule`: where particles are spawned, how many and how fast.
#[record(size = 0x630)]
pub struct PtxEmitterRule {
    pub base: PgBaseRefCounted,
    #[pad_after(4)]
    pub file_version: f32,
    pub name: String,
    #[note("a runtime pointer, carried")]
    last_evo_list_notused: u64,
    #[note("a runtime pointer, carried")]
    ui_data: u64,
    pub creation_domain_obj: PtxDomainObj,
    pub target_domain_obj: PtxDomainObj,
    pub attractor_domain_obj: PtxDomainObj,
    #[pad_after(4)]
    pad: [u8; 12],
    pub spawn_rate_over_time_kfp: PtxKeyframeProp,
    pub spawn_rate_over_dist_kfp: PtxKeyframeProp,
    pub particle_life_kfp: PtxKeyframeProp,
    pub playback_rate_scalar_kfp: PtxKeyframeProp,
    pub speed_scalar_kfp: PtxKeyframeProp,
    pub size_scalar_kfp: PtxKeyframeProp,
    pub accn_scalar_kfp: PtxKeyframeProp,
    pub dampening_scalar_kfp: PtxKeyframeProp,
    pub matrix_weight_scalar_kfp: PtxKeyframeProp,
    pub inherit_velocity_kfp: PtxKeyframeProp,
    pub keyframe_prop_list: Vec<Member<PtxKeyframeProp>>,
    pub is_one_shot: Bool8,
    pad1: [u8; 7],
}

/// `ptxEvent`: what every event of an effect's timeline starts with.
#[record(size = 0x30)]
pub struct PtxEvent {
    #[note("a runtime address, carried")]
    vft: u64,
    pub index: i32,
    pub r#type: u32,
    pub start_ratio: f32,
    pub end_ratio: f32,
    pub evolution_list: Option<Box<PtxEvolutionList>>,
    #[note("a runtime value, carried")]
    #[pad_after(4)]
    dist_sqr_to_camera: f32,
    #[note("a runtime pointer, carried")]
    ui_data: u64,
}

/// `ptxEventEmitter`: one emitter an effect fires over part of its duration, with the particle rule its particles follow.
#[record(size = 0x70)]
pub struct PtxEventEmitter<T: DrawableTarget> {
    pub base: PtxEvent,
    pub emitter_rule_name: String,
    pub particle_rule_name: String,
    /// The rule `emitter_rule_name` names, owned here where this is the first event to reach it.
    pub emitter_rule: OwnedOrKeyed<PtxEmitterRule>,
    /// The rule `particle_rule_name` names, found by key in the list's particle rules.
    pub particle_rule: OwnedOrKeyed<PtxParticleRule<T>>,
    pub playback_rate_scalar_min: f32,
    pub playback_rate_scalar_max: f32,
    pub zoom_scalar_min: f32,
    pub zoom_scalar_max: f32,
    pub colour_tint_min: u32,
    /// ⚠ The engine's event ends here; a resource allocates it to 0x70.
    #[pad_after(8)]
    pub colour_tint_max: u32,
}

/// `ptxTimeLine`: the events an effect fires.
#[record(size = 0x18)]
pub struct PtxTimeLine<T: DrawableTarget> {
    #[note("a runtime address, carried")]
    vft: u64,
    pub events: Vec<Option<Box<PtxEventEmitter<T>>>>,
}

/// `ptxEffectRule`: an effect — when its emitters fire, how it loops and when it is culled.
#[record(size = 0x3c0)]
pub struct PtxEffectRule<T: DrawableTarget> {
    pub base: PgBaseRefCounted,
    #[pad_after(4)]
    pub file_version: f32,
    pub name: String,
    /// The list holding the rule: its root.
    fx_list: Back<PtxFxList<T>>,
    pub timeline: PtxTimeLine<T>,
    pub evolution_list: Option<Box<PtxEvolutionList>>,
    pub num_loops: i32,
    pub sort_events_by_distance: Bool8,
    pub draw_list_id: u8,
    pub is_short_lived: Bool8,
    #[pad_after(8)]
    pub has_no_shadows: Bool8,
    pub rand_offset_pos: Vec3V,
    pub pre_update_time: f32,
    pub pre_update_time_interval: f32,
    pub duration_min: f32,
    pub duration_max: f32,
    pub playback_rate_scalar_min: f32,
    pub playback_rate_scalar_max: f32,
    pub viewport_culling_mode: u8,
    pub render_when_viewport_culled: Bool8,
    pub update_when_viewport_culled: Bool8,
    pub emit_when_viewport_culled: Bool8,
    pub distance_culling_mode: u8,
    pub render_when_distance_culled: Bool8,
    pub update_when_distance_culled: Bool8,
    pub emit_when_distance_culled: Bool8,
    pub viewport_culling_sphere_offset: Vec3V,
    pub viewport_culling_sphere_radius: f32,
    pub distance_culling_fade_dist: f32,
    pub distance_culling_cull_dist: f32,
    pub lod_evo_dist_min: f32,
    pub lod_evo_dist_max: f32,
    pub coln_range: f32,
    pub coln_probe_dist: f32,
    pub coln_type: u8,
    pub share_entity_coln: Bool8,
    pub only_use_bvh_coln: Bool8,
    pub game_flags: u8,
    pub colour_tint_min_kfp: PtxKeyframeProp,
    pub colour_tint_max_kfp: PtxKeyframeProp,
    pub zoom_scalar_kfp: PtxKeyframeProp,
    pub data_sphere_kfp: PtxKeyframeProp,
    pub data_capsule_kfp: PtxKeyframeProp,
    pub keyframe_prop_list: Vec<Member<PtxKeyframeProp>>,
    pub colour_tint_max_enable: Bool8,
    pub use_data_volume: Bool8,
    pub data_volume_type: u8,
    pad3: [u8; 1],
    pub num_active_instances: u32,
    #[pad_after(4)]
    pub zoom_level: f32,
    #[note("a runtime pointer, carried")]
    ui_data: u64,
    /// ⚠ The engine's rule runs four bytes past this; a resource allocates it to 0x3c0.
    pad4: [u8; 8],
}

/// `ptxFxList`: a `.ypt`'s root — every rule by name hash, and what they draw with.
#[record(size = 0x60)]
pub struct PtxFxList<T: DrawableTarget = Gen8> {
    pub base: PgBase,
    pub name: String,
    #[init(1)]
    #[pad_after(4)]
    ref_count: u32,
    pub texture_dict: Option<Box<PgDictionary<T::Texture>>>,
    #[pad_after(4)]
    texture_dict_pusher: [u8; 4],
    pub model_dict: Option<Box<PgDictionary<RmcDrawable<T>>>>,
    pub particle_rule_dict: Option<Box<PgDictionary<PtxParticleRule<T>>>>,
    #[pad_after(4)]
    texture_dict_popper: [u8; 4],
    pub effect_rule_dict: Option<Box<PgDictionary<PtxEffectRule<T>>>>,
    /// ⚠ The engine's list ends here; a resource allocates it to 0x60.
    #[pad_after(8)]
    pub emitter_rule_dict: Option<Box<PgDictionary<PtxEmitterRule>>>,
}

/// `ptxBehaviour`: what every behaviour starts with; `hash_name`, the class name's hash, selects the rest.
#[record(
    size = 0x30,
    tag = "hash_name",
    variant(0xF5B33BAA => PtxuAge),
    variant(0xD63D9F1B => PtxuAcceleration),
    variant(0x6C0719BC => PtxuVelocity),
    variant(0x1EE64552 => PtxuRotation),
    variant(0x38B60240 => PtxuSize),
    variant(0x052B1293 => PtxuDampening),
    variant(0x64E5D702 => PtxuMatrixWeight),
    variant(0x928A1C45 => PtxuCollision),
    variant(0xECA84C1E => PtxuAnimateTexture),
    variant(0x164AEA72 => PtxuColour),
    variant(0x68FA73F5 => PtxdSprite),
    variant(0x38B63978 => PtxuWind),
    variant(0x0544C710 => PtxuLight),
    variant(0x6232E25A => PtxdModel),
    variant(0x8F3B6036 => PtxuDecal),
    variant(0xA35C721F => PtxuZCull),
    variant(0xB77FED19 => PtxuNoise),
    variant(0x25AC9437 => PtxuAttractor),
    variant(0xC57377F8 => PtxdTrail),
    variant(0xA05DA63E => PtxuFogVolume),
    variant(0xD4594BEF => PtxuRiver),
    variant(0xA2D6DC3F => PtxuDecalPool),
    variant(0xDF229542 => PtxuLiquid),
    variant(0x843768CF => PtxuCameraEvolution)
)]
pub struct PtxBehaviour {
    #[note("a runtime address, carried")]
    vft: u64,
    #[pad_after(4)]
    pub hash_name: u32,
    /// Points at members of the variant, not of this base.
    pub keyframe_prop_list: Vec<Member<PtxKeyframeProp>>,
    #[note("a runtime count, carried")]
    accum_cpu_update_ticks: u32,
    pad: [u8; 12],
}

/// `ptxu_Age`: ages each particle; nothing past the base.
#[record(size = 0x30)]
pub struct PtxuAge {
    pub base: PtxBehaviour,
}

/// `ptxu_Velocity`: integrates each particle's velocity; nothing past the base.
#[record(size = 0x30)]
pub struct PtxuVelocity {
    pub base: PtxBehaviour,
}

/// `ptxu_Acceleration`: acceleration in a range, optionally with gravity.
#[record(size = 0x170)]
pub struct PtxuAcceleration {
    pub base: PtxBehaviour,
    pub xyz_min_kfp: PtxKeyframeProp,
    pub xyz_max_kfp: PtxKeyframeProp,
    #[note("a runtime pointer, carried")]
    global_data_notused: u64,
    pub reference_space: i32,
    pub is_affected_by_zoom: Bool8,
    pub enable_gravity: Bool8,
    #[pad_after(12)]
    pad: [u8; 6],
}

/// `ptxu_Rotation`: initial and per-frame angle, each in a range.
#[record(size = 0x280)]
pub struct PtxuRotation {
    pub base: PtxBehaviour,
    pub initial_angle_min_kfp: PtxKeyframeProp,
    pub initial_angle_max_kfp: PtxKeyframeProp,
    pub angle_min_kfp: PtxKeyframeProp,
    pub angle_max_kfp: PtxKeyframeProp,
    pub init_rotation_mode: i32,
    pub update_rotation_mode: i32,
    pub accumulate_angle: Bool8,
    pub rotate_angle_axes: Bool8,
    pub rotate_init_angle_axes: Bool8,
    pad: [u8; 1],
    pub speed_fade_thresh: f32,
}

/// `ptxu_Size`: width, height and depth in a range, and edge scalars.
#[record(size = 0x280)]
pub struct PtxuSize {
    pub base: PtxBehaviour,
    pub whd_min_kfp: PtxKeyframeProp,
    pub whd_max_kfp: PtxKeyframeProp,
    pub tblr_scalar_kfp: PtxKeyframeProp,
    pub tblr_vel_scalar_kfp: PtxKeyframeProp,
    pub keyframe_mode: i32,
    pub is_proportional: Bool8,
    pad: [u8; 11],
}

/// `ptxu_Dampening`: dampening in a range, optionally by air resistance.
#[record(size = 0x170)]
pub struct PtxuDampening {
    pub base: PtxBehaviour,
    pub xyz_min_kfp: PtxKeyframeProp,
    pub xyz_max_kfp: PtxKeyframeProp,
    #[note("a runtime pointer, carried")]
    global_data_notused: u64,
    pub reference_space: i32,
    pub enable_air_resistance: Bool8,
    #[pad_after(12)]
    pad: [u8; 7],
}

/// `ptxu_MatrixWeight`: how far each particle follows the effect's transform.
#[record(size = 0xd0)]
pub struct PtxuMatrixWeight {
    pub base: PtxBehaviour,
    pub mtx_weight_kfp: PtxKeyframeProp,
    pub reference_space: i32,
    pad: [u8; 12],
}

/// `ptxu_Collision`: how particles bounce off, or die on, what they hit.
#[record(size = 0x170)]
pub struct PtxuCollision {
    pub base: PtxBehaviour,
    pub bounciness_kfp: PtxKeyframeProp,
    pub bounce_dir_var_kfp: PtxKeyframeProp,
    pub radius_mult: f32,
    pub rest_speed: f32,
    pub coln_chance: i32,
    pub kill_chance: i32,
    pub debug_draw: Bool8,
    pad: [u8; 3],
    pub override_min_radius: f32,
    pad2: [u8; 8],
}

/// `ptxu_AnimateTexture`: frame-by-frame texture animation.
#[record(size = 0xd0)]
pub struct PtxuAnimateTexture {
    pub base: PtxBehaviour,
    pub anim_rate_kfp: PtxKeyframeProp,
    pub keyframe_mode: i32,
    pub last_frame_id: i32,
    pub loop_mode: i32,
    pub is_randomised: Bool8,
    pub is_scaled_over_particle_life: Bool8,
    pub is_held_on_last_frame: Bool8,
    pub do_frame_blending: Bool8,
}

/// `ptxu_Colour`: colour in a range, and emissive intensity.
#[record(size = 0x1f0)]
pub struct PtxuColour {
    pub base: PtxBehaviour,
    pub rgba_min_kfp: PtxKeyframeProp,
    pub rgba_max_kfp: PtxKeyframeProp,
    pub emissive_intensity_kfp: PtxKeyframeProp,
    pub keyframe_mode: i32,
    pub rgba_max_enable: Bool8,
    pub rgba_proportional: Bool8,
    pub rgb_can_tint: Bool8,
    pad: [u8; 9],
}

/// `ptxd_Sprite`: draws each particle as a sprite.
#[record(size = 0x70)]
pub struct PtxdSprite {
    pub base: PtxBehaviour,
    pub align_axis: Vec3V,
    pub alignment_mode: i32,
    pub flip_chance_u: f32,
    pub flip_chance_v: f32,
    pub near_clip_dist: f32,
    pub far_clip_dist: f32,
    pub projection_depth: f32,
    pub shadow_cast_intensity: f32,
    pub is_screen_space: Bool8,
    pub is_hi_res: Bool8,
    pub near_clip: Bool8,
    pub far_clip: Bool8,
    pub uv_clip: Bool8,
    pub disable_draw: Bool8,
    pad: [u8; 14],
}

/// `ptxu_Wind`: how wind moves particles near and far.
#[record(size = 0xf0)]
pub struct PtxuWind {
    pub base: PtxBehaviour,
    pub influence_kfp: PtxKeyframeProp,
    #[note("a runtime pointer, carried")]
    global_data_notused: u64,
    #[note("a runtime pointer, carried")]
    wind_eval_notused: u64,
    pub high_lod_range: f32,
    pub low_lod_range: f32,
    pub high_lod_disturbance_mode: i32,
    pub low_lod_disturbance_mode: i32,
    pub ignore_mtx_weight: Bool8,
    #[pad_after(8)]
    pad: [u8; 7],
}

/// `ptxu_Light`: a light each particle casts, and its corona.
#[record(size = 0x550)]
pub struct PtxuLight {
    pub base: PtxBehaviour,
    pub rgb_min_kfp: PtxKeyframeProp,
    pub rgb_max_kfp: PtxKeyframeProp,
    pub intensity_kfp: PtxKeyframeProp,
    pub range_kfp: PtxKeyframeProp,
    pub corona_rgb_min_kfp: PtxKeyframeProp,
    pub corona_rgb_max_kfp: PtxKeyframeProp,
    pub corona_intensity_kfp: PtxKeyframeProp,
    pub corona_size_kfp: PtxKeyframeProp,
    pub corona_flare_kfp: PtxKeyframeProp,
    pub corona_z_bias: f32,
    pub corona_use_light_col: Bool8,
    pub colour_from_particle: Bool8,
    pub colour_per_frame: Bool8,
    pub intensity_per_frame: Bool8,
    pub range_per_frame: Bool8,
    pub casts_shadows: Bool8,
    pub corona_not_in_reflection: Bool8,
    pub corona_only_in_reflection: Bool8,
    pub light_type: i32,
}

/// `ptxd_Model`: draws each particle as one of the rule's drawables.
#[record(size = 0x40)]
pub struct PtxdModel {
    pub base: PtxBehaviour,
    pub colour_control_shader_id: u32,
    pub camera_shrink: f32,
    pub shadow_cast_intensity: f32,
    pub disable_draw: Bool8,
    pad: [u8; 3],
}

/// `ptxu_Decal`: a decal each particle leaves where it lands.
#[record(size = 0x180)]
pub struct PtxuDecal {
    pub base: PtxBehaviour,
    pub dimensions_kfp: PtxKeyframeProp,
    pub alpha_kfp: PtxKeyframeProp,
    pub decal_id: i32,
    pub velocity_thresh: f32,
    pub total_life: f32,
    pub fade_in_time: f32,
    pub uv_mult_start: f32,
    pub uv_mult_end: f32,
    pub uv_mult_time: f32,
    pub duplicate_reject_dist: f32,
    pub flip_u: Bool8,
    pub flip_v: Bool8,
    pub proportional_size: Bool8,
    pub use_complex_coln: Bool8,
    pub projection_depth: f32,
    pub distance_scale: f32,
    pub is_directional: Bool8,
    pad: [u8; 3],
}

/// `ptxu_ZCull`: culls particles below a height or fades them by distance.
#[record(size = 0x170)]
pub struct PtxuZCull {
    pub base: PtxBehaviour,
    pub height_kfp: PtxKeyframeProp,
    pub fade_dist_kfp: PtxKeyframeProp,
    #[note("a runtime pointer, carried")]
    global_data_notused: u64,
    pub mode: i32,
    pub reference_space: i32,
    #[pad_after(12)]
    pad: [u8; 4],
}

/// `ptxu_Noise`: position and velocity noise, each in a range.
#[record(size = 0x280)]
pub struct PtxuNoise {
    pub base: PtxBehaviour,
    pub pos_noise_min_kfp: PtxKeyframeProp,
    pub pos_noise_max_kfp: PtxKeyframeProp,
    pub vel_noise_min_kfp: PtxKeyframeProp,
    pub vel_noise_max_kfp: PtxKeyframeProp,
    pub reference_space: i32,
    pub keep_constant_speed: Bool8,
    pad: [u8; 11],
}

/// `ptxu_Attractor`: pulls particles toward the emitter's attractor domain.
#[record(size = 0xc0)]
pub struct PtxuAttractor {
    pub base: PtxBehaviour,
    pub strength_kfp: PtxKeyframeProp,
}

/// `ptxd_Trail`: draws the particles of an emitter as one ribbon.
///
/// ⚠ Every file shipped holds the keyframed value before the axis.
#[record(size = 0xf0)]
pub struct PtxdTrail {
    pub base: PtxBehaviour,
    pub tex_info_kfp: PtxKeyframeProp,
    pub align_axis: Vec3V,
    pub alignment_mode: i32,
    pub tessellation_u: i32,
    pub tessellation_v: i32,
    pub smoothness_x: f32,
    pub smoothness_y: f32,
    pub projection_depth: f32,
    pub shadow_cast_intensity: f32,
    pub flip_u: Bool8,
    pub flip_v: Bool8,
    pub wrap_texture_over_particle_life: Bool8,
    pub disable_draw: Bool8,
}

/// `ptxu_FogVolume`: draws each particle as a fog volume.
#[record(size = 0x430)]
pub struct PtxuFogVolume {
    pub base: PtxBehaviour,
    pub rgb_tint_min_kfp: PtxKeyframeProp,
    pub rgb_tint_max_kfp: PtxKeyframeProp,
    pub density_range_kfp: PtxKeyframeProp,
    pub scale_min_kfp: PtxKeyframeProp,
    pub scale_max_kfp: PtxKeyframeProp,
    pub rotation_min_kfp: PtxKeyframeProp,
    pub rotation_max_kfp: PtxKeyframeProp,
    pub falloff: f32,
    pub hdr_mult: f32,
    pub lighting_type: i32,
    pub colour_tint_from_particle: Bool8,
    pub sort_with_particles: Bool8,
    pub use_ground_fog_colour: Bool8,
    pub use_effect_evo_values: Bool8,
}

/// `ptxu_River`: carries particles along a river's flow.
#[record(size = 0x40)]
pub struct PtxuRiver {
    pub base: PtxBehaviour,
    pub velocity_mult: f32,
    pub influence: f32,
    pad: [u8; 8],
}

/// `ptxu_DecalPool`: a pool decal each landing particle grows.
#[record(size = 0x50)]
pub struct PtxuDecalPool {
    pub base: PtxBehaviour,
    pub velocity_thresh: f32,
    pub liquid_type: i32,
    pub decal_id: i32,
    pub start_size: f32,
    pub end_size: f32,
    pub growth_rate: f32,
    pad: [u8; 8],
}

/// `ptxu_Liquid`: a pool and a trail each landing particle leaves.
#[record(size = 0x50)]
pub struct PtxuLiquid {
    pub base: PtxBehaviour,
    pub velocity_thresh: f32,
    pub liquid_type: i32,
    pub pool_start_size: f32,
    pub pool_end_size: f32,
    pub pool_growth_rate: f32,
    pub trail_width_min: f32,
    pub trail_width_max: f32,
    pad: [u8; 4],
}

/// `ptxu_CameraEvolution`: an Enhanced behaviour keyed over the camera's angle.
///
/// ⚠ No Legacy file holds one: 129 in 42 retail Enhanced `.ypt`, none in the corpus.
#[record(size = 0xd0)]
pub struct PtxuCameraEvolution {
    pub base: PtxBehaviour,
    pub evolution_curve: PtxKeyframeProp,
    #[note("two words no source names: 1 or 3, then 2, in all 129 shipped")]
    #[pad_after(8)]
    pub settings: [i32; 2],
}

/// Each behaviour class's name hash, which [`PtxBehaviour::hash_name`] holds,
/// and the ids of the keyframed values it lists, each the hash of
/// `class:member`; a class with none lists an empty run.
macro_rules! behaviour_classes {
    ($($class:ident, $kfps:ident, $name:literal, [$($kfp:literal),* $(,)?];)+) => {
        $(
            #[doc = concat!("`", $name, "`'s name hash.")]
            pub const $class: u32 = crate::string_hash($name.as_bytes());
            #[doc = concat!("The ids of `", $name, "`'s keyframed values, in the order it lists them.")]
            pub const $kfps: &[u32] = &[$(crate::string_hash($kfp.as_bytes())),*];
        )+
    };
}

behaviour_classes! {
    BEHAVIOUR_HASH_AGE, KFP_AGE, "ptxu_Age", [];
    BEHAVIOUR_HASH_ACCELERATION, KFP_ACCELERATION, "ptxu_Acceleration", ["ptxu_Acceleration:m_xyzMinKFP", "ptxu_Acceleration:m_xyzMaxKFP"];
    BEHAVIOUR_HASH_VELOCITY, KFP_VELOCITY, "ptxu_Velocity", [];
    BEHAVIOUR_HASH_ROTATION, KFP_ROTATION, "ptxu_Rotation", ["ptxu_Rotation:m_initialAngleMinKFP", "ptxu_Rotation:m_initialAngleMaxKFP", "ptxu_Rotation:m_angleMinKFP", "ptxu_Rotation:m_angleMaxKFP"];
    BEHAVIOUR_HASH_ROTATION_SPEED, KFP_ROTATION_SPEED, "ptxu_RotationRate", [];
    BEHAVIOUR_HASH_SIZE, KFP_SIZE, "ptxu_Size", ["ptxu_Size:m_whdMinKFP", "ptxu_Size:m_whdMaxKFP", "ptxu_Size:m_tblrScalarKFP", "ptxu_Size:m_tblrVelScalarKFP"];
    BEHAVIOUR_HASH_COLOUR, KFP_COLOUR, "ptxu_Colour", ["ptxu_Colour:m_rgbaMinKFP", "ptxu_Colour:m_rgbaMaxKFP", "ptxu_Colour:m_emissiveIntensityKFP"];
    BEHAVIOUR_HASH_ANIMATE_TEXTURE, KFP_ANIMATE_TEXTURE, "ptxu_AnimateTexture", ["ptxu_AnimateTexture:m_animRateKFP"];
    BEHAVIOUR_HASH_DAMPENING, KFP_DAMPENING, "ptxu_Dampening", ["ptxu_Dampening:m_xyzMinKFP", "ptxu_Dampening:m_xyzMaxKFP"];
    BEHAVIOUR_HASH_MATRIX_WEIGHT, KFP_MATRIX_WEIGHT, "ptxu_MatrixWeight", ["ptxu_MatrixWeight:m_mtxWeightKFP"];
    BEHAVIOUR_HASH_COLLISION, KFP_COLLISION, "ptxu_Collision", ["ptxu_Collision:m_bouncinessKFP", "ptxu_Collision:m_bounceDirVarKFP"];
    BEHAVIOUR_HASH_WIND, KFP_WIND, "ptxu_Wind", ["ptxu_Wind:m_influenceKFP"];
    BEHAVIOUR_HASH_NOISE, KFP_NOISE, "ptxu_Noise", ["ptxu_Noise:m_posNoiseMinKFP", "ptxu_Noise:m_posNoiseMaxKFP", "ptxu_Noise:m_velNoiseMinKFP", "ptxu_Noise:m_velNoiseMaxKFP"];
    BEHAVIOUR_HASH_ATTRACTOR, KFP_ATTRACTOR, "ptxu_Attractor", ["ptxu_Acceleration:m_strengthKFP"];
    BEHAVIOUR_HASH_PAUSE, KFP_PAUSE, "ptxu_Pause", [];
    BEHAVIOUR_HASH_FADE_IN, KFP_FADE_IN, "ptxu_FadeIn", [];
    BEHAVIOUR_HASH_FADE_OUT, KFP_FADE_OUT, "ptxu_FadeOut", [];
    BEHAVIOUR_HASH_GRAVITY, KFP_GRAVITY, "ptxu_Gravity", [];
    BEHAVIOUR_HASH_SPRITE, KFP_SPRITE, "ptxd_Sprite", [];
    BEHAVIOUR_HASH_TRAIL, KFP_TRAIL, "ptxd_Trail", ["ptxd_Trail:m_texInfoKFP"];
    BEHAVIOUR_HASH_MODEL, KFP_MODEL, "ptxd_Model", [];
    BEHAVIOUR_HASH_LIGHT, KFP_LIGHT, "ptxu_Light", ["ptxu_Light:m_rgbMinKFP", "ptxu_Light:m_rgbMaxKFP", "ptxu_Light:m_intensityKFP", "ptxu_Light:m_rangeKFP", "ptxu_Light:m_coronaRgbMinKFP", "ptxu_Light:m_coronaRgbMaxKFP", "ptxu_Light:m_coronaIntensityKFP", "ptxu_Light:m_coronaSizeKFP", "ptxu_Light:m_coronaFlareKFP"];
    BEHAVIOUR_HASH_DECAL, KFP_DECAL, "ptxu_Decal", ["ptxu_Decal:m_dimensionsKFP", "ptxu_Decal:m_alphaKFP"];
    BEHAVIOUR_HASH_ZCULL, KFP_ZCULL, "ptxu_ZCull", ["ptxu_ZCull:m_heightKFP", "ptxu_ZCull:m_fadeDistKFP"];
    BEHAVIOUR_HASH_FOG_VOLUME, KFP_FOG_VOLUME, "ptxu_FogVolume", ["ptxu_FogVolume:m_rgbTintMinKFP", "ptxu_FogVolume:m_rgbTintMaxKFP", "ptxu_FogVolume:m_densityRangeKFP", "ptxu_FogVolume:m_scaleMinKFP", "ptxu_FogVolume:m_scaleMaxKFP", "ptxu_FogVolume:m_rotationMinKFP", "ptxu_FogVolume:m_rotationMaxKFP"];
    BEHAVIOUR_HASH_RIVER, KFP_RIVER, "ptxu_River", [];
    BEHAVIOUR_HASH_DECAL_POOL, KFP_DECAL_POOL, "ptxu_DecalPool", [];
    BEHAVIOUR_HASH_LIQUID, KFP_LIQUID, "ptxu_Liquid", [];
}

/// One key of a keyframe as an editor holds it: when, and what, four lanes each.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawKeyframeEntry {
    pub time: [f32; 4],
    pub value: [f32; 4],
}

/// What a keyframed value states besides its keys.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct KfpMeta {
    pub property_id: u32,
    pub rand_index: i8,
    pub invert_bias_link: u8,
}

/// A [`PtxShaderVar`] as an editor holds it, by the layout its type selects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShaderVar {
    Vector {
        name_hash: u32,
        var_type: u32,
        id: u32,
        is_keyframeable: bool,
        owns_info: bool,
        value: [f32; 4],
    },

    Texture {
        name_hash: u32,
        id: u32,
        is_keyframeable: bool,
        owns_info: bool,
        texture_name: String,
        texture_hash_name: u32,
        external_reference: bool,
    },

    Keyframe {
        name_hash: u32,
        id: u32,
        is_keyframeable: bool,
        owns_info: bool,
        entries: Vec<RawKeyframeEntry>,
    },
}

/// The space a behaviour works in; the two water spaces measure heights from the water.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ReferenceSpace(pub u32);

impl ReferenceSpace {
    pub const WORLD: Self = Self(0);
    pub const EFFECT: Self = Self(1);
    pub const EMITTER: Self = Self(2);
    pub const PARTICLE: Self = Self(3);
    pub const CAMERA: Self = Self(4);
    pub const WATER_Z_CAMERA: Self = Self(5);
    pub const WATER_Z_POINT: Self = Self(6);

    pub fn is_water_relative(self) -> bool {
        matches!(self, Self::WATER_Z_CAMERA | Self::WATER_Z_POINT)
    }
}

impl From<u32> for ReferenceSpace {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

/// What a sprite or a trail faces; an unknown value reads as the camera.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum AlignmentMode {
    #[default]
    Camera = 0,
    Velocity = 1,
    World = 2,
    Effect = 3,
    Emitter = 4,
}

impl From<u32> for AlignmentMode {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::Velocity,
            2 => Self::World,
            3 => Self::Effect,
            4 => Self::Emitter,
            _ => Self::Camera,
        }
    }
}

/// Whose life a behaviour's keyframes run over; an unknown value reads as the particle's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum KeyframeMode {
    #[default]
    Point = 0,
    Effect = 1,
    Emitter = 2,
}

impl From<u32> for KeyframeMode {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::Effect,
            2 => Self::Emitter,
            _ => Self::Point,
        }
    }
}

/// What an animated texture does after its last frame; two names share each of the first three values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(transparent)]
pub struct AnimTexLoopMode(pub u32);

impl AnimTexLoopMode {
    pub const ONCE: Self = Self(0);
    pub const SAME: Self = Self(0);
    pub const LOOP: Self = Self(1);
    pub const ZERO: Self = Self(1);
    pub const PING_PONG: Self = Self(2);
    pub const MIN: Self = Self(2);
    pub const MAX: Self = Self(3);
    pub const RAND_MIN_MAX: Self = Self(4);
}

impl From<u32> for AnimTexLoopMode {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

/// An axis by its id: 0, 1, and 2 or above for Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Axis {
    #[default]
    X,
    Y,
    Z,
}

impl Axis {
    pub fn from_index(index: u32) -> Self {
        match index {
            0 => Self::X,
            1 => Self::Y,
            _ => Self::Z,
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

/// A list the game would misread.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error(
        "the {which} dictionary's hashes are not ascending, so a lookup would find the wrong rule"
    )]
    HashesUnsorted { which: &'static str },
    #[error("the {which} dictionary holds {hashes} hashes and {entries} entries")]
    Lengths {
        which: &'static str,
        hashes: usize,
        entries: usize,
    },
    #[error("model {hash:#010x}: {why}")]
    Model { hash: u32, why: DrawableBroken },
    #[error("{0}")]
    Aliased(#[from] Aliasing),
    #[error("{0}")]
    View(#[from] ViewError),
}

fn dictionary<T: Owned>(
    which: &'static str,
    dict: Option<View<'_, Raw<PgDictionary<T>>>>,
) -> Result<(), Broken>
where
    Raw<T>: Record,
{
    let Some(dict) = dict else {
        return Ok(());
    };
    let (codes, entries) = (dict.codes()?, dict.entries()?);
    if codes.len() != entries.len() {
        return Err(Broken::Lengths {
            which,
            hashes: codes.len(),
            entries: entries.len(),
        });
    }
    if !PgDictionary::<T>::codes_are_sorted(&codes) {
        return Err(Broken::HashesUnsorted { which });
    }
    Ok(())
}

/// Each model's mesh, and the buffers every model points at.
fn models<T: DrawableTarget>(
    dict: Option<View<'_, Raw<PgDictionary<RmcDrawable<T>>>>>,
    target: Generation,
    buffers: &mut Buffers,
) -> Result<(), Broken> {
    let Some(dict) = dict else {
        return Ok(());
    };
    let (codes, entries) = (dict.codes()?, dict.entries()?);
    for (i, hash) in codes.iter().enumerate() {
        if let Some(d) = entries.follow(i)? {
            check_drawable(&d, target).map_err(|why| Broken::Model { hash: *hash, why })?;
            buffers.visit(&d)?;
        }
    }
    Ok(())
}

macro_rules! check {
    ($t:ty, $image:expr, $root:expr, $target:expr, $buffers:expr) => {{
        let list = View::<Raw<PtxFxList<$t>>>::at_offset($image, $root)?;
        dictionary::<<$t as DrawableTarget>::Texture>("texture", list.texture_dict()?)?;
        dictionary::<RmcDrawable<$t>>("drawable", list.model_dict()?)?;
        dictionary::<PtxParticleRule<$t>>("particle rule", list.particle_rule_dict()?)?;
        dictionary::<PtxEffectRule<$t>>("effect rule", list.effect_rule_dict()?)?;
        dictionary::<PtxEmitterRule>("emitter rule", list.emitter_rule_dict()?)?;
        models::<$t>(list.model_dict()?, $target, $buffers)
    }};
}

/// Every dictionary's hashes against its entries and their order, each model's mesh, and the buffers the models point at.
pub fn validate(image: Image<'_>, root: usize, target: Generation) -> Result<(), Broken> {
    Ok(buffers(image, root, target)?.refuse()?)
}

/// The buffers every model of a list points at, once the list passes its checks.
pub fn buffers(image: Image<'_>, root: usize, target: Generation) -> Result<Buffers, Broken> {
    let mut buffers = Buffers::default();
    #[cfg(feature = "gen9")]
    if target == Generation::Gen9 {
        check!(Gen9, image, root, target, &mut buffers)?;
        return Ok(buffers);
    }
    check!(Gen8, image, root, target, &mut buffers)?;
    Ok(buffers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kfp_hashes_match_codewalker() {
        assert_eq!(
            KFP_ACCELERATION[0], 0x30e327d4,
            "ptxu_Acceleration:m_xyzMinKFP"
        );
        assert_eq!(
            KFP_ACCELERATION[1], 0x412a554c,
            "ptxu_Acceleration:m_xyzMaxKFP"
        );
        assert_eq!(KFP_SIZE[0], 0x1f641348, "ptxu_Size:m_whdMinKFP");
        assert_eq!(KFP_SIZE[1], 0x3dc78098, "ptxu_Size:m_whdMaxKFP");
        assert_eq!(KFP_SIZE[2], 0xa67a1155, "ptxu_Size:m_tblrScalarKFP");
        assert_eq!(KFP_SIZE[3], 0xd5c0fce5, "ptxu_Size:m_tblrVelScalarKFP");
        assert_eq!(
            KFP_MATRIX_WEIGHT[0], 0xe7af1a2c,
            "ptxu_MatrixWeight:m_mtxWeightKFP"
        );
        assert_eq!(KFP_COLOUR[0], 0x7fae9df8, "ptxu_Colour:m_rgbaMinKFP");
        assert_eq!(KFP_COLOUR[1], 0x60500691, "ptxu_Colour:m_rgbaMaxKFP");
        assert_eq!(
            KFP_COLOUR[2], 0x8306b23a,
            "ptxu_Colour:m_emissiveIntensityKFP"
        );
        assert_eq!(
            KFP_ROTATION[0], 0x1c256ba4,
            "ptxu_Rotation:m_initialAngleMinKFP"
        );
        assert_eq!(
            KFP_ROTATION[1], 0x351ed852,
            "ptxu_Rotation:m_initialAngleMaxKFP"
        );
        assert_eq!(KFP_ROTATION[2], 0xf0274f77, "ptxu_Rotation:m_angleMinKFP");
        assert_eq!(KFP_ROTATION[3], 0x687b4382, "ptxu_Rotation:m_angleMaxKFP");
        assert_eq!(
            KFP_COLLISION[0], 0x61532d47,
            "ptxu_Collision:m_bouncinessKFP"
        );
        assert_eq!(
            KFP_COLLISION[1], 0x686f965f,
            "ptxu_Collision:m_bounceDirVarKFP"
        );
        assert_eq!(
            KFP_ANIMATE_TEXTURE[0], 0x2946e76f,
            "ptxu_AnimateTexture:m_animRateKFP"
        );
        assert_eq!(KFP_DAMPENING[0], 0xd0ef73c5, "ptxu_Dampening:m_xyzMinKFP");
        assert_eq!(KFP_DAMPENING[1], 0x64c7fc25, "ptxu_Dampening:m_xyzMaxKFP");
        assert_eq!(KFP_WIND[0], 0x0aadcbef, "ptxu_Wind:m_influenceKFP");
        assert_eq!(KFP_DECAL[0], 0xfb8eb4e6, "ptxu_Decal:m_dimensionsKFP");
        assert_eq!(KFP_DECAL[1], 0xa7228870, "ptxu_Decal:m_alphaKFP");
        assert_eq!(KFP_LIGHT[0], 0xda8c99a6, "ptxu_Light:m_rgbMinKFP");
        assert_eq!(KFP_LIGHT[1], 0x12bbe65e, "ptxu_Light:m_rgbMaxKFP");
        assert_eq!(KFP_LIGHT[2], 0xef500a62, "ptxu_Light:m_intensityKFP");
        assert_eq!(KFP_LIGHT[3], 0x75990186, "ptxu_Light:m_rangeKFP");
        assert_eq!(KFP_LIGHT[4], 0xe364d5b2, "ptxu_Light:m_coronaRgbMinKFP");
        assert_eq!(KFP_LIGHT[5], 0xf8561886, "ptxu_Light:m_coronaRgbMaxKFP");
        assert_eq!(KFP_LIGHT[6], 0xe2c464a6, "ptxu_Light:m_coronaIntensityKFP");
        assert_eq!(KFP_LIGHT[7], 0xc35aaf9b, "ptxu_Light:m_coronaSizeKFP");
        assert_eq!(KFP_LIGHT[8], 0xb9410926, "ptxu_Light:m_coronaFlareKFP");
        assert_eq!(KFP_ZCULL[0], 0xce9adbfd, "ptxu_ZCull:m_heightKFP");
        assert_eq!(KFP_ZCULL[1], 0xea6afaba, "ptxu_ZCull:m_fadeDistKFP");
        assert_eq!(KFP_NOISE[0], 0x2d0d70b5, "ptxu_Noise:m_posNoiseMinKFP");
        assert_eq!(KFP_NOISE[1], 0xff31aaf3, "ptxu_Noise:m_posNoiseMaxKFP");
        assert_eq!(KFP_NOISE[2], 0xf256e579, "ptxu_Noise:m_velNoiseMinKFP");
        assert_eq!(KFP_NOISE[3], 0x513812a5, "ptxu_Noise:m_velNoiseMaxKFP");
        assert_eq!(
            KFP_ATTRACTOR[0], 0xd1be590a,
            "ptxu_Acceleration:m_strengthKFP"
        );
        assert_eq!(KFP_TRAIL[0], 0x72668c6f, "ptxd_Trail:m_texInfoKFP");
        assert_eq!(
            KFP_FOG_VOLUME[0], 0x3c599207,
            "ptxu_FogVolume:m_rgbTintMinKFP"
        );
        assert_eq!(
            KFP_FOG_VOLUME[1], 0x23f55175,
            "ptxu_FogVolume:m_rgbTintMaxKFP"
        );
        assert_eq!(
            KFP_FOG_VOLUME[2], 0x3ee8e85e,
            "ptxu_FogVolume:m_densityRangeKFP"
        );
        assert_eq!(
            KFP_FOG_VOLUME[3], 0xdafe6982,
            "ptxu_FogVolume:m_scaleMinKFP"
        );
        assert_eq!(
            KFP_FOG_VOLUME[4], 0x5473d2fe,
            "ptxu_FogVolume:m_scaleMaxKFP"
        );
        assert_eq!(
            KFP_FOG_VOLUME[5], 0x9ef3ceec,
            "ptxu_FogVolume:m_rotationMinKFP"
        );
        assert_eq!(
            KFP_FOG_VOLUME[6], 0x570dc9cd,
            "ptxu_FogVolume:m_rotationMaxKFP"
        );
    }

    #[test]
    fn every_behaviour_tag_is_its_class_names_hash() {
        let base = <Raw<PtxBehaviour> as Described>::describe();
        let selector = base.selector.expect("PtxBehaviour is tagged");
        for (tag, variant) in &selector.variants {
            // `PtxuCameraEvolution` is `ptxu_CameraEvolution`; the hash ignores case.
            let class = format!("{}_{}", variant[..4].to_lowercase(), &variant[4..]);
            assert_eq!(
                *tag,
                i64::from(crate::string_hash(class.as_bytes())),
                "{variant}'s tag is not the hash of `{class}`"
            );
        }
    }
}
