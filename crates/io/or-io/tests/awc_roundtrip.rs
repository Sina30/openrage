//! `.awc` over real bytes: does every container round-trip, and is the
//! decode right rather than merely non-erroring.
//!
//! Fixtures come from ordinary `or-test-support` discovery, which finds them
//! in the sibling corpus where they are staged. `OPENRAGE_AWC_FIXTURES`
//! overrides that and nothing needs it.
//!
//! ⚠ **They are staged in the sibling corpus and never in-tree.** One `.awc`
//! under `or-io-verify`'s `tests/fixtures` sets `has_content` and makes that
//! directory preferred for *every* format, so the thirty-odd suites reading
//! drawables and textures out of the sibling would find none and skip — and
//! a skipped suite reports green.

use std::collections::BTreeMap;
use std::path::PathBuf;

use or_io::awc::{codec, Accumulator, AwcChunkKind, AwcFile, AwcKey, Codec};
use or_test_support::Corpus;

/// A keys cache to take the AWC key out of, overriding the default location.
///
/// ⚠ **A path rather than the key itself.** The key is not in this
/// repository and must not end up in a shell history or a CI log; a cache
/// file is what a machine with a game on it already has.
const KEY_ENV: &str = "OPENRAGE_AWC_KEYS_CACHE";
const CORPUS_ENV: &str = "OPENRAGE_AWC_FIXTURES";

/// MP3 peaks' mean |error|, of 65,535, that a decode aligned to its stream's
/// first sample stays under. Over the Enhanced install it measures 857.8, and
/// the same decode shifted by the decoder's 529-sample delay 2,584.0.
const MP3_PEAK_BOUND: f64 = 2048.0;

fn corpus() -> Vec<PathBuf> {
    if let Some(raw) = std::env::var_os(CORPUS_ENV) {
        let root = PathBuf::from(&raw);
        assert!(
            root.is_dir(),
            "{CORPUS_ENV} is set to `{}`, which is not a directory. Every check below \
             would have skipped and reported success.",
            root.display()
        );
        return Corpus::with_root(root, "awc").all_targets().find();
    }
    Corpus::with_root(
        or_test_support::shared_corpus_root(env!("CARGO_MANIFEST_DIR")),
        "awc",
    )
    .all_targets()
    .find()
}

/// Where the editor and the CLI already keep a keys cache — the one a
/// machine with a game on it has without being told.
///
/// ⚠ **Defaulting to it is what lets the corpus-bearing machine exercise the
/// decode at all.** Measured over the 77 staged fixtures: every stream that
/// declares a peak sits in a container whose bytes are scrambled, so a run
/// with no key reproduces **nothing**. Read-only, and `KEY_ENV` overrides
/// it.
fn default_keys_cache() -> Option<PathBuf> {
    // The app's own directory, found without a dependency from a test: `OPENRAGE_CONFIG_DIR`, else
    // what `dirs::config_dir()` gives, which is `%APPDATA%`, `~/Library/Application Support` on a
    // Mac and `$XDG_CONFIG_HOME` or `~/.config` elsewhere.
    let dir = std::env::var_os("OPENRAGE_CONFIG_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let base = if cfg!(windows) {
                std::env::var_os("APPDATA").map(PathBuf::from)
            } else if cfg!(target_os = "macos") {
                std::env::var_os("HOME")
                    .map(|h| PathBuf::from(h).join("Library/Application Support"))
            } else {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            };
            base.map(|d| d.join("openrage"))
        });
    dir.map(|d| d.join("gta5_keys.bin")).filter(|p| p.is_file())
}

/// The AWC key this run has, or the reason it has none.
///
/// ⚠ No cache and a cache from an older schema that does not decode are the
/// same state, no AWC key, and the reason only names which for the skip.
/// Failing on a stale cache panics on exactly the machines that have a game to
/// exercise this suite.
struct KeyLookup {
    key: AwcKey,
    /// Why there is no key, for the ledger. `None` when there is one.
    absent: Option<String>,
}

impl KeyLookup {
    fn absent(why: impl Into<String>) -> Self {
        Self {
            key: AwcKey::default(),
            absent: Some(why.into()),
        }
    }

    /// What to record when nothing was decoded, naming the key as the cause
    /// where it is one rather than blaming the corpus for it.
    fn why_nothing_decoded(&self, otherwise: &str) -> String {
        match &self.absent {
            Some(why) => format!("{why}, so no stream could be descrambled to decode"),
            None => otherwise.to_string(),
        }
    }
}

fn lookup_key() -> KeyLookup {
    let explicit = std::env::var_os(KEY_ENV).map(PathBuf::from);
    // ⚠ A path that names nothing stays a failure, and it is the one thing
    // here that is not "no key": somebody asked for a specific cache and
    // got silence. Every failure *reading* the file it names is tolerated
    // below.
    if let Some(path) = explicit.as_ref() {
        assert!(
            path.is_file(),
            "{KEY_ENV} names {}, which is not there. Every obfuscated container below \
             would be refused and the run would look like a corpus with none in it.",
            path.display()
        );
    }
    let Some(path) = explicit.or_else(default_keys_cache) else {
        return KeyLookup::absent(format!(
            "no keys cache at the default location and {KEY_ENV} is unset"
        ));
    };
    let shown = path.display();

    // An older schema is a state rather than damage — it re-extracts the
    // next time anything asks for keys — so every outcome below reads as
    // "no key" and none of them as a failure.
    let cache = match or_io::container::KeysCache::read_from(&path) {
        Ok(Some(cache)) => cache,
        Ok(None) => return KeyLookup::absent(format!("the keys cache at {shown} is not there")),
        Err(e) => {
            return KeyLookup::absent(format!(
                "the keys cache at {shown} will not read ({e}), which is what an older \
                 schema looks like; it re-extracts the next time anything asks for keys"
            ))
        }
    };
    let keys = match cache.into_keys() {
        Ok(keys) => keys,
        Err(e) => {
            return KeyLookup::absent(format!("the keys cache at {shown} will not decode ({e})"))
        }
    };
    let key = AwcKey(keys.awc_key);
    if !key.is_usable() {
        return KeyLookup::absent(format!(
            "the keys cache at {shown} reads but carries no AWC key, so it predates the \
             AWC word being extracted"
        ));
    }
    KeyLookup { key, absent: None }
}

fn key() -> AwcKey {
    lookup_key().key
}

/// What one pass over the corpus measured.
#[derive(Default)]
struct Tally {
    files: usize,
    parsed: usize,
    /// Containers that could not be opened at all, with the reason.
    refused: Vec<String>,
    /// Containers refused only because no key was supplied.
    needed_a_key: usize,
    byte_identical: usize,
    unaccounted: usize,
    /// Peak checks, by codec: (checked, exact, total absolute error).
    pcm: Check,
    adpcm: Check,
    mp3: Check,
    /// Channels of multi-channel containers, de-interleaved and measured.
    channels: Continuity,
    /// Streams in a container that parsed and whose samples were **not**
    /// checked, by the reason, so the pass line never reads as full coverage.
    unchecked: BTreeMap<&'static str, usize>,
}

/// How smoothly de-interleaved channels run on across their block
/// boundaries, against how smoothly they run anywhere.
///
/// ⚠ **No container states a peak for a multi-channel stream's channels**:
/// its channel streams carry no `format` or `peak` chunk and `streamformat`
/// has no peak field. What the decoder does not read is the signal's own
/// continuity: a block joined to the next block of the same channel moves by
/// about as much as any two neighbouring samples, and one joined to another
/// channel's does not. A swap made identically in every block only relabels
/// the channels, which nothing but the ids in `streamformat` names.
#[derive(Default)]
struct Continuity {
    decoded: usize,
    /// |Δ| between neighbouring samples anywhere: (sum, count).
    interior: (u64, usize),
    /// |Δ| across each block boundary, within one channel.
    joined: (u64, usize),
    /// |Δ| across the same boundaries, into the next channel's block.
    swapped: u64,
}

impl Continuity {
    fn note(&mut self, channels: &[Vec<u8>], block_starts: &[usize]) {
        let at = |pcm: &[u8], i: usize| i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]) as i32;
        self.decoded += channels.len();
        for (c, pcm) in channels.iter().enumerate() {
            let n = pcm.len() / 2;
            for i in 1..n {
                self.interior.0 += at(pcm, i).abs_diff(at(pcm, i - 1)) as u64;
            }
            self.interior.1 += n.saturating_sub(1);
            let next = &channels[(c + 1) % channels.len()];
            for &b in block_starts.iter().filter(|&&b| b > 0 && b < n) {
                self.joined.0 += at(pcm, b).abs_diff(at(pcm, b - 1)) as u64;
                self.joined.1 += 1;
                self.swapped += at(next, b).abs_diff(at(pcm, b - 1)) as u64;
            }
        }
    }

    fn interior_mean(&self) -> f64 {
        self.interior.0 as f64 / self.interior.1.max(1) as f64
    }
    fn joined_mean(&self) -> f64 {
        self.joined.0 as f64 / self.joined.1.max(1) as f64
    }
    fn swapped_mean(&self) -> f64 {
        self.swapped as f64 / self.joined.1.max(1) as f64
    }
}

#[derive(Default)]
struct Check {
    checked: usize,
    exact: usize,
    total_error: u64,
    worst: u32,
}

impl Check {
    fn note(&mut self, declared: u16, decoded: u16) {
        self.checked += 1;
        let error = (declared as i32 - decoded as i32).unsigned_abs();
        if error == 0 {
            self.exact += 1;
        }
        self.total_error += error as u64;
        self.worst = self.worst.max(error);
    }

    fn mean(&self) -> f64 {
        if self.checked == 0 {
            0.0
        } else {
            self.total_error as f64 / self.checked as f64
        }
    }
}

fn walk(accumulator: Accumulator) -> Tally {
    let key = key();
    let mut tally = Tally::default();
    for path in corpus() {
        tally.files += 1;
        let bytes = std::fs::read(&path).expect("a corpus file reads");
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        let awc = match AwcFile::parse(&bytes, key) {
            Ok(a) => a,
            Err(e) => {
                let message = e.to_string();
                if !key.is_usable() && message.contains("no AWC key is available") {
                    tally.needed_a_key += 1;
                } else {
                    tally.refused.push(format!("{name}: {message}"));
                }
                continue;
            }
        };
        tally.parsed += 1;
        tally.unaccounted += awc.unaccounted_bytes();
        if awc.to_bytes(key).expect("rewrite") == bytes {
            tally.byte_identical += 1;
        }

        // The source stream and every channel stream it names are measured
        // here as one, rather than stream by stream below.
        let mut multi_channel = Vec::new();
        if let Some(source) = awc.streams.iter().find(|s| s.stream_format().is_some()) {
            let format = source.stream_format().expect("found by it");
            multi_channel.push(source.id);
            multi_channel.extend(format.channels.iter().map(|c| c.id));
            let block_starts: Vec<usize> = source
                .chunk(AwcChunkKind::SeekTable)
                .map(|t| {
                    t.payload
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|w| u32::from_le_bytes(*w) as usize)
                        .collect()
                })
                .unwrap_or_default();
            match awc.decode_channels(key, accumulator) {
                Some(channels) => tally.channels.note(&channels, &block_starts),
                None => {
                    let why = if format
                        .channels
                        .iter()
                        .all(|c| !matches!(c.codec, Codec::Unknown(_) | Codec::Xma))
                    {
                        "a multi-channel stream whose data could not be descrambled"
                    } else {
                        "a multi-channel stream in a codec with no decoder here"
                    };
                    *tally.unchecked.entry(why).or_default() += format.channels.len();
                }
            }
        }

        for stream in awc
            .streams
            .iter()
            .filter(|s| !multi_channel.contains(&s.id))
        {
            let mut skipped = |why: &'static str| {
                *tally.unchecked.entry(why).or_default() += 1;
            };
            let Some(format) = stream.format() else {
                skipped("no format chunk");
                continue;
            };
            if format.peak_value().is_none() {
                skipped("no declared peak, so there is nothing to check the decode against");
                continue;
            }
            if matches!(format.codec, Codec::Unknown(_) | Codec::Xma | Codec::Opus) {
                skipped("a codec with no decoder here");
                continue;
            }
            let (Some(declared), Some(pcm)) = (
                format.peak_value(),
                awc.decode_stream(stream, key, accumulator),
            ) else {
                skipped("its data could not be descrambled");
                continue;
            };
            let slot = match format.codec {
                Codec::Pcm16 => &mut tally.pcm,
                Codec::ImaAdpcm => &mut tally.adpcm,
                Codec::Mp3 => &mut tally.mp3,
                // Refused above, so this is unreachable. It stays explicit
                // so adding a decoder is a compile error here rather than a
                // silently unmeasured codec.
                Codec::Xma | Codec::Opus | Codec::Unknown(_) => continue,
            };
            slot.note(declared, codec::peak_envelope(&pcm, 1)[0]);
            if let Some(envelope) = stream.peak_envelope() {
                let windows = codec::peak_window_count(format.samples as usize);
                if windows > 0 && envelope.len() == windows {
                    let ours = codec::peak_envelope(&pcm, windows + 1);
                    for (i, want) in envelope.iter().enumerate() {
                        slot.note(*want, ours[i + 1]);
                    }
                }
            }
        }
    }
    tally
}

/// The one place the corpus is required, so every check below either runs on
/// real bytes or records a skip rather than passing on an empty loop.
fn require_a_corpus(what: &str) -> Option<Tally> {
    let tally = walk(Accumulator::Clamped);
    if tally.files == 0 {
        or_test_support::skip_corpus(
            what,
            format_args!(
                "no .awc fixtures; point {CORPUS_ENV} at a directory of them, or stage \
                 them in the sibling corpus"
            ),
        );
        return None;
    }
    assert!(
        tally.refused.is_empty(),
        "{} of {} container(s) would not open, so this suite would have measured only \
         the rest:\n  {}",
        tally.refused.len(),
        tally.files,
        tally.refused.join("\n  ")
    );
    Some(tally)
}

#[test]
fn every_container_in_the_corpus_opens_and_accounts_for_all_of_its_bytes() {
    let Some(tally) = require_a_corpus("awc: the container reads") else {
        return;
    };

    eprintln!(
        "awc corpus: {} file(s), {} parsed, {} needed a key this run had not",
        tally.files, tally.parsed, tally.needed_a_key
    );
    assert!(
        tally.parsed > 0,
        "{} file(s) found and none opened",
        tally.files
    );

    // ⚠ The only verification a container with no checksum admits of. Every
    // byte is header, table, payload or padding; a reader that leaves some
    // over has understood part of the file and reported success.
    assert_eq!(
        tally.unaccounted, 0,
        "{} byte(s) across {} container(s) belong to no chunk",
        tally.unaccounted, tally.parsed
    );
}

#[test]
fn a_container_that_parsed_writes_back_the_bytes_it_came_from() {
    let Some(tally) = require_a_corpus("awc: the write path round-trips") else {
        return;
    };
    assert_eq!(
        tally.byte_identical, tally.parsed,
        "{} of {} container(s) did not reproduce their own bytes",
        tally.byte_identical, tally.parsed
    );
    assert!(tally.parsed > 0, "nothing was exercised");
}

/// Every stream's `format` chunk may carry the first window of a peak
/// envelope, and its `peak` chunk carries the rest — one `u16` per 4,096
/// samples, each `min(2·|sample|, 65535)`. Rockstar's encoder measured those
/// over the samples it was given and nothing in the decode path reads them, so
/// reproducing them is evidence about the decode.
///
/// **PCM is lossless, so it must be exact**, and an exact envelope also pins
/// the container parse, both encryption layers, the chunk offsets and the
/// sample counts — any error in any of them moves a sample and moves a peak.
///
/// **ADPCM is lossy and the encoder measured the source**, so only the
/// residual's size is evidence:
/// `a_wrong_decoder_misses_the_peaks_by_far_more_than_the_codec_does`
/// measures what a wrong decoder costs.
#[test]
fn a_stream_reproduces_the_peaks_its_own_encoder_recorded() {
    let Some(tally) = require_a_corpus("awc: the decode matches the recorded peaks") else {
        return;
    };

    eprintln!(
        "awc peaks: PCM {} of {} exact (worst {}); ADPCM {} of {} exact, mean |e| {:.1}, \
         worst {}; MP3 mean |e| {:.1} over {}",
        tally.pcm.exact,
        tally.pcm.checked,
        tally.pcm.worst,
        tally.adpcm.exact,
        tally.adpcm.checked,
        tally.adpcm.mean(),
        tally.adpcm.worst,
        tally.mp3.mean(),
        tally.mp3.checked,
    );

    // Streams whose samples nothing here reads go to the ledger; the pass line
    // above counts only those reproduced.
    for (why, count) in &tally.unchecked {
        or_test_support::skip_corpus(
            "awc: streams whose decode is not checked",
            format_args!("{count} stream(s): {why}"),
        );
    }

    // ⚠ With no peak-bearing stream every assertion below is `0 == 0`, so a
    // vacuous pass fails — except with no AWC key, where every staged fixture
    // that declares a peak is scrambled and nothing could have been checked;
    // that skips, naming which absence.
    let lookup = lookup_key();
    if tally.pcm.checked + tally.adpcm.checked == 0 {
        if let Some(why) = &lookup.absent {
            or_test_support::skip_corpus(
                "awc: the decode matches the recorded peaks",
                format_args!(
                    "{why}. {} of {} container(s) would not open at all and every \
                     peak-bearing stream in the rest is scrambled, so nothing here \
                     could have been checked. Open a game once, or point {KEY_ENV} at \
                     a keys cache carrying an AWC key.",
                    tally.needed_a_key, tally.files
                ),
            );
            return;
        }
    }
    assert!(
        tally.pcm.checked + tally.adpcm.checked > 0,
        "{} container(s) parsed with an AWC key available and not one peak was \
         checked, so this test proved nothing about the decode",
        tally.parsed
    );

    if tally.pcm.checked > 0 {
        assert_eq!(
            tally.pcm.exact,
            tally.pcm.checked,
            "PCM is lossless, so every one of its {} declared peak(s) must be \
             reproduced exactly; {} were not, worst by {}",
            tally.pcm.checked,
            tally.pcm.checked - tally.pcm.exact,
            tally.pcm.worst
        );
    }

    if tally.adpcm.checked > 0 {
        // Measured over the whole Legacy install by `openrage-cli awc census`:
        // mean |e| 710.5 on the format peak and 590.1 across the peak chunk,
        // out of a 65,535 scale. A wrong decoder measures in the tens of
        // thousands.
        assert!(
            tally.adpcm.mean() < 4096.0,
            "ADPCM peaks are out by a mean of {:.1} of 65535, which is far more than \
             the codec's own quantisation; the decode is wrong, not lossy",
            tally.adpcm.mean()
        );
    }

    if tally.mp3.checked > 0 {
        // `every_enhanced_mp3_stream_decodes_to_its_stated_sample_count`
        // measures the whole Enhanced install and what a misaligned decode
        // costs there.
        assert!(
            tally.mp3.mean() < MP3_PEAK_BOUND,
            "MP3 peaks are out by a mean of {:.1} of 65535, above the {MP3_PEAK_BOUND} \
             a decode aligned to the stream's first sample stays under",
            tally.mp3.mean()
        );
    }
}

/// The falsification of the ADPCM bound: the same corpus decoded with the
/// nibble order reversed — the most likely way to get IMA ADPCM wrong — must
/// miss the peaks by an order of magnitude more.
#[test]
fn a_wrong_decoder_misses_the_peaks_by_far_more_than_the_codec_does() {
    let Some(right) = require_a_corpus("awc: the peak check can fail") else {
        return;
    };
    if right.adpcm.checked == 0 {
        or_test_support::skip_corpus(
            "awc: the peak check can fail",
            lookup_key()
                .why_nothing_decoded("no ADPCM stream in the corpus carries a declared peak"),
        );
        return;
    }

    let wrong = wrong_decoder_tally();
    eprintln!(
        "awc falsification: correct mean |e| {:.1}, nibble-swapped {:.1}, over {} check(s)",
        right.adpcm.mean(),
        wrong.mean(),
        wrong.checked
    );
    assert_eq!(
        wrong.checked, right.adpcm.checked,
        "the two runs must check the same population"
    );
    assert!(
        wrong.mean() > right.adpcm.mean() * 10.0,
        "a decoder with its nibbles reversed is out by a mean of only {:.1} against the \
         correct decoder's {:.1}, so the peak check cannot tell them apart and proves \
         nothing",
        wrong.mean(),
        right.adpcm.mean()
    );
}

/// Every multi-channel container de-interleaves, and each channel runs on
/// across its block boundaries as smoothly as it does anywhere, while the
/// same boundaries joined into the neighbouring channel do not: that is the
/// decoder that takes one channel's run for another's, and it must fail.
#[test]
fn a_multi_channel_container_de_interleaves_into_channels_that_run_on_across_blocks() {
    let Some(tally) = require_a_corpus("awc: multi-channel de-interleaving") else {
        return;
    };
    let lookup = lookup_key();
    if tally.channels.decoded == 0 {
        or_test_support::skip_corpus(
            "awc: multi-channel de-interleaving",
            lookup.why_nothing_decoded("no multi-channel container in the corpus"),
        );
        return;
    }
    let c = &tally.channels;
    eprintln!(
        "awc channels: {} decoded; mean |Δ| {:.1} anywhere, {:.1} across {} block \
         boundaries, {:.1} into the next channel",
        c.decoded,
        c.interior_mean(),
        c.joined_mean(),
        c.joined.1,
        c.swapped_mean()
    );
    assert!(c.joined.1 > 0, "no block boundary was measured");
    // Twice the mean step anywhere: a join within one channel measures near
    // one step, a join into another channel's audio several.
    let bound = 2.0 * c.interior_mean();
    assert!(
        c.joined_mean() < bound,
        "channels jump by a mean of {:.1} across their block boundaries against {:.1} \
         anywhere, so the blocks are not joined to their own channel",
        c.joined_mean(),
        c.interior_mean()
    );
    assert!(
        c.swapped_mean() > bound,
        "joining each block to the next channel's moves by only {:.1}, within the \
         bound of {bound:.1}, so this check cannot tell a swapped channel apart",
        c.swapped_mean()
    );
}

/// The same walk with one deliberate defect: high nibble before low.
fn wrong_decoder_tally() -> Check {
    let key = key();
    let mut check = Check::default();
    for path in corpus() {
        let bytes = std::fs::read(&path).expect("a corpus file reads");
        let Ok(awc) = AwcFile::parse(&bytes, key) else {
            continue;
        };
        for stream in &awc.streams {
            let Some(format) = stream.format() else {
                continue;
            };
            if format.codec != Codec::ImaAdpcm {
                continue;
            }
            let (Some(declared), Some(data)) = (format.peak_value(), awc.stream_data(stream, key))
            else {
                continue;
            };
            let swapped: Vec<u8> = data.iter().map(|b| b.rotate_left(4)).collect();
            let pcm = codec::decode_adpcm(&swapped, format.samples as usize, Accumulator::Clamped);
            check.note(declared, codec::peak_envelope(&pcm, 1)[0]);
            if let Some(envelope) = stream.peak_envelope() {
                let windows = codec::peak_window_count(format.samples as usize);
                if windows > 0 && envelope.len() == windows {
                    let ours = codec::peak_envelope(&pcm, windows + 1);
                    for (i, want) in envelope.iter().enumerate() {
                        check.note(*want, ours[i + 1]);
                    }
                }
            }
        }
    }
    check
}

/// Stock IMA ADPCM clamps the running predictor; the reference C#
/// implementation for this format clamps only what it emits. The two agree
/// until a stream saturates, so which is right is a question about the
/// shipped data rather than about the specification, and a corpus that never
/// saturates cannot tell them apart.
#[test]
fn the_two_predictor_models_are_told_apart_by_the_corpus_or_reported_as_indistinguishable() {
    let Some(clamped) = require_a_corpus("awc: which predictor model the data prefers") else {
        return;
    };
    if clamped.adpcm.checked == 0 {
        or_test_support::skip_corpus(
            "awc: which predictor model the data prefers",
            lookup_key()
                .why_nothing_decoded("no ADPCM stream in the corpus carries a declared peak"),
        );
        return;
    }
    let unclamped = walk(Accumulator::Unclamped);

    eprintln!(
        "awc predictor: clamped mean |e| {:.1}, unclamped {:.1}, over {} check(s)",
        clamped.adpcm.mean(),
        unclamped.adpcm.mean(),
        clamped.adpcm.checked
    );
    assert_eq!(
        clamped.adpcm.checked, unclamped.adpcm.checked,
        "the two models must be measured over the same population"
    );
    // Either the data prefers the default, or it cannot see the difference.
    // What must not happen is the default being measurably worse.
    assert!(
        clamped.adpcm.mean() <= unclamped.adpcm.mean() + f64::EPSILON,
        "the unclamped predictor fits the recorded peaks better ({:.3} against {:.3}), \
         so the default is the wrong one",
        unclamped.adpcm.mean(),
        clamped.adpcm.mean()
    );
}

/// How far ahead of the same audio in ADPCM an Enhanced MP3 stream decodes, in samples.
const MP3_LEAD: i64 = 47;

/// ⚠ **The shipped MP3 streams have their priming frame dropped.** Every stream the staged
/// Enhanced containers share with their Legacy twins decodes exactly 47 samples ahead of the
/// ADPCM stream of the same hash, which is the 1,152 a frame holds less the 1,105 an encoder and
/// decoder delay a signal by. `encode_mp3` drops its own for the same reason.
#[test]
fn an_enhanced_mp3_stream_decodes_47_samples_ahead_of_its_legacy_adpcm_twin() {
    let what = "awc: MP3 against its ADPCM twin";
    let lookup = lookup_key();
    let key = lookup.key;
    let mut lags: BTreeMap<i64, usize> = BTreeMap::new();
    for path in corpus() {
        let in_gen9 = path
            .parent()
            .and_then(|d| d.file_name())
            .is_some_and(|n| n == "gen9");
        let twin = path
            .parent()
            .and_then(|d| d.parent())
            .zip(path.file_name())
            .map(|(dir, name)| dir.join(name));
        let (true, Some(twin)) = (in_gen9, twin.filter(|t| t.is_file())) else {
            continue;
        };
        let read = |p: &PathBuf| std::fs::read(p).expect("a corpus file reads");
        let (Ok(enhanced), Ok(legacy)) = (
            AwcFile::parse(&read(&path), key),
            AwcFile::parse(&read(&twin), key),
        ) else {
            continue;
        };
        for stream in &enhanced.streams {
            let Some(mp3) = stream.format().filter(|f| f.codec == Codec::Mp3) else {
                continue;
            };
            let Some(adpcm) = legacy.streams.iter().find(|s| {
                s.id == stream.id && s.format().is_some_and(|f| f.codec == Codec::ImaAdpcm)
            }) else {
                continue;
            };
            let (Some(a), Some(m)) = (
                legacy.decode_stream(adpcm, key, Accumulator::Clamped),
                enhanced.decode_stream(stream, key, Accumulator::Clamped),
            ) else {
                continue;
            };
            let samples = |pcm: &[u8]| -> Vec<f64> {
                pcm.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| f64::from(i16::from_le_bytes([b[0], b[1]])))
                    .collect()
            };
            let (a, m) = (samples(&a), samples(&m));
            let from = 3000usize;
            let to = 9000usize.min(a.len().min(m.len()).saturating_sub(1300));
            if mp3.samples < 12_000 || to <= from {
                continue;
            }
            let best = (-1200i64..=1200)
                .map(|lag| {
                    let (mut dot, mut ea, mut em) = (0.0, 0.0, 0.0);
                    let shifted = &m[(from as i64 + lag) as usize..(to as i64 + lag) as usize];
                    for (x, y) in a[from..to].iter().zip(shifted) {
                        dot += x * y;
                        ea += x * x;
                        em += y * y;
                    }
                    (dot / ((ea * em).sqrt() + 1e-9), lag)
                })
                .max_by(|x, y| x.0.total_cmp(&y.0))
                .expect("lags");
            if best.0 > 0.9 {
                *lags.entry(best.1).or_default() += 1;
            }
        }
    }
    if lags.is_empty() {
        or_test_support::skip_corpus(
            what,
            lookup.why_nothing_decoded("no Enhanced container has a Legacy twin to compare with"),
        );
        return;
    }
    eprintln!("awc MP3 lead: streams by lag against their ADPCM twin: {lags:?}");
    assert_eq!(
        lags.keys().copied().collect::<Vec<_>>(),
        vec![-MP3_LEAD],
        "the MP3 streams do not all decode {MP3_LEAD} samples ahead of their ADPCM twin"
    );
}
