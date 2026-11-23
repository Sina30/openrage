//! An `.awc` opens as its own kind: its streams listed, one played through a
//! sink, exported to WAV, and replaced from WAV before the file saves.
//!
//! ⚠ Every container here is built by hand, so nothing under test produced
//! the bytes it is checked against.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_io::awc::wav::{read_wav, wav_from_pcm16};
use or_io::awc::{AwcFile, AwcKey};
use or_media::Codec;

use super::{AppState, DocumentId, DocumentKind, LoadingState};
use crate::audio::{AudioOut, AwcDocument, Heard, NullSink};

const PCM_ID: u32 = 0x0BAD_F00D & 0x1FFF_FFFF;
const ADPCM_ID: u32 = 0x0123_4567;
const MP3_ID: u32 = 0x0765_4321;

/// One stream as the builder lays it out: a 20-byte `format` and a `data`.
struct Built {
    id: u32,
    codec: u8,
    rate: u16,
    samples: u32,
    data: Vec<u8>,
}

pub(super) fn ramp(samples: u32) -> Vec<u8> {
    (0..samples as i32)
        .flat_map(|i| (((i * 137) % 20000 - 10000) as i16).to_le_bytes())
        .collect()
}

/// A PCM16 stream, an ADPCM stream and an MP3 stream, each chunk 16-aligned
/// with non-zero filler between them so a rewrite that zeroes padding shows.
fn container(streams: &[Built]) -> Vec<u8> {
    let tables = 16 + streams.len() * 4 + streams.len() * 2 * 8;
    let mut out = vec![0u8; tables];
    out[0..4].copy_from_slice(b"ADAT");
    out[4..6].copy_from_slice(&1u16.to_le_bytes());
    out[6..8].copy_from_slice(&0xFF00u16.to_le_bytes());
    out[8..12].copy_from_slice(&(streams.len() as u32).to_le_bytes());
    out[12..16].copy_from_slice(&(tables as u32).to_le_bytes());
    let mut entries = Vec::new();
    for (i, s) in streams.iter().enumerate() {
        let head = s.id | (2 << 29);
        out[16 + i * 4..20 + i * 4].copy_from_slice(&head.to_le_bytes());
        let mut format = vec![0u8; 20];
        format[0..4].copy_from_slice(&s.samples.to_le_bytes());
        format[4..8].copy_from_slice(&(-1i32).to_le_bytes());
        format[8..10].copy_from_slice(&s.rate.to_le_bytes());
        format[19] = s.codec;
        for (kind, payload) in [(0xFAu8, format), (0x55u8, s.data.clone())] {
            while !out.len().is_multiple_of(16) {
                out.push(0xEE);
            }
            let raw = (out.len() as u64) | ((payload.len() as u64) << 28) | ((kind as u64) << 56);
            entries.push(raw);
            out.extend_from_slice(&payload);
        }
    }
    let table_at = 16 + streams.len() * 4;
    for (i, raw) in entries.iter().enumerate() {
        out[table_at + i * 8..table_at + i * 8 + 8].copy_from_slice(&raw.to_le_bytes());
    }
    out
}

pub(super) fn three_streams() -> Vec<u8> {
    container(&[
        Built {
            id: PCM_ID,
            codec: 0,
            rate: 32000,
            samples: 100,
            data: ramp(100),
        },
        Built {
            id: ADPCM_ID,
            codec: 4,
            rate: 24000,
            samples: 9,
            data: vec![0x10, 0, 0, 0, 0x12, 0x34, 0x56, 0x78],
        },
        Built {
            id: MP3_ID,
            codec: 7,
            rate: 32000,
            samples: 1152,
            data: vec![0xFF, 0xFB, 0x88, 0xC4, 0, 0, 0, 0],
        },
    ])
}

fn open(state: &mut AppState, path: PathBuf) -> DocumentId {
    state.load_file(path);
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.load_receiver.is_some() && Instant::now() < deadline {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(state.loading, LoadingState::Idle),
        "the load did not finish: {}",
        state.error_dialog_message
    );
    let doc = state
        .active_document
        .expect("the .awc opened as a document");
    assert_eq!(state.document(doc).map(|d| d.kind), Some(DocumentKind::Awc));
    doc
}

fn quiet(state: &mut AppState) -> Heard {
    let sink = NullSink::default();
    let heard = Arc::clone(&sink.heard);
    state.audio = AudioOut::new(Box::new(sink));
    heard
}

pub(super) fn opened(bytes: &[u8]) -> (tempfile::TempDir, AppState, DocumentId, Heard) {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("sounds.awc");
    std::fs::write(&path, bytes).expect("the container is written");
    let mut state = AppState::new();
    let heard = quiet(&mut state);
    let doc = open(&mut state, path);
    (dir, state, doc, heard)
}

#[test]
fn an_awc_opens_with_its_streams_and_exports_one_to_wav() {
    let (_dir, mut state, doc, _) = opened(&three_streams());

    let rows = state.awc_streams(doc).expect("the streams list");
    let listed: Vec<_> = rows
        .iter()
        .map(|r| (r.id, r.codec, r.channels, r.sample_rate, r.samples))
        .collect();
    assert_eq!(
        listed,
        vec![
            (PCM_ID, Codec::Pcm16, 1, 32000, 100),
            (ADPCM_ID, Codec::RageAdpcm, 1, 24000, 9),
            (MP3_ID, Codec::Mp3, 1, 32000, 1152),
        ]
    );
    assert!(rows[0].name().contains(&format!("{PCM_ID:08X}")));

    let wav = state
        .awc_wav(doc, rows[0].stream)
        .expect("the PCM stream exports");
    assert_eq!(wav, wav_from_pcm16(&ramp(100), 32000, 1));

    // Nine ADPCM samples: the WAV holds exactly what the decoder emits.
    let wav = state
        .awc_wav(doc, rows[1].stream)
        .expect("the ADPCM stream exports");
    assert_eq!(wav.len(), 44 + 9 * 2);
}

#[test]
fn a_stream_plays_through_the_sink_and_stops() {
    let (_dir, mut state, doc, heard) = opened(&three_streams());
    let rows = state.awc_streams(doc).expect("the streams list");

    state
        .awc_play(doc, rows[0].stream)
        .expect("the PCM stream plays");
    assert_eq!(state.audio.playing(), Some((doc, rows[0].stream)));
    let want: Vec<i16> = ramp(100)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    assert_eq!(
        heard.lock().expect("the sink's record").last(),
        Some(&(32000, 1, want))
    );

    state.audio.stop();
    assert_eq!(state.audio.playing(), None);
}

/// ⚠ An MP3 stream whose frames do not decode is refused by name, not played
/// as noise: this one's single frame is cut short, and the reason has to say
/// MP3 rather than "failed".
#[test]
fn an_mp3_stream_says_why_it_cannot_play() {
    let (_dir, mut state, doc, _) = opened(&three_streams());
    let rows = state.awc_streams(doc).expect("the streams list");
    let why = state
        .awc_play(doc, rows[2].stream)
        .expect_err("a cut-short frame does not decode");
    assert!(why.contains("MP3"), "{why}");
    assert_eq!(state.audio.playing(), None);
    assert!(state.awc_wav(doc, rows[2].stream).is_err());
}

#[test]
fn a_stream_the_app_cannot_encode_is_refused_by_name() {
    let (_dir, mut state, doc, _) = opened(&three_streams());
    let rows = state.awc_streams(doc).expect("the streams list");
    let wav = wav_from_pcm16(&ramp(10), 24000, 1);
    let why = state
        .awc_replace(doc, rows[2].stream, &wav)
        .expect_err("an MP3 stream at another rate");
    assert!(why.contains("MP3") && why.contains("32000 Hz"), "{why}");
    let why = state
        .awc_replace(doc, rows[2].stream, &wav_from_pcm16(&ramp(10), 32000, 2))
        .expect_err("a stereo WAV into a mono stream");
    assert!(why.contains("channel"), "{why}");
    let why = state
        .awc_replace(doc, rows[1].stream, &wav_from_pcm16(&ramp(10), 22050, 1))
        .expect_err("an ADPCM stream at another rate");
    assert!(
        why.contains("22050 Hz") && why.contains("24000 Hz"),
        "{why}"
    );
    let why = state
        .awc_replace(doc, rows[0].stream, &wav_from_pcm16(&ramp(10), 24000, 2))
        .expect_err("a stereo WAV into a mono stream");
    assert!(why.contains("channel"), "{why}");
    assert!(!state.document(doc).expect("doc").dirty);
}

#[test]
fn a_replaced_stream_saves_and_reopens_equal() {
    let (dir, mut state, doc, _) = opened(&three_streams());
    let rows = state.awc_streams(doc).expect("the streams list");

    // Longer than the original, so every later chunk has to move.
    let pcm = ramp(1001);
    state
        .awc_replace(doc, rows[0].stream, &wav_from_pcm16(&pcm, 22050, 1))
        .expect("a PCM stream is replaced");
    assert!(state.document(doc).expect("doc").dirty);
    let replaced = state.awc_streams(doc).expect("the streams list");
    assert_eq!(
        (replaced[0].sample_rate, replaced[0].samples),
        (22050, 1001)
    );
    let mp3_before = state.awc_file(doc).expect("parsed").streams[2].clone();

    let path = dir.path().join("saved.awc");
    assert!(state.save_active_document_as(path.clone()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    let saved = std::fs::read(&path).expect("the save wrote the file");
    let reread = AwcFile::parse(&saved, AwcKey::default()).expect("the save parses");
    assert_eq!(reread.unaccounted_bytes(), 0);
    assert_eq!(&reread, state.awc_file(doc).expect("parsed"));
    assert_eq!(
        reread.streams[2]
            .chunks
            .iter()
            .map(|c| &c.payload)
            .collect::<Vec<_>>(),
        mp3_before
            .chunks
            .iter()
            .map(|c| &c.payload)
            .collect::<Vec<_>>(),
        "a stream nobody replaced keeps its bytes"
    );

    let mut again = AppState::new();
    quiet(&mut again);
    let reopened = open(&mut again, path);
    assert_eq!(again.awc_streams(reopened), state.awc_streams(doc));
    assert_eq!(
        again.awc_wav(reopened, rows[0].stream).expect("export"),
        wav_from_pcm16(&pcm, 22050, 1)
    );
}

/// A WAV into an ADPCM stream is encoded to ADPCM at the stream's own rate,
/// and what reopens decodes to the WAV's samples within ADPCM's error: a
/// quarter of the sine's largest step between samples, once the encoder's
/// step has climbed from its smallest.
#[test]
fn a_replaced_adpcm_stream_saves_and_reopens_within_adpcm_error() {
    let (dir, mut state, doc, _) = opened(&three_streams());
    let rows = state.awc_streams(doc).expect("the streams list");

    let (rate, hz, amplitude) = (24000.0, 440.0, 8000.0);
    let samples = 9001;
    let pcm: Vec<u8> = (0..samples)
        .flat_map(|i| {
            let s = amplitude * (std::f64::consts::TAU * hz * f64::from(i) / rate).sin();
            (s.round() as i16).to_le_bytes()
        })
        .collect();
    state
        .awc_replace(doc, rows[1].stream, &wav_from_pcm16(&pcm, 24000, 1))
        .expect("an ADPCM stream is replaced");
    assert!(state.document(doc).expect("doc").dirty);
    let replaced = state.awc_streams(doc).expect("the streams list");
    assert_eq!(
        (
            replaced[1].codec,
            replaced[1].sample_rate,
            replaced[1].samples
        ),
        (Codec::RageAdpcm, 24000, samples)
    );

    let path = dir.path().join("saved.awc");
    assert!(state.save_active_document_as(path.clone()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut again = AppState::new();
    quiet(&mut again);
    let reopened = open(&mut again, path);
    let wav = again.awc_wav(reopened, rows[1].stream).expect("export");
    let (back, back_rate, channels) = read_wav(&wav).expect("the export is a WAV");
    assert_eq!((back.len(), back_rate, channels), (pcm.len(), 24000, 1));
    let at = |b: &[u8], i: usize| i32::from(i16::from_le_bytes([b[i * 2], b[i * 2 + 1]]));
    let bound = std::f64::consts::TAU * hz * amplitude / rate / 4.0;
    let worst = (16..samples as usize)
        .map(|i| (at(&pcm, i) - at(&back, i)).abs())
        .max()
        .unwrap_or(0);
    assert!(f64::from(worst) <= bound, "off by {worst}, over {bound:.0}");
}

/// A whole-file-scrambled container opens without a key, says the key is
/// what it lacks, and reads once one is given.
#[test]
fn a_scrambled_container_waits_for_the_awc_key() {
    let key = AwcKey([0xA1B2_C3D4, 0x1122_3344, 0x5566_7788, 0x99AA_BBCC]);
    let mut bytes = three_streams();
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    let plain = bytes.clone();
    or_io::awc::crypto::encrypt(key, &mut bytes);

    let mut awc = AwcDocument::open(&bytes).expect("a scrambled file still opens");
    let why = awc.file(AwcKey::default()).expect_err("no key");
    assert!(why.contains("key"), "{why}");
    let file = awc.file(key).expect("the key unscrambles it");
    assert_eq!(file.streams.len(), 3);
    assert_eq!(awc.to_bytes().expect("rewrite"), bytes);
    assert_ne!(awc.to_bytes().expect("rewrite"), plain);
}

/// What saving `doc` would write now.
fn written(state: &AppState, doc: DocumentId) -> Vec<u8> {
    match state.document(doc).map(|d| &d.payload) {
        Some(super::DocPayload::Awc(awc)) => awc.to_bytes().expect("the container writes"),
        _ => panic!("not an .awc document"),
    }
}

/// ⚠ Longer samples move every later chunk and the filler between them, so
/// undoing has to put back the layout as well as the samples.
#[test]
fn a_replaced_stream_undoes_to_the_bytes_the_file_opened_with() {
    let bytes = three_streams();
    let (_dir, mut state, doc, _) = opened(&bytes);
    let rows = state.awc_streams(doc).expect("the streams list");
    state.awc_file(doc).expect("parsed");
    assert_eq!(
        written(&state, doc),
        bytes,
        "precondition: it rewrites itself"
    );

    let wav = wav_from_pcm16(&ramp(1001), 22050, 1);
    state
        .awc_replace(doc, rows[0].stream, &wav)
        .expect("a PCM stream is replaced");
    let replaced = written(&state, doc);
    assert_ne!(replaced, bytes);

    assert!(state.undo().is_some(), "the replace is not a step");
    assert_eq!(written(&state, doc), bytes, "undone, it writes other bytes");
    assert!(!state.document(doc).expect("doc").dirty);
    assert!(state.redo().is_some());
    assert_eq!(
        written(&state, doc),
        replaced,
        "redone, the replace is lost"
    );
}

pub(super) const PACKET: usize = 2048;
const ADPCM_PACKET_SAMPLES: usize = 4088;

fn sine(hz: f64, amplitude: f64, rate: f64, samples: usize) -> Vec<u8> {
    (0..samples)
        .flat_map(|i| {
            let s = amplitude * (std::f64::consts::TAU * hz * i as f64 / rate).sin();
            (s.round() as i16).to_le_bytes()
        })
        .collect()
}

/// An ADPCM container of `channels` laid out by hand from the block layout:
/// two packets per channel in a block that holds two and the table, and one
/// spare, the table's offsets absolute and the seek table one word per block.
fn adpcm_container(channels: &[Vec<u8>], rate: u16) -> Vec<u8> {
    const PER_BLOCK: usize = 2;
    let n = channels.len();
    let block_size = (1 + n * PER_BLOCK + 1) * PACKET;
    let samples = channels[0].len() / 2;
    let runs: Vec<Vec<u8>> = channels
        .iter()
        .map(|pcm| {
            let mut run = or_io::awc::codec::encode_adpcm(pcm);
            run.resize(run.len().next_multiple_of(PACKET), 0);
            run
        })
        .collect();
    let packets = samples.div_ceil(ADPCM_PACKET_SAMPLES);
    let blocks = packets.div_ceil(PER_BLOCK);

    let mut data = Vec::new();
    let mut starts = Vec::new();
    for b in 0..blocks {
        let first = b * PER_BLOCK;
        let here = (packets - first).min(PER_BLOCK);
        let from = first * ADPCM_PACKET_SAMPLES;
        let block_samples = (samples - from).min(here * ADPCM_PACKET_SAMPLES);
        starts.push(from as u32);
        let mut block = vec![0u8; (n * 24 + n * here * 4).next_multiple_of(PACKET)];
        let mut offsets = n * 24;
        for c in 0..n {
            let head = [c * here, here, 0, block_samples, 0, 0];
            for (i, word) in head.iter().enumerate() {
                let at = c * 24 + i * 4;
                block[at..at + 4].copy_from_slice(&(*word as u32).to_le_bytes());
            }
            for p in 0..here {
                let first_sample = (from + p * ADPCM_PACKET_SAMPLES) as u32;
                block[offsets..offsets + 4].copy_from_slice(&first_sample.to_le_bytes());
                offsets += 4;
            }
        }
        for run in &runs {
            block.extend_from_slice(&run[first * PACKET..(first + here) * PACKET]);
        }
        if b + 1 < blocks {
            block.resize(block_size, 0);
        }
        data.extend_from_slice(&block);
    }

    let mut format = Vec::new();
    for word in [blocks, block_size, n] {
        format.extend_from_slice(&(word as u32).to_le_bytes());
    }
    for c in 0..n {
        format.extend_from_slice(&(0x0100_0000 + c as u32).to_le_bytes());
        format.extend_from_slice(&(samples as u32).to_le_bytes());
        format.extend_from_slice(&(-191i16).to_le_bytes());
        format.extend_from_slice(&rate.to_le_bytes());
        format.extend_from_slice(&[4, 0, 0, 0]);
    }
    let seek: Vec<u8> = starts.iter().flat_map(|s| s.to_le_bytes()).collect();

    let streams = 1 + n;
    let tables = 16 + streams * 4 + 3 * 8;
    let mut out = vec![0u8; tables];
    out[0..4].copy_from_slice(b"ADAT");
    out[4..6].copy_from_slice(&1u16.to_le_bytes());
    out[6..8].copy_from_slice(&4u16.to_le_bytes());
    out[8..12].copy_from_slice(&(streams as u32).to_le_bytes());
    out[12..16].copy_from_slice(&(tables as u32).to_le_bytes());
    out[16..20].copy_from_slice(&(3u32 << 29).to_le_bytes());
    for c in 0..n {
        out[20 + c * 4..24 + c * 4].copy_from_slice(&(0x0100_0000 + c as u32).to_le_bytes());
    }
    let mut entries = Vec::new();
    for (kind, payload) in [(0x48u8, format), (0x55, data), (0xA3, seek)] {
        while !out.len().is_multiple_of(16) {
            out.push(0xEE);
        }
        let raw = (out.len() as u64) | ((payload.len() as u64) << 28) | (u64::from(kind) << 56);
        entries.push(raw);
        out.extend_from_slice(&payload);
    }
    let at = 16 + streams * 4;
    for (i, raw) in entries.iter().enumerate() {
        out[at + i * 8..at + i * 8 + 8].copy_from_slice(&raw.to_le_bytes());
    }
    out
}

/// Two ADPCM channels, each a sine, the container's rate 24 kHz.
fn a_pair() -> Vec<u8> {
    adpcm_container(
        &[
            sine(440.0, 8000.0, 24000.0, 20_000),
            sine(330.0, 6000.0, 24000.0, 20_000),
        ],
        24000,
    )
}

/// ⚠ A two-channel WAV into a two-channel ADPCM stream rebuilds the blocks
/// every channel shares: each channel decodes back to its samples within
/// ADPCM's error and runs on across every block join, which a rebuild that
/// restarted a channel's encoder at a block would not.
#[test]
fn a_replaced_two_channel_adpcm_stream_saves_and_decodes_continuous() {
    let (dir, mut state, doc, _) = opened(&a_pair());
    let rows = state.awc_streams(doc).expect("the streams list");
    assert_eq!(
        (rows[0].channels, rows[0].codec, rows[0].samples),
        (2, Codec::RageAdpcm, 20_000)
    );

    // Longer than the original, so the block count changes, and a different
    // signal in each channel so a swap shows.
    let samples = 30_001;
    let sources = [
        (600.0, 7000.0, sine(600.0, 7000.0, 24000.0, samples)),
        (250.0, 9000.0, sine(250.0, 9000.0, 24000.0, samples)),
    ];
    let interleaved: Vec<u8> = (0..samples)
        .flat_map(|i| {
            sources
                .iter()
                .flat_map(|(_, _, pcm)| pcm[i * 2..i * 2 + 2].to_vec())
                .collect::<Vec<u8>>()
        })
        .collect();
    state
        .awc_replace(doc, rows[0].stream, &wav_from_pcm16(&interleaved, 24000, 2))
        .expect("a two-channel ADPCM stream is replaced");
    assert!(state.document(doc).expect("doc").dirty);

    let path = dir.path().join("saved.awc");
    assert!(state.save_active_document_as(path.clone()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    let saved = std::fs::read(&path).expect("the save wrote the file");
    let file = AwcFile::parse(&saved, AwcKey::default()).expect("the save parses");
    assert_eq!(file.unaccounted_bytes(), 0);
    let format = file.streams[0].stream_format().expect("streamformat");
    assert_eq!(
        format.block_count as usize,
        samples.div_ceil(2 * ADPCM_PACKET_SAMPLES)
    );
    assert!(format
        .channels
        .iter()
        .all(|c| c.samples as usize == samples));

    let channels = file
        .decode_channels(AwcKey::default(), or_io::awc::Accumulator::Clamped)
        .expect("the rebuilt container decodes");
    let at = |b: &[u8], i: usize| i32::from(i16::from_le_bytes([b[i * 2], b[i * 2 + 1]]));
    for (c, (hz, amplitude, source)) in sources.iter().enumerate() {
        let decoded = &channels[c];
        assert_eq!(decoded.len(), source.len(), "channel {c}");
        let bound = std::f64::consts::TAU * hz * amplitude / 24000.0 / 4.0;
        let worst = (16..samples)
            .map(|i| (at(source, i) - at(decoded, i)).abs())
            .max()
            .unwrap_or(0);
        assert!(
            f64::from(worst) <= bound,
            "channel {c} off by {worst}, over {bound:.0}"
        );
        // Every join of a channel's blocks moves by no more than the sine
        // itself does between two samples, plus the codec's error.
        let slope = std::f64::consts::TAU * hz * amplitude / 24000.0;
        for join in (1..format.block_count as usize).map(|b| b * 2 * ADPCM_PACKET_SAMPLES) {
            let step = f64::from((at(decoded, join) - at(decoded, join - 1)).abs());
            assert!(
                step <= slope + 2.0 * bound,
                "channel {c} jumps by {step} at {join}"
            );
        }
    }

    assert!(state.undo().is_some(), "the replace is not a step");
    assert_eq!(
        written(&state, doc),
        a_pair(),
        "undone, it writes other bytes"
    );
}

/// Two 2,048-byte XMA packets holding 20 frames of 512 silent samples each: a frame is 31
/// bits, its length, a single subframe that sends no coefficients and the flag that another
/// frame follows in its packet.
fn silent_xma() -> Vec<u8> {
    let mut data = Vec::new();
    for frames in [13usize, 7] {
        let mut bits: Vec<bool> = Vec::new();
        let mut put =
            |value: usize, n: usize| bits.extend((0..n).rev().map(|i| (value >> i) & 1 == 1));
        for (value, n) in [(frames, 6), (0, 15), (1, 3), (0, 8)] {
            put(value, n);
        }
        for i in 0..frames {
            for (value, n) in [
                (31, 15),
                (1, 1),
                (0, 1),
                (0, 8),
                (0, 5),
                (usize::from(i + 1 < frames), 1),
            ] {
                put(value, n);
            }
        }
        bits.resize(2048 * 8, true);
        data.extend(
            bits.chunks(8)
                .map(|c| c.iter().fold(0u8, |v, b| (v << 1) | u8::from(*b))),
        );
    }
    data
}

/// ⚠ An XMA stream, which Legacy's Vinewood casino pack ships eight of, plays and exports
/// through `or-media`, and is replaced nowhere: no encoder exists, so a replace is refused by
/// what the stream is and leaves it as it was.
#[test]
fn an_xma_stream_plays_and_exports_and_is_left_alone_by_a_replace() {
    let xma = container(&[Built {
        id: 0x0344_F3E1,
        codec: 5,
        rate: 31988,
        samples: 20 * 512,
        data: silent_xma(),
    }]);
    let (_dir, mut state, doc, heard) = opened(&xma);
    let rows = state.awc_streams(doc).expect("the streams list");
    assert_eq!(rows[0].codec.label(), "XMA");

    state.awc_play(doc, rows[0].stream).expect("XMA plays");
    assert_eq!(
        heard.lock().expect("the sink's record").last(),
        Some(&(31988, 1, vec![0; 20 * 512]))
    );
    let wav = state.awc_wav(doc, rows[0].stream).expect("XMA exports");
    assert_eq!(wav, wav_from_pcm16(&vec![0; 20 * 512 * 2], 31988, 1));

    let why = state
        .awc_replace(doc, rows[0].stream, &wav_from_pcm16(&ramp(10), 31988, 1))
        .expect_err("no encoder");
    assert!(why.contains("XMA"), "{why}");
    assert!(!state.document(doc).expect("doc").dirty);
}
