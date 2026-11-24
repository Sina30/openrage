//! The region lists at the head of GTA VI's level archive: for each cell of a grid, the file that
//! holds the cell's region, and for some the map the region goes with. Read only.
//!
//! ⚠ They are no table of the map streamer: a row names a region's file by a hash no map body
//! carries, so which maps a camera streams in is still each map's own box.

use super::mission_tables::{Body, MissionTableError};

/// The type of a list's root: one counted array of region headers.
pub const REGION_LIST: u32 = 0xD56F_1E2C;
/// The type of a region header.
pub const REGION_HEADER: u32 = 0x9C83_4886;
/// The bytes of a region header.
pub const HEADER: usize = 0x20;

/// Why bytes are not a region list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MapGridError {
    #[error(transparent)]
    Body(#[from] MissionTableError),
    #[error("the body is no region list: {0}")]
    NotAList(&'static str),
}

/// One region of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// The cell, counted from the world's origin in cells of the list's own size.
    pub x: i32,
    pub y: i32,
    pub version: u32,
    /// The 32-bit name of the file that holds the region.
    pub file_name: u32,
    /// The region's place in that file, where one file holds several.
    pub index: u32,
    /// The name word of the map the region goes with; 0 for a region of the world at rest.
    pub map_word: u64,
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(std::array::from_fn(|k| bytes[at + k]))
}

fn le64(bytes: &[u8], at: usize) -> u64 {
    u64::from(le32(bytes, at)) | (u64::from(le32(bytes, at + 4)) << 32)
}

/// The regions a list states, in the order it states them; refused where the bytes are no list.
pub fn read(bytes: &[u8]) -> Result<Vec<Region>, MapGridError> {
    let body = Body::open(bytes)?;
    let no = MapGridError::NotAList;
    let mut roots = body.blocks.iter().filter(|b| b.class == REGION_LIST);
    let (Some(root), None) = (roots.next(), roots.next()) else {
        return Err(no("it does not hold one block of the list's type"));
    };
    let head = (root.offset.checked_add(16))
        .filter(|_| root.size >= 16)
        .and_then(|end| bytes.get(root.offset..end))
        .ok_or(no("its root is shorter than the array it holds"))?;
    // A counted array is its count, its capacity and a pointer: a block's number from 1 in the
    // low 12 bits and a byte offset from bit 20.
    let (count, capacity, pointer) = (le32(head, 0), le32(head, 4), le64(head, 8));
    if count > capacity {
        return Err(no("its array counts more regions than it has room for"));
    }
    if pointer == 0 && count == 0 {
        return Ok(Vec::new());
    }
    if (pointer >> 12) & 0xFF != 0 || pointer & 0xFFF == 0 {
        return Err(no("its array's pointer names no block"));
    }
    let block = (body.blocks.get((pointer & 0xFFF) as usize - 1))
        .filter(|b| b.class == REGION_HEADER && b.element.map(|e| e.0) == Some(HEADER))
        .ok_or(no(
            "its array does not point at a block of 32-byte region headers",
        ))?;
    let rows = usize::try_from(pointer >> 20)
        .ok()
        .filter(|offset| offset.is_multiple_of(HEADER))
        .and_then(|offset| {
            let len = (count as usize).checked_mul(HEADER)?;
            let end = offset.checked_add(len).filter(|end| *end <= block.size)?;
            bytes.get(block.offset.checked_add(offset)?..block.offset.checked_add(end)?)
        })
        .ok_or(no("its regions are not whole headers inside their block"))?;
    Ok(rows
        .chunks_exact(HEADER)
        .map(|row| Region {
            x: le32(row, 0) as i32,
            y: le32(row, 4) as i32,
            version: le32(row, 8),
            file_name: le32(row, 0x0C),
            index: le32(row, 0x10),
            map_word: le64(row, 0x18),
        })
        .collect())
}

#[cfg(test)]
#[path = "map_grid_tests.rs"]
mod tests;
