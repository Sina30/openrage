//! Bink audio built by hand, decoded by OpenRage and by FFmpeg, compared sample by sample.
//!
//! No Bink encoder exists, and every movie GTA V ships has stereo DCT sound, so this is the only
//! thing that holds the RDFT decoder, mono tracks and the 11,025 Hz block size to a reference. It
//! writes the bitstream from the format's documented layout: a packet is a length, then blocks of
//! per-channel floats, band step sizes and run-length-grouped coefficients, and nothing under test
//! wrote the bytes it is checked against. FFmpeg 7.0 is the oracle, so a machine without one
//! records a skip.

use std::io::Read;
use std::process::{Command, Stdio};

use or_media::bik;
use or_media::Decoders;
use or_test_support::skip_install;

const WHAT: &str = "hand-built Bink audio decodes as FFmpeg decodes it";

/// Run lengths a 4-bit code stands for, in groups of eight coefficients.
const RLE: [usize; 16] = [2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16, 32, 64];

/// A small deterministic generator: the streams are random, and the same every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self, below: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as u32) % below
    }
}

/// Bits written lowest first, as the format stores them.
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    at: usize,
}

impl Bits {
    fn put(&mut self, value: u32, n: u32) {
        for i in 0..n {
            if self.at.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> i) & 1 != 0 {
                *self.bytes.last_mut().expect("a byte") |= 1 << (self.at % 8);
            }
            self.at += 1;
        }
    }

    fn align32(&mut self) {
        while !self.at.is_multiple_of(32) {
            self.put(0, 1);
        }
    }

    /// A float as the format states it: a 5-bit power, a 23-bit mantissa and a sign.
    fn float(&mut self, rng: &mut Rng) {
        self.put(10 + rng.next(12), 5);
        self.put(rng.next(1 << 23), 23);
        self.put(rng.next(2), 1);
    }
}

/// What a case states about a track.
struct Case {
    name: &'static str,
    rate: u16,
    stereo: bool,
    dct: bool,
    /// Bands the coefficient step sizes are grouped in: how many the critical-band table leaves
    /// below half the rate (half of channels times the rate, for the RDFT's interleaved block).
    bands: usize,
    /// The transform size the format derives from the rate, doubled for a stereo RDFT block.
    frame_len: usize,
}

/// One block of random coefficients: one transform's worth, for every channel the block holds.
fn block(bits: &mut Bits, rng: &mut Rng, case: &Case) {
    let channels = if case.dct && case.stereo { 2 } else { 1 };
    if case.dct {
        bits.put(0, 2);
    }
    for _ in 0..channels {
        bits.float(rng);
        bits.float(rng);
        for _ in 0..case.bands {
            bits.put(rng.next(40), 8);
        }
        let mut i = 2;
        while i < case.frame_len {
            let span = if rng.next(2) == 0 {
                bits.put(0, 1);
                8
            } else {
                let code = rng.next(16);
                bits.put(1, 1);
                bits.put(code, 4);
                RLE[code as usize] * 8
            };
            let j = (i + span).min(case.frame_len);
            let width = rng.next(10);
            bits.put(width, 4);
            if width != 0 {
                for _ in i..j {
                    let coefficient = rng.next(1 << width);
                    bits.put(coefficient, width);
                    if coefficient != 0 {
                        bits.put(rng.next(2), 1);
                    }
                }
            }
            i = j;
        }
    }
    bits.align32();
}

/// A movie of `packets.len()` frames: a 64×48 picture nobody decodes, and a sound track.
fn movie(case: &Case, packets: &[Vec<u8>]) -> Vec<u8> {
    let tracks = 1usize;
    let table = 44 + 12 * tracks;
    let start = table + 4 * packets.len();
    let mut body = Vec::new();
    let mut offsets = Vec::new();
    for packet in packets {
        offsets.push((start + body.len()) as u32);
        body.extend((packet.len() as u32).to_le_bytes());
        body.extend_from_slice(packet);
        body.extend([0u8; 4]);
    }
    let mut file = vec![0u8; 44];
    file[..4].copy_from_slice(b"BIKi");
    let flags =
        0x4000u16 | if case.stereo { 0x2000 } else { 0 } | if case.dct { 0x1000 } else { 0 };
    for (at, value) in [
        (4, (start + body.len() - 8) as u32),
        (8, packets.len() as u32),
        (12, 64),
        (16, packets.len() as u32),
        (20, 64),
        (24, 48),
        (28, 30),
        (32, 1),
        (40, tracks as u32),
    ] {
        file[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    file.extend([0u8; 4]);
    file.extend(case.rate.to_le_bytes());
    file.extend(flags.to_le_bytes());
    file.extend([0u8; 4]);
    for offset in offsets {
        file.extend(offset.to_le_bytes());
    }
    file.extend(body);
    file
}

fn ffmpeg_samples(file: &std::path::Path) -> Option<Result<Vec<f32>, String>> {
    let child = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(file)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut child = child;
    let mut raw = Vec::new();
    child.stdout.take()?.read_to_end(&mut raw).ok()?;
    let status = child.wait().ok()?;
    if !status.success() {
        return Some(Err(format!("FFmpeg exited with {status}")));
    }
    Some(Ok(raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()))
}

#[test]
fn hand_built_bink_audio_decodes_as_ffmpeg_decodes_it() {
    let present = Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !present {
        skip_install(WHAT, "no FFmpeg on the PATH to decode the reference with");
        return;
    }
    // The bands are what FFmpeg's critical-band table leaves below half the rate: 11,025 Hz at
    // 22,050 is past 9,500 and under 12,000, the 23rd edge; 22,050 at 44,100 and everything above
    // reaches the 24,500 edge, the 25th; 5,513 at 11,025 is under 6,400, the 20th.
    let cases = [
        Case {
            name: "stereo DCT 44,100 Hz",
            rate: 44100,
            stereo: true,
            dct: true,
            bands: 25,
            frame_len: 2048,
        },
        Case {
            name: "mono DCT 22,050 Hz",
            rate: 22050,
            stereo: false,
            dct: true,
            bands: 23,
            frame_len: 1024,
        },
        Case {
            name: "stereo DCT 48,000 Hz",
            rate: 48000,
            stereo: true,
            dct: true,
            bands: 25,
            frame_len: 2048,
        },
        Case {
            name: "mono DCT 11,025 Hz",
            rate: 11025,
            stereo: false,
            dct: true,
            bands: 20,
            frame_len: 512,
        },
        Case {
            name: "mono RDFT 22,050 Hz",
            rate: 22050,
            stereo: false,
            dct: false,
            bands: 23,
            frame_len: 1024,
        },
        Case {
            name: "stereo RDFT 22,050 Hz",
            rate: 22050,
            stereo: true,
            dct: false,
            bands: 25,
            frame_len: 2048,
        },
        Case {
            name: "stereo RDFT 11,025 Hz",
            rate: 11025,
            stereo: true,
            dct: false,
            bands: 23,
            frame_len: 1024,
        },
    ];
    let dir = tempfile::tempdir().expect("a scratch directory");
    for (n, case) in cases.iter().enumerate() {
        let mut rng = Rng(7 + n as u64);
        // Three packets of one block each and a fourth of two, with the length a packet states.
        let packets: Vec<Vec<u8>> = [1, 1, 1, 2]
            .iter()
            .map(|&blocks| {
                let mut bits = Bits::default();
                bits.put(0, 32);
                for _ in 0..blocks {
                    block(&mut bits, &mut rng, case);
                }
                bits.bytes
            })
            .collect();
        let bytes = movie(case, &packets);
        let file = dir.path().join("movie.bik");
        std::fs::write(&file, &bytes).expect("the movie is written");

        let want = match ffmpeg_samples(&file) {
            Some(Ok(samples)) => samples,
            other => panic!("{}: FFmpeg gave no sound: {other:?}", case.name),
        };
        let mut stream = bik::audio_track(&bytes, 0, &Decoders::new()).expect("a sound track");
        let mut got = Vec::new();
        while let Some(block) = stream.read().expect("a block decodes") {
            got.extend(block.into_f32());
        }

        assert_eq!(got.len(), want.len(), "{}: samples", case.name);
        assert!(!got.is_empty(), "{}: no samples were compared", case.name);
        let peak = want.iter().fold(0.0f32, |p, s| p.max(s.abs()));
        let diff = got
            .iter()
            .zip(&want)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        eprintln!(
            "{}: {} samples, peak {peak}, largest difference {diff}",
            case.name,
            got.len()
        );
        assert!(peak > 0.0, "{}: FFmpeg decoded silence", case.name);
        assert!(
            diff <= 4.0e-5 * peak.max(1.0),
            "{}: differs from FFmpeg by {diff} on a peak of {peak}",
            case.name
        );
    }
}
