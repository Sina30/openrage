//! `openrage-cli awc`: read a wave container, export a stream, census an
//! install's worth of them, and find which of them holds a sound.
//!
//! The census — which codecs an install uses, how many containers are
//! obfuscated, whether Legacy and Enhanced differ — is a fold, so one file's
//! bytes are live at a time.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use or_depends::audio::{AudioIndex, Filter, Resolution};
use or_io::awc::{codec, ogg, wav, Accumulator, AwcFile, AwcKey, Codec};
use or_media::Decoders;
use or_title::install::crawl::{self, ControlFlow};

use super::keys::resolve_crypto;
use super::search::resolve_game_root;

pub fn run(args: &[String]) -> ExitCode {
    let Some((verb, rest)) = args.split_first() else {
        print_usage();
        return ExitCode::from(2);
    };
    match verb.as_str() {
        "-h" | "--help" | "help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        "info" => info(rest),
        "census" => census(rest),
        "export" => export(rest),
        "replace" => replace(rest),
        "which" => which(rest),
        "index" => index(rest),
        "decode-check" => super::awc_decode::run(rest),
        other => {
            eprintln!("Error: unknown subcommand: {other}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

/// Options shared by every subcommand that has to find an install's keys.
struct Common {
    game: Option<PathBuf>,
    keys: Option<PathBuf>,
    positional: Vec<String>,
    json: bool,
    accumulator: Accumulator,
    limit: Option<usize>,
    raw: bool,
    /// (export) A GTA VI Opus stream decoded to WAV rather than written as Ogg Opus.
    wav: bool,
    /// (export) The listing `rpf scan-dialogue --out` wrote, whose lines become comments.
    dialogue: Option<String>,
    filter: Filter,
}

fn parse_common(args: &[String]) -> Result<Common, String> {
    let mut out = Common {
        game: None,
        keys: None,
        positional: Vec::new(),
        json: false,
        accumulator: Accumulator::Clamped,
        limit: None,
        raw: false,
        wav: false,
        dialogue: None,
        filter: Filter::default(),
    };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg, None),
        };
        let value = |i: &mut usize| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            *i += 1;
            args.get(*i)
                .cloned()
                .ok_or_else(|| format!("{key} needs a value"))
        };
        match key {
            "--json" => out.json = true,
            "--raw" => out.raw = true,
            "--wav" => out.wav = true,
            "--dialogue" => out.dialogue = Some(value(&mut i)?),
            "--unclamped" => out.accumulator = Accumulator::Unclamped,
            "--game" => out.game = Some(PathBuf::from(value(&mut i)?)),
            "--keys" => out.keys = Some(PathBuf::from(value(&mut i)?)),
            "--container" => out.filter.container = Some(value(&mut i)?),
            "--class" => out.filter.class = Some(value(&mut i)?),
            "--limit" => {
                out.limit = Some(
                    value(&mut i)?
                        .parse()
                        .map_err(|_| "--limit needs a number".to_string())?,
                )
            }
            other if other.starts_with("--") => return Err(format!("unknown option: {other}")),
            other => out.positional.push(other.to_string()),
        }
        i += 1;
    }
    Ok(out)
}

fn info(args: &[String]) -> ExitCode {
    let common = match parse_common(args) {
        Ok(c) => c,
        Err(e) => return usage_error(&e),
    };
    let Some(path) = common.positional.first() else {
        return usage_error("info needs a file path");
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error: read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let key = key_for(&common);
    let awc = match AwcFile::parse(&bytes, key) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };
    print_info(path, &awc);
    ExitCode::SUCCESS
}

/// What `awc info` prints of a container, under the name `name`.
pub(crate) fn print_info(name: &str, awc: &AwcFile) {
    println!("{name}");
    println!(
        "  version {}  flags 0x{:04X}{}{}{}{}",
        awc.version,
        awc.flags,
        if awc.whole_file_encrypted {
            "  whole-file-encrypted"
        } else {
            ""
        },
        if awc.has_chunk_indices() {
            "  chunk-indices"
        } else {
            ""
        },
        if awc.multi_channel() {
            "  multi-channel"
        } else {
            ""
        },
        if awc.single_channel_encrypted() || awc.multi_channel_encrypted() {
            "  data-encrypted"
        } else {
            ""
        },
    );
    println!(
        "  {} stream(s), {} byte(s), {} unaccounted",
        awc.streams.len(),
        awc.plaintext_len(),
        awc.unaccounted_bytes()
    );
    for (at, stream) in awc.streams.iter().enumerate() {
        let chunks: Vec<String> = stream.chunks.iter().map(|c| c.kind.label()).collect();
        match stream.opus_format() {
            Ok(Some(format)) => {
                let looped = match format.loop_point {
                    -1 => String::new(),
                    at => format!("loop point {at} (a role inferred from its values)"),
                };
                println!(
                    "  {:08X}  {} [{}]",
                    stream.id,
                    opus_facts(format.samples_per_channel, format.channels, &looped),
                    chunks.join(" ")
                );
                continue;
            }
            Err(e) => {
                println!("  {:08X}  [{}]", stream.id, chunks.join(" "));
                println!("            {e}");
                continue;
            }
            Ok(None) => {}
        }
        if let Ok(Some(f)) = stream.coded_format() {
            println!(
                "  {:08X}  {:>9} smp  {:>5} Hz  {:<8} peak {:<6} [{}]",
                stream.id,
                f.samples_per_channel,
                f.stated_rate,
                f.codec.label(),
                f.peak,
                chunks.join(" ")
            );
            continue;
        }
        match stream.format() {
            Some(format) => println!(
                "  {:08X}  {:>9} smp  {:>5} Hz  {:<8} peak {:<6} [{}]",
                stream.id,
                format.samples,
                format.samples_per_second,
                format.codec.label(),
                format
                    .peak_value()
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "—".into()),
                chunks.join(" ")
            ),
            None => println!("  {:08X}  [{}]", stream.id, chunks.join(" ")),
        }
        if let Some(sf) = stream.stream_format() {
            println!(
                "            streamformat: {} channel(s), {} block(s) of {} bytes",
                sf.channels.len(),
                sf.block_count,
                sf.block_size
            );
            match awc.opus_streams(at) {
                Ok(groups) => {
                    for g in groups {
                        println!(
                            "            {:08X}  {}",
                            g.id,
                            opus_facts(g.samples, g.channels, "")
                        );
                    }
                }
                Err(e) => println!("            {e}"),
            }
        }
    }
}

/// An Opus stream's samples per channel, rate, channels and duration, as `awc info` prints them.
fn opus_facts(samples: u32, channels: u8, looped: &str) -> String {
    format!(
        "{samples:>9} smp  48000 Hz  Opus     {channels} ch  {:.3} s  {looped}",
        f64::from(samples) / 48_000.0
    )
    .trim_end()
    .to_string()
}

fn export(args: &[String]) -> ExitCode {
    let common = match parse_common(args) {
        Ok(c) => c,
        Err(e) => return usage_error(&e),
    };
    let (Some(path), Some(out_dir)) = (common.positional.first(), common.positional.get(1)) else {
        return usage_error("export needs <file.awc> <out_dir>");
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error: read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let key = key_for(&common);
    let awc = match AwcFile::parse(&bytes, key) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        eprintln!("Error: create {out_dir}: {e}");
        return ExitCode::FAILURE;
    }

    let comments = match common
        .dialogue
        .as_deref()
        .map(super::gta6_uidata::dialogue_comments)
    {
        Some(Err(e)) => return usage_error(&e),
        Some(Ok(comments)) => comments,
        None => Comments::new(),
    };
    let table = Decoders::with_accumulator(common.accumulator);
    let out = std::path::Path::new(out_dir);
    let (written, skipped) = export_each(&awc, key, common.raw, common.wav, &table, out, &comments);
    println!("wrote {written} file(s) into {out_dir}");
    for why in &skipped {
        println!("  skipped {why}");
    }
    ExitCode::SUCCESS
}

/// Each stream of `awc` written into `out_dir` as `awc export` writes it: the files written, and each stream skipped with why.
pub(crate) fn export_streams(
    awc: &AwcFile,
    key: AwcKey,
    raw: bool,
    table: &Decoders,
    out_dir: &std::path::Path,
) -> (usize, Vec<String>) {
    export_each(awc, key, raw, false, table, out_dir, &Comments::new())
}

/// The comments an exported Ogg Opus stream carries, by its stream id.
type Comments = BTreeMap<u32, Vec<(String, String)>>;

/// [`export_streams`], a GTA VI Opus stream decoded to one WAV where `opus_wav` asks for it.
fn export_each(
    awc: &AwcFile,
    key: AwcKey,
    raw: bool,
    opus_wav: bool,
    table: &Decoders,
    out_dir: &std::path::Path,
    comments: &Comments,
) -> (usize, Vec<String>) {
    let mut written = 0usize;
    let mut skipped = Vec::new();
    for (at, stream) in awc.streams.iter().enumerate() {
        // GTA VI's Opus is written as it is stored, one file per stream or per channel.
        match awc.opus_streams(at) {
            Ok(groups) if !groups.is_empty() && opus_wav && !raw => {
                let file = out_dir.join(format!("{:08X}.wav", stream.id));
                match decoded(awc, key, at, table) {
                    Ok(wave) => match std::fs::write(&file, &wave) {
                        Ok(()) => written += 1,
                        Err(e) => skipped.push(format!("{}: {e}", file.display())),
                    },
                    Err(why) => skipped.push(format!("{:08X}: {why}", stream.id)),
                }
                continue;
            }
            Ok(groups) if !groups.is_empty() && !raw => {
                for group in groups {
                    let file = out_dir.join(format!("{:08X}.opus", group.id));
                    let named = comments.get(&group.id).map_or(&[][..], Vec::as_slice);
                    let bytes = match ogg::ogg_opus_tagged(&group, named) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            skipped.push(format!("{:08X}: {e}", group.id));
                            continue;
                        }
                    };
                    match std::fs::write(&file, bytes) {
                        Ok(()) => written += 1,
                        Err(e) => skipped.push(format!("{}: {e}", file.display())),
                    }
                }
                continue;
            }
            Err(e) => {
                skipped.push(format!("{:08X}: {e}", stream.id));
                continue;
            }
            Ok(_) => {}
        }
        // A multi-channel container's blocks are one stream, exported interleaved.
        if stream
            .stream_format()
            .is_some_and(|f| !f.channels.is_empty())
            && !raw
        {
            let file = out_dir.join(format!("{:08X}.wav", stream.id));
            match decoded(awc, key, at, table) {
                Ok(wave) => match std::fs::write(&file, &wave) {
                    Ok(()) => written += 1,
                    Err(e) => skipped.push(format!("{}: {e}", file.display())),
                },
                Err(why) => skipped.push(format!("{:08X}: {why}", stream.id)),
            }
            continue;
        }
        // GTA VI states a PCM16 or XMA stream's format in the record an Opus stream has.
        let coded = stream.coded_format().ok().flatten().map(|f| f.codec);
        let Some(codec) = stream.format().map(|f| f.codec).or(coded) else {
            skipped.push(format!("{:08X}: no format chunk", stream.id));
            continue;
        };
        // ⚠ `--raw` writes the descrambled payload rather than samples, for
        // a codec nothing here decodes. Enhanced puts 207,436 of its
        // 254,731 streams behind codec byte 7, so the alternative to this
        // is that an install's audio is unreachable rather than merely
        // undecoded.
        if raw {
            let Some(data) = awc.stream_data(stream, key) else {
                skipped.push(format!("{:08X}: could not be descrambled", stream.id));
                continue;
            };
            let file = out_dir.join(format!(
                "{:08X}.{}.bin",
                stream.id,
                codec.label().to_ascii_lowercase()
            ));
            match std::fs::write(&file, &data) {
                Ok(()) => written += 1,
                Err(e) => skipped.push(format!("{}: {e}", file.display())),
            }
            continue;
        }
        let wave = match decoded(awc, key, at, table) {
            Ok(wave) => wave,
            Err(why) => {
                skipped.push(format!("{:08X}: {why}", stream.id));
                continue;
            }
        };
        let file = out_dir.join(format!("{:08X}.wav", stream.id));
        match std::fs::write(&file, &wave) {
            Ok(()) => written += 1,
            Err(e) => skipped.push(format!("{}: {e}", file.display())),
        }
    }
    (written, skipped)
}

/// One stream decoded through `or-media`'s table, as the bytes of a WAV.
fn decoded(awc: &AwcFile, key: AwcKey, stream: usize, table: &Decoders) -> Result<Vec<u8>, String> {
    let source = or_media::awc::open(awc, key, stream).map_err(|e| e.to_string())?;
    let info = source.info().audio.ok_or("there is no sound in this")?;
    let samples = source
        .audio(table)
        .and_then(|mut audio| audio.read_all_i16())
        .map_err(|e| e.to_string())?;
    let pcm: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    Ok(wav::wav_from_pcm16(&pcm, info.sample_rate, info.channels))
}

/// Replace one stream's samples from a 16-bit PCM WAV and write the container out.
fn replace(args: &[String]) -> ExitCode {
    let common = match parse_common(args) {
        Ok(c) => c,
        Err(e) => return usage_error(&e),
    };
    let [path, stream, wav_path, out_path] = common.positional.as_slice() else {
        return usage_error("replace needs <file.awc> <stream> <in.wav> <out.awc>");
    };
    let Ok(id) = u32::from_str_radix(stream.trim_start_matches("0x"), 16) else {
        return usage_error("the stream is the hexadecimal id `awc info` lists, such as 0344F3E1");
    };
    let read = |what: &str, path: &String| {
        std::fs::read(path).map_err(|e| {
            eprintln!("Error: read {what} {path}: {e}");
            ExitCode::FAILURE
        })
    };
    let (Ok(bytes), Ok(wave)) = (read("", path), read("", wav_path)) else {
        return ExitCode::FAILURE;
    };
    let key = key_for(&common);
    let awc = match AwcFile::parse(&bytes, key) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let samples = |s: &or_io::awc::AwcStream| {
        s.format().is_some()
            || s.stream_format().is_some()
            || s.opus_format().is_ok_and(|f| f.is_some())
            || s.coded_format().is_ok_and(|f| f.is_some())
    };
    let Some(at) = awc.streams.iter().position(|s| s.id == id && samples(s)) else {
        eprintln!("Error: no stream {id:08X} with samples to replace; `awc info` lists them");
        return ExitCode::FAILURE;
    };
    let (pcm, rate, channels) = match wav::read_wav(&wave) {
        Ok(read) => read,
        Err(e) => {
            eprintln!("Error: {wav_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    // A GTA VI Opus stream is encoded anew, as the app's Replace encodes it.
    let opus = (or_media::awc::streams(&awc).iter())
        .any(|r| r.stream == at && r.info.codec == or_media::Codec::Opus);
    let spectra = (or_media::awc::streams(&awc).iter())
        .any(|r| r.stream == at && r.info.codec == or_media::Codec::Spectra);
    let edited = if opus {
        or_media::opus::replaced(&awc, at, &pcm, rate, channels).map_err(|e| e.to_string())
    } else if spectra {
        or_media::spectra::replaced(&awc, at, &pcm, rate, channels).map_err(|e| e.to_string())
    } else {
        awc.with_stream_replaced(at, &pcm, rate, channels, key)
            .map_err(|e| e.to_string())
    };
    let written = edited.and_then(|edited| edited.to_bytes(key).map_err(|e| e.to_string()));
    match written {
        Ok(out) => match std::fs::write(out_path, &out) {
            Ok(()) => {
                println!(
                    "replaced stream {id:08X} with {} sample(s) of {channels} channel(s) at \
                     {rate} Hz; wrote {} byte(s) to {out_path}",
                    pcm.len() / 2 / usize::from(channels),
                    out.len()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: write {out_path}: {e}");
                ExitCode::FAILURE
            }
        },
        Err(why) => {
            eprintln!("Error: {why}");
            ExitCode::FAILURE
        }
    }
}

/// What a whole install's `.awc` population looks like.
#[derive(Default)]
struct Census {
    files: usize,
    parsed: usize,
    refused: Vec<(String, String)>,
    obfuscated: usize,
    multi_channel: usize,
    chunk_indices: usize,
    data_encrypted: usize,
    versions: BTreeMap<u16, usize>,
    chunk_kinds: BTreeMap<String, usize>,
    streams: usize,
    codecs: BTreeMap<String, usize>,
    sample_rates: BTreeMap<u16, usize>,
    unaccounted_files: usize,
    unaccounted_bytes: usize,
    /// The decode gate, split by codec.
    ///
    /// ⚠ **Split because the two codecs are held to different standards and
    /// one aggregate would hide both.** PCM is lossless, so its declared
    /// peak has to match exactly or the reader is wrong; ADPCM is lossy and
    /// the encoder measured the peak over the *source* samples, so a small
    /// residual is the codec rather than the decoder. Reporting one number
    /// would make a broken PCM path hide inside an expected ADPCM residual.
    peaks: BTreeMap<String, PeakCheck>,
    /// The `peak` chunk's whole envelope, same split.
    envelopes: BTreeMap<String, PeakCheck>,
    /// Codec-7 payloads whose MPEG frame header agrees with the sample rate
    /// the container declares, over those that carry a frame header at all.
    /// ⚠ This is the evidence for calling codec 7 MP3: two independent
    /// statements of the same number.
    mp3_rate_agrees: usize,
    /// The container's rate is not one MPEG can express — `48001` and
    /// friends — and the frame header rounds to the nearest that it can.
    mp3_rate_rounded: usize,
    mp3_rate_disagrees: usize,
    mp3_no_frame_header: usize,
}

/// The rates MPEG-1, MPEG-2 and MPEG-2.5 can express, so "the header
/// disagrees" can be told from "the container asked for a rate no MP3 frame
/// header has a code for".
const MPEG_RATES: [u32; 9] = [8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000];

fn nearest_mpeg_rate(hz: u32) -> u32 {
    *MPEG_RATES
        .iter()
        .min_by_key(|r| r.abs_diff(hz))
        .expect("the table is not empty")
}

#[derive(Default)]
struct PeakCheck {
    checked: usize,
    exact: usize,
    worst: u32,
    total_error: u64,
    /// Signed, so a decoder that is systematically quiet or loud is
    /// distinguishable from one whose error is the codec's own noise.
    /// ⚠ A mean of zero with a large magnitude is the signature of lossy
    /// re-encoding; a mean equal to the magnitude is the signature of a bug.
    signed_error: i64,
    /// Histogram of |declared − decoded| bucketed by power of two, so the
    /// shape survives a summary.
    buckets: BTreeMap<u32, usize>,
}

impl PeakCheck {
    fn note(&mut self, declared: u16, decoded: u16) {
        self.checked += 1;
        let error = (declared as i32 - decoded as i32).unsigned_abs();
        if error == 0 {
            self.exact += 1;
        }
        self.worst = self.worst.max(error);
        self.total_error += error as u64;
        self.signed_error += decoded as i64 - declared as i64;
        let bucket = if error == 0 {
            0
        } else {
            32 - error.leading_zeros()
        };
        *self.buckets.entry(bucket).or_default() += 1;
    }

    fn render(&self, what: &str, codec: &str, out: &mut String) {
        out.push_str(&format!(
            "  {codec:<8} {what}: {} of {} exact, worst {}, mean |e| {:.1}, mean e {:+.1}\n",
            self.exact,
            self.checked,
            self.worst,
            if self.checked == 0 {
                0.0
            } else {
                self.total_error as f64 / self.checked as f64
            },
            if self.checked == 0 {
                0.0
            } else {
                self.signed_error as f64 / self.checked as f64
            },
        ));
        for (bucket, count) in &self.buckets {
            let label = if *bucket == 0 {
                "0".to_string()
            } else {
                format!("< {}", 1u32 << bucket)
            };
            out.push_str(&format!("           {label:>10}  {count}\n"));
        }
    }
}

fn census(args: &[String]) -> ExitCode {
    let common = match parse_common(args) {
        Ok(c) => c,
        Err(e) => return usage_error(&e),
    };
    let game_root = match resolve_game_root(common.game.clone()) {
        Ok(p) => p,
        Err(e) => return usage_error(&e),
    };
    let resolved = resolve_crypto(&game_root, common.keys.as_deref());
    let key = resolved.awc_key;
    let mut warnings = resolved.notes;
    if !key.is_usable() {
        warnings.push(
            "no AWC key in this key set, so every obfuscated container will be refused; \
             re-extract keys from your game executable, which carries it"
                .into(),
        );
    }

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        let _ = super::jobs::install_stop_handler(move || cancel.store(true, Ordering::Relaxed));
    }
    eprintln!(
        "openrage-cli: surveying {} (Ctrl-C stops and still reports) ...",
        game_root.display()
    );

    let mut census = Census::default();
    let _ = crawl::crawl(
        &game_root,
        Arc::clone(&resolved.provider),
        &mut warnings,
        &mut |found| {
            if cancel.load(Ordering::Relaxed) {
                return ControlFlow::Break(());
            }
            if !found.leaf.to_ascii_lowercase().ends_with(".awc") {
                return ControlFlow::Continue(());
            }
            census.files += 1;
            if let Some(limit) = common.limit {
                if census.parsed + census.refused.len() >= limit {
                    return ControlFlow::Continue(());
                }
            }
            let Ok(bytes) = found.read() else {
                census
                    .refused
                    .push((found.leaf.to_string(), "unreadable".into()));
                return ControlFlow::Continue(());
            };
            fold_one(&mut census, found.leaf, &bytes, key, common.accumulator);
            ControlFlow::Continue(())
        },
    );

    if common.json {
        print_json(&census, &warnings);
    } else {
        for w in &warnings {
            eprintln!("note: {w}");
        }
        print!("{}", report(&census));
    }
    ExitCode::SUCCESS
}

fn fold_one(census: &mut Census, leaf: &str, bytes: &[u8], key: AwcKey, accumulator: Accumulator) {
    let awc = match AwcFile::parse(bytes, key) {
        Ok(a) => a,
        Err(e) => {
            census
                .refused
                .push((leaf.to_string(), first_clause(&e.to_string())));
            return;
        }
    };
    census.parsed += 1;
    if awc.whole_file_encrypted {
        census.obfuscated += 1;
    }
    if awc.multi_channel() {
        census.multi_channel += 1;
    }
    if awc.has_chunk_indices() {
        census.chunk_indices += 1;
    }
    if awc.single_channel_encrypted() || awc.multi_channel_encrypted() {
        census.data_encrypted += 1;
    }
    *census.versions.entry(awc.version).or_default() += 1;
    let unaccounted = awc.unaccounted_bytes();
    if unaccounted > 0 {
        census.unaccounted_files += 1;
        census.unaccounted_bytes += unaccounted;
    }

    for stream in &awc.streams {
        census.streams += 1;
        for chunk in &stream.chunks {
            *census.chunk_kinds.entry(chunk.kind.label()).or_default() += 1;
        }
        let Some(format) = stream.format() else {
            continue;
        };
        *census.codecs.entry(format.codec.label()).or_default() += 1;
        *census
            .sample_rates
            .entry(format.samples_per_second)
            .or_default() += 1;

        if format.codec == Codec::Mp3 {
            match awc
                .stream_data(stream, key)
                .as_deref()
                .and_then(codec::mpeg_frame_rate)
            {
                Some(rate) if rate == format.samples_per_second as u32 => {
                    census.mp3_rate_agrees += 1
                }
                Some(rate) if rate == nearest_mpeg_rate(format.samples_per_second as u32) => {
                    census.mp3_rate_rounded += 1
                }
                Some(_) => census.mp3_rate_disagrees += 1,
                None => census.mp3_no_frame_header += 1,
            }
        }

        // ⚠ The gate. The peak is what Rockstar's own encoder measured over
        // the samples it then encoded, and nothing in the decode path reads
        // it — so reproducing it is evidence about the decode rather than
        // about the reader agreeing with itself.
        let (Some(declared), Some(pcm)) = (
            format.peak_value(),
            awc.decode_stream(stream, key, accumulator),
        ) else {
            continue;
        };
        let label = format.codec.label();
        let decoded = codec::peak_envelope(&pcm, 1);
        census
            .peaks
            .entry(label.clone())
            .or_default()
            .note(declared, decoded[0]);

        if let Some(envelope) = stream.peak_envelope() {
            let windows = codec::peak_window_count(format.samples as usize);
            if envelope.len() == windows && windows > 0 {
                let ours = codec::peak_envelope(&pcm, windows + 1);
                let check = census.envelopes.entry(label).or_default();
                for (i, want) in envelope.iter().enumerate() {
                    check.note(*want, ours[i + 1]);
                }
            }
        }
    }
}

fn report(c: &Census) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} .awc file(s) found, {} parsed, {} refused\n",
        c.files,
        c.parsed,
        c.refused.len()
    ));
    out.push_str(&format!(
        "  obfuscated {}  multi-channel {}  chunk-indices {}  data-encrypted {}\n",
        c.obfuscated, c.multi_channel, c.chunk_indices, c.data_encrypted
    ));
    out.push_str(&format!("  versions: {}\n", counts(&c.versions)));
    out.push_str(&format!(
        "  {} stream(s); codecs: {}\n",
        c.streams,
        counts(&c.codecs)
    ));
    out.push_str(&format!("  sample rates: {}\n", counts(&c.sample_rates)));
    out.push_str(&format!("  chunks: {}\n", counts(&c.chunk_kinds)));
    out.push_str(&format!(
        "  bytes no chunk accounts for: {} in {} file(s)\n",
        c.unaccounted_bytes, c.unaccounted_files
    ));

    out.push_str("\ndecode against the peaks the encoder recorded\n");
    if c.peaks.is_empty() && c.envelopes.is_empty() {
        out.push_str("  0 of 0 checked: nothing carried both a peak and a decodable payload\n");
    }
    for (codec, check) in &c.peaks {
        check.render("format peak", codec, &mut out);
    }
    for (codec, check) in &c.envelopes {
        check.render("peak chunk ", codec, &mut out);
    }

    let mp3 = c.mp3_rate_agrees + c.mp3_rate_rounded + c.mp3_rate_disagrees + c.mp3_no_frame_header;
    if mp3 > 0 {
        out.push_str(&format!(
            "\ncodec 7 is MP3, on {} payload(s):\n  \
             {} carry a frame header stating exactly the container's rate\n  \
             {} state the nearest rate MPEG can express to a container rate it cannot\n  \
             {} state some other rate\n  \
             {} carry no frame header at all\n",
            mp3, c.mp3_rate_agrees, c.mp3_rate_rounded, c.mp3_rate_disagrees, c.mp3_no_frame_header
        ));
    }

    if !c.refused.is_empty() {
        out.push_str(&format!("\n{} refusal(s):\n", c.refused.len()));
        let mut why: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, reason) in &c.refused {
            *why.entry(reason.as_str()).or_default() += 1;
        }
        for (reason, count) in why {
            out.push_str(&format!("  {count:>6}  {reason}\n"));
        }
        for (name, reason) in c.refused.iter().take(10) {
            out.push_str(&format!("    {name}: {reason}\n"));
        }
    }
    out
}

fn print_json(c: &Census, warnings: &[String]) {
    let doc = serde_json::json!({
        "files": c.files,
        "parsed": c.parsed,
        "refused": c.refused.len(),
        "obfuscated": c.obfuscated,
        "multiChannel": c.multi_channel,
        "chunkIndices": c.chunk_indices,
        "dataEncrypted": c.data_encrypted,
        "versions": c.versions.iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>(),
        "streams": c.streams,
        "codecs": c.codecs,
        "sampleRates": c.sample_rates.iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>(),
        "chunkKinds": c.chunk_kinds,
        "unaccountedBytes": c.unaccounted_bytes,
        "unaccountedFiles": c.unaccounted_files,
        "formatPeak": as_json(&c.peaks),
        "peakChunk": as_json(&c.envelopes),
        "mp3RateAgrees": c.mp3_rate_agrees,
        "mp3RateRounded": c.mp3_rate_rounded,
        "mp3RateDisagrees": c.mp3_rate_disagrees,
        "mp3NoFrameHeader": c.mp3_no_frame_header,
        "warnings": warnings,
    });
    match serde_json::to_string_pretty(&doc) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("Error: serialize: {e}"),
    }
}

fn as_json(checks: &BTreeMap<String, PeakCheck>) -> serde_json::Value {
    serde_json::Value::Object(
        checks
            .iter()
            .map(|(codec, c)| {
                (
                    codec.clone(),
                    serde_json::json!({
                        "checked": c.checked,
                        "exact": c.exact,
                        "worst": c.worst,
                        "totalError": c.total_error,
                        "signedError": c.signed_error,
                    }),
                )
            })
            .collect(),
    )
}

fn counts<K: std::fmt::Display + Ord, V: std::fmt::Display>(map: &BTreeMap<K, V>) -> String {
    if map.is_empty() {
        return "—".to_string();
    }
    map.iter()
        .map(|(k, v)| format!("{k} x{v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn first_clause(message: &str) -> String {
    message
        .split(&[':', ';'][..])
        .nth(2)
        .or_else(|| message.split(':').nth(1))
        .unwrap_or(message)
        .trim()
        .chars()
        .take(72)
        .collect()
}

/// Crawl the install and index its audio, or say why not.
fn read_index(common: &Common, filter: &Filter) -> Result<AudioIndex, ExitCode> {
    let game_root = resolve_game_root(common.game.clone()).map_err(|e| usage_error(&e))?;
    let resolved = resolve_crypto(&game_root, common.keys.as_deref());
    let mut warnings = resolved.notes;
    let started = std::time::Instant::now();
    let index = AudioIndex::read(
        &game_root,
        resolved.provider,
        resolved.awc_key,
        filter,
        &mut warnings,
    );
    for w in &warnings {
        eprintln!("note: {w}");
    }
    eprintln!(
        "indexed {} container(s) and {} sound file(s) in {:.1}s, {} refused",
        index.containers().len(),
        index.rel_files().len(),
        started.elapsed().as_secs_f64(),
        index.refused().len()
    );
    Ok(index)
}

/// A query's hash: `0x` and hex digits as stated, anything else hashed as a name.
fn query_hash(query: &str) -> u32 {
    query
        .strip_prefix("0x")
        .or_else(|| query.strip_prefix("0X"))
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .unwrap_or_else(|| or_io::hash::string_hash_str(query))
}

fn which(args: &[String]) -> ExitCode {
    let common = match parse_common(args) {
        Ok(c) => c,
        Err(e) => return usage_error(&e),
    };
    let Some(query) = common.positional.first() else {
        return usage_error("which needs a sound name or a 0x hash");
    };
    let index = match read_index(&common, &common.filter) {
        Ok(index) => index,
        Err(code) => return code,
    };
    let sound = query_hash(query);
    if common.json {
        let hits: Vec<serde_json::Value> = index
            .find(sound, &common.filter)
            .iter()
            .map(|hit| {
                serde_json::json!({
                    "rel": hit.reference.map(|r| index.rel_files()[r.rel].clone()),
                    "item": hit.reference.map(|r| format!("0x{:08X}", r.item)),
                    "class": hit.reference.map(|r| r.class),
                    "bank": hit.reference.map(|r| format!("0x{:08X}", r.bank)),
                    "bankName": hit.reference.and_then(|r| index.bank_name(r.bank)),
                    "wave": hit.reference.map(|r| format!("0x{:08X}", r.wave)),
                    "resolution": hit.reference.map(|r| format!("{:?}", index.resolution(r))),
                    "containers": hit.containers.iter().map(|c| &c.logical).collect::<Vec<_>>(),
                })
            })
            .collect();
        let doc = serde_json::json!({
            "query": query,
            "hash": format!("0x{sound:08X}"),
            "hits": hits,
        });
        match serde_json::to_string_pretty(&doc) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("Error: serialize: {e}"),
        }
    } else {
        print!("{}", render_which(&index, query, sound, &common.filter));
    }
    ExitCode::SUCCESS
}

fn render_which(index: &AudioIndex, query: &str, sound: u32, filter: &Filter) -> String {
    let hits = index.find(sound, filter);
    let mut out = format!("{} hit(s) for {query} (0x{sound:08X})\n", hits.len());
    for hit in &hits {
        match hit.reference {
            Some(r) => {
                out.push_str(&format!(
                    "  {} 0x{:08X} in {}\n    bank 0x{:08X}{}, wave 0x{:08X}\n",
                    r.class,
                    r.item,
                    index.rel_files()[r.rel],
                    r.bank,
                    index
                        .bank_name(r.bank)
                        .map(|n| format!(" ({n})"))
                        .unwrap_or_default(),
                    r.wave
                ));
                match index.resolution(r) {
                    Resolution::Held => {}
                    Resolution::BankWithoutWave => out
                        .push_str("    no container of this bank in the install holds the wave\n"),
                    Resolution::NoBank => {
                        out.push_str("    no container in the install is this bank\n")
                    }
                }
            }
            None => out.push_str("  a wave no sound file names\n"),
        }
        for container in &hit.containers {
            out.push_str(&format!("    held by {}\n", container.logical));
        }
    }
    out
}

fn index(args: &[String]) -> ExitCode {
    let common = match parse_common(args) {
        Ok(c) => c,
        Err(e) => return usage_error(&e),
    };
    let index = match read_index(&common, &Filter::default()) {
        Ok(index) => index,
        Err(code) => return code,
    };
    let tally = tally(&index, &common.filter);
    if common.json {
        let classes: BTreeMap<&str, serde_json::Value> = tally
            .iter()
            .map(|(class, [held, bank_only, no_bank])| {
                (
                    *class,
                    serde_json::json!({
                        "held": held,
                        "bankWithoutWave": bank_only,
                        "noBank": no_bank,
                    }),
                )
            })
            .collect();
        let doc = serde_json::json!({
            "containers": index.containers().len(),
            "streams": index.containers().iter().map(|c| c.streams.len()).sum::<usize>(),
            "soundFiles": index.rel_files().len(),
            "refused": index.refused().len(),
            "classes": classes,
        });
        match serde_json::to_string_pretty(&doc) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("Error: serialize: {e}"),
        }
    } else {
        print!("{}", render_index(&index, &tally));
    }
    ExitCode::SUCCESS
}

/// Per item class: references held, whose bank holds no such wave, and whose bank is absent.
fn tally<'a>(index: &'a AudioIndex, filter: &Filter) -> BTreeMap<&'a str, [usize; 3]> {
    let mut out: BTreeMap<&str, [usize; 3]> = BTreeMap::new();
    for reference in index.references() {
        if filter
            .class
            .as_deref()
            .is_some_and(|want| !want.eq_ignore_ascii_case(reference.class))
        {
            continue;
        }
        let slot = match index.resolution(reference) {
            Resolution::Held => 0,
            Resolution::BankWithoutWave => 1,
            Resolution::NoBank => 2,
        };
        out.entry(reference.class).or_default()[slot] += 1;
    }
    out
}

fn render_index(index: &AudioIndex, tally: &BTreeMap<&str, [usize; 3]>) -> String {
    let mut out = format!(
        "{} container(s) holding {} stream(s); {} sound file(s); {} file(s) refused\n",
        index.containers().len(),
        index
            .containers()
            .iter()
            .map(|c| c.streams.len())
            .sum::<usize>(),
        index.rel_files().len(),
        index.refused().len()
    );
    if tally.is_empty() {
        out.push_str("  no sound file names a wave, so nothing was resolved\n");
    }
    let mut total = [0usize; 3];
    for (class, counts) in tally {
        out.push_str(&format!(
            "  {class:<16} {} held, {} whose bank holds no such wave, {} whose bank is absent\n",
            counts[0], counts[1], counts[2]
        ));
        for (sum, n) in total.iter_mut().zip(counts) {
            *sum += n;
        }
    }
    if tally.len() > 1 {
        out.push_str(&format!(
            "  {:<16} {} held, {} whose bank holds no such wave, {} whose bank is absent\n",
            "all", total[0], total[1], total[2]
        ));
    }
    let mut absent: BTreeMap<u32, usize> = BTreeMap::new();
    for reference in index.references() {
        if index.resolution(reference) == Resolution::NoBank {
            *absent.entry(reference.bank).or_default() += 1;
        }
    }
    let mut absent: Vec<(u32, usize)> = absent.into_iter().collect();
    absent.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if !absent.is_empty() {
        out.push_str(&format!(
            "\n{} absent bank(s), the most named first:\n",
            absent.len()
        ));
    }
    for (bank, count) in absent.iter().take(10) {
        out.push_str(&format!(
            "  {count:>6}  0x{bank:08X} {}\n",
            index.bank_name(*bank).unwrap_or("(no name table names it)")
        ));
    }
    for (path, why) in index.refused().iter().take(10) {
        out.push_str(&format!("    refused {path}: {}\n", first_clause(why)));
    }
    out
}

fn key_for(common: &Common) -> AwcKey {
    let Ok(root) = resolve_game_root(common.game.clone()) else {
        return AwcKey::default();
    };
    resolve_crypto(&root, common.keys.as_deref()).awc_key
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("Error: {msg}");
    print_usage();
    ExitCode::from(2)
}

fn print_usage() {
    eprintln!("Audio wave containers (.awc): inspect, export, census, and which holds a sound.");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  openrage-cli awc info <file.awc> [--game <dir>] [--keys <file>]");
    eprintln!(
        "  openrage-cli awc export <file.awc> <out_dir> [--wav] [--dialogue <file.tsv>] [--game <dir>]"
    );
    eprintln!("  openrage-cli awc replace <file.awc> <stream> <in.wav> <out.awc> [--game <dir>]");
    eprintln!("  openrage-cli awc decode-check <file.awc>");
    eprintln!("  openrage-cli awc decode-check <archive.rpf> <offset> [--limit <bytes>]");
    eprintln!("  openrage-cli awc census [--game <dir>] [--limit <n>] [--json]");
    eprintln!("  openrage-cli awc which <sound|0xHASH> [--container <text>] [--class <type>]");
    eprintln!("                         [--game <dir>] [--json]");
    eprintln!("  openrage-cli awc index [--class <type>] [--game <dir>] [--json]");
    eprintln!();
    eprintln!("`export` writes a multi-channel container's channels as one interleaved");
    eprintln!("WAV, named for its source stream, 00000000. `replace` re-encodes a 16-bit");
    eprintln!("PCM WAV into the stream `awc info` lists by that hexadecimal id, in the");
    eprintln!("codec it has; a multi-channel stream takes a WAV of as many channels,");
    eprintln!("and a WAV is never resampled, so an ADPCM or MP3 stream takes its own rate.");
    eprintln!("A GTA VI stream is Opus: `export` writes it, or each channel of a");
    eprintln!("multi-channel one, as an Ogg Opus file named for its id, with a pre-skip");
    eprintln!(
        "of {} samples that is inferred, not stated by the file; `--wav` decodes it",
        ogg::PRE_SKIP
    );
    eprintln!("to one WAV at 48000 Hz instead. `replace` encodes a 48000 Hz WAV of its");
    eprintln!("channels into Opus, a multi-channel source's blocks cut as the file's are.");
    eprintln!();
    eprintln!("`decode-check` decodes every GTA VI stream of a container, loose or a body of");
    eprintln!("a GTA VI archive at an offset `rpf scan` lists, in memory, writing nothing,");
    eprintln!("and prints for each whether it decoded, its samples per channel and its");
    eprintln!("peak, min(2|s|, 65535) over every channel, the scale the container states;");
    eprintln!("it exits non-zero when it refuses a stream.");
    eprintln!();
    eprintln!("`which` crawls the install and names the containers holding a sound and");
    eprintln!("the sound file items naming it, by the item's name or the wave's.");
    eprintln!("`index` counts every such item's wave held, missing from its bank, or");
    eprintln!("naming a bank no container in the install is.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --game <dir>   GTA install root. Default: the app's selected game.");
    eprintln!("                 An obfuscated container needs this even for a loose");
    eprintln!("                 file, because that is where the key comes from.");
    eprintln!("  --keys <file>  Override the keys cache.");
    eprintln!("  --limit <n>    Census at most n files. The count of files found");
    eprintln!("                 stays exact.");
    eprintln!("  --raw          (export) Write each stream's descrambled payload as");
    eprintln!("                 it is stored, rather than decoding it to WAV. The way");
    eprintln!("                 to reach a codec nothing here decodes.");
    eprintln!("  --wav          (export) Decode a GTA VI Opus stream to WAV rather than");
    eprintln!("                 writing it as Ogg Opus.");
    eprintln!("  --unclamped    Decode ADPCM with an unclamped predictor. The default");
    eprintln!("                 clamps every sample; `awc census` is how the two were");
    eprintln!("                 told apart.");
    eprintln!("  --container <text>  (which) Keep containers whose path or bank, such");
    eprintln!("                 as RESIDENT/explosions, contains the text; only those");
    eprintln!("                 containers are read.");
    eprintln!("  --class <type> (which, index) Keep items of one type, such as");
    eprintln!("                 SimpleSound, GranularSound or AutomationSound.");
    eprintln!("  --json         Machine-readable output.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peak_error_lands_in_the_bucket_its_magnitude_names() {
        let mut c = PeakCheck::default();
        c.note(1000, 1000);
        c.note(1000, 1001);
        c.note(1000, 1004);
        c.note(0, 65535);

        assert_eq!(c.checked, 4);
        assert_eq!(c.exact, 1);
        assert_eq!(c.worst, 65535);
        assert_eq!(c.total_error, 1 + 4 + 65535);
        assert_eq!(
            c.signed_error,
            1 + 4 + 65535,
            "the three misses all read high"
        );
        assert_eq!(c.buckets.get(&0), Some(&1), "exact");
        assert_eq!(c.buckets.get(&1), Some(&1), "error 1 is < 2");
        assert_eq!(c.buckets.get(&3), Some(&1), "error 4 is < 8");
        assert_eq!(c.buckets.get(&16), Some(&1), "error 65535 < 65536");
    }

    /// ⚠ **A census that checked nothing must not read as a clean one.** A
    /// report whose decode section is empty because nothing was decoded
    /// looks exactly like one where everything matched, so the empty case
    /// says so in words.
    #[test]
    fn a_census_that_exercised_nothing_says_so_rather_than_printing_a_clean_section() {
        let text = report(&Census::default());
        assert!(text.contains("0 of 0 checked"), "{text}");
        assert!(text.contains("0 .awc file(s) found"), "{text}");

        let mut c = Census::default();
        c.peaks.entry("PCM16".into()).or_default().note(5, 5);
        let text = report(&c);
        assert!(text.contains("1 of 1 exact"), "{text}");
        assert!(!text.contains("0 of 0 checked"), "{text}");
    }

    #[test]
    fn a_refusal_is_reduced_to_something_groupable() {
        assert_eq!(
            first_clause("Invalid data: awc: no `ADAT` magic, so the file is scrambled"),
            "no `ADAT` magic, so the file is scrambled"
        );
    }

    /// ⚠ A container rate of 48,001 is not a rate MPEG has a code for, so a
    /// frame header stating 48,000 is agreement rather than a discrepancy.
    /// Counting those as disagreements is what made the first run report
    /// 43,312 of them.
    #[test]
    fn a_container_rate_mpeg_cannot_express_rounds_to_one_it_can() {
        assert_eq!(nearest_mpeg_rate(48000), 48000);
        assert_eq!(nearest_mpeg_rate(48001), 48000);
        assert_eq!(nearest_mpeg_rate(32005), 32000);
        assert_eq!(nearest_mpeg_rate(22053), 22050);
        assert_eq!(nearest_mpeg_rate(27999), 24000);
        // Exactly halfway stays deterministic rather than depending on
        // iteration order.
        assert_eq!(nearest_mpeg_rate(28000), 24000);
    }

    #[test]
    fn a_query_is_a_stated_hash_or_a_hashed_name() {
        assert_eq!(query_hash("0x0000BEEF"), 0xBEEF);
        assert_eq!(query_hash("0XbeeF"), 0xBEEF);
        assert_eq!(
            query_hash("Boom_01"),
            or_io::hash::string_hash_str("boom_01")
        );
    }

    /// ⚠ An index of an install with no sound file resolves nothing, and must
    /// not read as one where every wave was found.
    #[test]
    fn an_index_that_resolved_nothing_says_so() {
        let index = AudioIndex::new();
        let text = render_index(&index, &tally(&index, &Filter::default()));
        assert!(text.contains("nothing was resolved"), "{text}");
        assert!(text.starts_with("0 container(s)"), "{text}");
    }

    #[test]
    fn a_wave_no_sound_file_names_is_still_found_in_its_container() {
        let mut awc = b"ADAT".to_vec();
        for word in [1u16, 0] {
            awc.extend_from_slice(&word.to_le_bytes());
        }
        let wave = or_io::hash::string_hash_str("boom_01") & 0x1FFF_FFFF;
        for word in [1u32, 20, wave] {
            awc.extend_from_slice(&word.to_le_bytes());
        }
        let mut index = AudioIndex::new();
        index.add_file(
            "x:/sfx/resident.rpf/explosions.awc",
            &awc,
            AwcKey::default(),
        );
        assert!(index.refused().is_empty(), "{:?}", index.refused());

        let text = render_which(&index, "boom_01", query_hash("boom_01"), &Filter::default());
        assert!(text.starts_with("1 hit(s) for boom_01"), "{text}");
        assert!(text.contains("a wave no sound file names"), "{text}");
        assert!(
            text.contains("held by x:/sfx/resident.rpf/explosions.awc"),
            "{text}"
        );
        let elsewhere = Filter {
            container: Some("weapons".into()),
            ..Filter::default()
        };
        let text = render_which(&index, "boom_01", query_hash("boom_01"), &elsewhere);
        assert!(text.starts_with("0 hit(s)"), "{text}");
    }

    /// A two-channel PCM16 container of one block, laid out by hand: a table, then 1,024 samples
    /// of each channel in a packet apiece.
    fn a_pair() -> Vec<u8> {
        let channel = |c: i16| -> Vec<u8> {
            (0..1024i16)
                .flat_map(|i| (i * (c + 1)).to_le_bytes())
                .collect()
        };
        let mut block = vec![0u8; 2048];
        for c in 0..2usize {
            for (i, word) in [c as u32, 1, 0, 1024].into_iter().enumerate() {
                block[c * 24 + i * 4..c * 24 + i * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            block[48 + c * 4..52 + c * 4].copy_from_slice(&0u32.to_le_bytes());
        }
        block.extend(channel(0));
        block.extend(channel(1));

        let mut format = Vec::new();
        for word in [1u32, 4 * 2048, 2] {
            format.extend_from_slice(&word.to_le_bytes());
        }
        for c in 0..2u32 {
            format.extend_from_slice(&(0x0100_0000 + c).to_le_bytes());
            format.extend_from_slice(&1024u32.to_le_bytes());
            format.extend_from_slice(&0i16.to_le_bytes());
            format.extend_from_slice(&24000u16.to_le_bytes());
            format.extend_from_slice(&[0, 0, 0, 0]);
        }
        let tables = 16 + 3 * 4 + 2 * 8;
        let mut out = vec![0u8; tables];
        out[0..4].copy_from_slice(b"ADAT");
        out[4..6].copy_from_slice(&1u16.to_le_bytes());
        out[6..8].copy_from_slice(&4u16.to_le_bytes());
        out[8..12].copy_from_slice(&3u32.to_le_bytes());
        out[12..16].copy_from_slice(&(tables as u32).to_le_bytes());
        out[16..20].copy_from_slice(&(2u32 << 29).to_le_bytes());
        out[20..24].copy_from_slice(&0x0100_0000u32.to_le_bytes());
        out[24..28].copy_from_slice(&0x0100_0001u32.to_le_bytes());
        let mut entries = Vec::new();
        for (kind, payload) in [(0x48u64, format), (0x55, block)] {
            while !out.len().is_multiple_of(16) {
                out.push(0);
            }
            entries.push((out.len() as u64) | ((payload.len() as u64) << 28) | (kind << 56));
            out.extend_from_slice(&payload);
        }
        for (i, raw) in entries.iter().enumerate() {
            out[28 + i * 8..36 + i * 8].copy_from_slice(&raw.to_le_bytes());
        }
        out
    }

    fn args(words: &[&std::path::Path]) -> Vec<String> {
        words.iter().map(|w| w.display().to_string()).collect()
    }

    /// One 2,048-byte XMA packet of `frames` silent frames: each is 31 bits, its length, a single
    /// 512-sample subframe that sends no coefficients, and the flag that another frame follows.
    fn silent_xma_packet(frames: usize) -> Vec<u8> {
        let mut bits: Vec<bool> = Vec::new();
        let put = |bits: &mut Vec<bool>, value: usize, n: usize| {
            bits.extend((0..n).rev().map(|i| (value >> i) & 1 == 1));
        };
        for (value, n) in [(frames, 6), (0, 15), (1, 3), (0, 8)] {
            put(&mut bits, value, n);
        }
        for i in 0..frames {
            for (value, n) in [(31, 15), (1, 1), (0, 1), (0, 8), (0, 5)] {
                put(&mut bits, value, n);
            }
            put(&mut bits, usize::from(i + 1 < frames), 1);
        }
        bits.resize(2048 * 8, true);
        bits.chunks(8)
            .map(|c| c.iter().fold(0u8, |v, b| (v << 1) | u8::from(*b)))
            .collect()
    }

    /// One stream of codec byte 5, XMA, at 32,000 Hz: a `format` chunk and four packets of data.
    fn an_xma_stream(samples: u32) -> Vec<u8> {
        let mut format = vec![0u8; 20];
        format[0..4].copy_from_slice(&samples.to_le_bytes());
        format[4..8].copy_from_slice(&(-1i32).to_le_bytes());
        format[8..10].copy_from_slice(&32000u16.to_le_bytes());
        format[19] = 5;
        let data: Vec<u8> = (0..4).flat_map(|_| silent_xma_packet(10)).collect();
        let tables = 16 + 4 + 2 * 8;
        let mut out = vec![0u8; tables];
        out[0..4].copy_from_slice(b"ADAT");
        out[4..6].copy_from_slice(&1u16.to_le_bytes());
        out[8..12].copy_from_slice(&1u32.to_le_bytes());
        out[12..16].copy_from_slice(&(tables as u32).to_le_bytes());
        out[16..20].copy_from_slice(&(0x0344_F3E1u32 | (2 << 29)).to_le_bytes());
        let mut entries = Vec::new();
        for (kind, payload) in [(0xFAu64, format), (0x55, data)] {
            while !out.len().is_multiple_of(16) {
                out.push(0);
            }
            entries.push((out.len() as u64) | ((payload.len() as u64) << 28) | (kind << 56));
            out.extend_from_slice(&payload);
        }
        for (i, raw) in entries.iter().enumerate() {
            out[20 + i * 8..28 + i * 8].copy_from_slice(&raw.to_le_bytes());
        }
        out
    }

    /// An XMA stream exports to a WAV of the samples it states, and what `export` cannot decode is
    /// listed rather than written: here a stream whose stated count its packets cannot hold.
    #[test]
    fn an_xma_stream_exports_to_wav_and_one_that_does_not_decode_is_skipped() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let source = dir.path().join("chips.awc");
        std::fs::write(&source, an_xma_stream(40 * 512)).expect("the container is written");
        let out = dir.path().join("out");
        assert!(export(&args(&[&source, &out])) == ExitCode::SUCCESS);
        let (pcm, rate, channels) =
            wav::read_wav(&std::fs::read(out.join("0344F3E1.wav")).expect("the export"))
                .expect("WAV");
        assert_eq!((rate, channels, pcm.len()), (32000, 1, 40 * 512 * 2));
        assert!(
            pcm.iter().all(|b| *b == 0),
            "silent frames decode to silence"
        );

        let source = dir.path().join("short.awc");
        std::fs::write(&source, an_xma_stream(40 * 512 + 1)).expect("the container is written");
        let out = dir.path().join("short");
        assert!(export(&args(&[&source, &out])) == ExitCode::SUCCESS);
        assert!(!out.join("0344F3E1.wav").exists());
    }

    /// ⚠ A multi-channel container exports as one WAV, named for its source stream, with the
    /// channels interleaved; `replace` then takes that WAV back, and the file it writes decodes to it.
    #[test]
    fn a_multi_channel_container_exports_interleaved_and_replaces_from_it() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let source = dir.path().join("pair.awc");
        std::fs::write(&source, a_pair()).expect("the container is written");

        let out = dir.path().join("out");
        assert!(export(&args(&[&source, &out])) == ExitCode::SUCCESS);
        let (pcm, rate, channels) =
            wav::read_wav(&std::fs::read(out.join("00000000.wav")).expect("the export"))
                .expect("WAV");
        assert_eq!((rate, channels, pcm.len()), (24000, 2, 1024 * 2 * 2));
        assert_eq!(
            &pcm[4..8],
            &[1, 0, 2, 0],
            "channel 0 then channel 1, sample 1"
        );

        // The same WAV back, one channel louder: the first channel decodes to what it was given.
        let edited: Vec<u8> = pcm
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|f| {
                let loud = i16::from_le_bytes([f[0], f[1]]).saturating_mul(3);
                [loud.to_le_bytes().to_vec(), f[2..4].to_vec()].concat()
            })
            .collect();
        let wave = dir.path().join("louder.wav");
        std::fs::write(&wave, wav::wav_from_pcm16(&edited, 24000, 2)).expect("the WAV is written");
        let written = dir.path().join("louder.awc");
        let mut replace_args = args(&[&source]);
        replace_args.push("0".into());
        replace_args.extend(args(&[&wave, &written]));
        assert!(replace(&replace_args) == ExitCode::SUCCESS);

        let awc = AwcFile::parse(
            &std::fs::read(&written).expect("written"),
            AwcKey::default(),
        )
        .expect("the file parses");
        let channels = awc
            .decode_channels(AwcKey::default(), Accumulator::Clamped)
            .expect("it decodes");
        assert_eq!(
            &channels[0][2..4],
            &3i16.to_le_bytes(),
            "sample 1 of channel 0, tripled"
        );
        assert_eq!(&channels[1][2..4], &2i16.to_le_bytes());
    }

    /// A stream the container has no samples for, a WAV of the wrong shape and a container nobody
    /// can open each fail without writing the file.
    #[test]
    fn a_replacement_that_cannot_be_made_writes_nothing() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let source = dir.path().join("pair.awc");
        std::fs::write(&source, a_pair()).expect("the container is written");
        let mono = dir.path().join("mono.wav");
        std::fs::write(&mono, wav::wav_from_pcm16(&[0; 400], 24000, 1))
            .expect("the WAV is written");
        let out = dir.path().join("out.awc");
        for (stream, wave) in [("0", &mono), ("1", &mono), ("zz", &mono)] {
            let mut words = args(&[&source]);
            words.push(stream.into());
            words.extend(args(&[wave, &out]));
            assert!(replace(&words) != ExitCode::SUCCESS, "stream {stream}");
        }
        assert!(!out.exists());
    }

    /// A GTA VI Opus stream exports with `--wav` to the WAV its decode makes, and `replace`
    /// encodes a 48 kHz WAV into it, refusing one at another rate without writing the file.
    #[test]
    fn an_opus_stream_exports_to_wav_and_is_replaced_from_one() {
        use or_io::awc::opus::OpusStream;

        let tone: Vec<i16> = (0..1000).map(|t| ((t * 37) % 4000 - 2000) as i16).collect();
        let settings = or_media::opus::Settings {
            bitrate: 64_000,
            voice: false,
        };
        let packets = or_media::opus::encode(&tone, 1, settings).expect("it encodes");
        let dir = tempfile::tempdir().expect("a scratch directory");
        let source = dir.path().join("speech.awc");
        let bytes = crate::awc_decode::opus_container(0x0A01, &packets, 1000);
        std::fs::write(&source, bytes).expect("the container is written");

        let out = dir.path().join("out");
        let mut words = args(&[&source, &out]);
        words.push("--wav".into());
        assert!(export(&words) == ExitCode::SUCCESS);
        let exported = std::fs::read(out.join("00000A01.wav")).expect("the export");
        let (pcm, rate, channels) = wav::read_wav(&exported).expect("WAV");
        let stream = OpusStream {
            id: 0x0A01,
            channels: 1,
            samples: 1000,
            packets,
        };
        let decoded = or_media::opus::decode(&stream).expect("it decodes");
        let want: Vec<u8> = decoded.iter().flat_map(|s| s.to_le_bytes()).collect();
        assert_eq!((rate, channels, pcm), (48_000, 1, want));

        let replace_from = |rate: u32, written: &std::path::Path| {
            let samples: Vec<u8> = (0..30_011i32)
                .flat_map(|i| (((i * 137) % 20000 - 10000) as i16).to_le_bytes())
                .collect();
            let wave = dir.path().join("new.wav");
            std::fs::write(&wave, wav::wav_from_pcm16(&samples, rate, 1)).expect("the WAV");
            let mut words = args(&[&source]);
            words.push("0A01".into());
            words.extend(args(&[&wave, written]));
            replace(&words)
        };
        let written = dir.path().join("new.awc");
        assert!(replace_from(48_000, &written) == ExitCode::SUCCESS);
        let awc = AwcFile::parse(
            &std::fs::read(&written).expect("written"),
            AwcKey::default(),
        )
        .expect("the file parses");
        let format = awc.streams[0]
            .opus_format()
            .expect("read")
            .expect("present");
        assert_eq!(format.samples_per_channel, 30_011);
        let refused = dir.path().join("refused.awc");
        assert!(replace_from(44_100, &refused) != ExitCode::SUCCESS);
        assert!(!refused.exists());
    }
}
