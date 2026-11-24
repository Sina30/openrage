//! A GTA VI interior as a streamer takes it: a `CMloArchetypeDef`'s rooms, portals and entity sets
//! as the link reader reads them, with what places each of its entities, each room's time-cycle
//! names and the distance its contents draw to; and what a `CMloInstanceDef` adds to the entity
//! that places an interior.
//!
//! ⚠ An interior's entities stand in the interior's own space and none takes a level-of-detail
//! parent, so an [`InteriorChild`] holds no level: the level rules a map's entity is held to do not
//! hold inside an interior.

use super::lights::{check_table, Layouts};
use super::links::{self, EntitySetLocation, Links, Portal, Room, TimeCycleModifier};
use super::{
    broken, check_layouts, f32_at, floats, u32_at, u64_at, Blocks, MapBody, Result, ARRAY,
    C_ENTITY_DEF, C_MLO_ARCHETYPE_DEF, C_MLO_INSTANCE_DEF, FLOAT, STRING_HASH, U32,
};
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::rbm::schema::RbmModel;

const C_MLO_ROOM_DEF: u32 = lit(b"CMloRoomDef");
const C_MLO_ENTITY_SET: u32 = lit(b"CMloEntitySet");

const INSTANCE_SIZE: usize = 0x250;
const ROOM_SIZE: usize = 0x70;
const ENTITY_SET_SIZE: usize = 0x28;
/// The bytes of a `CEntityDef` this reads, as the map reader reads its entities.
const ENTITY_READ: usize = 0x84;

/// What this reads beside the members the map reader and the link reader check.
const LAYOUTS: &Layouts = &[
    (
        C_MLO_ARCHETYPE_DEF,
        "CMloArchetypeDef",
        &[(lit(b"mloFlags"), 0xB8, U32)],
        None,
    ),
    (
        C_MLO_ROOM_DEF,
        "CMloRoomDef",
        &[
            (lit(b"blend"), 0x30, FLOAT),
            (lit(b"timecycleName"), 0x34, STRING_HASH),
            (lit(b"secondaryTimecycleName"), 0x38, STRING_HASH),
        ],
        None,
    ),
    (
        C_MLO_INSTANCE_DEF,
        "CMloInstanceDef",
        &[
            (lit(b"groupId"), 0x90, U32),
            (lit(b"floorId"), 0x94, U32),
            (lit(b"defaultEntitySets"), 0xA0, ARRAY),
            (lit(b"numExitPortals"), 0xB0, U32),
            (lit(b"MLOInstflags"), 0xB4, U32),
        ],
        Some(INSTANCE_SIZE as u32),
    ),
];

/// An entity of an interior or of one of its entity sets, in the interior's own space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteriorChild {
    pub archetype_word: u64,
    pub archetype_name: u32,
    pub position: [f32; 3],
    /// The quaternion that turns the model, x, y, z, w: the conjugate of the one stored.
    pub rotation: [f32; 4],
    pub scale_xy: f32,
    pub scale_z: f32,
    /// −1 where the archetype's applies.
    pub lod_dist: f32,
    pub flags: u32,
}

/// A room of an interior: what the link reader reads of it, and its time cycle.
#[derive(Debug, Clone, PartialEq)]
pub struct InteriorRoom {
    pub room: Room,
    pub blend: f32,
    pub timecycle_name: u32,
    pub secondary_timecycle_name: u32,
}

/// A `CMloEntitySet`: entities an instance draws only where it names the set.
#[derive(Debug, Clone, PartialEq)]
pub struct InteriorSet {
    pub name: u32,
    /// Where each of `entities` stands, in their order.
    pub locations: Vec<EntitySetLocation>,
    pub entities: Vec<InteriorChild>,
}

/// A `CMloArchetypeDef` as a streamer takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Interior {
    /// The archetype's index in its `CMapTypes`.
    pub archetype: usize,
    pub name: u32,
    /// The word a `CMloInstanceDef` names it by.
    pub name_word: u64,
    /// How far from the middle of its box its contents draw.
    pub lod_dist: f32,
    /// Its box, in its own space; it states no sphere.
    pub bb_min: [f32; 3],
    pub bb_max: [f32; 3],
    pub mlo_flags: u32,
    pub entities: Vec<InteriorChild>,
    pub rooms: Vec<InteriorRoom>,
    pub portals: Vec<Portal>,
    pub entity_sets: Vec<InteriorSet>,
    pub time_cycle_modifiers: Vec<TimeCycleModifier>,
}

impl Interior {
    /// The entities an instance draws: the interior's own, then those of each entity set `on` names.
    pub fn drawn<'a>(&'a self, on: &'a [u32]) -> impl Iterator<Item = &'a InteriorChild> {
        let sets = (self.entity_sets.iter()).filter(move |set| on.contains(&set.name));
        (self.entities.iter()).chain(sets.flat_map(|set| &set.entities))
    }
}

/// What a `CMloInstanceDef` states: where it places which interior, and the entity sets it turns on.
#[derive(Debug, Clone, PartialEq)]
pub struct InteriorInstance {
    /// Its place among the map's entities.
    pub entity: usize,
    /// The interior it places: [`Interior::name_word`].
    pub archetype_word: u64,
    pub archetype_name: u32,
    pub position: [f32; 3],
    /// The quaternion that turns the interior, x, y, z, w.
    pub rotation: [f32; 4],
    pub scale_xy: f32,
    pub scale_z: f32,
    /// −1 where the archetype's applies.
    pub lod_dist: f32,
    pub group_id: u32,
    pub floor_id: u32,
    /// The names of the interior's entity sets that are on.
    pub default_entity_sets: Vec<u32>,
    pub num_exit_portals: u32,
    pub flags: u32,
}

/// The interiors of the `CMapTypes` body these two sections hold; refused for a `CMapData`.
pub fn open(virtual_section: &[u8], physical: &[u8]) -> Result<Vec<Interior>> {
    read(&crate::meta::binary::rbm::gta6::read_model(
        virtual_section,
        physical,
    )?)
}

/// The interiors of a `CMapTypes` body's class table and data blocks, in the order of its archetypes.
pub fn read(model: &RbmModel) -> Result<Vec<Interior>> {
    check_layouts(model)?;
    check_table(model, LAYOUTS)?;
    let Links::Types(linked) = links::read(model)? else {
        return Err(broken(
            "the root block",
            "it is a CMapData, where an interior's archetype lies in a CMapTypes",
        ));
    };
    let blocks = Blocks::new(&model.data_blocks);
    let root = blocks.block(u64::from(model.root_block_id), "the root block")?;
    let Some((_, pointers, _)) = blocks.array(&root.data, 0x18, "archetypes")? else {
        return Ok(Vec::new());
    };
    (linked.into_iter())
        .map(|interior| {
            let what = format!("archetypes[{}]", interior.archetype);
            match blocks.target(u64_at(pointers, 8 * interior.archetype)?, &what)? {
                Some((C_MLO_ARCHETYPE_DEF, a)) => read_interior(&blocks, interior, a, &what),
                _ => Err(broken(what, "it is no interior")),
            }
        })
        .collect()
}

fn read_interior(
    blocks: &Blocks,
    linked: links::Interior,
    a: &[u8],
    what: &str,
) -> Result<Interior> {
    let room_records = blocks.records(
        a,
        0xD0,
        C_MLO_ROOM_DEF,
        ROOM_SIZE,
        &format!("{what}.rooms"),
    )?;
    let rooms = (linked.rooms.into_iter().zip(room_records))
        .map(|(room, r)| {
            Ok(InteriorRoom {
                room,
                blend: f32_at(r, 0x30, what)?,
                timecycle_name: u32_at(r, 0x34)?,
                secondary_timecycle_name: u32_at(r, 0x38)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let set_records = blocks.records(
        a,
        0xF0,
        C_MLO_ENTITY_SET,
        ENTITY_SET_SIZE,
        &format!("{what}.entitySets"),
    )?;
    let entity_sets = (linked.entity_sets.into_iter().zip(set_records))
        .enumerate()
        .map(|(j, (set, s))| {
            Ok(InteriorSet {
                name: set.name,
                locations: set.entities.iter().map(|(at, _)| *at).collect(),
                entities: children(blocks, s, 0x18, &format!("{what}.entitySets[{j}]"))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Interior {
        archetype: linked.archetype,
        name: linked.name,
        name_word: linked.name_word,
        lod_dist: f32_at(a, 0x38, what)?,
        bb_min: floats(a, 0x40, what)?,
        bb_max: floats(a, 0x4C, what)?,
        mlo_flags: u32_at(a, 0xB8)?,
        entities: children(blocks, a, 0xC0, &format!("{what}.entities"))?,
        rooms,
        portals: linked.portals,
        entity_sets,
        time_cycle_modifiers: linked.time_cycle_modifiers,
    })
}

/// The `CEntityDef` records the array of pointers at `at` names, each as an interior holds it.
fn children(blocks: &Blocks, bytes: &[u8], at: usize, what: &str) -> Result<Vec<InteriorChild>> {
    (blocks.pointed(bytes, at, ENTITY_READ, what)?.into_iter())
        .enumerate()
        .map(|(k, (class, e))| {
            let what = format!("{what}[{k}]");
            if class != C_ENTITY_DEF {
                return Err(broken(
                    what,
                    format!("it points at a block of type {class:#010X}, no CEntityDef"),
                ));
            }
            let [x, y, z, w]: [f32; 4] = floats(e, 0x10, &what)?;
            let norm = (x * x + y * y + z * z + w * w).sqrt();
            if (norm - 1.0).abs() >= 1e-3 {
                return Err(broken(
                    what,
                    format!("its rotation's norm is {norm:.5}, not 1"),
                ));
            }
            let archetype_name = u32_at(e, 0x44)?;
            if archetype_name == 0 {
                return Err(broken(what, "its archetypeName is 0"));
            }
            Ok(InteriorChild {
                archetype_word: u64_at(e, 0x20)?,
                archetype_name,
                position: floats(e, 0x38, &what)?,
                rotation: [-x, -y, -z, w],
                scale_xy: f32_at(e, 0x54, &what)?,
                scale_z: f32_at(e, 0x58, &what)?,
                lod_dist: f32_at(e, 0x60, &what)?,
                flags: u32_at(e, 0x48)?,
            })
        })
        .collect()
}

/// The interior instances of the `CMapData` body these two sections hold; refused for a `CMapTypes`.
pub fn open_instances(virtual_section: &[u8], physical: &[u8]) -> Result<Vec<InteriorInstance>> {
    read_instances(&crate::meta::binary::rbm::gta6::read_model(
        virtual_section,
        physical,
    )?)
}

/// The interior instances of a `CMapData` body's class table and data blocks, in the order of its entities.
pub fn read_instances(model: &RbmModel) -> Result<Vec<InteriorInstance>> {
    check_table(model, LAYOUTS)?;
    let MapBody::Data(map) = super::read(model)? else {
        return Err(broken(
            "the root block",
            "it is a CMapTypes, where an interior is placed by a CMapData",
        ));
    };
    let blocks = Blocks::new(&model.data_blocks);
    let root = blocks.block(u64::from(model.root_block_id), "the root block")?;
    let Some((_, pointers, _)) = blocks.array(&root.data, 0xD0, "entities")? else {
        return Ok(Vec::new());
    };
    (map.entities.iter().enumerate())
        .filter(|(_, e)| e.interior)
        .map(|(k, e)| {
            let what = format!("entities[{k}]");
            let record = blocks.target(u64_at(pointers, 8 * k)?, &what)?;
            let Some((_, i)) = record.filter(|(_, i)| i.len() >= INSTANCE_SIZE) else {
                return Err(broken(what, "it runs past its block"));
            };
            let default_entity_sets = match blocks.array(i, 0xA0, &what)? {
                None => Vec::new(),
                Some((_, names, count)) if names.len() >= 4 * count => (0..count)
                    .map(|n| u32_at(names, 4 * n))
                    .collect::<Result<_>>()?,
                Some(_) => {
                    return Err(broken(
                        what,
                        "its defaultEntitySets run past their block",
                    ))
                }
            };
            Ok(InteriorInstance {
                entity: k,
                archetype_word: e.archetype_word,
                archetype_name: e.archetype_name,
                position: e.position,
                rotation: e.model_rotation(),
                scale_xy: e.scale_xy,
                scale_z: e.scale_z,
                lod_dist: e.lod_dist,
                group_id: u32_at(i, 0x90)?,
                floor_id: u32_at(i, 0x94)?,
                default_entity_sets,
                num_exit_portals: u32_at(i, 0xB0)?,
                flags: u32_at(i, 0xB4)?,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "map_interior_tests.rs"]
mod tests;
