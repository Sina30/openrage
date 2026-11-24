//! What the index keeps of an archetype's lights: one fixed row a light, in the archetype's model
//! space, so the lights an entity places stand in the world without the map-types body being read again.

use bytemuck::{Pod, Zeroable};
use or_io::gta6::map::light_ext::{archetype_lights, Light, LightKind};
use or_io::meta::binary::rbm::schema::RbmModel;

/// One light an archetype holds, by the word its entities name the archetype by.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ArchetypeLight {
    pub archetype: u64,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub colour: [f32; 3],
    pub intensity: f32,
    pub radius: f32,
    pub hours: u32,
    pub flags: u32,
    pub cone_inner: f32,
    pub cone_outer: f32,
    pub length: f32,
    /// The `lightType` byte: 1 point, 2 spot, 4 capsule, 32 area.
    pub light_type: u32,
    pub pad: u32,
}

fn type_of(kind: LightKind) -> u32 {
    match kind {
        LightKind::Point => 1,
        LightKind::Spot => 2,
        LightKind::Capsule => 4,
        LightKind::Area => 32,
    }
}

impl ArchetypeLight {
    fn of(archetype: u64, l: &Light) -> Self {
        Self {
            archetype,
            position: l.position,
            direction: l.direction,
            colour: l.colour,
            intensity: l.intensity,
            radius: l.radius,
            hours: l.hours,
            flags: l.flags,
            cone_inner: l.cone_inner,
            cone_outer: l.cone_outer,
            length: l.length,
            light_type: type_of(l.kind),
            pad: 0,
        }
    }

    /// The light the row keeps; `None` for a row of a type no frame draws.
    pub fn light(&self) -> Option<Light> {
        Some(Light {
            kind: LightKind::of(u8::try_from(self.light_type).ok()?)?,
            position: self.position,
            direction: self.direction,
            colour: self.colour,
            intensity: self.intensity,
            radius: self.radius,
            // The frame's lights take no exponent: the level-of-detail copies of these lights state 1.
            falloff_exponent: 1.0,
            hours: self.hours,
            flags: self.flags,
            cone_inner: self.cone_inner,
            cone_outer: self.cone_outer,
            length: self.length,
        })
    }
}

/// The rows of every light the archetypes of the map-types body `model` hold; none for another body, or one whose lights do not read.
pub fn rows_of(model: &RbmModel) -> Vec<ArchetypeLight> {
    match archetype_lights(model) {
        Ok(found) => (found.iter())
            .flat_map(|(word, lights)| lights.iter().map(|l| ArchetypeLight::of(*word, l)))
            .collect(),
        Err(error) => {
            tracing::debug!(%error, "the lights of a map-types body are not read");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_row_gives_back_the_light_it_was_made_of() {
        let light = Light {
            kind: LightKind::Capsule,
            position: [1.0, 2.0, 3.0],
            direction: [0.0, 1.0, 0.0],
            colour: [1.0, 0.5, 0.25],
            intensity: 25.0,
            radius: 8.0,
            falloff_exponent: 1.0,
            hours: 0x00E0_007F,
            flags: 1 << 12,
            cone_inner: 10.0,
            cone_outer: 30.0,
            length: 4.0,
        };
        let row = ArchetypeLight::of(0xA1, &light);
        assert_eq!((row.archetype, row.light_type), (0xA1, 4));
        assert_eq!(row.light(), Some(light));
        assert_eq!(std::mem::size_of::<ArchetypeLight>(), 80);
        let odd = ArchetypeLight {
            light_type: 8,
            ..row
        };
        assert_eq!(odd.light(), None);
    }

    #[test]
    fn a_body_that_is_no_map_types_gives_no_row() {
        let model = super::super::lights_lod::tests::model(&[1.0], &[]);
        assert!(rows_of(&model).is_empty());
    }
}
