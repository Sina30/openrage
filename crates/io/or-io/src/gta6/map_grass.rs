//! The grass lists of a GTA VI map body as a streamer takes them: each list's archetype, box and
//! distances, and each instance's place in the box, tilt, colour, scale and turn.
//!
//! ⚠ A list of far stand-ins (`IsSLODList`) names several archetypes, and the `GUID` of each of
//! its instances picks one by an id no map body states: [`slod::solve`] works it out over every list.

use super::super::lights::{check_table, Layouts};
use super::super::links::MapLinks;
use super::super::{
    broken, f32_at, field, floats, u16_at, u32_at, u64_at, Aabb, Blocks, MapData, Result,
    ARRAY_MEMBER, ENUM8, FLOAT, GRASS_INSTANCE, GRASS_LIST, GRASS_LIST_SIZE, HASH64, LOD_LEVELS,
    U32, VEC3, VEC3V,
};
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::rbm::schema::RbmModel;

const BOOL: u8 = 0x01;
const U8: u8 = 0x11;
const U16: u8 = 0x13;
const INSTANCE_SIZE: usize = 0x10;
/// The steps of a whole turn a `RotationIdx` counts.
const TURN_STEPS: f32 = 72.0;

/// The members this reads beyond those the map and link readers check.
pub(in super::super) const LAYOUTS: &Layouts = &[
    (
        GRASS_LIST,
        "rage__fwGrassInstanceListDef",
        &[
            (0xC8A7_B90A, 0x20, HASH64),
            (lit(b"position"), 0x38, VEC3),
            (lit(b"lodDist"), 0x60, FLOAT),
            (lit(b"numChildren"), 0x68, U32),
            (lit(b"lodLevel"), 0x7E, ENUM8),
            (lit(b"ScaleRange"), 0xA0, VEC3V),
            (lit(b"LodFadeStartDist"), 0xB8, FLOAT),
            (lit(b"InstanceLodDist"), 0xBC, FLOAT),
            (lit(b"OrientToTerrain"), 0xC0, FLOAT),
            (lit(b"IsSLODList"), 0x150, BOOL),
        ],
        Some(GRASS_LIST_SIZE),
    ),
    (
        GRASS_INSTANCE,
        "rage__fwGrassInstanceListDef__InstanceData",
        &[
            (lit(b"NormalX"), 0x06, U8),
            (lit(b"NormalY"), 0x07, U8),
            (lit(b"Color"), 0x08, ARRAY_MEMBER),
            (lit(b"Scale"), 0x0B, U8),
            (lit(b"ScaleZ"), 0x0C, U8),
            (lit(b"RotationIdx"), 0x0D, U8),
            (lit(b"GUID"), 0x0E, U16),
        ],
        Some(INSTANCE_SIZE as u32),
    ),
];

/// A grass list as a streamer takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamGrassList {
    pub archetype_word: u64,
    pub archetype_name: u32,
    /// Where `lod_dist` is measured from.
    pub position: [f32; 3],
    /// `instance_lod_dist` and how far the farthest instance stands from `position`.
    pub lod_dist: f32,
    /// An index into [`LOD_LEVELS`].
    pub lod_level: u8,
    /// How many children it states.
    pub num_children: u32,
    pub batch_aabb: Aabb,
    /// The least and the most scale of an instance.
    pub scale_range: [f32; 2],
    /// How far from the camera an instance starts to fade.
    pub lod_fade_start_dist: f32,
    /// How far from the camera an instance is gone.
    pub instance_lod_dist: f32,
    /// How much of the ground's tilt an instance takes, 0 to 1.
    pub orient_to_terrain: f32,
    /// Whether it is a list of far stand-ins (`IsSLODList`), which names `archetypes`.
    pub slod: bool,
    /// The name and word of each archetype a list of far stand-ins stands in for; empty in any other.
    pub archetypes: Vec<(u32, u64)>,
    pub instances: Vec<GrassInstance>,
}

/// An instance of a grass list, as the body states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrassInstance {
    /// Where it stands in the list's box: each axis a fraction of the box, 65535 its far side.
    pub position: [u16; 3],
    /// The x and y of the ground's normal, 0 for −1 and 255 for 1.
    pub normal: [u8; 2],
    pub colour: [u8; 3],
    /// Its scale between the list's least and most, 255 the most.
    pub scale: u8,
    pub scale_z: u8,
    /// Its turn about its own up, in seventy-seconds of a whole turn.
    pub rotation: u8,
    /// Which archetype a far stand-in is of, by an id no map body states; 0 in any other list.
    pub guid: u16,
}

impl GrassInstance {
    /// The ground's normal where it stands: of length 1, and level at most.
    pub fn normal(&self) -> [f32; 3] {
        let [x, y] = self.normal.map(|n| f32::from(n) / 255.0 * 2.0 - 1.0);
        let flat = x * x + y * y;
        match flat > 1.0 {
            // A pair past the unit circle lies on the ground: it is brought back to the circle.
            true => [x / flat.sqrt(), y / flat.sqrt(), 0.0],
            false => [x, y, (1.0 - flat).sqrt()],
        }
    }

    /// How far it is turned about its own up, in radians.
    pub fn yaw(&self) -> f32 {
        f32::from(self.rotation) * (std::f32::consts::TAU / TURN_STEPS)
    }

    /// Its 16 bytes as the body stores them.
    pub fn record(&self) -> [u8; INSTANCE_SIZE] {
        let mut out = [0; INSTANCE_SIZE];
        for (axis, q) in self.position.iter().enumerate() {
            out[2 * axis..2 * axis + 2].copy_from_slice(&q.to_le_bytes());
        }
        out[0x06..0x08].copy_from_slice(&self.normal);
        out[0x08..0x0B].copy_from_slice(&self.colour);
        out[0x0B] = self.scale;
        out[0x0C] = self.scale_z;
        out[0x0D] = self.rotation;
        out[0x0E..].copy_from_slice(&self.guid.to_le_bytes());
        out
    }
}

impl StreamGrassList {
    /// Where `instance` stands in the world.
    pub fn position_of(&self, instance: &GrassInstance) -> [f32; 3] {
        let (min, max) = (self.batch_aabb.min, self.batch_aabb.max);
        std::array::from_fn(|i| {
            min[i] + (max[i] - min[i]) * f32::from(instance.position[i]) / 65535.0
        })
    }

    /// The scale of `instance` across, then up.
    pub fn scale_of(&self, instance: &GrassInstance) -> [f32; 2] {
        let [least, most] = self.scale_range;
        let between = |step: u8| least + (most - least) * f32::from(step) / 255.0;
        let across = between(instance.scale);
        // ⚠ Only a list of far stand-ins states a `ScaleZ`: every other list measured holds 0 in
        // each instance, and its instances scale alike across and up.
        match self.slod {
            true => [across, between(instance.scale_z)],
            false => [across, across],
        }
    }
}

/// Each grass list of a body the map and link readers accepted.
pub(super) fn lists(
    model: &RbmModel,
    map: &MapData,
    links: &MapLinks,
) -> Result<Vec<StreamGrassList>> {
    check_table(model, LAYOUTS)?;
    let blocks = Blocks::new(&model.data_blocks);
    let root = blocks.block(u64::from(model.root_block_id), "the root block")?;
    let records = blocks.records(
        &root.data,
        0x158,
        GRASS_LIST,
        GRASS_LIST_SIZE as usize,
        "GrassInstanceList",
    )?;
    (records
        .into_iter()
        .zip(&map.grass_lists)
        .zip(&links.grass_lists))
    .enumerate()
    .map(|(k, ((record, list), link))| {
        let what = format!("GrassInstanceList[{k}]");
        let lod_level = field::<1>(record, 0x7E)?[0];
        if usize::from(lod_level) >= LOD_LEVELS.len() {
            return Err(broken(
                what,
                format!("its lodLevel {lod_level} is no LODTYPES_DEPTH value"),
            ));
        }
        let slod = match field::<1>(record, 0x150)?[0] {
            0 => false,
            1 => true,
            other => {
                return Err(broken(
                    what,
                    format!("its IsSLODList {other} is neither 0 nor 1"),
                ))
            }
        };
        if slod == link.archetypes.is_empty() {
            return Err(broken(
                what,
                "its IsSLODList disagrees with its archetype arrays",
            ));
        }
        let scale_range: [f32; 2] = floats(record, 0xA0, &what)?;
        if scale_range[0] <= 0.0 || scale_range[0] > scale_range[1] {
            return Err(broken(
                what,
                "its ScaleRange is no least and most scale above 0",
            ));
        }
        let instances = (blocks.records(record, 0x178, GRASS_INSTANCE, INSTANCE_SIZE, &what)?)
            .into_iter()
            .map(|i| {
                Ok(GrassInstance {
                    position: [u16_at(i, 0)?, u16_at(i, 2)?, u16_at(i, 4)?],
                    normal: field(i, 0x06)?,
                    colour: field(i, 0x08)?,
                    scale: field::<1>(i, 0x0B)?[0],
                    scale_z: field::<1>(i, 0x0C)?[0],
                    rotation: field::<1>(i, 0x0D)?[0],
                    guid: u16_at(i, 0x0E)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(StreamGrassList {
            archetype_word: u64_at(record, 0x20)?,
            archetype_name: list.archetype_name,
            position: floats(record, 0x38, &what)?,
            lod_dist: f32_at(record, 0x60, &what)?,
            lod_level,
            num_children: u32_at(record, 0x68)?,
            batch_aabb: list.batch_aabb,
            scale_range,
            lod_fade_start_dist: f32_at(record, 0xB8, &what)?,
            instance_lod_dist: f32_at(record, 0xBC, &what)?,
            orient_to_terrain: f32_at(record, 0xC0, &what)?,
            slod,
            archetypes: link.archetypes.clone(),
            instances,
        })
    })
    .collect()
}

#[path = "map_slod.rs"]
pub mod slod;

#[cfg(test)]
#[path = "map_grass_tests.rs"]
mod tests;
