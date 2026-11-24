//! `rpf9`: the GTA VI (RPF9) archive explorer. `rpf9 tree <archive.rpf>` deciphers an archive's
//! table of contents and prints its entries — each entry's name64 and, where a recovered-names
//! corpus is given, its path — and, with `--nested`, the RPF9 archives held inside it.
//!
//! The key snapshot and the recovered-names corpus are game-derived and read at runtime; neither is
//! committed. Without a snapshot the archive cannot be deciphered and the command says so; without
//! the names corpus every entry shows as its hash.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_io::container::rpf::{Gta6Archive, GtaViKeys, NameResolver, Rpf9Header, NESTED_SCAN_CAP};

/// The environment variable that names a folder of recovered-names tables (`resolved-names.tsv`,
/// `toc-entries-resolved.tsv`), used when `--names` is not given.
const NAMES_ENV: &str = "OPENRAGE_GTA6_NAMES";

pub(crate) fn run(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("tree") => tree(&args[1..]),
        _ => {
            eprintln!("usage: openrage-cli rpf9 tree <archive.rpf> [--names <dir>] [--keys <snapshot>] [--groups] [--nested]");
            ExitCode::from(2)
        }
    }
}

fn tree(args: &[String]) -> ExitCode {
    let mut archive: Option<&str> = None;
    let mut names_dir: Option<PathBuf> = None;
    let mut keys_path: Option<PathBuf> = None;
    let mut show_groups = false;
    let mut nested = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--names" => match it.next() {
                Some(dir) => names_dir = Some(PathBuf::from(dir)),
                None => return usage("--names needs a folder"),
            },
            "--keys" => match it.next() {
                Some(path) => keys_path = Some(PathBuf::from(path)),
                None => return usage("--keys needs a snapshot file"),
            },
            "--groups" => show_groups = true,
            "--nested" => nested = true,
            other if other.starts_with("--") => return usage(&format!("unknown option {other}")),
            other => {
                if archive.replace(other).is_some() {
                    return usage("give one archive");
                }
            }
        }
    }
    let Some(archive) = archive else {
        return usage("give an archive to list");
    };

    let keys = match load_keys(keys_path.as_deref()) {
        Ok(Some(keys)) => keys,
        Ok(None) => {
            eprintln!(
                "No GTA VI key snapshot: set {} to one, or pass --keys, or place {} beside the keys cache.",
                or_io::container::rpf::GTA6_SNAPSHOT_ENV,
                or_io::container::rpf::GTA6_SNAPSHOT_FILE_NAME,
            );
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("Error reading the key snapshot: {e}");
            return ExitCode::FAILURE;
        }
    };

    let resolver = load_resolver(names_dir.as_deref());
    if resolver.is_empty() {
        eprintln!("(no recovered names found; entries show as their hash)");
    }

    match list_archive(Path::new(archive), &keys, &resolver, show_groups, nested) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("Error: {msg}");
    eprintln!("usage: openrage-cli rpf9 tree <archive.rpf> [--names <dir>] [--keys <snapshot>] [--groups] [--nested]");
    ExitCode::from(2)
}

/// The key snapshot: the one `--keys` names, else the one a session supplies for the install.
fn load_keys(explicit: Option<&Path>) -> Result<Option<GtaViKeys>, String> {
    if let Some(path) = explicit {
        return GtaViKeys::from_file(path)
            .map(Some)
            .map_err(|e| e.to_string());
    }
    GtaViKeys::for_install(or_title::config::config_dir().as_deref()).map_err(|e| e.to_string())
}

/// Load the recovered-names corpus from `dir` (or the folder [`NAMES_ENV`] names): both the 32-bit
/// `resolved-names.tsv` and the name64-keyed `toc-entries-resolved.tsv` when present. An empty
/// resolver is returned when neither file is there, so a missing corpus never blocks.
fn load_resolver(dir: Option<&Path>) -> NameResolver {
    let mut resolver = NameResolver::new();
    let Some(dir) = dir
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os(NAMES_ENV).map(PathBuf::from))
    else {
        return resolver;
    };
    if let Ok(text) = std::fs::read_to_string(dir.join("toc-entries-resolved.tsv")) {
        resolver.add_toc_entries_tsv(&text);
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("resolved-names.tsv")) {
        resolver.add_hash_name_tsv(&text);
    }
    resolver
}

/// Read a GTA VI archive's header and table of contents from `path` without loading its content: the
/// first 64 bytes give the table's end, and that many bytes are read. For `--nested` the whole file
/// is read instead so the content region can be scanned for nested headers — but only when its
/// content fits the scan cap; the returned flag is `true` only when the whole file was loaded.
fn read_prefix(path: &Path, whole: bool) -> Result<(Vec<u8>, bool), String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut head = [0u8; 64];
    file.read_exact(&mut head)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let header = Rpf9Header::parse(&head).map_err(|e| e.to_string())?;
    let toc_end = header.toc_end as u64;
    let file_len = file.metadata().map_err(|e| e.to_string())?.len();
    // The content fits the scan when it is no larger than the cap; otherwise only the table is read
    // and the nested scan is skipped rather than loading a slice of a many-gigabyte file.
    let whole = whole && file_len.saturating_sub(toc_end) <= NESTED_SCAN_CAP as u64;
    let want =
        usize::try_from(if whole { file_len } else { toc_end }).map_err(|e| e.to_string())?;
    let mut bytes = vec![0u8; want];
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    file.read_exact(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((bytes, whole))
}

fn list_archive(
    path: &Path,
    keys: &GtaViKeys,
    resolver: &NameResolver,
    show_groups: bool,
    nested: bool,
) -> Result<(), String> {
    let (bytes, content_loaded) = read_prefix(path, nested)?;
    let mut archive = Gta6Archive::open(&bytes, keys).map_err(|e| e.to_string())?;
    archive.resolve_names(resolver);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("archive");
    println!("{name}: {}", counts(&archive));
    print_listing(&archive, show_groups, 0);

    if nested && !content_loaded {
        println!(
            "  (too large to scan for nested archives: over {} MiB of content)",
            NESTED_SCAN_CAP / (1024 * 1024)
        );
    } else if nested {
        // The scan over the content region is flat and exhaustive: it finds every nested RPF9 header,
        // including one inside another nested archive, so there is no recursion to do — a recursive
        // rescan of each child's window would re-report the same headers combinatorially.
        if let Some(children) = archive.nested(&bytes, keys).map_err(|e| e.to_string())? {
            println!("  {} nested RPF9 archive(s):", children.len());
            for (offset, mut child) in children {
                child.resolve_names(resolver);
                println!("  nested RPF9 at {offset:#X}: {}", counts(&child));
                print_listing(&child, show_groups, 1);
            }
        }
    }
    Ok(())
}

/// The entry-count line for an archive: how many rows the listing holds and how many the header
/// states, which differ when the engine deduplicated a column, plus the index key.
fn counts(archive: &Gta6Archive) -> String {
    format!(
        "{} entries listed of {} the header states, index key {:#010X}",
        archive.entries.len(),
        archive.toc.entry_count,
        archive.toc.key_id
    )
}

fn print_listing(archive: &Gta6Archive, show_groups: bool, depth: usize) {
    let indent = "  ".repeat(depth + 1);
    if show_groups {
        println!("{indent}groups:");
        for group in archive.groups() {
            println!(
                "{indent}  {:#018X}  count {:>6}  width {:>3}  {}  [{}, {})",
                group.name_hash,
                group.count,
                group.width,
                if group.external { "ext " } else { "main" },
                group.data.start,
                group.data.end,
            );
        }
        println!("{indent}entries:");
    }
    for entry in &archive.entries {
        let size = entry
            .size()
            .map(|s| format!("{s:>10}"))
            .unwrap_or_else(|| "         -".to_string());
        let name = entry.name.as_deref().unwrap_or("<name not recovered>");
        println!(
            "{indent}  {:>6}  {:#018X}  {size}  {name}",
            entry.index, entry.name64
        );
    }
}
