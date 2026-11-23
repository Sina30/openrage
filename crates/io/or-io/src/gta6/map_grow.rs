//! A GTA VI map's entities added and removed: `entities` takes or loses a pointer, an added
//! entity's record goes at the end of its source's block, and the sections grow by the resource
//! rule.
//!
//! ⚠ An element's `parentIndex` indexes the map its selector picks from the map's 0x58 list, never
//! its own map: its entities, then its prop lists, then its grass lists. Those maps are other
//! bodies, which [`child_count_changed`], [`parent_index_removed`] and [`parent_index_inserted`]
//! edit one at a time.

use std::collections::HashSet;

use crate::container::rpf::Rpf9Sections;
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::rbm::gta6::read_model;
use crate::meta::binary::rbm::gta6_grow::{
    append_to_block, insert_element, remove_element, Gta6GrowError,
};
use crate::meta::binary::rbm::schema::RbmModel;

use super::links::{self, Links, LodLink};
use super::{
    read, u64_at, Aabb, Entity, MapBody, MapData, MapError, ARRAY, C_ENTITY_DEF, ENTITY_SIZE,
    ORPHAN_HD, PROP_LIST_SIZE,
};

/// `CMapData`'s `entities`, an `AtArray64` of pointers to entity records.
const ENTITIES: usize = 0xD0;
/// `CMapData`'s `entitiesExtentsMin`, with `entitiesExtentsMax` 16 bytes after it.
const ENTITIES_EXTENTS: usize = 0xB0;
/// `CEntityDef`'s `extensions`, an `AtArray64` a copy does not share with its source.
const EXTENSIONS: u32 = 0x28;
/// `CMapData`'s `instancedData.PropInstanceList`, an `AtArray64` of prop list records in place.
const PROP_LISTS: usize = 0x148;
// An entity's or prop list's `id`, `parentIndex`, `hash_E74C3C23`, `numChildren`, `lodLevel` and
// parent-map selector.
const ID: usize = 0x08;
const PARENT_INDEX: usize = 0x5C;
const INSTANCE: usize = 0x5E;
const NUM_CHILDREN: usize = 0x68;
const LOD_LEVEL: usize = 0x7E;
const SELECTOR: usize = 0x7F;
/// `LODTYPES_DEPTH_HD`.
const HD: u8 = 0;

#[derive(Debug, thiserror::Error)]
pub enum MapGrowError {
    #[error(transparent)]
    Map(#[from] MapError),
    #[error(transparent)]
    Grow(#[from] Gta6GrowError),
    #[error("the body is a CMapTypes, which places no entity")]
    NotMapData,
    #[error("entity {index} is past the map's {count}")]
    NoEntity { index: usize, count: usize },
    #[error("entity {0} places an interior, whose entity sets and portals are arrays a copy does not take")]
    Interior(usize),
    #[error("entity {0}'s record is not where its pointer names")]
    Pointer(usize),
    #[error("entity {index} is the level-of-detail parent of {children} elements, which go first")]
    Children { index: usize, children: u32 },
    #[error("entity {0} is a level-of-detail parent, and a copy of it would have no children: it is added only as an orphan")]
    LodSource(usize),
    #[error("parentIndex {0} names no entity of this map: a prop or grass list's numChildren is not known")]
    NotAnEntity(i16),
    #[error("entity {index} holds {children} children, and {by} more or fewer would leave a count no map holds")]
    ChildCount {
        index: usize,
        children: u32,
        by: i32,
    },
    #[error("{what} {index} names element {at} of the map that goes, and an element left with no parent is not written")]
    Orphaned {
        what: &'static str,
        index: usize,
        at: usize,
    },
    #[error("{what} {index}'s parentIndex would pass the 16 bits it is stored in")]
    IndexRange { what: &'static str, index: usize },
    #[error("the {0} is not inside the section its block names")]
    Outside(&'static str),
    #[error("the parent map {0:#010X} is not among the map bodies given")]
    NoParentMap(u32),
    #[error("the edited map does not read back as written: {0}")]
    ReadBack(String),
}

type Result<T> = std::result::Result<T, MapGrowError>;

/// Adds a copy of entity `from` as the map's last entity; returns its index.
// Its extensions are empty and its `id` one no entity of the map holds. An orphan copy names no
// parent and is ORPHANHD. Any other keeps the source's link, which takes
// an HD or orphan source; its parent's numChildren is [`child_count_changed`]'s, in the parent map.
pub fn add_entity(sections: &mut Rpf9Sections, from: usize, orphan: bool) -> Result<usize> {
    let mut s = sections.clone();
    let (model, map) = map_data(&s)?;
    let count = map.entities.len();
    let source = (map.entities.get(from)).ok_or(MapGrowError::NoEntity { index: from, count })?;
    if source.interior {
        return Err(MapGrowError::Interior(from));
    }
    if !orphan && !matches!(source.lod_level, HD | ORPHAN_HD) {
        return Err(MapGrowError::LodSource(from));
    }
    let held: HashSet<u64> = map.entities.iter().map(|e| e.id).collect();
    // At most `count` ids are held, so this stops within `count + 1` steps.
    let mut id = source.id.wrapping_add(1);
    while held.contains(&id) {
        id = id.wrapping_add(1);
    }
    let extensions = lit(b"extensions");
    let declared = model.classes.get(&C_ENTITY_DEF).is_some_and(|c| {
        c.entries.iter().any(|e| {
            e.name_hash == extensions && e.offset == EXTENSIONS && e.data_type.to_byte() == ARRAY
        })
    });
    if !declared {
        return Err(MapError::Layout {
            class: "CEntityDef",
            member: extensions,
            offset: EXTENSIONS,
            code: ARRAY,
        }
        .into());
    }
    let (block, at) = record(&model, from)?;
    let size = ENTITY_SIZE as usize;
    let mut copy = (data(&model, block))
        .and_then(|d| d.get(at..at + size))
        .ok_or(MapGrowError::Pointer(from))?
        .to_vec();
    let extensions_at = EXTENSIONS as usize;
    copy[extensions_at..extensions_at + 16].fill(0);
    copy[ID..ID + 8].copy_from_slice(&id.to_le_bytes());
    let mut expected_entity = source.clone();
    expected_entity.id = id;
    if orphan {
        copy[PARENT_INDEX..PARENT_INDEX + 2].copy_from_slice(&(-1i16).to_le_bytes());
        copy[INSTANCE..INSTANCE + 2].copy_from_slice(&(-1i16).to_le_bytes());
        copy[NUM_CHILDREN..NUM_CHILDREN + 4].fill(0);
        copy[LOD_LEVEL] = ORPHAN_HD;
        copy[SELECTOR] = 0xFF;
        expected_entity.parent_index = -1;
        expected_entity.hash_e74c3c23 = -1;
        expected_entity.num_children = 0;
        expected_entity.lod_level = ORPHAN_HD;
    }
    let offset = append_to_block(&mut s, block, 16, &copy)?;
    let raw = u64::from(block) | (offset as u64) << 20;
    insert_element(
        &mut s,
        model.root_block_id,
        ENTITIES,
        count,
        &raw.to_le_bytes(),
    )?;
    let mut expected = map.clone();
    expected.entities.push(expected_entity);
    check(&s, &expected)?;
    *sections = s;
    Ok(count)
}

/// Removes entity `index`, which is no element's parent.
// Its record stays in its block as bytes nothing names. The extents are kept, a box that still
// holds every entity left.
pub fn remove_entity(sections: &mut Rpf9Sections, index: usize) -> Result<()> {
    let mut s = sections.clone();
    let (model, map) = map_data(&s)?;
    let count = map.entities.len();
    let e = (map.entities.get(index)).ok_or(MapGrowError::NoEntity { index, count })?;
    if e.num_children > 0 {
        return Err(MapGrowError::Children {
            index,
            children: e.num_children,
        });
    }
    remove_element(&mut s, model.root_block_id, ENTITIES, index)?;
    let mut expected = map;
    expected.entities.remove(index);
    check(&s, &expected)?;
    *sections = s;
    Ok(())
}

/// The union of the entities' boxes `placed` gives and the lists' `BatchAABB`s: `entitiesExtents`.
// `None` where `placed` places no box of an entity.
pub fn entities_union(map: &MapData, placed: impl Fn(&Entity) -> Option<Aabb>) -> Option<Aabb> {
    let lists = (map.prop_lists.iter().chain(&map.grass_lists)).map(|l| Some(l.batch_aabb));
    let mut held: Option<Aabb> = None;
    for b in map.entities.iter().map(placed).chain(lists) {
        let b = b?;
        held = Some(match held {
            None => b,
            Some(h) => Aabb {
                min: std::array::from_fn(|a| h.min[a].min(b.min[a])),
                max: std::array::from_fn(|a| h.max[a].max(b.max[a])),
            },
        });
    }
    held
}

/// Writes `extents` as the map's `entitiesExtents`, each corner's fourth lane kept as read.
pub fn set_entities_extents(sections: &mut Rpf9Sections, extents: Aabb) -> Result<()> {
    let mut s = sections.clone();
    let (model, map) = map_data(&s)?;
    for (at, corner) in [
        (ENTITIES_EXTENTS, extents.min),
        (ENTITIES_EXTENTS + 0x10, extents.max),
    ] {
        let bytes: Vec<u8> = corner.iter().flat_map(|f| f.to_le_bytes()).collect();
        put_in(&mut s, &model, model.root_block_id, at, &bytes)?;
    }
    let mut expected = map;
    expected.entities_extents = extents;
    check(&s, &expected)?;
    *sections = s;
    Ok(())
}

/// [`remove_entity`] with the maps of `others` it renumbers; returns which of them changed.
// The parent map's `numChildren` and the later `parentIndex` of each map that names `map`.
// `others` holds every map body that names `map` as a parent, and its own parent map; a refusal
// changes none of them.
pub fn remove_entity_across(
    map: &mut Rpf9Sections,
    others: &mut [Rpf9Sections],
    index: usize,
) -> Result<Vec<usize>> {
    let name = map_data(map)?.1.name;
    let parent = lod_parent(map, index)?;
    let mut m = map.clone();
    remove_entity(&mut m, index)?;
    let work = across(others, parent, -1, |s| parent_index_removed(s, name, index))?;
    *map = m;
    Ok(commit(others, work))
}

/// [`add_entity`] with the maps of `others` it renumbers; returns its index and which changed.
pub fn add_entity_across(
    map: &mut Rpf9Sections,
    others: &mut [Rpf9Sections],
    from: usize,
    orphan: bool,
) -> Result<(usize, Vec<usize>)> {
    let name = map_data(map)?.1.name;
    let parent = match orphan {
        true => None,
        false => lod_parent(map, from)?,
    };
    let mut m = map.clone();
    let at = add_entity(&mut m, from, orphan)?;
    let work = across(others, parent, 1, |s| parent_index_inserted(s, name, at))?;
    *map = m;
    Ok((at, commit(others, work)))
}

/// `others` as the edit leaves them, `None` for one it does not change: the parent map's child
/// count moved `by`, then `renumber` made over each.
fn across(
    others: &[Rpf9Sections],
    parent: Option<(u32, i16)>,
    by: i32,
    renumber: impl Fn(&mut Rpf9Sections) -> Result<usize>,
) -> Result<Vec<Option<Rpf9Sections>>> {
    let mut work: Vec<Option<Rpf9Sections>> = vec![None; others.len()];
    if let Some((parent, parent_index)) = parent {
        let k = (others.iter())
            .position(|s| map_data(s).is_ok_and(|(_, m)| m.name == parent))
            .ok_or(MapGrowError::NoParentMap(parent))?;
        let mut s = others[k].clone();
        child_count_changed(&mut s, parent_index, by)?;
        work[k] = Some(s);
    }
    for (k, other) in others.iter().enumerate() {
        // A map types body names no parent map.
        if let Err(MapGrowError::NotMapData) = map_links(other) {
            continue;
        }
        let mut s = work[k].clone().unwrap_or_else(|| other.clone());
        if renumber(&mut s)? > 0 {
            work[k] = Some(s);
        }
    }
    Ok(work)
}

fn commit(others: &mut [Rpf9Sections], work: Vec<Option<Rpf9Sections>>) -> Vec<usize> {
    let mut changed = Vec::new();
    for (k, s) in work.into_iter().enumerate() {
        if let (Some(s), Some(held)) = (s, others.get_mut(k)) {
            *held = s;
            changed.push(k);
        }
    }
    changed
}

/// The parent map's name and the `parentIndex` entity `index` names there, `None` for no parent.
pub fn lod_parent(sections: &Rpf9Sections, index: usize) -> Result<Option<(u32, i16)>> {
    let (_, l) = map_links(sections)?;
    let count = l.entities.len();
    let link = (l.entities.get(index)).ok_or(MapGrowError::NoEntity { index, count })?;
    let map = link.map.and_then(|m| l.lod_parents.get(m));
    Ok(map.map(|p| (p.name, link.parent_index)))
}

/// Entity `parent_index` given `by` more children, 1 or −1 as a child elsewhere comes or goes.
pub fn child_count_changed(sections: &mut Rpf9Sections, parent_index: i16, by: i32) -> Result<()> {
    let mut s = sections.clone();
    let (model, map) = map_data(&s)?;
    let index = (usize::try_from(parent_index).ok())
        .filter(|k| *k < map.entities.len())
        .ok_or(MapGrowError::NotAnEntity(parent_index))?;
    let mut expected = map.clone();
    let e = &mut expected.entities[index];
    // Only a level-of-detail entity has children, and it has at least one.
    let lod = !matches!(e.lod_level, HD | ORPHAN_HD);
    let now = (u32::try_from(i64::from(e.num_children) + i64::from(by)).ok())
        .filter(|n| lod && *n > 0)
        .ok_or(MapGrowError::ChildCount {
            index,
            children: e.num_children,
            by,
        })?;
    e.num_children = now;
    let (block, at) = record(&model, index)?;
    put_in(&mut s, &model, block, at + NUM_CHILDREN, &now.to_le_bytes())?;
    check(&s, &expected)?;
    *sections = s;
    Ok(())
}

/// The `parentIndex` past entity `index` of the map `parent` lowered as it goes; returns how many.
pub fn parent_index_removed(
    sections: &mut Rpf9Sections,
    parent: u32,
    index: usize,
) -> Result<usize> {
    shift(sections, parent, &|what, k, at| match at.cmp(&index) {
        std::cmp::Ordering::Less => Ok(None),
        std::cmp::Ordering::Equal => Err(MapGrowError::Orphaned { what, index: k, at }),
        std::cmp::Ordering::Greater => Ok(Some(at - 1)),
    })
}

/// The `parentIndex` from `index` on of the map `parent` raised for an entity inserted there.
// An entity appended to a map goes before its prop and grass lists in its index space.
pub fn parent_index_inserted(
    sections: &mut Rpf9Sections,
    parent: u32,
    index: usize,
) -> Result<usize> {
    shift(sections, parent, &|_, _, at| {
        Ok((at >= index).then_some(at + 1))
    })
}

/// What `to` makes of each `parentIndex` that names the map `parent`, given the element's kind,
/// its index and the index it names; `None` keeps it.
type Shift<'a> = &'a dyn Fn(&'static str, usize, usize) -> Result<Option<usize>>;

fn shift(sections: &mut Rpf9Sections, parent: u32, to: Shift<'_>) -> Result<usize> {
    let mut s = sections.clone();
    let (model, l) = map_links(&s)?;
    let Some(selected) = l.lod_parents.iter().position(|p| p.name == parent) else {
        return Ok(0);
    };
    let mut places = Vec::new();
    for (k, link) in l.entities.iter().enumerate() {
        if link.map == Some(selected) {
            places.push(("entity", k, record(&model, k)?, link));
        }
    }
    let root = data(&model, model.root_block_id).ok_or(MapGrowError::Outside("root block"))?;
    let lists = u64_at(root, PROP_LISTS)?;
    for (k, link) in l.prop_lists.iter().enumerate() {
        if link.map == Some(selected) {
            let at = (usize::try_from(lists >> 20).ok())
                .and_then(|first| first.checked_add(PROP_LIST_SIZE as usize * k))
                .ok_or(MapGrowError::Outside("prop list"))?;
            places.push(("prop list", k, ((lists & 0xFFF) as u32, at), link));
        }
    }
    let mut expected = l.clone();
    let mut changed = 0;
    for (what, k, (block, at), link) in places {
        let Some(now) = to(what, k, usize::try_from(link.parent_index).unwrap_or(0))? else {
            continue;
        };
        let now = i16::try_from(now).map_err(|_| MapGrowError::IndexRange { what, index: k })?;
        put_in(&mut s, &model, block, at + PARENT_INDEX, &now.to_le_bytes())?;
        let held: Option<&mut LodLink> = match what {
            "entity" => expected.entities.get_mut(k),
            _ => expected.prop_lists.get_mut(k),
        };
        if let Some(held) = held {
            held.parent_index = now;
        }
        changed += 1;
    }
    let (_, again) = map_links(&s).map_err(|e| MapGrowError::ReadBack(e.to_string()))?;
    if again != expected {
        return Err(MapGrowError::ReadBack(
            "a parentIndex reads back otherwise than it was written".to_string(),
        ));
    }
    *sections = s;
    Ok(changed)
}

fn map_links(s: &Rpf9Sections) -> Result<(RbmModel, links::MapLinks)> {
    let model = read_model(&s.virtual_section, &s.physical).map_err(MapError::from)?;
    match links::read(&model)? {
        Links::Data(l) => Ok((model, l)),
        Links::Types(_) => Err(MapGrowError::NotMapData),
    }
}

/// `bytes` written at `at` of block `block`, in the section its address names.
fn put_in(
    s: &mut Rpf9Sections,
    model: &RbmModel,
    block: u32,
    at: usize,
    bytes: &[u8],
) -> Result<()> {
    let outside = MapGrowError::Outside("record");
    let Some(b) = (i32::try_from(block).ok()).and_then(|id| model.block(id)) else {
        return Err(outside);
    };
    let Some(end) = at
        .checked_add(bytes.len())
        .filter(|end| *end <= b.data.len())
    else {
        return Err(outside);
    };
    let section = match b.data_addr >> 28 {
        5 => &mut s.virtual_section,
        6 => &mut s.physical,
        _ => return Err(outside),
    };
    let from = (b.data_addr & 0x0FFF_FFFF) as usize;
    match section.get_mut(from + at..from + end) {
        Some(dst) => {
            dst.copy_from_slice(bytes);
            Ok(())
        }
        None => Err(outside),
    }
}

fn map_data(s: &Rpf9Sections) -> Result<(RbmModel, MapData)> {
    let model = read_model(&s.virtual_section, &s.physical).map_err(MapError::from)?;
    match read(&model)? {
        MapBody::Data(map) => Ok((model, map)),
        MapBody::Types(_) => Err(MapGrowError::NotMapData),
    }
}

fn data(model: &RbmModel, block: u32) -> Option<&[u8]> {
    let id = i32::try_from(block).ok()?;
    model.block(id).map(|b| b.data.as_slice())
}

/// The block and offset of entity `k`'s record.
fn record(model: &RbmModel, k: usize) -> Result<(u32, usize)> {
    let root = data(model, model.root_block_id).ok_or(MapGrowError::Pointer(k))?;
    let array = u64_at(root, ENTITIES)?;
    let pointers = data(model, (array & 0xFFF) as u32).ok_or(MapGrowError::Pointer(k))?;
    let at = (usize::try_from(array >> 20).ok())
        .and_then(|first| first.checked_add(8 * k))
        .ok_or(MapGrowError::Pointer(k))?;
    let raw = u64_at(pointers, at)?;
    let offset = usize::try_from(raw >> 20).map_err(|_| MapGrowError::Pointer(k))?;
    Ok(((raw & 0xFFF) as u32, offset))
}

fn check(s: &Rpf9Sections, expected: &MapData) -> Result<()> {
    let (_, again) = map_data(s).map_err(|e| MapGrowError::ReadBack(e.to_string()))?;
    match again == *expected {
        true => Ok(()),
        false => Err(MapGrowError::ReadBack(format!(
            "it reads {} entities where {} were written, or another field differs",
            again.entities.len(),
            expected.entities.len()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::super::{C_MAP_DATA, ENTITY, MAP_DATA, MAP_DATA_SIZE};
    use super::*;
    use crate::container::rpf::{read_rpf9_body, write_rpf9_resource};
    use crate::meta::binary::rbm::gta6_grow::tests::{body, member, pointer};

    const POINTER64: u8 = 0x07;
    const ARRAY_INFO: u32 = 0x100;
    /// The type of a block of pointers.
    const POINTERS: u32 = 7;

    /// A class of `members`, each array's element a pointer.
    fn class(name: u32, size: u32, members: &[(u32, u32, u8)]) -> (u32, u32, Vec<[u8; 16]>) {
        let element = members.len() as u16;
        let mut records: Vec<[u8; 16]> = (members.iter())
            .map(|&(hash, offset, code)| {
                let ah = if code == ARRAY { element } else { 0 };
                member(hash, offset, code, ah, 0)
            })
            .collect();
        records.push(member(ARRAY_INFO, 0, POINTER64, 0, 0));
        (name, size, records)
    }

    fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
        buf[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn floats(buf: &mut [u8], at: usize, values: &[f32]) {
        for (i, v) in values.iter().enumerate() {
            put(buf, at + 4 * i, &v.to_le_bytes());
        }
    }

    fn entity(seed: u32, level: u8, parent: i16) -> Vec<u8> {
        let mut r = vec![0u8; ENTITY_SIZE as usize];
        put(&mut r, 0x08, &(u64::from(seed) << 32 | 0xA1).to_le_bytes());
        floats(&mut r, 0x10, &[0.0, 0.0, 0.6, 0.8]);
        put(&mut r, 0x20, &(u64::from(seed) << 32 | 0xB1).to_le_bytes());
        floats(&mut r, 0x38, &[seed as f32, 2.0 * seed as f32, -3.0]);
        put(&mut r, 0x44, &(seed | 0xC1).to_le_bytes());
        put(&mut r, 0x48, &0x18_0000u32.to_le_bytes());
        floats(&mut r, 0x54, &[1.5, 2.5]);
        put(&mut r, 0x5C, &parent.to_le_bytes());
        floats(&mut r, 0x60, &[-1.0, 30.0]);
        r[0x7E] = level;
        // The first parent map where it has a parent, none where not.
        r[0x7F] = if parent < 0 { 0xFF } else { 0 };
        r
    }

    /// A LOD entity with no parent and `children` children.
    fn lod_entity(seed: u32, children: u32) -> Vec<u8> {
        let mut r = entity(seed, 1, -1);
        put(&mut r, NUM_CHILDREN, &children.to_le_bytes());
        r
    }

    /// A map of two entities in one record block: an HD one whose parentIndex is 3 and whose
    /// extensions hold one pointer, and an orphan.
    fn map_body(parent: u32) -> Rpf9Sections {
        let entities = [entity(0x100, 0, 3), entity(0x200, 5, -1)];
        map_of(0x1111_0001, parent, &[0x3333_0001], &entities)
    }

    /// The map `name` whose parent maps are `parents`, of `entities` in one record block, the
    /// first one's extensions holding one pointer.
    fn map_of(name: u32, parent: u32, parents: &[u32], entities: &[Vec<u8>]) -> Rpf9Sections {
        let mut root = vec![0u8; MAP_DATA_SIZE];
        put(&mut root, 0x08, &name.to_le_bytes());
        put(&mut root, 0x10, &0x2222_0000_0000_0002u64.to_le_bytes());
        put(&mut root, 0x68, &parent.to_le_bytes());
        put(&mut root, 0x80, &0x81u32.to_le_bytes());
        floats(
            &mut root,
            0x90,
            &[-100.0, -200.0, -30.0, 0.0, 100.0, 200.0, 30.0],
        );
        floats(&mut root, 0xB0, &[-10.0, -20.0, -3.0, 0.0, 10.0, 20.0, 3.0]);
        let n = entities.len() as u16;
        put(&mut root, ENTITIES, &pointer(2, 0).to_le_bytes());
        put(
            &mut root,
            ENTITIES + 8,
            &[n.to_le_bytes(), n.to_le_bytes()].concat(),
        );
        let pointers: Vec<u8> = (0..entities.len() as u64)
            .flat_map(|k| pointer(3, k * u64::from(ENTITY_SIZE)).to_le_bytes())
            .collect();
        let mut records = entities.concat();
        put(&mut records, 0x28, &pointer(4, 0).to_le_bytes());
        put(&mut records, 0x30, &[1, 0, 1, 0]);
        let mut blocks = vec![
            (C_MAP_DATA, root),
            (POINTERS, pointers),
            (C_ENTITY_DEF, records),
            (POINTERS, pointer(3, 0).to_le_bytes().to_vec()),
        ];
        if !parents.is_empty() {
            let p = parents.len() as u32;
            let root = &mut blocks[0].1;
            put(root, 0x18, &[p.to_le_bytes(), p.to_le_bytes()].concat());
            put(root, 0x20, &pointer(5, 0).to_le_bytes());
            put(root, 0x58, &pointer(6, 0).to_le_bytes());
            put(
                root,
                0x60,
                &[p as u16, p as u16].map(u16::to_le_bytes).concat(),
            );
            let words: Vec<u8> = (parents.iter())
                .flat_map(|n| (u64::from(*n) << 32 | 0x4F).to_le_bytes())
                .collect();
            blocks.push((0x4F, words));
            blocks.push((0x4A, parents.iter().flat_map(|n| n.to_le_bytes()).collect()));
        }
        let mut map_members = MAP_DATA.to_vec();
        map_members.extend([(0x86AA_B4BF, 0x18, 0x5C), (0x75D1_6ADF, 0x58, ARRAY)]);
        let mut entity_members = ENTITY.to_vec();
        entity_members.push((lit(b"extensions"), EXTENSIONS, ARRAY));
        entity_members.push((0x8479_9847, SELECTOR as u32, 0x10));
        body(
            &[
                class(C_MAP_DATA, MAP_DATA_SIZE as u32, &map_members),
                class(C_ENTITY_DEF, ENTITY_SIZE, &entity_members),
            ],
            &blocks,
            1,
        )
    }

    fn links_of(s: &Rpf9Sections) -> links::MapLinks {
        map_links(s).expect("a map's links").1
    }

    fn map(s: &Rpf9Sections) -> MapData {
        map_data(s).expect("a map").1
    }

    /// What a read of the loose file `s` writes returns.
    fn loose(s: &Rpf9Sections) -> Rpf9Sections {
        let file = write_rpf9_resource(s).expect("written");
        let (find, bytes) = read_rpf9_body(Cursor::new(&file), 0, u64::MAX).expect("it reads");
        Rpf9Sections::of(&find, &bytes).expect("a resource")
    }

    /// Entity `k` of `m` as its copy with the id `id` reads.
    fn copied(m: &MapData, k: usize, id: u64) -> super::super::Entity {
        let mut e = m.entities[k].clone();
        e.id = id;
        e
    }

    #[test]
    fn an_added_entity_is_its_sources_copy_at_the_end_and_reads_back_from_the_loose_file() {
        let mut s = map_body(0x7777_0001);
        let before = map(&s);
        assert_eq!(add_entity(&mut s, 0, false).expect("added"), 2);
        let mut expected = before.clone();
        // The next id no entity of the map holds.
        expected
            .entities
            .push(copied(&before, 0, before.entities[0].id + 1));
        assert_eq!(map(&s), expected);
        assert_eq!(links_of(&s).entities[2], links_of(&s).entities[0]);

        let (model, _) = map_data(&s).expect("a map");
        let extensions = |k: usize| {
            let (block, at) = record(&model, k).expect("a record");
            let bytes = data(&model, block).expect("a block");
            bytes[at + 0x28..at + 0x38].to_vec()
        };
        assert_eq!(extensions(2), vec![0; 16]);
        assert_eq!(extensions(0)[..8], pointer(4, 0).to_le_bytes());

        let again = loose(&s);
        assert_eq!(again, s);
        assert_eq!(map(&again), expected);
    }

    #[test]
    fn a_removed_entity_gives_its_pointers_room_back_and_the_rest_read_in_order() {
        let mut s = map_body(0x7777_0001);
        let before = map(&s);
        add_entity(&mut s, 0, false).expect("added");
        let grown = s.virtual_section.len();
        remove_entity(&mut s, 0).expect("removed");
        let mut expected = before.clone();
        let copy = copied(&before, 0, before.entities[0].id + 1);
        expected.entities = vec![before.entities[1].clone(), copy];
        assert_eq!(map(&s), expected);
        // The pointers go back to the 16 bytes they had; the copy's record stays in its block.
        assert_eq!(s.virtual_section.len(), grown - 16);
        assert_eq!(map(&loose(&s)), expected);
    }

    #[test]
    fn a_removal_s_extents_are_the_union_of_the_boxes_left_and_read_back() {
        let mut s = map_body(0x7777_0001);
        remove_entity(&mut s, 0).expect("removed");
        // A box 1 m either side of each origin; a box not known leaves no union.
        let placed = |e: &Entity| {
            Some(Aabb {
                min: e.position.map(|x| x - 1.0),
                max: e.position.map(|x| x + 1.0),
            })
        };
        let m = map(&s);
        assert_eq!(entities_union(&m, |_| None), None);
        let union = entities_union(&m, placed).expect("every box placed");
        let p = m.entities[0].position;
        assert_eq!(union.min, p.map(|x| x - 1.0));
        assert_eq!(union.max, p.map(|x| x + 1.0));
        set_entities_extents(&mut s, union).expect("written");
        let mut expected = m.clone();
        expected.entities_extents = union;
        assert_eq!(map(&s), expected);
        assert_eq!(map(&loose(&s)), expected);
    }

    #[test]
    fn a_parentless_map_removes_any_childless_entity_and_refuses_a_parent_with_children() {
        let entities = [
            entity(0x100, 0, 3),
            entity(0x200, 5, -1),
            lod_entity(0x300, 2),
        ];
        let mut s = map_of(0x1111_0001, 0, &[0x3333_0001], &entities);
        let kept = s.clone();
        let err = remove_entity(&mut s, 2).expect_err("refused").to_string();
        assert!(err.contains("parent of 2 elements"), "{err}");
        let err = add_entity(&mut s, 3, true)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("entity 3 is past the map's 3"), "{err}");
        let err = add_entity(&mut s, 2, false)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("added only as an orphan"), "{err}");
        assert_eq!(s, kept);

        remove_entity(&mut s, 1).expect("removed");
        let held = map(&kept).entities;
        assert_eq!(map(&s).entities, vec![held[0].clone(), held[2].clone()]);
    }

    #[test]
    fn an_orphan_copy_names_no_parent_and_takes_an_id_no_entity_holds() {
        // The second entity holds the id after the first's, so the copy takes the one after that.
        let mut next = entity(0x200, 5, -1);
        let first = u64::from(0x100u32) << 32 | 0xA1;
        put(&mut next, ID, &(first + 1).to_le_bytes());
        let entities = [lod_entity(0x100, 2), next];
        let mut s = map_of(0x1111_0001, 0, &[0x3333_0001], &entities);
        let before = map(&s);
        assert_eq!(add_entity(&mut s, 0, true).expect("added"), 2);
        let mut want = copied(&before, 0, first + 2);
        (want.parent_index, want.hash_e74c3c23) = (-1, -1);
        (want.num_children, want.lod_level) = (0, ORPHAN_HD);
        assert_eq!(map(&s).entities[2], want);
        assert_eq!(links_of(&s).entities[2].map, None);
        assert_eq!(lod_parent(&s, 2).expect("read"), None);
        assert_eq!(map(&loose(&s)).entities[2], want);
    }

    #[test]
    fn a_removed_childs_parent_loses_a_child_and_another_maps_later_elements_are_renumbered() {
        const MAP: u32 = 0x1111_0001;
        const PARENT: u32 = 0x3333_0001;
        let mut m = map_of(
            MAP,
            0,
            &[PARENT],
            &[
                entity(0x100, 0, 0),
                entity(0x101, 0, 0),
                entity(0x102, 5, -1),
            ],
        );
        let mut parent = map_of(PARENT, 0, &[], &[lod_entity(0x300, 2)]);
        let child_entities = [
            entity(0x500, 0, 2),
            entity(0x501, 0, 0),
            entity(0x502, 5, -1),
        ];
        let mut child = map_of(0x5555_0001, 0, &[MAP], &child_entities);

        // Entity 1 of the map goes: its parent is entity 0 of the parent map.
        assert_eq!(lod_parent(&m, 1).expect("read"), Some((PARENT, 0)));
        child_count_changed(&mut parent, 0, -1).expect("one child fewer");
        assert_eq!(map(&parent).entities[0].num_children, 1);
        assert_eq!(
            parent_index_removed(&mut child, MAP, 1).expect("renumbered"),
            1
        );
        let indices = |s: &Rpf9Sections| -> Vec<i16> {
            map(s).entities.iter().map(|e| e.parent_index).collect()
        };
        assert_eq!(indices(&child), [1, 0, -1]);
        remove_entity(&mut m, 1).expect("removed");
        assert_eq!(map(&m).entities.len(), 2);

        // Entity 0 is the parent of the child map's entity 1, so its renumbering is refused.
        let kept = child.clone();
        let err = parent_index_removed(&mut child, MAP, 0)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("entity 1 names element 0"), "{err}");
        assert_eq!(child, kept);
        // A LOD parent keeps one child at least, and a prop list's count is not known.
        let err = child_count_changed(&mut parent, 0, -1)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("holds 1 children"), "{err}");
        assert!(child_count_changed(&mut parent, 1, 1).is_err());
        assert!(
            child_count_changed(&mut m, 0, 1).is_err(),
            "an HD entity has no children"
        );

        // An entity appended at 2 comes before the map's prop lists: every index from 2 on moves.
        assert_eq!(
            parent_index_inserted(&mut child, MAP, 2).expect("renumbered"),
            0
        );
        assert_eq!(
            parent_index_inserted(&mut child, MAP, 0).expect("renumbered"),
            2
        );
        assert_eq!(indices(&child), [2, 1, -1]);
        assert_eq!(
            parent_index_inserted(&mut parent, MAP, 0).expect("none named"),
            0
        );
        assert_eq!(indices(&loose(&child)), [2, 1, -1]);
    }

    #[test]
    fn an_edit_across_maps_writes_the_parent_and_the_children_together_or_none_of_them() {
        const MAP: u32 = 0x1111_0001;
        const PARENT: u32 = 0x3333_0001;
        let mut m = map_of(
            MAP,
            0,
            &[PARENT],
            &[
                entity(0x100, 0, 0),
                entity(0x101, 0, 0),
                entity(0x102, 5, -1),
            ],
        );
        let parent = map_of(PARENT, 0, &[], &[lod_entity(0x300, 2)]);
        let child_entities = [
            entity(0x500, 0, 2),
            entity(0x501, 0, 0),
            entity(0x502, 5, -1),
        ];
        let child = map_of(0x5555_0001, 0, &[MAP], &child_entities);
        let indices = |s: &Rpf9Sections| -> Vec<i16> {
            map(s).entities.iter().map(|e| e.parent_index).collect()
        };

        // Without its parent map the removal is refused, and nothing is written.
        let kept = m.clone();
        let mut alone = [child.clone()];
        let err = remove_entity_across(&mut m, &mut alone, 1).expect_err("refused");
        assert!(err.to_string().contains("0x33330001"), "{err}");
        assert_eq!((&m, &alone[0]), (&kept, &child));

        let mut others = [parent, child];
        let changed = remove_entity_across(&mut m, &mut others, 1).expect("removed");
        assert_eq!(changed, [0, 1]);
        assert_eq!(map(&m).entities.len(), 2);
        assert_eq!(map(&others[0]).entities[0].num_children, 1);
        assert_eq!(indices(&others[1]), [1, 0, -1]);

        // A sibling copy of entity 0 goes at 2: its parent gains a child, and no child map's
        // index reaches 2.
        let (at, changed) = add_entity_across(&mut m, &mut others, 0, false).expect("added");
        assert_eq!((at, changed), (2, vec![0]));
        assert_eq!(map(&others[0]).entities[0].num_children, 2);
        // An orphan copy names no parent; the child map's indices from 3 on would move.
        let (at, changed) = add_entity_across(&mut m, &mut others, 0, true).expect("added");
        assert_eq!((at, changed), (3, vec![]));
        assert_eq!(indices(&others[1]), [1, 0, -1]);
    }
}
