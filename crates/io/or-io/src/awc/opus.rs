//! GTA VI's Opus streams: the 32-byte `OpusFormat` record, data in 2,048-byte packets of
//! `u16`-length Opus packets, and GTA V's multi-channel blocks holding runs of them. Each split is
//! checked against the sample positions the container states beside it before it is returned.

use super::{refuse, AwcChunkKind, AwcFile, AwcStream, Codec, MULTI_CHANNEL_PACKET};
use crate::error::Result;

/// Samples of one 20 ms Opus frame at 48 kHz, the only frame GTA VI's streams hold.
pub const OPUS_FRAME: u32 = 960;

/// The bytes of an `OpusFormat` chunk, the only size measured.
const FORMAT_BYTES: usize = 32;

/// A stream's `OpusFormat` chunk; samples are in 48 kHz units, the decode's rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AwcOpusFormat {
    /// Samples of every channel together.
    pub samples: u32,
    /// −1, or a sample of each channel: a loop point, a role inferred from its values.
    pub loop_point: i32,
    /// A rate the record states that is not the decode's: 32,000 only on streams with SILK frames.
    pub stated_rate: u16,
    /// Carried as stated; what it means is not known.
    pub word_0a: i16,
    pub samples_per_channel: u32,
    /// Carried as stated: `0xFFFF`, or the loop point's low half.
    pub word_16: u16,
    /// Carried as stated: `0xFFFF` or the last sample's low half, and which is not known.
    pub word_18: u16,
    /// The peak envelope's first window, which the `peak` chunk continues.
    pub peak: u16,
    pub codec: Codec,
    pub channels: u8,
}

/// One stream's Opus packets, or one channel's of a multi-channel container, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusStream {
    /// The stream's name hash, or the channel's.
    pub id: u32,
    pub channels: u8,
    /// Samples of each channel the container states.
    pub samples: u32,
    /// Each a TOC byte and one 20 ms frame.
    pub packets: Vec<Vec<u8>>,
}

impl AwcStream {
    /// The `OpusFormat` chunk of an Opus stream, refused unless it holds the record measured.
    pub fn opus_format(&self) -> Result<Option<AwcOpusFormat>> {
        let Some(format) = self.format_record()? else {
            return Ok(None);
        };
        match format.codec {
            Codec::Opus => {}
            // `AwcStream::coded_format` reads these; one its data contradicts is refused here too.
            codec if matches!(codec, Codec::Pcm16 | Codec::Xma) || codec == Codec::SPECTRA => {
                return super::gta6_codecs::checked(self, format).map(|_| None);
            }
            other => return Err(super::gta6_codecs::unplayable(self.id, other)),
        }
        let whole = u64::from(format.samples_per_channel) * u64::from(format.channels);
        if !(1..=2).contains(&format.channels) || whole != u64::from(format.samples) {
            return Err(refuse(format!(
                "stream {:08X}'s format record states {} channel(s), {} samples and {} per \
                 channel, which is not one or two channels of whole samples",
                self.id, format.channels, format.samples, format.samples_per_channel
            )));
        }
        Ok(Some(format))
    }

    /// The 32-byte format record as it is stated, whatever codec it names.
    pub(super) fn format_record(&self) -> Result<Option<AwcOpusFormat>> {
        let Some(chunk) = self.chunk(AwcChunkKind::OpusFormat) else {
            return Ok(None);
        };
        let Ok(b) = <&[u8; FORMAT_BYTES]>::try_from(chunk.payload.as_slice()) else {
            return Err(refuse(format!(
                "a GTA VI format record is {FORMAT_BYTES} bytes, and stream {:08X}'s is {}",
                self.id,
                chunk.payload.len()
            )));
        };
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let format = AwcOpusFormat {
            samples: u32_at(0x00),
            loop_point: u32_at(0x04) as i32,
            stated_rate: u16_at(0x08),
            word_0a: u16_at(0x0A) as i16,
            samples_per_channel: u32_at(0x0C),
            word_16: u16_at(0x16),
            word_18: u16_at(0x18),
            peak: u16_at(0x1A),
            codec: Codec::from_byte(b[0x1C]),
            channels: b[0x1D],
        };
        Ok(Some(format))
    }
}

/// Whether a stream is stored as GTA VI's Opus, whose data no flag bit scrambles.
pub(super) fn is_opus(stream: &AwcStream) -> bool {
    stream.chunk(AwcChunkKind::OpusFormat).is_some()
        || stream
            .stream_format()
            .is_some_and(|f| f.channels.iter().any(|c| c.codec == Codec::Opus))
}

impl AwcFile {
    /// The Opus packets of stream `stream`: one group for a single stream, one per channel of a
    /// multi-channel source; `Ok` is empty for a stream that holds no Opus.
    pub fn opus_streams(&self, stream: usize) -> Result<Vec<OpusStream>> {
        let s = self
            .streams
            .get(stream)
            .ok_or_else(|| refuse(format!("there is no stream {stream}")))?;
        if let Some(format) = s.opus_format()? {
            return Ok(vec![single(s, format)?]);
        }
        if s.stream_format().is_some() && is_opus(s) {
            return channels(s);
        }
        Ok(Vec::new())
    }
}

fn payload(stream: &AwcStream, kind: AwcChunkKind) -> Result<&[u8]> {
    stream
        .chunk(kind)
        .map(|c| c.payload.as_slice())
        .ok_or_else(|| {
            refuse(format!(
                "stream {:08X} has no {} chunk",
                stream.id,
                kind.label()
            ))
        })
}

fn words(bytes: &[u8], what: &str) -> Result<Vec<u32>> {
    let (words, rest) = bytes.as_chunks::<4>();
    if !rest.is_empty() {
        return Err(refuse(format!(
            "a {what} of {} bytes is not whole words",
            bytes.len()
        )));
    }
    Ok(words.iter().map(|w| u32::from_le_bytes(*w)).collect())
}

/// The Opus packets of a single stream's data chunk, framed and checked as a stream's are.
pub fn split_packets(data: &[u8], channels: u8) -> Result<Vec<Vec<u8>>> {
    if !data.len().is_multiple_of(MULTI_CHANNEL_PACKET) {
        return Err(refuse(format!(
            "a data chunk of {} bytes is not whole 2,048-byte packets",
            data.len()
        )));
    }
    let mut out = Vec::new();
    for unit in data.chunks(MULTI_CHANNEL_PACKET) {
        let packets = packets_in(unit, channels)?;
        out.extend(packets.into_iter().map(<[u8]>::to_vec));
    }
    Ok(out)
}

/// Whether a TOC byte is a 20 ms configuration of one frame, stereo exactly when `channels` is 2.
pub(super) fn is_measured_toc(toc: u8, channels: u8) -> bool {
    let config = toc >> 3;
    let twenty_ms = match config {
        0..=11 => config % 4 == 1,
        12..=15 => config % 2 == 1,
        _ => config % 4 == 3,
    };
    twenty_ms && toc & 3 == 0 && (toc & 4 != 0) == (channels == 2)
}

/// The Opus packets of one 2,048-byte packet: `u16` lengths and their bytes, to a zero length or
/// the end.
fn packets_in(unit: &[u8], channels: u8) -> Result<Vec<&[u8]>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 2 <= unit.len() {
        let n = usize::from(u16::from_le_bytes([unit[at], unit[at + 1]]));
        if n == 0 {
            break;
        }
        let packet = unit.get(at + 2..at + 2 + n).ok_or_else(|| {
            refuse(format!(
                "an Opus packet of {n} bytes at {at:#x} runs past its 2,048-byte packet"
            ))
        })?;
        if !is_measured_toc(packet[0], channels) {
            return Err(refuse(format!(
                "an Opus packet opens with TOC byte {:#04X}, not one 20 ms frame of {}",
                packet[0],
                if channels == 2 { "stereo" } else { "mono" }
            )));
        }
        out.push(packet);
        at += 2 + n;
    }
    Ok(out)
}

fn single(stream: &AwcStream, format: AwcOpusFormat) -> Result<OpusStream> {
    let data = payload(stream, AwcChunkKind::Data)?;
    if !data.len().is_multiple_of(MULTI_CHANNEL_PACKET) {
        return Err(refuse(format!(
            "stream {:08X}'s data of {} bytes is not whole 2,048-byte packets",
            stream.id,
            data.len()
        )));
    }
    let units = data
        .chunks(MULTI_CHANNEL_PACKET)
        .map(|unit| packets_in(unit, format.channels))
        .collect::<Result<Vec<_>>>()?;
    let frames: usize = units.iter().map(Vec::len).sum();
    if frames == 0 {
        return Err(refuse(format!(
            "stream {:08X}'s data chunk holds no Opus packet",
            stream.id
        )));
    }
    let seek = words(payload(stream, AwcChunkKind::SeekTable)?, "seek table")?;
    if seek.len() != units.len() + 1 {
        return Err(refuse(format!(
            "stream {:08X}'s seek table has {} entries for {} packets, not one more",
            stream.id,
            seek.len(),
            units.len()
        )));
    }
    let step = u64::from(OPUS_FRAME) * u64::from(format.channels);
    let mut before = 0u64;
    for (i, unit) in units.iter().enumerate() {
        if u64::from(seek[i]) != step * before {
            return Err(refuse(format!(
                "stream {:08X}'s seek table puts packet {i} at sample {}, and the packets \
                 before it hold {}",
                stream.id,
                seek[i],
                step * before
            )));
        }
        before += unit.len() as u64;
    }
    let last = step * (before - 1);
    if u64::from(seek[units.len()]) != last {
        return Err(refuse(format!(
            "stream {:08X}'s seek table ends at {}, not at the last frame's sample {last}",
            stream.id,
            seek[units.len()]
        )));
    }
    Ok(OpusStream {
        id: stream.id,
        channels: format.channels,
        samples: format.samples_per_channel,
        packets: units.into_iter().flatten().map(<[u8]>::to_vec).collect(),
    })
}

/// `len` bytes at `at` of `bytes`, or a refusal naming `what`.
fn span<'a>(bytes: &'a [u8], at: usize, len: Option<usize>, what: &str) -> Result<&'a [u8]> {
    len.and_then(|len| bytes.get(at..at.checked_add(len)?))
        .ok_or_else(|| refuse(format!("{what} runs past its block")))
}

/// Each channel's packets of a multi-channel source, its blocks laid out as GTA V's with the
/// packets of a channel stated in its own header, each block's run kept from the sample the runs
/// before it reached.
///
/// ⚠ Not by dropping each block's `repeat` samples: a block's cut packet can hold frames past the
/// next block's start, or none, and the repeat still states that start.
fn channels(source: &AwcStream) -> Result<Vec<OpusStream>> {
    let format = payload(source, AwcChunkKind::StreamFormat)?;
    let head = words(
        span(format, 0, Some(12), "the stream format")?,
        "stream format",
    )?;
    let (block_count, block_size, n) = (head[0] as usize, head[1] as usize, head[2] as usize);
    if Some(format.len()) != n.checked_mul(16).map(|b| b + 12) {
        return Err(refuse(format!(
            "a stream format of {} bytes does not hold the {n} channel records it states",
            format.len()
        )));
    }
    let mut out = Vec::with_capacity(n);
    for c in 0..n {
        let r = &format[12 + c * 16..28 + c * 16];
        let channels = r[13];
        if Codec::from_byte(r[12]) != Codec::Opus || !(1..=2).contains(&channels) {
            return Err(refuse(format!(
                "channel {c} is codec {} with {channels} channel(s), not Opus of one or two",
                r[12]
            )));
        }
        out.push(OpusStream {
            id: u32::from_le_bytes([r[0], r[1], r[2], r[3]]),
            channels,
            samples: u32::from_le_bytes([r[4], r[5], r[6], r[7]]) / u32::from(channels),
            packets: Vec::new(),
        });
    }
    let k = out.first().map_or(1, |c| c.channels);
    if let Some(other) = out.iter().find(|c| c.channels != k) {
        return Err(refuse(format!(
            "channel {:08X} has {} channel(s) and channel {:08X} {k}: a source's channels are all \
             mono or all stereo",
            other.id, other.channels, out[0].id
        )));
    }
    let step = u64::from(OPUS_FRAME) * u64::from(k);
    if block_size == 0 || !block_size.is_multiple_of(MULTI_CHANNEL_PACKET) {
        return Err(refuse(format!(
            "a block of {block_size} bytes is not whole 2,048-byte packets"
        )));
    }
    let data = payload(source, AwcChunkKind::Data)?;
    let seek = words(payload(source, AwcChunkKind::SeekTable)?, "seek table")?;
    if data.len().div_ceil(block_size) != block_count || seek.len() != block_count {
        return Err(refuse(format!(
            "{block_count} block(s) of {block_size} bytes do not match {} bytes of data and a \
             seek table of {} entries",
            data.len(),
            seek.len()
        )));
    }
    // The sample each channel's runs have reached; frames before it in a run are held twice.
    let mut reached = vec![0u64; n];
    for (b, block) in data.chunks(block_size).enumerate() {
        let heads = words(
            span(block, 0, n.checked_mul(24), "the channel headers")?,
            "header",
        )?;
        let mut at = n * 24;
        let mut firsts = Vec::with_capacity(n);
        for (c, h) in heads.chunks(6).enumerate() {
            if h[0] != u32::MAX || h[4] != 0 || h[5] != 0 {
                return Err(refuse(format!(
                    "block {b}'s header of channel {c} is not {{0xFFFFFFFF, packets, repeat, \
                     samples, 0, 0}}"
                )));
            }
            let count = h[1] as usize;
            let what = "a table of first samples";
            firsts.push(words(span(block, at, count.checked_mul(4), what)?, what)?);
            at += count * 4;
        }
        at = at.next_multiple_of(MULTI_CHANNEL_PACKET);
        for (c, (h, first)) in heads.chunks(6).zip(&firsts).enumerate() {
            let channel = &mut out[c];
            let (count, repeat, samples) = (first.len(), u64::from(h[2]), u64::from(h[3]));
            let what = "a channel's run of packets";
            let run = span(block, at, count.checked_mul(MULTI_CHANNEL_PACKET), what)?;
            at += run.len();
            let mut packets = Vec::new();
            for (i, unit) in run.chunks(MULTI_CHANNEL_PACKET).enumerate() {
                let held = packets_in(unit, channel.channels)?;
                let next = u64::from(first[i]) + step * held.len() as u64;
                if first.get(i + 1).is_some_and(|f| u64::from(*f) != next) {
                    return Err(refuse(format!(
                        "block {b}, channel {c}: packet {} starts at sample {}, and packet {i} \
                         ends at {next}",
                        i + 1,
                        first[i + 1]
                    )));
                }
                packets.extend(held);
            }
            if let Some(start) = first.first() {
                if u64::from(seek[b]) != u64::from(*start) + repeat {
                    return Err(refuse(format!(
                        "block {b}, channel {c}: the seek table states {}, not the first \
                         sample {start} and the repeat {repeat}",
                        seek[b]
                    )));
                }
                let next = seek.get(b + 1).map(|s| u64::from(*s));
                if next.is_some_and(|next| u64::from(*start) + samples != next) {
                    return Err(refuse(format!(
                        "block {b}, channel {c}: {samples} samples from the first sample \
                         {start} do not reach the next block's seek entry {}",
                        seek[b + 1]
                    )));
                }
            }
            if !repeat.is_multiple_of(step) {
                return Err(refuse(format!(
                    "block {b}, channel {c}: a repeat of {repeat} samples is not whole frames"
                )));
            }
            let start = first.first().map_or(reached[c], |s| u64::from(*s));
            let held = reached[c].checked_sub(start);
            let frames = held.filter(|held| held.is_multiple_of(step));
            let skip = frames.map(|held| held / step);
            let Some(skip) = skip.filter(|skip| *skip <= packets.len() as u64) else {
                return Err(refuse(format!(
                    "block {b}, channel {c}: its run starts at sample {start}, which is not a \
                     frame of the {} samples its runs before it reached",
                    reached[c]
                )));
            };
            let skip = skip as usize;
            let had = channel.packets.len();
            let twice = had.checked_sub(skip).map(|from| &channel.packets[from..]);
            if !twice.is_some_and(|held| held.iter().zip(&packets).all(|(was, is)| was == is)) {
                return Err(refuse(format!(
                    "block {b}, channel {c}: the {skip} frame(s) it holds again differ from the \
                     block before"
                )));
            }
            reached[c] = start + step * packets.len() as u64;
            let kept = packets[skip..].iter().map(|p| p.to_vec());
            channel.packets.extend(kept);
        }
    }
    if let Some(empty) = out.iter().find(|c| c.packets.is_empty()) {
        return Err(refuse(format!(
            "channel {:08X} holds no Opus packet",
            empty.id
        )));
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{AwcKey, GTA6_TABLES};
    use super::*;

    /// An Opus packet: `toc` and `len - 1` bytes after it, none of them zero.
    pub(crate) fn packet(toc: u8, len: usize, seed: usize) -> Vec<u8> {
        let mut p = vec![toc];
        p.extend((1..len).map(|i| ((i * 7 + seed * 13) % 251 + 1) as u8));
        p
    }

    /// `packets` in 2,048-byte packets, each a `u16` length and its bytes, then zero fill.
    pub(crate) fn units(packets: &[Vec<u8>]) -> (Vec<u8>, Vec<usize>) {
        let (mut data, mut counts) = (vec![0u8; MULTI_CHANNEL_PACKET], vec![0usize]);
        let mut at = 0;
        for p in packets {
            if at + 2 + p.len() > MULTI_CHANNEL_PACKET {
                data.resize(data.len() + MULTI_CHANNEL_PACKET, 0);
                counts.push(0);
                at = 0;
            }
            let base = data.len() - MULTI_CHANNEL_PACKET + at;
            data[base..base + 2].copy_from_slice(&(p.len() as u16).to_le_bytes());
            data[base + 2..base + 2 + p.len()].copy_from_slice(p);
            at += 2 + p.len();
            *counts.last_mut().expect("one packet") += 1;
        }
        (data, counts)
    }

    /// The seek table of a single stream: each packet's first sample, then the last frame's.
    pub(crate) fn seek_table(counts: &[usize], channels: u8) -> Vec<u8> {
        let step = OPUS_FRAME * u32::from(channels);
        let mut before = 0u32;
        let mut out = Vec::new();
        for n in counts {
            out.extend((step * before).to_le_bytes());
            before += *n as u32;
        }
        out.extend((step * (before - 1)).to_le_bytes());
        out
    }

    pub(crate) fn format_record(per_channel: u32, channels: u8, loop_point: i32) -> Vec<u8> {
        let mut r = vec![0u8; FORMAT_BYTES];
        r[0x00..0x04].copy_from_slice(&(per_channel * u32::from(channels)).to_le_bytes());
        r[0x04..0x08].copy_from_slice(&loop_point.to_le_bytes());
        r[0x08..0x0A].copy_from_slice(&32_000u16.to_le_bytes());
        r[0x0A..0x0C].copy_from_slice(&(-41i16).to_le_bytes());
        r[0x0C..0x10].copy_from_slice(&per_channel.to_le_bytes());
        r[0x16..0x18].copy_from_slice(&0xFFFFu16.to_le_bytes());
        r[0x18..0x1A].copy_from_slice(&((per_channel - 1) as u16).to_le_bytes());
        r[0x1A..0x1C].copy_from_slice(&4321u16.to_le_bytes());
        r[0x1C] = 11;
        r[0x1D] = channels;
        r
    }

    /// A stream's chunks: each a kind byte and a payload.
    pub(crate) type Chunks = Vec<(u8, Vec<u8>)>;

    /// A container in GTA V's layout, or in GTA VI's when `flags` holds [`GTA6_TABLES`], with
    /// `extra` as its extra words; payloads follow the tables on 16-byte steps.
    pub(crate) fn container(flags: u16, extra: &[u32], streams: &[(u32, Chunks)]) -> Vec<u8> {
        let sc = streams.len();
        let chunks: usize = streams.iter().map(|(_, c)| c.len()).sum();
        let gta6 = flags & GTA6_TABLES == GTA6_TABLES;
        let mut out = b"ADAT".to_vec();
        out.extend(1u16.to_le_bytes());
        out.extend(flags.to_le_bytes());
        out.extend((sc as u32).to_le_bytes());
        out.extend([0; 4]);
        if gta6 && flags & 0x40 != 0 {
            out.extend(0x1230u32.to_le_bytes());
        }
        let mut first = 0u16;
        if flags & 1 != 0 {
            for (_, c) in streams {
                out.extend(first.to_le_bytes());
                first += c.len() as u16;
            }
        }
        if gta6 {
            assert_eq!(extra.len(), sc + 1);
            extra.iter().for_each(|w| out.extend(w.to_le_bytes()));
        }
        for (id, c) in streams {
            out.extend(((c.len() as u32) << 29 | id).to_le_bytes());
        }
        let table = out.len();
        out.resize(table + chunks * 8, 0);
        let data_offset = out.len() as u32;
        out[12..16].copy_from_slice(&data_offset.to_le_bytes());
        let mut word = table;
        for (kind, bytes) in streams.iter().flat_map(|(_, c)| c) {
            out.resize(out.len().next_multiple_of(16), 0);
            let raw = u64::from(*kind) << 56 | (bytes.len() as u64) << 28 | out.len() as u64;
            out[word..word + 8].copy_from_slice(&raw.to_le_bytes());
            word += 8;
            out.extend_from_slice(bytes);
        }
        out
    }

    /// The chunks of a single Opus stream of `packets`.
    pub(crate) fn single_stream(packets: &[Vec<u8>], channels: u8) -> Vec<(u8, Vec<u8>)> {
        let (data, counts) = units(packets);
        let per_channel = OPUS_FRAME * packets.len() as u32 - 2400;
        vec![
            (0x36, vec![1, 0, 2, 0]),
            (0x55, data),
            (0x76, format_record(per_channel, channels, -1)),
            (0xA3, seek_table(&counts, channels)),
        ]
    }

    pub(crate) fn mono(count: usize, seed: usize) -> Vec<Vec<u8>> {
        let tocs = [0xF8, 0x08, 0x48, 0x78, 0xB8];
        (0..count)
            .map(|i| packet(tocs[i % tocs.len()], 3 + (i * 37 + seed) % 300, i + seed))
            .collect()
    }

    fn parse(bytes: &[u8]) -> AwcFile {
        AwcFile::parse(bytes, AwcKey::default()).expect("the container parses")
    }

    fn refusal(bytes: &[u8], stream: usize) -> String {
        let awc = parse(bytes);
        format!("{}", awc.opus_streams(stream).expect_err("refused"))
    }

    #[test]
    fn a_single_stream_reads_back_the_packets_written_into_its_packets() {
        let packets = mono(40, 1);
        let bytes = container(0xFF02, &[], &[(0x0ABC, single_stream(&packets, 1))]);
        let awc = parse(&bytes);
        assert_eq!(awc.unaccounted_bytes(), 0);
        let format = awc.streams[0]
            .opus_format()
            .expect("read")
            .expect("present");
        assert_eq!(
            (
                format.codec,
                format.channels,
                format.loop_point,
                format.peak
            ),
            (Codec::Opus, 1, -1, 4321)
        );
        assert_eq!(format.samples_per_channel, 960 * 40 - 2400);
        assert_eq!((format.stated_rate, format.word_0a), (32_000, -41));
        let read = awc.opus_streams(0).expect("split");
        assert_eq!(read.len(), 1);
        assert_eq!((read[0].id, read[0].channels), (0x0ABC, 1));
        assert_eq!(read[0].packets, packets);
        let kinds: Vec<_> = awc.streams[0].chunks.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&AwcChunkKind::OpusFormat));
        assert_eq!(AwcChunkKind::from_byte(0xB2), AwcChunkKind::Loudness);
    }

    #[test]
    fn a_data_chunk_splits_into_the_packets_written_into_it() {
        let packets = mono(90, 5);
        let (data, counts) = units(&packets);
        assert!(counts.len() > 1, "{counts:?}");
        assert_eq!(split_packets(&data, 1).expect("split"), packets);
        let why = |data: &[u8], channels| {
            format!("{}", split_packets(data, channels).expect_err("refused"))
        };
        assert!(why(&data[..data.len() - 1], 1).contains("not whole 2,048-byte packets"));
        assert!(why(&data, 2).contains("not one 20 ms frame of stereo"));
        let mut cut = data.clone();
        cut[0..2].copy_from_slice(&2047u16.to_le_bytes());
        assert!(why(&cut, 1).contains("runs past its 2,048-byte packet"));
    }

    /// ⚠ Flag bit 1 is set on every GTA VI single-stream container, and its data is plain.
    #[test]
    fn flag_bit_1_does_not_descramble_an_opus_stream() {
        let packets = mono(12, 2);
        let bytes = container(0xFF02, &[], &[(7, single_stream(&packets, 1))]);
        let awc = parse(&bytes);
        assert!(awc.single_channel_encrypted());
        let key = AwcKey([1, 2, 3, 4]);
        let data = awc.stream_data(&awc.streams[0], key).expect("plain");
        assert_eq!(data, units(&packets).0);
        assert!(awc
            .stream_data(&awc.streams[0], AwcKey::default())
            .is_some());
    }

    #[test]
    fn a_gta6_table_reads_its_streams_and_writes_back_the_words_it_does_not_name() {
        let stereo: Vec<_> = (0..30).map(|i| packet(0xFC, 120 + i, i)).collect();
        let streams = [
            (0x11, single_stream(&mono(25, 3), 1)),
            (0x22, vec![]),
            (0x33, single_stream(&stereo, 2)),
        ];
        for flags in [0xFF21, 0xFF61] {
            let bytes = container(flags, &[0x0BAD_F00D, 5, 6, 7], &streams);
            let awc = parse(&bytes);
            assert_eq!(awc.unaccounted_bytes(), 0, "{flags:#X}");
            assert_eq!(awc.chunk_indices, vec![0, 4, 4]);
            let extra = awc.extra.as_ref().expect("carried");
            assert_eq!(extra.words, vec![0x0BAD_F00D, 5, 6, 7]);
            assert_eq!(extra.boundary, (flags & 0x40 != 0).then_some(0x1230));
            assert_eq!(awc.opus_streams(0).expect("mono")[0].packets, mono(25, 3));
            assert_eq!(awc.opus_streams(1).expect("nothing"), vec![]);
            let read = awc.opus_streams(2).expect("stereo");
            assert_eq!((read[0].channels, &read[0].packets), (2, &stereo));
            assert_eq!(awc.to_bytes(AwcKey::default()).expect("written"), bytes);
            let mut grown = awc.clone();
            grown.streams.push(grown.streams[1].clone());
            grown.chunk_indices.push(4);
            let mut regrouped = awc.clone();
            let moved = regrouped.streams[0].chunks.pop().expect("a chunk");
            regrouped.streams[1].chunks.push(moved);
            for changed in [grown, regrouped] {
                let why = changed.to_bytes(AwcKey::default()).expect_err("refused");
                assert!(format!("{why}").contains("only with the streams"), "{why}");
            }
        }
    }

    /// GTA V's own chunk-index layout is still read when bit 5 is clear.
    #[test]
    fn flag_bit_0_without_bit_5_keeps_the_gta5_layout() {
        let bytes = container(0xFF01, &[], &[(0x44, vec![(0x55, vec![9; 32])])]);
        let awc = parse(&bytes);
        assert!(awc.extra.is_none());
        assert_eq!(awc.streams[0].chunks[0].payload, vec![9; 32]);
        assert_eq!(awc.to_bytes(AwcKey::default()).expect("written"), bytes);
    }

    #[test]
    fn a_gta6_table_that_breaks_a_stated_relation_is_refused() {
        let streams = [
            (0x11, single_stream(&mono(4, 3), 1)),
            (0x22, vec![(0xBD, vec![1; 8])]),
        ];
        let good = container(0xFF21, &[1, 2, 3], &streams);
        parse(&good);
        let refused = |bytes: &[u8]| {
            format!(
                "{}",
                AwcFile::parse(bytes, AwcKey::default()).expect_err("refused")
            )
        };
        // The second stream's index is 4, the first stream's chunk count.
        let mut index = good.clone();
        index[18..20].copy_from_slice(&3u16.to_le_bytes());
        assert!(
            refused(&index).contains("first-chunk index is 3"),
            "{}",
            refused(&index)
        );
        let mut offset = good.clone();
        let stated = u32::from_le_bytes(good[12..16].try_into().expect("four"));
        offset[12..16].copy_from_slice(&(stated + 16).to_le_bytes());
        assert!(refused(&offset).contains("not at the declared data offset"));
        let mut count = good.clone();
        count[8..12].copy_from_slice(&0x3FFFu32.to_le_bytes());
        assert!(AwcFile::parse(&count, AwcKey::default()).is_err());
        assert!(refused(&good[..24]).contains("run past the end"));
    }

    #[test]
    fn a_format_record_off_the_measured_one_is_refused() {
        let mut streams = single_stream(&mono(4, 1), 1);
        streams[2].1.push(0);
        let bytes = container(0xFF02, &[], &[(1, streams)]);
        assert!(
            refusal(&bytes, 0).contains("is 32 bytes"),
            "{}",
            refusal(&bytes, 0)
        );
        for (at, value, want) in [
            (0x1D, 3u8, "not one or two"),
            (0x00, 0x55, "not one or two"),
            (0x1C, 7, "codec 7, not Opus"),
        ] {
            let mut streams = single_stream(&mono(4, 1), 1);
            streams[2].1[at] = value;
            let bytes = container(0xFF02, &[], &[(1, streams)]);
            let why = refusal(&bytes, 0);
            assert!(why.contains(want), "{at:#X}: {why}");
        }
    }

    #[test]
    fn a_stream_whose_packets_break_the_framing_is_refused() {
        type Break = fn(&mut Chunks);
        let cases: [(&str, Break); 9] = [
            ("not whole 2,048-byte packets", |s| s[1].1.push(0)),
            ("runs past its 2,048-byte packet", |s| {
                s[1].1[0..2].copy_from_slice(&2047u16.to_le_bytes())
            }),
            ("TOC byte 0xF0", |s| s[1].1[2] = 0xF0),
            ("TOC byte 0xF9", |s| s[1].1[2] = 0xF9),
            ("TOC byte 0xFC", |s| s[1].1[2] = 0xFC),
            ("holds no Opus packet", |s| s[1].1.fill(0)),
            ("entries for", |s| s[3].1.extend([0; 4])),
            ("puts packet 1 at sample", |s| s[3].1[4] ^= 1),
            ("not at the last frame", |s| {
                let end = s[3].1.len();
                s[3].1[end - 4] ^= 1
            }),
        ];
        for (want, break_it) in cases {
            let mut streams = single_stream(&mono(300, 4), 1);
            break_it(&mut streams);
            let bytes = container(0xFF02, &[], &[(1, streams)]);
            let why = refusal(&bytes, 0);
            assert!(why.contains(want), "{want}: {why}");
        }
        let mut streams = single_stream(&mono(4, 4), 1);
        streams.remove(3);
        let bytes = container(0xFF02, &[], &[(1, streams)]);
        assert!(refusal(&bytes, 0).contains("no seektable chunk"));
    }

    /// One block of a multi-channel source: per channel its `repeat` and its packets.
    fn block(channels: &[(u8, u32, &[Vec<u8>])], seek: u32, size: usize) -> Vec<u8> {
        let runs: Vec<_> = channels.iter().map(|(_, _, p)| units(p)).collect();
        let mut out = Vec::new();
        for ((k, repeat, _), (data, _)) in channels.iter().zip(&runs) {
            let count = (data.len() / MULTI_CHANNEL_PACKET) as u32;
            let samples = OPUS_FRAME * u32::from(*k) * 99;
            for w in [u32::MAX, count, *repeat, samples, 0, 0] {
                out.extend(w.to_le_bytes());
            }
        }
        for ((k, repeat, _), (_, counts)) in channels.iter().zip(&runs) {
            let mut first = seek - repeat;
            for n in counts {
                out.extend(first.to_le_bytes());
                first += OPUS_FRAME * u32::from(*k) * *n as u32;
            }
        }
        out.resize(out.len().next_multiple_of(MULTI_CHANNEL_PACKET), 0);
        runs.iter().for_each(|(data, _)| out.extend(data));
        assert!(out.len() <= size);
        out
    }

    /// The block size the two-channel source is written in: a table packet, six packets of runs
    /// and the spare one.
    pub(crate) const BLOCK: usize = 8 * MULTI_CHANNEL_PACKET;

    /// A multi-channel source of mono channels `ids` whose blocks are not written yet: a stream
    /// format, one data packet and one seek entry.
    pub(crate) fn unwritten(ids: &[u32], block_size: usize) -> AwcFile {
        let mut format = Vec::new();
        for w in [1, block_size as u32, ids.len() as u32] {
            format.extend(w.to_le_bytes());
        }
        for id in ids {
            format.extend(id.to_le_bytes());
            format.extend(960u32.to_le_bytes());
            format.extend([0xF9, 0xFF, 0x80, 0xBB, 11, 1, 0xFF, 0xFF]);
        }
        let source = vec![
            (0x48, format),
            (0x55, vec![0; MULTI_CHANNEL_PACKET]),
            (0xA3, vec![0; 4]),
        ];
        let mut streams = vec![(0, source)];
        streams.extend(ids.iter().map(|id| (*id, vec![(0xBD, vec![3; 8])])));
        parse(&container(0xFF25, &vec![0; ids.len() + 2], &streams))
    }

    /// Two mono channels of 24 frames, the first's three and the second's four to a 2,048-byte
    /// packet, written in blocks of [`BLOCK`]: every block is cut at frame 9, then 18.
    pub(crate) fn two_channels() -> (Vec<u8>, [Vec<Vec<u8>>; 2]) {
        let a: Vec<_> = (0..24).map(|i| packet(0x78, 600, i)).collect();
        let b: Vec<_> = (0..24).map(|i| packet(0x48, 450, 50 + i)).collect();
        let channels = [(0x0C01, &a), (0x0C02, &b)].map(|(id, packets)| OpusStream {
            id,
            channels: 1,
            samples: OPUS_FRAME * 24 - 2000,
            packets: packets.clone(),
        });
        let file = unwritten(&[0x0C01, 0x0C02], BLOCK);
        let written = file
            .with_opus_channels_replaced(0, &channels)
            .expect("written");
        let bytes = written.to_bytes(AwcKey::default()).expect("saved");
        (bytes, [a, b])
    }

    /// Where packet `i` of channel `c`'s run in block `b` of `data` lies, by its headers' counts.
    pub(crate) fn packet_at(data: &[u8], n: usize, b: usize, c: usize, i: usize) -> usize {
        let block = b * BLOCK;
        let count = |c: usize| {
            let at = block + 24 * c + 4;
            u32::from_le_bytes(data[at..at + 4].try_into().expect("four")) as usize
        };
        let counted: usize = (0..n).map(count).sum();
        let table = (24 * n + 4 * counted).next_multiple_of(MULTI_CHANNEL_PACKET);
        block + table + ((0..c).map(count).sum::<usize>() + i) * MULTI_CHANNEL_PACKET
    }

    fn data_at(bytes: &[u8], kind: AwcChunkKind) -> usize {
        let awc = parse(bytes);
        awc.streams[0].chunk(kind).expect("written").offset as usize
    }

    #[test]
    fn a_multi_channel_container_reads_back_each_channels_frames_once() {
        let (bytes, [a, b]) = two_channels();
        let awc = parse(&bytes);
        let read = awc.opus_streams(0).expect("split");
        assert_eq!(read.len(), 2);
        let samples = OPUS_FRAME * 24 - 2000;
        for (c, id) in read.iter().zip([0x0C01, 0x0C02]) {
            assert_eq!((c.id, c.channels, c.samples), (id, 1, samples));
        }
        assert_eq!(read[0].packets, a);
        assert_eq!(read[1].packets, b);
        assert_eq!(awc.opus_streams(1).expect("no chunk"), vec![]);
    }

    /// ⚠ A cut packet kept past the next block's start, or written empty, still states that start:
    /// dropping each block's repeat would hold a frame twice or lose two.
    #[test]
    fn a_cut_packet_longer_or_emptier_than_its_cut_still_reads_each_frame_once() {
        let (good, [a, b]) = two_channels();
        let data = data_at(&good, AwcChunkKind::Data);
        let mut long = good.clone();
        // Block 0's last packet of channel 1 holds frame 8; frame 9 goes in after it.
        let at = data + packet_at(&good[data..], 2, 0, 1, 2) + 2 + b[8].len();
        long[at..at + 2].copy_from_slice(&(b[9].len() as u16).to_le_bytes());
        long[at + 2..at + 2 + b[9].len()].copy_from_slice(&b[9]);
        let mut empty = good.clone();
        let at = data + packet_at(&good[data..], 2, 1, 1, 2);
        empty[at..at + MULTI_CHANNEL_PACKET].fill(0);
        for bytes in [long, empty] {
            let read = parse(&bytes).opus_streams(0).expect("split");
            assert_eq!((&read[0].packets, &read[1].packets), (&a, &b));
        }
    }

    #[test]
    fn a_multi_channel_container_that_breaks_a_stated_relation_is_refused() {
        let (good, _) = two_channels();
        let (format, data, seek) = (
            data_at(&good, AwcChunkKind::StreamFormat),
            data_at(&good, AwcChunkKind::Data),
            data_at(&good, AwcChunkKind::SeekTable),
        );
        let run_packet = |b, c, i| data + packet_at(&good[data..], 2, b, c, i);
        let cases: [(&str, usize, u8); 10] = [
            ("not Opus of one or two", format + 12 + 12, 7),
            ("all mono or all stereo", format + 12 + 16 + 13, 3),
            ("not whole 2,048-byte packets", format + 4, 1),
            ("do not match", format, 3),
            ("is not {0xFFFFFFFF", data + 16, 1),
            ("starts at sample", data + 48 + 4, 1),
            ("the seek table states", seek, 1),
            ("do not reach the next block's seek entry", data + 12, 1),
            ("it holds again differ", run_packet(1, 1, 0) + 3, 1),
            ("runs past its block", data + BLOCK + 24 + 4, 0x40),
        ];
        for (want, at, xor) in cases {
            let mut bytes = good.clone();
            bytes[at] ^= xor;
            let why = refusal(&bytes, 0);
            assert!(why.contains(want), "{want}: {why}");
        }
        // Channel 0's frames 6 to 8 lost: the next block starts past what the runs reached.
        let mut gap = good.clone();
        let at = run_packet(0, 0, 2);
        gap[at..at + MULTI_CHANNEL_PACKET].fill(0);
        assert!(refusal(&gap, 0).contains("which is not a frame of the"));
    }

    /// A channel whose header counts no packet has nothing to export.
    #[test]
    fn a_multi_channel_channel_left_with_no_packet_is_refused() {
        let size = 2 * MULTI_CHANNEL_PACKET;
        let a: Vec<_> = (0..2).map(|i| packet(0x78, 100, i)).collect();
        let data = block(&[(1, 1920, &a)], 1920, size);
        let mut format = Vec::new();
        for w in [1u32, size as u32, 1, 0x0C01, 960] {
            format.extend(w.to_le_bytes());
        }
        format.extend([0, 0, 0x80, 0xBB, 11, 1, 0xFF, 0xFF]);
        let streams = [
            (
                0,
                vec![
                    (0x48, format),
                    (0x55, data),
                    (0xA3, 1920u32.to_le_bytes().to_vec()),
                ],
            ),
            (0x0C01, vec![]),
        ];
        let counted = container(0xFF25, &[0, 0, 0], &streams);
        assert_eq!(parse(&counted).opus_streams(0).expect("read")[0].packets, a);
        let mut uncounted = counted.clone();
        let header = data_at(&counted, AwcChunkKind::Data);
        uncounted[header + 4..header + 12].fill(0);
        let why = refusal(&uncounted, 0);
        assert!(why.contains("00000C01 holds no Opus packet"), "{why}");
    }
}
