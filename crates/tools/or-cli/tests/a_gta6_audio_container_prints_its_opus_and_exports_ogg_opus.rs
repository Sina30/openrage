//! `openrage-cli awc info`, `awc export` and `rpf scan-open` over GTA VI audio containers built
//! here: a flag-bit-0 table of a looped mono and a stereo Opus stream, and a multi-channel one.
//!
//! ⚠ The commands print with `println!`, which a test cannot read back in its own process, so
//! each runs in a child of this test binary that calls `or_cli::run` between two markers.

use std::io::Write;
use std::process::{Command, ExitCode};

const CHILD_ENV: &str = "OPENRAGE_GTA6_AUDIO_ARGS";
const CHILD_TEST: &str = "run_openrage_cli_for_a_parent";
const SEPARATOR: char = '\u{1f}';
const BEGIN: &str = "\u{1e}stdout begins\u{1e}";
const END: &str = "\u{1e}stdout ends\u{1e}";

/// Not a test of its own: the body a parent's child process runs.
#[test]
fn run_openrage_cli_for_a_parent() {
    let Some(args) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let args: Vec<String> = args
        .to_string_lossy()
        .split(SEPARATOR)
        .map(str::to_string)
        .collect();
    let mut out = std::io::stdout();
    let _ = write!(out, "{BEGIN}");
    let _ = out.flush();
    let code = or_cli::run(&args);
    let _ = out.flush();
    let _ = write!(out, "{END}{}", u8::from(code == ExitCode::SUCCESS));
    let _ = out.flush();
}

/// What `openrage-cli <args>` prints, and whether it succeeded.
fn run(args: &[&str]) -> (String, bool) {
    let output = Command::new(std::env::current_exe().expect("this test binary"))
        .args([CHILD_TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, args.join(&SEPARATOR.to_string()))
        .env("OPENRAGE_SCHEMA", "none")
        .output()
        .expect("this test binary runs as a child");
    let text = String::from_utf8_lossy(&output.stdout);
    let (_, rest) = text
        .split_once(BEGIN)
        .unwrap_or_else(|| panic!("the child printed no start marker: {text}"));
    let (printed, tail) = rest.split_once(END).unwrap_or_else(|| {
        panic!(
            "the command panicked: {rest}{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (printed.to_string(), tail.starts_with('1'))
}

/// `count` Opus packets of one 20 ms frame, each a TOC byte and nonzero bytes.
fn opus_packets(toc: u8, count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| {
            let mut p = vec![toc];
            p.extend((0..40 + i % 30).map(|j| (j % 200 + 1) as u8));
            p
        })
        .collect()
}

/// Packets as `u16` lengths and bytes in 2,048-byte packets, and the count each holds.
fn units(packets: &[Vec<u8>]) -> (Vec<u8>, Vec<u32>) {
    let (mut data, mut counts, mut at) = (vec![0u8; 2048], vec![0u32], 0);
    for p in packets {
        if at + 2 + p.len() > 2048 {
            data.resize(data.len() + 2048, 0);
            counts.push(0);
            at = 0;
        }
        let base = data.len() - 2048 + at;
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

/// A single stream's chunks: format record, data, and a seek table of each packet's first
/// sample and the last frame's.
fn single(packets: &[Vec<u8>], channels: u8, loop_point: i32) -> Vec<(u8, Vec<u8>)> {
    let (data, counts) = units(packets);
    let step = 960 * u32::from(channels);
    let frames = packets.len() as u32;
    let per_channel = 960 * frames - 2000;
    let mut record = vec![0u8; 32];
    record[0..4].copy_from_slice(&(per_channel * u32::from(channels)).to_le_bytes());
    record[4..8].copy_from_slice(&loop_point.to_le_bytes());
    record[8..10].copy_from_slice(&48_000u16.to_le_bytes());
    record[12..16].copy_from_slice(&per_channel.to_le_bytes());
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

/// A stream's chunks: each a kind byte and a payload.
type Chunks = Vec<(u8, Vec<u8>)>;

/// A container in GTA VI's layout: flag bits 0 and 5, a word when bit 6 is set, the first-chunk
/// index, `streams + 1` words, the stream and chunk tables, then payloads on 16-byte steps.
fn container(flags: u16, streams: &[(u32, Chunks)]) -> Vec<u8> {
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

/// One block of two mono channels, no repeat, then the streams of a multi-channel container.
fn multi_channel(a: &[Vec<u8>], b: &[Vec<u8>]) -> Vec<u8> {
    let (runs, size) = ([units(a), units(b)], 4 * 2048u32);
    let mut data = Vec::new();
    for (run, _) in &runs {
        data.extend(le(&[u32::MAX, (run.len() / 2048) as u32, 0, 0, 0, 0]));
    }
    for (_, counts) in &runs {
        let mut first = 0;
        for n in counts {
            data.extend(le(&[first]));
            first += 960 * n;
        }
    }
    data.resize(2048, 0);
    runs.iter().for_each(|(run, _)| data.extend(run));
    let mut format = le(&[1, size, 2]);
    for id in [0x0D01u32, 0x0D02] {
        format.extend(le(&[id, 960 * 10]));
        format.extend([0, 0, 0x80, 0xBB, 11, 1, 0xFF, 0xFF]);
    }
    container(
        0xFF25,
        &[
            (0, vec![(0x48, format), (0x55, data), (0xA3, le(&[0]))]),
            (0x0D01, vec![]),
            (0x0D02, vec![]),
        ],
    )
}

/// The channel count of an Ogg Opus file's `OpusHead`, once each page's head is where it says.
fn ogg_channels(file: &[u8]) -> u8 {
    let mut at = 0;
    let mut pages = 0;
    while at < file.len() {
        assert_eq!(&file[at..at + 4], b"OggS");
        let n = usize::from(file[at + 26]);
        let body: usize = file[at + 27..at + 27 + n]
            .iter()
            .map(|l| usize::from(*l))
            .sum();
        at += 27 + n + body;
        pages += 1;
    }
    assert_eq!(at, file.len());
    assert!(pages >= 3, "{pages} pages");
    let head = 27 + usize::from(file[26]);
    assert_eq!(&file[head..head + 8], b"OpusHead");
    file[head + 9]
}

#[test]
fn info_prints_the_opus_facts_and_export_writes_one_ogg_opus_file_per_stream() {
    let dir = tempfile::tempdir().expect("a folder");
    let mono = opus_packets(0xF8, 100);
    let stereo = opus_packets(0xFC, 60);
    let table = container(
        0xFF61,
        &[
            (0x0A01, single(&mono, 1, 1000)),
            (0x0A02, single(&stereo, 2, -1)),
        ],
    );
    let file = dir.path().join("table.awc");
    std::fs::write(&file, &table).expect("written");
    let file = file.display().to_string();

    let (info, ok) = run(&["awc", "info", &file]);
    assert!(ok, "{info}");
    assert!(info.contains("2 stream(s)"), "{info}");
    let per_channel = [960 * 100 - 2000, 960 * 60 - 2000];
    for (id, samples, channels, looped) in [
        (
            "00000A01",
            per_channel[0],
            1,
            "loop point 1000 (a role inferred from its values)",
        ),
        ("00000A02", per_channel[1], 2, ""),
    ] {
        let line = info
            .lines()
            .find(|l| l.trim_start().starts_with(id))
            .unwrap_or_else(|| panic!("{id}: {info}"));
        let seconds = format!("{:.3} s", f64::from(samples) / 48_000.0);
        for want in [
            &format!("{samples} smp"),
            "48000 Hz",
            "Opus",
            &format!("{channels} ch"),
            &seconds,
            looped,
        ] {
            assert!(line.contains(want), "{want}: {line}");
        }
        assert_eq!(line.contains("loop point"), !looped.is_empty(), "{line}");
    }

    let out = dir.path().join("out");
    let out_dir = out.display().to_string();
    let (printed, ok) = run(&["awc", "export", &file, &out_dir]);
    assert!(ok, "{printed}");
    assert!(printed.contains("wrote 2 file(s)"), "{printed}");
    let read = |name: &str| std::fs::read(out.join(name)).expect(name);
    assert_eq!(ogg_channels(&read("00000A01.opus")), 1);
    assert_eq!(ogg_channels(&read("00000A02.opus")), 2);

    let multi = dir.path().join("multi.awc");
    std::fs::write(&multi, multi_channel(&mono[..30], &mono[30..50])).expect("written");
    let multi = multi.display().to_string();
    let (info, ok) = run(&["awc", "info", &multi]);
    assert!(ok, "{info}");
    assert!(info.contains("streamformat: 2 channel(s)"), "{info}");
    assert_eq!(
        info.matches("     9600 smp  48000 Hz  Opus     1 ch")
            .count(),
        2,
        "{info}"
    );
    let (printed, ok) = run(&["awc", "export", &multi, &out_dir]);
    assert!(ok, "{printed}");
    assert!(printed.contains("wrote 2 file(s)"), "{printed}");
    assert_eq!(ogg_channels(&read("00000D01.opus")), 1);
    assert_eq!(ogg_channels(&read("00000D02.opus")), 1);
}

#[test]
fn scan_open_prints_a_gta6_table_as_awc_info_does() {
    // A head of layout 4 that holds every measured rule, its table to 0x1A0.
    let mut archive = vec![0u8; 0x1A0];
    archive[..4].copy_from_slice(b"9FPR");
    for (at, word) in [
        (0x04, 0x1A0u32),
        (0x08, 0x190),
        (0x0C, 0x40),
        (0x1C, 0x150),
        (0x24, 1),
        (0x30, 0x10),
    ] {
        archive[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    (archive[0x19], archive[0x1A], archive[0x20]) = (0x7A, 4, 5);
    (archive[0x2D], archive[0x2E]) = (0x0F, 0x09);
    archive[0x22] = 1;
    archive[0x2C] = 1;
    archive[0x40..].fill(0x5A);
    archive.resize(0x1000, 0);
    archive.extend(container(
        0xFF21,
        &[(0x0B01, single(&opus_packets(0xB8, 20), 1, -1))],
    ));
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("audio.rpf");
    std::fs::write(&path, &archive).expect("written");
    let path = path.display().to_string();
    let (scan, ok) = run(&["rpf", "scan", &path]);
    assert!(ok, "{scan}");
    assert!(
        scan.contains("flags 0xFF21, 1 stream(s), 3 chunk(s)"),
        "{scan}"
    );
    let (open, ok) = run(&["rpf", "scan-open", &path, "0x1000"]);
    assert!(ok, "{open}");
    assert!(
        open.contains("17200 smp  48000 Hz  Opus     1 ch"),
        "{open}"
    );
    assert!(open.contains("the streams are Opus"), "{open}");
}
