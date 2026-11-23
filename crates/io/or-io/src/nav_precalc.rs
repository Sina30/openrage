//! `navprecalc.dat`: a `NAVP` table of cells, each two signed values and four
//! counted groups of four-byte entries, then a `BITS` block, little-endian.
//!
//! ⚠ No code in the game names this file, so past the counts the header states
//! every field stays as stated. The one measured meaning: the shipped file's
//! 3600 cells, laid out 60 to a row, draw the world's outline.

use crate::error::{IoError, Result};

const TAG: &[u8; 4] = b"NAVP";
const BITS_TAG: &[u8; 4] = b"BITS";
const HEADER_LEN: usize = 16;

/// One precalc file: its cells and the `BITS` block after them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NavPrecalc {
    /// The two 16-bit values after the tag, 3 and 1 in the shipped file.
    pub header: [u16; 2],
    pub cells: Vec<NavPrecalcCell>,
    /// The `BITS` block's bytes after its tag and length.
    pub bits: Vec<u8>,
}

/// One cell: two signed values (the first never above the second in the shipped file) and its entries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NavPrecalcCell {
    pub values: [i16; 2],
    /// Four groups, each count stated as a byte before the entries.
    pub groups: [Vec<[u8; 4]>; 4],
}

impl NavPrecalcCell {
    /// The cell's entries over its four groups.
    pub fn entries(&self) -> usize {
        self.groups.iter().map(Vec::len).sum()
    }
}

impl NavPrecalc {
    /// Reads a whole file, refusing a tag, count or length the bytes do not bear out.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != TAG {
            return Err(refuse("the tag is not `NAVP`".into()));
        }
        let header = [r.u16()?, r.u16()?];
        let num_cells = r.u32()? as usize;
        let num_entries = r.u32()? as usize;
        if num_cells > bytes.len() / 8 {
            return Err(refuse(format!(
                "{num_cells} cells cannot fit {} bytes",
                bytes.len()
            )));
        }
        let mut cells = Vec::with_capacity(num_cells);
        for _ in 0..num_cells {
            let values = [r.u16()? as i16, r.u16()? as i16];
            let counts = r.take(4)?.to_vec();
            let mut groups: [Vec<[u8; 4]>; 4] = Default::default();
            for (group, &count) in groups.iter_mut().zip(&counts) {
                for _ in 0..count {
                    let e = r.take(4)?;
                    group.push([e[0], e[1], e[2], e[3]]);
                }
            }
            cells.push(NavPrecalcCell { values, groups });
        }
        let counted: usize = cells.iter().map(NavPrecalcCell::entries).sum();
        if counted != num_entries {
            return Err(refuse(format!(
                "the header states {num_entries} entries and the cells hold {counted}"
            )));
        }
        if r.take(4)? != BITS_TAG {
            return Err(refuse(format!(
                "no `BITS` block follows the cells at byte {}",
                r.at - 4
            )));
        }
        let bits_len = r.u32()? as usize;
        let bits = r.take(bits_len)?.to_vec();
        if r.at != bytes.len() {
            return Err(refuse(format!(
                "{} bytes follow the `BITS` block",
                bytes.len() - r.at
            )));
        }
        Ok(Self {
            header,
            cells,
            bits,
        })
    }

    /// Writes the file, refusing a group of more than 255 entries or a count past u32.
    pub fn save(&self) -> Result<Vec<u8>> {
        let u32_of = |n: usize, what: &str| {
            u32::try_from(n).map_err(|_| refuse(format!("{n} {what} passes u32")))
        };
        let entries: usize = self.cells.iter().map(NavPrecalcCell::entries).sum();
        let mut out = Vec::with_capacity(HEADER_LEN + 8 * self.cells.len() + 4 * entries);
        out.extend_from_slice(TAG);
        for v in self.header {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&u32_of(self.cells.len(), "cells")?.to_le_bytes());
        out.extend_from_slice(&u32_of(entries, "entries")?.to_le_bytes());
        for (i, cell) in self.cells.iter().enumerate() {
            for v in cell.values {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for group in &cell.groups {
                out.push(u8::try_from(group.len()).map_err(|_| {
                    refuse(format!("cell {i} has a group of {} entries", group.len()))
                })?);
            }
            for entry in cell.groups.iter().flatten() {
                out.extend_from_slice(entry);
            }
        }
        out.extend_from_slice(BITS_TAG);
        out.extend_from_slice(&u32_of(self.bits.len(), "BITS bytes")?.to_le_bytes());
        out.extend_from_slice(&self.bits);
        Ok(out)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let got = self
            .bytes
            .get(self.at..self.at.saturating_add(n))
            .ok_or_else(|| {
                refuse(format!(
                    "the file ends at {} before byte {}",
                    self.bytes.len(),
                    self.at.saturating_add(n)
                ))
            })?;
        self.at += n;
        Ok(got)
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

fn refuse(why: String) -> IoError {
    IoError::InvalidData {
        context: format!("navprecalc: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn precalc() -> NavPrecalc {
        NavPrecalc {
            header: [3, 1],
            cells: vec![
                NavPrecalcCell::default(),
                NavPrecalcCell {
                    values: [-6, 4],
                    groups: [
                        vec![],
                        vec![[0, 0xfe, 0xc1, 0xff]],
                        vec![],
                        vec![[1, 2, 3, 4]],
                    ],
                },
            ],
            bits: vec![0x81, 0xff, 0x01],
        }
    }

    #[test]
    fn cells_and_bits_round_trip_with_their_counts() {
        let bytes = precalc().save().expect("saves");
        assert_eq!(&bytes[8..16], &[2, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(&bytes[24..32], &[0xfa, 0xff, 4, 0, 0, 1, 0, 1]);
        assert_eq!(&bytes[40..48], b"BITS\x03\0\0\0");
        assert_eq!(NavPrecalc::open(&bytes).expect("reopens"), precalc());
    }

    #[test]
    fn damaged_bytes_are_refused_by_name() {
        let bytes = precalc().save().expect("saves");
        let mut wrong_total = bytes.clone();
        wrong_total[12] = 3;
        let error = NavPrecalc::open(&wrong_total).unwrap_err().to_string();
        assert!(error.contains("navprecalc"), "{error}");
        let mut long = bytes.clone();
        long.push(0);
        assert!(NavPrecalc::open(&long).is_err());
        assert!(NavPrecalc::open(&bytes[..bytes.len() - 1]).is_err());
        let mut big = precalc();
        big.cells[1].groups[0] = vec![[0; 4]; 256];
        assert!(big.save().is_err());
    }
}
