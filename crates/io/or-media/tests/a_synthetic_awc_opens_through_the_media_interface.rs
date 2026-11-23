//! An `.awc` built by hand opens through the media interface: its info, its samples, a seek, a
//! codec with no decoder refused by name, and a codec added later as one row of the table.
//!
//! ⚠ Every container here is laid out from the format's documented chunks and blocks, so nothing
//! under test wrote the bytes it is checked against.

use or_io::awc::codec::decode_adpcm;
use or_io::awc::{Accumulator, AwcFile, AwcKey, AWC_MAGIC_LE};
use or_media::{awc, AudioInfo, Codec, Decoder, Decoders, MediaError, Pcm, Samples};

const DATA_KIND: u8 = 0x55;
const FORMAT_KIND: u8 = 0xFA;
const STREAMFORMAT_KIND: u8 = 0x48;

/// One single-channel stream as the builder lays it out: a 20-byte `format` and a `data`.
struct Built {
    id: u32,
    codec: u8,
    rate: u16,
    samples: u32,
    data: Vec<u8>,
}

fn ramp(samples: u32) -> Vec<i16> {
    (0..samples as i32)
        .map(|i| ((i * 137) % 20000 - 10000) as i16)
        .collect()
}

fn pcm_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

fn put(out: &mut [u8], at: usize, value: usize) {
    out[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
}

fn chunk(kind: u8, size: usize, offset: usize) -> [u8; 8] {
    ((offset as u64) | ((size as u64) << 28) | (u64::from(kind) << 56)).to_le_bytes()
}

/// Streams with a `format` and a `data` each, every chunk 16-aligned.
fn container(streams: &[Built], flags: u16) -> Vec<u8> {
    let tables = 16 + streams.len() * 4 + streams.len() * 2 * 8;
    let mut out = vec![0u8; tables];
    put(&mut out, 0, AWC_MAGIC_LE as usize);
    out[4..6].copy_from_slice(&1u16.to_le_bytes());
    out[6..8].copy_from_slice(&flags.to_le_bytes());
    put(&mut out, 8, streams.len());
    put(&mut out, 12, tables);
    let mut entries = Vec::new();
    for (i, s) in streams.iter().enumerate() {
        put(&mut out, 16 + i * 4, s.id as usize | (2 << 29));
        let mut format = vec![0u8; 20];
        format[0..4].copy_from_slice(&s.samples.to_le_bytes());
        format[4..8].copy_from_slice(&(-1i32).to_le_bytes());
        format[8..10].copy_from_slice(&s.rate.to_le_bytes());
        format[19] = s.codec;
        for (kind, payload) in [(FORMAT_KIND, format), (DATA_KIND, s.data.clone())] {
            while !out.len().is_multiple_of(16) {
                out.push(0xEE);
            }
            entries.push(chunk(kind, payload.len(), out.len()));
            out.extend_from_slice(&payload);
        }
    }
    let at = 16 + streams.len() * 4;
    for (i, entry) in entries.iter().enumerate() {
        out[at + i * 8..at + i * 8 + 8].copy_from_slice(entry);
    }
    out
}

fn parsed(bytes: &[u8]) -> AwcFile {
    AwcFile::parse(bytes, AwcKey::default()).expect("the container parses")
}

fn one(codec: u8, samples: u32, data: Vec<u8>) -> Vec<u8> {
    container(
        &[Built {
            id: 0x0123_4567,
            codec,
            rate: 32000,
            samples,
            data,
        }],
        0,
    )
}

#[test]
fn a_pcm16_stream_opens_with_its_info_and_samples() {
    let file = parsed(&one(0, 100, pcm_bytes(&ramp(100))));
    let listed = awc::streams(&file);
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].stream, listed[0].id), (0, 0x0123_4567));

    let source = awc::open(&file, AwcKey::default(), 0).expect("the stream opens");
    let info = source.info();
    assert_eq!(
        info.audio,
        Some(AudioInfo {
            codec: Codec::Pcm16,
            channels: 1,
            sample_rate: 32000,
            frames: 100,
        })
    );
    assert_eq!(info.audio, Some(listed[0].info));
    assert!(info.video.is_none());
    assert_eq!(
        info.duration().map(|d| d.as_secs_f64()),
        Some(100.0 / 32000.0)
    );

    let mut audio = source.audio(&Decoders::new()).expect("PCM16 decodes");
    assert_eq!(audio.read_all_i16().expect("the samples"), ramp(100));
    assert_eq!(audio.read().expect("the end"), None);
}

#[test]
fn a_stream_seeks_to_a_sample_frame() {
    let file = parsed(&one(0, 100, pcm_bytes(&ramp(100))));
    let source = awc::open(&file, AwcKey::default(), 0).expect("the stream opens");
    let mut audio = source.audio(&Decoders::new()).expect("PCM16 decodes");

    assert_eq!(audio.read_all_i16().expect("all"), ramp(100));
    audio.seek(40).expect("PCM16 seeks");
    assert_eq!(audio.read_all_i16().expect("the rest"), ramp(100)[40..]);
    audio.seek(1_000).expect("past the end is the end");
    assert_eq!(audio.read().expect("the end"), None);
}

#[test]
fn an_adpcm_stream_decodes_as_or_io_decodes_it() {
    let data = vec![0x10, 0, 0, 0, 0x12, 0x34, 0x56, 0x78];
    let want = decode_adpcm(&data, 9, Accumulator::Clamped);
    let file = parsed(&one(4, 9, data));
    let source = awc::open(&file, AwcKey::default(), 0).expect("the stream opens");
    assert_eq!(
        source.info().audio.map(|a| a.codec),
        Some(Codec::RageAdpcm),
        "the table calls it by the engine's name for it, not the container's byte"
    );
    let got = source
        .audio(&Decoders::new())
        .expect("ADPCM decodes")
        .read_all_i16()
        .expect("the samples");
    assert_eq!(pcm_bytes(&got), want);
}

#[test]
fn a_frame_that_does_not_decode_is_refused_naming_its_codec() {
    // One MPEG-1 Layer III frame header and four bytes: the frame is cut short.
    let file = parsed(&one(7, 1152, vec![0xFF, 0xFB, 0x88, 0xC4, 0, 0, 0, 0]));
    let source = awc::open(&file, AwcKey::default(), 0).expect("the stream opens");
    let Err(why) = source.audio(&Decoders::new()) else {
        panic!("a cut-short frame does not decode");
    };
    assert!(why.to_string().contains("MP3"), "{why}");
}

#[test]
fn a_codec_with_no_decoder_is_refused_by_name_and_its_info_still_reads() {
    let strange = parsed(&one(9, 10, vec![0; 20]));
    let source = awc::open(&strange, AwcKey::default(), 0).expect("its info reads");
    assert_eq!(
        source.info().audio.map(|a| a.codec),
        Some(Codec::Unknown(9))
    );
    let Err(why) = source.audio(&Decoders::new()) else {
        panic!("codec 9 has no decoder");
    };
    assert_eq!(why, MediaError::NoDecoder(Codec::Unknown(9)));
    assert!(why.to_string().contains("unknown(9)"), "{why}");
}

#[test]
fn a_codec_added_later_is_one_row() {
    let file = parsed(&one(9, 6, vec![0; 16]));
    let source = awc::open(&file, AwcKey::default(), 0).expect("the stream opens");

    let table = Decoders::new().with(
        Codec::Unknown(9),
        Decoder::audio(|info, data| {
            assert_eq!(
                data.len(),
                16,
                "the row is handed the stream's stored bytes"
            );
            let frames = info.frames as usize * usize::from(info.channels);
            Ok(Box::new(Pcm::new(
                Samples::I16(vec![7; frames]),
                info.channels,
            )))
        }),
    );
    let got = source
        .audio(&table)
        .expect("the added row decodes it")
        .read_all_i16()
        .expect("the samples");
    assert_eq!(got, vec![7; 6]);

    // The first row for a codec decodes it: a route added after one cannot replace it.
    let file = parsed(&one(0, 3, pcm_bytes(&[1, 2, 3])));
    let source = awc::open(&file, AwcKey::default(), 0).expect("the stream opens");
    let table = Decoders::new().with(
        Codec::Pcm16,
        Decoder::audio(|_, _| Err(MediaError::Decode("not this one".into()))),
    );
    let got = source
        .audio(&table)
        .expect("the built-in row wins")
        .read_all_i16()
        .expect("the samples");
    assert_eq!(got, vec![1, 2, 3]);
}

#[test]
fn a_scrambled_stream_without_its_key_says_the_key_is_what_it_lacks() {
    let bytes = container(
        &[Built {
            id: 1,
            codec: 0,
            rate: 32000,
            samples: 4,
            data: vec![0; 8],
        }],
        2,
    );
    let file = parsed(&bytes);
    let Err(why) = awc::open(&file, AwcKey::default(), 0) else {
        panic!("no key, no stream");
    };
    assert!(why.to_string().contains("key"), "{why}");
}

const PACKET: usize = 2048;
const CHANNELS: usize = 3;
const PACKETS_PER_BLOCK: usize = 2;
const PACKET_SAMPLES: usize = PACKET / 2;
/// Two whole blocks and part of a third.
const SAMPLES: usize = 2 * PACKETS_PER_BLOCK * PACKET_SAMPLES + 1500;

/// Channel `c`'s sample `i`: a rising ramp, a falling one and a steeper one.
fn signal(c: usize, i: usize) -> i16 {
    match c {
        0 => i as i16,
        1 => -(i as i16),
        _ => (i * 3 % 30_000) as i16,
    }
}

/// Three PCM16 channels in shared blocks, laid out from the block layout.
fn multi_channel() -> Vec<u8> {
    let table_bytes = CHANNELS * 24 + CHANNELS * PACKETS_PER_BLOCK * 4;
    let block_size = table_bytes.next_multiple_of(PACKET) + CHANNELS * PACKETS_PER_BLOCK * PACKET;
    let blocks = SAMPLES.div_ceil(PACKETS_PER_BLOCK * PACKET_SAMPLES);
    let streams = 1 + CHANNELS;
    let tables_end = 16 + streams * 4 + 2 * 8;
    let format_len = 12 + CHANNELS * 16;
    let data_at = (tables_end + format_len).next_multiple_of(PACKET);
    let mut out = vec![0u8; data_at + blocks * block_size];

    put(&mut out, 0, AWC_MAGIC_LE as usize);
    out[4..6].copy_from_slice(&1u16.to_le_bytes());
    out[6..8].copy_from_slice(&4u16.to_le_bytes());
    put(&mut out, 8, streams);
    put(&mut out, 12, tables_end);
    put(&mut out, 16, 2 << 29);
    for c in 0..CHANNELS {
        put(&mut out, 20 + c * 4, 0x0100_0000 + c);
    }
    let at = 16 + streams * 4;
    out[at..at + 8].copy_from_slice(&chunk(STREAMFORMAT_KIND, format_len, tables_end));
    out[at + 8..at + 16].copy_from_slice(&chunk(DATA_KIND, blocks * block_size, data_at));

    put(&mut out, tables_end, blocks);
    put(&mut out, tables_end + 4, block_size);
    put(&mut out, tables_end + 8, CHANNELS);
    for c in 0..CHANNELS {
        let entry = tables_end + 12 + c * 16;
        put(&mut out, entry, 0x0100_0000 + c);
        put(&mut out, entry + 4, SAMPLES);
        out[entry + 10..entry + 12].copy_from_slice(&32_000u16.to_le_bytes());
        out[entry + 12] = 0;
    }

    let per_block = PACKETS_PER_BLOCK * PACKET_SAMPLES;
    for b in 0..blocks {
        let block = data_at + b * block_size;
        let samples = (SAMPLES - b * per_block).min(per_block);
        let packets = samples.div_ceil(PACKET_SAMPLES);
        let mut offsets = block + CHANNELS * 24;
        let mut run = block + table_bytes.next_multiple_of(PACKET);
        for c in 0..CHANNELS {
            put(&mut out, block + c * 24 + 4, packets);
            put(&mut out, block + c * 24 + 12, samples);
            for p in 0..packets {
                put(&mut out, offsets, p * PACKET_SAMPLES);
                offsets += 4;
            }
            for i in 0..samples {
                let sample = signal(c, b * per_block + i).to_le_bytes();
                out[run + i * 2..run + i * 2 + 2].copy_from_slice(&sample);
            }
            run += packets * PACKET;
        }
    }
    out
}

#[test]
fn a_multi_channel_container_opens_as_one_interleaved_stream() {
    let file = parsed(&multi_channel());
    let listed = awc::streams(&file);
    assert_eq!(listed.len(), 1, "the source stream is the one that plays");
    let source = awc::open(&file, AwcKey::default(), listed[0].stream).expect("it opens");
    assert_eq!(
        source.info().audio,
        Some(AudioInfo {
            codec: Codec::Pcm16,
            channels: CHANNELS as u16,
            sample_rate: 32_000,
            frames: SAMPLES as u64,
        })
    );

    let mut audio = source.audio(&Decoders::new()).expect("it decodes");
    let got = audio.read_all_i16().expect("the samples");
    let want: Vec<i16> = (0..SAMPLES)
        .flat_map(|i| (0..CHANNELS).map(move |c| signal(c, i)))
        .collect();
    let first_wrong = got.iter().zip(&want).position(|(a, b)| a != b);
    assert!(
        got.len() == want.len() && first_wrong.is_none(),
        "{} samples against {}, first wrong at {first_wrong:?}",
        got.len(),
        want.len()
    );

    // A seek is a whole frame, one sample of every channel.
    audio.seek(1000).expect("a decoded buffer seeks");
    assert_eq!(
        audio.read_all_i16().expect("the rest"),
        want[1000 * CHANNELS..]
    );
}

/// Bits pushed most-significant first, then packed into bytes.
#[derive(Default)]
struct Bits(Vec<bool>);

impl Bits {
    fn put(&mut self, value: usize, n: usize) {
        self.0.extend((0..n).rev().map(|i| (value >> i) & 1 == 1));
    }

    fn bytes(&self) -> Vec<u8> {
        self.0
            .chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0u8, |v, (i, b)| v | (u8::from(*b) << (7 - i)))
            })
            .collect()
    }
}

/// One frame of XMA with a single 512-sample subframe that carries no coefficients: its 15-bit
/// length, the layout and subframe-length bits, the 8-bit gain, the five flag bits that are all
/// clear, a bit that is skipped and the flag that says another frame follows in the packet.
fn silent_frame(bits: &mut Bits, length: usize, more: bool) {
    bits.put(length, 15);
    bits.put(1, 1); // every channel splits alike
    bits.put(0, 1); // one subframe of the whole frame
    bits.put(0, 8); // dynamic range gain
    bits.put(0, 1); // no trims
    bits.put(0, 1); // no extended header
    bits.put(0, 1); // the reserved bit
    bits.put(0, 1); // the channel sends no coefficients
    bits.put(0, 1); // skipped
    bits.put(usize::from(more), 1);
}

/// 2,048-byte packets of silent frames, `frames` of them in all, `per_packet` to a packet.
fn silent_xma(frames: usize, per_packet: usize, length: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut left = frames;
    while left > 0 {
        let here = left.min(per_packet);
        left -= here;
        let mut bits = Bits::default();
        bits.put(here, 6);
        bits.put(0, 15);
        bits.put(1, 3);
        bits.put(0, 8);
        for i in 0..here {
            silent_frame(&mut bits, length, i + 1 < here);
        }
        bits.0.resize(2048 * 8, true);
        out.extend(bits.bytes());
    }
    out
}

#[test]
fn a_hand_built_xma_stream_of_silent_frames_decodes_to_the_samples_it_states() {
    // 13 and 7 frames of 512 samples in two packets, each frame 31 bits.
    let file = parsed(&one(5, 20 * 512, silent_xma(20, 13, 31)));
    let source = awc::open(&file, AwcKey::default(), 0).expect("its info reads");
    assert_eq!(source.info().audio.map(|a| a.codec), Some(Codec::Xma));
    let mut audio = source
        .audio(&Decoders::new())
        .expect("XMA is one row of the table");
    assert_eq!(
        audio.read_all_i16().expect("the samples"),
        vec![0; 20 * 512]
    );

    // The same frames claiming another length are refused, and the refusal names the codec.
    let file = parsed(&one(5, 20 * 512, silent_xma(20, 13, 32)));
    let source = awc::open(&file, AwcKey::default(), 0).expect("its info reads");
    let Err(why) = source.audio(&Decoders::new()) else {
        panic!("a frame that ends before its length says is broken");
    };
    assert!(why.to_string().contains("XMA"), "{why}");
}

#[test]
fn an_xma_stream_that_is_not_whole_packets_or_states_more_than_it_holds_is_refused_naming_xma() {
    let file = parsed(&one(5, 1024, vec![0x38, 0, 1, 0, 8, 0x2F, 0xFC, 3]));
    let source = awc::open(&file, AwcKey::default(), 0).expect("its info reads");
    let Err(why) = source.audio(&Decoders::new()) else {
        panic!("eight bytes are not a packet");
    };
    assert!(matches!(why, MediaError::Decode(_)), "{why:?}");
    assert!(why.to_string().contains("XMA"), "{why}");

    // Twenty frames hold 10,240 samples and the stream states one more.
    let file = parsed(&one(5, 20 * 512 + 1, silent_xma(20, 13, 31)));
    let source = awc::open(&file, AwcKey::default(), 0).expect("its info reads");
    let Err(why) = source.audio(&Decoders::new()) else {
        panic!("the packets do not hold the samples stated");
    };
    assert!(why.to_string().contains("XMA"), "{why}");
}
