//! `.ynv`: one 150 m cell of the navigation mesh — quantised vertices,
//! polygons over a shared index run, per-corner adjacency, special links out
//! of the cell, and a quadtree of sectors.
//!
//! ⚠ **Four of the runs are split arrays.** An [`AiSplitArray`] holds a table
//! of 16 KiB sub-arrays, each with its own count, and a table of where each
//! starts; item `i` is `sub_arrays[i / capacity][i % capacity]` with
//! `capacity = 16384 / stride`. The file states the partition and never the
//! stride, so it is carried as the file states it and [`validate`] holds it to
//! the stride.
//!
//! ⚠ **A quadtree node's five pointers sit at four modulo eight**, so
//! [`CNavMeshQuadTree`] is packed and its accessors copy. A canonical save
//! lays a split array's `start_indices` straight after its last sub-array's
//! items, so after an odd number of indices a peek of that run refuses it as
//! misaligned; the owned value reads it wherever it lies.
//!
//! Every running total the mesh states (a split array's starts, a polygon's
//! first corner and first special link, the sub-array it sits in, a leaf's
//! first cover point) and the memory total are written at save.

use std::mem::{offset_of, size_of};

use super::path_region::PgBasePageMap;
use super::shared::{flags, set_flags, Bits, Vector3};
use crate::decl::*;
use crate::desc::Value;
use crate::Generation;

impl Resource for CNavMesh {
    const EXT: &'static str = "ynv";
    const TARGET: Generation = Generation::Gen8;
}

/// The bytes one sub-array of an [`AiSplitArray`] holds.
pub const CHUNK_BYTES: usize = 16384;

/// `Matrix34`: three axes and a translation, each a [`Vector3`].
#[record(size = 0x40)]
pub struct Matrix34 {
    pub a: Vector3,
    pub b: Vector3,
    pub c: Vector3,
    pub d: Vector3,
}

/// `CNavMesh`: a `.ynv`'s root, with its `pgBase` members first.
#[record(size = 0x170, recompute = CNavMesh::count)]
pub struct CNavMesh {
    vft: u64,
    pub first_node: Option<Box<PgBasePageMap>>,
    /// The DLC group in the top eight bits.
    pub flags: u32,
    #[pad_after(8)]
    pub file_version: u32,
    pub matrix: Matrix34,
    pub extents: Vector3,
    pub compressed_vertex_array: Option<Box<AiSplitArray<CNavMeshCompressedVertex>>>,
    vertex_pool: Ptr<Opaque>,
    pub vertex_index_array: Option<Box<AiSplitArray<u16>>>,
    /// One per index: the polygon across each corner's edge.
    pub adjacent_polys_array: Option<Box<AiSplitArray<TAdjPoly>>>,
    #[note("the index count, written from the index array at save")]
    size_of_pools: u32,
    pub adjacent_meshes: CAdjacentNavmeshLookups,
    pub polys_array: Option<Box<AiSplitArray<TNavMeshPoly>>>,
    pub quad_tree: Option<Box<CNavMeshQuadTree>>,
    #[len(field = "num_special_links")]
    pub special_links: Vec<CSpecialLinkInfo>,
    #[len(field = "num_special_links_indices")]
    pub special_links_indices: Vec<u16>,
    #[note("written from the vertex array at save")]
    num_vertices: u32,
    #[note("written from the polygon array at save")]
    num_polys: u32,
    /// `x + 100 * y` of the cell.
    pub index_of_mesh: u32,
    #[note("the bytes of the items, the quadtree and the special links; written at save")]
    total_memory_used: u32,
    #[note("written from the quadtree's leaves at save")]
    num_cover_points: u32,
    num_special_links: u32,
    #[pad_after(4)]
    num_special_links_indices: u32,
    non_resourced_data: Ptr<Opaque>,
    pub build_id: u32,
    /// ⚠ The engine's mesh ends here; a resource allocates it to 0x170.
    #[pad_after(8)]
    padding: u32,
}

impl CNavMesh {
    /// Writes the totals the arrays and the quadtree hold, and stamps the
    /// polygons' and the leaves' running totals into what the save writes.
    fn count(
        &self,
        raw: &mut Raw<Self>,
        kids: &mut [(u32, Value)],
        at: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        fn items<T: Owned>(list: &Option<Box<AiSplitArray<T>>>) -> usize
        where
            Raw<T>: Plain + Element + Param,
        {
            list.as_deref().map_or(0, AiSplitArray::len)
        }
        fn bytes<T: Owned>(list: &Option<Box<AiSplitArray<T>>>) -> usize
        where
            Raw<T>: Plain + Element + Param,
        {
            items(list) * size_of::<Raw<T>>()
        }
        fn nodes<'a>(node: &'a CNavMeshQuadTree, out: &mut Vec<&'a CNavMeshQuadTree>) {
            out.push(node);
            for c in node.children.iter().flatten() {
                nodes(c, out);
            }
        }
        let mut tree = Vec::new();
        if let Some(root) = self.quad_tree.as_deref() {
            nodes(root, &mut tree);
        }
        let leaves = || tree.iter().copied().filter_map(|n| n.leaf_data.as_deref());
        let word = |what: &str, n: usize| {
            u32::try_from(n).map_err(|_| OwnedError::Invalid {
                what: format!("CNavMesh.{what}"),
                why: format!("{n}, and the field is thirty-two bits"),
            })
        };
        raw.num_vertices = word(
            "compressed_vertex_array",
            items(&self.compressed_vertex_array),
        )?;
        raw.size_of_pools = word("vertex_index_array", items(&self.vertex_index_array))?;
        raw.num_polys = word("polys_array", items(&self.polys_array))?;
        raw.num_cover_points = word("quad_tree", leaves().map(|l| l.cover_points.len()).sum())?;
        // ⚠ Measured on the corpus: the special link indices and every header are not counted.
        raw.total_memory_used = word(
            "total_memory_used",
            bytes(&self.compressed_vertex_array)
                + bytes(&self.vertex_index_array)
                + bytes(&self.adjacent_polys_array)
                + bytes(&self.polys_array)
                + tree.len() * size_of::<Raw<CNavMeshQuadTree>>()
                + leaves()
                    .map(|l| {
                        size_of::<Raw<TNavMeshQuadTreeLeafData>>()
                            + l.polys.len() * size_of::<u16>()
                            + l.cover_points.len() * size_of::<Raw<CNavMeshCoverPoint>>()
                    })
                    .sum::<usize>()
                + self.special_links.len() * size_of::<Raw<CSpecialLinkInfo>>(),
        )?;
        let (polys, quad_tree) = (
            at + offset_of!(Raw<Self>, polys_array),
            at + offset_of!(Raw<Self>, quad_tree),
        );
        for (o, kid) in kids.iter_mut() {
            if *o as usize == polys {
                stamp_polygons(kid)?;
            } else if *o as usize == quad_tree {
                stamp_leaves(kid, &mut 0)?;
            }
        }
        Ok(())
    }
}

/// Each polygon's first corner, first special link and sub-array, as the running totals before it.
fn stamp_polygons(list: &mut Value) -> Result<(), OwnedError> {
    let refuse = || OwnedError::Invalid {
        what: "CNavMesh.polys_array".into(),
        why: "the saved split array is not a record holding a run of sub-arrays".into(),
    };
    let Some(list) = list.target_mut() else {
        return Ok(());
    };
    let subs = list
        .record_mut()
        .and_then(|r| r.child_at_mut(offset_of!(Raw<AiSplitArray<TNavMeshPoly>>, sub_arrays)))
        .ok_or_else(refuse)?;
    let Some(subs) = subs.target_mut() else {
        return Ok(());
    };
    let Value::Array { elements, .. } = subs else {
        return Err(refuse());
    };
    let (mut first, mut links) = (0usize, 0usize);
    for (index, sub) in elements.iter_mut().enumerate() {
        let polygons = sub
            .record_mut()
            .and_then(|r| r.child_at_mut(offset_of!(Raw<SubArray<TNavMeshPoly>>, elements)))
            .ok_or_else(refuse)?;
        stamp_run::<Raw<TNavMeshPoly>>(polygons, |p| {
            p.as_integer2 =
                FIRST_VERTEX_INDEX.put(p.as_integer2, first, "TNavMeshPoly.first_vertex_index")?;
            p.as_integer4 =
                POLY_ARRAY_INDEX.put(p.as_integer4, index, "TNavMeshPoly.poly_array_index")?;
            p.as_integer4 = SPECIAL_LINKS_START_INDEX.put(
                p.as_integer4,
                links,
                "TNavMeshPoly.special_links_start_index",
            )?;
            first += NUM_VERTICES.get(p.as_integer1) as usize;
            links += NUM_SPECIAL_LINKS.get(p.as_integer4) as usize;
            Ok(())
        })?;
    }
    Ok(())
}

/// Each leaf's first cover point: the running total of the leaves before it, depth first.
fn stamp_leaves(node: &mut Value, running: &mut usize) -> Result<(), OwnedError> {
    let Some(node) = node.target_mut().and_then(Value::record_mut) else {
        return Ok(());
    };
    if let Some(leaf) = node.child_at_mut(offset_of!(Raw<CNavMeshQuadTree>, leaf_data)) {
        stamp_run::<Raw<TNavMeshQuadTreeLeafData>>(leaf, |l| {
            l.index_of_first_cover_point_in_list =
                u16::try_from(*running).map_err(|_| OwnedError::Invalid {
                    what: "TNavMeshQuadTreeLeafData.index_of_first_cover_point_in_list".into(),
                    why: format!("{running}, and it is sixteen bits"),
                })?;
            *running += usize::from(l.num_cover_points);
            Ok(())
        })?;
    }
    if let Some(Value::Array { elements, .. }) =
        node.child_at_mut(offset_of!(Raw<CNavMeshQuadTree>, children))
    {
        for child in elements {
            stamp_leaves(child, running)?;
        }
    }
    Ok(())
}

/// `CAdjacentNavmeshLookups`: the meshes this one's edges lead into.
#[record(size = 0x84, recompute = CAdjacentNavmeshLookups::count)]
pub struct CAdjacentNavmeshLookups {
    #[note("the leading non-zero indices, written at save")]
    num_indices: u32,
    /// ⚠ Zero ends the list: every shipped mesh fills it from the front.
    pub navmesh_indices: [u32; 32],
}

impl CAdjacentNavmeshLookups {
    fn count(
        &self,
        raw: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        raw.num_indices = self
            .navmesh_indices
            .iter()
            .position(|i| *i == 0)
            .unwrap_or(self.navmesh_indices.len()) as u32;
        Ok(())
    }
}

/// `aiSplitArray<T>`: a run held as 16 KiB sub-arrays, with where each starts.
#[record(size = 0x30, recompute = Self::count)]
pub struct AiSplitArray<T> {
    vft: u64,
    #[note("written from the sub-arrays at save")]
    #[pad_after(4)]
    num_elements: u32,
    /// ⚠ Every sub-array but the last holds `16384 / stride` items: the game
    /// finds an item by that division, not by these counts.
    #[len(field = "num_sub_arrays")]
    pub sub_arrays: Vec<SubArray<T>>,
    #[note(
        "each sub-array's first item: the running total of the counts before it; written at save"
    )]
    #[codec(Ptr<[u32]>)]
    #[len(field = "num_sub_arrays")]
    start_indices: StartIndices,
    /// ⚠ The engine's array ends at 0x28; a resource allocates it to 0x30.
    #[pad_after(12)]
    num_sub_arrays: u32,
}

/// A split array's `start_indices`, the running total of its sub-arrays' counts, for a private `#[codec]` field.
///
/// ⚠ Opening refuses starts that are not the running total, which a save would not keep.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct StartIndices;

impl StartIndices {
    /// The start of each sub-array in the run `sub_arrays` holds.
    fn derived(sub_arrays: Option<&Value>) -> Result<Vec<u32>, String> {
        let Some(run) = sub_arrays.and_then(Value::target) else {
            return Ok(Vec::new());
        };
        let n = run.run_len().ok_or("`sub_arrays` is not a run")?;
        let mut running = 0u32;
        (0..n)
            .map(|i| {
                let count = run
                    .struct_at(i)
                    .and_then(|s| s.int("count"))
                    .and_then(|c| u32::try_from(c).ok())
                    .ok_or("a sub-array states no count")?;
                let start = running;
                running = running.checked_add(count).ok_or("the counts overflow")?;
                Ok(start)
            })
            .collect()
    }
}

impl Heap for StartIndices {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl Codec for StartIndices {
    fn open(site: &Site<'_>, _: &Rx<'_>) -> Result<Self, OwnedError> {
        let stated: Vec<u32> = match site.value() {
            Some(Value::Ptr { target: None, .. }) => Vec::new(),
            Some(Value::Ptr {
                target: Some(run), ..
            }) => match run.as_ref() {
                Value::Bytes(b) => b
                    .as_chunks::<{ size_of::<u32>() }>()
                    .0
                    .iter()
                    .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                    .collect(),
                _ => return Err(site.refuse("the starts are not a run of words")),
            },
            _ => return Err(site.refuse("the starts are not behind a pointer")),
        };
        let derived = Self::derived(site.child("sub_arrays")).map_err(|e| site.refuse(e))?;
        if stated != derived {
            return Err(site.refuse(format!(
                "the starts {stated:?} are not the running total {derived:?} of the sub-arrays' counts"
            )));
        }
        Ok(Self)
    }

    fn save(&self, site: &mut SiteMut<'_>, _: &Cx) -> Result<(), OwnedError> {
        let starts = Self::derived(site.child("sub_arrays")).map_err(|e| site.refuse(e))?;
        site.put(if starts.is_empty() {
            Value::null_ptr()
        } else {
            Value::ptr_to(Value::Bytes(
                starts.iter().flat_map(|s| s.to_le_bytes()).collect(),
            ))
        });
        Ok(())
    }
}

impl<T: Owned> AiSplitArray<T>
where
    Raw<T>: Plain + Element + Param,
{
    fn count(
        &self,
        raw: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        raw.num_elements = u32::try_from(self.len()).map_err(|_| OwnedError::Invalid {
            what: "AiSplitArray.sub_arrays".into(),
            why: format!("{} items, and the count is thirty-two bits", self.len()),
        })?;
        Ok(())
    }

    /// Every item, in order, across the sub-arrays.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.sub_arrays.iter().flat_map(|s| s.elements.iter())
    }

    /// The items the sub-arrays hold between them.
    pub fn len(&self) -> usize {
        self.sub_arrays.iter().map(|s| s.elements.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.sub_arrays
            .iter_mut()
            .flat_map(|s| s.elements.iter_mut())
    }

    /// Removes item `index` and pulls each later sub-array's first item back, so every sub-array but the last stays full.
    fn remove(&mut self, index: usize) -> Option<T> {
        let mut before = 0;
        let at = self.sub_arrays.iter().position(|s| {
            let here = index < before + s.elements.len();
            before += usize::from(!here) * s.elements.len();
            here
        })?;
        let item = self.sub_arrays[at].elements.remove(index - before);
        for later in at + 1..self.sub_arrays.len() {
            if self.sub_arrays[later].elements.is_empty() {
                break;
            }
            let first = self.sub_arrays[later].elements.remove(0);
            self.sub_arrays[later - 1].elements.push(first);
        }
        if self
            .sub_arrays
            .last()
            .is_some_and(|s| s.elements.is_empty())
        {
            self.sub_arrays.pop();
        }
        Some(item)
    }
}

/// `aiSplitArray<T>::SubArray`: one sub-array's items.
#[record(size = 0x10)]
pub struct SubArray<T> {
    #[len(field = "count")]
    pub elements: Vec<T>,
    #[pad_after(4)]
    count: u32,
}

/// `CNavMeshCompressedVertex`: one vertex, quantised into the mesh's extents.
#[record(size = 0x06)]
pub struct CNavMeshCompressedVertex {
    pub data: [u16; 3],
}

/// `TAdjPoly`: the polygon across one corner's edge, as two words of bit fields.
#[record(size = 0x08)]
pub struct TAdjPoly {
    pub as_integer1: u32,
    pub as_integer2: u32,
}

/// The lookup slot of the cell an edge leads into, in `TAdjPoly`'s first word.
const ADJ_SLOT: Bits = Bits { low: 0, width: 5 };
/// The polygon an edge leads to, in `TAdjPoly`'s first word.
const ADJ_POLYGON: Bits = Bits { low: 5, width: 15 };
/// The bits the second word repeats from the first, on every shipped edge.
const ADJ_TWIN: u32 = ADJ_SLOT.mask() | ADJ_POLYGON.mask();

/// The polygon an edge states when it leads nowhere: all ones in its field.
pub const NO_NEIGHBOUR: usize = 0x7fff;

impl TAdjPoly {
    /// An edge that leads to `polygon` of the cell in lookup slot `slot`; `NO_NEIGHBOUR` leads nowhere.
    pub fn leading_to(slot: usize, polygon: usize) -> Result<Self, OwnedError> {
        ADJ_SLOT.put(0, slot, "TAdjPoly.slot")?;
        ADJ_POLYGON.put(0, polygon, "TAdjPoly.polygon")?;
        let mut edge = Self::default();
        edge.set(slot, polygon);
        Ok(edge)
    }

    /// The lookup slot and the polygon the edge leads to; `None` where it leads nowhere.
    pub fn neighbour(&self) -> Option<(usize, usize)> {
        let polygon = ADJ_POLYGON.get(self.as_integer1) as usize;
        (polygon != NO_NEIGHBOUR).then(|| (ADJ_SLOT.get(self.as_integer1) as usize, polygon))
    }

    /// Writes the slot and the polygon into both words, keeping the bits above them; the values must fit their fields.
    fn set(&mut self, slot: usize, polygon: usize) {
        let low = (((slot as u32) << ADJ_SLOT.low) & ADJ_SLOT.mask())
            | (((polygon as u32) << ADJ_POLYGON.low) & ADJ_POLYGON.mask());
        self.as_integer1 = (self.as_integer1 & !ADJ_TWIN) | low;
        self.as_integer2 = (self.as_integer2 & !ADJ_TWIN) | low;
    }
}

/// `TShortMinMax`: a quantised box.
#[record(size = 0x0c)]
pub struct TShortMinMax {
    pub min_x: i16,
    pub max_x: i16,
    pub min_y: i16,
    pub max_y: i16,
    pub min_z: i16,
    pub max_z: i16,
}

/// `TNavMeshPoly`: one polygon, a run of the shared indices, and its box.
#[record(size = 0x30)]
pub struct TNavMeshPoly {
    #[note("flags in bits 0 to 20, the corner count in 21 to 24, the point enum in 25 to 31")]
    as_integer1: u32,
    #[note(
        "the first corner's index in bits 0 to 15, written at save; the mesh in the next fourteen"
    )]
    as_integer2: u32,
    time_stamp: u16,
    #[pad_after(4)]
    a_star_time_stamp: u16,
    path_parent_poly: Ptr<Opaque>,
    pub min_max: TShortMinMax,
    pub as_integer3: u32,
    pub centroid_x: u8,
    pub centroid_y: u8,
    pub cover_directions_bitfield: u8,
    pub immediate_mode_flags: u8,
    #[note(
        "the parent exit edge in bits 0 to 3, then the sub-array, the special link count and the first special link; a save writes the sub-array and the first special link"
    )]
    as_integer4: u32,
}

/// `num_vertices`, in a polygon's `as_integer1`.
const NUM_VERTICES: Bits = Bits { low: 21, width: 4 };
/// `first_vertex_index`, in `as_integer2`.
const FIRST_VERTEX_INDEX: Bits = Bits { low: 0, width: 16 };
/// `poly_array_index`, in `as_integer4`.
const POLY_ARRAY_INDEX: Bits = Bits { low: 4, width: 8 };
/// `num_special_links`, in `as_integer4`.
const NUM_SPECIAL_LINKS: Bits = Bits { low: 12, width: 3 };
/// `special_links_start_index`, in `as_integer4`.
const SPECIAL_LINKS_START_INDEX: Bits = Bits { low: 15, width: 9 };

// ⚠ The first corner and the first special link are running totals: the runs
// tile the shared lists, so one wrong count moves every later polygon's.
impl TNavMeshPoly {
    const WORD1: usize = offset_of!(Raw<Self>, as_integer1);
    const WORD2: usize = offset_of!(Raw<Self>, as_integer2);
    const WORD4: usize = offset_of!(Raw<Self>, as_integer4);
    const COUNTED4: u32 =
        POLY_ARRAY_INDEX.mask() | NUM_SPECIAL_LINKS.mask() | SPECIAL_LINKS_START_INDEX.mask();

    /// The corners: the shared index list's next, after every earlier polygon's.
    pub fn num_vertices(&self) -> usize {
        NUM_VERTICES.read(&self.__carry, Self::WORD1)
    }

    /// Sets the corner count; refused above 15, which is all its four bits hold.
    pub fn set_num_vertices(&mut self, n: usize) -> Result<(), OwnedError> {
        NUM_VERTICES.write(
            &mut self.__carry,
            Self::WORD1,
            n,
            "TNavMeshPoly.num_vertices",
        )
    }

    /// `as_integer1`'s flags and point enum; the corner count reads zero.
    pub fn as_integer1(&self) -> u32 {
        flags(&self.__carry, Self::WORD1, NUM_VERTICES.mask())
    }

    /// Sets `as_integer1` but for the corner count.
    pub fn set_as_integer1(&mut self, bits: u32) {
        set_flags(&mut self.__carry, Self::WORD1, NUM_VERTICES.mask(), bits)
    }

    /// `as_integer2`'s mesh index and reserved bits; the first corner reads zero.
    pub fn as_integer2(&self) -> u32 {
        flags(&self.__carry, Self::WORD2, FIRST_VERTEX_INDEX.mask())
    }

    /// Sets `as_integer2` but for the first corner, which a save writes.
    pub fn set_as_integer2(&mut self, bits: u32) {
        set_flags(
            &mut self.__carry,
            Self::WORD2,
            FIRST_VERTEX_INDEX.mask(),
            bits,
        )
    }

    /// The special links: the mesh's next special link indices, after every earlier polygon's.
    pub fn num_special_links(&self) -> usize {
        NUM_SPECIAL_LINKS.read(&self.__carry, Self::WORD4)
    }

    /// Sets the special link count; refused above 7, which is all its three bits hold.
    pub fn set_num_special_links(&mut self, n: usize) -> Result<(), OwnedError> {
        NUM_SPECIAL_LINKS.write(
            &mut self.__carry,
            Self::WORD4,
            n,
            "TNavMeshPoly.num_special_links",
        )
    }

    /// `as_integer4`'s parent exit edge and unused bits; the rest read zero.
    pub fn as_integer4(&self) -> u32 {
        flags(&self.__carry, Self::WORD4, Self::COUNTED4)
    }

    /// Sets `as_integer4` but for the sub-array, the special link count and the first special link.
    pub fn set_as_integer4(&mut self, bits: u32) {
        set_flags(&mut self.__carry, Self::WORD4, Self::COUNTED4, bits)
    }
}

/// `TCoverPointFlags`: a cover point's direction and type, as one word of bit fields.
#[record(size = 0x02)]
pub struct TCoverPointFlags {
    pub as_integer: u16,
}

/// `CNavMeshCoverPoint`: one point a quadtree leaf carries.
#[record(size = 0x08)]
pub struct CNavMeshCoverPoint {
    pub x: u16,
    pub y: u16,
    pub z: u16,
    pub cover_point_flags: TCoverPointFlags,
}

/// `TNavMeshQuadTreeLeafData`: the polygons a quadtree leaf overlaps, and its cover points.
#[record(size = 0x20)]
pub struct TNavMeshQuadTreeLeafData {
    #[note("the running total of the cover points of the leaves before this one, depth first; written at save")]
    index_of_first_cover_point_in_list: u16,
    unused: u8,
    #[pad_after(4)]
    reserved: u8,
    #[len(field = "num_polys")]
    pub polys: Vec<u16>,
    #[len(field = "num_cover_points")]
    pub cover_points: Vec<CNavMeshCoverPoint>,
    num_polys: u16,
    #[pad_after(4)]
    num_cover_points: u16,
}

/// `CNavMeshQuadTree`: one node, a box and four children or a leaf's data.
#[record(size = 0x60, packed(4))]
pub struct CNavMeshQuadTree {
    pub mins: Vector3,
    pub maxs: Vector3,
    pub min_max: TShortMinMax,
    pub leaf_data: Option<Box<TNavMeshQuadTreeLeafData>>,
    /// ⚠ The engine's node ends here; a resource allocates it to 0x60.
    #[pad_after(12)]
    pub children: [Option<Box<CNavMeshQuadTree>>; 4],
}

/// `CSpecialLinkInfo`: one link, such as a ladder or a drop, between two polygons.
#[record(size = 0x1c)]
pub struct CSpecialLinkInfo {
    pub link_type: u8,
    pub link_flags: u8,
    a_star_time_stamp: u16,
    pub link_from_pos_x: u16,
    pub link_from_pos_y: u16,
    pub link_from_pos_z: u16,
    pub link_to_pos_x: u16,
    pub link_to_pos_y: u16,
    pub link_to_pos_z: u16,
    pub original_link_from_poly: u16,
    pub link_from_poly: u16,
    pub original_link_to_poly: u16,
    pub link_to_poly: u16,
    pub as_integer1: u32,
}

/// A lookup table entry that names no cell: the one a vehicle mesh holds, all ones in a fourteen-bit cell field.
const NO_CELL: u32 = 0x3fff;

/// A mesh's lookup table, read without borrowing the mesh, so edges can be edited while it is consulted.
struct Lookup {
    own: u32,
    /// The entries before the first zero, which ends the table.
    cells: Vec<u32>,
}

impl Lookup {
    fn of(mesh: &CNavMesh) -> Self {
        let table = &mesh.adjacent_meshes.navmesh_indices;
        Self {
            own: mesh.index_of_mesh,
            cells: table.iter().copied().take_while(|c| *c != 0).collect(),
        }
    }

    /// The slot and the polygon of an edge that leads into the mesh itself.
    fn inside(&self, edge: &TAdjPoly) -> Option<(usize, usize)> {
        let (slot, polygon) = edge.neighbour()?;
        (self.cells.get(slot) == Some(&self.own)).then_some((slot, polygon))
    }

    /// The slot every shipped edge that leads nowhere states: the first that does not name the mesh itself.
    fn nowhere(&self) -> usize {
        (0..32)
            .find(|slot| self.cells.get(*slot) != Some(&self.own))
            .unwrap_or(0)
    }

    fn neighbours(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for cell in &self.cells {
            if *cell != self.own && *cell != NO_CELL && !out.contains(cell) {
                out.push(*cell);
            }
        }
        out
    }
}

/// What still leads to a polygon, as its mesh states it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Referenced {
    /// Edges of the mesh that lead into the polygon, as the polygon and the corner each belongs to.
    pub edges: Vec<(usize, usize)>,
    /// Special links, by index, that start or end at the polygon or sit in its run.
    pub special_links: Vec<usize>,
    /// Cells the lookup table lists, whose own edges may name the mesh's polygons by number.
    pub cells: Vec<u32>,
}

impl Referenced {
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty() && self.special_links.is_empty() && self.cells.is_empty()
    }
}

/// `items` joined, the first eight and a count of the rest.
fn named(items: Vec<String>) -> String {
    const SHOWN: usize = 8;
    match items.len().checked_sub(SHOWN) {
        Some(more) if more > 0 => format!("{} and {more} more", items[..SHOWN].join(", ")),
        _ => items.join(", "),
    }
}

impl std::fmt::Display for Referenced {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if !self.edges.is_empty() {
            let edges = (self.edges.iter())
                .map(|(polygon, corner)| format!("polygon {polygon} corner {corner}"))
                .collect();
            parts.push(format!("edges lead into it: {}", named(edges)));
        }
        if !self.special_links.is_empty() {
            let links = (self.special_links.iter())
                .map(|link| format!("special link {link}"))
                .collect();
            parts.push(format!(
                "special links start or end at it: {}",
                named(links)
            ));
        }
        if !self.cells.is_empty() {
            let cells = self.cells.iter().map(|c| format!("cell {c}")).collect();
            parts.push(format!(
                "cells that list this one name its polygons by number: {}",
                named(cells)
            ));
        }
        f.write_str(&parts.join("; "))
    }
}

/// How a polygon edit is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolygonError {
    #[error("the mesh holds {polygons} polygons, so there is no polygon {polygon}")]
    Missing { polygon: usize, polygons: usize },
    #[error("polygon {polygon} is not deleted, since {references}{}", renumbered(.shifted))]
    Referenced {
        polygon: usize,
        references: Referenced,
        /// The polygons after it, whose numbers a deletion would lower by one.
        shifted: std::ops::Range<usize>,
    },
}

fn renumbered(shifted: &std::ops::Range<usize>) -> String {
    match shifted.len() {
        0 => String::new(),
        1 => format!("; polygon {} would change number", shifted.start),
        _ => format!(
            "; polygons {} to {} would change number",
            shifted.start,
            shifted.end - 1
        ),
    }
}

/// Drops `polygon` from every leaf's list and lowers each later number by one.
fn drop_from_leaves(node: &mut CNavMeshQuadTree, polygon: u16) {
    if let Some(leaf) = node.leaf_data.as_deref_mut() {
        leaf.polys.retain(|p| *p != polygon);
        for p in leaf.polys.iter_mut().filter(|p| **p > polygon) {
            *p -= 1;
        }
    }
    for child in node.children.iter_mut().flatten() {
        drop_from_leaves(child, polygon);
    }
}

impl CNavMesh {
    /// Each polygon's corners as a range of the shared index and edge lists, every run after the last's.
    fn corners(&self) -> Vec<std::ops::Range<usize>> {
        let mut end = 0;
        let polygons = self
            .polys_array
            .as_deref()
            .into_iter()
            .flat_map(|l| l.iter());
        polygons
            .map(|p| {
                let run = end..end + p.num_vertices();
                end = run.end;
                run
            })
            .collect()
    }

    fn corner_run(&self, polygon: usize) -> Result<std::ops::Range<usize>, PolygonError> {
        let runs = self.corners();
        let polygons = runs.len();
        (runs.into_iter().nth(polygon)).ok_or(PolygonError::Missing { polygon, polygons })
    }

    /// The cells the lookup table lists besides this one: the meshes whose edges may name this one's polygons by number.
    pub fn neighbours(&self) -> Vec<u32> {
        Lookup::of(self).neighbours()
    }

    /// Per polygon, whether no edge leads out of it and no edge of this mesh leads into it.
    pub fn disabled_polygons(&self) -> Vec<bool> {
        let runs = self.corners();
        let lookup = Lookup::of(self);
        let edges: Vec<&TAdjPoly> =
            (self.adjacent_polys_array.as_deref()).map_or(Vec::new(), |l| l.iter().collect());
        let mut disabled = vec![true; runs.len()];
        for (polygon, run) in runs.into_iter().enumerate() {
            for edge in run.filter_map(|at| edges.get(at)) {
                if edge.neighbour().is_some() {
                    disabled[polygon] = false;
                }
                if let Some((_, target)) = lookup.inside(edge) {
                    if let Some(led_to) = disabled.get_mut(target) {
                        *led_to = false;
                    }
                }
            }
        }
        disabled
    }

    /// What still leads to `polygon`: edges of this mesh, special links, and the cells that may name it.
    pub fn references(&self, polygon: usize) -> Result<Referenced, PolygonError> {
        self.corner_run(polygon)?;
        let lookup = Lookup::of(self);
        let edges: Vec<&TAdjPoly> =
            (self.adjacent_polys_array.as_deref()).map_or(Vec::new(), |l| l.iter().collect());
        let mut found = Referenced::default();
        for (other, run) in self.corners().into_iter().enumerate() {
            if other == polygon {
                continue;
            }
            for (corner, at) in run.enumerate() {
                let led_to = edges.get(at).and_then(|e| lookup.inside(e));
                if led_to.is_some_and(|(_, target)| target == polygon) {
                    found.edges.push((other, corner));
                }
            }
        }
        // The polygon's run in the special link indices follows every earlier polygon's.
        let polygons = self
            .polys_array
            .as_deref()
            .into_iter()
            .flat_map(|l| l.iter());
        let counts: Vec<usize> = polygons.map(|p| p.num_special_links()).collect();
        let before: usize = counts.iter().take(polygon).sum();
        let own = counts.get(polygon).copied().unwrap_or(0);
        let run = self.special_links_indices.iter().skip(before).take(own);
        let mut links: Vec<usize> = run.map(|i| usize::from(*i)).collect();
        if let Ok(number) = u16::try_from(polygon) {
            for (at, link) in self.special_links.iter().enumerate() {
                let ends = [
                    link.original_link_from_poly,
                    link.link_from_poly,
                    link.original_link_to_poly,
                    link.link_to_poly,
                ];
                if ends.contains(&number) {
                    links.push(at);
                }
            }
        }
        links.sort_unstable();
        links.dedup();
        found.special_links = links;
        found.cells = lookup.neighbours();
        Ok(found)
    }

    /// Cuts `polygon` off: every edge out of it and every edge of this mesh into it leads nowhere; returns how many it changed.
    pub fn disable_polygon(&mut self, polygon: usize) -> Result<usize, PolygonError> {
        let run = self.corner_run(polygon)?;
        let lookup = Lookup::of(self);
        let slot = lookup.nowhere();
        let Some(edges) = self.adjacent_polys_array.as_deref_mut() else {
            return Ok(0);
        };
        let mut changed = 0;
        for (at, edge) in edges.iter_mut().enumerate() {
            let leads_in = lookup
                .inside(edge)
                .is_some_and(|(_, target)| target == polygon);
            if edge.neighbour().is_some() && (run.contains(&at) || leads_in) {
                edge.set(slot, NO_NEIGHBOUR);
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Deletes `polygon` and renumbers every reference to a later one, or refuses naming what still leads to it and changes nothing.
    pub fn delete_polygon(&mut self, polygon: usize) -> Result<(), PolygonError> {
        let run = self.corner_run(polygon)?;
        let references = self.references(polygon)?;
        if !references.is_empty() {
            let polygons = self.corners().len();
            return Err(PolygonError::Referenced {
                polygon,
                references,
                shifted: polygon + 1..polygons,
            });
        }
        let lookup = Lookup::of(self);
        if let Some(edges) = self.adjacent_polys_array.as_deref_mut() {
            for edge in edges.iter_mut() {
                if let Some((slot, target)) = lookup.inside(edge).filter(|(_, t)| *t > polygon) {
                    edge.set(slot, target - 1);
                }
            }
            for _ in run.clone() {
                edges.remove(run.start);
            }
        }
        if let Some(indices) = self.vertex_index_array.as_deref_mut() {
            for _ in run.clone() {
                indices.remove(run.start);
            }
        }
        if let Some(polygons) = self.polys_array.as_deref_mut() {
            polygons.remove(polygon);
        }
        if let Ok(number) = u16::try_from(polygon) {
            if let Some(root) = self.quad_tree.as_deref_mut() {
                drop_from_leaves(root, number);
            }
            for link in &mut self.special_links {
                for end in [
                    &mut link.original_link_from_poly,
                    &mut link.link_from_poly,
                    &mut link.original_link_to_poly,
                    &mut link.link_to_poly,
                ] {
                    *end -= u16::from(*end > number);
                }
            }
        }
        Ok(())
    }
}

/// A navmesh the game would misread.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error(
        "the {list} array's sub-arrays hold {held} items against the {stated} it states; the game \
         addresses an item by the partition, so the rest would be read from the wrong sub-array"
    )]
    Partition {
        list: &'static str,
        stated: usize,
        held: usize,
    },
    #[error(
        "the {list} array's sub-array {chunk} holds {count} items where a {stride}-byte stride \
         puts {expected}; the game finds item i in sub-array i / {capacity}"
    )]
    Unsaturated {
        list: &'static str,
        chunk: usize,
        count: usize,
        expected: usize,
        stride: usize,
        capacity: usize,
    },
    #[error(
        "the {list} array's sub-array {chunk} starts at {stated}, not the {running} before it"
    )]
    Starts {
        list: &'static str,
        chunk: usize,
        stated: usize,
        running: usize,
    },
    #[error(
        "polygon {polygon} starts its corners at {stated}, not the {running} before it; one wrong \
         start gives every later polygon somebody else's corners"
    )]
    IndexRuns {
        polygon: usize,
        stated: usize,
        running: usize,
    },
    #[error("the polygons' corners close on {closed} of the {indices} indices")]
    IndexRunsOpen { closed: usize, indices: usize },
    #[error(
        "the polygons' special link counts sum to {counted} and the mesh holds {indices} special \
         link indices; a later polygon's first special link would point past the list"
    )]
    SpecialLinkRunsOpen { counted: usize, indices: usize },
    #[error("{edges} edges against {indices} indices: one edge per polygon corner")]
    EdgeCount { edges: usize, indices: usize },
    #[error("an index names vertex {vertex} of {vertices}; the game reads the pool unchecked")]
    VertexIndex { vertex: usize, vertices: usize },
    #[error("{0}")]
    View(#[from] ViewError),
}

/// A split array's items, in order, after holding its partition to the stride.
pub fn items<'a, T>(
    list: &View<'a, Raw<AiSplitArray<T>>>,
    name: &'static str,
) -> Result<Vec<&'a Raw<T>>, Broken>
where
    T: Owned,
    Raw<T>: Plain + Element + Param,
{
    let stride = std::mem::size_of::<Raw<T>>();
    let capacity = CHUNK_BYTES / stride;
    let chunks = list.sub_arrays()?;
    let starts = start_indices::<T>(list)?;
    let mut out = Vec::with_capacity(list.num_elements as usize);
    let (mut running, n) = (0usize, chunks.len());
    for (i, chunk) in chunks.views().enumerate() {
        let count = chunk.count as usize;
        let expected = if i + 1 < n { capacity } else { count };
        if count != expected || count > capacity || count == 0 {
            return Err(Broken::Unsaturated {
                list: name,
                chunk: i,
                count,
                expected: if i + 1 < n {
                    capacity
                } else {
                    count.min(capacity)
                },
                stride,
                capacity,
            });
        }
        let stated = starts.get(i).map_or(usize::MAX, |s| *s as usize);
        if stated != running {
            return Err(Broken::Starts {
                list: name,
                chunk: i,
                stated,
                running,
            });
        }
        out.extend(chunk.elements()?.as_slice());
        running += count;
    }
    if running != list.num_elements as usize {
        return Err(Broken::Partition {
            list: name,
            stated: list.num_elements as usize,
            held: running,
        });
    }
    Ok(out)
}

/// A split array's start indices, read at whatever alignment a save laid them.
fn start_indices<T>(list: &View<'_, Raw<AiSplitArray<T>>>) -> Result<Vec<u32>, Broken>
where
    T: Owned,
    Raw<T>: Plain + Element + Param,
{
    let (ptr, n) = ({ list.start_indices }, list.num_sub_arrays as usize);
    if ptr.is_null() || n == 0 {
        return Ok(Vec::new());
    }
    let image = View::image(list);
    let bytes = image
        .resolve(ptr.raw())
        .and_then(|at| image.slice_at(at, 4 * n))
        .map_err(|e| ViewError::BadPointer {
            ty: "AiSplitArray".into(),
            field: "start_indices",
            address: ptr.raw(),
            reason: e.to_string().into(),
        })?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect())
}

fn list<'a, T>(
    ptr: Option<View<'a, Raw<AiSplitArray<T>>>>,
    name: &'static str,
) -> Result<Vec<&'a Raw<T>>, Broken>
where
    T: Owned,
    Raw<T>: Plain + Element + Param,
{
    ptr.map_or(Ok(Vec::new()), |l| items::<T>(&l, name))
}

/// Every array's partition, the polygons' index and special link runs, the edge count and every index.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), Broken> {
    let mesh = View::<Raw<CNavMesh>>::at_offset(image, root)?;
    let vertices = list::<CNavMeshCompressedVertex>(mesh.compressed_vertex_array()?, "vertex")?;
    let indices = list::<u16>(mesh.vertex_index_array()?, "index")?;
    let edges = list::<TAdjPoly>(mesh.adjacent_polys_array()?, "edge")?;
    let polygons = list::<TNavMeshPoly>(mesh.polys_array()?, "polygon")?;
    let (mut running, mut links) = (0usize, 0usize);
    for (i, p) in polygons.iter().enumerate() {
        let stated = (p.as_integer2 & 0xffff) as usize;
        if stated != running {
            return Err(Broken::IndexRuns {
                polygon: i,
                stated,
                running,
            });
        }
        running += ((p.as_integer1 >> 21) & 0xf) as usize;
        links += ((p.as_integer4 >> 12) & 0x7) as usize;
    }
    if running != indices.len() {
        return Err(Broken::IndexRunsOpen {
            closed: running,
            indices: indices.len(),
        });
    }
    let special = mesh.special_links_indices()?.len();
    if links != special {
        return Err(Broken::SpecialLinkRunsOpen {
            counted: links,
            indices: special,
        });
    }
    if edges.len() != indices.len() {
        return Err(Broken::EdgeCount {
            edges: edges.len(),
            indices: indices.len(),
        });
    }
    if let Some(bad) = indices
        .iter()
        .map(|i| usize::from(**i))
        .find(|v| *v >= vertices.len())
    {
        return Err(Broken::VertexIndex {
            vertex: bad,
            vertices: vertices.len(),
        });
    }
    Ok(())
}
