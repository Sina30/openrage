//! A multi-channel `.awc` built by hand from the container's documented block
//! layout, with a distinct signal per channel, decoded back channel by
//! channel.
//!
//! ⚠ **No shipped container states a peak for a channel**, so the corpus can
//! only check continuity across blocks, and a decoder that swaps two channels
//! the same way in every block stays continuous. Here each channel's source is
//! known, so such a swap fails.

use or_io::awc::{Accumulator, AwcFile, AwcKey, AWC_MAGIC_LE};

const PACKET: usize = 2048;
const CHANNELS: usize = 3;
const PACKETS_PER_BLOCK: usize = 2;
/// PCM16 samples in one packet.
const PACKET_SAMPLES: usize = PACKET / 2;
/// Two whole blocks and part of a third.
const SAMPLES: usize = 2 * PACKETS_PER_BLOCK * PACKET_SAMPLES + 1500;
const DATA_KIND: u8 = 0x55;
const STREAMFORMAT_KIND: u8 = 0x48;

/// Channel `c`'s sample `i`: a rising ramp, a falling one and a steeper one.
fn signal(c: usize, i: usize) -> i16 {
    match c {
        0 => i as i16,
        1 => -(i as i16),
        _ => (i * 3 % 30_000) as i16,
    }
}

fn source(c: usize) -> Vec<u8> {
    (0..SAMPLES)
        .flat_map(|i| signal(c, i).to_le_bytes())
        .collect()
}

fn put_u32(out: &mut [u8], at: usize, v: usize) {
    out[at..at + 4].copy_from_slice(&(v as u32).to_le_bytes());
}

/// The container, and where its data chunk starts.
fn container() -> (Vec<u8>, usize) {
    let table_bytes = CHANNELS * 24 + CHANNELS * PACKETS_PER_BLOCK * 4;
    let block_size = table_bytes.next_multiple_of(PACKET) + CHANNELS * PACKETS_PER_BLOCK * PACKET;
    let blocks = SAMPLES.div_ceil(PACKETS_PER_BLOCK * PACKET_SAMPLES);
    let streams = 1 + CHANNELS;
    let tables_end = 16 + streams * 4 + 2 * 8;
    let format_len = 12 + CHANNELS * 16;
    let data_at = (tables_end + format_len).next_multiple_of(PACKET);
    let mut out = vec![0u8; data_at + blocks * block_size];

    put_u32(&mut out, 0, AWC_MAGIC_LE as usize);
    out[4..6].copy_from_slice(&1u16.to_le_bytes());
    out[6..8].copy_from_slice(&4u16.to_le_bytes());
    put_u32(&mut out, 8, streams);
    put_u32(&mut out, 12, tables_end);
    // The source stream, hash 0, with two chunks; each channel stream has none.
    put_u32(&mut out, 16, 2 << 29);
    let id = |c: usize| 0x0100_0000 + c;
    for c in 0..CHANNELS {
        put_u32(&mut out, 20 + c * 4, id(c));
    }
    let chunk = |kind: u8, size: usize, offset: usize| -> [u8; 8] {
        ((offset as u64) | ((size as u64) << 28) | ((kind as u64) << 56)).to_le_bytes()
    };
    let at = 16 + streams * 4;
    out[at..at + 8].copy_from_slice(&chunk(STREAMFORMAT_KIND, format_len, tables_end));
    out[at + 8..at + 16].copy_from_slice(&chunk(DATA_KIND, blocks * block_size, data_at));

    put_u32(&mut out, tables_end, blocks);
    put_u32(&mut out, tables_end + 4, block_size);
    put_u32(&mut out, tables_end + 8, CHANNELS);
    for c in 0..CHANNELS {
        let entry = tables_end + 12 + c * 16;
        put_u32(&mut out, entry, id(c));
        put_u32(&mut out, entry + 4, SAMPLES);
        out[entry + 10..entry + 12].copy_from_slice(&32_000u16.to_le_bytes());
        // Codec 0, PCM16.
        out[entry + 12] = 0;
    }

    let sources: Vec<Vec<u8>> = (0..CHANNELS).map(source).collect();
    let per_block = PACKETS_PER_BLOCK * PACKET_SAMPLES;
    for b in 0..blocks {
        let block = data_at + b * block_size;
        let samples = (SAMPLES - b * per_block).min(per_block);
        let packets = samples.div_ceil(PACKET_SAMPLES);
        let mut offsets = block + CHANNELS * 24;
        let mut run = block + table_bytes.next_multiple_of(PACKET);
        for (c, pcm) in sources.iter().enumerate() {
            put_u32(&mut out, block + c * 24 + 4, packets);
            put_u32(&mut out, block + c * 24 + 12, samples);
            for p in 0..packets {
                put_u32(&mut out, offsets, p * PACKET_SAMPLES);
                offsets += 4;
            }
            let from = b * per_block * 2;
            out[run..run + samples * 2].copy_from_slice(&pcm[from..from + samples * 2]);
            run += packets * PACKET;
        }
    }
    (out, data_at)
}

#[test]
fn each_channel_of_a_hand_built_multi_channel_container_decodes_to_its_source() {
    let (bytes, _) = container();
    let awc = AwcFile::parse(&bytes, AwcKey::default()).expect("parse");
    assert!(awc.multi_channel());
    assert_eq!(awc.unaccounted_bytes(), 0);

    let channels = awc
        .decode_channels(AwcKey::default(), Accumulator::Clamped)
        .expect("channels");
    assert_eq!(channels.len(), CHANNELS);
    for (c, pcm) in channels.iter().enumerate() {
        let want = source(c);
        let first_wrong = pcm.chunks(2).zip(want.chunks(2)).position(|(a, b)| a != b);
        assert!(
            pcm.len() == want.len() && first_wrong.is_none(),
            "channel {c}: {} bytes against {}, first wrong sample {first_wrong:?}",
            pcm.len(),
            want.len()
        );
    }
}

/// A block whose header claims more packets than the block holds is a
/// refusal, not a read past its end into the next block.
#[test]
fn a_multi_channel_block_that_overflows_itself_is_refused() {
    let (mut bytes, data_at) = container();
    let last = data_at + (CHANNELS - 1) * 24 + 4;
    put_u32(&mut bytes, last, PACKETS_PER_BLOCK + 1);
    let awc = AwcFile::parse(&bytes, AwcKey::default()).expect("parse");
    assert!(awc
        .decode_channels(AwcKey::default(), Accumulator::Clamped)
        .is_none());
}
