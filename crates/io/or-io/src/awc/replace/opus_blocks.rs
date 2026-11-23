//! A multi-channel Opus source's blocks rebuilt: each channel packed once, every block cut at one
//! frame for all channels, and the next block opening on each channel's cut packet again, whole.

use super::{refuse, Refused, LONGEST_FRAME, OPUS_SPARE, PACKET_ROOM};
use crate::awc::opus::{is_measured_toc, OpusStream, OPUS_FRAME};
use crate::awc::{AwcChunkKind, AwcFile, Codec, MULTI_CHANNEL_PACKET};

/// Bytes of a block's header per channel: `{0xFFFFFFFF, packets, repeat, samples, 0, 0}`.
const HEADER: usize = 24;

impl AwcFile {
    /// This container with multi-channel Opus source `stream` storing `channels`' packets in order.
    pub fn with_opus_channels_replaced(
        &self,
        stream: usize,
        channels: &[OpusStream],
    ) -> Result<AwcFile, Refused> {
        // One group per channel record, whose other words are carried. ⚠ A shipped cut packet
        // that holds more frames than its cut, or none, is written clean: it reads back the same.
        let Some(s) = self.streams.get(stream) else {
            return refuse("no such stream".to_string());
        };
        let Some(chunk) = s.chunk(AwcChunkKind::StreamFormat) else {
            return refuse(format!(
                "stream {stream} has no stream format to lay its blocks out by"
            ));
        };
        let record = &chunk.payload;
        let word = |at: usize| {
            let b = record.get(at..at + 4)?;
            Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        let (Some(block_size), Some(stated)) = (word(4), word(8)) else {
            return refuse("this stream's format states no block size".to_string());
        };
        let n = channels.len();
        if n == 0 || stated as usize != n || record.len() != 12 + 16 * n {
            return refuse(format!(
                "{n} channel(s) of packets were given for a stream format of {} bytes that \
                 states {stated}",
                record.len()
            ));
        }
        let block_size = block_size as usize;
        if block_size == 0 || !block_size.is_multiple_of(MULTI_CHANNEL_PACKET) {
            return refuse(format!(
                "this stream's blocks are {block_size} bytes, which is not a whole number of \
                 2,048-byte packets"
            ));
        }
        let k = record[12 + 13];
        for (c, new) in channels.iter().enumerate() {
            let r = &record[12 + 16 * c..28 + 16 * c];
            let id = u32::from_le_bytes([r[0], r[1], r[2], r[3]]);
            if Codec::from_byte(r[12]) != Codec::Opus || r[13] != k || !(1..=2).contains(&k) {
                return refuse(format!(
                    "channel {c} is codec {} of {} channel(s): a source is rebuilt only when its \
                     channels are all Opus and all mono or all stereo",
                    r[12], r[13]
                ));
            }
            if (new.id, new.channels) != (id, k) {
                return refuse(format!(
                    "the packets given for channel {c} are {:08X}'s of {} channel(s), and its \
                     record states {id:08X} of {k}",
                    new.id, new.channels
                ));
            }
            let fits = |p: &&Vec<u8>| p.len() <= LONGEST_FRAME && is_measured_toc(p[0], k);
            if let Some(p) = new.packets.iter().find(|p| p.is_empty() || !fits(p)) {
                return refuse(format!(
                    "an Opus packet of {} bytes is not one 20 ms frame of {k} channel(s) that a \
                     2,048-byte packet holds",
                    p.len()
                ));
            }
        }
        let frames = channels[0].packets.len();
        if let Some(other) = channels.iter().find(|c| c.packets.len() != frames) {
            return refuse(format!(
                "channel {:08X} has {} Opus frames and channel {:08X} {frames}: every channel of \
                 a source holds as many",
                other.id,
                other.packets.len(),
                channels[0].id
            ));
        }
        let held = frames as u64 * u64::from(OPUS_FRAME);
        let spare = |samples: u32| held.checked_sub(u64::from(samples));
        let short = |c: &&OpusStream| !spare(c.samples).is_some_and(|s| OPUS_SPARE.contains(&s));
        if let Some(c) = channels.iter().find(short) {
            return refuse(format!(
                "{frames} Opus frames hold {held} samples per channel, which is not {} to {} \
                 more than the {} channel {:08X} states, as in every shipped stream",
                OPUS_SPARE.start, OPUS_SPARE.end, c.samples, c.id
            ));
        }
        let step = u64::from(OPUS_FRAME) * u64::from(k);
        let whole: Option<Vec<u32>> = (channels.iter())
            .map(|c| c.samples.checked_mul(u32::from(k)))
            .collect();
        let (Some(whole), true) = (whole, step * frames as u64 <= u64::from(u32::MAX)) else {
            return refuse(format!(
                "{frames} Opus frames do not fit the block headers' and the seek table's words"
            ));
        };

        let packed: Vec<Vec<usize>> = channels.iter().map(|c| pack(&c.packets)).collect();
        let (data, seek) = blocks(channels, &packed, frames, block_size, step)?;
        let mut format = record.clone();
        format[0..4].copy_from_slice(&(seek.len() as u32).to_le_bytes());
        for (c, samples) in whole.iter().enumerate() {
            let at = 12 + 16 * c + 4;
            format[at..at + 4].copy_from_slice(&samples.to_le_bytes());
        }

        let mut edited = self.clone();
        let mut set = |kind, payload| {
            edited
                .set_chunk_payload(stream, kind, payload)
                .map_err(|e| Refused(e.to_string()))
        };
        set(AwcChunkKind::StreamFormat, format)?;
        set(
            AwcChunkKind::SeekTable,
            seek.iter().flat_map(|s| s.to_le_bytes()).collect(),
        )?;
        set(AwcChunkKind::Data, data)?;
        Ok(edited)
    }
}

/// The frame each 2,048-byte packet of `frames` starts on, packed as the game's encoder packs.
fn pack(frames: &[Vec<u8>]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut used = PACKET_ROOM;
    for (i, frame) in frames.iter().enumerate() {
        if used + 2 + frame.len() > PACKET_ROOM {
            starts.push(i);
            used = 0;
        }
        used += 2 + frame.len();
    }
    starts
}

/// The packet of `starts` that holds frame `f`.
fn holding(starts: &[usize], f: usize) -> usize {
    starts.partition_point(|s| *s <= f).saturating_sub(1)
}

/// Every block's bytes, and the sample each block is cut at in every channel.
///
/// ⚠ **A block is cut at the latest frame at which every channel's packets, their table and one
/// spare packet fit**, the last packet of each channel cut to the frames before it; the next block
/// starts each channel on that packet again, and its repeat counts the samples before the cut.
fn blocks(
    channels: &[OpusStream],
    packed: &[Vec<usize>],
    frames: usize,
    block_size: usize,
    step: u64,
) -> Result<(Vec<u8>, Vec<u32>), Refused> {
    let n = channels.len();
    let room = block_size / MULTI_CHANNEL_PACKET;
    let fits = |p: usize| p + (HEADER * n + 4 * p).div_ceil(MULTI_CHANNEL_PACKET) < room;
    // `frames` × `step` fits a `u32`, so every sample written does.
    let sample = |f: usize| (step * f as u64) as u32;
    let (mut data, mut seek) = (Vec::new(), Vec::new());
    let mut t = 0;
    while t < frames {
        let first: Vec<usize> = packed.iter().map(|s| holding(s, t)).collect();
        let mut last = first.clone();
        let mut p = n;
        if !fits(p) {
            return refuse(format!(
                "this stream's {room}-packet blocks are too small to hold a frame of each of its \
                 {n} channels and their table"
            ));
        }
        let mut end = t + 1;
        while end < frames {
            let opens = |s: &Vec<usize>, l: usize| s.get(l + 1) == Some(&end);
            let grows = (packed.iter().zip(&last))
                .filter(|(s, l)| opens(s, **l))
                .count();
            if !fits(p + grows) {
                break;
            }
            for (s, l) in packed.iter().zip(last.iter_mut()) {
                if opens(s, *l) {
                    *l += 1;
                }
            }
            p += grows;
            end += 1;
        }

        seek.push(sample(t));
        let at = data.len();
        let runs: Vec<&[usize]> = (packed.iter().zip(first.iter().zip(&last)))
            .map(|(s, (g0, gl))| &s[*g0..=*gl])
            .collect();
        for run in &runs {
            let ps0 = run[0];
            let samples = if end == frames {
                frames - ps0 - 1
            } else {
                end - ps0
            };
            let (count, repeat, held) = (run.len() as u32, sample(t - ps0), sample(samples));
            for w in [u32::MAX, count, repeat, held, 0, 0] {
                data.extend(w.to_le_bytes());
            }
        }
        for f in runs.iter().flat_map(|run| run.iter()) {
            data.extend(sample(*f).to_le_bytes());
        }
        let table = data.len() - at;
        data.resize(at + table.next_multiple_of(MULTI_CHANNEL_PACKET), 0);
        for (run, channel) in runs.iter().zip(channels) {
            for (j, from) in run.iter().enumerate() {
                let to = run.get(j + 1).copied().unwrap_or(end);
                let unit = data.len();
                for frame in &channel.packets[*from..to] {
                    data.extend((frame.len() as u16).to_le_bytes());
                    data.extend_from_slice(frame);
                }
                data.resize(unit + MULTI_CHANNEL_PACKET, 0);
            }
        }
        if end < frames {
            data.resize(seek.len() * block_size, 0);
        }
        t = end;
    }
    Ok((data, seek))
}

#[cfg(test)]
mod tests {
    use super::super::super::opus::tests::{packet, two_channels, unwritten, BLOCK};
    use super::*;
    use crate::awc::AwcKey;

    fn parse(bytes: &[u8]) -> AwcFile {
        AwcFile::parse(bytes, AwcKey::default()).expect("the container parses")
    }

    fn words(bytes: &[u8], at: usize, n: usize) -> Vec<u32> {
        (0..n)
            .map(|i| {
                let w = &bytes[at + 4 * i..at + 4 * i + 4];
                u32::from_le_bytes([w[0], w[1], w[2], w[3]])
            })
            .collect()
    }

    #[test]
    fn a_source_rebuilt_cuts_every_block_at_one_frame_and_repeats_each_cut_packet() {
        let (bytes, _) = two_channels();
        let awc = parse(&bytes);
        let chunk = |kind| &awc.streams[0].chunk(kind).expect("written").payload;
        let frame = OPUS_FRAME;
        assert_eq!(
            words(chunk(AwcChunkKind::SeekTable), 0, 3),
            [0, 9 * frame, 18 * frame]
        );
        let format = chunk(AwcChunkKind::StreamFormat);
        assert_eq!(words(format, 0, 3), [3, BLOCK as u32, 2]);
        for c in 0..2 {
            let record = &format[12 + 16 * c..28 + 16 * c];
            assert_eq!(words(record, 4, 1), [24 * frame - 2000]);
            assert_eq!(record[8..16], [0xF9, 0xFF, 0x80, 0xBB, 11, 1, 0xFF, 0xFF]);
        }
        let data = chunk(AwcChunkKind::Data);
        assert_eq!(data.len(), 2 * BLOCK + 5 * MULTI_CHANNEL_PACKET);
        // Per block and channel: packets, repeat and samples.
        let want = [
            [(3, 0, 9), (3, 0, 9)],
            [(3, 0, 9), (3, 1, 10)],
            [(2, 0, 5), (2, 2, 7)],
        ];
        for (b, channels) in want.iter().enumerate() {
            for (c, (packets, repeat, samples)) in channels.iter().enumerate() {
                let head = words(data, b * BLOCK + HEADER * c, 6);
                let stated = [u32::MAX, *packets, repeat * frame, samples * frame, 0, 0];
                assert_eq!(head, stated, "block {b}, channel {c}");
            }
        }
        let read = awc.opus_streams(0).expect("split");
        let again = awc.with_opus_channels_replaced(0, &read).expect("rebuilt");
        assert_eq!(again.to_bytes(AwcKey::default()).expect("saved"), bytes);
    }

    #[test]
    fn a_frame_packs_while_the_packet_holds_2046_bytes() {
        // Three frames of 510 bytes and their lengths fill 1,536; a fourth would fill 2,048.
        let frames: Vec<_> = (0..7).map(|i| packet(0x78, 510, i)).collect();
        assert_eq!(pack(&frames), [0, 3, 6]);
        assert_eq!(holding(&[0, 3, 6], 5), 1);
    }

    #[test]
    fn a_source_its_blocks_cannot_hold_as_given_is_refused() {
        let (bytes, _) = two_channels();
        let awc = parse(&bytes);
        let good = awc.opus_streams(0).expect("split");
        let why = |file: &AwcFile, channels: &[OpusStream]| {
            let refused = file.with_opus_channels_replaced(0, channels);
            format!("{}", refused.expect_err("refused"))
        };
        let mut shorter = good.clone();
        shorter[1].packets.pop();
        let mut renamed = good.clone();
        renamed[1].id = 0x0C03;
        let mut long = good.clone();
        long[0].packets[4] = packet(0x78, LONGEST_FRAME + 1, 4);
        let mut stated = good.clone();
        stated[0].samples += 1000;
        for (channels, want) in [
            (&good[..1], "1 channel(s) of packets"),
            (&shorter[..], "every channel of a source holds as many"),
            (&renamed[..], "are 00000C03's of 1 channel(s)"),
            (&long[..], "2045 bytes"),
            (&stated[..], "which is not 1920 to 2880 more"),
        ] {
            let why = why(&awc, channels);
            assert!(why.contains(want), "{want}: {why}");
        }
        let tiny = unwritten(&[0x0C01, 0x0C02], MULTI_CHANNEL_PACKET);
        let why = why(&tiny, &good[..]);
        assert!(why.contains("too small to hold a frame"), "{why}");
    }
}
