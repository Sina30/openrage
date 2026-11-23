//! `.awc` codecs over both installs: Legacy's XMA streams are identified by
//! their packets and refused by name, a shipped multi-channel container
//! rebuilds into its own geometry, and a shipped stream's audio encodes to
//! MP3 frames of the shape Enhanced's streams have.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, `;`-separated.
//! The `.awc` files sit inside archives, so each install's own executable
//! supplies the archive and AWC keys.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::awc::codec::{self, Accumulator};
use or_io::awc::wav::interleave_pcm16;
use or_io::awc::{AwcChunkKind, AwcFile, AwcKey, Codec};
use or_io::container::rpf::crypto::{extract_keys, Gta5Keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::{RpfArchive, RpfCryptoProvider};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};
use rayon::prelude::*;

/// How deep a `.rpf` inside a `.rpf` is followed, as the install crawl does.
const MAX_NEST_DEPTH: u32 = 8;

/// Bytes of `.awc` held before a batch is measured on every core.
const BATCH_BYTES: usize = 256 << 20;

/// Two speech archives whose single-channel streams stand for the install's audio: 3,107 and 4,277
/// ADPCM streams at 32 or 48 kHz in Legacy, the same logical files as 3,110 and 4,278 MP3 streams in
/// Enhanced.
const SAMPLE: [&str; 2] = ["x64/audio/sfx/SS_DE.rpf", "x64/audio/sfx/SS_QR.rpf"];

struct Install {
    root: PathBuf,
    enhanced: bool,
}

fn installs(what: &str) -> Vec<Install> {
    let found: Vec<Install> = game_roots()
        .into_iter()
        .filter_map(|root| {
            if root.join("GTA5_Enhanced.exe").is_file() {
                Some(Install {
                    root,
                    enhanced: true,
                })
            } else if root.join("GTA5.exe").is_file() {
                Some(Install {
                    root,
                    enhanced: false,
                })
            } else {
                None
            }
        })
        .collect();
    if found.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
    }
    found
}

fn keys_of(install: &Install) -> Arc<Gta5Keys> {
    let exe = if install.enhanced {
        "GTA5_Enhanced.exe"
    } else {
        "GTA5.exe"
    };
    let keys = Arc::new(
        extract_keys(install.root.join(exe), &|_| {}).expect("keys from the install's executable"),
    );
    assert!(AwcKey(keys.awc_key).is_usable(), "{exe} carries no AWC key");
    keys
}

fn provider(keys: &Arc<Gta5Keys>) -> Arc<dyn RpfCryptoProvider> {
    Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(keys)))
}

fn walk_archive(
    archive: &Arc<RpfArchive>,
    label: &str,
    depth: u32,
    visit: &mut dyn FnMut(String, Result<Vec<u8>, String>),
) {
    for node in archive.nodes.iter().filter(|n| !n.is_dir()) {
        let name = node.name().to_ascii_lowercase();
        let path = format!("{label}/{}", node.path());
        let i = node.entry_index();
        if name.ends_with(".rpf") && depth < MAX_NEST_DEPTH {
            match archive.open_nested(i) {
                Ok(nested) => walk_archive(&Arc::new(nested), &path, depth + 1, visit),
                Err(e) => visit(path, Err(e.to_string())),
            }
        } else if name.ends_with(".awc") {
            visit(path, archive.read_entry(i).map_err(|e| e.to_string()));
        }
    }
}

/// Every `.awc` under `root`, in an archive at any depth; with `only`, those in the archives it names.
fn every_awc(
    root: &Path,
    keys: &Arc<Gta5Keys>,
    only: Option<&[&str]>,
    visit: &mut dyn FnMut(String, Result<Vec<u8>, String>),
) {
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let label = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let wanted =
                only.is_none_or(|names| names.iter().any(|n| n.eq_ignore_ascii_case(&label)));
            if wanted && label.to_ascii_lowercase().ends_with(".rpf") {
                match RpfArchive::open_with(&path, provider(keys)) {
                    Ok(archive) => walk_archive(&Arc::new(archive), &label, 0, visit),
                    Err(e) => visit(label, Err(e.to_string())),
                }
            }
        }
    }
}

/// Every `.awc` of an install through `measure`, a batch at a time on every core.
fn measure_install<T: Default + Send>(
    install: &Install,
    only: Option<&[&str]>,
    measure: impl Fn(&str, &AwcFile, AwcKey) -> T + Sync,
    merge: impl Fn(T, T) -> T + Sync + Send,
) -> (T, usize, Vec<String>) {
    let keys = keys_of(install);
    let key = AwcKey(keys.awc_key);
    let (mut total, mut files, mut unread) = (T::default(), 0usize, Vec::new());
    let mut batch: Vec<(String, Vec<u8>)> = Vec::new();
    let mut held = 0usize;
    let flush = |batch: &mut Vec<(String, Vec<u8>)>, total: &mut T, files: &mut usize| {
        let measured = batch
            .par_iter()
            .map(|(label, bytes)| match AwcFile::parse(bytes, key) {
                Ok(awc) => (measure(label, &awc, key), 1usize),
                // Not a container at all: two files under `common/shaders/fxdb` ship as `.awc`.
                Err(_) => (T::default(), 0),
            })
            .reduce(|| (T::default(), 0), |a, b| (merge(a.0, b.0), a.1 + b.1));
        *total = merge(std::mem::take(total), measured.0);
        *files += measured.1;
        batch.clear();
    };
    every_awc(
        &install.root,
        &keys,
        only,
        &mut |label, bytes| match bytes {
            Ok(bytes) => {
                held += bytes.len();
                batch.push((label, bytes));
                if held > BATCH_BYTES {
                    flush(&mut batch, &mut total, &mut files);
                    held = 0;
                }
            }
            Err(e) => unread.push(format!("{label}: {e}")),
        },
    );
    flush(&mut batch, &mut total, &mut files);
    (total, files, unread)
}

fn samples(pcm: &[u8]) -> impl Iterator<Item = f64> + '_ {
    pcm.as_chunks::<2>()
        .0
        .iter()
        .map(|b| f64::from(i16::from_le_bytes([b[0], b[1]])))
}

/// Signal-to-error in dB of `got` against `want`, `None` where `want` is silence there is nothing to be close to.
fn snr_db(want: &[u8], got: &[u8]) -> Option<f64> {
    let signal: f64 = samples(want).map(|s| s * s).sum();
    let error: f64 = samples(want)
        .zip(samples(got))
        .map(|(a, b)| (a - b) * (a - b))
        .sum();
    if signal / (want.len() / 2).max(1) as f64 <= 16.0 {
        return None;
    }
    Some(10.0 * (signal / error.max(1.0)).log10())
}

// ---------------------------------------------------------------- XMA

/// Frames each 2,048-byte packet of an XMA stream holds, if every packet is well formed.
///
/// ⚠ **This is the structure the XMA identification rests on.** A packet is a 32-bit header whose
/// first six bits count its frames, then frames that each open with their own length in bits (15
/// bits, the length field included), then bits that are all ones to the packet's end. A stream that
/// is not XMA does not chain this way across its packets.
fn xma_frames_per_packet(data: &[u8]) -> Option<Vec<usize>> {
    const PACKET: usize = 2048;
    if data.is_empty() || !data.len().is_multiple_of(PACKET) {
        return None;
    }
    let bits = |packet: &[u8], at: usize, n: usize| -> usize {
        (at..at + n).fold(0, |v, b| {
            (v << 1) | usize::from((packet[b / 8] >> (7 - b % 8)) & 1)
        })
    };
    let mut counts = Vec::new();
    for packet in data.chunks(PACKET) {
        let frames = bits(packet, 0, 6);
        let mut at = 32;
        for _ in 0..frames {
            let length = bits(packet, at, 15);
            if length < 15 || at + length > PACKET * 8 {
                return None;
            }
            at += length;
        }
        if !(at..PACKET * 8).all(|b| bits(packet, b, 1) == 1) {
            return None;
        }
        counts.push(frames);
    }
    Some(counts)
}

/// One install's XMA: the containers holding it, and how many of its streams are what they say.
#[derive(Default)]
struct Xma {
    streams: usize,
    channels: usize,
    packets: usize,
    containers: Vec<String>,
    wrong: Vec<String>,
}

fn xma_of(label: &str, awc: &AwcFile, key: AwcKey) -> Xma {
    let mut found = Xma::default();
    for stream in &awc.streams {
        if let Some(format) = stream.stream_format() {
            let n = format
                .channels
                .iter()
                .filter(|c| c.codec == Codec::Xma)
                .count();
            found.channels += n;
        }
        let Some(format) = stream.format().filter(|f| f.codec == Codec::Xma) else {
            continue;
        };
        found.streams += 1;
        if !found.containers.iter().any(|c| c == label) {
            found.containers.push(label.to_string());
        }
        let name = format!("{label} {:08x}", stream.id);
        let Some(data) = awc.stream_data(stream, key) else {
            found.wrong.push(format!("{name}: not descrambled"));
            continue;
        };
        let Some(frames) = xma_frames_per_packet(&data) else {
            found
                .wrong
                .push(format!("{name}: its packets do not chain"));
            continue;
        };
        found.packets += frames.len();
        // The seek table is one big-endian word per packet: the samples decoded so far, 512 a frame.
        let seek: Vec<usize> = stream
            .chunk(AwcChunkKind::SeekTable)
            .map(|c| {
                c.payload
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|w| u32::from_be_bytes(*w) as usize)
                    .collect()
            })
            .unwrap_or_default();
        let mut cumulative = 0;
        let want: Vec<usize> = frames
            .iter()
            .map(|f| {
                cumulative += f * 512;
                cumulative
            })
            .collect();
        if seek != want || want.last() != Some(&(format.samples as usize)) {
            found
                .wrong
                .push(format!("{name}: its seek table is not the frames' samples"));
        }
        if awc
            .decode_stream(stream, key, Accumulator::Clamped)
            .is_some()
        {
            found.wrong.push(format!("{name}: decodes"));
        }
    }
    found
}

fn merge_xma(mut a: Xma, b: Xma) -> Xma {
    a.streams += b.streams;
    a.channels += b.channels;
    a.packets += b.packets;
    a.containers.extend(b.containers);
    a.wrong.extend(b.wrong);
    a
}

/// ⚠ **Measured, not named by the file**: codec byte 5 is XMA because every one of its streams'
/// payloads is whole 2,048-byte packets that chain by their own frame lengths with ones to pad, and
/// because its seek table is the cumulative samples at each packet's end, 512 for each frame its
/// header counts. Legacy has eight such streams, all in one casino pack's container; Enhanced has
/// none. The coding inside a frame needs tables no permissively licensed source carries, so they
/// refuse to decode by name.
#[test]
fn legacys_eight_codec_5_streams_are_xma_packets_and_refuse_to_decode() {
    for install in installs("Legacy's codec-5 streams") {
        let started = std::time::Instant::now();
        let (xma, files, unread) = measure_install(&install, None, xma_of, merge_xma);
        eprintln!(
            "{}: {} .awc in {:.0?}; {} XMA stream(s) in {:?}, {} packet(s), {} XMA channel(s)",
            install.root.display(),
            files,
            started.elapsed(),
            xma.streams,
            xma.containers,
            xma.packets,
            xma.channels
        );
        for why in &unread {
            eprintln!("  not read: {why}");
        }
        assert!(xma.wrong.is_empty(), "{}", xma.wrong.join("\n"));
        assert_eq!(xma.channels, 0, "a multi-channel container holds XMA");
        if install.enhanced {
            assert_eq!(xma.streams, 0, "Enhanced holds XMA");
        } else {
            assert_eq!(xma.streams, 8);
            assert_eq!(xma.containers.len(), 1);
            assert!(xma.containers[0].ends_with("vw_counterfeit_chips.awc"));
            assert_eq!(xma.packets, 52);
        }
    }
}

// ------------------------------------------------- multi-channel rebuild

/// What one container's rebuild measured.
#[derive(Default)]
struct Rebuilt {
    containers: usize,
    channels: usize,
    /// Each channel's signal-to-error against the shipped decode, in dB.
    snr: Vec<f64>,
    /// Containers whose last block the shipped file pads to the block size, where this trims it.
    last_block_padded: usize,
    /// Containers cut to their first blocks, and containers left out for their size.
    cut: usize,
    too_large: usize,
    failed: Vec<String>,
}

/// A block of a container as its table states it.
#[derive(PartialEq, Debug)]
struct Table {
    packets: Vec<u32>,
    first_packets: Vec<u32>,
    samples: u32,
    offsets: Vec<u32>,
}

fn tables(data: &[u8], block_size: usize, n: usize) -> Vec<(Table, usize)> {
    data.chunks(block_size)
        .map(|block| {
            let word = |at: usize| u32::from_le_bytes(block[at..at + 4].try_into().unwrap());
            let packets: Vec<u32> = (0..n).map(|c| word(c * 24 + 4)).collect();
            let mut at = n * 24;
            let mut offsets = Vec::new();
            for p in &packets {
                for _ in 0..*p {
                    offsets.push(word(at));
                    at += 4;
                }
            }
            (
                Table {
                    first_packets: (0..n).map(|c| word(c * 24)).collect(),
                    samples: word(12),
                    packets,
                    offsets,
                },
                block.len(),
            )
        })
        .collect()
}

fn fail(out: &mut Rebuilt, label: &str, why: String) {
    out.failed.push(format!("{label}: {why}"));
}

/// Blocks of a container the rebuild is made over: an encode costs the container's size and the
/// geometry repeats from block to block, so a longer container is cut to this many.
const CHECKED_BLOCKS: usize = 3;

/// Bytes of block data past which a container is left out, for the memory its decode takes.
const LARGEST_CONTAINER: usize = 16 << 20;

fn rebuilt_of(label: &str, awc: &AwcFile, key: AwcKey) -> Rebuilt {
    let mut out = Rebuilt::default();
    let Some(source) = awc.streams.iter().position(|s| s.stream_format().is_some()) else {
        return out;
    };
    let format = awc.streams[source].stream_format().expect("found by it");
    let n = format.channels.len();
    let block_size = format.block_size as usize;
    if format.channels.iter().any(|c| c.codec != Codec::ImaAdpcm) {
        return out;
    }
    let Some(old_data) = awc.stream_data(&awc.streams[source], key) else {
        fail(&mut out, label, "its data is not descrambled".into());
        return out;
    };
    if old_data.len() > LARGEST_CONTAINER {
        out.too_large = 1;
        return out;
    }
    let Some(shipped) = awc.decode_channels(key, Accumulator::Clamped) else {
        fail(&mut out, label, "does not decode".into());
        return out;
    };
    let was = tables(&old_data, block_size, n);
    let whole = was.len() <= CHECKED_BLOCKS;
    let keep = if whole {
        format.channels[0].samples as usize
    } else {
        was[..CHECKED_BLOCKS]
            .iter()
            .map(|(t, _)| t.samples as usize)
            .sum()
    };
    // ⚠ One sample of each channel is changed by a bit, because a channel left exactly as it
    // decodes keeps its shipped packets and nothing would be encoded.
    let cut: Vec<Vec<u8>> = shipped
        .iter()
        .map(|c| {
            let mut c = c[..keep * 2].to_vec();
            c[0] ^= 1;
            c
        })
        .collect();
    let rate = u32::from(format.channels[0].samples_per_second);
    let replaced =
        match awc.with_stream_replaced(source, &interleave_pcm16(&cut), rate, n as u16, key) {
            Ok(replaced) => replaced,
            Err(why) => {
                fail(&mut out, label, format!("is not replaced: {why}"));
                return out;
            }
        };
    out.containers = 1;
    out.channels = n;
    out.cut = usize::from(!whole);

    let new = &replaced.streams[source];
    let new_format = new.stream_format().expect("kept");
    let blocks = if whole { was.len() } else { CHECKED_BLOCKS };
    if (new_format.block_count as usize, new_format.block_size) != (blocks, format.block_size) {
        fail(
            &mut out,
            label,
            format!(
                "blocks {} of {} became {} of {}",
                blocks, format.block_size, new_format.block_count, new_format.block_size
            ),
        );
        return out;
    }
    let Some(new_data) = replaced.stream_data(new, key) else {
        fail(&mut out, label, "the data chunk is gone".into());
        return out;
    };
    let now = tables(&new_data, block_size, n);
    // A cut container's last block is full where the shipped one is not, so only the blocks
    // before it are the same.
    let same = if whole { blocks } else { blocks - 1 };
    for (b, ((a, a_len), (c, c_len))) in was.iter().zip(&now).enumerate().take(same) {
        let last = b + 1 == was.len();
        // The shipped file's last block states the true count of an odd-length stream.
        let samples_agree = a.samples == c.samples || (last && a.samples == c.samples + 1);
        if a.packets != c.packets
            || a.first_packets != c.first_packets
            || a.offsets != c.offsets
            || !samples_agree
        {
            fail(&mut out, label, format!("block {b}: {a:?} became {c:?}"));
            return out;
        }
        if !last && (*a_len != block_size || *c_len != block_size) {
            fail(
                &mut out,
                label,
                format!("block {b} is {a_len} then {c_len} bytes of {block_size}"),
            );
            return out;
        }
        if last && a_len != c_len {
            if *a_len == block_size {
                out.last_block_padded = 1;
            } else {
                fail(
                    &mut out,
                    label,
                    format!("the last block is {a_len} bytes, then {c_len}"),
                );
                return out;
            }
        }
    }
    let seek = |awc: &AwcFile| {
        awc.streams[source]
            .chunk(AwcChunkKind::SeekTable)
            .map(|c| c.payload.clone())
    };
    let (was_seek, now_seek) = (seek(awc), seek(&replaced));
    let seek_agrees = match (&was_seek, &now_seek) {
        (Some(a), Some(b)) => a[..blocks * 4] == b[..],
        (None, None) => true,
        _ => false,
    };
    if !seek_agrees {
        fail(&mut out, label, "the seek table changed".into());
        return out;
    }

    let Some(decoded) = replaced.decode_channels(key, Accumulator::Clamped) else {
        fail(
            &mut out,
            label,
            "the rebuilt container does not decode".into(),
        );
        return out;
    };
    for (c, (want, got)) in cut.iter().zip(&decoded).enumerate() {
        if want.len() != got.len() {
            fail(
                &mut out,
                label,
                format!("channel {c} is {} bytes, then {}", want.len(), got.len()),
            );
            return out;
        }
        out.snr.extend(snr_db(want, got));
    }
    out
}

fn merge_rebuilt(mut a: Rebuilt, b: Rebuilt) -> Rebuilt {
    a.containers += b.containers;
    a.channels += b.channels;
    a.snr.extend(b.snr);
    a.last_block_padded += b.last_block_padded;
    a.cut += b.cut;
    a.too_large += b.too_large;
    a.failed.extend(b.failed);
    a
}

/// Signal-to-error, in dB, no channel's re-encode falls below. The shipped single-channel streams'
/// re-encodes measure a worst of 18.0 and a median of 41.3.
const CHANNEL_FLOOR_DB: f64 = 15.0;

/// ⚠ **The rebuilt container has the shipped one's own blocks**, not merely blocks this reads: the
/// same packets of each channel in each block, first-packet words, packet offsets, samples,
/// seek table and block lengths, over every multi-channel container whose channels are ADPCM. What
/// differs is the packets' bytes, which the game's encoder chose block by block, and the last
/// block, which some shipped containers pad to the block size and this trims.
#[test]
fn every_shipped_adpcm_multi_channel_container_rebuilds_into_its_own_blocks() {
    for install in installs("every shipped multi-channel ADPCM container rebuilt") {
        let started = std::time::Instant::now();
        let (rebuilt, files, unread) = measure_install(&install, None, rebuilt_of, merge_rebuilt);
        let mut snr = rebuilt.snr.clone();
        snr.sort_by(f64::total_cmp);
        eprintln!(
            "{}: {} .awc in {:.0?}; {} multi-channel ADPCM container(s) rebuilt, {} channel(s), \
             {} cut to {CHECKED_BLOCKS} blocks and {} left out as over {} MiB; {} shipped with \
             the last block padded; worst channel {:.1} dB, median {:.1}",
            install.root.display(),
            files,
            started.elapsed(),
            rebuilt.containers,
            rebuilt.channels,
            rebuilt.cut,
            rebuilt.too_large,
            LARGEST_CONTAINER >> 20,
            rebuilt.last_block_padded,
            snr.first().copied().unwrap_or(f64::NAN),
            snr.get(snr.len() / 2).copied().unwrap_or(f64::NAN),
        );
        for why in &unread {
            eprintln!("  not read: {why}");
        }
        assert!(
            rebuilt.failed.is_empty(),
            "{} failure(s), the first of them:\n{}",
            rebuilt.failed.len(),
            rebuilt.failed[..rebuilt.failed.len().min(20)].join("\n")
        );
        assert!(rebuilt.containers > 0, "no multi-channel ADPCM container");
        assert!(
            snr.first().is_none_or(|worst| *worst >= CHANNEL_FLOOR_DB),
            "a channel re-encodes at {:.1} dB, under {CHANNEL_FLOOR_DB}",
            snr[0]
        );
    }
}

// ------------------------------------------------------------------ MP3

/// What encoding one install's sample of single-channel streams to MP3 measured.
#[derive(Default)]
struct Encoded {
    streams: usize,
    frames: usize,
    snr: Vec<f64>,
    failed: Vec<String>,
}

fn encoded_of(label: &str, awc: &AwcFile, key: AwcKey) -> Encoded {
    let mut out = Encoded::default();
    for stream in &awc.streams {
        let Some(format) = stream
            .format()
            .filter(|f| matches!(f.codec, Codec::Mp3 | Codec::ImaAdpcm))
        else {
            continue;
        };
        let name = format!("{label} {:08x}", stream.id);
        let Some(pcm) = awc.decode_stream(stream, key, Accumulator::Clamped) else {
            out.failed.push(format!("{name}: does not decode"));
            continue;
        };
        // An MP3 stream's frames say the rate they are at; an ADPCM one is encoded at its own.
        let rate = match format.codec {
            Codec::Mp3 => awc
                .stream_data(stream, key)
                .and_then(|d| codec::mpeg_frame_rate(&d)),
            _ => Some(u32::from(format.samples_per_second)),
        };
        let Some(rate) = rate.filter(|r| matches!(r, 32000 | 48000)) else {
            continue;
        };
        out.streams += 1;
        let mp3 = match codec::encode_mp3(&pcm, rate, 96.0) {
            Ok(mp3) => mp3,
            Err(why) => {
                out.failed.push(format!("{name}: {why}"));
                continue;
            }
        };
        out.frames += mp3.frame_sizes.len();
        let Some(back) = codec::decode(&mp3.data, mp3.samples, Codec::Mp3, Accumulator::Clamped)
        else {
            out.failed.push(format!("{name}: the frames do not decode"));
            continue;
        };
        let start = codec::MP3_START * 2;
        out.snr
            .extend(snr_db(&pcm, &back[start..start + pcm.len()]));
    }
    out
}

fn merge_encoded(mut a: Encoded, b: Encoded) -> Encoded {
    a.streams += b.streams;
    a.frames += b.frames;
    a.snr.extend(b.snr);
    a.failed.extend(b.failed);
    a
}

/// Signal-to-error, in dB, no stream's MP3 falls below, against its own decode. MP3 is lossy and the
/// audio is already a lossy decode, so this is a floor on what the encoder does to real sound: over
/// the sample the worst measures 17.7 dB in Legacy and 18.6 in Enhanced.
const MP3_FLOOR_DB: f64 = 15.0;

/// ⚠ **Real audio from both installs encodes to the shape Enhanced's streams have**: every frame
/// MPEG-1 mono at 32 or 48 kHz with no CRC, no padding and no borrowed bytes, which `encode_mp3`
/// refuses a frame for lacking, and the decode matches the source after the codec's delay.
#[test]
fn a_shipped_streams_audio_encodes_to_mp3_frames_of_the_shipped_shape() {
    for install in installs("shipped audio encoded to MP3") {
        let started = std::time::Instant::now();
        let (encoded, files, unread) =
            measure_install(&install, Some(&SAMPLE), encoded_of, merge_encoded);
        let mut snr = encoded.snr.clone();
        snr.sort_by(f64::total_cmp);
        eprintln!(
            "{}: {} .awc in {:.0?}; {} stream(s) encoded to {} frame(s); worst {:.1} dB, median {:.1}",
            install.root.display(),
            files,
            started.elapsed(),
            encoded.streams,
            encoded.frames,
            snr.first().copied().unwrap_or(f64::NAN),
            snr.get(snr.len() / 2).copied().unwrap_or(f64::NAN),
        );
        for why in &unread {
            eprintln!("  not read: {why}");
        }
        assert!(
            encoded.failed.is_empty(),
            "{} failure(s), the first of them:\n{}",
            encoded.failed.len(),
            encoded.failed[..encoded.failed.len().min(20)].join("\n")
        );
        assert!(
            encoded.streams > 0,
            "no stream at 32 or 48 kHz in the sample"
        );
        assert!(
            snr.first().is_none_or(|worst| *worst >= MP3_FLOOR_DB),
            "a stream encodes at {:.1} dB, under {MP3_FLOOR_DB}",
            snr[0]
        );
    }
}

// ------------------------------------------------- the shipped MP3 shape

/// The kilobits a Layer III MPEG-1 frame header's bitrate index states.
const MPEG1_KBPS: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];

/// What walking the frames of every single-channel MP3 stream measured.
#[derive(Default)]
struct Shape {
    streams: usize,
    frames: usize,
    kilobits: u64,
    /// Streams whose frames are not MPEG-1 mono without CRC or padding, do not reach the payload's
    /// end exactly, borrow bytes from the frame before, or state another rate than 32 or 48 kHz.
    off_shape: Vec<String>,
    /// Streams whose seek table is not one little-endian `u16` of byte length per frame.
    seek_wrong: Vec<String>,
    /// Streams whose frames hold a whole frame more samples than they state, or fewer.
    count_off: Vec<String>,
}

fn shape_of(label: &str, awc: &AwcFile, key: AwcKey) -> Shape {
    let mut out = Shape::default();
    for stream in &awc.streams {
        let Some(format) = stream.format().filter(|f| f.codec == Codec::Mp3) else {
            continue;
        };
        let name = format!("{label} {:08x}", stream.id);
        let Some(data) = awc.stream_data(stream, key) else {
            out.off_shape.push(format!("{name}: not descrambled"));
            continue;
        };
        out.streams += 1;
        let (mut at, mut sizes) = (0usize, Vec::new());
        let mut shape =
            data.len() >= 4 && matches!(codec::mpeg_frame_rate(&data), Some(32000 | 48000));
        while shape && at < data.len() {
            let Some(h) = data.get(at..at + 6) else {
                shape = false;
                break;
            };
            let kbps = MPEG1_KBPS[usize::from(h[2] >> 4)];
            let rate = codec::mpeg_frame_rate(&data[at..]).unwrap_or(0);
            // MPEG-1, Layer III, no CRC; no padding; mono; no bytes borrowed from the frame before.
            shape = h[0] == 0xFF
                && h[1] == 0xFB
                && (h[2] >> 1) & 1 == 0
                && h[3] >> 6 == 3
                && ((usize::from(h[4]) << 1) | usize::from(h[5] >> 7)) == 0
                && kbps > 0
                && rate > 0;
            if shape {
                let size = 144_000 * kbps as usize / rate as usize;
                out.kilobits += u64::from(kbps);
                sizes.push(size);
                at += size;
            }
        }
        if !shape || at != data.len() {
            out.off_shape.push(name);
            continue;
        }
        out.frames += sizes.len();
        if let Some(seek) = stream.chunk(AwcChunkKind::SeekTable) {
            let listed: Vec<usize> = seek
                .payload
                .as_chunks::<2>()
                .0
                .iter()
                .map(|w| usize::from(u16::from_le_bytes([w[0], w[1]])))
                .collect();
            if listed != sizes {
                out.seek_wrong.push(name.clone());
            }
        }
        let holds = sizes.len() * 1152;
        if holds < format.samples as usize || holds - format.samples as usize >= 1152 {
            out.count_off.push(name);
        }
    }
    out
}

fn merge_shape(mut a: Shape, b: Shape) -> Shape {
    a.streams += b.streams;
    a.frames += b.frames;
    a.kilobits += b.kilobits;
    a.off_shape.extend(b.off_shape);
    a.seek_wrong.extend(b.seek_wrong);
    a.count_off.extend(b.count_off);
    a
}

/// ⚠ **This is the shape `encode_mp3` writes, measured over everything Enhanced ships**: every
/// single-channel MP3 stream is MPEG-1 mono with no CRC and no padding, at 32 or 48 kHz, each frame
/// self-contained, its seek table the frames' byte lengths as `u16`, and its frames holding between
/// none and 1,151 samples more than it states. One stream holds 57 fewer, which the decode pads
/// with silence.
#[test]
fn every_enhanced_single_channel_mp3_stream_has_the_shape_the_encoder_writes() {
    for install in installs("the shape of Enhanced's MP3 streams")
        .into_iter()
        .filter(|i| i.enhanced)
    {
        let started = std::time::Instant::now();
        let (shape, files, _) = measure_install(&install, None, shape_of, merge_shape);
        eprintln!(
            "{}: {} .awc in {:.0?}; {} MP3 stream(s), {} frame(s), mean {:.1} kbps; {} off the \
             shape, {} with another seek table, {} with a count off by a frame",
            install.root.display(),
            files,
            started.elapsed(),
            shape.streams,
            shape.frames,
            shape.kilobits as f64 / shape.frames.max(1) as f64,
            shape.off_shape.len(),
            shape.seek_wrong.len(),
            shape.count_off.len(),
        );
        let first = |list: &[String]| list[..list.len().min(10)].to_vec();
        assert!(shape.streams > 0, "no MP3 stream");
        assert!(shape.off_shape.is_empty(), "{:?}", first(&shape.off_shape));
        assert!(
            shape.seek_wrong.is_empty(),
            "{:?}",
            first(&shape.seek_wrong)
        );
        assert!(shape.count_off.len() <= 1, "{:?}", first(&shape.count_off));
    }
}
