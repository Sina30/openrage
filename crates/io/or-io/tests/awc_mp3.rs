//! Enhanced `.awc` MP3 over the install: every stream decodes to the samples
//! it states, the peaks its encoder recorded fit the decode, and each channel
//! of a multi-channel stream runs on across its blocks.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, `;`-separated,
//! and only an Enhanced one holds MP3. The `.awc` files sit inside archives,
//! so each install's own executable supplies the archive and AWC keys.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::awc::{codec, Accumulator, AwcFile, AwcKey, Codec};
use or_io::container::rpf::crypto::{extract_keys, Gta5Keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::{RpfArchive, RpfCryptoProvider};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};
use rayon::prelude::*;

/// How deep a `.rpf` inside a `.rpf` is followed, as the install crawl does.
const MAX_NEST_DEPTH: u32 = 8;

/// Bytes of `.awc` held before a batch is decoded on every core.
const BATCH_BYTES: usize = 256 << 20;

/// An MP3 decoder's output delay in samples: what a decode that trimmed it
/// would drop from the start of every stream.
const DECODER_DELAY: usize = 529;

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

/// Every `.awc` under `root`, in an archive at any depth.
fn every_awc(
    root: &Path,
    keys: &Arc<Gta5Keys>,
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
            if label.to_ascii_lowercase().ends_with(".rpf") {
                match RpfArchive::open_with(&path, provider(keys)) {
                    Ok(archive) => walk_archive(&Arc::new(archive), &label, 0, visit),
                    Err(e) => visit(label, Err(e.to_string())),
                }
            }
        }
    }
}

/// What one install's MP3 measured.
#[derive(Default)]
struct Tally {
    files: usize,
    refused: Vec<String>,
    streams: usize,
    channels: usize,
    failed: Vec<String>,
    /// Peak |error| against the decode from sample 0: (sum, count).
    peaks: (u64, usize),
    /// The same peaks against the decode shifted by [`DECODER_DELAY`].
    peaks_delayed: (u64, usize),
    /// |Δ| between neighbouring samples anywhere: (sum, count).
    interior: (u64, usize),
    /// |Δ| across each block join of a channel: (sum, count).
    joined: (u64, usize),
}

impl Tally {
    fn merge(mut self, other: Tally) -> Tally {
        self.files += other.files;
        self.refused.extend(other.refused);
        self.streams += other.streams;
        self.channels += other.channels;
        self.failed.extend(other.failed);
        for (a, b) in [
            (&mut self.peaks, other.peaks),
            (&mut self.peaks_delayed, other.peaks_delayed),
            (&mut self.interior, other.interior),
            (&mut self.joined, other.joined),
        ] {
            a.0 += b.0;
            a.1 += b.1;
        }
        self
    }

    fn mean((sum, count): (u64, usize)) -> f64 {
        sum as f64 / count.max(1) as f64
    }
}

fn add_peaks(slot: &mut (u64, usize), pcm: &[u8], declared: u16, envelope: &[u16], samples: usize) {
    let windows = codec::peak_window_count(samples);
    let ours = codec::peak_envelope(pcm, windows + 1);
    slot.0 += (ours[0] as i64 - declared as i64).unsigned_abs();
    slot.1 += 1;
    if windows > 0 && envelope.len() == windows {
        for (i, want) in envelope.iter().enumerate() {
            slot.0 += (ours[i + 1] as i64 - *want as i64).unsigned_abs();
            slot.1 += 1;
        }
    }
}

fn sample(pcm: &[u8], i: usize) -> i32 {
    i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]) as i32
}

/// Where each block after the first starts in a channel's decode: the sum,
/// over the blocks before it, of its samples (`u32` at +12) less the repeat
/// of the previous block it opens with (`u32` at +8).
fn block_joins(data: &[u8], block_size: usize, channel: usize) -> Vec<usize> {
    let mut at = 0usize;
    let mut joins = Vec::new();
    for block in data.chunks(block_size) {
        let u32_at = |o: usize| u32::from_le_bytes(block[o..o + 4].try_into().unwrap()) as usize;
        if at > 0 {
            joins.push(at);
        }
        // The +12 count is signed: a last block may take samples back.
        at = at.saturating_add_signed(u32_at(channel * 24 + 12) as u32 as i32 as isize)
            - u32_at(channel * 24 + 8);
    }
    joins
}

fn measure(label: &str, bytes: &[u8], key: AwcKey) -> Tally {
    let mut tally = Tally {
        files: 1,
        ..Tally::default()
    };
    let awc = match AwcFile::parse(bytes, key) {
        Ok(awc) => awc,
        Err(e) => {
            // Not a container at all: two files under `common/shaders/fxdb` ship
            // as `.awc`. `awc_roundtrip.rs` and the census answer for containers.
            tally.refused.push(format!("{label}: {e}"));
            return tally;
        }
    };
    for stream in &awc.streams {
        let Some(format) = stream.format().filter(|f| f.codec == Codec::Mp3) else {
            continue;
        };
        tally.streams += 1;
        let samples = format.samples as usize;
        let Some(pcm) = awc.decode_stream(stream, key, Accumulator::Clamped) else {
            tally.failed.push(format!(
                "{label} {:08x}: does not decode to its {samples} samples",
                stream.id
            ));
            continue;
        };
        assert_eq!(pcm.len(), samples * 2, "{label} {:08x}", stream.id);
        if let (Some(declared), Some(envelope)) = (format.peak_value(), stream.peak_envelope()) {
            add_peaks(&mut tally.peaks, &pcm, declared, &envelope, samples);
            let delayed = &pcm[(DECODER_DELAY * 2).min(pcm.len())..];
            add_peaks(
                &mut tally.peaks_delayed,
                delayed,
                declared,
                &envelope,
                samples,
            );
        }
    }

    let Some(source) = awc.streams.iter().find(|s| s.stream_format().is_some()) else {
        return tally;
    };
    let format = source.stream_format().expect("found by it");
    if !format.channels.iter().any(|c| c.codec == Codec::Mp3) {
        return tally;
    }
    let (Some(channels), Some(data)) = (
        awc.decode_channels(key, Accumulator::Clamped),
        awc.stream_data(source, key),
    ) else {
        tally
            .failed
            .push(format!("{label}: its MP3 channels do not decode"));
        return tally;
    };
    for (c, pcm) in channels.iter().enumerate() {
        tally.channels += 1;
        let n = pcm.len() / 2;
        for i in 1..n {
            tally.interior.0 += sample(pcm, i).abs_diff(sample(pcm, i - 1)) as u64;
        }
        tally.interior.1 += n.saturating_sub(1);
        for b in block_joins(&data, format.block_size as usize, c) {
            if b > 0 && b < n {
                tally.joined.0 += sample(pcm, b).abs_diff(sample(pcm, b - 1)) as u64;
                tally.joined.1 += 1;
            }
        }
    }
    tally
}

fn measure_install(root: &Path) -> Tally {
    let exe = root.join("GTA5_Enhanced.exe");
    let keys = Arc::new(extract_keys(&exe, &|_| {}).expect("keys from the install's executable"));
    let key = AwcKey(keys.awc_key);
    assert!(key.is_usable(), "{} carries no AWC key", exe.display());

    let mut tally = Tally::default();
    let mut batch: Vec<(String, Vec<u8>)> = Vec::new();
    let mut held = 0usize;
    let flush = |batch: &mut Vec<(String, Vec<u8>)>, tally: &mut Tally| {
        let measured = batch
            .par_iter()
            .map(|(label, bytes)| measure(label, bytes, key))
            .reduce(Tally::default, Tally::merge);
        *tally = std::mem::take(tally).merge(measured);
        batch.clear();
    };
    every_awc(root, &keys, &mut |label, bytes| match bytes {
        Ok(bytes) => {
            held += bytes.len();
            batch.push((label, bytes));
            if held > BATCH_BYTES {
                flush(&mut batch, &mut tally);
                held = 0;
            }
        }
        // An archive that will not open is MP3 this run did not see.
        Err(e) => tally.failed.push(format!("{label}: {e}")),
    });
    flush(&mut batch, &mut tally);
    tally
}

/// Every codec-7 stream of an Enhanced install decodes to exactly the samples
/// its `format` chunk or `streamformat` entry states, which the decode
/// refuses rather than pads. That count is something the decoder reads, so
/// two checks it does not read stand beside it: the peaks the encoder
/// recorded must fit the decode from sample 0 better than one shifted by the
/// decoder's own delay, and each multi-channel block must join its
/// channel's previous block as smoothly as neighbouring samples do anywhere.
#[test]
fn every_enhanced_mp3_stream_decodes_to_its_stated_sample_count() {
    let what = "every Enhanced MP3 stream to its stated sample count";
    let roots: Vec<PathBuf> = game_roots()
        .into_iter()
        .filter(|root| root.join("GTA5_Enhanced.exe").is_file())
        .collect();
    if roots.is_empty() {
        skip_install(
            what,
            format_args!("no Enhanced install root; set {GAME_ROOT_ENV}"),
        );
        return;
    }
    for root in roots {
        let started = std::time::Instant::now();
        let t = measure_install(&root);
        eprintln!(
            "{}: {} .awc, {} MP3 stream(s), {} MP3 channel(s) in {:.0?}; peak mean |e| {:.1} \
             from sample 0 against {:.1} delayed, over {}; mean |Δ| {:.1} anywhere and {:.1} \
             across {} block join(s)",
            root.display(),
            t.files,
            t.streams,
            t.channels,
            started.elapsed(),
            Tally::mean(t.peaks),
            Tally::mean(t.peaks_delayed),
            t.peaks.1,
            Tally::mean(t.interior),
            Tally::mean(t.joined),
            t.joined.1,
        );
        for refused in &t.refused {
            eprintln!("  not a container, so not measured: {refused}");
        }
        assert!(
            t.failed.is_empty(),
            "{}: {} failure(s), the first of them:\n{}",
            root.display(),
            t.failed.len(),
            t.failed[..t.failed.len().min(20)].join("\n")
        );
        assert!(
            t.streams > 0 && t.channels > 0,
            "{}: no MP3 found",
            root.display()
        );
        assert!(
            Tally::mean(t.peaks) * 2.0 < Tally::mean(t.peaks_delayed),
            "the recorded peaks fit a decode delayed by {DECODER_DELAY} samples nearly as \
             well, so they say nothing about where the decode starts"
        );
        assert!(
            t.joined.1 > 0 && Tally::mean(t.joined) < 2.0 * Tally::mean(t.interior),
            "channels jump by a mean of {:.1} across their block joins against {:.1} \
             anywhere, so a block does not carry on from the previous one",
            Tally::mean(t.joined),
            Tally::mean(t.interior)
        );
    }
}
