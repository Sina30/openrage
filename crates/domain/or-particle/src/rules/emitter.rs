use super::{EvolutionList, ParticleRule};
use crate::domain::{Domain, DomainKeyframeData};
use crate::keyframe::KeyframeData;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmitterRule {
    pub name: String,
    pub particle_rule: Option<ParticleRule>,
    pub spawn_domain: Domain,
    pub velocity_domain: Domain,
    pub attractor_domain: Option<Domain>,
    pub spawn_domain_keyframes: DomainKeyframeData,
    pub velocity_domain_keyframes: DomainKeyframeData,
    pub attractor_domain_keyframes: Option<DomainKeyframeData>,
    pub spawn_rate: f32,
    pub spawn_rate_over_time: Option<KeyframeData>,
    pub spawn_rate_over_dist: f32,
    pub particle_life: f32,
    pub playback_rate_scalar: f32,
    pub speed_scalar: f32,
    pub size_scalar: f32,
    pub acceleration_scalar: f32,
    pub dampening_scalar: f32,
    pub matrix_weight_scalar: f32,
    pub inherit_velocity: f32,
    pub event_start_ratio: f32,
    pub event_end_ratio: f32,
    pub event_evolution_list: Option<EvolutionList>,
    pub event_playback_rate_scalar_min: f32,
    pub event_playback_rate_scalar_max: f32,
    pub event_zoom_scalar_min: f32,
    pub event_zoom_scalar_max: f32,
    pub event_colour_tint_min: u32,
    pub event_colour_tint_max: u32,
    pub is_one_shot: bool,
    pub file_version: f32,
    pub max_particles: u32,
    pub lifetime_min: f32,
    pub lifetime_max: f32,
    pub rotation_min: f32,
    pub rotation_max: f32,
    pub size_min: f32,
    pub size_max: f32,
}

impl EmitterRule {
    pub fn new(name: String) -> Self {
        Self {
            name,
            particle_rule: None,
            spawn_domain: Domain::default(),
            velocity_domain: Domain::default(),
            attractor_domain: None,
            spawn_domain_keyframes: DomainKeyframeData::default(),
            velocity_domain_keyframes: DomainKeyframeData::default(),
            attractor_domain_keyframes: None,
            spawn_rate: 10.0,
            spawn_rate_over_time: None,
            spawn_rate_over_dist: 0.0,
            particle_life: 1.0,
            playback_rate_scalar: 1.0,
            speed_scalar: 1.0,
            size_scalar: 1.0,
            acceleration_scalar: 1.0,
            dampening_scalar: 1.0,
            matrix_weight_scalar: 1.0,
            inherit_velocity: 0.0,
            event_start_ratio: 0.0,
            event_end_ratio: 1.0,
            event_evolution_list: None,
            event_playback_rate_scalar_min: 1.0,
            event_playback_rate_scalar_max: 1.0,
            event_zoom_scalar_min: 1.0,
            event_zoom_scalar_max: 1.0,
            event_colour_tint_min: 0xFFFFFFFF,
            event_colour_tint_max: 0xFFFFFFFF,
            is_one_shot: false,
            file_version: 4.1,
            max_particles: 200,
            lifetime_min: 1.0,
            lifetime_max: 2.0,
            rotation_min: 0.0,
            rotation_max: 360.0,
            size_min: 1.0,
            size_max: 1.0,
        }
    }
}

impl Default for EmitterRule {
    fn default() -> Self {
        Self::new(String::new())
    }
}
