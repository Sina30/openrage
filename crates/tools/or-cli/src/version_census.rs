//! `openrage-cli versions`: which RSC7 resource versions an install actually
//! ships, per resource type. Only each file's 16-byte RSC7 header is inspected.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_io::container::resource::ResourceType;
use or_io::container::resource::{ResourceEnvelope, Rsc7, Rsc7Seal};
use or_io::Generation;
use or_job::human_bytes;

use super::keys;

/// Every type the version table states a version for, because this census is
/// what that table is measured by.
const CENSUS_TYPES: [ResourceType; 17] = ResourceType::ALL;

fn is_census_extension(ext: &str) -> Option<ResourceType> {
    ResourceType::from_extension(ext).filter(|rt| CENSUS_TYPES.contains(rt))
}

#[derive(Debug, Default, Clone)]
struct VersionTally {
    files: u64,
    bytes: u64,
    nibbles: BTreeMap<(u8, u8), u64>,
    examples: Vec<String>,
}

impl VersionTally {
    fn record(&mut self, seal: &Rsc7Seal, size: u64, source: &str) {
        self.files += 1;
        self.bytes += size;
        let pair = seal.version_nibbles();
        *self.nibbles.entry(pair).or_default() += 1;
        if self.examples.len() < 3 {
            self.examples.push(source.to_string());
        }
    }
}

#[derive(Debug, Default)]
struct Census {
    versions: BTreeMap<(&'static str, u32), VersionTally>,
    files_scanned: u64,
    // Files with a census extension whose header is not RSC7.
    files_not_resources: u64,
    files_unreadable: u64,
}

impl Census {
    fn record(&mut self, resource_type: ResourceType, seal: &Rsc7Seal, size: u64, source: &str) {
        self.versions
            .entry((resource_type.extension(), seal.version()))
            .or_default()
            .record(seal, size, source);
    }

    fn unknown_versions(&self) -> Vec<(&'static str, u32, &VersionTally)> {
        self.versions
            .iter()
            .filter(|((ext, version), _)| {
                ResourceType::from_extension(ext)
                    .and_then(|rt| rt.generation_of_version(*version))
                    .is_none()
            })
            .map(|((ext, version), tally)| (*ext, *version, tally))
            .collect()
    }

    fn generation_of(ext: &str, version: u32) -> Option<Generation> {
        ResourceType::from_extension(ext).and_then(|rt| rt.generation_of_version(version))
    }
}

fn scan_header(
    census: &mut Census,
    resource_type: ResourceType,
    header_bytes: &[u8],
    size: u64,
    source: &str,
) {
    census.files_scanned += 1;
    match Rsc7::peek(header_bytes) {
        Ok(seal) => census.record(resource_type, &seal, size, source),
        Err(_) => census.files_not_resources += 1,
    }
}

fn scan_directory(census: &mut Census, root: &Path, verbose: bool) {
    let mut stack = vec![root.to_path_buf()];
    let mut files: Vec<(PathBuf, ResourceType)> = Vec::new();

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(rt) = path
                .extension()
                .and_then(|e| e.to_str())
                .and_then(is_census_extension)
            {
                files.push((path, rt));
            }
        }
    }

    files.sort();

    for (path, rt) in files {
        let mut buf = [0u8; 16];
        let Ok(size) = read_prefix(&path, &mut buf) else {
            census.files_unreadable += 1;
            if verbose {
                eprintln!("  unreadable {}", path.display());
            }
            continue;
        };
        let source = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        scan_header(census, rt, &buf, size, &source);
    }
}

fn read_prefix(path: &Path, buf: &mut [u8; 16]) -> std::io::Result<u64> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    // A file shorter than an RSC7 header is an `UnexpectedEof`.
    file.read_exact(buf)?;
    Ok(size)
}

fn scan_install(
    census: &mut Census,
    game_root: &Path,
    keys_path: Option<&Path>,
    verbose: bool,
) -> Result<(), String> {
    let game = keys::mount_all(game_root, keys_path)?;

    for finding in game.diagnostics.messages() {
        eprintln!("warning: {finding}");
    }
    eprintln!(
        "install detected as GTA V {} edition",
        game.game.generation().edition_label()
    );

    let mut targets: Vec<(String, ResourceType)> = Vec::new();
    game.for_each_file(|logical, _container, leaf| {
        if let Some(rt) = leaf.rsplit('.').next().and_then(is_census_extension) {
            targets.push((logical.to_string(), rt));
        }
    });
    targets.sort();
    targets.dedup();

    eprintln!("scanning {} resource files...", targets.len());

    for (i, (logical, rt)) in targets.iter().enumerate() {
        if verbose && i > 0 && i % 20000 == 0 {
            eprintln!("  {i}/{} ...", targets.len());
        }
        let Some(data) = game.vfs.open(logical) else {
            census.files_unreadable += 1;
            continue;
        };
        scan_header(census, *rt, &data, data.len() as u64, logical);
    }

    Ok(())
}

fn print_report(census: &Census) {
    println!();
    println!("Resource version census");
    println!(
        "  files scanned {}   not RSC7 {}   unreadable {}",
        census.files_scanned, census.files_not_resources, census.files_unreadable
    );
    println!();

    if census.versions.is_empty() {
        println!("  no RSC7 resources found");
        return;
    }

    println!(
        "  {:<6} {:>8}  {:<12} {:<9} {:>12}  nibbles",
        "ext", "version", "generation", "files", "bytes"
    );
    println!("  {}", "-".repeat(76));

    let mut last_ext = "";
    for ((ext, version), tally) in &census.versions {
        if !last_ext.is_empty() && *ext != last_ext {
            println!();
        }
        last_ext = ext;
        let generation = match Census::generation_of(ext, *version) {
            Some(g) => g.slug().to_string(),
            None => "UNKNOWN".to_string(),
        };
        let nibbles = tally
            .nibbles
            .keys()
            .map(|(v, p)| format!("({v:#x},{p:#x})"))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "  {:<6} {:>8}  {:<12} {:<9} {:>12}  {}",
            ext,
            version,
            generation,
            tally.files,
            human_bytes(tally.bytes),
            nibbles
        );
    }

    let unknown = census.unknown_versions();
    println!();
    if unknown.is_empty() {
        println!("  every version present maps to a known generation");
    } else {
        println!(
            "  {} (extension, version) pair(s) map to neither generation:",
            unknown.len()
        );
        for (ext, version, tally) in unknown {
            println!(
                "    .{ext} v{version} ({} files) e.g. {}",
                tally.files,
                tally.examples.join(", ")
            );
        }
    }
    println!();
}

fn print_json(census: &Census) {
    let entries: Vec<serde_json::Value> = census
        .versions
        .iter()
        .map(|((ext, version), tally)| {
            serde_json::json!({
                "extension": ext,
                "version": version,
                "generation": Census::generation_of(ext, *version).map(|g| g.slug()),
                "files": tally.files,
                "bytes": tally.bytes,
                "nibbles": tally
                    .nibbles
                    .iter()
                    .map(|((v, p), count)| serde_json::json!({
                        "virtual": v,
                        "physical": p,
                        "files": count,
                    }))
                    .collect::<Vec<_>>(),
                "examples": tally.examples,
            })
        })
        .collect();

    let doc = serde_json::json!({
        "filesScanned": census.files_scanned,
        "filesNotResources": census.files_not_resources,
        "filesUnreadable": census.files_unreadable,
        "versions": entries,
        "unknown": census
            .unknown_versions()
            .iter()
            .map(|(ext, version, _)| format!(".{ext} v{version}"))
            .collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
}

pub fn print_usage() {
    eprintln!("Tally the RSC7 resource versions present, per file extension");
    eprintln!();
    eprintln!("Usage: openrage-cli versions <dir> [--json] [-v]");
    eprintln!("       openrage-cli versions --game <install-root> [--keys <file>] [--json] [-v]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --game <path>   Mount a GTA V install and walk it through the VFS");
    eprintln!("  --keys <path>   Key cache for encrypted archives (default: auto)");
    eprintln!("  --json          Emit machine-readable output");
    eprintln!("  -v, --verbose   Report scan progress and unreadable files");
    eprintln!();
    eprintln!("Reads only each file's 16-byte header, so a whole install takes seconds.");
    eprintln!("Exits 1 when a version maps to neither Legacy nor Enhanced.");
}

pub fn run(args: &[String]) -> ExitCode {
    match run_inner(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_inner(args: &[String]) -> Result<ExitCode, String> {
    let mut dir: Option<PathBuf> = None;
    let mut game: Option<PathBuf> = None;
    let mut keys_path: Option<PathBuf> = None;
    let mut json = false;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                return Ok(ExitCode::SUCCESS);
            }
            "--json" => json = true,
            "-v" | "--verbose" => verbose = true,
            "--game" => {
                i += 1;
                game = Some(PathBuf::from(
                    args.get(i).ok_or("--game needs a path".to_string())?,
                ));
            }
            "--keys" => {
                i += 1;
                keys_path = Some(PathBuf::from(
                    args.get(i).ok_or("--keys needs a path".to_string())?,
                ));
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'"));
            }
            other => dir = Some(PathBuf::from(other)),
        }
        i += 1;
    }

    let mut census = Census::default();

    match (&game, &dir) {
        (Some(root), _) => scan_install(&mut census, root, keys_path.as_deref(), verbose)?,
        (None, Some(root)) => {
            if !root.exists() {
                return Err(format!("no such path: {}", root.display()));
            }
            if root.is_dir() {
                scan_directory(&mut census, root, verbose);
            } else {
                let Some(rt) = root
                    .extension()
                    .and_then(|e| e.to_str())
                    .and_then(is_census_extension)
                else {
                    return Err(format!(
                        "not a known resource extension: {}",
                        root.display()
                    ));
                };
                let data = std::fs::read(root).map_err(|e| e.to_string())?;
                let source = root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                let size = data.len() as u64;
                scan_header(&mut census, rt, &data, size, &source);
            }
        }
        (None, None) => {
            print_usage();
            return Ok(ExitCode::from(2));
        }
    }

    if json {
        print_json(&census);
    } else {
        print_report(&census);
    }

    Ok(if census.unknown_versions().is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ A short file is counted unreadable, not read as a header of zeros.
    #[test]
    fn a_file_shorter_than_a_header_is_refused_and_a_longer_one_reads_its_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let short = dir.path().join("short.ydr");
        std::fs::write(&short, [1u8; 15]).expect("write");
        let mut buf = [0u8; 16];
        let err = read_prefix(&short, &mut buf).expect_err("15 bytes is no header");
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);

        let long = dir.path().join("long.ydr");
        let bytes: Vec<u8> = (0..40).collect();
        std::fs::write(&long, &bytes).expect("write");
        assert_eq!(read_prefix(&long, &mut buf).expect("reads"), 40);
        assert_eq!(buf[..], bytes[..16]);
    }
}
