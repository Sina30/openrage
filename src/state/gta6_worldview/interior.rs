//! The contents of the interiors a map places: each entity of an interior and of the entity sets
//! its instance turns on, in the world under the instance, drawn while the camera is within the
//! interior's own distance.
//!
//! ⚠ No visibility pass: the rooms and portals are not read and every child in range draws.
//! A child comes and goes at its distance with no fade.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs::File;

use glam::{Mat4, Quat, Vec3};
use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9ZlibRun};
use or_io::gta6::map::interior::{self, Interior, InteriorInstance};
use or_io::gta6::map::stream::StreamMap;
use or_world::streaming::Level;

use super::index::{BodyRow, ModelRef, WorldIndex};
use super::stream::{rows_of, stretch, Rows};

/// The bytes a body of interiors is read to.
const BODY_LIMIT: u64 = 256 << 20;

/// One entity an interior draws, in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Child {
    pub(super) model: ModelRef,
    pub(super) rows: Rows,
    /// The sphere of its model.
    pub(super) centre: Vec3,
    pub(super) radius: f32,
    /// How far from where it stands it draws.
    pub(super) lod_dist: f32,
    /// Which of the view's scales its distance takes.
    pub(super) level: Level,
    /// The hours it draws in, bit `h` for the hour from `h`: its archetype's.
    pub(super) hours: u32,
}

/// One interior as a map places it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Placed {
    /// The sphere of the interior's box.
    pub(super) centre: Vec3,
    pub(super) radius: f32,
    /// How far from `centre` the camera may be while the contents draw.
    pub(super) lod_dist: f32,
    pub(super) children: Vec<Child>,
}

fn placing(scale_xy: f32, scale_z: f32, rotation: [f32; 4], position: [f32; 3]) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::new(scale_xy, scale_xy, scale_z),
        Quat::from_array(rotation).normalize(),
        Vec3::from(position),
    )
}

/// `interior` as `instance` places it: each entity it draws that the index has a model for.
pub(super) fn place(
    index: &WorldIndex,
    instance: &InteriorInstance,
    interior: &Interior,
) -> Placed {
    let outer = placing(
        instance.scale_xy,
        instance.scale_z,
        instance.rotation,
        instance.position,
    );
    let children = (interior.drawn(&instance.default_entity_sets))
        .filter_map(|(e, _)| {
            let archetype = index.archetype(e.archetype_word)?;
            let model = index.model(archetype)?;
            let m = outer * placing(e.scale_xy, e.scale_z, e.rotation, e.position);
            let rows = rows_of(m);
            Some(Child {
                model,
                rows,
                centre: m.transform_point3(Vec3::from(archetype.centre)),
                radius: archetype.radius * stretch(&rows),
                // An entity that states no distance takes its archetype's.
                lod_dist: if e.lod_dist > 0.0 {
                    e.lod_dist
                } else {
                    archetype.lod_dist
                },
                level: Level::from_raw(e.lod_level),
                hours: archetype.hours,
            })
        })
        .collect();
    let (min, max) = (Vec3::from(interior.bb_min), Vec3::from(interior.bb_max));
    Placed {
        centre: outer.transform_point3((min + max) / 2.0),
        radius: (max - min).length() / 2.0 * stretch(&rows_of(outer)),
        lod_dist: if instance.lod_dist > 0.0 {
            instance.lod_dist
        } else {
            interior.lod_dist
        },
        children,
    }
}

impl Placed {
    /// Call `draw` with each child that draws for a camera at `eye` at the hour `hour`, 0 to 23,
    /// distances scaled by `scales` by level, and how far from the camera it stands.
    pub(super) fn each(
        &self,
        eye: Vec3,
        scales: &[f32; Level::COUNT],
        hour: u32,
        mut draw: impl FnMut(&Child, f32),
    ) {
        // A camera inside the interior's own sphere is in range whatever distance it states.
        let scale = scales[Level::Hd as usize];
        if eye.distance(self.centre) > (self.lod_dist * scale).max(self.radius) {
            return;
        }
        for child in &self.children {
            if (child.hours >> hour) & 1 == 0 {
                continue;
            }
            let r = &child.rows;
            let away = eye.distance(Vec3::new(r[0][3], r[1][3], r[2][3]));
            if away <= child.lod_dist * scales[child.level as usize] {
                draw(child, away);
            }
        }
    }
}

/// The interiors of the map types body `row` names, read through the worker's open archives.
fn interiors_at(
    index: &WorldIndex,
    files: &mut HashMap<u32, File>,
    row: &BodyRow,
) -> Option<Vec<Interior>> {
    let file = match files.entry(row.archive) {
        Entry::Occupied(open) => open.into_mut(),
        Entry::Vacant(slot) => {
            let path = index.archives.get(row.archive as usize)?;
            slot.insert(File::open(path).ok()?)
        }
    };
    let (find, bytes) = read_rpf9_body(&*file, row.offset, BODY_LIMIT).ok()?;
    let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
        resource: Some(resource),
        ..
    }) = find.kind
    else {
        return None;
    };
    let (virtual_section, physical) = resource.sections(&bytes);
    interior::open(virtual_section, physical)
        .inspect_err(|error| tracing::debug!(offset = row.offset, %error, "interiors not read"))
        .ok()
}

/// The interiors the map body of these two sections places, each from its archetype's body, read
/// through `files`; a map that places none costs one look at `map`.
// ponytail: a body of interiors is read again for each map that places one of its interiors; a cache a worker if map loads show it.
pub(super) fn read_placed(
    index: &WorldIndex,
    files: &mut HashMap<u32, File>,
    map: &StreamMap,
    virtual_section: &[u8],
    physical: &[u8],
) -> Vec<Placed> {
    if !map.entities.iter().any(|e| e.interior) {
        return Vec::new();
    }
    let instances = match interior::open_instances(virtual_section, physical) {
        Ok(instances) => instances,
        Err(error) => {
            tracing::debug!(map = map.name, %error, "a map's interior instances were not read");
            return Vec::new();
        }
    };
    let mut bodies: HashMap<(u32, u64), Vec<Interior>> = HashMap::new();
    let mut placed = Vec::with_capacity(instances.len());
    for instance in &instances {
        let Some(row) = index.interior(instance.archetype_word) else {
            continue;
        };
        let body = match bodies.entry((row.archive, row.offset)) {
            Entry::Occupied(read) => read.into_mut(),
            Entry::Vacant(slot) => slot.insert(interiors_at(index, files, row).unwrap_or_default()),
        };
        let stated = (body.iter()).find(|i| i.name_word == instance.archetype_word);
        placed.extend(stated.map(|interior| place(index, instance, interior)));
    }
    placed
}

#[cfg(test)]
#[path = "interior_tests.rs"]
mod tests;
