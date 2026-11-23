//! GTA VI's world data over many bodies: which bodies are world data, a census that counts a body
//! at a time, an index of the entity rows of many maps for drawing an area, and a grid that finds
//! the points in view without a walk of them all.
//!
//! ⚠ An entity group's entities stand in the group's own space and nothing measured places a
//! group, so they are counted and never indexed.

use std::collections::HashMap;
use std::ops::Range;

use super::lanes::{self, LaneBody};
use super::lights::{self, check_table, rotate, Layouts};
use super::{
    broken, field, u32_at, Aabb, Blocks, MapBody, MapData, MapError, Result, ARRAY, C_ENTITY_DEF,
    C_MAP_DATA, C_MAP_TYPES, ENTITY_SIZE, STRING_HASH,
};
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::rbm::schema::RbmModel;

/// The root class of an entity group: a named list of entities in the group's own space.
pub const ENTITY_GROUP: u32 = 0x35AB_C248;

const BITSET8: u8 = 0x63;
const GROUP_SIZE: usize = 0x20;

const LAYOUTS: &Layouts = &[(
    ENTITY_GROUP,
    "the entity group",
    &[
        (lit(b"flags"), 0x08, BITSET8),
        (lit(b"name"), 0x0C, STRING_HASH),
        (0xD222_611A, 0x10, ARRAY),
    ],
    Some(GROUP_SIZE as u32),
)];

/// What a world-data body is, by its root class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldKind {
    MapData,
    MapTypes,
    EntityGroup,
    LaneNodes,
    LaneLinks,
    LaneJunctions,
}

impl WorldKind {
    /// The kind a body of root class `root` is; `None` for a body that is no world data.
    pub fn of_root(root: u32) -> Option<Self> {
        Some(match root {
            C_MAP_DATA => Self::MapData,
            C_MAP_TYPES => Self::MapTypes,
            ENTITY_GROUP => Self::EntityGroup,
            lanes::NODE_LIST => Self::LaneNodes,
            lanes::LINK_LIST => Self::LaneLinks,
            lanes::JUNCTION_LIST => Self::LaneJunctions,
            _ => return None,
        })
    }
}

/// An entity group: its name and how many entities it lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityGroup {
    pub name: u32,
    pub flags: u8,
    pub entities: usize,
}

/// The entity group of a headerless RBM resource's class table and data blocks.
pub fn read_group(model: &RbmModel) -> Result<EntityGroup> {
    check_table(model, LAYOUTS)?;
    let size = (model.classes.get(&C_ENTITY_DEF)).map_or(ENTITY_SIZE, |c| c.data_size);
    if size != ENTITY_SIZE {
        return Err(MapError::Size {
            class: "CEntityDef",
            found: size,
            expected: ENTITY_SIZE,
        });
    }
    let blocks = Blocks::new(&model.data_blocks);
    let root = blocks.block(u64::from(model.root_block_id), "the root block")?;
    if root.data_type != ENTITY_GROUP {
        return Err(MapError::NotAMap(root.data_type));
    }
    if root.data.len() < GROUP_SIZE {
        return Err(broken("the root block", "it is shorter than its class"));
    }
    let entities = blocks.records(
        &root.data,
        0x10,
        C_ENTITY_DEF,
        ENTITY_SIZE as usize,
        "the group's entities",
    )?;
    Ok(EntityGroup {
        name: u32_at(&root.data, 0x0C)?,
        flags: field::<1>(&root.data, 0x08)?[0],
        entities: entities.len(),
    })
}

/// What a walk of world-data bodies counted; each body adds to it and is then dropped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Census {
    /// The `CMapData` bodies, and those of them that place an entity.
    pub maps: u64,
    pub placement_maps: u64,
    pub map_types: u64,
    /// The entities of the maps, interior instances among them.
    pub entities: u64,
    pub interior_instances: u64,
    pub prop_instances: u64,
    pub grass_instances: u64,
    /// The archetypes of the map types, interiors among them.
    pub archetypes: u64,
    pub interiors: u64,
    pub light_effects: u64,
    /// The lights on entities, on archetypes, on the entities of interiors and on those of their
    /// entity sets.
    pub entity_lights: u64,
    pub archetype_lights: u64,
    pub interior_lights: u64,
    pub entity_set_lights: u64,
    /// The map bodies that read and whose lights did not.
    pub lights_unread: u64,
    pub entity_groups: u64,
    pub group_entities: u64,
    pub lane_nodes: u64,
    pub lane_links: u64,
    pub junctions: u64,
    pub signalised_junctions: u64,
    /// The world-data bodies no reader read.
    pub unread: u64,
}

/// A world-data body as a census read it: what a caller may keep of it.
#[derive(Debug, Clone, PartialEq)]
pub enum Counted {
    Map(MapBody),
    Group(EntityGroup),
    Lanes(LaneBody),
}

impl Census {
    /// Every light counted.
    pub fn lights(&self) -> u64 {
        self.entity_lights + self.archetype_lights + self.interior_lights + self.entity_set_lights
    }

    /// Count the body of these two sections; `None` for a body that is no world data, and why it
    /// did not read for one that is counted unread.
    pub fn add_body(
        &mut self,
        virtual_section: &[u8],
        physical: &[u8],
    ) -> Option<std::result::Result<Counted, String>> {
        let kind = WorldKind::of_root(super::root_type(virtual_section)?)?;
        let read = crate::meta::binary::rbm::gta6::read_model(virtual_section, physical)
            .map_err(|e| e.to_string())
            .and_then(|model| self.add_model(kind, &model));
        if read.is_err() {
            self.unread += 1;
        }
        Some(read)
    }

    /// Count a body of `kind` from its class table and data blocks.
    pub fn add_model(
        &mut self,
        kind: WorldKind,
        model: &RbmModel,
    ) -> std::result::Result<Counted, String> {
        match kind {
            WorldKind::MapData | WorldKind::MapTypes => {
                let body = super::read(model).map_err(|e| e.to_string())?;
                self.add_map(&body);
                match lights::read(model) {
                    Ok(found) => self.add_lights(&found),
                    Err(_) => self.lights_unread += 1,
                }
                Ok(Counted::Map(body))
            }
            WorldKind::EntityGroup => {
                let group = read_group(model).map_err(|e| e.to_string())?;
                self.entity_groups += 1;
                self.group_entities += group.entities as u64;
                Ok(Counted::Group(group))
            }
            WorldKind::LaneNodes | WorldKind::LaneLinks | WorldKind::LaneJunctions => {
                let body = lanes::read(model).map_err(|e| e.to_string())?;
                match &body {
                    LaneBody::Nodes(n) => self.lane_nodes += n.len() as u64,
                    LaneBody::Links(l) => self.lane_links += l.len() as u64,
                    LaneBody::Junctions(j) => {
                        self.junctions += j.len() as u64;
                        self.signalised_junctions +=
                            j.iter().filter(|j| j.is_signalised()).count() as u64;
                    }
                }
                Ok(Counted::Lanes(body))
            }
        }
    }

    fn add_map(&mut self, body: &MapBody) {
        match body {
            MapBody::Data(m) => {
                self.maps += 1;
                self.placement_maps += u64::from(!m.entities.is_empty());
                self.entities += m.entities.len() as u64;
                self.interior_instances += m.entities.iter().filter(|e| e.interior).count() as u64;
                let instances = |lists: &[super::InstanceList]| {
                    lists.iter().map(|l| l.instances as u64).sum::<u64>()
                };
                self.prop_instances += instances(&m.prop_lists);
                self.grass_instances += instances(&m.grass_lists);
            }
            MapBody::Types(t) => {
                self.map_types += 1;
                self.archetypes += t.archetypes.len() as u64;
                self.interiors += (t.archetypes.iter())
                    .filter(|a| a.class == super::ArchetypeClass::Interior)
                    .count() as u64;
            }
        }
    }

    fn add_lights(&mut self, found: &lights::MapLights) {
        self.light_effects += found.effects.len() as u64;
        for (owner, effect) in &found.effects {
            let lights = effect.lights().count() as u64;
            match owner {
                lights::LightOwner::Entity(_) => self.entity_lights += lights,
                lights::LightOwner::Archetype(_) => self.archetype_lights += lights,
                lights::LightOwner::InteriorEntity { .. } => self.interior_lights += lights,
                lights::LightOwner::EntitySetEntity { .. } => self.entity_set_lights += lights,
            }
        }
    }

    /// Add what another census counted, as the census of one shard joins the whole.
    pub fn merge(&mut self, other: &Census) {
        for (mine, theirs) in self.fields_mut().into_iter().zip(other.fields()) {
            *mine += theirs;
        }
    }

    fn fields(&self) -> [u64; 22] {
        let mut copy = *self;
        copy.fields_mut().map(|f| *f)
    }

    fn fields_mut(&mut self) -> [&mut u64; 22] {
        [
            &mut self.maps,
            &mut self.placement_maps,
            &mut self.map_types,
            &mut self.entities,
            &mut self.interior_instances,
            &mut self.prop_instances,
            &mut self.grass_instances,
            &mut self.archetypes,
            &mut self.interiors,
            &mut self.light_effects,
            &mut self.entity_lights,
            &mut self.archetype_lights,
            &mut self.interior_lights,
            &mut self.entity_set_lights,
            &mut self.lights_unread,
            &mut self.entity_groups,
            &mut self.group_entities,
            &mut self.lane_nodes,
            &mut self.lane_links,
            &mut self.junctions,
            &mut self.signalised_junctions,
            &mut self.unread,
        ]
    }
}

/// What a world-data body gives a view of an area, read once: a map with its lights, where they
/// read, or a list of the lane graph. An entity group stands in its own space and gives nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum AreaBody {
    Map(MapBody, Option<lights::MapLights>),
    Lanes(LaneBody),
}

/// What the body of these two sections gives a view of an area; `None` for a body that is no
/// world data, an entity group, or a body its reader refuses.
pub fn read_area_body(virtual_section: &[u8], physical: &[u8]) -> Option<AreaBody> {
    let kind = WorldKind::of_root(super::root_type(virtual_section)?)?;
    if kind == WorldKind::EntityGroup {
        return None;
    }
    let model = crate::meta::binary::rbm::gta6::read_model(virtual_section, physical).ok()?;
    area_body(kind, &model)
}

/// What a body of `kind` gives a view of an area, from its class table and data blocks.
pub fn area_body(kind: WorldKind, model: &RbmModel) -> Option<AreaBody> {
    match kind {
        WorldKind::MapData | WorldKind::MapTypes => Some(AreaBody::Map(
            super::read(model).ok()?,
            lights::read(model).ok(),
        )),
        WorldKind::LaneNodes | WorldKind::LaneLinks | WorldKind::LaneJunctions => {
            lanes::read(model).ok().map(AreaBody::Lanes)
        }
        WorldKind::EntityGroup => None,
    }
}

/// One entity of an indexed map: what places it and what draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementRow {
    pub archetype_name: u32,
    /// The archetype's 64-bit word, which an archetype's box is joined on.
    pub archetype_word: u64,
    pub position: [f32; 3],
    /// The quaternion that turns the model, x, y, z, w.
    pub rotation: [f32; 4],
    pub scale_xy: f32,
    pub scale_z: f32,
    /// The entity's `parentIndex`, −1 for none: an index into the entities and then the prop lists
    /// of the map its own map's `parent` names, and of no known meaning in a map with no parent.
    pub lod_parent: i16,
    /// −1 where the archetype's applies.
    pub lod_dist: f32,
    /// An index into [`super::LOD_LEVELS`].
    pub lod_level: u8,
    pub interior: bool,
}

impl PlacementRow {
    /// Where the row's model stands.
    pub fn pose(&self) -> lights::Pose {
        lights::Pose {
            position: self.position,
            rotation: self.rotation,
            scale_xy: self.scale_xy,
            scale_z: self.scale_z,
        }
    }

    /// The eight corners in the world of a model's box, `bb_min` to `bb_max`, placed by this row.
    pub fn corners(&self, bb_min: [f32; 3], bb_max: [f32; 3]) -> [[f32; 3]; 8] {
        let scale = [self.scale_xy, self.scale_xy, self.scale_z];
        std::array::from_fn(|corner| {
            let local = [0, 1, 2].map(|axis| {
                let side = if corner >> axis & 1 == 0 {
                    bb_min[axis]
                } else {
                    bb_max[axis]
                };
                side * scale[axis]
            });
            let turned = rotate(self.rotation, local);
            [0, 1, 2].map(|axis| self.position[axis] + turned[axis])
        })
    }
}

/// A map of the index: where its body is, what it names, and its rows.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedMap {
    /// Where the caller found the body, such as its offset in an archive.
    pub offset: u64,
    pub name: u32,
    /// The map `lod_parent` indexes, 0 for none.
    pub parent: u32,
    /// The box its entities are streamed in.
    pub streaming_extents: Aabb,
    pub rows: Range<usize>,
}

/// The entity rows of many maps, map after map, for drawing an area at once.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlacementIndex {
    pub maps: Vec<IndexedMap>,
    pub rows: Vec<PlacementRow>,
}

impl PlacementIndex {
    /// Add the entities of `map`, found at `offset`; a map that places none adds nothing.
    pub fn add(&mut self, offset: u64, map: &MapData) {
        if map.entities.is_empty() {
            return;
        }
        let start = self.rows.len();
        self.rows.extend(map.entities.iter().map(|e| PlacementRow {
            archetype_name: e.archetype_name,
            archetype_word: e.archetype_word,
            position: e.position,
            rotation: e.model_rotation(),
            scale_xy: e.scale_xy,
            scale_z: e.scale_z,
            lod_parent: e.parent_index,
            lod_dist: e.lod_dist,
            lod_level: e.lod_level,
            interior: e.interior,
        }));
        self.maps.push(IndexedMap {
            offset,
            name: map.name,
            parent: map.parent,
            streaming_extents: map.streaming_extents,
            rows: start..self.rows.len(),
        });
    }

    /// The rows of map `map` of the index.
    pub fn rows_of(&self, map: usize) -> &[PlacementRow] {
        match self.maps.get(map) {
            Some(m) => &self.rows[m.rows.clone()],
            None => &[],
        }
    }

    /// The maps whose streaming box meets the rectangle `min` to `max` of the ground plane.
    pub fn maps_within(
        &self,
        min: [f32; 2],
        max: [f32; 2],
    ) -> impl Iterator<Item = (usize, &IndexedMap)> {
        self.maps.iter().enumerate().filter(move |(_, m)| {
            let b = &m.streaming_extents;
            (0..2).all(|i| b.min[i] <= max[i] && b.max[i] >= min[i])
        })
    }

    /// The rows that stand inside the rectangle `min` to `max` of the ground plane.
    pub fn rows_within(&self, min: [f32; 2], max: [f32; 2]) -> impl Iterator<Item = &PlacementRow> {
        self.maps_within(min, max)
            .flat_map(move |(_, m)| &self.rows[m.rows.clone()])
            .filter(move |r| (0..2).all(|i| r.position[i] >= min[i] && r.position[i] <= max[i]))
    }
}

/// Points of the ground plane by the square cell each stands in, so those of a rectangle are found
/// without a walk of them all; a point is known by its place in the order they were given in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Grid {
    cell: f32,
    cells: HashMap<[i32; 2], Vec<u32>>,
}

impl Grid {
    /// The grid of `points` over cells `cell` metres wide; a point that is not finite is in no cell.
    pub fn new(cell: f32, points: impl IntoIterator<Item = [f32; 2]>) -> Self {
        let cell = if cell.is_finite() && cell > 0.0 {
            cell
        } else {
            1.0
        };
        let mut cells: HashMap<[i32; 2], Vec<u32>> = HashMap::new();
        for (k, p) in points.into_iter().enumerate() {
            if p[0].is_finite() && p[1].is_finite() {
                (cells.entry(Self::cell_at(cell, p)).or_default()).push(k as u32);
            }
        }
        Self { cell, cells }
    }

    // `as` saturates, so a far-off point stands in the outermost cell.
    fn cell_at(cell: f32, p: [f32; 2]) -> [i32; 2] {
        [(p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32]
    }

    /// The width of a cell, in metres.
    pub fn cell(&self) -> f32 {
        self.cell
    }

    /// How many cells hold a point.
    pub fn cells(&self) -> usize {
        self.cells.len()
    }

    /// Each cell that holds a point and meets the rectangle `min` to `max`: its lowest corner and
    /// the points in it, which may stand outside the rectangle.
    pub fn cells_within(&self, min: [f32; 2], max: [f32; 2]) -> Vec<([f32; 2], &[u32])> {
        let (lo, hi) = (Self::cell_at(self.cell, min), Self::cell_at(self.cell, max));
        if (0..2).any(|i| lo[i] > hi[i]) || !min.iter().chain(&max).all(|x| x.is_finite()) {
            return Vec::new();
        }
        let corner = |c: [i32; 2]| [c[0] as f32 * self.cell, c[1] as f32 * self.cell];
        let span: u128 = (0..2)
            .map(|i| (i64::from(hi[i]) - i64::from(lo[i]) + 1) as u128)
            .product();
        // The rectangle's cells are looked up while they are fewer than the cells held.
        if span <= self.cells.len() as u128 {
            (lo[0]..=hi[0])
                .flat_map(|x| (lo[1]..=hi[1]).map(move |y| [x, y]))
                .filter_map(|c| Some((corner(c), self.cells.get(&c)?.as_slice())))
                .collect()
        } else {
            (self.cells.iter())
                .filter(|(c, _)| (0..2).all(|i| c[i] >= lo[i] && c[i] <= hi[i]))
                .map(|(c, held)| (corner(*c), held.as_slice()))
                .collect()
        }
    }
}

#[cfg(test)]
#[path = "map_world_tests.rs"]
mod tests;
