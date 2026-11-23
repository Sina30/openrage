//! Legacy's eight XMA streams decode through the media interface as FFmpeg's `xma2` decoder
//! decodes them.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names Legacy's install, whose executable supplies the
//! archive and AWC keys, and the suite skips where FFmpeg with its `xma2` decoder is not on the
//! PATH.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use or_io::awc::{AwcFile, AwcKey};
use or_io::container::rpf::crypto::{extract_keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::RpfArchive;
use or_media::{awc, AudioInfo, Codec, Decoders};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

const CHIPS: &str = "vw_counterfeit_chips.awc";
const CHIPS_PACK: &str = "update/x64/dlcpacks/mpvinewood/dlc.rpf";

fn find_in(archive: &Arc<RpfArchive>, depth: u32) -> Option<Vec<u8>> {
    for node in archive.nodes.iter().filter(|n| !n.is_dir()) {
        let name = node.name().to_ascii_lowercase();
        let i = node.entry_index();
        if name == CHIPS {
            return archive.read_entry(i).ok();
        }
        if name.ends_with(".rpf") && depth < 4 {
            if let Ok(nested) = archive.open_nested(i) {
                if let Some(found) = find_in(&Arc::new(nested), depth + 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// The container holding the eight XMA streams, from a Legacy install.
fn chips_container() -> Option<(Vec<u8>, AwcKey)> {
    for root in game_roots() {
        if !root.join("GTA5.exe").is_file() || root.join("GTA5_Enhanced.exe").is_file() {
            continue;
        }
        let keys = Arc::new(extract_keys(root.join("GTA5.exe"), &|_| {}).ok()?);
        let pack = root.join(CHIPS_PACK);
        let provider = Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&keys)));
        let archive = RpfArchive::open_with(pack, provider).ok()?;
        if let Some(bytes) = find_in(&Arc::new(archive), 0) {
            return Some((bytes, AwcKey(keys.awc_key)));
        }
    }
    None
}

fn le16(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// A RIFF `WAVE` holding XMA2 packets: the `XMA2WAVEFORMATEX` form FFmpeg's demuxer reads.
fn xma2_riff(data: &[u8], channels: u16, rate: u32, samples: u32) -> Vec<u8> {
    let blocks = (data.len() / 2048) as u16;
    let mut fmt = Vec::new();
    fmt.extend(le16(0x0166));
    fmt.extend(le16(channels));
    fmt.extend(le32(rate));
    fmt.extend(le32(rate * 2 * u32::from(channels)));
    fmt.extend(le16(2048));
    fmt.extend(le16(16));
    fmt.extend(le16(34));
    // The 34 bytes of `XMA2WAVEFORMATEX`: streams, channel mask, samples, block bytes, play and
    // loop ranges, loop count, encoder version, block count.
    fmt.extend(le16(1));
    fmt.extend(le32(0));
    fmt.extend(le32(samples));
    fmt.extend(le32(2048));
    fmt.extend(le32(0));
    fmt.extend(le32(samples));
    fmt.extend(le32(0));
    fmt.extend(le32(0));
    fmt.push(0);
    fmt.push(4);
    fmt.extend(le16(blocks));
    let mut body = Vec::new();
    body.extend(b"WAVE");
    body.extend(b"fmt ");
    body.extend(le32(fmt.len() as u32));
    body.extend(&fmt);
    body.extend(b"data");
    body.extend(le32(data.len() as u32));
    body.extend(data);
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend(le32(body.len() as u32));
    out.extend(body);
    out
}

fn ffmpeg_has_xma2() -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-decoders"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(" xma2 "))
        .unwrap_or(false)
}

/// What FFmpeg's `xma2` decoder makes of the packets, as `f32` samples, and what it said.
fn ffmpeg_decode(dir: &Path, name: &str, riff: &[u8]) -> (Vec<f32>, String) {
    let input: PathBuf = dir.join(format!("{name}.wav"));
    let output: PathBuf = dir.join(format!("{name}.f32"));
    std::fs::write(&input, riff).expect("write the RIFF");
    let run = Command::new("ffmpeg")
        .args(["-v", "warning", "-y", "-i"])
        .arg(&input)
        .args(["-f", "f32le", "-c:a", "pcm_f32le"])
        .arg(&output)
        .output()
        .expect("run ffmpeg");
    let said = String::from_utf8_lossy(&run.stderr).to_string();
    let bytes = std::fs::read(&output).unwrap_or_default();
    let samples = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    (samples, said)
}

/// One XMA stream of the container: its id, its info and its stored packets.
struct Shipped {
    id: u32,
    rate: u32,
    frames: u64,
    packets: Vec<u8>,
    /// The `format` chunk's peak and the `peak` chunk's windows after it.
    stated_peaks: Vec<u16>,
}

/// The container's XMA streams.
fn shipped() -> Option<Vec<Shipped>> {
    let (bytes, key) = chips_container()?;
    let file = AwcFile::parse(&bytes, key).expect("the container parses");
    Some(
        awc::streams(&file)
            .into_iter()
            .filter(|row| row.info.codec == Codec::Xma)
            .map(|row| {
                let stream = &file.streams[row.stream];
                Shipped {
                    id: row.id,
                    rate: row.info.sample_rate,
                    frames: row.info.frames,
                    packets: file
                        .stream_data(stream, key)
                        .expect("the packets descramble"),
                    stated_peaks: stream
                        .format()
                        .and_then(|f| f.peak_value())
                        .into_iter()
                        .chain(stream.peak_envelope().unwrap_or_default())
                        .collect(),
                }
            })
            .collect(),
    )
}

/// What the table makes of `packets` as the stream, as `f32` samples.
fn decode(stream: &Shipped, packets: &[u8]) -> Result<Vec<f32>, or_media::MediaError> {
    let info = AudioInfo {
        codec: Codec::Xma,
        channels: 1,
        sample_rate: stream.rate,
        frames: stream.frames,
    };
    let table = Decoders::new();
    let mut audio = table.audio_decoder(Codec::Xma)?(&info, packets)?;
    let mut out = Vec::new();
    while let Some(block) = audio.read()? {
        out.extend(block.into_f32());
    }
    Ok(out)
}

macro_rules! need_streams {
    ($what:expr) => {
        match shipped() {
            Some(streams) => streams,
            None => {
                skip_install(
                    $what,
                    format_args!("no Legacy install holding {CHIPS}; set {GAME_ROOT_ENV}"),
                );
                return;
            }
        }
    };
}

/// The largest difference allowed between this decoder's samples and FFmpeg's, in full-scale units.
///
/// Both decode in `f32`, so what differs is rounding, which measured 3.6e-7 at worst over the eight
/// streams: a hundredth of one 16-bit step (3.05e-5).
const TOLERANCE: f32 = 1e-6;

/// FFmpeg's `xma2` decode of `packets`, and what it logged beyond the two lines it always does.
fn ffmpeg_samples(dir: &Path, name: &str, stream: &Shipped, packets: &[u8]) -> Vec<f32> {
    let riff = xma2_riff(packets, 1, stream.rate, stream.frames as u32);
    let (samples, said) = ffmpeg_decode(dir, name, &riff);
    let complaints: Vec<&str> = said
        .lines()
        .filter(|l| !l.contains("Estimating duration") && !l.contains("Guessed Channel Layout"))
        .collect();
    assert!(complaints.is_empty(), "{name}: ffmpeg said {complaints:?}");
    samples
}

/// ⚠ **FFmpeg is the oracle, and the two never read each other's code at run time.** FFmpeg keeps
/// the range of a stream the first frame, a 64-sample delay and the frame headers' trims leave, so
/// it returns fewer samples than the stated count, and this decoder returns the stated count, the
/// same samples and then silence.
#[test]
fn legacys_eight_xma_streams_decode_as_ffmpeg_decodes_them() {
    let streams = need_streams!("Legacy's XMA streams against FFmpeg");
    if !ffmpeg_has_xma2() {
        skip_install(
            "Legacy's XMA streams against FFmpeg",
            "no ffmpeg with an xma2 decoder on the PATH",
        );
        return;
    }
    assert_eq!(streams.len(), 8, "Legacy ships eight XMA streams");
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let dir = scratch.path();
    let mut worst_of_all = 0.0f32;
    for s in &streams {
        let name = format!("{:08x}", s.id);
        let theirs = ffmpeg_samples(dir, &name, s, &s.packets);
        let ours = decode(s, &s.packets).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(ours.len() as u64, s.frames, "{name}: the stated count");
        assert!(
            theirs.len() <= ours.len() && theirs.len() + 1024 > ours.len(),
            "{name}: ffmpeg keeps {} of {}",
            theirs.len(),
            ours.len()
        );
        let worst = theirs
            .iter()
            .zip(&ours)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        eprintln!(
            "{name}: {} samples at {} Hz, ffmpeg keeps {}, worst difference {worst:e}",
            ours.len(),
            s.rate,
            theirs.len()
        );
        assert!(worst <= TOLERANCE, "{name}: differs by {worst}");
        assert!(
            ours[theirs.len()..].iter().all(|v| *v == 0.0),
            "{name}: what ffmpeg trims is silence here"
        );
        worst_of_all = worst_of_all.max(worst);
    }
    eprintln!("worst of the eight: {worst_of_all:e} against a tolerance of {TOLERANCE:e}");
}

fn envelope(samples: &[f32], windows: usize) -> Vec<u16> {
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s * 32768.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes())
        .collect();
    or_io::awc::codec::peak_envelope(&pcm, windows)
}

fn mean_error(a: &[u16], b: &[u16]) -> f64 {
    let n = a.len().min(b.len());
    (0..n)
        .map(|i| (f64::from(a[i]) - f64::from(b[i])).abs())
        .sum::<f64>()
        / n as f64
}

/// The peaks each stream states were measured by Rockstar's encoder over the samples it was given,
/// and nothing here reads them, so a decode that fits them is right about more than FFmpeg's reading.
/// XMA is lossy, so the fit is a few percent rather than exact, and the check is against the same
/// peaks fitted by every other stream's decode.
#[test]
fn the_decode_fits_the_peaks_each_stream_states_and_no_other_streams_does() {
    let streams = need_streams!("Legacy's XMA streams against their peaks");
    assert_eq!(streams.len(), 8);
    let decoded: Vec<Vec<f32>> = streams
        .iter()
        .map(|s| decode(s, &s.packets).expect("decodes"))
        .collect();
    let (mut own_sum, mut cross_sum) = (0.0, 0.0);
    for (i, s) in streams.iter().enumerate() {
        let windows = s.stated_peaks.len();
        assert!(windows >= 6, "{:08x} states {windows} windows", s.id);
        let own = mean_error(&envelope(&decoded[i], windows), &s.stated_peaks);
        let cross = decoded
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, d)| mean_error(&envelope(d, windows), &s.stated_peaks))
            .sum::<f64>()
            / 7.0;
        eprintln!(
            "{:08x}: {own:.1} of 65535 over {windows} windows, {cross:.1} for the others' decodes",
            s.id
        );
        assert!(own * 2.0 < cross, "{:08x}: {own} against {cross}", s.id);
        own_sum += own;
        cross_sum += cross;
    }
    eprintln!("mean {:.1} against {:.1}", own_sum / 8.0, cross_sum / 8.0);
    assert!(own_sum * 4.0 < cross_sum, "{own_sum} against {cross_sum}");
}

const PACKET_BITS: usize = 2048 * 8;

fn bits_of(data: &[u8]) -> Vec<bool> {
    data.iter()
        .flat_map(|b| (0..8).map(move |i| (b >> (7 - i)) & 1 == 1))
        .collect()
}

fn read(bits: &[bool], at: usize, n: usize) -> usize {
    bits[at..at + n]
        .iter()
        .fold(0, |v, b| (v << 1) | usize::from(*b))
}

fn put(out: &mut Vec<bool>, value: usize, n: usize) {
    out.extend((0..n).rev().map(|i| (value >> i) & 1 == 1));
}

/// Each frame of a stream's packets as its bits: the length prefix first, the trailing flag last.
fn frames_of(packets: &[u8]) -> Vec<Vec<bool>> {
    let mut frames = Vec::new();
    for packet in packets.chunks(2048) {
        let bits = bits_of(packet);
        let mut at = 32;
        for _ in 0..read(&bits, 0, 6) {
            let len = read(&bits, at, 15);
            frames.push(bits[at..at + len].to_vec());
            at += len;
        }
    }
    frames
}

/// The frames laid into packets back to back, so that a frame starts in one packet and ends in the next.
///
/// A frame's last bit says whether another frame starts in its packet, so it is set again for
/// the new packets; the shipped streams never span, which leaves nothing else to carry over.
fn spanned(frames: &[Vec<bool>]) -> Vec<u8> {
    const CAPACITY: usize = PACKET_BITS - 32;
    let mut starts = Vec::new();
    let mut total = 0;
    for f in frames {
        starts.push(total);
        total += f.len();
    }
    let mut payload = Vec::with_capacity(total);
    for (i, f) in frames.iter().enumerate() {
        let mut f = f.clone();
        let last = f.len() - 1;
        f[last] = starts
            .get(i + 1)
            .is_some_and(|next| next / CAPACITY == starts[i] / CAPACITY);
        payload.extend(f);
    }
    let mut out = Vec::new();
    for lo in (0..total).step_by(CAPACITY) {
        let hi = (lo + CAPACITY).min(total);
        let beginning = starts.iter().filter(|s| (lo..hi).contains(s)).count();
        // Bits of a frame begun in an earlier packet that this packet carries.
        let carried = starts
            .iter()
            .zip(frames)
            .find(|(s, f)| **s < lo && **s + f.len() > lo)
            .map_or(0, |(s, f)| (*s + f.len()).min(hi) - lo);
        let mut bits = Vec::new();
        put(&mut bits, beginning.min(63), 6);
        put(&mut bits, carried, 15);
        put(&mut bits, 1, 3);
        put(&mut bits, 0, 8);
        bits.extend(&payload[lo..hi]);
        bits.resize(PACKET_BITS, true);
        out.extend(
            bits.chunks(8)
                .map(|b| b.iter().fold(0u8, |v, x| (v << 1) | u8::from(*x))),
        );
    }
    out
}

/// ⚠ **The shipped streams never span packets**, so the bit reservoir that finishes a frame the
/// last packet began is checked by laying their frames into packets that do, which this decoder and
/// FFmpeg must both read as the packets they came from.
#[test]
fn frames_that_span_packets_decode_as_they_do_whole() {
    let streams = need_streams!("Legacy's XMA frames spanning packets");
    let ffmpeg = ffmpeg_has_xma2();
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let dir = scratch.path();
    for s in &streams {
        let name = format!("{:08x}", s.id);
        let frames = frames_of(&s.packets);
        let respun = spanned(&frames);
        let carrying = respun
            .chunks(2048)
            .filter(|p| read(&bits_of(&p[..4]), 6, 15) > 0)
            .count();
        assert!(
            carrying >= respun.len() / 2048 / 2,
            "{name}: {carrying} of {} packets carry a frame over",
            respun.len() / 2048
        );
        let whole = decode(s, &s.packets).expect("decodes");
        let span = decode(s, &respun).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(span, whole, "{name}: spanning changed the decode");
        if ffmpeg {
            let theirs = ffmpeg_samples(dir, &name, s, &respun);
            let worst = theirs
                .iter()
                .zip(&span)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(worst <= TOLERANCE, "{name}: differs from ffmpeg by {worst}");
            eprintln!(
                "{name}: {} packets became {}, {carrying} carrying a frame over; ffmpeg differs by {worst:e}",
                s.packets.len() / 2048,
                respun.len() / 2048
            );
        }
    }
}

/// A damaged stream is refused or decoded, and never takes the reader down with it.
#[test]
fn a_damaged_stream_is_decoded_or_refused_and_never_a_panic() {
    let streams = need_streams!("Legacy's XMA streams, damaged");
    let mut seed = 99u32;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as usize % n
    };
    let (mut decoded, mut refused) = (0, 0);
    for s in &streams {
        for _ in 0..40 {
            let mut packets = s.packets.clone();
            for _ in 0..1 + next(6) {
                let at = next(packets.len());
                packets[at] ^= 1 << next(8);
            }
            match decode(s, &packets) {
                Ok(_) => decoded += 1,
                Err(_) => refused += 1,
            }
        }
    }
    eprintln!("{decoded} damaged streams decoded and {refused} were refused");
    assert!(
        decoded > 0 && refused > 0,
        "damage is sometimes survived and sometimes refused"
    );
}
