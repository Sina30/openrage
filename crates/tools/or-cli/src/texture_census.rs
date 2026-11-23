use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_io::codec::texture::{decode_level, DecodeSupport, GrcTextureFormat, TextureSource};
use or_io::decl::Gen8;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::{File as _, GtaDrawable, PgDictionary, PtxFxList};
use or_job::human_bytes;

use super::keys;

const TEXTURE_BEARING: [&str; 4] = ["ytd", "ydr", "ydd", "ypt"];

/// How many decode failures the census keeps the text of.
///
/// ⚠ `--verify` decodes every texture it finds — 513,528 of them on a retail
/// Legacy install — and a build whose decoder is broken fails on all of them,
/// so a formatted line per failure is unbounded. Nothing else here grows with
/// the data.
///
/// ⚠ **It bounds the retained text and not the count.** How many failed is
/// what the report and the exit code turn on, and it stays exact however
/// many lines were kept — so a truncated run still fails, and still says how
/// many.
const FAILURE_SAMPLE: usize = 200;

#[derive(Debug, Default, Clone)]
struct FormatTally {
    textures: u64,
    pixels: u64,
    payload_bytes: u64,
    examples: Vec<String>,
}

impl FormatTally {
    fn record(&mut self, texture: &Seen<'_>, source: &str) {
        self.textures += 1;
        self.pixels += u64::from(texture.width) * u64::from(texture.height);
        self.payload_bytes += texture.pixels.payload().len() as u64;
        if self.examples.len() < 3 {
            self.examples.push(format!("{source}:{}", texture.name));
        }
    }
}

/// What the census reads of a texture, from either target.
struct Seen<'a> {
    name: &'a str,
    width: u16,
    height: u16,
    pixels: &'a dyn TextureSource,
}

/// Decode failures: all of them counted, a bounded sample of them spelled.
#[derive(Debug)]
struct Failures {
    /// ⚠ Not `kept.len()`. The report and the exit code both turn on how many
    /// textures failed, which is not how many lines were room for.
    total: u64,
    kept: Vec<String>,
    ceiling: Option<usize>,
    /// The ceiling, once it has been reached.
    stopped_at: Option<usize>,
}

impl Default for Failures {
    fn default() -> Self {
        Self {
            total: 0,
            kept: Vec::new(),
            ceiling: Some(FAILURE_SAMPLE),
            stopped_at: None,
        }
    }
}

impl Failures {
    fn record(&mut self, line: String) {
        self.total += 1;
        match self.ceiling {
            Some(limit) if self.kept.len() >= limit => self.stopped_at = Some(limit),
            _ => self.kept.push(line),
        }
    }

    fn is_empty(&self) -> bool {
        self.total == 0
    }

    fn truncated(&self) -> Option<usize> {
        self.stopped_at
    }
}

#[derive(Debug, Default)]
struct Census {
    formats: BTreeMap<u32, (GrcTextureFormat, FormatTally)>,
    files_scanned: u64,
    files_failed: u64,
    textures: u64,
    verified: u64,
    failures: Failures,
}

impl Census {
    fn record(&mut self, texture: &Seen<'_>, source: &str, verify: bool) {
        let format = texture.pixels.pixel_format();
        self.textures += 1;
        self.formats
            .entry(format as u32)
            .or_insert_with(|| (format, FormatTally::default()))
            .1
            .record(texture, source);

        let has_pixels = !texture.pixels.payload().is_empty();
        if verify && has_pixels && format.decode_support().is_decodable() {
            match decode_level(texture.pixels, 0) {
                Ok(_) => self.verified += 1,
                Err(e) => self.failures.record(format!(
                    "{source}:{} ({:?} {}x{}): {e}",
                    texture.name, format, texture.width, texture.height
                )),
            }
        }
    }

    fn record_dictionary(&mut self, dict: &PgDictionary<GrcTexture>, source: &str, verify: bool) {
        for texture in dict.entries.iter().flatten() {
            let seen = Seen {
                name: &texture.name,
                width: texture.width,
                height: texture.height,
                pixels: &**texture,
            };
            self.record(&seen, source, verify);
        }
    }

    fn undecodable(&self) -> Vec<(GrcTextureFormat, &FormatTally)> {
        self.formats
            .values()
            .filter(|(f, _)| !f.decode_support().is_decodable())
            .filter(|(f, _)| f.decode_support() != DecodeSupport::Empty)
            .map(|(f, t)| (*f, t))
            .collect()
    }
}

fn scan_bytes(
    census: &mut Census,
    path: &Path,
    data: &[u8],
    source: &str,
    verify: bool,
) -> Result<(), String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "ytd" => {
            let dict =
                crate::texture::Listing::from_bytes(path, data).map_err(|e| e.to_string())?;
            for texture in dict.textures() {
                let seen = Seen {
                    name: texture.name,
                    width: texture.width,
                    height: texture.height,
                    pixels: texture.pixels,
                };
                census.record(&seen, source, verify);
            }
        }
        "ydr" => {
            let drawable = GtaDrawable::<Gen8>::from_bytes(data).map_err(|e| e.to_string())?;
            if let Some(dict) = embedded(&drawable) {
                census.record_dictionary(dict, source, verify);
            }
        }
        "ydd" => {
            let dict = PgDictionary::<GtaDrawable>::from_bytes(data).map_err(|e| e.to_string())?;
            for drawable in dict.entries.iter().flatten() {
                if let Some(embedded) = embedded(drawable) {
                    census.record_dictionary(embedded, source, verify);
                }
            }
        }
        "ypt" => {
            let list = PtxFxList::<Gen8>::from_bytes(data).map_err(|e| e.to_string())?;
            if let Some(dict) = list.texture_dict.as_deref() {
                census.record_dictionary(dict, source, verify);
            }
        }
        other => return Err(format!("not a texture-bearing extension: .{other}")),
    }
    Ok(())
}

/// The textures a drawable's shader group embeds.
fn embedded(drawable: &GtaDrawable) -> Option<&PgDictionary<GrcTexture>> {
    drawable
        .shader_group
        .as_deref()?
        .texture_dictionary
        .as_deref()
}

fn scan_directory(census: &mut Census, root: &Path, verbose: bool, verify: bool) {
    let mut stack = vec![root.to_path_buf()];
    let mut files: Vec<PathBuf> = Vec::new();

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| TEXTURE_BEARING.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
            {
                files.push(path);
            }
        }
    }

    files.sort();

    for path in files {
        let Ok(data) = std::fs::read(&path) else {
            census.files_failed += 1;
            continue;
        };
        let source = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        census.files_scanned += 1;
        if let Err(e) = scan_bytes(census, &path, &data, &source, verify) {
            census.files_failed += 1;
            if verbose {
                eprintln!("  skip {}: {e}", path.display());
            }
        }
    }
}

fn scan_install(
    census: &mut Census,
    game_root: &Path,
    keys_path: Option<&Path>,
    verbose: bool,
    verify: bool,
) -> Result<(), String> {
    let game = keys::mount_all(game_root, keys_path)?;

    for finding in game.diagnostics.messages() {
        eprintln!("warning: {finding}");
    }

    let mut targets: Vec<String> = Vec::new();
    game.for_each_file(|logical, _container, leaf| {
        let is_target = leaf
            .rsplit('.')
            .next()
            .map(|e| TEXTURE_BEARING.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or(false);
        if is_target {
            targets.push(logical.to_string());
        }
    });
    targets.sort();
    targets.dedup();

    eprintln!("scanning {} texture-bearing files...", targets.len());

    for (i, logical) in targets.iter().enumerate() {
        if verbose && i > 0 && i % 5000 == 0 {
            eprintln!("  {i}/{} ...", targets.len());
        }
        let Some(data) = game.vfs.open(logical) else {
            census.files_failed += 1;
            continue;
        };
        census.files_scanned += 1;
        let path = PathBuf::from(logical);
        if let Err(e) = scan_bytes(census, &path, &data, logical, verify) {
            census.files_failed += 1;
            if verbose {
                eprintln!("  skip {logical}: {e}");
            }
        }
    }

    Ok(())
}

fn print_report(census: &Census) {
    println!();
    println!("Texture format census");
    println!(
        "  files scanned {}  (failed {})   textures {}",
        census.files_scanned, census.files_failed, census.textures
    );
    println!();

    if census.textures == 0 {
        println!("  no textures found");
        return;
    }

    println!(
        "  {:<24} {:>9} {:>7}  {:>12}  {:<14}",
        "format", "textures", "share", "payload", "decode"
    );
    println!("  {}", "-".repeat(72));

    let mut rows: Vec<_> = census.formats.values().collect();
    rows.sort_by_key(|(format, tally)| (std::cmp::Reverse(tally.textures), *format as u32));

    for (format, tally) in rows {
        let share = (tally.textures as f64 / census.textures as f64) * 100.0;
        println!(
            "  {:<24} {:>9} {:>6.2}%  {:>12}  {:<14}",
            format!("{format:?}"),
            tally.textures,
            share,
            human_bytes(tally.payload_bytes),
            format.decode_support().label(),
        );
    }

    let gaps = census.undecodable();
    println!();
    if gaps.is_empty() {
        println!("  every format present decodes to real pixels");
    } else {
        println!("  {} format(s) present with no usable decoder:", gaps.len());
        for (format, tally) in gaps {
            println!(
                "    {format:?} ({} textures) e.g. {}",
                tally.textures,
                tally.examples.join(", ")
            );
        }
    }

    if census.verified > 0 || !census.failures.is_empty() {
        println!();
        println!(
            "  verified: {} decoded, {} failed",
            census.verified, census.failures.total
        );
        for failure in census.failures.kept.iter().take(20) {
            println!("    {failure}");
        }
        let shown = census.failures.kept.len().min(20) as u64;
        if census.failures.total > shown {
            println!("    ... and {} more", census.failures.total - shown);
        }
        if let Some(limit) = census.failures.truncated() {
            // ⚠ A census that truncates silently is worse than one that
            // refuses, because the figure it prints is quotable and wrong.
            println!(
                "    ⚠ TRUNCATED: the text of only the first {limit} failure(s) was kept, \
                 at the retained-sample ceiling. The {} count above is exact. Raise it \
                 with --failure-sample N, or remove it with --all-failures.",
                census.failures.total
            );
        }
    }
    println!();
}

fn print_json(census: &Census) {
    let entries: Vec<serde_json::Value> = census
        .formats
        .values()
        .map(|(format, tally)| {
            serde_json::json!({
                "format": format!("{format:?}"),
                "raw": format.to_raw_format(),
                "textures": tally.textures,
                "pixels": tally.pixels,
                "payloadBytes": tally.payload_bytes,
                "decodeSupport": format.decode_support().label(),
                "examples": tally.examples,
            })
        })
        .collect();

    let doc = serde_json::json!({
        "filesScanned": census.files_scanned,
        "filesFailed": census.files_failed,
        "textures": census.textures,
        "verified": census.verified,
        // ⚠ The count and the sample are separate keys: a consumer reading
        // `len(decodeFailures)` as the failure count reads the ceiling instead.
        "decodeFailureCount": census.failures.total,
        "decodeFailuresTruncated": census.failures.truncated().is_some(),
        "decodeFailures": census.failures.kept,
        "formats": entries,
        "undecodable": census
            .undecodable()
            .iter()
            .map(|(f, _)| format!("{f:?}"))
            .collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
}

pub fn print_usage(program: &str) {
    eprintln!("Tally the pixel formats present in a directory or a whole install");
    eprintln!();
    eprintln!("Usage: {program} census <dir> [--verify] [--json] [-v]");
    eprintln!("       {program} census --game <install-root> [--keys <file>] [--verify] [--json]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --game <path>   Mount a GTA V install and walk it through the VFS");
    eprintln!("  --keys <path>   Key cache for encrypted archives (default: auto)");
    eprintln!("  --verify        Actually decode every texture, not just tally formats.");
    eprintln!("                  Slower, but catches files whose dimensions or mip");
    eprintln!("                  offsets break a decoder that otherwise exists.");
    eprintln!("  --json          Emit machine-readable output");
    eprintln!("  -v, --verbose   Report per-file skips and scan progress");
    eprintln!("  --failure-sample N");
    eprintln!("                  Keep the text of N decode failures (default 200). The");
    eprintln!("                  count is always exact; this bounds only the lines kept.");
    eprintln!("  --all-failures  Keep every failure's text. An install-wide --verify on a");
    eprintln!("                  build with a broken decoder can fail on every texture,");
    eprintln!("                  so this is unbounded by design.");
    eprintln!();
    eprintln!("Exits 1 when a format has no decoder, or when --verify hits a failure.");
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut dir: Option<PathBuf> = None;
    let mut game: Option<PathBuf> = None;
    let mut keys_path: Option<PathBuf> = None;
    let mut json = false;
    let mut verbose = false;
    let mut verify = false;
    let mut ceiling = Some(FAILURE_SAMPLE);

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage("openrage-cli texture");
                return Ok(ExitCode::SUCCESS);
            }
            "--json" => json = true,
            "--verify" => verify = true,
            "-v" | "--verbose" => verbose = true,
            "--all-failures" => ceiling = None,
            "--failure-sample" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or("--failure-sample needs a number".to_string())?;
                match value.parse::<usize>() {
                    Ok(n) => ceiling = Some(n),
                    Err(_) => return Err("--failure-sample needs a number".to_string()),
                }
            }
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
    census.failures.ceiling = ceiling;
    if ceiling.is_none() && verify {
        eprintln!(
            "openrage-cli: --all-failures removes the {FAILURE_SAMPLE}-line ceiling on \
             retained failure text. A --verify sweep of a whole install on a build whose \
             decoder is broken fails on every texture, so watch it."
        );
    }

    match (&game, &dir) {
        (Some(root), _) => scan_install(&mut census, root, keys_path.as_deref(), verbose, verify)?,
        (None, Some(root)) => {
            if !root.exists() {
                return Err(format!("no such path: {}", root.display()));
            }
            if root.is_dir() {
                scan_directory(&mut census, root, verbose, verify);
            } else {
                let data = std::fs::read(root).map_err(|e| e.to_string())?;
                let source = root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                census.files_scanned += 1;
                scan_bytes(&mut census, root, &data, &source, verify)?;
            }
        }
        (None, None) => {
            print_usage("openrage-cli texture");
            return Ok(ExitCode::from(2));
        }
    }

    if json {
        print_json(&census);
    } else {
        print_report(&census);
    }

    Ok(
        if census.undecodable().is_empty() && census.failures.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failures(ceiling: Option<usize>, count: usize) -> Failures {
        let mut out = Failures {
            ceiling,
            ..Failures::default()
        };
        for i in 0..count {
            out.record(format!("failure {i}"));
        }
        out
    }

    /// ⚠ The count is what the report and the exit code read, and it must
    /// survive the ceiling: a run that kept 200 lines of 9,000 failures still
    /// failed 9,000 times and must still exit non-zero.
    #[test]
    fn the_count_is_exact_however_few_lines_were_kept() {
        let over = failures(Some(3), 9_000);
        assert_eq!(over.total, 9_000);
        assert_eq!(over.kept.len(), 3);
        assert!(!over.is_empty());
        assert_eq!(over.truncated(), Some(3));
    }

    /// A run inside the ceiling must not claim it stopped short, or the
    /// warning stops meaning anything.
    #[test]
    fn a_run_inside_the_ceiling_does_not_say_it_truncated() {
        let under = failures(Some(200), 199);
        assert_eq!(under.kept.len(), 199);
        assert_eq!(under.truncated(), None);

        let exact = failures(Some(200), 200);
        assert_eq!(exact.kept.len(), 200);
        assert_eq!(
            exact.truncated(),
            None,
            "the ceiling is reached, not exceeded"
        );
    }

    #[test]
    fn the_override_is_the_only_way_to_keep_everything() {
        assert_eq!(Failures::default().ceiling, Some(FAILURE_SAMPLE));

        let all = failures(None, 5_000);
        assert_eq!(all.kept.len(), 5_000);
        assert_eq!(all.truncated(), None);
    }

    #[test]
    fn no_failures_is_not_a_truncated_run() {
        let none = failures(Some(200), 0);
        assert!(none.is_empty());
        assert_eq!(none.truncated(), None);
    }
}
