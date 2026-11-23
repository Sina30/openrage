//! BCPack, the console's entropy coder over BC blocks, which a `.ttpf` body of codec kind 1 to 7
//! is stored through: the kind is the BC number of the blocks.
//!
//! A body is one byte stream shared by 12 or 16 rANS lanes, each giving one symbol for every 8
//! decoded bytes. The lanes of every mode decode here; only BC1's symbols are rebuilt into blocks.
//!
//! A block's endpoints are predicted from an earlier block, and a reference to before a body's
//! first block reads a 256-slot preset that is a constant of the coder and is in no stream: only
//! BC1's is known from a source OpenRage can take it from, and the other modes' bodies read theirs.

mod bc1;
mod bc1_tables;
mod rans;
mod tables;

/// Decoded bytes for each symbol of a lane, in every mode.
const ELEMENT_BYTES: u64 = 8;

/// Why a BCPack body does not decode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BcPackError {
    #[error("codec kind {kind} is not a BCPack mode: the modes are 1 and 3 to 7")]
    NotAMode { kind: u8 },
    #[error(
        "the entropy layer of BC{kind} decodes, but {}, so the body does not decode",
        missing(*kind)
    )]
    NoBlockTables { kind: u8 },
    #[error("a decoded size of {size} bytes is not a whole, non-zero number of 8-byte elements")]
    Size { size: u64 },
    #[error("lane {lane}'s fragment header narrows its symbols below one bit")]
    Header { lane: usize },
    #[error(
        "the stream ends after {consumed} bytes with symbols still owed; a piece of the file may \
         be missing"
    )]
    Truncated { consumed: usize },
    #[error("lanes {lanes:?} do not end in a state the coder leaves a lane in")]
    EndState { lanes: Vec<usize> },
    #[error("the lanes read {consumed} bytes, and the {rest} after them are not all zero")]
    Trailing { consumed: usize, rest: usize },
    #[error("block {block} carries {symbol} in lane 1, which is zero in every measured body")]
    Lane1 { block: usize, symbol: u16 },
    #[error(
        "block {block} is predicted from slot {slot} of the endpoints preset before a body's \
         first block, and no value is known for that slot"
    )]
    PresetNotKnown { block: usize, slot: usize },
}

/// What rebuilding the blocks of codec kind `kind` lacks.
fn missing(kind: u8) -> &'static str {
    match kind {
        4 | 5 => {
            "its blocks follow a 256-slot preset of endpoints, which every measured body reads, \
             and a table of index pairs, and no source OpenRage can take them from gives either"
        }
        3 => {
            "its blocks follow a 256-slot preset of endpoints, which most measured bodies read, \
             and tables of index pairs and quadrants, and no source OpenRage can take them from \
             gives them"
        }
        _ => {
            "which lane feeds which field of a block is not measured, and its blocks follow \
             presets of endpoints and indices that no source OpenRage can take them from gives"
        }
    }
}

/// Each lane's symbol width in bits for codec kind `kind`, in stream order; `None` if the kind is no BCPack mode a pack uses.
pub fn lane_widths(kind: u8) -> Option<&'static [u8]> {
    Some(match kind {
        1 => &[8, 8, 5, 6, 5, 5, 6, 5, 8, 8, 8, 8],
        3 => &[4, 4, 8, 6, 6, 6, 6, 5, 6, 5, 8, 8],
        4 => &[8, 8, 8, 8, 6, 6, 6, 6, 6, 6, 6, 6],
        5 => &[4, 4, 8, 8, 6, 6, 6, 6, 6, 6, 6, 6],
        6 => &[4, 4, 8, 8, 8, 5, 5, 5, 6, 6, 6, 8, 8, 8, 8, 5],
        7 => &[4, 4, 8, 8, 8, 8, 8, 7, 7, 5, 6, 8, 8, 8, 8, 5],
        _ => return None,
    })
}

/// Whether [`decode`] rebuilds blocks for codec kind `kind`.
pub fn decodes(kind: u8) -> bool {
    kind == 1
}

/// The symbols of each lane of a body of codec kind `kind` that decodes to `size` bytes: `size / 8` for each lane.
pub fn lanes(kind: u8, stored: &[u8], size: u64) -> Result<Vec<Vec<u16>>, BcPackError> {
    let widths = lane_widths(kind).ok_or(BcPackError::NotAMode { kind })?;
    let target = usize::try_from(size / ELEMENT_BYTES)
        .ok()
        .filter(|&n| n > 0 && size.is_multiple_of(ELEMENT_BYTES))
        .ok_or(BcPackError::Size { size })?;
    let alphabet_bits: Vec<u8> = widths.iter().map(|width| width - 1).collect();
    let mut bytes = stored.iter().copied();
    let (lanes, consumed) =
        rans::decode_lanes(|_| bytes.next(), &alphabet_bits, target, stored.len())?;
    if stored[consumed..].iter().any(|&b| b != 0) {
        return Err(BcPackError::Trailing {
            consumed,
            rest: stored.len() - consumed,
        });
    }
    Ok(lanes)
}

/// The `size` bytes of BC blocks a body of codec kind `kind` decodes to, in the order the body's row stores them.
pub fn decode(kind: u8, stored: &[u8], size: u64) -> Result<Vec<u8>, BcPackError> {
    let lanes = lanes(kind, stored, size)?;
    match kind {
        1 => bc1::blocks(&lanes),
        _ => Err(BcPackError::NoBlockTables { kind }),
    }
}

#[cfg(test)]
pub(super) mod tests;
