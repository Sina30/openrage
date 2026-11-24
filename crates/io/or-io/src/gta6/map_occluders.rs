//! The occluders a GTA VI map body states: a `CMapData`'s `boxOccluders` and `occludeModels`, each
//! in the world as a culling pass takes it.
//!
//! ⚠ A box states no position: its corners are in a frame turned about the world's origin, and a
//! corner stands where each of its three coordinates moves along one row of the box's matrix.

use super::lights::{check_table, Layouts};
use super::{
    aabb, broken, check_layouts, floats, u16_at, u32_at, u64_at, Aabb, Blocks, MapError, Result,
    ARRAY, C_MAP_DATA, C_MAP_TYPES, MAP_DATA_SIZE, STRUCT, U32, VEC3V,
};
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::rbm::schema::RbmModel;

pub const BOX_OCCLUDER: u32 = lit(b"BoxOccluder");
pub const OCCLUDE_MODEL: u32 = lit(b"OccludeModel");
/// The class of the one member a `BoxOccluder` holds: a box and the matrix that turns it.
const TURNED_BOX: u32 = 0x1E5B_E266;
/// The type of a block of bytes.
const BYTES: u32 = 0x11;

const U16: u8 = 0x13;
const MAT33V: u8 = 0x35;
const COUNTED_POINTER: u8 = 0x5A;
const BOX_SIZE: usize = 0x50;
const MODEL_SIZE: usize = 0x40;

const LAYOUTS: &Layouts = &[
    (
        C_MAP_DATA,
        "CMapData",
        &[
            (lit(b"boxOccluders"), 0x110, ARRAY),
            (lit(b"occludeModels"), 0x120, ARRAY),
        ],
        None,
    ),
    (
        BOX_OCCLUDER,
        "BoxOccluder",
        &[(0xBFB7_9EE6, 0x00, STRUCT)],
        Some(BOX_SIZE as u32),
    ),
    (
        TURNED_BOX,
        "the box of a BoxOccluder",
        &[
            (lit(b"localBox"), 0x00, STRUCT),
            (lit(b"localToWorld"), 0x20, MAT33V),
        ],
        Some(BOX_SIZE as u32),
    ),
    (
        OCCLUDE_MODEL,
        "OccludeModel",
        &[
            (lit(b"bmin"), 0x00, VEC3V),
            (lit(b"bmax"), 0x10, VEC3V),
            (lit(b"dataSize"), 0x20, U32),
            (lit(b"verts"), 0x28, COUNTED_POINTER),
            (lit(b"numVertsInBytes"), 0x30, U16),
            (lit(b"numTris"), 0x32, U16),
            (lit(b"flags"), 0x34, U32),
        ],
        Some(MODEL_SIZE as u32),
    ),
];

/// A box that hides what is behind it, in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxOccluder {
    /// Corner `k` is at the low side of the box's own x where bit 0 of `k` is clear, of its y for bit 1 and of its z for bit 2.
    pub corners: [[f32; 3]; 8],
}

/// A mesh that hides what is behind it, in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct OccludeModel {
    pub bounds: Aabb,
    pub flags: u32,
    pub vertices: Vec<[f32; 3]>,
    /// Three indices into `vertices` a triangle.
    pub triangles: Vec<[u8; 3]>,
}

/// The occluders of one map.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapOccluders {
    pub boxes: Vec<BoxOccluder>,
    pub models: Vec<OccludeModel>,
}

/// The triangles of a [`BoxOccluder`]'s corners, two a face, each wound to face out of the box.
pub const BOX_TRIANGLES: [[u8; 3]; 12] = [
    [0, 4, 6],
    [0, 6, 2],
    [1, 3, 7],
    [1, 7, 5],
    [0, 1, 5],
    [0, 5, 4],
    [2, 7, 3],
    [2, 6, 7],
    [0, 2, 3],
    [0, 3, 1],
    [4, 5, 7],
    [4, 7, 6],
];

impl MapOccluders {
    /// Every triangle of the boxes, then of the models, as three points of the world.
    pub fn triangles(&self) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
        let boxes = (self.boxes.iter())
            .flat_map(|b| BOX_TRIANGLES.map(|t| t.map(|corner| b.corners[usize::from(corner)])));
        let models = self.models.iter().flat_map(|m| {
            (m.triangles.iter()).filter_map(move |t| {
                let point = |index: u8| m.vertices.get(usize::from(index)).copied();
                Some([point(t[0])?, point(t[1])?, point(t[2])?])
            })
        });
        boxes.chain(models)
    }
}

/// The occluders of the `CMapData` body these two sections hold; both lists empty for a map with none.
pub fn open(virtual_section: &[u8], physical: &[u8]) -> Result<MapOccluders> {
    read(&crate::meta::binary::rbm::gta6::read_model(
        virtual_section,
        physical,
    )?)
}

/// The occluders of a `CMapData` body's class table and data blocks.
pub fn read(model: &RbmModel) -> Result<MapOccluders> {
    check_layouts(model)?;
    check_table(model, LAYOUTS)?;
    let blocks = Blocks::new(&model.data_blocks);
    let root = blocks.block(u64::from(model.root_block_id), "the root block")?;
    match root.data_type {
        C_MAP_DATA if root.data.len() >= MAP_DATA_SIZE => {}
        C_MAP_DATA => return Err(broken("the root block", "it is shorter than its class")),
        C_MAP_TYPES => {
            return Err(broken(
                "the root block",
                "it is a CMapTypes, where occluders lie in a CMapData",
            ))
        }
        other => return Err(MapError::NotAMap(other)),
    }
    let boxes = (blocks
        .records(&root.data, 0x110, BOX_OCCLUDER, BOX_SIZE, "boxOccluders")?
        .into_iter()
        .enumerate())
    .map(|(k, b)| box_occluder(b, &format!("boxOccluders[{k}]")))
    .collect::<Result<_>>()?;
    let models = (blocks
        .records(
            &root.data,
            0x120,
            OCCLUDE_MODEL,
            MODEL_SIZE,
            "occludeModels",
        )?
        .into_iter()
        .enumerate())
    .map(|(k, m)| occlude_model(&blocks, m, &format!("occludeModels[{k}]")))
    .collect::<Result<_>>()?;
    Ok(MapOccluders { boxes, models })
}

fn box_occluder(b: &[u8], what: &str) -> Result<BoxOccluder> {
    let local = aabb(b, 0x00, what)?;
    let rows: [[f32; 3]; 3] = [
        floats(b, 0x20, what)?,
        floats(b, 0x30, what)?,
        floats(b, 0x40, what)?,
    ];
    for i in 0..3 {
        for j in i..3 {
            let dot: f32 = (0..3).map(|k| rows[i][k] * rows[j][k]).sum();
            let wanted = if i == j { 1.0 } else { 0.0 };
            if (dot - wanted).abs() >= 1e-3 {
                return Err(broken(
                    what,
                    "the rows of its localToWorld are not three unit axes at right angles",
                ));
            }
        }
    }
    Ok(BoxOccluder {
        corners: std::array::from_fn(|corner| {
            // A box may state an axis from its high side to its low one.
            let own: [f32; 3] = std::array::from_fn(|axis| {
                let (one, other) = (local.min[axis], local.max[axis]);
                if corner >> axis & 1 == 0 {
                    one.min(other)
                } else {
                    one.max(other)
                }
            });
            std::array::from_fn(|i| (0..3).map(|axis| own[axis] * rows[axis][i]).sum::<f32>())
        }),
    })
}

fn occlude_model(blocks: &Blocks, m: &[u8], what: &str) -> Result<OccludeModel> {
    let bounds = aabb(m, 0x00, what)?;
    if (0..3).any(|i| bounds.min[i] > bounds.max[i]) {
        return Err(broken(what, "its bmin is over its bmax"));
    }
    let data_size = u32_at(m, 0x20)? as usize;
    let vertex_bytes = usize::from(u16_at(m, 0x30)?);
    let triangles = usize::from(u16_at(m, 0x32)?);
    if !vertex_bytes.is_multiple_of(12) || data_size != vertex_bytes + 3 * triangles {
        return Err(broken(
            what,
            format!(
                "its dataSize {data_size} is not {vertex_bytes} bytes of vertices and {triangles} triangles"
            ),
        ));
    }
    // The vertices, twelve bytes each, then the triangles, three bytes each.
    let data: &[u8] = match blocks.target(u64_at(m, 0x28)?, what)? {
        Some((BYTES, data)) if data.len() >= data_size => &data[..data_size],
        None if data_size == 0 => &[],
        _ => {
            return Err(broken(
                what,
                format!("its verts name no block of {data_size} bytes"),
            ))
        }
    };
    let (points, indices) = data.split_at(vertex_bytes);
    let vertices = (0..vertex_bytes / 12)
        .map(|k| floats(points, 12 * k, what))
        .collect::<Result<Vec<[f32; 3]>>>()?;
    let triangles: Vec<[u8; 3]> = indices.as_chunks::<3>().0.to_vec();
    if (triangles.iter().flatten()).any(|index| usize::from(*index) >= vertices.len()) {
        return Err(broken(
            what,
            format!(
                "a triangle names a vertex past its {} vertices",
                vertices.len()
            ),
        ));
    }
    Ok(OccludeModel {
        bounds,
        flags: u32_at(m, 0x34)?,
        vertices,
        triangles,
    })
}

#[cfg(test)]
#[path = "map_occluders_tests.rs"]
mod tests;
