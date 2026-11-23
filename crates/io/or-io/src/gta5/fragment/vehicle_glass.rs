//! A vehicle's breakable glass: the `HWGV` block, its windows, and the
//! shattermap each one carries.

/// A glass block, window or shattermap row that cannot be read or spelled.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Malformed(pub String);

pub type Result<T, E = Malformed> = std::result::Result<T, E>;

/// `VGWH` as it appears in the file, which is `HWGV` read as bytes.
pub const VEHICLE_GLASS_MAGIC: [u8; 4] = *b"HWGV";

/// `VGWC` as it appears in the file, which is `CWGV` read as bytes.
pub const VEHICLE_GLASS_WINDOW_MAGIC: [u8; 4] = *b"CWGV";

/// The block's own prologue: magic, header stride, count, total length.
pub const BLOCK_HEADER_LEN: usize = 0x0C;

/// One window's fixed part, before its shattermap.
pub const WINDOW_HEADER_LEN: usize = 0x70;

/// `Start2` when a row has no second run.
///
/// ⚠ A sentinel in the same byte a column index is spelled with, so column
/// 255 of a 256-wide map cannot start a second run. No shipped map is that
/// wide.
pub const NO_SECOND_RUN: u8 = 255;

/// One row of a shattermap, as the file spells it: up to two runs of health
/// bytes, each at its own column.
///
/// ⚠ The two runs may be adjacent — `infernus.yft` splits one 37-cell row as
/// `[0..0]` then `[1..36]` — so where the second run starts is data a save
/// keeps, not something it can recompute from the cells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShatterMapRow {
    pub start1: u8,
    pub data1: Vec<u8>,
    pub start2: u8,
    pub data2: Vec<u8>,
}

impl ShatterMapRow {
    /// How many bytes this row occupies in the packed form.
    ///
    /// ⚠ Not `2 + data1.len() + 2 + data2.len()`. A row with no first run is
    /// two bytes and states no `start2` at all; a row whose `start2` is
    /// [`NO_SECOND_RUN`] states no `end2`. The offsets table addresses rows by
    /// this number, so a caller computing it the obvious way mis-addresses
    /// every row after the first.
    pub fn packed_len(&self) -> usize {
        if self.data1.is_empty() {
            return 2;
        }
        let mut len = 2 + self.data1.len() + 1;
        if self.start2 != NO_SECOND_RUN {
            len += 1 + self.data2.len();
        }
        len
    }

    fn read(bytes: &[u8], at: usize) -> Result<(Self, usize)> {
        let mut cursor = at;
        let start1 = *byte_at(bytes, cursor)?;
        let end1 = *byte_at(bytes, cursor + 1)?;
        cursor += 2;

        let run1 = (end1 as i32 - start1 as i32) + 1;
        if run1 <= 0 {
            return Ok((
                Self {
                    start1,
                    data1: Vec::new(),
                    start2: NO_SECOND_RUN,
                    data2: Vec::new(),
                },
                cursor - at,
            ));
        }

        let data1 = slice_at(bytes, cursor, run1 as usize)?.to_vec();
        cursor += run1 as usize;

        let start2 = *byte_at(bytes, cursor)?;
        cursor += 1;
        if start2 == NO_SECOND_RUN {
            return Ok((
                Self {
                    start1,
                    data1,
                    start2,
                    data2: Vec::new(),
                },
                cursor - at,
            ));
        }

        let end2 = *byte_at(bytes, cursor)?;
        cursor += 1;
        let run2 = (end2 as i32 - start2 as i32) + 1;
        let data2 = if run2 > 0 {
            let run = slice_at(bytes, cursor, run2 as usize)?.to_vec();
            cursor += run2 as usize;
            run
        } else {
            Vec::new()
        };

        Ok((
            Self {
                start1,
                data1,
                start2,
                data2,
            },
            cursor - at,
        ))
    }

    fn write(&self, out: &mut Vec<u8>) {
        if self.data1.is_empty() {
            // ⚠ An empty row is spelled `end1 < start1`, and there is no
            // sentinel — the reader keys on the subtraction. So `start1` of
            // zero has nothing below it and a wrapped `end1` of 255 reads back
            // as a 256-cell run rather than as nothing. Column one is the
            // lowest an empty row can claim, and the column it claims means
            // nothing when there is no run at it.
            let start = self.start1.max(1);
            out.push(start);
            out.push(start - 1);
            return;
        }
        out.push(self.start1);
        out.push(self.start1 + (self.data1.len() as u8 - 1));
        out.extend_from_slice(&self.data1);
        out.push(self.start2);
        if self.start2 != NO_SECOND_RUN {
            if self.data2.is_empty() {
                out.push(self.start2.wrapping_sub(1));
            } else {
                out.push(self.start2 + (self.data2.len() as u8 - 1));
                out.extend_from_slice(&self.data2);
            }
        }
    }
}

/// One window's shattermap: its rows, and the width they are read against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShatterMap {
    pub width: u16,
    pub rows: Vec<ShatterMapRow>,
}

impl ShatterMap {
    /// The row-offsets table and the rows, as the file holds them.
    ///
    /// ⚠ The offsets are relative to the end of the table, not to its start.
    pub fn encode(&self) -> Vec<u8> {
        let mut rows = Vec::new();
        let mut offsets = Vec::with_capacity(self.rows.len() * 2);
        let mut offset = 0usize;
        for row in &self.rows {
            offsets.extend_from_slice(&(offset as u16).to_le_bytes());
            offset += row.packed_len();
            row.write(&mut rows);
        }
        offsets.extend_from_slice(&rows);
        offsets
    }
}

/// One pane of a vehicle's breakable glass.
///
/// The 112-byte header is carried whole, because only its shattermap is
/// modelled and a field named for a guess is worse than a byte run named for
/// its offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleGlassWindow {
    /// The window's own 112 bytes, shattermap counts included.
    pub header: [u8; WINDOW_HEADER_LEN],
    pub shatter_map: ShatterMap,
}

impl VehicleGlassWindow {
    pub fn item_id(&self) -> u16 {
        u16::from_le_bytes([self.header[0x44], self.header[0x45]])
    }

    /// The shattermap's length in bytes, its offsets table included.
    pub fn item_data_byte_length(&self) -> u16 {
        u16::from_le_bytes([self.header[0x4C], self.header[0x4D]])
    }
}

/// `fwVehicleGlassWindowData`: a vehicle's whole `HWGV` block, decoded.
///
/// ⚠ The `(item_id, offset)` table and each window's trailing padding are
/// derived: a save writes them from the windows, and a block that is not
/// their derivation is refused at open. The table has one **more** slot than
/// there are windows when the count is odd, padded to a pair.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FwVehicleGlassWindowData {
    pub windows: Vec<VehicleGlassWindow>,
    /// `Unknown_4h`, the per-window header stride the block states.
    pub header_stride: u16,
    /// The four bytes between the offsets table and the first window.
    pub after_offsets: u32,
}

impl FwVehicleGlassWindowData {
    /// Decode the block a fragment carries in `vehicle_window_data`.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < BLOCK_HEADER_LEN || bytes[..4] != VEHICLE_GLASS_MAGIC {
            return Err(Malformed(
                "vehicle glass block does not open HWGV".to_string(),
            ));
        }
        let header_stride = u16::from_le_bytes([bytes[4], bytes[5]]);
        let item_count = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let total_length = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        if total_length > bytes.len() {
            return Err(Malformed(format!(
                "vehicle glass block states {total_length} bytes but only {} are carried",
                bytes.len()
            )));
        }

        let slots = item_count + (item_count & 1);
        let mut cursor = BLOCK_HEADER_LEN + slots * 8;
        let mut after_offsets = [0u8; 4];
        after_offsets.copy_from_slice(slice_at(bytes, cursor, 4)?);
        let after_offsets = u32::from_le_bytes(after_offsets);
        cursor += 4;

        let mut windows = Vec::with_capacity(item_count);
        for _ in 0..item_count {
            let mut header = [0u8; WINDOW_HEADER_LEN];
            header.copy_from_slice(slice_at(bytes, cursor, WINDOW_HEADER_LEN)?);
            if header[0x40..0x44] != VEHICLE_GLASS_WINDOW_MAGIC {
                return Err(Malformed(format!(
                    "vehicle glass window {} does not open VGWC",
                    windows.len()
                )));
            }
            cursor += WINDOW_HEADER_LEN;

            let row_count = u16::from_le_bytes([header[0x4A], header[0x4B]]) as usize;
            let item_data_byte_length = u16::from_le_bytes([header[0x4C], header[0x4D]]);
            let width = u16::from_le_bytes([header[0x48], header[0x49]]);

            let mut rows = Vec::with_capacity(row_count);
            if item_data_byte_length != 0 {
                // The offsets table is read past rather than followed: it
                // restates where each row begins, and the rows are contiguous.
                cursor += row_count * 2;
                for _ in 0..row_count {
                    let (row, len) = ShatterMapRow::read(bytes, cursor)?;
                    cursor += len;
                    rows.push(row);
                }
            }

            cursor += (16 - (cursor % 16)) % 16;

            windows.push(VehicleGlassWindow {
                header,
                shatter_map: ShatterMap { width, rows },
            });
        }

        Ok(Self {
            windows,
            header_stride,
            after_offsets,
        })
    }

    /// Re-encode the block.
    ///
    /// ⚠ The offsets table and `total_length` are **recomputed**, so a caller
    /// that changed a shattermap's length gets a coherent block. Each window's
    /// trailing padding is written as zeros.
    pub fn write(&self) -> Vec<u8> {
        let item_count = self.windows.len();
        let slots = item_count + (item_count & 1);
        let prologue = BLOCK_HEADER_LEN + slots * 8 + 4;

        let mut body = Vec::new();
        let mut offsets = Vec::with_capacity(slots);
        for window in &self.windows {
            offsets.push((window.item_id() as u32, (prologue + body.len()) as u32));
            body.extend_from_slice(&window.header);
            if window.item_data_byte_length() != 0 {
                body.extend_from_slice(&window.shatter_map.encode());
            }
            let pad = (16 - ((prologue + body.len()) % 16)) % 16;
            body.extend(std::iter::repeat_n(0u8, pad));
        }
        while offsets.len() < slots {
            offsets.push((0, 0));
        }

        let mut out = Vec::with_capacity(prologue + body.len());
        out.extend_from_slice(&VEHICLE_GLASS_MAGIC);
        out.extend_from_slice(&self.header_stride.to_le_bytes());
        out.extend_from_slice(&(item_count as u16).to_le_bytes());
        out.extend_from_slice(&((prologue + body.len()) as u32).to_le_bytes());
        for (id, off) in &offsets {
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&off.to_le_bytes());
        }
        out.extend_from_slice(&self.after_offsets.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }
}

fn byte_at(bytes: &[u8], at: usize) -> Result<&u8> {
    bytes.get(at).ok_or_else(|| {
        Malformed(format!(
            "vehicle glass block ends at {} but a shattermap row reaches {at}",
            bytes.len()
        ))
    })
}

fn slice_at(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    bytes.get(at..at + len).ok_or_else(|| {
        Malformed(format!(
            "vehicle glass block ends at {} but {len} bytes were wanted at {at}",
            bytes.len()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_run_row() -> ShatterMapRow {
        ShatterMapRow {
            start1: 2,
            data1: vec![0xFF, 0xF0, 0x80],
            start2: 8,
            data2: vec![0x40, 0x10],
        }
    }

    /// ⚠ The row's own length rule, which the offsets table depends on.
    ///
    /// The empty row is in here on purpose: it is the one whose length is not
    /// `2 + data1 + 2 + data2`, and the one whose `end1` cannot simply be
    /// `start1 - 1`.
    #[test]
    fn a_rows_packed_length_is_what_the_encoder_emits() {
        for row in [
            two_run_row(),
            ShatterMapRow {
                start1: 0,
                data1: vec![1, 2, 3],
                start2: NO_SECOND_RUN,
                data2: Vec::new(),
            },
            ShatterMapRow::default(),
        ] {
            let mut out = Vec::new();
            row.write(&mut out);
            assert_eq!(out.len(), row.packed_len(), "{row:?}");
            let (back, len) = ShatterMapRow::read(&out, 0).unwrap();
            assert_eq!(len, out.len(), "{row:?}");
            assert_eq!(back.data1, row.data1, "{row:?}");
            assert_eq!(back.data2, row.data2, "{row:?}");
            if !row.data1.is_empty() {
                assert_eq!(back.start1, row.start1, "{row:?}");
            }
        }
    }
}
