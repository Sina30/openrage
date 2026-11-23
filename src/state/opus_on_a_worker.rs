//! A GTA VI stream's Opus plays, exports as WAV and is replaced on a worker the job list shows and
//! stops, and a scanned container's replaced stream is kept by Save As.
//!
//! ⚠ Every container here is built by hand, so nothing under test produced
//! the bytes it is checked against.

use std::time::{Duration, Instant};

use or_io::awc::wav::wav_from_pcm16;
use or_io::awc::{AwcFile, AwcKey};

use super::jobs::{JobKind, Outcome};
use super::opening_a_gta_vi_awc::{gta_vi, opus_packets, opus_single};
use super::opening_an_awc::{opened, ramp};
use super::AppState;

/// Waits out the Opus job, then polls the jobs once, which says how it ended.
fn worked(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while (state.running_jobs().iter()).any(|job| job.kind == JobKind::OpusAudio) {
        assert!(Instant::now() < deadline, "the Opus job never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
    state.poll_jobs();
}

/// The newest ended Opus job's outcome and what it said.
fn last(state: &AppState) -> (Outcome, String) {
    let ended = (state.jobs.ended().iter()).find(|job| job.kind == JobKind::OpusAudio);
    let ended = ended.expect("the Opus job is in the log");
    (ended.outcome.clone(), ended.said.clone())
}

#[test]
fn an_opus_stream_plays_exports_and_is_replaced_on_a_worker_the_list_stops() {
    let mono = opus_packets(0xF8, 100);
    let bytes = gta_vi(0xFF61, &[(0x0A01, opus_single(&mono, 1, -1))]);
    let (dir, mut state, doc, heard) = opened(&bytes);
    let samples = state.awc_streams(doc).expect("the streams list")[0].samples as usize;
    let want = state.awc_wav(doc, 0).expect("the stream decodes");

    state
        .awc_opus_decode(doc, 0, None)
        .expect("the play starts");
    worked(&mut state);
    let played: Vec<_> = (heard.lock().expect("the sink's record").iter())
        .map(|(rate, channels, pcm)| (*rate, *channels, pcm.len()))
        .collect();
    assert_eq!(played, [(48_000, 1, samples)]);
    assert_eq!(state.audio.playing(), Some((doc, 0)));

    let path = dir.path().join("decoded.wav");
    state
        .awc_opus_decode(doc, 0, Some(path.clone()))
        .expect("the export starts");
    worked(&mut state);
    assert_eq!(std::fs::read(&path).expect("the worker wrote it"), want);
    assert!(
        matches!(last(&state).0, Outcome::Done(_)),
        "{:?}",
        last(&state)
    );

    let wav = dir.path().join("new.wav");
    std::fs::write(&wav, wav_from_pcm16(&ramp(30_011), 48_000, 1)).expect("a WAV");
    state
        .awc_opus_replace(doc, 0, wav.clone())
        .expect("the replace starts");
    worked(&mut state);
    assert_eq!(state.awc_streams(doc).expect("listed")[0].samples, 30_011);
    assert!(
        state.document(doc).is_some_and(|d| d.dirty),
        "the replace is an edit"
    );

    std::fs::write(&wav, wav_from_pcm16(&ramp(100), 44_100, 1)).expect("a WAV");
    state
        .awc_opus_replace(doc, 0, wav)
        .expect("the replace starts");
    worked(&mut state);
    let (outcome, said) = last(&state);
    assert_eq!(outcome, Outcome::Failed);
    assert!(said.contains("resamples nothing"), "{said}");
    assert_eq!(state.awc_streams(doc).expect("listed")[0].samples, 30_011);

    // A long stream, stopped from the list while it decodes, plays nothing and says it stopped.
    let long = gta_vi(
        0xFF61,
        &[(0x0A02, opus_single(&opus_packets(0xF8, 5000), 1, -1))],
    );
    let (_dir, mut state, doc, heard) = opened(&long);
    state
        .awc_opus_decode(doc, 0, None)
        .expect("the play starts");
    let listed = state.running_jobs();
    let job = (listed.iter()).find(|job| job.kind == JobKind::OpusAudio);
    assert!(job.is_some_and(|job| job.fraction.is_some()), "{listed:?}");
    state.cancel_job(JobKind::OpusAudio);
    worked(&mut state);
    assert!(
        matches!(last(&state).0, Outcome::Stopped(_)),
        "{:?}",
        last(&state)
    );
    assert!(heard.lock().expect("the sink's record").is_empty());
    assert_eq!(state.audio.playing(), None);
}

#[test]
fn a_scanned_container_s_replaced_stream_is_saved_by_save_as_as_the_container_rewritten() {
    use super::opening_an_rpf9_archive::{row_at, rows, rpf9_head, scanned, settle, written};

    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut bytes = rpf9_head(1, 0x1A0);
    bytes.resize(0x1000, 0);
    bytes.extend(gta_vi(
        0xFF21,
        &[(0x0B01, opus_single(&opus_packets(0xB8, 20), 1, -1))],
    ));
    let archive = written(dir.path(), &bytes);
    let mut state = AppState::new();
    let scan = scanned(&mut state, archive.clone());
    state.open_scanned_body(scan, row_at(&rows(&state, scan), 0x1000));
    settle(&mut state);
    let audio = (state.active_document)
        .filter(|&d| d != scan)
        .expect("the audio container opened");
    assert!(state.document(audio).is_some_and(|d| d.read_only.is_some()));

    let wav = dir.path().join("new.wav");
    std::fs::write(&wav, wav_from_pcm16(&ramp(30_011), 48_000, 1)).expect("a WAV");
    state
        .awc_opus_replace(audio, 0, wav)
        .expect("a scanned container takes a replace");
    worked(&mut state);
    assert_eq!(state.awc_streams(audio).expect("listed")[0].samples, 30_011);

    state.set_active_document(audio);
    let saved = dir.path().join("replaced.awc");
    assert!(state.save_active_document_as(saved.clone()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.is_running() {
        state.poll_extract_job();
        assert!(Instant::now() < deadline, "the save never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
    let file = std::fs::read(&saved).expect("Save As wrote the container");
    let held = state.awc_file(audio).expect("the container");
    assert_eq!(file, held.to_bytes(AwcKey::default()).expect("it writes"));
    let read = AwcFile::parse(&file, AwcKey::default()).expect("it opens again");
    let format = read.streams[0]
        .opus_format()
        .expect("read")
        .expect("present");
    assert_eq!(format.samples_per_channel, 30_011);

    // Save still writes nothing into the archive.
    assert!(!state.save_active_document());
    assert_eq!(std::fs::read(&archive).expect("the archive"), bytes);
}
