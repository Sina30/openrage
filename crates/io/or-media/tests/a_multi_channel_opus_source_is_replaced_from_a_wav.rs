//! A multi-channel Opus source replaced from a WAV keeps the packets of a channel the WAV leaves as
//! it decodes, and plays the channel the WAV changes as the WAV holds it.

use std::f64::consts::TAU;

use or_io::awc::ogg::PRE_SKIP;
use or_io::awc::opus::OpusStream;
use or_io::awc::{AwcFile, AwcKey};
use or_media::opus::{decode, encode, replaced, Settings};

/// Samples of each channel.
const SAMPLES: usize = 30_000;

/// A container of mono Opus channels `ids` whose blocks are not written yet: a stream format, one
/// data packet and one seek entry, in GTA V's table layout.
fn unwritten(ids: &[u32], block_size: u32) -> Vec<u8> {
    let mut format = Vec::new();
    for w in [1, block_size, ids.len() as u32] {
        format.extend(w.to_le_bytes());
    }
    for id in ids {
        format.extend(id.to_le_bytes());
        format.extend(960u32.to_le_bytes());
        format.extend([0, 0, 0x80, 0xBB, 11, 1, 0xFF, 0xFF]);
    }
    let chunks = [(0x48u64, format), (0x55, vec![0; 2048]), (0xA3, vec![0; 4])];
    let streams = 1 + ids.len();
    let tables = 16 + 4 * streams + 8 * chunks.len();
    let mut out = b"ADAT".to_vec();
    for w in [0xFF00_0001u32, streams as u32, tables as u32, 3 << 29] {
        out.extend(w.to_le_bytes());
    }
    ids.iter().for_each(|id| out.extend(id.to_le_bytes()));
    let mut at = tables;
    for (kind, payload) in &chunks {
        out.extend((kind << 56 | (payload.len() as u64) << 28 | at as u64).to_le_bytes());
        at += payload.len();
    }
    chunks.iter().for_each(|(_, payload)| out.extend(payload));
    out
}

fn tone(hz: f64) -> Vec<i16> {
    (0..SAMPLES)
        .map(|t| (6000.0 * (TAU * hz * t as f64 / 48_000.0).sin()).round() as i16)
        .collect()
}

/// Signal over error in dB, from 20 ms in to 20 ms before the end.
fn snr(want: &[i16], got: &[i16]) -> f64 {
    let (want, got) = (&want[960..want.len() - 960], &got[960..got.len() - 960]);
    let (mut signal, mut error) = (0.0, 0.0);
    for (w, g) in want.iter().zip(got) {
        let (w, g) = (f64::from(*w), f64::from(*g));
        signal += w * w;
        error += (w - g) * (w - g);
    }
    10.0 * (signal / f64::max(error, 1.0)).log10()
}

#[test]
fn a_channel_the_wav_leaves_keeps_its_packets_and_the_one_it_changes_plays_as_the_wav() {
    let settings = Settings {
        bitrate: 48_000,
        voice: false,
    };
    let old: Vec<OpusStream> = [(0x0C01u32, 440.0), (0x0C02, 660.0)]
        .iter()
        .map(|(id, hz)| OpusStream {
            id: *id,
            channels: 1,
            samples: SAMPLES as u32,
            packets: encode(&tone(*hz), 1, settings).expect("it encodes"),
        })
        .collect();
    let bytes = unwritten(&[0x0C01, 0x0C02], 16 * 2048);
    let shell = AwcFile::parse(&bytes, AwcKey::default()).expect("parses");
    let file = shell.with_opus_channels_replaced(0, &old).expect("built");

    let kept = decode(&old[0]).expect("it decodes");
    let want = tone(1000.0);
    let mut wav = Vec::with_capacity(4 * SAMPLES);
    for (a, b) in kept.iter().zip(&want) {
        wav.extend(a.to_le_bytes());
        wav.extend(b.to_le_bytes());
    }
    let edited = replaced(&file, 0, &wav, 48_000, 2).expect("replaced");
    let saved = edited.to_bytes(AwcKey::default()).expect("written");
    let read = AwcFile::parse(&saved, AwcKey::default()).expect("it reads back");
    let read = read.opus_streams(0).expect("split");
    assert_eq!(read[0], old[0]);
    assert_ne!(read[1].packets, old[1].packets);
    let facts = (read[1].id, read[1].samples, read[1].packets.len());
    assert_eq!(facts, (0x0C02, SAMPLES as u32, old[1].packets.len()));

    // The floor: 9 dB above the tone left late by the pre-skip, half a 1 kHz period out.
    let mut late = vec![0; usize::from(PRE_SKIP)];
    late.extend_from_slice(&want);
    let floor = snr(&want, &late[..SAMPLES]) + 9.0;
    let heard = snr(&want, &decode(&read[1]).expect("it decodes"));
    assert!(heard > floor, "{heard:.1} dB, floor {floor:.1} dB");

    for (rate, channels, said) in [
        (44_100, 2, "resamples nothing"),
        (48_000, 1, "save it with 2"),
    ] {
        let why = replaced(&file, 0, &wav, rate, channels).expect_err("refused");
        assert!(why.to_string().contains(said), "{why}");
    }
}
