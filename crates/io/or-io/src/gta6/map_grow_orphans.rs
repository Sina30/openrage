//! A level-of-detail parent removed with its children: each child, in its own map or another,
//! takes the form a parentless entity of its level holds.
//!
//! ⚠ An entity with selector −1 and a `parentIndex` names an earlier entity of its own map, so a
//! removal there lowers every such index above the entity that goes.

use crate::container::rpf::Rpf9Sections;
use crate::meta::binary::rbm::gta6_grow::remove_element;
use crate::meta::binary::rbm::schema::RbmModel;

use super::super::links::{LodLink, MapLinks};
use super::super::{Entity, ORPHAN_HD};
use super::{
    check, data, map_data, map_links, put_in, record, MapGrowError, Result, ENTITIES, HD, INSTANCE,
    LOD_LEVEL, NUM_CHILDREN, PARENT_INDEX, SELECTOR,
};

/// `contentFlags` bit 15, which no map that selects no parent map holds.
const CONTENT_BIT_15: u32 = 1 << 15;
/// The bytes of a `CEntityDef` an orphan's form is judged by.
const READ: usize = 0x84;

/// Entity `index` removed, its children in its own map made parentless and the later own-map
/// links lowered; `elsewhere` counts its children in other maps, `None` where none were looked for.
pub(super) fn remove_own(
    sections: &mut Rpf9Sections,
    index: usize,
    elsewhere: Option<u32>,
) -> Result<()> {
    let mut s = sections.clone();
    let (model, map) = map_data(&s)?;
    let (_, l) = map_links(&s)?;
    let count = map.entities.len();
    let e = (map.entities.get(index)).ok_or(MapGrowError::NoEntity { index, count })?;
    let link = *(l.entities.get(index)).ok_or(MapGrowError::NoEntity { index, count })?;
    let own = own_children(&l, index);
    let found = u32::try_from(own.len())
        .unwrap_or(u32::MAX)
        .saturating_add(elsewhere.unwrap_or(0));
    if found != e.num_children {
        return Err(match elsewhere {
            None => MapGrowError::Children {
                index,
                children: e.num_children,
            },
            Some(_) => MapGrowError::ChildrenNotFound {
                index,
                children: e.num_children,
                found,
            },
        });
    }
    let mut expected = map.clone();
    let mut links = l.clone();
    for &k in &own {
        let (Some(e), Some(link)) = (expected.entities.get_mut(k), links.entities.get_mut(k))
        else {
            return Err(MapGrowError::Pointer(k));
        };
        orphan(&mut s, &model, map.name, k, e, link)?;
    }
    // An in-map parent loses the child that goes.
    if link.map.is_none() && link.parent_index >= 0 {
        let p = (usize::try_from(link.parent_index).ok())
            .filter(|p| *p < count)
            .ok_or(MapGrowError::NotAnEntity(link.parent_index))?;
        let held = expected.entities[p].num_children;
        let Some(now) = held.checked_sub(1).filter(|n| *n > 0) else {
            return Err(MapGrowError::ChildCount {
                index: p,
                children: held,
                by: -1,
            });
        };
        expected.entities[p].num_children = now;
        let (block, at) = record(&model, p)?;
        put_in(&mut s, &model, block, at + NUM_CHILDREN, &now.to_le_bytes())?;
    }
    for (k, link) in l.entities.iter().enumerate() {
        let later = usize::try_from(link.parent_index).is_ok_and(|at| at > index);
        if link.map.is_some() || !later {
            continue;
        }
        let now = link.parent_index - 1;
        let (block, at) = record(&model, k)?;
        put_in(&mut s, &model, block, at + PARENT_INDEX, &now.to_le_bytes())?;
        if let (Some(e), Some(held)) = (expected.entities.get_mut(k), links.entities.get_mut(k)) {
            (e.parent_index, held.parent_index) = (now, now);
        }
    }
    remove_element(&mut s, model.root_block_id, ENTITIES, index)?;
    expected.entities.remove(index);
    links.entities.remove(index);
    check(&s, &expected)?;
    links_hold(&s, &links)?;
    *sections = s;
    Ok(())
}

/// How many entities of `others` pick the map `parent` and name its entity `index`.
pub(super) fn children_across(others: &[Rpf9Sections], parent: u32, index: usize) -> Result<u32> {
    let mut found = 0usize;
    for other in others {
        match map_links(other) {
            Ok((_, l)) => found += selected_children(&l, parent, index).len(),
            Err(MapGrowError::NotMapData) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(u32::try_from(found).unwrap_or(u32::MAX))
}

/// The entities that pick the map `parent` and name its entity `index` made parentless; returns
/// how many.
pub(super) fn orphan_selected(
    sections: &mut Rpf9Sections,
    parent: u32,
    index: usize,
) -> Result<usize> {
    let mut s = sections.clone();
    let (model, map) = map_data(&s)?;
    let (_, l) = map_links(&s)?;
    let children = selected_children(&l, parent, index);
    if children.is_empty() {
        return Ok(0);
    }
    let mut expected = map.clone();
    let mut links = l.clone();
    for &k in &children {
        let (Some(e), Some(link)) = (expected.entities.get_mut(k), links.entities.get_mut(k))
        else {
            return Err(MapGrowError::Pointer(k));
        };
        orphan(&mut s, &model, map.name, k, e, link)?;
    }
    let selects = (links.entities.iter().chain(&links.prop_lists)).any(|k| k.map.is_some());
    if !selects && map.content_flags & CONTENT_BIT_15 != 0 {
        return Err(MapGrowError::SelectsNothing {
            map: map.name,
            held: "its contentFlags hold bit 15",
        });
    }
    if !selects && map.parent != 0 {
        return Err(MapGrowError::SelectsNothing {
            map: map.name,
            held: "its parent word is set",
        });
    }
    check(&s, &expected)?;
    links_hold(&s, &links)?;
    *sections = s;
    Ok(children.len())
}

/// The entities of `l` with selector −1 whose `parentIndex` is `index`.
fn own_children(l: &MapLinks, index: usize) -> Vec<usize> {
    (l.entities.iter().enumerate())
        .filter(|(_, k)| k.map.is_none() && usize::try_from(k.parent_index).ok() == Some(index))
        .map(|(k, _)| k)
        .collect()
}

/// The entities of `l` that pick the map `parent` and name its entity `index`.
fn selected_children(l: &MapLinks, parent: u32, index: usize) -> Vec<usize> {
    let Some(selected) = l.lod_parents.iter().position(|p| p.name == parent) else {
        return Vec::new();
    };
    (l.entities.iter().enumerate())
        .filter(|(_, k)| {
            k.map == Some(selected) && usize::try_from(k.parent_index).ok() == Some(index)
        })
        .map(|(k, _)| k)
        .collect()
}

/// Entity `k`'s record written parentless: an HD entity becomes ORPHANHD, another keeps its
/// level and `numChildren`, and no other byte changes.
fn orphan(
    s: &mut Rpf9Sections,
    model: &RbmModel,
    map: u32,
    k: usize,
    e: &mut Entity,
    link: &mut LodLink,
) -> Result<()> {
    let (block, at) = record(model, k)?;
    let bytes = (data(model, block))
        .and_then(|d| d.get(at..at + READ))
        .ok_or(MapGrowError::Pointer(k))?;
    if e.lod_level == HD && !shipped_orphan(e, bytes) {
        return Err(MapGrowError::OrphanForm { map, index: k });
    }
    let none = (-1i16).to_le_bytes();
    put_in(s, model, block, at + PARENT_INDEX, &none)?;
    put_in(s, model, block, at + INSTANCE, &none)?;
    put_in(s, model, block, at + SELECTOR, &[0xFF])?;
    if e.lod_level == HD {
        put_in(s, model, block, at + LOD_LEVEL, &[ORPHAN_HD])?;
        e.lod_level = ORPHAN_HD;
    }
    (e.parent_index, e.hash_e74c3c23) = (-1, -1);
    *link = LodLink {
        map: None,
        parent_index: -1,
        instance: -1,
    };
    Ok(())
}

/// Whether an HD entity's other fields are ones a shipped ORPHANHD holds.
// No ORPHANHD holds a `numChildren`, flag bit 15, or together +0x7D 0, +0x82 0, a `childLodDist`
// of 0 or less, flag bits 19 and 20 set, bit 7 clear and a `lodDist` of −1.
fn shipped_orphan(e: &Entity, record: &[u8]) -> bool {
    let bit = |b: u32| e.flags & (1 << b) != 0;
    let signature = record.get(0x7D) == Some(&0)
        && record.get(0x82..0x84) == Some(&[0, 0][..])
        && e.child_lod_dist <= 0.0
        && bit(19)
        && bit(20)
        && !bit(7)
        && e.lod_dist.to_bits() == (-1.0f32).to_bits();
    e.num_children == 0 && !bit(15) && !signature
}

fn links_hold(s: &Rpf9Sections, expected: &MapLinks) -> Result<()> {
    let (_, again) = map_links(s).map_err(|e| MapGrowError::ReadBack(e.to_string()))?;
    match again == *expected {
        true => Ok(()),
        false => Err(MapGrowError::ReadBack(
            "a parent link reads back otherwise than it was written".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entity, links_of, lod_entity, loose, map, map_of, put};
    use super::super::{remove_entity, remove_entity_across};
    use super::*;

    const MAP: u32 = 0x1111_0001;
    const PARENT: u32 = 0x3333_0001;
    const CHILD: u32 = 0x5555_0001;

    /// An entity record of level `level` with selector −1 and in-map `parentIndex` `parent`.
    fn in_map(seed: u32, level: u8, parent: i16, children: u32) -> Vec<u8> {
        let mut r = lod_entity(seed, children);
        r[LOD_LEVEL] = level;
        put(&mut r, PARENT_INDEX, &parent.to_le_bytes());
        r
    }

    /// A LOD entity whose parent is entity 0 of the first parent map, with `children` children.
    fn parented_lod(seed: u32, children: u32) -> Vec<u8> {
        let mut r = entity(seed, 1, 0);
        put(&mut r, NUM_CHILDREN, &children.to_le_bytes());
        r
    }

    fn content_flags(s: &mut Rpf9Sections, flags: u32) {
        let (model, _) = map_data(s).expect("a map");
        put_in(s, &model, model.root_block_id, 0x84, &flags.to_le_bytes()).expect("written");
    }

    /// The map, its parent map, and a child map whose entities are `children`.
    fn three(children: &[Vec<u8>], parent_word: u32) -> (Rpf9Sections, [Rpf9Sections; 2]) {
        let m = map_of(
            MAP,
            0,
            &[PARENT],
            &[parented_lod(0x100, 2), parented_lod(0x101, 1)],
        );
        let parent = map_of(PARENT, 0, &[], &[lod_entity(0x300, 3)]);
        (m, [parent, map_of(CHILD, parent_word, &[MAP], children)])
    }

    #[test]
    fn a_removed_parent_s_children_elsewhere_take_the_parentless_form_and_the_rest_renumber() {
        let children = [
            entity(0x500, 0, 0),
            entity(0x501, 0, 0),
            entity(0x502, 0, 1),
        ];
        let (mut m, mut others) = three(&children, 0);

        // Without the child map its children are not found, and nothing is written.
        let kept = (m.clone(), others[0].clone());
        let mut alone = [others[0].clone()];
        let err = remove_entity_across(&mut m, &mut alone, 0)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("holds 2 children, and 0 are found"), "{err}");
        assert_eq!((&m, &alone[0]), (&kept.0, &kept.1));

        let before = map(&others[1]);
        let listed = links_of(&others[1]).lod_parents;
        let changed = remove_entity_across(&mut m, &mut others, 0).expect("removed");
        assert_eq!(changed, [0, 1]);
        assert_eq!(map(&m).entities.len(), 1);
        assert_eq!(map(&others[0]).entities[0].num_children, 2);
        let mut want = before.clone();
        for e in &mut want.entities[..2] {
            (e.parent_index, e.hash_e74c3c23, e.lod_level) = (-1, -1, ORPHAN_HD);
        }
        want.entities[2].parent_index = 0;
        assert_eq!(map(&others[1]), want);
        let l = links_of(&others[1]);
        assert_eq!(l.entities[0].map, None);
        assert_eq!(l.entities[2].map, Some(0));
        assert_eq!(l.lod_parents, listed, "the 0x58 list is kept");
        assert_eq!(map(&loose(&others[1])), want);
    }

    #[test]
    fn a_child_map_left_selecting_nothing_is_refused_where_its_parent_word_or_bit_15_is_set() {
        let one = [entity(0x500, 0, 1)];
        for (word, flags, why) in [(MAP, 0, "parent word"), (0, CONTENT_BIT_15, "bit 15")] {
            let (mut m, mut others) = three(&one, word);
            content_flags(&mut others[1], flags);
            let kept = others.clone();
            let err = remove_entity_across(&mut m, &mut others, 1)
                .expect_err("refused")
                .to_string();
            assert!(err.contains(why), "{err}");
            assert_eq!(others, kept);
        }
        let (mut m, mut others) = three(&one, 0);
        remove_entity_across(&mut m, &mut others, 1).expect("removed");
        let l = links_of(&others[1]);
        assert_eq!(l.entities[0].map, None);
        assert_eq!(l.lod_parents.len(), 1, "the entry no element selects stays");
    }

    #[test]
    fn an_hd_child_whose_fields_no_orphan_holds_and_a_grandparent_left_childless_are_refused() {
        let mut odd = entity(0x500, 0, 1);
        put(&mut odd, 0x64, &0f32.to_le_bytes());
        let (mut m, mut others) = three(&[odd], 0);
        let err = remove_entity_across(&mut m, &mut others, 1)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("no shipped ORPHANHD"), "{err}");

        let (mut m, [_, child]) = three(&[entity(0x500, 0, 1)], 0);
        let mut others = [map_of(PARENT, 0, &[], &[lod_entity(0x300, 1)]), child];
        let err = remove_entity_across(&mut m, &mut others, 1)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("holds 1 children"), "{err}");
    }

    #[test]
    fn an_in_map_child_goes_parentless_and_later_in_map_links_lower_with_its_parent() {
        // Level 6, a level 4 under it, a level 3 under that, all in one map with no parent maps.
        let entities = [
            in_map(0x100, 6, -1, 1),
            in_map(0x101, 4, 0, 1),
            in_map(0x102, 3, 1, 1),
        ];
        let s = map_of(MAP, 0, &[], &entities);

        // Entity 1 goes: entity 2 is its child, and entity 0 would be left with no child.
        let mut work = s.clone();
        let err = remove_entity(&mut work, 1)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("entity 0 holds 1 children"), "{err}");
        assert_eq!(work, s);

        let before = map(&s);
        remove_entity(&mut work, 0).expect("removed");
        let mut want = vec![before.entities[1].clone(), before.entities[2].clone()];
        (want[0].parent_index, want[0].hash_e74c3c23) = (-1, -1);
        want[1].parent_index = 0;
        assert_eq!(map(&work).entities, want);
        assert_eq!(map(&work).entities[0].lod_level, 4, "a LOD level is kept");
        assert_eq!(map(&work).entities[0].num_children, 1);
        assert_eq!(links_of(&work).entities[1].parent_index, 0);
        assert_eq!(map(&loose(&work)).entities, want);

        // A level 3 whose child is in another map is refused alone.
        let err = remove_entity(&mut work, 1)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("parent of 1 elements"), "{err}");
    }
}
