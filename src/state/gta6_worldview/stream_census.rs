//! What a selection holds, counted for the check against what the game loads: the maps by name,
//! and the nodes of the loaded maps and those drawn by kind and level.

use std::collections::HashMap;

use or_world::streaming::{Forest, Level, Selection};

use super::read::{Instances, Loaded};

/// A selection's census; a kind is an entity, a prop list, a grass list or a far grass list, in that order.
#[derive(Debug, Clone, Default)]
pub struct Census {
    /// The names of the maps wanted, their parents among them.
    pub wanted: Vec<u32>,
    /// The names of the maps loaded.
    pub loaded: Vec<u32>,
    /// The nodes of the loaded maps, by kind and by level as a map numbers it.
    pub nodes: [[usize; Level::COUNT]; 4],
    /// The nodes that draw, by kind and level.
    pub drawn: [[usize; Level::COUNT]; 4],
    /// The instances the lists that draw put in the selection.
    pub list_instances: usize,
}

/// The kind of the node at `at` of `map`.
fn kind(map: &Loaded, at: usize) -> usize {
    let Some(list) = at.checked_sub(map.entities.len()) else {
        return 0;
    };
    match (list < map.props, map.lists.get(list).map(|l| &l.instances)) {
        (true, _) => 1,
        (false, Some(Instances::Cards(_))) => 3,
        _ => 2,
    }
}

/// The census of `selection`, made of the maps `wanted` and `loaded` and the nodes `forest` holds.
// ponytail: every loaded node is looked up at each selection; count on insert if a selection shows it.
pub(super) fn count(
    wanted: &HashMap<u32, f32>,
    loaded: &HashMap<u32, Loaded>,
    forest: &Forest,
    selection: &Selection,
    list_instances: usize,
) -> Census {
    let mut census = Census {
        wanted: wanted.keys().copied().collect(),
        loaded: loaded.keys().copied().collect(),
        list_instances,
        ..Census::default()
    };
    let level = |key: u64, at: u32| forest.node((key, at)).map(|node| node.level as usize);
    for (name, map) in loaded {
        let key = u64::from(*name);
        for at in 0..map.drawn.len() {
            if let Some(level) = level(key, at as u32) {
                census.nodes[kind(map, at)][level] += 1;
            }
        }
    }
    let drawn = (selection.drawn.iter().map(|d| d.node)).chain(selection.lists.iter().map(|l| l.node));
    for (key, at) in drawn {
        let map = loaded.get(&(key as u32));
        if let (Some(map), Some(level)) = (map, level(key, at)) {
            census.drawn[kind(map, at as usize)][level] += 1;
        }
    }
    census
}
