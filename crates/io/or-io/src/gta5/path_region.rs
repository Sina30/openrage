//! `.ynd`: one region of the traffic path graph — nodes, the links out of
//! them, and junctions with the heightmap patches under them.
//!
//! ⚠ **Every run here is indexed by another**: a node's first link is the
//! running total of link counts before it, and a junction's first height
//! sample the running total of patch sizes. Inserting one moves every one after it,
//! and the result loads and is wrong, so a save writes both from the counts
//! and [`validate`] refuses a region whose chains do not close.

use super::shared::{flags, set_flags, Bits};
use crate::decl::*;
use crate::desc::Value;
use crate::Generation;

impl Resource for CPathRegion {
    const EXT: &'static str = "ynd";
    const TARGET: Generation = Generation::Gen8;
}

/// A `.ynd`'s root: `CPathRegion`, with its `pgBase` members first.
#[record(size = 0x70, recompute = CPathRegion::chains)]
pub struct CPathRegion {
    vft: u64,
    pub first_node: Option<Box<PgBasePageMap>>,
    /// Vehicle nodes first, then pedestrian ones: nothing in a node says which it is.
    #[len(field = "num_nodes")]
    pub nodes: Vec<CPathNode>,
    num_nodes: i32,
    pub num_nodes_car_nodes: i32,
    #[pad_after(4)]
    pub num_nodes_ped_nodes: i32,
    #[len(field = "num_links")]
    pub links: Vec<CPathNodeLink>,
    #[pad_after(4)]
    num_links: i32,
    /// ⚠ Counted five words below, by `num_junctions`.
    #[len(field = "num_junctions")]
    pub virtual_junctions: Vec<CPathVirtualJunction>,
    /// One byte array every junction takes a rectangle out of.
    #[len(field = "num_height_samples")]
    pub height_samples: Vec<u8>,
    pub junction_map: JunctionMapContainer,
    num_junctions: i32,
    /// ⚠ The engine's region ends here; a resource allocates it to 0x70.
    #[pad_after(8)]
    num_height_samples: u32,
}

impl CPathRegion {
    /// Writes each node's first link and each junction's first height sample: the running totals of the counts before it.
    fn chains(
        &self,
        _: &mut Raw<Self>,
        kids: &mut [(u32, Value)],
        at: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        let start = |what: &str, n: usize| {
            u16::try_from(n).map_err(|_| OwnedError::Invalid {
                what: format!("CPathRegion.{what}"),
                why: format!("a run starts at {n}, and its start is sixteen bits"),
            })
        };
        let nodes = at + std::mem::offset_of!(Raw<Self>, nodes);
        let junctions = at + std::mem::offset_of!(Raw<Self>, virtual_junctions);
        for (o, kid) in kids.iter_mut() {
            let mut running = 0usize;
            if *o as usize == nodes {
                stamp_run::<Raw<CPathNode>>(kid, |n| {
                    n.start_index_of_links = start("nodes", running)? as i16;
                    running += NUM_LINKS.get(n.as_integer2) as usize;
                    Ok(())
                })?;
            } else if *o as usize == junctions {
                stamp_run::<Raw<CPathVirtualJunction>>(kid, |j| {
                    j.start_index_of_height_samples = start("virtual_junctions", running)?;
                    running += usize::from(j.x_samples) * usize::from(j.y_samples);
                    Ok(())
                })?;
            }
        }
        Ok(())
    }
}

/// `pgBaseNode`: the defragmenter's list link, a runtime pointer.
#[record(size = 0x08)]
pub struct PgBaseNode {
    next: Ptr<Opaque>,
}

/// `pgBasePageMap`: the page counts a resource's first node leads to.
#[record(size = 0x10)]
pub struct PgBasePageMap {
    pub base: PgBaseNode,
    virtual_count: u8,
    physical_count: u8,
    pub root_virtual_chunk: u8,
    pub subtype: u8,
    pad: u32,
    #[note(
        "one slot per page, zero in a file: the game fills them at load, so a save writes the room"
    )]
    #[codec(Trailing<u64>)]
    #[len(sum = "virtual_count + physical_count")]
    page_info: PageSlots,
}

/// A page map's `page_info`, one zero slot per page, for a private `#[codec]` field.
///
/// ⚠ Opening refuses a slot that is not zero: a save writes every slot zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PageSlots;

impl Heap for PageSlots {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl Codec for PageSlots {
    fn open(site: &Site<'_>, _: &Rx<'_>) -> Result<Self, OwnedError> {
        match site.value() {
            Some(Value::Bytes(slots)) if slots.iter().all(|b| *b == 0) => Ok(Self),
            Some(Value::Bytes(_)) => Err(site.refuse(
                "a slot is not zero, and a save writes every slot zero for the game to fill at load",
            )),
            _ => Err(site.refuse("the slots are not a run of words")),
        }
    }

    fn save(&self, site: &mut SiteMut<'_>, _: &Cx) -> Result<(), OwnedError> {
        let pages = site
            .int("virtual_count")
            .zip(site.int("physical_count"))
            .and_then(|(v, p)| usize::try_from(v + p).ok())
            .ok_or_else(|| site.refuse("no page counts to size the slots by"))?;
        site.put(Value::Bytes(vec![0; pages * std::mem::size_of::<u64>()]));
        Ok(())
    }
}

/// Which junction a node has: a binary map from its address to the junction's index.
#[record(size = 0x18)]
pub struct JunctionMapContainer {
    pub junction_map: AtBinaryMap,
}

/// `atBinaryMap<s32, u32>`: pairs sorted by key, so a lookup is a binary search.
#[record(size = 0x18)]
pub struct AtBinaryMap {
    #[note("1 in every shipped region")]
    pub sorted: u8,
    #[pad_after(4)]
    pad: [u8; 3],
    /// ⚠ Sorted by node address, so a node's junction is found by binary search.
    pub data: Vec<AtBinaryMapDataPair>,
}

/// One pair of an [`AtBinaryMap`].
#[record(size = 0x08)]
pub struct AtBinaryMapDataPair {
    /// A node's address: its region in the low sixteen bits, its index in the high.
    pub key: u32,
    pub data: i32,
}

impl AtBinaryMapDataPair {
    /// (region, index), the order the map is sorted in.
    pub fn address(&self) -> (u16, u16) {
        (self.key as u16, (self.key >> 16) as u16)
    }
}

/// A node's global address: which region, and which node in it.
#[record(size = 0x04)]
pub struct CNodeAddress {
    pub region: u16,
    pub index: u16,
}

/// One node of the graph.
///
/// ⚠ Positions are fixed point and nothing in the file states the divisor,
/// so they are the integers they are.
#[record(size = 0x28)]
pub struct CPathNode {
    next: Ptr<Opaque>,
    previous: Ptr<Opaque>,
    pub address: CNodeAddress,
    pub street_name_hash: u32,
    pub distance_to_target: i16,
    #[note("the running total of the link counts of every node before this one; written at save")]
    start_index_of_links: i16,
    pub coors_x: i16,
    pub coors_y: i16,
    /// Flags in the low sixteen bits, the height in the high sixteen.
    pub as_integer1: u32,
    #[note("flags, and in bits 11 to 15 the number of links")]
    as_integer2: u32,
}

/// `num_links`, in a node's `as_integer2`.
const NUM_LINKS: Bits = Bits { low: 11, width: 5 };

impl CPathNode {
    const WORD2: usize = std::mem::offset_of!(Raw<Self>, as_integer2);

    pub fn coors_z(&self) -> i16 {
        (self.as_integer1 >> 16) as i16
    }

    /// The links out of this node: the region's next, after every earlier node's.
    pub fn num_links(&self) -> usize {
        NUM_LINKS.read(&self.__carry, Self::WORD2)
    }

    /// Sets the link count; refused above 31, which is all its five bits hold.
    pub fn set_num_links(&mut self, n: usize) -> Result<(), OwnedError> {
        NUM_LINKS.write(&mut self.__carry, Self::WORD2, n, "CPathNode.num_links")
    }

    /// `as_integer2`'s flags: every bit but the link count's, which reads zero.
    pub fn as_integer2(&self) -> u32 {
        flags(&self.__carry, Self::WORD2, NUM_LINKS.mask())
    }

    /// Sets `as_integer2`'s flags; bits under the link count are ignored.
    pub fn set_as_integer2(&mut self, bits: u32) {
        set_flags(&mut self.__carry, Self::WORD2, NUM_LINKS.mask(), bits)
    }
}

/// One directed edge out of a node.
///
/// ⚠ The target is a global address: its region names another `.ynd`, and a
/// link crossing a region boundary resolves in no single file.
#[record(size = 0x08)]
pub struct CPathNodeLink {
    pub other_node: CNodeAddress,
    pub as_integer1: u32,
}

/// A junction, and the patch of the region's height samples under it.
#[record(size = 0x0c)]
pub struct CPathVirtualJunction {
    pub max_z: u16,
    /// The patch's corner, not the junction's centre.
    pub min_x: i16,
    pub min_y: i16,
    pub height_base_world: u16,
    #[note("the running total of x_samples * y_samples before it; written at save")]
    start_index_of_height_samples: u16,
    pub x_samples: u8,
    pub y_samples: u8,
}

impl CPathVirtualJunction {
    pub fn num_height_samples(&self) -> usize {
        usize::from(self.x_samples) * usize::from(self.y_samples)
    }
}

/// A region the game would misread, and which of its chains is broken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error(
        "the region holds {nodes} nodes and states {vehicle} vehicle and {ped} pedestrian ones"
    )]
    NodeSplit { nodes: u32, vehicle: u32, ped: u32 },
    #[error("node {node} starts its links at {states} and the links before it end at {chain}")]
    LinkChain {
        node: usize,
        states: u16,
        chain: usize,
    },
    #[error("the nodes' link counts sum to {chain} and the region holds {links} links")]
    LinkTotal { chain: usize, links: usize },
    #[error(
        "junction {junction} starts its patch at {states} and the patches before it end at {chain}"
    )]
    HeightmapChain {
        junction: usize,
        states: u16,
        chain: usize,
    },
    #[error("the junction patches sum to {chain} bytes and the heightmap holds {bytes}")]
    HeightmapTotal { chain: usize, bytes: usize },
    #[error("junction reference {index} is out of (area, node) order, so a lookup would miss it")]
    RefsUnsorted { index: usize },
    #[error("junction reference {index} names junction {junction} of {junctions}")]
    RefOutOfRange {
        index: usize,
        junction: u32,
        junctions: usize,
    },
    #[error("{0}")]
    View(#[from] ViewError),
}

/// Every chain a region's runs state about each other.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), Broken> {
    let region = View::<Raw<CPathRegion>>::at_offset(image, root)?;
    let (nodes, vehicle, ped) = (
        region.num_nodes as u32,
        region.num_nodes_car_nodes as u32,
        region.num_nodes_ped_nodes as u32,
    );
    if u64::from(vehicle) + u64::from(ped) != u64::from(nodes) {
        return Err(Broken::NodeSplit {
            nodes,
            vehicle,
            ped,
        });
    }
    let links = region.links()?.len();
    let mut chain = 0usize;
    for (node, n) in region.nodes()?.iter().enumerate() {
        if n.start_index_of_links as u16 as usize != chain {
            return Err(Broken::LinkChain {
                node,
                states: n.start_index_of_links as u16,
                chain,
            });
        }
        chain += ((n.as_integer2 >> 11) & 0x1f) as usize;
    }
    if chain != links {
        return Err(Broken::LinkTotal { chain, links });
    }
    let junctions = region.virtual_junctions()?;
    let mut chain = 0usize;
    for (junction, j) in junctions.iter().enumerate() {
        if usize::from(j.start_index_of_height_samples) != chain {
            return Err(Broken::HeightmapChain {
                junction,
                states: j.start_index_of_height_samples,
                chain,
            });
        }
        chain += usize::from(j.x_samples) * usize::from(j.y_samples);
    }
    let bytes = region.height_samples()?.len();
    if chain != bytes {
        return Err(Broken::HeightmapTotal { chain, bytes });
    }
    let map = region.junction_map().junction_map();
    let refs = map.data()?;
    let address = |key: u32| (key as u16, (key >> 16) as u16);
    for (index, r) in refs.iter().enumerate() {
        let key = { r.key };
        if index > 0 && address(refs[index - 1].key) > address(key) {
            return Err(Broken::RefsUnsorted { index });
        }
        let data = { r.data };
        if data < 0 || data as usize >= junctions.len() {
            return Err(Broken::RefOutOfRange {
                index,
                junction: data as u32,
                junctions: junctions.len(),
            });
        }
    }
    Ok(())
}
