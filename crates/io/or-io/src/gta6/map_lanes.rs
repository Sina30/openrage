//! GTA VI's vehicle lane graph: the node list, the link list and the junction list, each the root
//! of its own headerless RBM body, and the three joined into one graph.
//!
//! The lists' own classes have no name here, so each is known by its hash and a field by the role
//! it was measured to play; a field whose role is not known keeps its hash.
//!
//! ⚠ A node has two addresses. A link names its target by the one the `CPathNode` holds
//! ([`PathNode::address`]); a junction, an entrance and a turn name a node by the one its list
//! element holds after it ([`PathNode::junction_address`]), which two nodes can share and whose
//! region is the square of the ground the node stands in.

use std::cell::Cell;
use std::collections::HashMap;

use super::lights::{check_table, Layouts};
use super::{
    broken, field, u32_at, u64_at, Blocks, MapError, FLOAT, STRING_HASH, STRUCT, U32, VEC3,
};
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::rbm::schema::RbmModel;

/// The root classes of the three bodies.
pub const NODE_LIST: u32 = 0x8F85_8F02;
pub const LINK_LIST: u32 = 0xC88E_AAF8;
pub const JUNCTION_LIST: u32 = 0x73B2_4A10;

const NODE_ELEMENT: u32 = 0x0C69_A100;
const C_PATH_NODE: u32 = lit(b"CPathNode");
const C_NODE_ADDRESS: u32 = lit(b"CNodeAddress");
const LINK: u32 = 0x0F2D_A4C2;
const JUNCTION: u32 = 0xEBA8_AEBB;
const C_JUNCTION_ENTRANCE: u32 = lit(b"CJunctionEntrance");
const TURN: u32 = 0xBADD_6DEF;
const PHASE: u32 = lit(b"CTrafficLightPhase");
const LIGHT_BOX: u32 = lit(b"CTrafficLightBox");

const BOOL: u8 = 0x01;
const U8: u8 = 0x11;
const U16: u8 = 0x13;
const U64: u8 = 0x17;
const COUNTED: u8 = 0x5C;
const BITSET32: u8 = 0x65;

const NODE_SIZE: usize = 0x3C;
const LINK_SIZE: usize = 0x14;
const JUNCTION_SIZE: usize = 0x58;
const ENTRANCE_SIZE: usize = 0x50;
const TURN_SIZE: usize = 0xB0;
const PHASE_SIZE: usize = 0x18;
const LIGHT_BOX_SIZE: usize = 0x10;

const LAYOUTS: &Layouts = &[
    (
        NODE_LIST,
        "the node list",
        &[(lit(b"Nodes"), 0x00, COUNTED)],
        Some(0x10),
    ),
    (
        LINK_LIST,
        "the link list",
        &[(lit(b"Links"), 0x00, COUNTED)],
        Some(0x10),
    ),
    (
        JUNCTION_LIST,
        "the junction list",
        &[(0x0BB6_5C7A, 0x00, COUNTED)],
        Some(0x10),
    ),
    (
        NODE_ELEMENT,
        "the node list's element",
        &[
            (lit(b"Node"), 0x00, STRUCT),
            (0x7748_7F26, 0x34, STRUCT),
            (0x7C2B_FB66, 0x38, U32),
        ],
        Some(NODE_SIZE as u32),
    ),
    (
        C_PATH_NODE,
        "CPathNode",
        &[
            (lit(b"Position"), 0x00, VEC3),
            (lit(b"Flags"), 0x0C, BITSET32),
            (lit(b"Address"), 0x14, STRUCT),
            (0x3374_D50E, 0x18, STRUCT),
            (0x2522_B86A, 0x1C, STRUCT),
            (lit(b"StreetName"), 0x20, STRING_HASH),
        ],
        Some(0x34),
    ),
    (
        C_NODE_ADDRESS,
        "CNodeAddress",
        &[(lit(b"Region"), 0x00, U16), (lit(b"Index"), 0x02, U16)],
        Some(4),
    ),
    (
        LINK,
        "the link",
        &[
            (lit(b"OtherNode"), 0x00, STRUCT),
            (lit(b"Distance"), 0x04, U16),
            (lit(b"LanesFromOtherNode"), 0x06, U8),
            (lit(b"LanesToOtherNode"), 0x07, U8),
            (lit(b"Flags"), 0x0C, BITSET32),
        ],
        Some(LINK_SIZE as u32),
    ),
    (
        JUNCTION,
        "the junction",
        &[
            (lit(b"junctionNodeAddress"), 0x00, STRUCT),
            (lit(b"entrances"), 0x08, COUNTED),
            (lit(b"phases"), 0x28, COUNTED),
        ],
        Some(JUNCTION_SIZE as u32),
    ),
    (
        C_JUNCTION_ENTRANCE,
        "CJunctionEntrance",
        &[
            (0x244A_2817, 0x08, COUNTED),
            (lit(b"nodeAddress"), 0x18, STRUCT),
            (lit(b"bIsStop"), 0x47, BOOL),
            (lit(b"bIsYield"), 0x48, BOOL),
        ],
        Some(ENTRANCE_SIZE as u32),
    ),
    (
        TURN,
        "the turn",
        &[
            (lit(b"nodeAddress"), 0x78, STRUCT),
            (lit(b"fPathLength"), 0x8C, FLOAT),
            (0x07FA_0E56, 0x90, VEC3),
            (0x1484_276A, 0x9C, VEC3),
        ],
        Some(TURN_SIZE as u32),
    ),
    (
        PHASE,
        "CTrafficLightPhase",
        &[
            (lit(b"fDuration"), 0x00, FLOAT),
            (lit(b"trafficLightBoxes"), 0x08, COUNTED),
        ],
        Some(PHASE_SIZE as u32),
    ),
    (
        LIGHT_BOX,
        "CTrafficLightBox",
        &[(lit(b"entityId"), 0x00, U64), (lit(b"boxId"), 0x08, U8)],
        Some(LIGHT_BOX_SIZE as u32),
    ),
];

/// Why a lane-graph body is not read, or the three do not join.
#[derive(Debug, thiserror::Error)]
pub enum LaneError {
    #[error(transparent)]
    Body(#[from] MapError),
    #[error(
        "the root structure {0:#010X} is none of the lane graph's node, link and junction lists"
    )]
    NotALaneBody(u32),
    #[error(
        "node {node}'s first link {first} is before the node before it's or past the {links} links"
    )]
    FirstLink {
        node: usize,
        first: u32,
        links: usize,
    },
    #[error("link {link} names a node at {target:?}, an address no node has")]
    LinkTarget { link: usize, target: NodeAddress },
    #[error("junction {junction} names a node at {address:?}, an address no node has")]
    JunctionNode {
        junction: usize,
        address: NodeAddress,
    },
}

type Result<T> = std::result::Result<T, LaneError>;

/// A `CNodeAddress`: its `Region`, then its `Index` among the nodes of that region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeAddress(pub u16, pub u16);

impl NodeAddress {
    pub fn region(self) -> u16 {
        self.0
    }

    pub fn index(self) -> u16 {
        self.1
    }
}

/// A `CPathNode` with what its list element holds after it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathNode {
    /// In the world (`Position`); it can be not finite, and such a node stands nowhere.
    pub position: [f32; 3],
    /// The bits of `Flags`, 51 of them.
    pub flags: u64,
    /// The address a link names this node by (`Address`): region 0, and its index in the list.
    pub address: NodeAddress,
    pub hash_2522b86a: NodeAddress,
    pub street_name: u32,
    /// The address a junction, an entrance and a turn name this node by (the element's
    /// `hash_77487F26`, and the node's `hash_3374D50E`).
    pub junction_address: NodeAddress,
    /// The index of this node's first link; its links run to the next node's first.
    pub first_link: u32,
}

/// A link from the node that holds it to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    /// The node it leads to (`OtherNode`).
    pub target: NodeAddress,
    /// Its length in metres (`Distance`).
    pub distance: u16,
    /// The lanes that come from the target and those that go to it; the link back swaps the two.
    pub lanes_from_other_node: u8,
    pub lanes_to_other_node: u8,
    /// The bits of `Flags`.
    pub flags: u32,
}

/// A path across a junction from one of its entrances.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turn {
    /// The node it names (`nodeAddress`).
    pub node: NodeAddress,
    pub path_length: f32,
    /// The two ends of the path (`hash_07FA0E56`, `hash_1484276A`).
    pub ends: [[f32; 3]; 2],
}

/// A `CJunctionEntrance`.
#[derive(Debug, Clone, PartialEq)]
pub struct Entrance {
    /// The node it names (`nodeAddress`).
    pub node: NodeAddress,
    pub is_stop: bool,
    pub is_yield: bool,
    pub turns: Vec<Turn>,
}

/// One box of traffic lights a phase names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightBox {
    pub entity_id: u64,
    pub box_id: u8,
}

/// A phase of a junction's traffic lights.
#[derive(Debug, Clone, PartialEq)]
pub struct Phase {
    /// In seconds.
    pub duration: f32,
    pub light_boxes: Vec<LightBox>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Junction {
    /// The node it names (`junctionNodeAddress`).
    pub node: NodeAddress,
    pub entrances: Vec<Entrance>,
    pub phases: Vec<Phase>,
}

impl Junction {
    /// Whether traffic lights run the junction: it has a phase.
    pub fn is_signalised(&self) -> bool {
        !self.phases.is_empty()
    }
}

/// What a lane-graph body holds.
#[derive(Debug, Clone, PartialEq)]
pub enum LaneBody {
    Nodes(Vec<PathNode>),
    Links(Vec<Link>),
    Junctions(Vec<Junction>),
}

/// Whether `root`, a body's root class, is one of the three lists.
pub fn is_lane_root(root: u32) -> bool {
    [NODE_LIST, LINK_LIST, JUNCTION_LIST].contains(&root)
}

/// The lane-graph list a headerless RBM resource holds, from its two inflated sections.
pub fn open(virtual_section: &[u8], physical: &[u8]) -> Result<LaneBody> {
    let model = crate::meta::binary::rbm::gta6::read_model(virtual_section, physical)
        .map_err(MapError::from)?;
    read(&model)
}

/// The lane-graph list of a headerless RBM resource's class table and data blocks.
pub fn read(model: &RbmModel) -> Result<LaneBody> {
    check_table(model, LAYOUTS)?;
    let blocks = Blocks::new(&model.data_blocks);
    let root = blocks.block(u64::from(model.root_block_id), "the root block")?;
    // A record is at least four bytes, so the lists of a body that aliases no block fit this.
    let lists = Lists {
        blocks: &blocks,
        left: Cell::new(
            model
                .data_blocks
                .iter()
                .map(|b| b.data.len())
                .sum::<usize>()
                / 4,
        ),
    };
    let records =
        |class: u32, size: usize, what: &str| lists.records(&root.data, 0, class, size, what);
    Ok(match root.data_type {
        NODE_LIST => LaneBody::Nodes(
            (records(NODE_ELEMENT, NODE_SIZE, "the nodes")?.into_iter())
                .map(node)
                .collect::<super::Result<_>>()?,
        ),
        LINK_LIST => LaneBody::Links(
            (records(LINK, LINK_SIZE, "the links")?.into_iter())
                .map(link)
                .collect::<super::Result<_>>()?,
        ),
        JUNCTION_LIST => LaneBody::Junctions(
            (records(JUNCTION, JUNCTION_SIZE, "the junctions")?.into_iter())
                .enumerate()
                .map(|(k, j)| junction(&lists, j, &format!("junctions[{k}]")))
                .collect::<super::Result<_>>()?,
        ),
        other => return Err(LaneError::NotALaneBody(other)),
    })
}

/// The blocks of a body and how many more records its counted lists may name.
struct Lists<'b, 'm> {
    blocks: &'b Blocks<'m>,
    left: Cell<usize>,
}

impl<'m> Lists<'_, 'm> {
    /// The in-place records of `size` bytes of `class` a counted list at `at` holds: a count, the
    /// same count again, then the pointer.
    fn records(
        &self,
        bytes: &[u8],
        at: usize,
        class: u32,
        size: usize,
        what: &str,
    ) -> super::Result<Vec<&'m [u8]>> {
        let count = u32_at(bytes, at)?;
        if u32_at(bytes, at + 4)? != count {
            return Err(broken(what, "its two counts differ"));
        }
        let count = usize::try_from(count).unwrap_or(usize::MAX);
        let Some(left) = self.left.get().checked_sub(count) else {
            return Err(broken(
                what,
                "the lists of this body name more records than its blocks hold",
            ));
        };
        self.left.set(left);
        match self.blocks.target(u64_at(bytes, at + 8)?, what)? {
            None if count == 0 => Ok(Vec::new()),
            None => Err(broken(
                what,
                format!("{count} records are behind a null pointer"),
            )),
            Some((kind, rest)) if kind != class || rest.len() / size < count => Err(broken(
                what,
                format!("its {count} records do not fit a block of their type, {class:#010X}"),
            )),
            Some((_, rest)) => Ok(rest.chunks_exact(size).take(count).collect()),
        }
    }
}

fn address(bytes: &[u8], at: usize) -> super::Result<NodeAddress> {
    Ok(NodeAddress(
        u16::from_le_bytes(field(bytes, at)?),
        u16::from_le_bytes(field(bytes, at + 2)?),
    ))
}

/// Three floats as they are stored, finite or not.
fn point(bytes: &[u8], at: usize) -> super::Result<[f32; 3]> {
    let mut out = [0.0; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = f32::from_le_bytes(field(bytes, at + 4 * i)?);
    }
    Ok(out)
}

fn node(n: &[u8]) -> super::Result<PathNode> {
    let junction_address = address(n, 0x34)?;
    if address(n, 0x18)? != junction_address {
        return Err(broken(
            "a node",
            "the address its element holds is not the one at 0x18 of the node",
        ));
    }
    Ok(PathNode {
        position: point(n, 0x00)?,
        flags: u64_at(n, 0x0C)?,
        address: address(n, 0x14)?,
        hash_2522b86a: address(n, 0x1C)?,
        street_name: u32_at(n, 0x20)?,
        junction_address,
        first_link: u32_at(n, 0x38)?,
    })
}

fn link(l: &[u8]) -> super::Result<Link> {
    Ok(Link {
        target: address(l, 0x00)?,
        distance: u16::from_le_bytes(field(l, 0x04)?),
        lanes_from_other_node: field::<1>(l, 0x06)?[0],
        lanes_to_other_node: field::<1>(l, 0x07)?[0],
        flags: u32_at(l, 0x0C)?,
    })
}

fn junction(lists: &Lists, j: &[u8], what: &str) -> super::Result<Junction> {
    let entrances = lists
        .records(j, 0x08, C_JUNCTION_ENTRANCE, ENTRANCE_SIZE, what)?
        .into_iter()
        .map(|e| {
            let turns = (lists.records(e, 0x08, TURN, TURN_SIZE, what)?.into_iter())
                .map(|t| {
                    Ok(Turn {
                        node: address(t, 0x78)?,
                        path_length: f32::from_le_bytes(field(t, 0x8C)?),
                        ends: [point(t, 0x90)?, point(t, 0x9C)?],
                    })
                })
                .collect::<super::Result<_>>()?;
            Ok(Entrance {
                node: address(e, 0x18)?,
                is_stop: field::<1>(e, 0x47)?[0] != 0,
                is_yield: field::<1>(e, 0x48)?[0] != 0,
                turns,
            })
        })
        .collect::<super::Result<_>>()?;
    let phases = (lists.records(j, 0x28, PHASE, PHASE_SIZE, what)?.into_iter())
        .map(|p| {
            let light_boxes = lists
                .records(p, 0x08, LIGHT_BOX, LIGHT_BOX_SIZE, what)?
                .into_iter()
                .map(|b| {
                    Ok(LightBox {
                        entity_id: u64_at(b, 0x00)?,
                        box_id: field::<1>(b, 0x08)?[0],
                    })
                })
                .collect::<super::Result<_>>()?;
            Ok(Phase {
                duration: f32::from_le_bytes(field(p, 0x00)?),
                light_boxes,
            })
        })
        .collect::<super::Result<_>>()?;
    Ok(Junction {
        node: address(j, 0x00)?,
        entrances,
        phases,
    })
}

/// The three lists joined: each node's links, and the node an address names.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneGraph {
    pub nodes: Vec<PathNode>,
    pub links: Vec<Link>,
    pub junctions: Vec<Junction>,
    by_address: HashMap<NodeAddress, usize>,
    by_junction_address: HashMap<NodeAddress, usize>,
}

impl LaneGraph {
    /// The graph of the three lists, refused where a node's links are out of order or past the
    /// links, or a link or a junction names a node that is not there.
    pub fn join(nodes: Vec<PathNode>, links: Vec<Link>, junctions: Vec<Junction>) -> Result<Self> {
        let mut by_address = HashMap::with_capacity(nodes.len());
        let mut by_junction_address = HashMap::with_capacity(nodes.len());
        let mut before = 0;
        for (k, n) in nodes.iter().enumerate() {
            if n.first_link < before || n.first_link as usize > links.len() {
                return Err(LaneError::FirstLink {
                    node: k,
                    first: n.first_link,
                    links: links.len(),
                });
            }
            before = n.first_link;
            // Of two nodes with one address the first is the one named.
            by_address.entry(n.address).or_insert(k);
            by_junction_address.entry(n.junction_address).or_insert(k);
        }
        if let Some(link) = (links.iter()).position(|l| !by_address.contains_key(&l.target)) {
            let target = links[link].target;
            return Err(LaneError::LinkTarget { link, target });
        }
        let lost = (junctions.iter()).position(|j| !by_junction_address.contains_key(&j.node));
        if let Some(junction) = lost {
            let address = junctions[junction].node;
            return Err(LaneError::JunctionNode { junction, address });
        }
        Ok(Self {
            nodes,
            links,
            junctions,
            by_address,
            by_junction_address,
        })
    }

    /// The links that leave node `node`.
    pub fn links_of(&self, node: usize) -> &[Link] {
        let Some(n) = self.nodes.get(node) else {
            return &[];
        };
        let end = (self.nodes.get(node + 1)).map_or(self.links.len(), |n| n.first_link as usize);
        &self.links[n.first_link as usize..end]
    }

    /// The node a link names.
    pub fn target(&self, link: &Link) -> Option<usize> {
        self.by_address.get(&link.target).copied()
    }

    /// The node a junction, an entrance or a turn names by `address`.
    pub fn junction_node(&self, address: NodeAddress) -> Option<usize> {
        self.by_junction_address.get(&address).copied()
    }

    /// How many links have a link back from their target to the node that holds them.
    pub fn links_with_a_reverse(&self) -> usize {
        let pairs: std::collections::HashSet<(usize, usize)> = (0..self.nodes.len())
            .flat_map(|from| {
                (self.links_of(from).iter()).filter_map(move |l| Some((from, self.target(l)?)))
            })
            .collect();
        (0..self.nodes.len())
            .flat_map(|from| self.links_of(from).iter().map(move |l| (from, l)))
            .filter(|(from, l)| (self.target(l)).is_some_and(|to| pairs.contains(&(to, *from))))
            .count()
    }

    /// How many junctions traffic lights run.
    pub fn signalised(&self) -> usize {
        (self.junctions.iter())
            .filter(|j| j.is_signalised())
            .count()
    }
}

#[cfg(test)]
#[path = "map_lanes_tests.rs"]
mod tests;
