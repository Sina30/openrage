//! GTA VI's `.ttpf` texture packs, read against every pack of a GTA VI folder.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names a GTA VI folder. A folder still downloading holds
//! packs whose head is not on disk yet, and packs shorter than their sentinel or sparse: both are
//! reported, and neither fails the suite. Every head that is present must hold to the container
//! model.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use or_io::gta6::bcpack::BcPackError;
use or_io::gta6::ttpf::{TexturePack, TtpfError};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};
use rayon::prelude::*;

fn packs_under(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            packs_under(&path, found);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ttpf"))
        {
            found.push(path);
        }
    }
}

fn packs(what: &str) -> Option<Vec<PathBuf>> {
    let mut found = Vec::new();
    for root in game_roots() {
        packs_under(&root, &mut found);
    }
    found.sort();
    if found.is_empty() {
        skip_install(what, format_args!("no .ttpf under {GAME_ROOT_ENV}"));
        return None;
    }
    Some(found)
}

/// Whether the first 16 bytes are on disk; a download writes pieces out of order, so a missing head reads as zeros.
fn head_present(path: &Path) -> bool {
    let mut head = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
        .is_ok_and(|()| head != [0; 16])
}

/// The reason a texture is refused, as the tally counts it; `None` for a fault of the model.
fn reason(pack: &TexturePack, e: &TtpfError) -> Option<String> {
    Some(match e {
        TtpfError::UnidentifiedCodec { kind, .. } => format!("a body of codec kind {kind}"),
        TtpfError::BcPack { kind, source, .. } => match source {
            BcPackError::PresetNotKnown { .. } => {
                format!("a BC{kind} coder body that reads a preset slot not known")
            }
            _ => format!("a BC{kind} coder body that does not decode"),
        },
        TtpfError::UnknownShape { dimension, .. } => format!("dimension {dimension}"),
        TtpfError::Volume { .. } => "a volume".into(),
        TtpfError::RangeNotKnown { .. } => "an entry, or the next, not on disk".into(),
        TtpfError::CubeNotMeasured { .. } => "a cube unlike the measured ones".into(),
        TtpfError::UnknownFormat { code, .. } => format!("format {code:#05x}"),
        TtpfError::TopMipInTail { format, .. } => format!("a top mip in the {format} tail"),
        TtpfError::NotOnDisk { .. } => "a body past the end of the file".into(),
        TtpfError::Inflate { row, .. } if zero_head(pack, *row) => {
            "a zlib body whose head is zeros (a piece not downloaded)".into()
        }
        TtpfError::Inflate { .. } => "a zlib body cut by zeros inside it".into(),
        _ => return None,
    })
}

/// Whether body `row` starts with two zero bytes, which no zlib stream does.
fn zero_head(pack: &TexturePack, row: usize) -> bool {
    let mut head = [0u8; 2];
    std::fs::File::open(pack.path())
        .and_then(|mut f| {
            std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(pack.rows[row].offset))?;
            std::io::Read::read_exact(&mut f, &mut head)
        })
        .is_ok_and(|()| head == [0, 0])
}

#[test]
fn every_present_head_holds_to_the_container_model_and_each_texture_reads_or_is_refused_by_name() {
    let Some(packs) = packs("gta6 ttpf") else {
        return;
    };
    let (mut headless, mut complete, mut short) = (Vec::new(), 0usize, Vec::new());
    // Keyed by whether the pack is complete, then by "read" or the reason a texture is refused.
    let mut tally: BTreeMap<(&str, String), usize> = BTreeMap::new();
    let (mut mips_read, mut whole_chains) = (0u64, 0usize);
    for path in &packs {
        if !head_present(path) {
            headless.push(path);
            continue;
        }
        let pack = match TexturePack::open(path) {
            Ok(pack) => pack,
            Err(e @ TtpfError::HeadNotOnDisk { .. }) => {
                eprintln!("{}: {e}", path.display());
                headless.push(path);
                continue;
            }
            Err(e) => panic!(
                "{} does not hold to the container model: {e}",
                path.display()
            ),
        };
        let state = if pack.is_complete() {
            complete += 1;
            "complete"
        } else {
            let sentinel = pack.rows.last().map_or(0, |row| row.offset);
            short.push((path, pack.size_on_disk(), sentinel));
            "incomplete"
        };
        let results: Vec<_> = (0..pack.entries.len())
            .into_par_iter()
            .map(|index| (index, pack.texture(index)))
            .collect();
        for (index, result) in results {
            let outcome = match result {
                Ok(texture) => {
                    mips_read += u64::from(texture.levels);
                    let stated = u32::from(pack.entries[index].mip_count).max(1);
                    whole_chains += usize::from(texture.levels >= stated);
                    match (texture.faces, texture.format) {
                        (6, format) => format!("read: a {} cube", format.name()),
                        (_, format) if !format.format().is_compressed() => {
                            format!("read: {}", format.name())
                        }
                        _ => "read".into(),
                    }
                }
                Err(e) => {
                    let Some(why) = reason(&pack, &e) else {
                        panic!("{} texture {index}: {e}", path.display());
                    };
                    assert!(
                        !pack.is_complete()
                            || !matches!(
                                e,
                                TtpfError::Inflate { .. } | TtpfError::NotOnDisk { .. }
                            ),
                        "{} is complete, and texture {index}: {e}",
                        path.display()
                    );
                    why
                }
            };
            *tally.entry((state, outcome)).or_default() += 1;
        }
    }

    for (path, on_disk, sentinel) in &short {
        eprintln!(
            "incomplete: {} holds {on_disk} of {sentinel} bytes ({:.1} %), or is sparse",
            path.display(),
            *on_disk as f64 * 100.0 / *sentinel as f64
        );
    }
    eprintln!(
        "ttpf: {} pack(s): {} with no head on disk, {complete} complete, {} short or sparse; \
         {mips_read} mips read, {whole_chains} texture(s) with every stated mip",
        packs.len(),
        headless.len(),
        short.len()
    );
    for ((state, outcome), n) in &tally {
        eprintln!("ttpf: {state:>8} {n:>6}  {outcome}");
    }
    if complete + short.len() == 0 {
        skip_install("gta6 ttpf", "no .ttpf head is on disk yet");
    }
}
