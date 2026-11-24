//! A pack's cube or array given another size, format or mip count: a cube's rows each hold its six
//! faces at one size, the last at its own; an array's rows each hold one piece of one slice's
//! chain, piece after piece.

use super::super::super::{
    malformed, smallest_row_holding, Entry, TexturePack, TtpfError, TEXTURE_LIMIT,
};
use super::super::TOP_ROW;
use super::{Planned, Shape};
use crate::codec::texture::DxgiFormat;
use crate::gta6::tiling::{chain, chain_lengths, Level, Tile};

/// What a cube's or an array's planner gives: its levels, rows and places, and the flags, nibble
/// and tile mode its entry states.
struct Laid {
    tile: Tile,
    levels: Vec<Level>,
    rows: Vec<u64>,
    places: Vec<Vec<(usize, Level)>>,
    flags: usize,
    nibble: usize,
    tile_mode: u8,
}

impl TexturePack {
    /// The entry, rows and places of cube or array `index` given `shape`, as the pack's own cubes and arrays have them.
    pub(super) fn planned_slices(
        &self,
        index: usize,
        was: Entry,
        shape: Shape,
    ) -> Result<Planned, TtpfError> {
        let laid = match was.dimension {
            2 => cube(index, shape)?,
            _ => array(index, shape, u64::from(was.slices))?,
        };
        let Shape {
            width,
            height,
            format,
            ..
        } = shape;
        let grows = if format.dxgi() == DxgiFormat::Bc6hSf16 {
            16
        } else {
            1
        };
        if grows * laid.rows.iter().sum::<u64>() > TEXTURE_LIMIT {
            return Err(TtpfError::ShapeTooLarge {
                index,
                width,
                height,
            });
        }
        if laid.flags > 7 || laid.nibble > 15 {
            let mips = laid.levels.len() as u32;
            return Err(TtpfError::NibbleCannotState { index, mips });
        }
        let dxgi = u16::try_from(format.dxgi().to_raw()).unwrap_or_default();
        let entry = Entry {
            width,
            height,
            mip_count: laid.nibble as u8,
            format_code: dxgi << 3 | laid.flags as u16,
            tile_mode: laid.tile_mode,
            row_codes: [0; 4],
            small_rows: 0,
            align_pages: 0,
            ..was
        };
        Ok(Planned {
            entry,
            format,
            tile: laid.tile,
            levels: laid.levels,
            rows: laid.rows,
            places: laid.places,
        })
    }
}

/// A cube in 4 KiB tiles: flags 0 for one level, else the explicit levels of a face's chain, at
/// most 3; `flags + 1` rows, row j from the last the six faces at size `w >> j`, each row the
/// smallest a row-size code states; the tile mode the row count, and 0 for four rows.
fn cube(index: usize, shape: Shape) -> Result<Laid, TtpfError> {
    let Shape {
        width,
        height,
        format,
        mips,
    } = shape;
    let bytes = format.element_bytes();
    if format.element_side() == 1 && bytes != 4 {
        return Err(TtpfError::CubeNotMeasured { index, bytes });
    }
    let tile = Tile::of(format, false);
    let lengths = chain_lengths(width, height, format, &tile);
    let explicit = lengths.len().saturating_sub(1);
    let whole = lengths.last().copied().unwrap_or_default();
    let all = chain(width, height, format, &tile, false, whole);
    let all = all.unwrap_or_default();
    let mips = mips.unwrap_or(all.len() as u32);
    let wanted = mips as usize;
    if wanted == 0 || wanted > all.len() {
        return Err(TtpfError::MipsNotPlaced {
            index,
            mips,
            most: all.len(),
        });
    }
    // A face of several levels holds its whole chain, which a nibble of 0 or 1 does not state,
    // and at least its explicit levels.
    let flags = if wanted == 1 { 0 } else { explicit.min(3) };
    let nibble = if wanted == 1 {
        1
    } else {
        wanted.saturating_sub(flags)
    };
    if wanted > 1 && (wanted < explicit || nibble < 2) {
        return Err(TtpfError::NibbleCannotState { index, mips });
    }
    let (mut rows, mut places, mut levels) = (Vec::new(), vec![Vec::new(); 6], Vec::new());
    let mut base = 0;
    for j in (0..=flags).rev() {
        let (w, h) = ((width >> j).max(1), (height >> j).max(1));
        let lengths = chain_lengths(w, h, format, &tile);
        let (unit, lone) = match wanted {
            1 => (lengths.first(), true),
            _ => (lengths.last(), false),
        };
        let unit = unit.copied().unwrap_or_default();
        let face = chain(w, h, format, &tile, lone, unit).unwrap_or_default();
        let row = smallest_row_holding(6 * unit);
        let row = row.ok_or(TtpfError::RowNotCoded { index })?;
        for (f, written) in places.iter_mut().enumerate() {
            let at = base + f as u64 * unit;
            for (k, level) in face.iter().enumerate().filter(|(k, _)| j + k < wanted) {
                let placed = Level {
                    at: at + level.at,
                    ..*level
                };
                written.push((j + k, placed));
            }
        }
        if j == 0 {
            levels = face.into_iter().take(wanted).collect();
        }
        rows.push(row);
        base += row;
    }
    if levels.len() != wanted {
        return Err(malformed(format!(
            "texture {index}'s new faces place {} mips, not {wanted}",
            levels.len()
        )));
    }
    let tile_mode = match rows.len() {
        4 => 0,
        n => n as u8,
    };
    Ok(Laid {
        tile,
        levels,
        rows,
        places,
        flags,
        nibble,
        tile_mode,
    })
}

/// An array in 64 KiB tiles: flags its slice chain's explicit levels, 1 for one level, and the
/// nibble the tail levels it holds; a row for each piece of each slice, piece after piece.
fn array(index: usize, shape: Shape, slices: u64) -> Result<Laid, TtpfError> {
    let Shape {
        width,
        height,
        format,
        mips,
    } = shape;
    let tile = Tile::of(format, true);
    let lengths = chain_lengths(width, height, format, &tile);
    let explicit = lengths.len().saturating_sub(1);
    let whole = lengths.last().copied().unwrap_or_default();
    let most = chain(width, height, format, &tile, false, whole);
    let mips = mips.unwrap_or(most.map_or(1, |levels| levels.len().max(1)) as u32);
    let wanted = mips as usize;
    let lone = wanted == 1 && explicit == 0;
    let all = chain(width, height, format, &tile, lone, whole);
    let all = all.unwrap_or_default();
    if wanted == 0 || wanted > all.len() {
        return Err(TtpfError::MipsNotPlaced {
            index,
            mips,
            most: all.len(),
        });
    }
    // Levels that stop above the tail are a chain of their own, with no tail tile.
    let (unit, levels) = if wanted <= explicit {
        let unit = lengths[wanted - 1];
        let levels = chain(width, height, format, &tile, false, unit);
        (unit, levels.unwrap_or_default())
    } else {
        (whole, all[..wanted].to_vec())
    };
    if levels.len() != wanted {
        return Err(malformed(format!(
            "texture {index}'s new slices place {} mips, not {wanted}",
            levels.len()
        )));
    }
    let held = levels.iter().filter(|l| !l.tail).count();
    let tails = wanted - held;
    let flags = if wanted == 1 { 1 } else { held };
    // The pieces in chain order: the tail tile, then each explicit level smallest first.
    let mut pieces: Vec<(u64, u64)> = (levels.iter())
        .filter(|l| !l.tail)
        .map(|l| (l.at, l.len))
        .collect();
    if tails > 0 {
        pieces.push((0, tile.bytes()));
    }
    pieces.sort_unstable();
    let sum: u64 = pieces.iter().map(|&(_, len)| len).sum();
    if sum != unit || pieces.iter().any(|&(_, len)| len > TOP_ROW) {
        return Err(TtpfError::RowNotCoded { index });
    }
    let rows: Vec<u64> = (pieces.iter())
        .flat_map(|&(_, len)| std::iter::repeat_n(len, slices as usize))
        .collect();
    let places: Vec<Vec<(usize, Level)>> = (0..slices)
        .map(|slice| {
            let placed = |level: &Level| {
                let from = level.from();
                let at = slices * from + slice * level.len + level.at - from;
                Level { at, ..*level }
            };
            levels.iter().map(placed).enumerate().collect()
        })
        .collect();
    Ok(Laid {
        tile,
        levels,
        rows,
        places,
        flags,
        nibble: tails,
        tile_mode: 0,
    })
}
