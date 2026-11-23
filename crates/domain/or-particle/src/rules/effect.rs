use super::EmitterRule;
use crate::domain::Vec3;
use crate::evolution::{Colour, EvolutionList};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParticleEffectsList {
    pub version: u32,
    pub name: String,
    pub effect_rules: Vec<ParticleEffectRule>,
    pub names: Vec<String>,
}

impl ParticleEffectsList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn effect_count(&self) -> usize {
        self.effect_rules.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleEffectRule {
    pub name: String,
    pub duration_min: f32,
    pub duration_max: f32,
    pub loop_count: i32,
    pub cull_radius: f32,
    pub viewport_culling_offset: Vec3,
    pub cull_distance: f32,
    pub lod_near: f32,
    pub lod_far: f32,
    pub lod_evo_dist_min: f32,
    pub lod_evo_dist_max: f32,
    pub collision_type: CollisionType,
    pub collision_range: f32,
    pub collision_probe_distance: f32,
    pub colour_tint_min: Colour,
    pub colour_tint_max: Colour,
    pub rand_offset_pos: Vec3,
    pub draw_list_id: u8,
    pub game_flags: u8,
    pub colour_tint_max_enable: bool,
    pub use_data_volume: bool,
    pub data_volume_type: u8,
    pub sort_events_by_distance: bool,
    pub is_short_lived: bool,
    pub has_no_shadows: bool,
    pub pre_update_time: f32,
    pub pre_update_time_interval: f32,
    pub playback_rate_scalar_min: f32,
    pub playback_rate_scalar_max: f32,
    pub viewport_culling_mode: u8,
    pub render_when_viewport_culled: bool,
    pub update_when_viewport_culled: bool,
    pub emit_when_viewport_culled: bool,
    pub distance_culling_mode: u8,
    pub render_when_distance_culled: bool,
    pub update_when_distance_culled: bool,
    pub emit_when_distance_culled: bool,
    pub distance_culling_fade_dist: f32,
    pub share_entity_coln: bool,
    pub only_use_bvh_coln: bool,
    pub zoom_level: f32,
    pub evolution_list: Option<EvolutionList>,
    pub emitter_rules: Vec<EmitterRule>,
}

impl ParticleEffectRule {
    pub fn new(name: String) -> Self {
        Self {
            name,
            duration_min: 1.0,
            duration_max: 1.0,
            loop_count: 0,
            cull_radius: 10.0,
            viewport_culling_offset: Vec3::zero(),
            cull_distance: 100.0,
            lod_near: 0.0,
            lod_far: 100.0,
            lod_evo_dist_min: 0.0,
            lod_evo_dist_max: 0.0,
            collision_type: CollisionType::None,
            collision_range: 0.0,
            collision_probe_distance: 0.0,
            colour_tint_min: Colour::white(),
            colour_tint_max: Colour::white(),
            rand_offset_pos: Vec3::zero(),
            draw_list_id: 0,
            game_flags: 0,
            colour_tint_max_enable: false,
            use_data_volume: false,
            data_volume_type: 0,
            sort_events_by_distance: false,
            is_short_lived: false,
            has_no_shadows: false,
            pre_update_time: 0.0,
            pre_update_time_interval: 0.25,
            playback_rate_scalar_min: 1.0,
            playback_rate_scalar_max: 1.0,
            viewport_culling_mode: 0,
            render_when_viewport_culled: false,
            update_when_viewport_culled: false,
            emit_when_viewport_culled: false,
            distance_culling_mode: 0,
            render_when_distance_culled: false,
            update_when_distance_culled: false,
            emit_when_distance_culled: false,
            distance_culling_fade_dist: 90.0,
            share_entity_coln: false,
            only_use_bvh_coln: false,
            // The engine's reset value. Authoring metadata only: the shipped
            // runtime ignores it, because the zoom step is compiled into
            // editor builds only.
            zoom_level: 100.0,
            evolution_list: None,
            emitter_rules: Vec::new(),
        }
    }
}

impl Default for ParticleEffectRule {
    fn default() -> Self {
        Self::new(String::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum CollisionType {
    #[default]
    None = 0,
    Sphere = 1,
    Ray = 2,
    World = 3,
}

impl CollisionType {
    pub fn from_id(id: u32) -> Self {
        match id {
            0 => Self::None,
            1 => Self::Sphere,
            2 => Self::Ray,
            3 => Self::World,
            _ => Self::None,
        }
    }

    pub fn to_id(self) -> u32 {
        self as u32
    }
}
