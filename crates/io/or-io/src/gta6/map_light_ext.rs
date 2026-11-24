//! The lights of a GTA VI map body's light extensions as a frame draws them: each light where it
//! stands in the space of what owns it, and an archetype's lights by the word its entities name it by.
//!
//! ⚠ A light stands at `offsetPosition + rootOffsetPosition` (`hash_30ECA388`): `posn` is relative
//! to the bone `boneId` names, and its direction is already in the model's space.

use super::lights::{self, rotate, LightAttr, LightEffect, LightOwner, Pose};
use super::{MapBody, Result};
use crate::meta::binary::rbm::schema::RbmModel;

/// The flag of a light that draws a corona and lights nothing.
pub const CORONA_ONLY: u32 = 1 << 15;

/// The shape a light shines in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightKind {
    Point,
    Spot,
    /// A segment along the light's direction, `length` long.
    Capsule,
    /// Type 32, which a quad-shaped pass draws.
    Area,
}

impl LightKind {
    /// The kind a `lightType` byte names; `None` for any other value.
    pub fn of(light_type: u8) -> Option<Self> {
        match light_type {
            1 => Some(Self::Point),
            2 => Some(Self::Spot),
            4 => Some(Self::Capsule),
            32 => Some(Self::Area),
            _ => None,
        }
    }
}

/// One light as a frame draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub kind: LightKind,
    pub position: [f32; 3],
    /// A unit vector.
    pub direction: [f32; 3],
    /// Red, green and blue, 0 to 1.
    pub colour: [f32; 3],
    pub intensity: f32,
    /// How far it reaches, in metres.
    pub radius: f32,
    pub falloff_exponent: f32,
    /// One bit an hour of the day, bit 0 the first.
    pub hours: u32,
    pub flags: u32,
    /// Half angles of a spot's cone, in degrees.
    pub cone_inner: f32,
    pub cone_outer: f32,
    /// A capsule's length along its direction, in metres.
    pub length: f32,
}

impl Light {
    /// `light` of `effect` in the space of what owns it; `None` for a type no frame draws, or a light that only draws a corona.
    pub fn of(effect: &LightEffect, light: &LightAttr) -> Option<Self> {
        let kind = LightKind::of(light.light_type)?;
        if light.flags & CORONA_ONLY != 0 {
            return None;
        }
        Some(Self {
            kind,
            position: [0, 1, 2].map(|i| effect.offset_position[i] + light.hash_30eca388[i]),
            direction: light.direction,
            colour: light.colour.map(|c| f32::from(c) / 255.0),
            intensity: light.intensity,
            radius: light.falloff,
            falloff_exponent: light.falloff_exponent,
            hours: light.time_flags,
            flags: light.flags,
            cone_inner: light.cone_inner_angle,
            cone_outer: light.cone_outer_angle,
            length: light.extents[0],
        })
    }

    /// This light in the space `pose` stands in: its place scaled, turned and moved, its direction turned.
    pub fn placed(&self, pose: &Pose) -> Self {
        Self {
            position: pose.apply(self.position),
            direction: rotate(pose.rotation, self.direction),
            ..*self
        }
    }

    /// Whether it is lit in the hour `hour` of the day, 0 to 23.
    pub fn lit_at(&self, hour: u32) -> bool {
        hour < 24 && self.hours >> hour & 1 == 1
    }
}

/// The lights of `effect` a frame draws.
pub fn lights_of(effect: &LightEffect) -> impl Iterator<Item = Light> + '_ {
    effect.lights().filter_map(|light| Light::of(effect, light))
}

/// The lights of each archetype of a map-types body that holds any, by the word its entities name it by, in the body's order; none for a map body of entities.
pub fn archetype_lights(model: &RbmModel) -> Result<Vec<(u64, Vec<Light>)>> {
    let MapBody::Types(types) = super::read(model)? else {
        return Ok(Vec::new());
    };
    let read = lights::read(model)?;
    let mut out: Vec<(u64, Vec<Light>)> = Vec::new();
    for (owner, effect) in &read.effects {
        // An interior's own entities stand through the interior's instance, which the interiors place.
        let LightOwner::Archetype(k) = owner else {
            continue;
        };
        let Some(archetype) = types.archetypes.get(*k) else {
            continue;
        };
        let lights: Vec<Light> = lights_of(effect).collect();
        if lights.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some((word, held)) if *word == archetype.name_word => held.extend(lights),
            _ => out.push((archetype.name_word, lights)),
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "map_light_ext_tests.rs"]
mod tests;
