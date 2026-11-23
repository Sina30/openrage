//! `openrage-cli archives --game <root>`: does any archive in an install, nested
//! ones included, contain an escrow-wrapped resource?
//!
//! A shipped install is the denominator, not the staged corpus, which holds only
//! what somebody chose to stage.
//!
//! Escrow would show as a **number in a header**, needing no key, in one of two
//! places, and the census reports the distribution of both:
//!
//! - **The archive's encryption magic.** An unknown mode lands in
//!   `RpfEncryption::Other` rather than being mistaken for a known one.
//! - **A binary entry's `encrypted_raw`**, the whole high word of the entry
//!   bitfield. The reader treats *any* non-zero value as "encrypted", so a mode
//!   marked in that word would be silently lumped in with ordinary encryption.
//!
//! If both hold only known values, there is nothing to write a detector against
//! and the zero has a denominator.
//!
//! ⚠ It reads **entry tables only** — no extraction, no decompression. An escrow
//! *body* that is simply undecryptable is not distinguishable from a wrong key,
//! so it does not guess between them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::container::rpf::{RpfArchive, RpfEncryption, RpfEntry};

/// How deep nested archives are followed.
///
/// The same eight the VFS uses, and for the same reason: it is the cycle guard
/// as well as the depth limit.
const MAX_NEST_DEPTH: u32 = 8;

#[derive(Debug, Default)]
struct Census {
    archives: u64,
    archives_unreadable: u64,
    nested_archives: u64,
    nested_unreadable: u64,
    directories: u64,
    binaries: u64,
    resources: u64,
    /// Archive header encryption magic → how many archives carry it.
    modes: BTreeMap<u32, u64>,
    /// Binary entry `encrypted_raw` → how many entries carry it.
    entry_flags: BTreeMap<u32, u64>,
    /// Anything the reader would have to call `Other`, with where it was seen.
    unknown_modes: Vec<(String, u32)>,
    /// A binary entry whose high word is neither 0 nor 1.
    unknown_entry_flags: Vec<(String, String, u32)>,
}

impl Census {
    fn record_archive(&mut self, label: &str, archive: &RpfArchive) {
        let raw = archive.header.encryption.to_raw();
        *self.modes.entry(raw).or_default() += 1;
        if matches!(archive.header.encryption, RpfEncryption::Other(_)) {
            self.unknown_modes.push((label.to_string(), raw));
        }

        for (index, entry) in archive.entries.iter().enumerate() {
            match entry {
                RpfEntry::Directory(_) => self.directories += 1,
                RpfEntry::Resource(_) => self.resources += 1,
                RpfEntry::Binary(binary) => {
                    self.binaries += 1;
                    *self.entry_flags.entry(binary.encrypted_raw).or_default() += 1;
                    // ⚠ 0 and 1 are the two the reader can tell apart. Anything
                    // else is read as "encrypted" and is the candidate.
                    if binary.encrypted_raw > 1 {
                        let name = archive
                            .entry_path(index as u32)
                            .unwrap_or_else(|| format!("entry {index}"));
                        self.unknown_entry_flags.push((
                            label.to_string(),
                            name,
                            binary.encrypted_raw,
                        ));
                    }
                }
            }
        }
    }
}

fn mode_name(raw: u32) -> String {
    match RpfEncryption::from_raw(raw) {
        RpfEncryption::None => "NONE".to_string(),
        RpfEncryption::Open => "OPEN".to_string(),
        RpfEncryption::Aes => "AES".to_string(),
        RpfEncryption::Ng => "NG".to_string(),
        RpfEncryption::Other(raw) | RpfEncryption::Tagged(raw) => format!("UNKNOWN({raw:#010x})"),
    }
}

fn archives_under(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let named_rpf = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("rpf"));
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => archives_under(&path, out),
            Ok(kind) if kind.is_file() && named_rpf => out.push(path),
            _ => {}
        }
    }
}

fn walk_nested(census: &mut Census, archive: &Arc<RpfArchive>, label: &str, depth: u32) {
    if depth >= MAX_NEST_DEPTH {
        return;
    }
    let indices: Vec<(u32, String)> = archive
        .nodes
        .iter()
        .filter(|node| !node.is_dir())
        .filter(|node| {
            node.path()
                .rsplit('.')
                .next()
                .is_some_and(|e| e.eq_ignore_ascii_case("rpf"))
        })
        .map(|node| (node.entry_index(), node.path().replace('\\', "/")))
        .collect();

    for (entry_index, inside) in indices {
        let nested_label = format!("{label}/{inside}");
        match archive.open_nested(entry_index) {
            Ok(nested) => {
                census.nested_archives += 1;
                census.record_archive(&nested_label, &nested);
                let nested = Arc::new(nested);
                walk_nested(census, &nested, &nested_label, depth + 1);
            }
            Err(_) => census.nested_unreadable += 1,
        }
    }
}

fn scan(census: &mut Census, root: &Path, crypto: Arc<dyn RpfCryptoProvider>, verbose: bool) {
    let mut paths = Vec::new();
    archives_under(root, &mut paths);
    paths.sort();
    eprintln!("{}: {} archive(s) on disk", root.display(), paths.len());

    for (i, path) in paths.iter().enumerate() {
        if verbose && i > 0 && i % 25 == 0 {
            eprintln!("  {i}/{} ...", paths.len());
        }
        let label = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        match RpfArchive::open_with(path, Arc::clone(&crypto)) {
            Ok(archive) => {
                census.archives += 1;
                census.record_archive(&label, &archive);
                let archive = Arc::new(archive);
                walk_nested(census, &archive, &label, 0);
            }
            Err(e) => {
                census.archives_unreadable += 1;
                if verbose {
                    eprintln!("  warn: {label}: {e}");
                }
            }
        }
    }
}

fn report(census: &Census) {
    println!();
    println!("Archive shape census");
    println!(
        "  {} top-level archive(s) read, {} unreadable",
        census.archives, census.archives_unreadable
    );
    println!(
        "  {} nested archive(s) read, {} unreadable",
        census.nested_archives, census.nested_unreadable
    );
    println!(
        "  {} director(ies), {} binar(ies), {} resource(s)",
        census.directories, census.binaries, census.resources
    );

    println!();
    println!("  archive encryption mode:");
    for (raw, count) in &census.modes {
        println!("    {:>24}  {count}", mode_name(*raw));
    }

    println!();
    println!("  binary entry high word (`encrypted_raw`):");
    for (raw, count) in &census.entry_flags {
        let note = match raw {
            0 => "  plaintext",
            1 => "  encrypted",
            _ => "  <- NOT a value this reader can tell apart",
        };
        println!("    {raw:#010x}  {count}{note}");
    }

    println!();
    if census.unknown_modes.is_empty() && census.unknown_entry_flags.is_empty() {
        println!(
            "  ESCROW: no archive and no entry in this install carries a marker \
             this reader cannot already name."
        );
        println!(
            "  Nothing to write a detector against. Record the zero with the \
             denominators above."
        );
    } else {
        println!("  ESCROW CANDIDATES — a detector now has an example to be written against:");
        for (label, raw) in &census.unknown_modes {
            println!("    archive {label}: encryption {raw:#010x}");
        }
        for (label, name, raw) in census.unknown_entry_flags.iter().take(40) {
            println!("    entry {label} :: {name}: high word {raw:#010x}");
        }
        if census.unknown_entry_flags.len() > 40 {
            println!(
                "    ... and {} more entr(ies)",
                census.unknown_entry_flags.len() - 40
            );
        }
    }
}

pub fn print_usage() {
    eprintln!("Usage: openrage-cli archives --game <path> [--game <path>...] [options]");
    eprintln!();
    eprintln!("Walks every .rpf under each install, including nested ones, and reports");
    eprintln!("the distribution of archive encryption modes and binary-entry flags.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --game <path>   An install root. May be given more than once.");
    eprintln!("  --keys <path>   A keys cache to use instead of deriving one.");
    eprintln!("  --verbose       Progress and per-archive warnings.");
}

pub fn run(args: &[String]) -> ExitCode {
    match run_inner(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_inner(args: &[String]) -> Result<ExitCode, String> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut keys: Option<PathBuf> = None;
    let mut verbose = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--game" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--game needs a path".to_string())?;
                roots.push(PathBuf::from(v));
                i += 2;
            }
            "--keys" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--keys needs a path".to_string())?;
                keys = Some(PathBuf::from(v));
                i += 2;
            }
            "--verbose" | "-v" => {
                verbose = true;
                i += 1;
            }
            "-h" | "--help" => {
                print_usage();
                return Ok(ExitCode::SUCCESS);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if roots.is_empty() {
        print_usage();
        return Err("no --game given".to_string());
    }

    let mut census = Census::default();
    for root in &roots {
        if !root.is_dir() {
            return Err(format!("not a directory: {}", root.display()));
        }
        let resolved = super::keys::resolve_crypto(root, keys.as_deref());
        for note in &resolved.notes {
            eprintln!("note: {note}");
        }
        scan(&mut census, root, resolved.provider, verbose);
    }

    report(&census);
    Ok(ExitCode::SUCCESS)
}
