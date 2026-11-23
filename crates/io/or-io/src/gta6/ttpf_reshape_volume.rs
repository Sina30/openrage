//! A pack's volume of one level given another width, height or format: its z-slabs of 4 KiB tiles
//! in one row, as the pack's own volumes hold them, its depth kept.

use super::super::super::{malformed, Entry, TexturePack, TtpfError};
use super::super::TOP_ROW;
use super::{Planned, Shape};
use crate::gta6::tiling::{volume_chain, Tile};

impl TexturePack {
    /// The entry, row and places of volume `index` given `shape`: tile mode 1, flags 0, a nibble of 1 and one row, each z-plane a slice.
    pub(super) fn planned_volume(
        &self,
        index: usize,
        was: Entry,
        shape: Shape,
    ) -> Result<Planned, TtpfError> {
        if was.tile_mode != 1 {
            let tile_mode = was.tile_mode;
            return Err(TtpfError::Volume { index, tile_mode });
        }
        let Shape {
            width,
            height,
            format,
            mips,
        } = shape;
        // The flags, nibble and rows of a pack volume of several levels are not measured.
        let mips = mips.unwrap_or(1);
        if mips != 1 {
            return Err(TtpfError::MipsNotPlaced {
                index,
                mips,
                most: 1,
            });
        }
        let tile = Tile::volume(format).ok_or(TtpfError::ReshapeShape { index })?;
        let side = format.element_side();
        let (w, h) = (
            width.div_ceil(side) as usize,
            height.div_ceil(side) as usize,
        );
        let slab = (w.div_ceil(tile.width) * h.div_ceil(tile.height)) as u64 * tile.bytes();
        let depth = u32::from(was.slices);
        let row = slab * u64::from(depth).div_ceil(tile.depth as u64);
        // A measured volume is one row of at most 512 KiB; how a larger one is cut is not known.
        if row > TOP_ROW {
            return Err(TtpfError::RowNotCoded { index });
        }
        let levels = volume_chain([width, height, depth], format, &tile, 1, row)
            .map(|(levels, _)| levels)
            .filter(|levels| levels.len() == 1)
            .ok_or_else(|| {
                malformed(format!(
                    "texture {index}'s new z-slabs are not one level of {width}x{height}x{depth}"
                ))
            })?;
        let places = vec![vec![(0, levels[0])]; usize::from(was.slices)];
        let dxgi = u16::try_from(format.dxgi().to_raw()).unwrap_or_default();
        let entry = Entry {
            width,
            height,
            mip_count: 1,
            format_code: dxgi << 3,
            tile_mode: 1,
            row_codes: [0; 4],
            small_rows: 0,
            align_pages: 0,
            ..was
        };
        Ok(Planned {
            entry,
            format,
            tile,
            levels,
            rows: vec![row],
            places,
        })
    }
}
