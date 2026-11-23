//! `worldblanket.dat`: a square grid of 16-bit cells with no header, read
//! little-endian.
//!
//! ⚠ Nothing in the file states its side, its orientation or what a cell
//! means, and the one shipped file (in the `stripped` level, whose data file
//! list does not name it) holds only 0 and 1, so the cells stay stated values.
//! Little-endian is measured: a set cell's bytes are `01 00`.

use crate::error::{IoError, Result};

/// One world blanket: its cells, row by row as stored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorldBlanket {
    pub cells: Vec<u16>,
}

impl WorldBlanket {
    /// Reads a whole file, refusing a length that is not a square of two-byte cells.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let cells = bytes.len() / 2;
        let side = cells.isqrt();
        if cells == 0 || !bytes.len().is_multiple_of(2) || side * side != cells {
            return Err(IoError::InvalidData {
                context: format!(
                    "worldblanket: {} bytes is not a square grid of two-byte cells",
                    bytes.len()
                ),
            });
        }
        Ok(Self {
            cells: bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect(),
        })
    }

    /// Writes the cells back as stored.
    pub fn save(&self) -> Vec<u8> {
        self.cells.iter().flat_map(|c| c.to_le_bytes()).collect()
    }

    /// The grid's side; a cell count that is not square rounds down.
    pub fn side(&self) -> usize {
        self.cells.len().isqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_grid_round_trips_and_anything_else_is_refused() {
        let blanket = WorldBlanket {
            cells: vec![0, 1, 0, 0, 0, 0, 1, 0, 0],
        };
        let bytes = blanket.save();
        assert_eq!(&bytes[2..4], &[1, 0]);
        let reopened = WorldBlanket::open(&bytes).expect("reopens");
        assert_eq!((reopened.side(), &reopened), (3, &blanket));
        for bad in [&bytes[..17], &bytes[..16], &[][..]] {
            assert!(WorldBlanket::open(bad).is_err(), "{} bytes", bad.len());
        }
    }
}
