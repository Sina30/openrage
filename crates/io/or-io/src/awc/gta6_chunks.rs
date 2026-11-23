//! Two chunks a GTA VI stream may carry beside its audio: level records and a stored animation
//! resource.

use super::{AwcChunkKind, AwcStream};
use crate::container::rpf::rsc8::Rsc8;
use crate::error::{IoError, Result};

const LOUDNESS_RECORD: usize = 8;

/// One record of a stream's [`AwcChunkKind::Loudness`] chunk: a sample position and a level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AwcLoudness {
    /// As stated: 4,800 times the record's index in nearly every stream, not in all.
    pub position: u32,
    pub value: f32,
}

impl AwcStream {
    /// The records of the stream's loudness chunk; `None` when it has none.
    pub fn loudness(&self) -> Option<Result<Vec<AwcLoudness>>> {
        let payload = &self.chunk(AwcChunkKind::Loudness)?.payload;
        if payload.len() % LOUDNESS_RECORD != 0 {
            return Some(Err(IoError::Invalid {
                what: "a loudness chunk".into(),
                why: format!("its {} bytes are not whole 8-byte records", payload.len()),
            }));
        }
        let records = payload
            .as_chunks::<LOUDNESS_RECORD>()
            .0
            .iter()
            .map(|r| AwcLoudness {
                position: u32::from_le_bytes([r[0], r[1], r[2], r[3]]),
                value: f32::from_le_bytes([r[4], r[5], r[6], r[7]]),
            });
        Some(Ok(records.collect()))
    }

    /// The RSC8 resource the stream's animation chunk stores, which fills the chunk exactly; `None`
    /// when it has none.
    pub fn animation(&self) -> Option<Result<Rsc8>> {
        let payload = &self.chunk(AwcChunkKind::Animation)?.payload;
        Some(Rsc8::open(payload).and_then(|rsc| {
            let len = Rsc8::HEADER_LEN + rsc.virtual_size() + rsc.physical_size();
            if len != payload.len() {
                return Err(IoError::Invalid {
                    what: "an animation chunk".into(),
                    why: format!(
                        "its RSC8 states {len} bytes and the chunk holds {}",
                        payload.len()
                    ),
                });
            }
            Ok(rsc)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::AwcChunk;
    use super::*;

    fn stream(kind: AwcChunkKind, payload: Vec<u8>) -> AwcStream {
        AwcStream {
            id: 1,
            chunks: vec![AwcChunk {
                kind,
                offset: 0,
                payload,
            }],
        }
    }

    #[test]
    fn a_loudness_chunk_reads_as_its_records_and_a_partial_record_is_refused() {
        let records = [(0u32, -20.5f32), (4800, -18.0)];
        let payload: Vec<u8> = records
            .iter()
            .flat_map(|(p, v)| p.to_le_bytes().into_iter().chain(v.to_le_bytes()))
            .collect();
        let read = stream(AwcChunkKind::Loudness, payload.clone())
            .loudness()
            .expect("a chunk")
            .expect("whole records");
        let read: Vec<_> = read.iter().map(|r| (r.position, r.value)).collect();
        assert_eq!(read, records);
        let cut = stream(AwcChunkKind::Loudness, payload[..12].to_vec());
        assert!(cut.loudness().expect("a chunk").is_err());
        assert!(stream(AwcChunkKind::Data, payload).loudness().is_none());
    }

    #[test]
    fn an_animation_chunk_opens_as_the_rsc8_that_fills_it() {
        const VIRTUAL: u32 = 0x40;
        let mut payload = Vec::new();
        for word in [
            u32::from_le_bytes(*b"RSC8"),
            0x1F << 8 | 0x41,
            VIRTUAL | 4,
            1,
        ] {
            payload.extend(word.to_le_bytes());
        }
        payload.resize(Rsc8::HEADER_LEN + VIRTUAL as usize, 0);
        let rsc = stream(AwcChunkKind::Animation, payload.clone())
            .animation()
            .expect("a chunk")
            .expect("opens");
        assert_eq!(
            (rsc.resource_id(), rsc.virtual_size()),
            (0x41, VIRTUAL as usize)
        );
        payload.extend([0; 16]);
        let padded = stream(AwcChunkKind::Animation, payload);
        assert!(padded.animation().expect("a chunk").is_err());
    }
}
