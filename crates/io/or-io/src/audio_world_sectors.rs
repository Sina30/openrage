//! `audioworldsectorsinfo.dat`: the audio engine's 100 x 100 grid of world
//! sectors, four bytes each, row by row, with no header.
//!
//! ⚠ The last byte packs the tree count above the water bit, the big-endian
//! bitfield order: set across the shipped grid, the low bit traces the
//! island's coastline, so it is the water bit.

use crate::error::{IoError, Result};

/// Sectors along X, and along Y.
pub const SECTORS_ACROSS: usize = 100;
const SECTOR_LEN: usize = 4;

/// The whole grid, `SECTORS_ACROSS` sectors per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioWorldSectors {
    pub sectors: Vec<AudSector>,
}

impl Default for AudioWorldSectors {
    fn default() -> Self {
        Self {
            sectors: vec![AudSector::default(); SECTORS_ACROSS * SECTORS_ACROSS],
        }
    }
}

/// One sector's counts, as the file states them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudSector {
    pub num_highway_nodes: u8,
    pub tallest_building: u8,
    pub num_buildings: u8,
    /// Seven bits: 0..=127.
    pub num_trees: u8,
    pub is_water_sector: bool,
}

impl AudioWorldSectors {
    /// Reads the whole grid, refusing any length but 100 x 100 sectors.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let expected = SECTORS_ACROSS * SECTORS_ACROSS * SECTOR_LEN;
        if bytes.len() != expected {
            return Err(refuse(format!(
                "{} bytes, not {expected} for {SECTORS_ACROSS} x {SECTORS_ACROSS} sectors",
                bytes.len()
            )));
        }
        let sectors = bytes
            .as_chunks::<SECTOR_LEN>()
            .0
            .iter()
            .map(|s| AudSector {
                num_highway_nodes: s[0],
                tallest_building: s[1],
                num_buildings: s[2],
                num_trees: s[3] >> 1,
                is_water_sector: s[3] & 1 != 0,
            })
            .collect();
        Ok(Self { sectors })
    }

    /// Writes the grid back, refusing a count the file cannot hold.
    pub fn save(&self) -> Result<Vec<u8>> {
        if self.sectors.len() != SECTORS_ACROSS * SECTORS_ACROSS {
            return Err(refuse(format!(
                "{} sectors, not {SECTORS_ACROSS} x {SECTORS_ACROSS}",
                self.sectors.len()
            )));
        }
        let mut out = Vec::with_capacity(self.sectors.len() * SECTOR_LEN);
        for (i, s) in self.sectors.iter().enumerate() {
            if s.num_trees > 0x7f {
                return Err(refuse(format!(
                    "sector {i} has {} trees; seven bits hold 127",
                    s.num_trees
                )));
            }
            out.extend_from_slice(&[
                s.num_highway_nodes,
                s.tallest_building,
                s.num_buildings,
                (s.num_trees << 1) | u8::from(s.is_water_sector),
            ]);
        }
        Ok(out)
    }
}

fn refuse(why: String) -> IoError {
    IoError::InvalidData {
        context: format!("audio world sectors: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sector_packs_its_trees_above_the_water_bit() {
        let mut grid = AudioWorldSectors::default();
        grid.sectors[1] = AudSector {
            num_highway_nodes: 0,
            tallest_building: 0x21,
            num_buildings: 4,
            num_trees: 17,
            is_water_sector: false,
        };
        grid.sectors[2].is_water_sector = true;
        let bytes = grid.save().expect("saves");
        assert_eq!(bytes.len(), 40_000);
        assert_eq!(&bytes[4..12], &[0, 0x21, 4, 0x22, 0, 0, 0, 1]);
        assert_eq!(AudioWorldSectors::open(&bytes).expect("reopens"), grid);

        assert!(AudioWorldSectors::open(&bytes[..39_996]).is_err());
        grid.sectors[0].num_trees = 128;
        assert!(grid.save().is_err());
    }
}
