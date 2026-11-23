//! The bodies after a nested archive's first entry that no detector names: islands of bytes that
//! zlib cannot shrink, that start no codec and hold no GTA V script program, stated and never read.

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};

/// Zero bytes in a row that end a piece of a body.
const GAP: usize = 16;
/// The bytes from a piece's start within which a zlib head makes it a body's later piece.
const HEAD_REACH: usize = 32;
/// A stream inside a first piece that inflates this many bytes has its clear prefix checked.
const CHECKED_PREFIX: u64 = 1024;
/// The clear bytes a stream keeps for that check.
const KEPT: usize = 4096;
/// The bytes past which a stream counts as compressed data: ciphertext stops after a few.
const MOST_INFLATED: u64 = 64 << 20;
const PAGE: u64 = 0x1000;

/// Why a body is refused as opaque: a rule shows the classification is wrong.
#[derive(Debug, thiserror::Error)]
#[error("the body at {at:#X} breaks rule {rule}, so it is not an opaque body: {why}")]
pub struct NotOpaque {
    pub rule: &'static str,
    pub at: u64,
    pub why: &'static str,
}

/// Where a body starts against the 4 KiB page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// At a page's start.
    Page,
    /// 16 bytes into a page, behind a zero slot, as a headerless resource is placed.
    AfterZeroSlot,
    /// Elsewhere, 16-aligned, after the zero pad of the body before.
    Unpaged,
}

/// A run of a body's bytes between runs of 16 or more zero bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub offset: u64,
    pub len: u64,
    /// Where a later piece's zlib stream starts; `None` for a body's first piece.
    pub zlib_head: Option<u64>,
}

/// An entry of the story script archives' coder, which nothing decodes: a body behind a zero
/// slot, whose first two bytes are a tag of a small alphabet and whose next six are a head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoderEntry {
    pub tag: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueBody {
    pub offset: u64,
    pub placement: Placement,
    pub pieces: Vec<Piece>,
    /// The coder entry the body is by where it is placed; `None` leaves the body unnamed.
    pub coder: Option<CoderEntry>,
}

/// The opaque bodies of a span, and where the names its archive's header states lie in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueSpan {
    pub bodies: Vec<OpaqueBody>,
    pub names: Option<u64>,
}

/// The bodies of `span`, the bytes at `start` up to the next row a scan lists; see [`OpaqueSpan`].
// A last body of one piece as long as the archive's names is those names.
pub fn opaque_bodies(
    span: &[u8],
    start: u64,
    names_len: Option<u64>,
) -> Result<OpaqueSpan, NotOpaque> {
    let mut bodies: Vec<Vec<Piece>> = Vec::new();
    for (s, e) in islands(span) {
        let offset = start + s as u64;
        let head = zlib_head_within(&span[s..e], offset);
        // A piece not 16-aligned, or with a zlib head, goes on the body before it.
        let later = !offset.is_multiple_of(16) || head.is_some();
        if !later || bodies.is_empty() {
            bodies.push(Vec::new());
        }
        if let Some(body) = bodies.last_mut() {
            body.push(Piece {
                offset,
                len: (e - s) as u64,
                zlib_head: head.map(|o| offset + o as u64),
            });
        }
    }
    let mut names = None;
    if let Some(&[last]) = bodies.last().map(Vec::as_slice) {
        if Some(last.len) == names_len {
            names = Some(last.offset);
            bodies.pop();
        }
    }
    let bodies = (bodies.into_iter())
        .map(|pieces| check(span, start, pieces))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OpaqueSpan { bodies, names })
}

/// The `[start, end)` runs of `bytes` that start non-zero, each ending where 16 zero bytes start.
fn islands(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(s) = (bytes.get(i..)).and_then(|rest| rest.iter().position(|b| *b != 0)) {
        let s = i + s;
        let mut zeros = 0;
        let mut e = bytes.len();
        for (k, b) in bytes.iter().enumerate().skip(s) {
            zeros = if *b == 0 { zeros + 1 } else { 0 };
            if zeros == GAP {
                e = k + 1 - GAP;
                break;
            }
        }
        out.push((s, e));
        i = e;
    }
    out
}

fn is_zlib_head(bytes: &[u8], at: usize) -> bool {
    bytes.get(at) == Some(&0x78) && matches!(bytes.get(at + 1), Some(0x01 | 0x5E | 0x9C | 0xDA))
}

/// The first 16-aligned place in the first 32 bytes of `piece`, at `offset`, with a zlib head.
fn zlib_head_within(piece: &[u8], offset: u64) -> Option<usize> {
    let first = (16 - (offset % 16) as usize) % 16;
    (first..HEAD_REACH.min(piece.len()))
        .step_by(16)
        .find(|o| is_zlib_head(piece, *o))
}

/// Whether the GTA V script program's pointer words, at +0x10, +0x60 and +0x68, lie at +0 or +0x10.
fn holds_script_program(bytes: &[u8]) -> bool {
    let word = |at: usize| {
        (bytes.get(at..at + 8)).map(|w| u64::from_le_bytes(std::array::from_fn(|k| w[k])))
    };
    let words_at = |base: usize| {
        [0x10usize, 0x60, 0x68]
            .into_iter()
            .all(|o| word(base + o).is_some_and(|w| w >> 28 == 5))
    };
    words_at(0) || words_at(0x10)
}

/// The coder entry a body placed as `placement` is, whose first piece is `first`.
// The framing's zero bytes are the slot before the body: no pad lies inside one, and a body at
// a page's start carries no tag of the alphabet.
fn coder_entry(first: &[u8], placement: Placement) -> Option<CoderEntry> {
    let head = first.first_chunk::<8>()?;
    (placement == Placement::AfterZeroSlot).then(|| CoderEntry {
        tag: u16::from_le_bytes([head[0], head[1]]),
    })
}

/// What a zlib stream at the start of some bytes gives before it stops.
struct Stream {
    /// It ended, or inflated past [`MOST_INFLATED`].
    ends: bool,
    inflated: u64,
    prefix: Vec<u8>,
}

fn inflate(bytes: &[u8]) -> Stream {
    let mut z = Decompress::new(true);
    let mut out = vec![0u8; 1 << 15];
    let mut prefix = Vec::new();
    loop {
        let (taken, given) = (z.total_in(), z.total_out());
        let input = bytes.get(taken as usize..).unwrap_or_default();
        let status = z.decompress(input, &mut out, FlushDecompress::None);
        let made = (z.total_out() - given) as usize;
        let room = KEPT.saturating_sub(prefix.len());
        prefix.extend_from_slice(&out[..made.min(room)]);
        let ends = match status {
            Ok(Status::StreamEnd) => true,
            Ok(_) if z.total_out() > MOST_INFLATED => true,
            Ok(_) if z.total_in() > taken || made > 0 => continue,
            _ => false,
        };
        return Stream {
            ends,
            inflated: z.total_out(),
            prefix,
        };
    }
}

/// The bytes zlib at level 9 writes of `bytes`.
fn deflated_len(bytes: &[u8]) -> u64 {
    let mut z = ZlibEncoder::new(std::io::sink(), Compression::best());
    // A sink takes every write, so neither call fails.
    (z.write_all(bytes).and_then(|()| z.try_finish())).map_or(u64::MAX, |()| z.total_out())
}

/// The bytes of `piece`, of the span that starts at `start`.
fn bytes_of<'a>(span: &'a [u8], start: u64, piece: &Piece) -> &'a [u8] {
    let from = (piece.offset - start) as usize;
    &span[from..from + piece.len as usize]
}

/// `pieces` as an opaque body, refused by the rule (O1 to O6) that shows it is not one.
fn check(span: &[u8], start: u64, pieces: Vec<Piece>) -> Result<OpaqueBody, NotOpaque> {
    let at = pieces[0].offset;
    let no = |rule, why| Err(NotOpaque { rule, at, why });
    let first = bytes_of(span, start, &pieces[0]);
    if zlib_head_within(first, at).is_some() {
        return no(
            "O1",
            "its first piece starts with a zlib stream, which a scan lists",
        );
    }
    // A real zlib stream also passes: this tells ciphertext from structured data, not from compression.
    if deflated_len(first).saturating_mul(50) < 49 * first.len() as u64 {
        return no(
            "O2",
            "zlib shrinks its first piece below 0.98 of its length",
        );
    }
    if holds_script_program(first) {
        return no(
            "O3",
            "its first piece holds a GTA V script program's pointer words",
        );
    }
    let aligned = (16 - (at % 16) as usize) % 16;
    for o in (aligned..first.len().saturating_sub(1)).step_by(16) {
        if !is_zlib_head(first, o) {
            continue;
        }
        let stream = inflate(&first[o..]);
        if stream.ends {
            return no(
                "O6",
                "a zlib stream inside its first piece inflates to its end or past 64 MiB",
            );
        }
        if stream.inflated >= CHECKED_PREFIX && holds_script_program(&stream.prefix) {
            return no(
                "O3",
                "a zlib stream inside it inflates to a GTA V script program's pointer words",
            );
        }
    }
    for piece in &pieces[1..] {
        let Some(head) = piece.zlib_head else {
            return no("O4", "a later piece has no zlib head in its first 32 bytes");
        };
        let stream = inflate(&bytes_of(span, start, piece)[(head - piece.offset) as usize..]);
        if stream.ends {
            return no(
                "O5",
                "a later piece's zlib stream inflates to its end or past 64 MiB",
            );
        }
        if holds_script_program(&stream.prefix) {
            return no(
                "O3",
                "a later piece inflates to a GTA V script program's pointer words",
            );
        }
    }
    let placement = match at % PAGE {
        0 => Placement::Page,
        16 => Placement::AfterZeroSlot,
        _ => Placement::Unpaged,
    };
    let coder = coder_entry(first, placement);
    Ok(OpaqueBody {
        offset: at,
        placement,
        pieces,
        coder,
    })
}

#[cfg(test)]
#[path = "script_bodies_tests.rs"]
mod tests;
