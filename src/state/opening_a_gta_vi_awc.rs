//! A GTA VI `.awc` opens as its Opus streams, listed and exported as Ogg Opus, and the view
//! draws a GTA V container's streams as before.
//!
//! ⚠ Every container here is built by hand, so nothing under test produced
//! the bytes it is checked against.

use std::path::PathBuf;

use or_io::awc::wav::wav_from_pcm16;
use or_media::Codec;

use super::opening_an_awc::{opened, ramp, three_streams, PACKET};
use super::{AppState, DocumentId};

/// `count` Opus packets of one 20 ms frame, each a TOC byte and nonzero bytes.
pub(super) fn opus_packets(toc: u8, count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| {
            let mut p = vec![toc];
            p.extend((0..40 + i % 30).map(|j| (j % 200 + 1) as u8));
            p
        })
        .collect()
}

/// Packets as `u16` lengths and bytes in 2,048-byte packets, and the count each holds.
fn opus_units(packets: &[Vec<u8>]) -> (Vec<u8>, Vec<u32>) {
    let (mut data, mut counts, mut at) = (vec![0u8; PACKET], vec![0u32], 0);
    for p in packets {
        if at + 2 + p.len() > PACKET {
            data.resize(data.len() + PACKET, 0);
            counts.push(0);
            at = 0;
        }
        let base = data.len() - PACKET + at;
        data[base..base + 2].copy_from_slice(&(p.len() as u16).to_le_bytes());
        data[base + 2..base + 2 + p.len()].copy_from_slice(p);
        at += 2 + p.len();
        *counts.last_mut().expect("a packet") += 1;
    }
    (data, counts)
}

fn le(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A stream's chunks: each a kind byte and a payload.
type Chunks = Vec<(u8, Vec<u8>)>;

/// The samples per channel a stream of `frames` Opus frames states.
fn per_channel(frames: usize) -> u32 {
    960 * frames as u32 - 2000
}

/// A single Opus stream's chunks: its format record, its data, and a seek table of each
/// packet's first sample and the last frame's.
pub(super) fn opus_single(packets: &[Vec<u8>], channels: u8, loop_point: i32) -> Chunks {
    let (data, counts) = opus_units(packets);
    let step = 960 * u32::from(channels);
    let samples = per_channel(packets.len());
    let mut record = vec![0u8; 32];
    record[0..4].copy_from_slice(&(samples * u32::from(channels)).to_le_bytes());
    record[4..8].copy_from_slice(&loop_point.to_le_bytes());
    record[8..10].copy_from_slice(&48_000u16.to_le_bytes());
    record[12..16].copy_from_slice(&samples.to_le_bytes());
    record[0x1C] = 11;
    record[0x1D] = channels;
    let mut seek = Vec::new();
    let mut before = 0;
    for n in counts {
        seek.push(step * before);
        before += n;
    }
    seek.push(step * (before - 1));
    vec![(0x55, data), (0x76, record), (0xA3, le(&seek))]
}

/// A container in GTA VI's layout: flag bits 0 and 5, a word when bit 6 is set, the first-chunk
/// index, `streams + 1` words, the stream and chunk tables, then payloads on 16-byte steps.
pub(super) fn gta_vi(flags: u16, streams: &[(u32, Chunks)]) -> Vec<u8> {
    let mut out = b"ADAT".to_vec();
    out.extend(1u16.to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(le(&[streams.len() as u32, 0]));
    if flags & 0x40 != 0 {
        out.extend(le(&[0x1230]));
    }
    let mut first = 0u16;
    for (_, chunks) in streams {
        out.extend(first.to_le_bytes());
        first += chunks.len() as u16;
    }
    out.extend(le(&vec![0x0BAD_F00D; streams.len() + 1]));
    for (id, chunks) in streams {
        out.extend(le(&[(chunks.len() as u32) << 29 | id]));
    }
    let table = out.len();
    out.resize(table + usize::from(first) * 8, 0);
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

/// A multi-channel container of two mono Opus channels in one block, each stating 9,600 samples.
fn gta_vi_multi(a: &[Vec<u8>], b: &[Vec<u8>]) -> Vec<u8> {
    let (runs, size) = ([opus_units(a), opus_units(b)], 4 * PACKET as u32);
    let mut data = Vec::new();
    for (run, _) in &runs {
        data.extend(le(&[u32::MAX, (run.len() / PACKET) as u32, 0, 0, 0, 0]));
    }
    for (_, counts) in &runs {
        let mut first = 0;
        for n in counts {
            data.extend(le(&[first]));
            first += 960 * n;
        }
    }
    data.resize(PACKET, 0);
    runs.iter().for_each(|(run, _)| data.extend(run));
    let mut format = le(&[1, size, 2]);
    for id in [0x0D01u32, 0x0D02] {
        format.extend(le(&[id, 960 * 10]));
        format.extend([0, 0, 0x80, 0xBB, 11, 1, 0xFF, 0xFF]);
    }
    gta_vi(
        0xFF25,
        &[
            (0, vec![(0x48, format), (0x55, data), (0xA3, le(&[0]))]),
            (0x0D01, vec![]),
            (0x0D02, vec![]),
        ],
    )
}

/// A looped mono and a stereo Opus stream in a flag-bit-0 table, and the packets of the mono one.
fn gta_vi_table() -> (Vec<u8>, Vec<Vec<u8>>) {
    let mono = opus_packets(0xF8, 100);
    let bytes = gta_vi(
        0xFF61,
        &[
            (0x0A01, opus_single(&mono, 1, 1000)),
            (0x0A02, opus_single(&opus_packets(0xFC, 60), 2, -1)),
        ],
    );
    (bytes, mono)
}

/// The packets of an Ogg file, read page by page from each page's lacing values, once each page
/// is checked to start where the one before it ends, in sequence, under `serial`.
fn ogg_packets(file: &[u8], serial: u32) -> Vec<Vec<u8>> {
    let (mut packets, mut packet, mut at, mut sequence) = (Vec::new(), Vec::new(), 0, 0u32);
    while at < file.len() {
        assert_eq!(&file[at..at + 4], b"OggS", "a page at {at}");
        assert_eq!(file[at + 14..at + 18], serial.to_le_bytes());
        assert_eq!(file[at + 18..at + 22], sequence.to_le_bytes());
        let lacing = &file[at + 27..at + 27 + usize::from(file[at + 26])];
        let mut body = at + 27 + lacing.len();
        for &n in lacing {
            packet.extend_from_slice(&file[body..body + usize::from(n)]);
            body += usize::from(n);
            if n < 255 {
                packets.push(std::mem::take(&mut packet));
            }
        }
        at = body;
        sequence += 1;
    }
    assert!(packet.is_empty(), "a packet runs past the last page");
    packets
}

/// Every text one frame of `doc`'s view painted with `events`, and the centre of each.
fn frame(
    ctx: &egui::Context,
    state: &mut AppState,
    doc: DocumentId,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.text().to_string(),
                t.pos + t.galley.rect.center().to_vec2(),
            )),
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let draw = (state.document(doc))
        .and_then(|d| super::capabilities::for_payload(&d.payload))
        .and_then(|row| row.inspector)
        .expect("an .awc has a view");
    let input = egui::RawInput {
        events,
        ..Default::default()
    };
    let out = ctx.run_ui(input, |ui| draw(ui, state, doc, None));
    let mut texts = Vec::new();
    out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
    texts
}

/// The texts of `doc`'s view once its grid has measured itself.
fn drawn(state: &mut AppState, doc: DocumentId) -> Vec<String> {
    let ctx = egui::Context::default();
    frame(&ctx, state, doc, Vec::new());
    let texts = frame(&ctx, state, doc, Vec::new());
    texts.into_iter().map(|(text, _)| text).collect()
}

/// Clicks the first button whose label holds `label`, the dialog it asks answered with `pick`.
fn click(state: &mut AppState, doc: DocumentId, label: &str, pick: PathBuf) {
    let ctx = egui::Context::default();
    frame(&ctx, state, doc, Vec::new());
    let (_, at) = (frame(&ctx, state, doc, Vec::new()).into_iter())
        .find(|(text, _)| text.contains(label))
        .unwrap_or_else(|| panic!("no {label} button"));
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    crate::platform::file_dialogs::answer_next(pick);
    frame(
        &ctx,
        state,
        doc,
        vec![egui::Event::PointerMoved(at), press(true)],
    );
    frame(&ctx, state, doc, vec![press(false)]);
}

#[test]
fn a_gta_vi_container_draws_its_opus_streams_and_plays_them() {
    let (_dir, mut state, doc, heard) = opened(&gta_vi_table().0);
    let rows = state.awc_streams(doc).expect("the streams list");
    let listed: Vec<_> = (rows.iter())
        .map(|r| {
            (
                r.id,
                r.codec,
                r.channels,
                r.sample_rate,
                r.samples,
                r.loop_point,
            )
        })
        .collect();
    assert_eq!(
        listed,
        vec![
            (0x0A01, Codec::Opus, 1, 48_000, per_channel(100), Some(1000)),
            (0x0A02, Codec::Opus, 2, 48_000, per_channel(60), None),
        ]
    );
    let texts = drawn(&mut state, doc);
    let seconds = |samples: u32| format!("{:.2}", f64::from(samples) / 48_000.0);
    let (mono, stereo) = (per_channel(100), per_channel(60));
    for want in [
        "0x00000A01".to_string(),
        "Opus".to_string(),
        "48000 Hz".to_string(),
        "2".to_string(),
        format!(
            "{} s, {mono} samples, loop point 1000 (a role inferred from its values)",
            seconds(mono)
        ),
        format!("{} s, {stereo} samples", seconds(stereo)),
    ] {
        assert!(texts.contains(&want), "{want} is not drawn: {texts:?}");
    }
    let buttons = |label: &str| texts.iter().filter(|t| t.contains(label)).count();
    assert_eq!(
        (
            buttons("Play"),
            buttons("Export WAV"),
            buttons("Export Ogg Opus")
        ),
        (2, 2, 2)
    );
    assert_eq!(
        buttons("Replace"),
        2,
        "a single Opus stream is offered a replace"
    );

    for row in &rows {
        let samples = row.samples as usize * usize::from(row.channels);
        state
            .awc_play(doc, row.stream)
            .expect("an Opus stream plays");
        let played: Vec<_> = (heard.lock().expect("the sink's record").iter())
            .map(|(rate, channels, pcm)| (*rate, *channels, pcm.len()))
            .collect();
        assert_eq!(played, [(48_000, row.channels, samples)]);
        let wav = state
            .awc_wav(doc, row.stream)
            .expect("an Opus stream exports");
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 2 * samples);
    }
    assert_eq!(state.audio.playing(), Some((doc, 1)));

    let pcm = ramp(30_011);
    state
        .awc_replace(doc, 0, &wav_from_pcm16(&pcm, 48_000, 1))
        .expect("a mono stream is replaced from a mono WAV");
    let replaced = state.awc_streams(doc).expect("the streams list");
    assert_eq!(
        (replaced[0].samples, replaced[0].loop_point),
        (30_011, Some(1000))
    );
    let wav = state.awc_wav(doc, 0).expect("the new stream exports");
    assert_eq!(wav.len(), 44 + 2 * 30_011);
    let stereo = state.awc_replace(doc, 1, &wav_from_pcm16(&pcm, 48_000, 1));
    assert!(stereo.expect_err("refused").contains("save it with 2"));
    let rate = state.awc_replace(doc, 0, &wav_from_pcm16(&pcm, 44_100, 1));
    assert!(rate.expect_err("refused").contains("resamples nothing"));
    let file = state.awc_file(doc).expect("the container");
    assert!(file.to_bytes(Default::default()).is_ok());

    let (a, b) = (opus_packets(0xF8, 30), opus_packets(0xF8, 20));
    let (_dir, mut state, doc, heard) = opened(&gta_vi_multi(&a, &b));
    let rows = state.awc_streams(doc).expect("the streams list");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].codec, rows[0].channels, rows[0].samples),
        (Codec::Opus, 2, 9600)
    );
    assert_eq!(
        rows[0].opus_channels,
        vec![(0x0D01, 1, 9600), (0x0D02, 1, 9600)]
    );
    let texts = drawn(&mut state, doc);
    for want in ["  0x00000D01", "  0x00000D02", "0.20 s, 9600 samples"] {
        assert!(
            texts.iter().any(|t| t == want),
            "{want} is not drawn: {texts:?}"
        );
    }
    state
        .awc_play(doc, 0)
        .expect("a multi-channel Opus source plays");
    let played: Vec<_> = (heard.lock().expect("the sink's record").iter())
        .map(|(rate, channels, pcm)| (*rate, *channels, pcm.len()))
        .collect();
    assert_eq!(played, [(48_000, 2, 2 * 9600)]);
    // A multi-channel Opus source is replaced block by block, so its row offers Replace.
    assert!(texts.iter().any(|t| t.contains("Replace")), "{texts:?}");
}

#[test]
fn an_opus_stream_exports_as_ogg_pages_holding_the_packets_it_stores() {
    let (table, mono) = gta_vi_table();
    let (dir, mut state, doc, _) = opened(&table);
    let out = dir.path().join("out");
    std::fs::create_dir(&out).expect("a folder to export to");
    click(&mut state, doc, "Export Ogg Opus", out.clone());
    let file = std::fs::read(out.join("00000A01.opus")).expect("the export wrote the file");
    let packets = ogg_packets(&file, 0x0A01);
    assert!(packets[0].starts_with(b"OpusHead"), "{:?}", packets[0]);
    assert_eq!(packets[0][9], 1, "the channel count");
    assert!(packets[1].starts_with(b"OpusTags"));
    assert_eq!(packets[2..], mono[..]);
    let assumed = format!(
        "with a pre-skip of {} samples that is inferred, not stated by the file",
        or_io::awc::ogg::PRE_SKIP
    );
    assert!(
        (state.toasts.iter()).any(|t| t.message.contains("Wrote 1 Ogg Opus file to")
            && t.message.contains(&assumed)),
        "no toast said what was written and what it assumed"
    );

    let (a, b) = (opus_packets(0xF8, 30), opus_packets(0xF8, 20));
    let (_dir, mut state, doc, _) = opened(&gta_vi_multi(&a, &b));
    let files = state.awc_ogg_opus(doc, 0).expect("each channel exports");
    let names: Vec<_> = files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["00000D01.opus", "00000D02.opus"]);
    for ((_, file), (id, want)) in files.iter().zip([(0x0D01, &a), (0x0D02, &b)]) {
        assert_eq!(ogg_packets(file, id)[2..], want[..]);
    }
}

#[test]
fn a_container_of_many_streams_draws_only_the_rows_in_view() {
    let packets = opus_packets(0xF8, 3);
    let streams: Vec<_> = (1..=2000)
        .map(|id| (id, opus_single(&packets, 1, -1)))
        .collect();
    let (_dir, mut state, doc, _) = opened(&gta_vi(0xFF61, &streams));
    assert_eq!(
        state.awc_streams(doc).expect("the streams list").len(),
        2000
    );
    let draw = (state.document(doc))
        .and_then(|d| super::capabilities::for_payload(&d.payload))
        .and_then(|row| row.inspector)
        .expect("an .awc has a view");
    // A widget out of view lays out but paints nothing, so the height laid out is the measure.
    let ctx = egui::Context::default();
    let (mut laid_out, mut screen) = (0.0, 0.0);
    for _ in 0..2 {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw(ui, &mut state, doc, None);
            (laid_out, screen) = (ui.min_rect().height(), ui.ctx().content_rect().height());
        });
    }
    assert!(
        laid_out < 2.0 * screen,
        "{laid_out} px laid out on a screen of {screen}"
    );
    assert!(drawn(&mut state, doc).contains(&"0x00000001".to_string()));
}

#[test]
fn streams_whose_opus_record_is_refused_are_counted_with_the_first_reason() {
    let refused = || {
        let mut chunks = opus_single(&opus_packets(0xF8, 10), 1, -1);
        chunks[1].1 = vec![0; 32];
        chunks
    };
    let bytes = gta_vi(
        0xFF61,
        &[
            (0x0A01, opus_single(&opus_packets(0xF8, 10), 1, -1)),
            (0x0A02, refused()),
            (0x0A03, refused()),
        ],
    );
    let (_dir, mut state, doc, _) = opened(&bytes);
    assert_eq!(state.awc_streams(doc).expect("the streams list").len(), 1);
    let why = (state.awc_file(doc).expect("the container").streams[1].opus_format())
        .expect_err("an all-zero record is refused");
    let want =
        format!("2 streams are not listed, because OpenRage refuses their Opus. The first: {why}");
    let texts = drawn(&mut state, doc);
    assert!(texts.contains(&want), "{want} is not drawn: {texts:?}");

    let (_dir, mut state, doc, _) = opened(&three_streams());
    let texts = drawn(&mut state, doc);
    assert!(!texts.iter().any(|t| t.contains("not listed")), "{texts:?}");
}

#[test]
fn a_gta_v_container_still_draws_its_streams_and_exports_one_from_its_view() {
    let (dir, mut state, doc, _) = opened(&three_streams());
    let texts = drawn(&mut state, doc);
    for want in [
        "PCM16", "ADPCM", "MP3", "32000 Hz", "24000 Hz", "0.00 s", "0.04 s",
    ] {
        assert!(
            texts.iter().any(|t| t == want),
            "{want} is not drawn: {texts:?}"
        );
    }
    let buttons = |label: &str| texts.iter().filter(|t| t.contains(label)).count();
    assert_eq!(
        (buttons("Play"), buttons("Export WAV"), buttons("Replace")),
        (3, 3, 3)
    );
    assert_eq!(buttons("Export Ogg Opus") + buttons("samples"), 0);

    let path = dir.path().join("pcm.wav");
    click(&mut state, doc, "Export WAV", path.clone());
    let wav = std::fs::read(&path).expect("the export wrote the file");
    assert_eq!(wav, wav_from_pcm16(&ramp(100), 32000, 1));
}
