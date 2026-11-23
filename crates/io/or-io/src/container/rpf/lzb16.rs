//! Oodle's LZB16 (decoder type 2): a byte-oriented LZ with 16-bit offsets, which RDR2 uses for its
//! small files. Decode and encode, in safe Rust.
//!
//! ⚠ The format is OpenRage's own measurement, not a port. It was read off streams the game's
//! `oo2core` produced and consumed (decoded by it, and encoded by it from inputs chosen to expose each
//! field), and every decode was compared with that library's output.
//!
//! A stream is quanta of 0x40000 output bytes. A quantum is a two-byte header (`0x8C`, or `0xCC` when
//! the quantum is stored, then `0x02`) and pieces of 0x4000 bytes, each led by a big-endian `u16`:
//! `0x7FFF` and a byte repeat that byte; the top bit set stores `(v & 0x7FFF) + 1` bytes; any other
//! value is a coded piece of `v + 1` bytes. A coded piece is a run of units: a token whose low nibble
//! is a literal count and whose high nibble is a match code, the literals, and the match.
//!
//! - A count of 15 is extended by a byte, or by two when the first is `0xC0` or more
//!   (`(b & 0x3F) << 8 | next`).
//! - A match code of 1 to 14 is a length of code + 4 and a 16-bit little-endian offset.
//! - A code of 15 is followed by a byte `x`. Below `0x80` it is a length of 19 + `x` and a 16-bit
//!   offset, and `0x7F` adds an extended count after the offset. From `0x80` its bits 3 to 6 are a
//!   length code (+ 4; 15 is 19 plus an extended count after `x`) and its low three bits an offset of
//!   1 to 7, or the last offset when zero.
//! - A match code of 0 ends the piece's matches: the unit is literals only, and the last unit's
//!   count is however many bytes the piece still lacks.

use crate::error::{IoError, Result};

const QUANTUM: usize = 0x40000;
const PIECE: usize = 0x4000;
const WINDOW: usize = 0xFFFF;
/// The shortest match the token form holds, and the bytes at a piece's end that stay literal.
const MIN_MATCH: usize = 5;
const TAIL: usize = 16;

fn bad(what: &str) -> IoError {
    IoError::DecompressionFailed(format!("LZB16: {what}"))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn byte(&mut self) -> Result<usize> {
        let b = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| bad("stream ends early"))?;
        self.at += 1;
        Ok(usize::from(b))
    }

    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or_else(|| bad("stream ends early"))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn count(&mut self) -> Result<usize> {
        let b = self.byte()?;
        if b < 0xC0 {
            Ok(b)
        } else {
            Ok((b & 0x3F) << 8 | self.byte()?)
        }
    }

    fn offset(&mut self) -> Result<usize> {
        Ok(self.byte()? | self.byte()? << 8)
    }
}

/// Whether `stream` begins an LZB16 stream.
pub(super) fn is_lzb16(stream: &[u8]) -> bool {
    matches!(stream, [b0, b1, ..] if b0 & 0xF == 0xC && b1 & 0x7F == 2)
}

/// A stream of exactly `wanted` bytes.
pub(super) fn decode(stream: &[u8], wanted: usize) -> Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(wanted);
    let mut c = Cursor {
        bytes: stream,
        at: 0,
    };
    while out.len() < wanted {
        let (b0, b1) = (c.byte()?, c.byte()?);
        if b0 & 0xF != 0xC || b1 & 0x7F != 2 {
            return Err(bad("a quantum is not LZB16"));
        }
        let quantum = (wanted - out.len()).min(QUANTUM);
        if b0 & 0x40 != 0 {
            out.extend_from_slice(c.take(quantum)?);
            continue;
        }
        let end = out.len() + quantum;
        while out.len() < end {
            let want = (end - out.len()).min(PIECE);
            let v = c.byte()? << 8 | c.byte()?;
            if v == 0x7FFF {
                let fill = c.byte()?;
                out.resize(out.len() + want, fill as u8);
            } else if v & 0x8000 != 0 {
                let n = (v & 0x7FFF) + 1;
                if n != want {
                    return Err(bad("a stored piece is not the piece's size"));
                }
                out.extend_from_slice(c.take(n)?);
            } else {
                let piece = c.take(v + 1)?;
                piece_into(piece, want, &mut out)?;
            }
        }
    }
    Ok(out)
}

fn piece_into(piece: &[u8], want: usize, out: &mut Vec<u8>) -> Result<()> {
    let target = out.len() + want;
    let mut c = Cursor {
        bytes: piece,
        at: 0,
    };
    let mut last = 1usize;
    while out.len() < target {
        let token = c.byte()?;
        let (lo, hi) = (token & 15, token >> 4);
        let mut lits = lo;
        if lo == 15 {
            lits += c.count()?;
        }
        out.extend_from_slice(c.take(lits.min(target - out.len()))?);
        if out.len() >= target {
            break;
        }
        let (len, off);
        if hi == 15 {
            let x = c.byte()?;
            if x < 0x80 {
                off = c.offset()?;
                len = 19 + x + if x == 0x7F { c.count()? } else { 0 };
            } else {
                let code = (x >> 3) & 15;
                len = if code == 15 {
                    19 + c.count()?
                } else {
                    code + 4
                };
                off = if x & 7 == 0 { last } else { x & 7 };
            }
        } else {
            len = hi + 4;
            off = c.offset()?;
        }
        if off == 0 || off > out.len() {
            return Err(bad("a match reaches before the start"));
        }
        last = off;
        for _ in 0..len.min(target - out.len()) {
            out.push(out[out.len() - off]);
        }
    }
    Ok(())
}

fn put_count(out: &mut Vec<u8>, n: usize) {
    if n < 0xC0 {
        out.push(n as u8);
    } else {
        out.push(0xC0 | (n >> 8) as u8);
        out.push(n as u8);
    }
}

/// A stream the game's own decoder reads back to `data`.
pub(super) fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    for (q, quantum) in data.chunks(QUANTUM).enumerate() {
        let at = out.len();
        out.extend_from_slice(&[0x8C, 0x02]);
        for (p, piece) in quantum.chunks(PIECE).enumerate() {
            let start = q * QUANTUM + p * PIECE;
            encode_piece(data, start, start + piece.len(), &mut out);
        }
        // A quantum no smaller than its bytes is stored as it is.
        if out.len() - at >= quantum.len() + 2 {
            out.truncate(at);
            out.extend_from_slice(&[0xCC, 0x02]);
            out.extend_from_slice(quantum);
        }
    }
    out
}

fn encode_piece(data: &[u8], start: usize, end: usize, out: &mut Vec<u8>) {
    let piece = &data[start..end];
    if let Some(&fill) = piece.first().filter(|&&f| piece.iter().all(|&b| b == f)) {
        out.extend_from_slice(&[0x7F, 0xFF, fill]);
        return;
    }
    let coded = code_piece(data, start, end);
    if coded.len() >= piece.len() {
        let v = 0x8000 | (piece.len() - 1);
        out.extend_from_slice(&[(v >> 8) as u8, v as u8]);
        out.extend_from_slice(piece);
    } else {
        let v = coded.len() - 1;
        out.extend_from_slice(&[(v >> 8) as u8, v as u8]);
        out.extend_from_slice(&coded);
    }
}

fn hash4(data: &[u8], i: usize) -> usize {
    let word = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
    (word.wrapping_mul(0x9E37_79B1) >> 16) as usize
}

/// A piece as units, matching back into the 64 KiB before it; its last bytes stay literal.
fn code_piece(data: &[u8], start: usize, end: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let match_end = end.saturating_sub(TAIL).max(start);
    // The newest position that hashed to each slot; a slot is only a candidate until it is compared.
    let mut heads = vec![usize::MAX; 1 << 16];
    // A quantum is decoded on its own, so nothing before it can be matched.
    let quantum_start = start / QUANTUM * QUANTUM;
    for i in start.saturating_sub(WINDOW).max(quantum_start)..start {
        if i + 4 <= data.len() {
            heads[hash4(data, i)] = i;
        }
    }
    let (mut i, mut lit_from) = (start, start);
    while i + MIN_MATCH <= match_end {
        let slot = hash4(data, i);
        let candidate = heads[slot];
        heads[slot] = i;
        if candidate == usize::MAX || i - candidate > WINDOW {
            i += 1;
            continue;
        }
        let mut len = 0;
        while i + len < match_end && data[candidate + len] == data[i + len] {
            len += 1;
        }
        if len < MIN_MATCH {
            i += 1;
            continue;
        }
        emit_unit(&mut out, &data[lit_from..i], Some((len, i - candidate)));
        for j in i + 1..(i + len).min(match_end) {
            heads[hash4(data, j)] = j;
        }
        i += len;
        lit_from = i;
    }
    emit_unit(&mut out, &data[lit_from..end], None);
    out
}

/// One unit: `lits`, then a match of `(length, offset)` when there is one.
fn emit_unit(out: &mut Vec<u8>, lits: &[u8], matched: Option<(usize, usize)>) {
    let lo = lits.len().min(15);
    // A match of 5 to 18 bytes and an offset past 7 has its length in the token; the other forms use 15.
    let code = match matched {
        None => 0,
        Some((len, off)) if off > 7 && len < 19 => len - 4,
        Some(_) => 15,
    };
    out.push((code << 4 | lo) as u8);
    if lo == 15 {
        put_count(out, lits.len() - 15);
    }
    out.extend_from_slice(lits);
    let Some((len, off)) = matched else {
        return;
    };
    match code {
        15 if off <= 7 => {
            let code = (len - 4).min(15);
            out.push((0x80 | code << 3 | off) as u8);
            if code == 15 {
                put_count(out, len - 19);
            }
        }
        15 => {
            let ext = len - 19;
            out.push(ext.min(0x7F) as u8);
            out.extend_from_slice(&[off as u8, (off >> 8) as u8]);
            if ext >= 0x7F {
                put_count(out, ext - 0x7F);
            }
        }
        _ => out.extend_from_slice(&[off as u8, (off >> 8) as u8]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(data: &[u8]) {
        let packed = encode(data);
        assert_eq!(decode(&packed, data.len()).expect("decodes"), data);
    }

    #[test]
    fn a_stream_is_read_as_the_games_library_wrote_it() {
        // `8c 02 7f ff 00` is 300 zeros; the 8 bytes of text are a stored quantum.
        assert_eq!(
            decode(&[0x8C, 2, 0x7F, 0xFF, 0], 300).unwrap(),
            vec![0; 300]
        );
        assert_eq!(
            decode(&[0xCC, 2, b'a', b'b', b'c'], 3).unwrap(),
            b"abc".to_vec()
        );
    }

    #[test]
    fn a_unit_with_a_short_offset_repeats_the_byte_before() {
        // "ab" then a run of 5 at offset 1... as a short descriptor: token ff (15 literals + ext 0),
        // 15 literals, then x = 0x88 | (1 << 3) | 1, then the closing literals.
        let mut piece = vec![0xFF, 0];
        piece.extend((0..15u8).map(|b| b + 1));
        piece.push(0x80 | (1 << 3) | 1);
        piece.extend_from_slice(&[0x08, 9, 8, 7, 6, 5, 4, 3, 2]);
        let mut stream = vec![0x8C, 2, 0, (piece.len() - 1) as u8];
        stream.extend_from_slice(&piece);
        let want = 15 + 5 + 8;
        let got = decode(&stream, want).unwrap();
        assert_eq!(&got[14..20], &[15, 15, 15, 15, 15, 15]);
        assert_eq!(&got[20..], &[9, 8, 7, 6, 5, 4, 3, 2]);
    }

    #[test]
    fn what_is_encoded_decodes() {
        round(b"");
        round(b"x");
        round(&[0; 70_000]);
        round(
            &(0..200_000u32)
                .map(|i| (i * 7 % 251) as u8)
                .collect::<Vec<_>>(),
        );
        let text: Vec<u8> = b"the quick brown fox jumps over the lazy dog. "
            .iter()
            .cycle()
            .take(300_000)
            .copied()
            .collect();
        round(&text);
        let mut noise = 0x2545_F491u32;
        let random: Vec<u8> = (0..90_000)
            .map(|_| {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                noise as u8
            })
            .collect();
        round(&random);
        let mut mixed = text[..40_000].to_vec();
        mixed.extend_from_slice(&random[..30_000]);
        mixed.extend_from_slice(&text[..90_000]);
        round(&mixed);
    }

    #[test]
    fn a_hostile_stream_is_refused_not_trusted() {
        assert!(decode(&[0x8C, 2, 0x00, 0x02, 0xFF], 10).is_err());
        assert!(decode(&[0x8C, 2, 0x00, 0x03, 0x0F, 5, 0xF0, 0x7F], 10).is_err());
        assert!(decode(&[0x8C, 6, 0, 0], 4).is_err());
    }
}
