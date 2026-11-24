//! A GTA VI map body as a streamer takes it: one read of a `CMapData` gives what places and levels
//! each entity and each instance of its prop and grass lists, and where each one's
//! level-of-detail parent is.
//!
//! ⚠ An entity alone takes a parent in its own map, and only an earlier entity: any other index
//! with no parent map picked is of no known meaning, so the element reads with no parent and the
//! body still reads, as the link reader takes it.

use super::lights::{check_table, Layouts};
use super::links::{self, Links, LodLink, MapLinks};
use super::{
    broken, f32_at, field, floats, u16_at, u32_at, u64_at, Aabb, Blocks, MapBody, MapData, Result,
    ARRAY, FLOAT, LOD_LEVELS, PROP_INSTANCE, PROP_LIST, PROP_LIST_SIZE, U32, VEC3,
};
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::rbm::schema::RbmModel;

pub use grass::{GrassInstance, StreamGrassList};

/// The type of a block of 16-bit signed numbers.
const S16_BLOCK: u32 = 0x12;

/// The members of a prop list this reads beyond those the map and link readers check.
pub(super) const LAYOUTS: &Layouts = &[
    (
        PROP_LIST,
        "rage__fwPropInstanceListDef",
        &[
            (lit(b"position"), 0x38, VEC3),
            (lit(b"flags"), 0x48, U32),
            (lit(b"numChildren"), 0x68, U32),
            (0xCFBC_3397, 0x80, ARRAY),
            (lit(b"InstanceLodDist"), 0x168, FLOAT),
        ],
        None,
    ),
    (
        PROP_INSTANCE,
        "rage__fwPropInstanceListDef__InstanceData",
        &[(lit(b"Tint"), 0x34, U32)],
        None,
    ),
];

/// A map body as a streamer takes it: what places and levels each entity and each prop instance.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamMap {
    pub name: u32,
    /// The name of the map's block map (the `CMapData` member at +0x50), 0 for none.
    pub block: u32,
    pub parent: u32,
    pub flags: u32,
    pub content_flags: u32,
    pub streaming_extents: Aabb,
    pub entities_extents: Aabb,
    /// The names of the maps its level-of-detail parents lie in, which a link's `map` indexes.
    pub lod_parents: Vec<u32>,
    pub entities: Vec<StreamEntity>,
    pub prop_lists: Vec<StreamPropList>,
    /// How many grass lists it holds: a parent index counts past the entities and the prop lists into them.
    pub grass_lists: usize,
    /// Its grass lists, `grass_lists` of them.
    pub grass: Vec<StreamGrassList>,
}

/// An entity as a streamer takes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamEntity {
    pub archetype_word: u64,
    pub archetype_name: u32,
    pub position: [f32; 3],
    /// The quaternion that turns the model, x, y, z, w.
    pub rotation: [f32; 4],
    pub scale_xy: f32,
    pub scale_z: f32,
    /// −1 where the archetype's applies.
    pub lod_dist: f32,
    pub child_lod_dist: f32,
    pub num_children: u32,
    /// An index into [`LOD_LEVELS`].
    pub lod_level: u8,
    pub flags: u32,
    pub interior: bool,
    pub parent: Option<StreamLink>,
}

/// Where an element's level-of-detail parent is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamLink {
    /// The parent map among `lod_parents`; `None` for an earlier entity of the element's own map.
    pub map: Option<u8>,
    /// The parent among that map's entities, then its prop lists, then its grass lists.
    pub index: u16,
    /// The instance of the parent prop list, where the parent is one and names an instance.
    pub instance: Option<u16>,
}

/// A prop list as a streamer takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamPropList {
    pub archetype_word: u64,
    pub archetype_name: u32,
    /// Where `lod_dist` is measured from.
    pub position: [f32; 3],
    /// `instance_lod_dist` and how far the farthest instance stands from `position`.
    pub lod_dist: f32,
    pub child_lod_dist: f32,
    /// How far from the camera one instance draws.
    pub instance_lod_dist: f32,
    /// An index into [`LOD_LEVELS`].
    pub lod_level: u8,
    /// Bits 11 and 12 make it a list of the batch LOD and SLOD levels.
    pub flags: u32,
    /// How many children it states, the entities that stand in for its instances among them.
    pub num_children: u32,
    pub parent: Option<StreamLink>,
    pub batch_aabb: Aabb,
    /// Each instance's placing: three rows of four, the model's axes with the translation in the fourth column.
    pub instances: Vec<[[f32; 4]; 3]>,
    /// The instance of the parent list each instance stands in for; empty in a list with no parent.
    pub parent_instances: Vec<u16>,
    /// Each instance's `Tint`: red, green, blue, alpha.
    pub tints: Vec<[u8; 4]>,
}

/// Where an instance's placing puts a point of its model: row `k` is where the model's axis `k` points, and the fourth column moves it.
pub fn place(instance: &[[f32; 4]; 3], point: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| {
        instance[i][3]
            + (0..3)
                .map(|axis| instance[axis][i] * point[axis])
                .sum::<f32>()
    })
}

/// The map body of these two sections as a streamer takes it; refused where it is no `CMapData` or a reader refuses it.
pub fn open(virtual_section: &[u8], physical: &[u8]) -> Result<StreamMap> {
    read(&crate::meta::binary::rbm::gta6::read_model(
        virtual_section,
        physical,
    )?)
}

/// The same from a body's class table and data blocks.
pub fn read(model: &RbmModel) -> Result<StreamMap> {
    let no_map_data = || {
        broken(
            "the root block",
            "it is a CMapTypes, where a streamer takes a CMapData",
        )
    };
    let MapBody::Data(map) = super::read(model)? else {
        return Err(no_map_data());
    };
    let Links::Data(links) = links::read(model)? else {
        return Err(no_map_data());
    };
    let entities = (map.entities.iter().zip(&links.entities).enumerate())
        .map(|(k, (e, link))| StreamEntity {
            archetype_word: e.archetype_word,
            archetype_name: e.archetype_name,
            position: e.position,
            rotation: e.model_rotation(),
            scale_xy: e.scale_xy,
            scale_z: e.scale_z,
            lod_dist: e.lod_dist,
            child_lod_dist: e.child_lod_dist,
            num_children: e.num_children,
            lod_level: e.lod_level,
            flags: e.flags,
            interior: e.interior,
            parent: parent(link, k),
        })
        .collect();
    Ok(StreamMap {
        name: map.name,
        block: map.hash_6c0f9c8c,
        parent: map.parent,
        flags: map.flags,
        content_flags: map.content_flags,
        streaming_extents: map.streaming_extents,
        entities_extents: map.entities_extents,
        lod_parents: links.lod_parents.iter().map(|p| p.name).collect(),
        entities,
        prop_lists: prop_lists(model, &map, &links)?,
        grass_lists: map.grass_lists.len(),
        grass: grass::lists(model, &map, &links)?,
    })
}

/// A link as a streamer follows it, for an element that may take any of the first `earlier` entities of its own map as its parent.
fn parent(link: &LodLink, earlier: usize) -> Option<StreamLink> {
    let index = u16::try_from(link.parent_index).ok()?;
    if link.map.is_none() && usize::from(index) >= earlier {
        return None;
    }
    Some(StreamLink {
        // A selector is a signed byte the link reader held below the parent maps' count.
        map: link.map.map(|selector| selector as u8),
        index,
        instance: u16::try_from(link.instance).ok(),
    })
}

/// Each prop list of a body the map and link readers accepted, with what neither holds: its
/// archetype's word, its own level-of-detail fields, each instance's `InstMat` and `Tint`, and
/// the parent's instance each one stands in for.
fn prop_lists(model: &RbmModel, map: &MapData, links: &MapLinks) -> Result<Vec<StreamPropList>> {
    check_table(model, LAYOUTS)?;
    let blocks = Blocks::new(&model.data_blocks);
    let root = blocks.block(u64::from(model.root_block_id), "the root block")?;
    let records = blocks.records(
        &root.data,
        0x148,
        PROP_LIST,
        PROP_LIST_SIZE as usize,
        "PropInstanceList",
    )?;
    (records
        .into_iter()
        .zip(&map.prop_lists)
        .zip(&links.prop_lists))
    .enumerate()
    .map(|(k, ((record, list), link))| {
        let what = format!("PropInstanceList[{k}]");
        let lod_level = field::<1>(record, 0x7E)?[0];
        if usize::from(lod_level) >= LOD_LEVELS.len() {
            return Err(broken(
                what,
                format!("its lodLevel {lod_level} is no LODTYPES_DEPTH value"),
            ));
        }
        let placed = blocks.records(record, 0x90, PROP_INSTANCE, 0x40, &what)?;
        let instances = (placed.iter())
            .map(|i| {
                Ok([
                    floats(i, 0x00, &what)?,
                    floats(i, 0x10, &what)?,
                    floats(i, 0x20, &what)?,
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        let tints = (placed.iter())
            .map(|i| field::<4>(i, 0x34))
            .collect::<Result<Vec<_>>>()?;
        let parent_instances = match blocks.array(record, 0x80, &what)? {
            None => Vec::new(),
            Some((S16_BLOCK, rest, count))
                if count == instances.len() && rest.len() >= 2 * count =>
            {
                (0..count)
                    .map(|at| u16_at(rest, 2 * at))
                    .collect::<Result<Vec<_>>>()?
            }
            Some((_, _, count)) => {
                return Err(broken(
                    what,
                    format!(
                        "its array at 0x80 holds {count} elements, not a 16-bit instance of its parent for each of its {}",
                        instances.len()
                    ),
                ))
            }
        };
        if parent_instances.iter().any(|at| *at > i16::MAX as u16) {
            return Err(broken(what, "it stands in for an instance below 0"));
        }
        Ok(StreamPropList {
            archetype_word: u64_at(record, 0x20)?,
            archetype_name: list.archetype_name,
            position: floats(record, 0x38, &what)?,
            lod_dist: f32_at(record, 0x60, &what)?,
            child_lod_dist: f32_at(record, 0x64, &what)?,
            instance_lod_dist: f32_at(record, 0x168, &what)?,
            lod_level,
            flags: u32_at(record, 0x48)?,
            num_children: u32_at(record, 0x68)?,
            // A prop list takes no parent in its own map.
            parent: parent(link, 0),
            batch_aabb: list.batch_aabb,
            instances,
            parent_instances,
            tints,
        })
    })
    .collect()
}

#[path = "map_grass.rs"]
pub mod grass;

#[cfg(test)]
#[path = "map_stream_tests.rs"]
mod tests;
