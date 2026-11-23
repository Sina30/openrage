//! Replacing a multi-channel `.awc` stream from PCM16: the shared blocks are
//! rebuilt with every channel in its own codec, in the geometry the shipped
//! containers have.
//!
//! ⚠ Every container here is laid out by hand from the block layout, so
//! nothing under test produced the bytes it is checked against.

use or_io::awc::codec::{encode_adpcm, ADPCM_BLOCK_PAYLOAD_BYTES};
use or_io::awc::{crypto, Accumulator, AwcChunkKind, AwcFile, AwcKey, AWC_MAGIC_LE};

const PACKET: usize = 2048;
const PCM16_PACKET_SAMPLES: usize = PACKET / 2;
const ADPCM_PACKET_SAMPLES: usize = ADPCM_BLOCK_PAYLOAD_BYTES * 2;
const NO_FIRST_PACKET: u32 = u32::MAX;

fn a_key() -> AwcKey {
    AwcKey([0xA1B2_C3D4, 0x1122_3344, 0x5566_7788, 0x99AA_BBCC])
}

/// Channel `c`'s sample `i`: a slow triangle wave each channel runs at its own pace.
fn signal(c: usize, i: usize) -> i16 {
    let period = 400 + 90 * c;
    let phase = (i % period) as i32;
    let half = (period / 2) as i32;
    let tri = if phase < half {
        phase
    } else {
        period as i32 - phase
    };
    ((tri - half / 2) * 60) as i16
}

fn channel_pcm(c: usize, samples: usize) -> Vec<u8> {
    (0..samples)
        .flat_map(|i| signal(c, i).to_le_bytes())
        .collect()
}

fn interleaved(channels: usize, samples: usize) -> Vec<u8> {
    let per: Vec<Vec<u8>> = (0..channels).map(|c| channel_pcm(c, samples)).collect();
    (0..samples)
        .flat_map(|i| per.iter().flat_map(move |p| p[i * 2..i * 2 + 2].to_vec()))
        .collect()
}

fn put(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// A container of `channels` PCM16 or ADPCM channels, two packets each per block,
/// in blocks of `block_packets` packets, its table's first-packet words as given.
fn container(
    channels: usize,
    adpcm: bool,
    samples: usize,
    block_packets: usize,
    first_packets: bool,
    flags: u16,
    key: AwcKey,
) -> Vec<u8> {
    const PER_BLOCK: usize = 2;
    let per_packet = if adpcm {
        ADPCM_PACKET_SAMPLES
    } else {
        PCM16_PACKET_SAMPLES
    };
    let block_size = block_packets * PACKET;
    let runs: Vec<Vec<u8>> = (0..channels)
        .map(|c| {
            let pcm = channel_pcm(c, samples);
            let mut run = if adpcm { encode_adpcm(&pcm) } else { pcm };
            run.resize(run.len().next_multiple_of(PACKET), 0);
            run
        })
        .collect();
    let packets = samples.div_ceil(per_packet);
    let blocks = packets.div_ceil(PER_BLOCK);
    let mut data = Vec::new();
    for b in 0..blocks {
        let first = b * PER_BLOCK;
        let here = (packets - first).min(PER_BLOCK);
        let held = (samples - first * per_packet).min(here * per_packet);
        let mut block = vec![0u8; (channels * 24 + channels * here * 4).next_multiple_of(PACKET)];
        let mut offsets = channels * 24;
        for c in 0..channels {
            put(
                &mut block,
                c * 24,
                if first_packets {
                    (c * here) as u32
                } else {
                    NO_FIRST_PACKET
                },
            );
            put(&mut block, c * 24 + 4, here as u32);
            put(&mut block, c * 24 + 12, held as u32);
            for p in 0..here {
                put(&mut block, offsets, ((first + p) * per_packet) as u32);
                offsets += 4;
            }
        }
        for run in &runs {
            block.extend_from_slice(&run[first * PACKET..(first + here) * PACKET]);
        }
        block.resize(block_size, 0);
        data.extend_from_slice(&block);
    }
    if flags & 8 != 0 {
        for block in data.chunks_mut(block_size) {
            crypto::encrypt(key, block);
        }
    }

    let mut format = Vec::new();
    for word in [blocks, block_size, channels] {
        format.extend_from_slice(&(word as u32).to_le_bytes());
    }
    for c in 0..channels {
        format.extend_from_slice(&(0x0100_0000 + c as u32).to_le_bytes());
        format.extend_from_slice(&(samples as u32).to_le_bytes());
        format.extend_from_slice(&(-191i16).to_le_bytes());
        format.extend_from_slice(&24000u16.to_le_bytes());
        format.extend_from_slice(&[if adpcm { 4 } else { 0 }, 0, 0, 0]);
    }
    let seek: Vec<u8> = (0..blocks)
        .flat_map(|b| ((b * PER_BLOCK * per_packet) as u32).to_le_bytes())
        .collect();

    let streams = 1 + channels;
    let tables = 16 + streams * 4 + 3 * 8;
    let mut out = vec![0u8; tables];
    put(&mut out, 0, AWC_MAGIC_LE);
    out[4..6].copy_from_slice(&1u16.to_le_bytes());
    out[6..8].copy_from_slice(&flags.to_le_bytes());
    put(&mut out, 8, streams as u32);
    put(&mut out, 12, tables as u32);
    put(&mut out, 16, 3 << 29);
    for c in 0..channels {
        put(&mut out, 20 + c * 4, 0x0100_0000 + c as u32);
    }
    let mut entries = Vec::new();
    for (kind, payload) in [(0x48u64, format), (0x55, data), (0xA3, seek)] {
        while !out.len().is_multiple_of(16) {
            out.push(0xEE);
        }
        entries.push((out.len() as u64) | ((payload.len() as u64) << 28) | (kind << 56));
        out.extend_from_slice(&payload);
    }
    let at = 16 + streams * 4;
    for (i, raw) in entries.iter().enumerate() {
        out[at + i * 8..at + i * 8 + 8].copy_from_slice(&raw.to_le_bytes());
    }
    out
}

fn replaced(awc: &AwcFile, channels: usize, samples: usize, key: AwcKey) -> AwcFile {
    awc.with_stream_replaced(
        0,
        &interleaved(channels, samples),
        24000,
        channels as u16,
        key,
    )
    .expect("the stream is replaced")
}

/// A block of the rebuilt container, as the layout states it: what each
/// channel holds, in packets and samples, and where its first packet is said to be.
struct Block {
    packets: Vec<usize>,
    samples: Vec<usize>,
    first_packets: Vec<u32>,
    len: usize,
}

fn blocks(awc: &AwcFile, key: AwcKey) -> Vec<Block> {
    let source = &awc.streams[0];
    let format = source.stream_format().expect("streamformat");
    let n = format.channels.len();
    let data = awc.stream_data(source, key).expect("data");
    data.chunks(format.block_size as usize)
        .map(|block| {
            let word = |at: usize| u32::from_le_bytes(block[at..at + 4].try_into().unwrap());
            Block {
                packets: (0..n).map(|c| word(c * 24 + 4) as usize).collect(),
                samples: (0..n).map(|c| word(c * 24 + 12) as usize).collect(),
                first_packets: (0..n).map(|c| word(c * 24)).collect(),
                len: block.len(),
            }
        })
        .collect()
}

/// ⚠ **The packets a channel has in a full block are measured, not chosen.**
/// Over both installs' multi-channel containers every full block holds the
/// most packets of each channel that fit with its table and one spare
/// packet, and these are the shapes the installs have, as
/// `(channels, packets in a block, packets of each channel in it)`.
const MEASURED_SHAPES: [(usize, usize, usize); 13] = [
    (1, 256, 254),
    (2, 256, 127),
    (3, 256, 84),
    (4, 256, 63),
    (5, 256, 50),
    (6, 256, 42),
    (7, 256, 36),
    (8, 256, 31),
    (10, 256, 25),
    (12, 256, 21),
    (16, 256, 15),
    (16, 504, 31),
    (2, 150, 74),
];

#[test]
fn a_rebuilt_container_has_the_blocks_the_shipped_ones_have() {
    for (n, block_packets, per_block) in MEASURED_SHAPES {
        let pcm_samples = per_block * PCM16_PACKET_SAMPLES * 2 + 700;
        let original = container(n, false, 2000, block_packets, true, 4, AwcKey::default());
        let awc = AwcFile::parse(&original, AwcKey::default()).expect("parse");
        let rebuilt = replaced(&awc, n, pcm_samples, AwcKey::default());

        let got = blocks(&rebuilt, AwcKey::default());
        assert_eq!(got.len(), 3, "{n} channels in {block_packets} packets");
        for block in &got[..2] {
            assert_eq!(block.packets, vec![per_block; n], "{n} in {block_packets}");
            assert_eq!(block.len, block_packets * PACKET, "a full block is padded");
        }
        assert_eq!(got[2].samples, vec![700; n]);
        assert_eq!(got[2].packets, vec![1; n]);
        let table = (n * 24 + n * 4).next_multiple_of(PACKET);
        assert_eq!(
            got[2].len,
            table + n * PACKET,
            "the last block ends with its runs"
        );
    }
}

/// ⚠ The first-packet words are what no reader here uses and the shipped
/// containers still carry: the spare packets dealt to the first channels in a
/// full block, none in the last, and −1 throughout a container that never
/// states them.
#[test]
fn the_blocks_state_their_first_packets_as_the_shipped_ones_do() {
    let original = container(4, false, 2000, 256, true, 4, AwcKey::default());
    let awc = AwcFile::parse(&original, AwcKey::default()).expect("parse");
    let got = blocks(
        &replaced(&awc, 4, 63 * 1024 * 2 + 900, AwcKey::default()),
        AwcKey::default(),
    );
    // 254 spare packets over four channels: 63 each and two over.
    assert_eq!(got[0].first_packets, vec![0, 64, 128, 191]);
    assert_eq!(got[1].first_packets, vec![0, 64, 128, 191]);
    assert_eq!(got[2].first_packets, vec![0, 1, 2, 3]);

    let unset = container(4, false, 2000, 256, false, 4, AwcKey::default());
    let awc = AwcFile::parse(&unset, AwcKey::default()).expect("parse");
    let got = blocks(
        &replaced(&awc, 4, 63 * 1024 * 2 + 900, AwcKey::default()),
        AwcKey::default(),
    );
    assert!(got
        .iter()
        .all(|b| b.first_packets.iter().all(|w| *w == NO_FIRST_PACKET)));
}

#[test]
fn a_rebuilt_pcm16_container_decodes_to_the_wav_exactly() {
    let original = container(3, false, 5000, 8, true, 4, AwcKey::default());
    let awc = AwcFile::parse(&original, AwcKey::default()).expect("parse");
    let samples = 9 * PCM16_PACKET_SAMPLES + 5;
    let rebuilt = replaced(&awc, 3, samples, AwcKey::default());
    let reparsed = AwcFile::parse(
        &rebuilt.to_bytes(AwcKey::default()).expect("write"),
        AwcKey::default(),
    )
    .expect("reparse");
    assert_eq!(reparsed.unaccounted_bytes(), 0);

    let channels = reparsed
        .decode_channels(AwcKey::default(), Accumulator::Clamped)
        .expect("channels");
    for (c, pcm) in channels.iter().enumerate() {
        assert!(*pcm == channel_pcm(c, samples), "channel {c}");
    }
    let seek = reparsed.streams[0]
        .chunk(AwcChunkKind::SeekTable)
        .expect("seektable");
    let starts: Vec<u32> = seek
        .payload
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    let format = reparsed.streams[0].stream_format().expect("streamformat");
    assert_eq!(starts.len(), format.block_count as usize);
    assert_eq!(starts[..2], [0, (2 * PCM16_PACKET_SAMPLES) as u32]);
    assert!(format
        .channels
        .iter()
        .all(|c| c.samples as usize == samples));
}

/// ⚠ A channel's ADPCM run is encoded whole and cut into packets, so a block
/// join is no restart: the join moves by about as much as any two samples,
/// where an encoder restarted from zero at every block would jump.
#[test]
fn a_rebuilt_adpcm_channel_runs_on_across_its_block_joins() {
    let original = container(2, true, 3000, 6, true, 4, AwcKey::default());
    let awc = AwcFile::parse(&original, AwcKey::default()).expect("parse");
    let samples = 5 * 2 * ADPCM_PACKET_SAMPLES + 321;
    let rebuilt = replaced(&awc, 2, samples, AwcKey::default());
    let format = rebuilt.streams[0].stream_format().expect("streamformat");
    assert_eq!(format.block_count, 6);

    let channels = rebuilt
        .decode_channels(AwcKey::default(), Accumulator::Clamped)
        .expect("channels");
    let at = |pcm: &[u8], i: usize| i32::from(i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]));
    for (c, decoded) in channels.iter().enumerate() {
        let source = channel_pcm(c, samples);
        let interior: i64 = (1..samples)
            .map(|i| i64::from((at(decoded, i) - at(decoded, i - 1)).abs()))
            .sum();
        let joins: Vec<usize> = (1..6).map(|b| b * 2 * ADPCM_PACKET_SAMPLES).collect();
        let joined: i64 = joins
            .iter()
            .map(|j| i64::from((at(decoded, *j) - at(decoded, *j - 1)).abs()))
            .sum();
        let (interior, joined) = (
            interior as f64 / (samples - 1) as f64,
            joined as f64 / joins.len() as f64,
        );
        assert!(
            joined < 2.0 * interior + 8.0,
            "channel {c} moves by {joined:.1} across its joins and {interior:.1} anywhere"
        );
        let worst = (16..samples)
            .map(|i| (at(&source, i) - at(decoded, i)).abs())
            .max()
            .unwrap_or(0);
        assert!(worst < 400, "channel {c} is off by {worst}");
    }
}

#[test]
fn a_scrambled_multi_channel_container_is_rebuilt_scrambled_block_by_block() {
    let original = container(2, false, 5000, 8, true, 4 | 8, a_key());
    let awc = AwcFile::parse(&original, a_key()).expect("parse");
    let samples = 4 * PCM16_PACKET_SAMPLES + 9;
    let rebuilt = replaced(&awc, 2, samples, a_key());
    let bytes = rebuilt.to_bytes(a_key()).expect("write");
    let reparsed = AwcFile::parse(&bytes, a_key()).expect("reparse");
    let channels = reparsed
        .decode_channels(a_key(), Accumulator::Clamped)
        .expect("channels");
    assert!(channels[1] == channel_pcm(1, samples));
    // Not the plain blocks: the file's data still scrambles.
    let plain = AwcFile::parse(&bytes, AwcKey::default()).expect("reparse");
    assert!(plain
        .decode_channels(AwcKey::default(), Accumulator::Clamped)
        .is_none_or(|c| c[1] != channel_pcm(1, samples)));

    let why = awc
        .with_stream_replaced(0, &interleaved(2, samples), 24000, 2, AwcKey::default())
        .expect_err("no key to scramble with");
    assert!(why.to_string().contains("key"), "{why}");
}

#[test]
fn a_replacement_the_container_cannot_take_is_refused_by_name() {
    let original = container(2, true, 5000, 8, true, 4, AwcKey::default());
    let awc = AwcFile::parse(&original, AwcKey::default()).expect("parse");
    let key = AwcKey::default();
    let wav = interleaved(2, 3000);

    let why = awc
        .with_stream_replaced(0, &wav, 24000, 1, key)
        .expect_err("mono");
    assert!(
        why.to_string().contains("1 channel(s)") && why.to_string().contains("has 2"),
        "{why}"
    );
    let why = awc
        .with_stream_replaced(0, &wav, 22050, 2, key)
        .expect_err("rate");
    assert!(
        why.to_string().contains("22050 Hz") && why.to_string().contains("24000 Hz"),
        "{why}"
    );
    let why = awc
        .with_stream_replaced(0, &[], 24000, 2, key)
        .expect_err("empty");
    assert!(why.to_string().contains("no samples"), "{why}");
    let why = awc
        .with_stream_replaced(9, &wav, 24000, 2, key)
        .expect_err("no stream");
    assert!(why.to_string().contains("no such stream"), "{why}");
    // A channel with no stream of its own carries no format to replace by.
    let why = awc
        .with_stream_replaced(1, &wav, 24000, 2, key)
        .expect_err("a channel");
    assert!(why.to_string().contains("no format"), "{why}");

    // Channels in different codecs: one of them becomes MP3's number.
    let mut mixed = original.clone();
    let at = awc.streams[0]
        .chunk(AwcChunkKind::StreamFormat)
        .expect("chunk")
        .offset as usize;
    mixed[at + 12 + 16 + 12] = 0;
    let awc = AwcFile::parse(&mixed, key).expect("parse");
    let why = awc
        .with_stream_replaced(0, &wav, 24000, 2, key)
        .expect_err("mixed");
    assert!(why.to_string().contains("different codecs"), "{why}");

    // MP3 (7) and XMA (5) channels have no encoder here.
    for (byte, name) in [(7u8, "MP3"), (5, "XMA")] {
        let mut other = original.clone();
        other[at + 12 + 12] = byte;
        other[at + 12 + 16 + 12] = byte;
        let awc = AwcFile::parse(&other, key).expect("parse");
        let why = awc
            .with_stream_replaced(0, &wav, 24000, 2, key)
            .expect_err(name);
        assert!(why.to_string().contains(name), "{why}");
    }
}

/// A WAV needs no more of the file than the stream it replaces: the others'
/// bytes, and the chunks after them, are kept, and the rest of the container
/// still accounts for every byte.
#[test]
fn a_replaced_stream_leaves_the_rest_of_the_container_whole() {
    let original = container(2, false, 5000, 8, true, 4, AwcKey::default());
    let awc = AwcFile::parse(&original, AwcKey::default()).expect("parse");
    let rebuilt = replaced(&awc, 2, 9000, AwcKey::default());
    assert_eq!(rebuilt.unaccounted_bytes(), 0);
    assert_eq!(rebuilt.streams.len(), awc.streams.len());
    assert_eq!(
        rebuilt.streams.iter().map(|s| s.id).collect::<Vec<_>>(),
        awc.streams.iter().map(|s| s.id).collect::<Vec<_>>()
    );
    let bytes = rebuilt.to_bytes(AwcKey::default()).expect("write");
    assert_eq!(
        AwcFile::parse(&bytes, AwcKey::default()).expect("reparse"),
        rebuilt
    );
    // Nothing was written to the original.
    assert_eq!(awc.to_bytes(AwcKey::default()).expect("write"), original);
}

/// ⚠ **A channel the WAV leaves as it decodes keeps the packets it shipped with**, so an edit to
/// one channel costs no other a generation of ADPCM. These packets are not the encoder's own: each
/// block's header starts from a state of its own and the nibbles are noise, so re-encoding their
/// decode gives other bytes, which the control below shows.
#[test]
fn a_channel_the_wav_leaves_as_it_decodes_keeps_its_shipped_packets() {
    // A container whose channel 0's packets are noise, hand-laid like `container`'s, over
    // channel 1's from the encoder.
    let mut original = container(
        2,
        true,
        3 * ADPCM_PACKET_SAMPLES,
        6,
        true,
        4,
        AwcKey::default(),
    );
    let awc = AwcFile::parse(&original, AwcKey::default()).expect("parse");
    let data_at = awc.streams[0]
        .chunk(AwcChunkKind::Data)
        .expect("data")
        .offset as usize;
    let table = (2usize * 24 + 2 * 2 * 4).next_multiple_of(PACKET);
    let mut seed = 7u32;
    for packet in 0..2 {
        // Channel 0's two packets of the first block.
        let at = data_at + table + packet * PACKET;
        original[at] = 20 + packet as u8 * 9;
        original[at + 1] = 0;
        original[at + 2..at + 4].copy_from_slice(&(1200i16 * (packet as i16 + 1)).to_le_bytes());
        for byte in &mut original[at + 4..at + PACKET] {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *byte = (seed >> 24) as u8;
        }
    }
    let awc = AwcFile::parse(&original, AwcKey::default()).expect("parse");
    let key = AwcKey::default();
    let shipped = awc
        .decode_channels(key, Accumulator::Clamped)
        .expect("channels");
    let samples = shipped[0].len() / 2;
    let wav = |channel_0: &[u8], channel_1: &[u8]| -> Vec<u8> {
        channel_0
            .as_chunks::<2>()
            .0
            .iter()
            .zip(channel_1.as_chunks::<2>().0.iter())
            .flat_map(|(a, b)| [a.as_slice(), b.as_slice()].concat())
            .collect()
    };
    let louder: Vec<u8> = shipped[1]
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|s| {
            i16::from_le_bytes([s[0], s[1]])
                .saturating_div(2)
                .to_le_bytes()
        })
        .collect();

    // Channel 1 edited, channel 0 as it decodes: channel 0 decodes to exactly what it did.
    let kept = awc
        .with_stream_replaced(0, &wav(&shipped[0], &louder), 24000, 2, key)
        .expect("replaced");
    let after = kept
        .decode_channels(key, Accumulator::Clamped)
        .expect("channels");
    assert!(after[0] == shipped[0], "channel 0 changed");
    assert!(after[1] != shipped[1], "channel 1 was not replaced");

    // Control: one sample of channel 0 changed by a bit and it is encoded, so it no longer decodes
    // to what the noise did.
    let mut nudged = shipped[0].clone();
    nudged[2] ^= 1;
    let encoded = awc
        .with_stream_replaced(0, &wav(&nudged, &louder), 24000, 2, key)
        .expect("replaced");
    let after = encoded
        .decode_channels(key, Accumulator::Clamped)
        .expect("channels");
    assert!(after[0] != shipped[0], "the control kept the noise too");
    assert_eq!(after[0].len() / 2, samples);
}
