//! The sparse octree of one 128 m navigation cell: a 0x60-byte root, a hash map of 16-slot groups
//! from location codes to node numbers, and one word a node.
//!
//! Not read, since what they hold is not settled: a surface leaf's byte 1 but its bit 0, an open
//! leaf's byte 1 and its layer code's unit, and the x and y order of a code's bits.

use std::collections::HashSet;

use super::bytes::{sections, u32_at, u64_at, Bytes};
use super::{HeaderlessError, Result};
use crate::gta6::hash::fmix64;

/// The table's offset: the groups follow the root.
const TABLE: usize = 0x60;
/// A group: 16 control bytes, then 16 slots of a u64 key and a u64 node number.
const GROUP: usize = 0x110;
const EMPTY: u8 = 0x80;
/// The word of a node with children, and of a placeholder leaf.
const INTERNAL: u32 = 0xFF00;

/// What the octree holds, by node kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavOctree {
    /// `x + 512 y` of the cell whose minimum corner is `corner`'s x and y.
    pub cell: u32,
    /// The root cube's minimum corner; its edge is `edge` metres.
    pub corner: [f32; 3],
    pub edge: u32,
    pub groups: u32,
    pub nodes: u32,
    /// The deepest level of a code, the root at 0.
    pub depth: u32,
    pub internal: u32,
    /// Leaves of byte 0 zero, the word an internal node's.
    pub placeholder: u32,
    /// Leaves of byte 0 one: free space, its byte 1 the medium.
    pub open: u32,
    /// Leaves of byte 0 three: the ground, its byte 1 bit 0 set under water.
    pub surface: u32,
}

pub fn read_octree(virtual_section: &[u8]) -> Result<NavOctree> {
    sections(virtual_section, Some(&[]), |v, _| octree_at(v))
}

fn octree_at(v: Bytes) -> Result<NavOctree> {
    let groups = v.u32("group count", 0x10)?;
    let nodes = v.u32("node count", 0x14)?;
    if !groups.is_power_of_two() {
        return Err(constant("group count", 0x10, groups.into()));
    }
    let again = v.u64("node count", 0x50)?;
    if again != u64::from(nodes) {
        return Err(HeaderlessError::Disagree {
            what: "node counts at 0x14 and 0x50",
            first: nodes.into(),
            second: again,
        });
    }
    let table = v.to("hash table", 0x18)?;
    if table != TABLE {
        return Err(constant("hash table pointer", 0x18, table as u64));
    }
    let edge = v.u32("root edge", 0x28)?;
    for at in [0x2C, 0x30] {
        if v.u32("root edge", at)? != edge {
            return Err(constant(
                "root edge",
                at as u64,
                v.u32("root edge", at)?.into(),
            ));
        }
    }
    if edge < 128 || !edge.is_power_of_two() {
        return Err(constant("root edge", 0x28, edge.into()));
    }
    let words = v.array("node words", v.to("node words", 0x20)?, nodes as usize, 4)?;
    let table = v.array("hash table", table, groups as usize, GROUP)?;
    let mut keys = vec![None; nodes as usize];
    for group in table.as_chunks::<GROUP>().0 {
        for (slot, tag) in group[..16].iter().enumerate() {
            if *tag == EMPTY {
                continue;
            }
            let key = u64_at(group, 16 + 16 * slot);
            let node = u64_at(group, 24 + 16 * slot);
            if *tag >= EMPTY || fmix64(key) & 0x7F != u64::from(*tag) {
                return Err(HeaderlessError::HashTag { key, tag: *tag });
            }
            if key == 0 || !(u64::BITS - 1 - key.leading_zeros()).is_multiple_of(3) {
                return Err(HeaderlessError::OctreeKey { key });
            }
            match keys.get_mut(node as usize) {
                Some(k @ None) => *k = Some(key),
                Some(Some(_)) => {
                    return Err(HeaderlessError::Disagree {
                        what: "node number two keys name",
                        first: node,
                        second: key,
                    })
                }
                None => {
                    return Err(HeaderlessError::IndexPastCount {
                        what: "hash slot's node number",
                        index: node,
                        count: nodes.into(),
                    })
                }
            }
        }
    }
    let keys: Vec<u64> = keys
        .into_iter()
        .map(|k| {
            k.ok_or(HeaderlessError::Disagree {
                what: "full hash slots and the node count",
                first: 0,
                second: nodes.into(),
            })
        })
        .collect::<Result<_>>()?;
    let present: HashSet<u64> = keys.iter().copied().collect();
    let parents: HashSet<u64> = keys.iter().filter(|k| **k != 1).map(|k| k >> 3).collect();
    if let Some(orphan) = keys
        .iter()
        .find(|k| **k != 1 && !present.contains(&(**k >> 3)))
    {
        return Err(HeaderlessError::OctreeKey { key: *orphan });
    }
    let mut tree = NavOctree {
        cell: v.u32("cell", 0x34)?,
        corner: v.vec3("corner", 0x40)?,
        edge,
        groups,
        nodes,
        depth: 0,
        internal: 0,
        placeholder: 0,
        open: 0,
        surface: 0,
    };
    for (node, key) in keys.iter().enumerate() {
        tree.depth = tree.depth.max((u64::BITS - 1 - key.leading_zeros()) / 3);
        let word = u32_at(words, 4 * node);
        let count = match (parents.contains(key), word & 0xFF) {
            (true, _) if word == INTERNAL => &mut tree.internal,
            (false, 0) if word == INTERNAL => &mut tree.placeholder,
            (false, 1) => &mut tree.open,
            (false, 3) => &mut tree.surface,
            _ => return Err(HeaderlessError::NodeWord { node, word }),
        };
        *count += 1;
    }
    Ok(tree)
}

fn constant(what: &'static str, at: u64, found: u64) -> HeaderlessError {
    HeaderlessError::Constant { what, at, found }
}
