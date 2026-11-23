//! A first entry that is raw DEFLATE streams, one per entry of its archive, each listed as a body
//! of its own; and the zlib streams a resource block's bytes are cut into.

use std::io::{Read, Seek};

use flate2::{Decompress, FlushDecompress, Status};

use super::{
    classify, Reader, Rpf9Block, Rpf9BodyKind, Rpf9Content, Rpf9Scan, Rpf9ScanError, Rpf9ScanFind,
    CHUNK_LIMIT, INPUT_STEP, PIECE, RPF9_SLOT, STREAM_CAP, TEXT_PROBE,
};

/// The fewest bytes a raw DEFLATE entry inflates to.
const LEAST_OUT: u64 = 16;
/// The most streams a block's partition is walked through, past the 256 MiB a section reaches.
const MOST_STREAMS: usize = 4096;

/// One DEFLATE stream inflated: the compressed bytes it took, what it inflated to, its first bytes.
struct Stream {
    consumed: u64,
    produced: u64,
    head: Vec<u8>,
}

/// Whether a raw DEFLATE entry's head names a metadata document, the only content measured.
fn metadata(content: Rpf9Content) -> bool {
    matches!(content, Rpf9Content::Pso | Rpf9Content::Rbf)
}

impl<R: Read + Seek> Reader<R> {
    /// The DEFLATE stream at `at`, zlib-headed where `zlib`, its bytes appended to `out` when
    /// given; `None` where none ends there, or past `limit` inflated bytes with no `out`.
    fn deflate_stream(
        &mut self,
        at: u64,
        zlib: bool,
        limit: u64,
        mut out: Option<&mut Vec<u8>>,
    ) -> Result<Option<Stream>, Rpf9ScanError> {
        let mut z = Decompress::new(zlib);
        let mut buf = vec![0u8; 32 << 10];
        let mut head = Vec::new();
        loop {
            let input = self.get(at + z.total_in(), INPUT_STEP)?;
            if input.is_empty() {
                return Ok(None);
            }
            let (before_in, before_out) = (z.total_in(), z.total_out());
            let Ok(status) = z.decompress(input, &mut buf, FlushDecompress::None) else {
                return Ok(None);
            };
            let produced = (z.total_out() - before_out) as usize;
            if z.total_out() > limit {
                return match out {
                    Some(_) => Err(Rpf9ScanError::InflateLimit { offset: at, limit }),
                    None => Ok(None),
                };
            }
            let probe = produced.min(TEXT_PROBE.saturating_sub(head.len()));
            head.extend_from_slice(&buf[..probe]);
            if let Some(out) = &mut out {
                out.extend_from_slice(&buf[..produced]);
            }
            if status == Status::StreamEnd {
                return Ok(Some(Stream {
                    consumed: z.total_in(),
                    produced: z.total_out(),
                    head,
                }));
            }
            if z.total_in() == before_in && produced == 0 {
                return Ok(None);
            }
        }
    }

    /// The first slot from `at`, a slot boundary, that is not all zero, looked for within 64 KiB.
    fn after_zero_slots(&mut self, at: u64) -> Result<u64, Rpf9ScanError> {
        let mut next = at;
        while next < at + PIECE {
            let slot = self.get(next, RPF9_SLOT as usize)?;
            if slot.is_empty() || slot.iter().any(|b| *b != 0) {
                break;
            }
            next += RPF9_SLOT;
        }
        Ok(next)
    }

    /// The raw DEFLATE stream at `at` and its bytes, inflated, where it inflates to metadata.
    pub(super) fn raw_deflate_body(
        &mut self,
        at: u64,
        limit: u64,
    ) -> Result<Option<(Rpf9ScanFind, Vec<u8>)>, Rpf9ScanError> {
        let mut bytes = Vec::new();
        let Some(stream) = self.deflate_stream(at, false, limit, Some(&mut bytes))? else {
            return Ok(None);
        };
        let content = classify(&stream.head, stream.produced);
        if stream.produced < LEAST_OUT || !metadata(content) {
            return Ok(None);
        }
        let span = stream.consumed.next_multiple_of(RPF9_SLOT);
        Ok(Some((raw_find(at, content, &stream, span), bytes)))
    }
}

fn raw_find(offset: u64, content: Rpf9Content, stream: &Stream, span: u64) -> Rpf9ScanFind {
    Rpf9ScanFind {
        offset,
        kind: Rpf9BodyKind::RawDeflate { content },
        span,
        stored_len: Some(stream.consumed),
        inflated_len: Some(stream.produced),
        container: None,
    }
}

impl<R: Read + Seek> Rpf9Scan<R> {
    /// The first of the raw DEFLATE streams the first entry at `at` starts, the others queued:
    /// where the streams, each metadata and zero-padded to a slot, are as many as the entries
    /// its nested archive states.
    pub(super) fn raw_deflate_entries(
        &mut self,
        at: u64,
    ) -> Result<Option<Rpf9ScanFind>, Rpf9ScanError> {
        let Some((_, _, entries)) = self.carry.archive else {
            return Ok(None);
        };
        let entries = entries as usize;
        let mut streams = Vec::new();
        let mut next = at;
        // One stream past the entries is tried, so a run longer than its archive is refused.
        while streams.len() <= entries {
            let Some(stream) = self.reader.deflate_stream(next, false, CHUNK_LIMIT, None)? else {
                break;
            };
            let content = classify(&stream.head, stream.produced);
            if stream.produced < LEAST_OUT || !metadata(content) {
                break;
            }
            let end = (next + stream.consumed).next_multiple_of(RPF9_SLOT);
            streams.push((next, content, stream));
            next = self.reader.after_zero_slots(end)?;
        }
        if streams.len() != entries {
            return Ok(None);
        }
        let mut claimed = Vec::with_capacity(entries);
        for (i, (offset, content, stream)) in streams.iter().enumerate() {
            let last = stream.consumed.next_multiple_of(RPF9_SLOT);
            let following = streams.get(i + 1);
            let span = following.map_or(last, |(after, ..)| after - offset);
            claimed.push(self.claim(raw_find(*offset, *content, stream, span)));
        }
        let mut claimed = claimed.into_iter();
        let first = claimed.next();
        self.queued.extend(claimed);
        Ok(first)
    }
}

/// What each zlib stream of `block`, a block of a resource in `source`, inflates to, in order.
pub fn rpf9_block_streams<R: Read + Seek>(
    source: R,
    block: &Rpf9Block,
) -> Result<Vec<u64>, Rpf9ScanError> {
    if !block.zlib {
        return Ok(vec![block.inflated_len]);
    }
    let mut reader = Reader::new(source)?;
    let (mut at, end) = (block.offset, block.offset + block.stored_len);
    let mut streams = Vec::new();
    while at < end && streams.len() < MOST_STREAMS {
        let stream = reader.deflate_stream(at, true, STREAM_CAP, None)?;
        let stream = stream.ok_or(Rpf9ScanError::ResourceRefused {
            offset: block.offset,
            at,
            why: "a block's zlib stream does not inflate",
        })?;
        streams.push(stream.produced);
        at = (at + stream.consumed).next_multiple_of(RPF9_SLOT);
    }
    Ok(streams)
}
