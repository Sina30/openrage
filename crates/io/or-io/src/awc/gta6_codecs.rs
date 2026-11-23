//! GTA VI's format records of mono PCM16, XMA and codec-2 streams: the 32-byte record Opus streams
//! have, its codec byte 0, 5 or 2, and its data in the clear.

use super::opus::AwcOpusFormat;
use super::{refuse, AwcChunkKind, AwcStream, Codec, MULTI_CHANNEL_PACKET};
use crate::error::{IoError, Result};

/// Samples of one XMA frame; a record's word 0x00 counts them before any trim.
const XMA_FRAME_SAMPLES: u64 = 512;

/// Floats of one codec-2 block: bins 0 to 1,023 of a 2,048-point real FFT, re and im interleaved.
pub(super) const SPECTRA_BLOCK_FLOATS: usize = 2048;

/// Samples from one codec-2 block's window to the next; a window spans two.
pub(super) const SPECTRA_HOP: u64 = 1024;

/// The first word of a codec-2 stream's last block.
const SPECTRA_TRAILER: u32 = 0xFFFF_ACE1;

/// The fewest blocks a codec-2 stream stores.
const SPECTRA_MIN_BLOCKS: u64 = 20;

impl Codec {
    /// GTA VI's codec 2: a mono signal stored as overlapping 2,048-point spectra.
    pub const SPECTRA: Codec = Codec::Unknown(2);
}

/// The blocks a codec-2 stream of `samples` stores: signal, zero blocks and the trailer block.
pub(super) fn spectra_blocks(samples: u64) -> u64 {
    (samples.div_ceil(SPECTRA_HOP) + 3).max(SPECTRA_MIN_BLOCKS)
}

/// The block a codec-2 stream stores between its signal and its trailer: zeros, with the imaginary
/// parts of bins 512 to 1,023 negative.
pub(super) fn spectra_zero_block() -> Vec<u8> {
    let mut block = vec![0u8; 4 * SPECTRA_BLOCK_FLOATS];
    for float in (SPECTRA_BLOCK_FLOATS / 2 + 1..SPECTRA_BLOCK_FLOATS).step_by(2) {
        block[4 * float + 3] = 0x80;
    }
    block
}

impl AwcStream {
    /// The record of a PCM16, XMA or codec-2 stream, checked against its data; `None` for another.
    // Its `stated_rate` is the stream's rate and its `samples_per_channel` the samples heard.
    pub fn coded_format(&self) -> Result<Option<AwcOpusFormat>> {
        match self.format_record()? {
            Some(format)
                if matches!(format.codec, Codec::Pcm16 | Codec::Xma)
                    || format.codec == Codec::SPECTRA =>
            {
                checked(self, format).map(Some)
            }
            _ => Ok(None),
        }
    }
}

/// `format` when `stream`'s data holds what a PCM16, XMA or codec-2 record states, else a refusal
/// by codec.
pub(super) fn checked(stream: &AwcStream, format: AwcOpusFormat) -> Result<AwcOpusFormat> {
    let id = stream.id;
    let spectra = format.codec == Codec::SPECTRA;
    let codec = if spectra {
        "codec 2".to_string()
    } else {
        format.codec.label()
    };
    if format.channels != 1 {
        return Err(refuse(format!(
            "stream {id:08X}'s {codec} record states {} channel(s), and only a mono {codec} \
             stream is read",
            format.channels
        )));
    }
    let data = (stream.chunk(AwcChunkKind::Data))
        .map(|c| c.payload.as_slice())
        .ok_or_else(|| refuse(format!("stream {id:08X} has no data chunk")))?;
    let (stated, heard) = (
        u64::from(format.samples),
        u64::from(format.samples_per_channel),
    );
    if spectra {
        return spectra_checked(id, data, stated, heard).map(|()| format);
    }
    if format.codec == Codec::Pcm16 {
        if stated != heard || data.len() as u64 != 2 * heard {
            return Err(refuse(format!(
                "stream {id:08X}'s PCM16 record states {stated} and {heard} samples, and its \
                 data of {} bytes is not two bytes for each of one count",
                data.len()
            )));
        }
        return Ok(format);
    }
    if data.is_empty() || !data.len().is_multiple_of(MULTI_CHANNEL_PACKET) {
        return Err(refuse(format!(
            "stream {id:08X}'s XMA data of {} bytes is not whole 2,048-byte packets",
            data.len()
        )));
    }
    // A packet's first six bits count the frames it holds; no frame spans two packets.
    let frames: u64 = (data.chunks(MULTI_CHANNEL_PACKET))
        .map(|packet| u64::from(packet[0] >> 2))
        .sum();
    if stated != XMA_FRAME_SAMPLES * frames || heard > stated {
        return Err(refuse(format!(
            "stream {id:08X}'s XMA record states {stated} samples before trimming and {heard} \
             heard, and its packets hold {frames} frames of 512"
        )));
    }
    Ok(format)
}

/// `Ok` when codec-2 `data` holds the blocks a record of `stated` floats and `heard` samples
/// states: the signal's blocks, the zero block up to the last, and the trailer.
fn spectra_checked(id: u32, data: &[u8], stated: u64, heard: u64) -> Result<()> {
    let floats = SPECTRA_BLOCK_FLOATS as u64;
    let (signal, blocks) = (heard.div_ceil(SPECTRA_HOP), spectra_blocks(heard));
    if stated != floats * blocks || data.len() as u64 != 4 * stated {
        return Err(refuse(format!(
            "stream {id:08X}'s codec 2 record states {stated} floats for {heard} samples, which \
             take {blocks} blocks of 2,048, and its data holds {} bytes",
            data.len()
        )));
    }
    let (blocks, _) = data.as_chunks::<{ 4 * SPECTRA_BLOCK_FLOATS }>();
    let blocks: Vec<&[u8]> = blocks.iter().map(|b| &b[..]).collect();
    // Float 1 of a signal block, bin 0's imaginary part, is zero of either sign.
    let bin_0_im = |b: &[u8]| u32::from_le_bytes([b[4], b[5], b[6], b[7]]) & 0x7FFF_FFFF;
    if let Some(b) = (0..=signal as usize).find(|b| bin_0_im(blocks[*b]) != 0) {
        return Err(refuse(format!(
            "stream {id:08X}'s codec 2 block {b} states an imaginary part for bin 0"
        )));
    }
    let zero = spectra_zero_block();
    let last = blocks.len() - 1;
    if let Some(b) = (signal as usize + 1..last).find(|b| blocks[*b] != zero.as_slice()) {
        return Err(refuse(format!(
            "stream {id:08X}'s codec 2 block {b} is not the zero block every codec 2 stream \
             stores after its signal"
        )));
    }
    if blocks[last][0..4] != SPECTRA_TRAILER.to_le_bytes() {
        return Err(refuse(format!(
            "stream {id:08X}'s last codec 2 block does not open with {SPECTRA_TRAILER:#010X}"
        )));
    }
    Ok(())
}

/// Why a GTA VI format record of `codec`, neither Opus, PCM16, XMA nor codec 2, is not read.
pub(super) fn unplayable(id: u32, codec: Codec) -> IoError {
    refuse(format!(
        "stream {id:08X}'s format record states codec {}, not Opus (11), PCM16 (0), XMA (5) or \
         codec 2",
        codec.as_byte()
    ))
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::opus::tests::{container, Chunks};
    use super::super::{AwcFile, AwcKey};
    use super::*;

    fn record(codec: u8, stated: u32, heard: u32, rate: u16, channels: u8) -> Vec<u8> {
        let mut r = vec![0u8; 32];
        r[0x00..0x04].copy_from_slice(&stated.to_le_bytes());
        r[0x04..0x08].copy_from_slice(&(-1i32).to_le_bytes());
        r[0x08..0x0A].copy_from_slice(&rate.to_le_bytes());
        r[0x0A..0x0C].copy_from_slice(&(-7i16).to_le_bytes());
        r[0x0C..0x10].copy_from_slice(&heard.to_le_bytes());
        r[0x16..0x1A].fill(0xFF);
        r[0x1A..0x1C].copy_from_slice(&900u16.to_le_bytes());
        (r[0x1C], r[0x1D]) = (codec, channels);
        r
    }

    /// Whole XMA packets whose headers count `frames` frames each, and nothing else.
    fn xma_packets(frames: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; frames.len() * MULTI_CHANNEL_PACKET];
        for (p, f) in frames.iter().enumerate() {
            data[p * MULTI_CHANNEL_PACKET] = f << 2;
        }
        data
    }

    fn pcm_stream(samples: u32) -> Chunks {
        let data = (0..samples).flat_map(|i| (i as i16 * 37 - 500).to_le_bytes());
        vec![
            (0x55, data.collect()),
            (0x76, record(0, samples, samples, 44_100, 1)),
        ]
    }

    fn xma_stream(frames: &[u8], heard: u32) -> Chunks {
        let stated = 512 * frames.iter().map(|f| u32::from(*f)).sum::<u32>();
        vec![
            (0x55, xma_packets(frames)),
            (0x76, record(5, stated, heard, 32_007, 1)),
            (0xA3, vec![0; 4 * frames.len()]),
        ]
    }

    fn parse(streams: &[(u32, Chunks)]) -> (Vec<u8>, AwcFile) {
        let extra = vec![0; streams.len() + 1];
        let bytes = container(0xFF21, &extra, streams);
        let awc = AwcFile::parse(&bytes, AwcKey::default()).expect("the container parses");
        (bytes, awc)
    }

    fn refusal(chunks: Chunks) -> String {
        let (_, awc) = parse(&[(0x0E01, chunks)]);
        let coded = awc.streams[0].coded_format().expect_err("refused");
        let opus = awc.streams[0].opus_format().expect_err("refused there too");
        assert_eq!(coded.to_string(), opus.to_string());
        coded.to_string()
    }

    #[test]
    fn a_pcm16_and_an_xma_record_read_with_their_rate_and_heard_samples_and_save_unchanged() {
        let (bytes, awc) = parse(&[
            (0x0E01, pcm_stream(300)),
            (0x0E02, xma_stream(&[15, 7], 9_000)),
        ]);
        let pcm = awc.streams[0].coded_format().expect("read").expect("PCM16");
        assert_eq!(
            (
                pcm.codec,
                pcm.channels,
                pcm.stated_rate,
                pcm.samples_per_channel
            ),
            (Codec::Pcm16, 1, 44_100, 300)
        );
        let xma = awc.streams[1].coded_format().expect("read").expect("XMA");
        assert_eq!(
            (
                xma.codec,
                xma.samples,
                xma.samples_per_channel,
                xma.stated_rate
            ),
            (Codec::Xma, 512 * 22, 9_000, 32_007)
        );
        assert_eq!((xma.word_0a, xma.peak), (-7, 900));
        for (at, s) in awc.streams.iter().enumerate() {
            assert_eq!(s.opus_format().expect("not refused"), None);
            assert_eq!(awc.opus_streams(at).expect("not refused"), vec![]);
            let data = awc
                .stream_data(s, AwcKey([1, 2, 3, 4]))
                .expect("in the clear");
            assert_eq!(data, s.chunk(AwcChunkKind::Data).expect("data").payload);
        }
        assert_eq!(awc.to_bytes(AwcKey::default()).expect("written"), bytes);
    }

    #[test]
    fn a_record_its_data_contradicts_is_refused_by_codec() {
        let mut short = pcm_stream(300);
        short[0].1.pop();
        let mut counts = pcm_stream(300);
        counts[1].1[0..4].copy_from_slice(&301u32.to_le_bytes());
        let mut stereo = xma_stream(&[4], 100);
        stereo[1].1[0x1D] = 2;
        let mut cut = xma_stream(&[4], 100);
        cut[0].1.pop();
        let mut frames = xma_stream(&[4, 4], 100);
        frames[0].1[MULTI_CHANNEL_PACKET] = 5 << 2;
        let mut nothing = pcm_stream(4);
        nothing.remove(0);
        let cases = [
            (short, "data of 599 bytes"),
            (counts, "states 301 and 300 samples"),
            (stereo, "only a mono XMA stream"),
            (cut, "not whole 2,048-byte packets"),
            (frames, "hold 9 frames of 512"),
            (xma_stream(&[1], 513), "513 heard"),
            (nothing, "has no data chunk"),
        ];
        for (chunks, want) in cases {
            let why = refusal(chunks);
            assert!(why.contains(want), "{want}: {why}");
        }
    }

    #[test]
    fn a_codec_no_reader_reads_is_refused_by_name() {
        let record = record(4, 2048, 400, 48_000, 1);
        let (_, awc) = parse(&[(0x0E03, vec![(0x55, vec![0; 8192]), (0x76, record)])]);
        let coded = awc.streams[0].coded_format();
        assert_eq!(coded.expect("not this reader's"), None);
        let why = awc.streams[0].opus_format().expect_err("refused");
        let want = "codec 4, not Opus (11), PCM16 (0), XMA (5) or codec 2";
        assert!(why.to_string().contains(want), "{why}");
        assert!(awc.opus_streams(0).is_err());
    }

    /// A codec-2 stream of `heard` samples: made-up signal blocks, the zero blocks and a trailer.
    pub(crate) fn spectra_stream(heard: u32) -> Chunks {
        let blocks = spectra_blocks(heard.into()) as usize;
        let signal = (u64::from(heard).div_ceil(SPECTRA_HOP) + 1) as usize;
        let mut data = Vec::new();
        for b in 0..signal {
            let floats = (0..SPECTRA_BLOCK_FLOATS).map(|f| {
                if f == 1 {
                    0.0
                } else {
                    (b * 7 + f) as f32 / 4096.0 - 0.25
                }
            });
            data.extend(floats.flat_map(f32::to_le_bytes));
        }
        for _ in signal..blocks - 1 {
            data.extend(spectra_zero_block());
        }
        let mut trailer = vec![0u8; 4 * SPECTRA_BLOCK_FLOATS];
        trailer[0..4].copy_from_slice(&SPECTRA_TRAILER.to_le_bytes());
        trailer[4..8].copy_from_slice(&[16, 0, 19, 0]);
        data.extend(trailer);
        let stated = (SPECTRA_BLOCK_FLOATS * blocks) as u32;
        vec![(0x55, data), (0x76, record(2, stated, heard, 48_000, 1))]
    }

    #[test]
    fn a_codec_2_record_reads_with_its_rate_and_samples_and_saves_unchanged() {
        assert_eq!(spectra_blocks(17 * SPECTRA_HOP), SPECTRA_MIN_BLOCKS);
        assert_eq!(spectra_blocks(17 * SPECTRA_HOP + 1), SPECTRA_MIN_BLOCKS + 1);
        let zero = spectra_zero_block();
        // Bins 512 to 1,023 each hold one -0.0, bin 512's real part +0.0.
        assert_eq!(zero.iter().filter(|b| **b != 0).count(), 512);
        assert_eq!(zero[4096..4104], [0, 0, 0, 0, 0, 0, 0, 0x80]);
        for heard in [400, 30_000] {
            let (bytes, awc) = parse(&[(0x0E04, spectra_stream(heard))]);
            let s = &awc.streams[0];
            let format = s.coded_format().expect("read").expect("codec 2");
            let blocks = spectra_blocks(heard.into());
            assert_eq!(
                (format.codec, format.samples_per_channel, format.stated_rate),
                (Codec::SPECTRA, heard, 48_000)
            );
            assert_eq!(u64::from(format.samples), 2048 * blocks);
            assert_eq!(s.opus_format().expect("not refused"), None);
            assert_eq!(awc.opus_streams(0).expect("not refused"), vec![]);
            let data = awc
                .stream_data(s, AwcKey([1, 2, 3, 4]))
                .expect("in the clear");
            assert_eq!(data, s.chunk(AwcChunkKind::Data).expect("data").payload);
            assert_eq!(awc.to_bytes(AwcKey::default()).expect("written"), bytes);
        }
    }

    #[test]
    fn a_codec_2_stream_its_blocks_contradict_is_refused_by_name() {
        let block = 4 * SPECTRA_BLOCK_FLOATS;
        let mut count = spectra_stream(400);
        count[1].1[0x0C..0x10].copy_from_slice(&(17 * 1024 + 1u32).to_le_bytes());
        let mut im = spectra_stream(400);
        im[0].1[block + 4] = 1;
        // Block 2 is the first zero block; its last byte is float 2,047's sign.
        let mut plain = spectra_stream(400);
        plain[0].1[3 * block - 1] = 0;
        let mut trailer = spectra_stream(400);
        let end = trailer[0].1.len();
        trailer[0].1[end - block] ^= 1;
        let mut stereo = spectra_stream(400);
        stereo[1].1[0x1D] = 2;
        let cases = [
            (
                count,
                "40960 floats for 17409 samples, which take 21 blocks",
            ),
            (im, "block 1 states an imaginary part for bin 0"),
            (plain, "block 2 is not the zero block"),
            (trailer, "does not open with 0xFFFFACE1"),
            (stereo, "only a mono codec 2 stream"),
        ];
        for (chunks, want) in cases {
            let why = refusal(chunks);
            assert!(why.contains(want), "{want}: {why}");
        }
    }
}
