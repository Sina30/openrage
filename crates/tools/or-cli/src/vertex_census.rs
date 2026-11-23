//! `openrage-cli channels`: which vertex channels shipped geometry
//! actually carries, and in which encodings.
//!
//! An FVF can name 8 texcoord sets and 2 colour sets; this measures, over an
//! install, which of them files actually use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use or_io::codec::mesh::{GrcDataSize, GrcFvfChannel, VertexLayout};
use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::gta5::gen8::GrcFvf;
use or_title::install::crawl::{self, ControlFlow};

use super::keys::resolve_crypto;
use super::search::resolve_game_root;

/// Extensions whose resources carry a `GrcVertexBuffer`.
const GEOMETRY: [&str; 4] = ["ydr", "ydd", "yft", "ypt"];

#[derive(Default)]
struct Census {
    files: usize,
    files_with_geometry: usize,
    unreadable: usize,
    refused: usize,
    buffers: usize,
    /// Per channel: how many buffers carry it.
    present: BTreeMap<u8, usize>,
    /// Per channel: the encodings seen, and how often.
    encodings: BTreeMap<u8, BTreeMap<u8, usize>>,
    /// How many texcoord sets a buffer carries → how many buffers.
    texcoord_sets: BTreeMap<usize, usize>,
    /// How many colour sets a buffer carries → how many buffers.
    colour_sets: BTreeMap<usize, usize>,
    /// A declared stride that is not the sum of the channels.
    padded: usize,
    /// Windows passing a predicate of the same shape that no real
    /// declaration can satisfy. The recogniser's own error bar.
    control: usize,
    /// Per extension, so a format that skews the totals is visible.
    per_ext: BTreeMap<String, usize>,
}

impl Census {
    fn record(&mut self, ext: &str, fvf: &GrcFvf, declared_stride: u16) {
        self.buffers += 1;
        *self.per_ext.entry(ext.to_string()).or_default() += 1;

        let layout = VertexLayout::of(fvf.fvf, fvf.channel_sizes);
        if declared_stride != 0 && declared_stride as usize != layout.packed_stride() {
            self.padded += 1;
        }

        let mut texcoords = 0usize;
        let mut colours = 0usize;
        for (channel, slot) in layout.present() {
            *self.present.entry(channel as u8).or_default() += 1;
            *self
                .encodings
                .entry(channel as u8)
                .or_default()
                .entry(slot.size as u8)
                .or_default() += 1;
            match channel {
                GrcFvfChannel::Tex0
                | GrcFvfChannel::Tex1
                | GrcFvfChannel::Tex2
                | GrcFvfChannel::Tex3
                | GrcFvfChannel::Tex4
                | GrcFvfChannel::Tex5
                | GrcFvfChannel::Tex6
                | GrcFvfChannel::Tex7 => texcoords += 1,
                GrcFvfChannel::Diffuse | GrcFvfChannel::Specular => colours += 1,
                _ => {}
            }
        }
        *self.texcoord_sets.entry(texcoords).or_default() += 1;
        *self.colour_sets.entry(colours).or_default() += 1;
    }
}

pub fn run(args: &[String]) -> ExitCode {
    let mut game: Option<PathBuf> = None;
    let mut keys: Option<PathBuf> = None;
    let mut only: Vec<String> = Vec::new();
    let mut limit = usize::MAX;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg, None),
        };
        match key {
            "-h" | "--help" | "help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            "--game" | "--keys" | "--ext" | "--limit" => {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("Error: {key} needs a value");
                                return ExitCode::from(2);
                            }
                        }
                    }
                };
                match key {
                    "--game" => game = Some(PathBuf::from(v)),
                    "--keys" => keys = Some(PathBuf::from(v)),
                    "--ext" => only.push(v.trim_start_matches('.').to_ascii_lowercase()),
                    _ => match v.parse() {
                        Ok(n) => limit = n,
                        Err(_) => {
                            eprintln!("Error: --limit needs a number");
                            return ExitCode::from(2);
                        }
                    },
                }
            }
            other => {
                eprintln!("Error: unknown option '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let game_root = match resolve_game_root(game) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("Error: {message}");
            return ExitCode::from(2);
        }
    };

    let wanted: Vec<String> = if only.is_empty() {
        GEOMETRY.iter().map(|e| e.to_string()).collect()
    } else {
        only
    };

    let resolved = resolve_crypto(&game_root, keys.as_deref());
    let mut warnings = resolved.notes;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        let _ = super::jobs::install_stop_handler(move || cancel.store(true, Ordering::Relaxed));
    }

    eprintln!(
        "openrage-cli: reading vertex declarations in {} (Ctrl-C stops and still reports) ...",
        game_root.display()
    );

    let census = survey(
        &game_root,
        resolved.provider,
        &wanted,
        limit,
        &cancel,
        &mut warnings,
    );

    for w in &warnings {
        eprintln!("note: {w}");
    }
    print!("{}", report(&census));
    ExitCode::SUCCESS
}

fn survey(
    game_root: &Path,
    provider: Arc<dyn RpfCryptoProvider>,
    wanted: &[String],
    limit: usize,
    cancel: &AtomicBool,
    warnings: &mut Vec<String>,
) -> Census {
    let mut census = Census::default();
    let _ = crawl::crawl(game_root, provider, warnings, &mut |found| {
        if cancel.load(Ordering::Relaxed) || census.files >= limit {
            return ControlFlow::Break(());
        }
        let Some(ext) = found
            .leaf
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
        else {
            return ControlFlow::Continue(());
        };
        if !wanted.contains(&ext) {
            return ControlFlow::Continue(());
        }
        census.files += 1;
        let Ok(bytes) = found.read() else {
            census.unreadable += 1;
            return ControlFlow::Continue(());
        };
        let before = census.buffers;
        if !harvest(&mut census, &ext, &bytes) {
            census.refused += 1;
        } else if census.buffers > before {
            census.files_with_geometry += 1;
        }
        ControlFlow::Continue(())
    });
    census
}

/// Pull every vertex declaration out of one file's bytes.
///
/// Reads the resource image rather than a per-format reader: an FVF is a
/// fixed 16-byte record whatever owns it.
fn harvest(census: &mut Census, ext: &str, bytes: &[u8]) -> bool {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};

    let Ok(opened) = Rsc7::open(bytes) else {
        return false;
    };
    let end = (opened.virtual_size as usize).min(opened.data.len());
    let mut control = 0usize;
    let found = scan_declarations(&opened.data[..end], &mut control);
    census.control += control;
    for (fvf, stride) in found {
        census.record(ext, &fvf, stride);
    }
    true
}

/// Every `GrcFvf`-shaped record in the virtual heap, plus a measurement of
/// how often the recogniser is wrong.
///
/// A declaration is recognised by five of its own fields agreeing. Without a
/// control such a rule recognised 1,345,300 records with `tex6` spread
/// near-uniformly across all 16 encodings, which real geometry is not.
///
/// The **control** runs the identical predicate with the one agreement no
/// real record can satisfy — a declared size one byte off the stride its own
/// nibbles imply — so whatever it finds is the recogniser's false-positive
/// floor, printed beside the count.
fn scan_declarations(heap: &[u8], control: &mut usize) -> Vec<(GrcFvf, u16)> {
    let mut out = Vec::new();
    // ⚠ Stepping by 4 quadruples the windows a rule of this shape is
    // offered. A paged heap allocates these on 16-byte boundaries.
    let mut at = 0usize;
    while at + 16 <= heap.len() {
        let fvf_bits = u32::from_le_bytes([heap[at], heap[at + 1], heap[at + 2], heap[at + 3]]);
        let fvf_size = heap[at + 4];
        let flags = heap[at + 5];
        let dynamic_order = heap[at + 6];
        let channel_count = heap[at + 7];
        let sizes = u64::from_le_bytes(heap[at + 8..at + 16].try_into().unwrap());

        let candidate = GrcFvf {
            fvf: fvf_bits,
            fvf_size,
            flags,
            dynamic_order,
            channel_count,
            channel_sizes: sizes,
            ..GrcFvf::default()
        };
        if plausible(&candidate) {
            let stride = packed_stride(&candidate);
            if stride == fvf_size as u16 {
                out.push((candidate, fvf_size as u16));
            } else if stride == fvf_size as u16 + 1 {
                *control += 1;
            }
        }
        at += 16;
    }
    out
}

/// The four agreements that do not involve the stride: a vertex has a
/// position, the channel count counts the channels, the only defined flag is
/// `PREXFORM`, and a static declaration's dynamic order is zero.
fn plausible(fvf: &GrcFvf) -> bool {
    fvf.fvf_size > 0
        && fvf.fvf & GrcFvfChannel::Position.mask() != 0
        && fvf.channel_count as u32 == fvf.fvf.count_ones()
        && fvf.channel_count >= 2
        && fvf.flags & !0x01 == 0
        && fvf.dynamic_order == 0
}

/// The stride a declaration's own channels imply.
fn packed_stride(fvf: &GrcFvf) -> u16 {
    VertexLayout::of(fvf.fvf, fvf.channel_sizes).packed_stride() as u16
}

fn report(census: &Census) -> String {
    let mut s = String::new();
    s.push_str("Vertex channels\n\n");
    s.push_str(&format!(
        "files read      {}\nwith geometry   {}\nrefused         {}\nunreadable      {}\n\
         declarations    {}\npadded stride   {}\ncontrol         {} ({:.3}% of the count)\n\n\
         The control is what a predicate of the same shape finds when given the one\n\
         agreement no real declaration can satisfy. It is this recogniser's own\n\
         false-positive floor, and every figure below carries it.\n\n",
        census.files,
        census.files_with_geometry,
        census.refused,
        census.unreadable,
        census.buffers,
        census.padded,
        census.control,
        100.0 * census.control as f64 / census.buffers.max(1) as f64,
    ));

    if census.buffers == 0 {
        s.push_str("No vertex declarations found.\n");
        return s;
    }

    s.push_str("channel    buffers      share  encodings\n");
    for channel in GrcFvfChannel::ALL {
        let n = census.present.get(&(channel as u8)).copied().unwrap_or(0);
        let encodings = census
            .encodings
            .get(&(channel as u8))
            .map(|m| {
                let mut seen: Vec<_> = m.iter().collect();
                seen.sort_by(|a, b| b.1.cmp(a.1));
                seen.iter()
                    .map(|(size, count)| {
                        format!("{:?} {}", GrcDataSize::ALL[**size as usize], count)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        s.push_str(&format!(
            "{:<9}  {:>7}  {:>8.2}%  {}\n",
            channel.name(),
            n,
            100.0 * n as f64 / census.buffers as f64,
            encodings
        ));
    }

    s.push_str("\nsets per buffer   texcoord    colour\n");
    let widest = census
        .texcoord_sets
        .keys()
        .chain(census.colour_sets.keys())
        .copied()
        .max()
        .unwrap_or(0);
    for n in 0..=widest {
        s.push_str(&format!(
            "{n:<17} {:>8}  {:>8}\n",
            census.texcoord_sets.get(&n).copied().unwrap_or(0),
            census.colour_sets.get(&n).copied().unwrap_or(0),
        ));
    }

    s.push_str("\nper extension\n");
    for (ext, n) in &census.per_ext {
        s.push_str(&format!("  {ext:<6} {n:>8}\n"));
    }
    s
}

fn print_usage() {
    println!(
        "openrage-cli channels --game <dir> [--ext <ext>] [--limit <n>] [--keys <file>]\n\n\
         Which vertex channels shipped geometry carries, and in which encodings.\n\n\
         Defaults to .ydr, .ydd, .yft and .ypt."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A declaration of `channels`, with the size and count its own fields agree on.
    fn declaration(channels: &[(GrcFvfChannel, GrcDataSize)]) -> GrcFvf {
        let mut fvf = GrcFvf::default();
        for (channel, size) in channels {
            fvf.fvf |= channel.mask();
            let shift = *channel as u64 * 4;
            fvf.channel_sizes = (fvf.channel_sizes & !(0xF << shift)) | ((*size as u64) << shift);
        }
        fvf.fvf_size = packed_stride(&fvf) as u8;
        fvf.channel_count = fvf.fvf.count_ones() as u8;
        fvf
    }

    #[test]
    fn a_declaration_is_recognised_only_when_its_own_fields_agree() {
        let fvf = declaration(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Tex0, GrcDataSize::Float2),
        ]);

        let mut census = Census::default();
        census.record("ydr", &fvf, fvf.fvf_size as u16);
        assert_eq!(census.buffers, 1);
        assert_eq!(census.texcoord_sets.get(&1), Some(&1));
        assert_eq!(census.colour_sets.get(&0), Some(&1));
        assert_eq!(census.padded, 0);
        assert_eq!(
            census.present.get(&(GrcFvfChannel::Position as u8)),
            Some(&1)
        );
        assert_eq!(census.present.get(&(GrcFvfChannel::Tex1 as u8)), None);

        // A stride that does not match the channels is padding, and is
        // counted rather than silently accepted as the packed one.
        let mut padded = Census::default();
        padded.record("ydr", &fvf, 32);
        assert_eq!(padded.padded, 1);
    }

    #[test]
    fn the_report_survives_an_empty_census() {
        let out = report(&Census::default());
        assert!(out.contains("No vertex declarations found"));
    }

    fn a_real_declaration() -> GrcFvf {
        declaration(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Normal, GrcDataSize::Float3),
            (GrcFvfChannel::Tex0, GrcDataSize::Float2),
        ])
    }

    #[test]
    fn breaking_any_single_agreement_rejects_the_candidate() {
        assert!(plausible(&a_real_declaration()));

        let no_position = declaration(&[
            (GrcFvfChannel::Normal, GrcDataSize::Float3),
            (GrcFvfChannel::Tex0, GrcDataSize::Float2),
        ]);
        assert!(!plausible(&no_position), "a vertex without a position");

        for break_it in [
            (|f: &mut GrcFvf| f.channel_count = 7) as fn(&mut GrcFvf),
            |f| f.flags = 0x80,
            |f| f.dynamic_order = 3,
            |f| f.fvf_size = 0,
        ] {
            let mut fvf = a_real_declaration();
            break_it(&mut fvf);
            assert!(!plausible(&fvf), "{fvf:?} should not have been recognised");
        }
    }

    #[test]
    fn the_scan_finds_a_planted_declaration_and_its_control_does_not() {
        let fvf = a_real_declaration();
        let mut heap = vec![0u8; 64];
        heap[32..36].copy_from_slice(&fvf.fvf.to_le_bytes());
        heap[36] = fvf.fvf_size;
        heap[37] = fvf.flags;
        heap[38] = fvf.dynamic_order;
        heap[39] = fvf.channel_count;
        heap[40..48].copy_from_slice(&fvf.channel_sizes.to_le_bytes());

        let mut control = 0usize;
        let found = scan_declarations(&heap, &mut control);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0.fvf, fvf.fvf);
        assert_eq!(control, 0);

        // One byte off the stride its own nibbles imply is the control's
        // shape, and it must land there rather than in the count.
        let mut wrong = heap.clone();
        wrong[36] -= 1;
        let mut control = 0usize;
        assert!(scan_declarations(&wrong, &mut control).is_empty());
        assert_eq!(control, 1);
    }

    /// A declaration off the 16-byte grid is missed: a real limit of the scan.
    #[test]
    fn a_declaration_off_the_alignment_grid_is_not_found() {
        let fvf = a_real_declaration();
        let mut heap = vec![0u8; 64];
        let at = 36;
        heap[at..at + 4].copy_from_slice(&fvf.fvf.to_le_bytes());
        heap[at + 4] = fvf.fvf_size;
        heap[at + 7] = fvf.channel_count;
        heap[at + 8..at + 16].copy_from_slice(&fvf.channel_sizes.to_le_bytes());

        let mut control = 0usize;
        assert!(scan_declarations(&heap, &mut control).is_empty());
    }
}
