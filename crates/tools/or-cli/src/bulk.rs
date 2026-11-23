//! Extract from a whole install, by pattern, through the VFS.
//!
//! `openrage-cli bulk --game <root> --glob '**/*.ytd' --out <dir>`.
//!
//! ⚠ **It holds no `RpfArchive`.** "Every texture in the install" is a question
//! about DLC override order and a logical namespace, and walking archives
//! directly would be a second implementation of overlay precedence. It asks
//! `MountedGame` what files exist and `Vfs` for their bytes;
//! `which_side_of_the_vfs_each_job_is_on` holds that. `openrage-cli rpf extract`,
//! handed one archive file, is the opposite case and sits beside the VFS.
//!
//! ⚠ **It extracts the *winning* copy of each logical path**, not every copy: a
//! file a DLC pack overrides appears once, with the bytes the game would load.
//! An every-copy mode would need a traversal that does not go through the mount.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_title::vfs::Pattern;

pub fn print_usage() {
    eprintln!("Usage: openrage-cli bulk --game <path> --glob <pattern> [options]");
    eprintln!();
    eprintln!("Extracts every file in a mounted install matching a glob, in DLC");
    eprintln!("override order, writing the copy the game would load.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --game <path>   The install root.");
    eprintln!("  --glob <pat>    A VFS glob: `**/*.ytd`, `x64*/**/*.ydr`, `*.ypt`.");
    eprintln!("  --out <dir>     Where to write. Omit for a count without writing.");
    eprintln!("  --keys <path>   A keys cache to use instead of deriving one.");
    eprintln!("  --flat          Write basenames into one directory rather than a tree.");
    eprintln!("  --limit <n>     Stop after n files. For trying a pattern out.");
    eprintln!();
    eprintln!("With no --out this is a dry run: it reports what it would write.");
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

struct Options {
    root: PathBuf,
    glob: String,
    out: Option<PathBuf>,
    keys: Option<PathBuf>,
    flat: bool,
    limit: Option<usize>,
}

/// Whether `args` are ones `bulk` takes, read as it reads them and without opening an install.
pub(crate) fn accepts(args: &[String]) -> Result<(), String> {
    parse(args).map(drop)
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut root = None;
    let mut glob = None;
    let mut out = None;
    let mut keys = None;
    let mut flat = false;
    let mut limit = None;
    let mut i = 0;
    while i < args.len() {
        let need = |what: &str| -> Result<&String, String> {
            args.get(i + 1)
                .ok_or_else(|| format!("{what} needs a value"))
        };
        match args[i].as_str() {
            "--game" => {
                root = Some(PathBuf::from(need("--game")?));
                i += 2;
            }
            "--glob" => {
                glob = Some(need("--glob")?.clone());
                i += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(need("--out")?));
                i += 2;
            }
            "--keys" => {
                keys = Some(PathBuf::from(need("--keys")?));
                i += 2;
            }
            "--limit" => {
                limit = Some(
                    need("--limit")?
                        .parse::<usize>()
                        .map_err(|e| format!("--limit: {e}"))?,
                );
                i += 2;
            }
            "--flat" => {
                flat = true;
                i += 1;
            }
            "-h" | "--help" => {
                print_usage();
                return Err(String::new());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Options {
        root: root.ok_or_else(|| "missing --game".to_string())?,
        glob: glob.ok_or_else(|| "missing --glob".to_string())?,
        out,
        keys,
        flat,
        limit,
    })
}

fn run_inner(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        print_usage();
        return Err("no arguments".to_string());
    }
    let options = parse(args)?;
    if !options.root.is_dir() {
        return Err(format!("not a directory: {}", options.root.display()));
    }

    let game = super::keys::mount_all(&options.root, options.keys.as_deref())?;
    for finding in game.diagnostics.messages() {
        eprintln!("warning: {finding}");
    }
    eprintln!(
        "mounted GTA V {} edition",
        game.game.generation().edition_label()
    );

    // The mount's own enumeration, so DLC override order is the one `or-title`
    // already tests rather than one this file invents.
    let pattern = Pattern::new(&options.glob);
    let mut matched: Vec<String> = Vec::new();
    game.for_each_file(|logical, _container, _leaf| {
        if pattern.matches_path(logical) {
            matched.push(logical.to_string());
        }
    });
    matched.sort();
    matched.dedup();
    if let Some(limit) = options.limit {
        matched.truncate(limit);
    }

    println!("{} file(s) match `{}`", matched.len(), options.glob);
    if matched.is_empty() {
        println!(
            "  Nothing matched. `--glob` is a path pattern, so `*.ytd` matches \
             only at the mount root — `**/*.ytd` is almost always what is meant."
        );
        return Ok(ExitCode::SUCCESS);
    }

    let Some(out_dir) = options.out.as_ref() else {
        for logical in matched.iter().take(20) {
            println!("  {logical}");
        }
        if matched.len() > 20 {
            println!("  ... and {} more", matched.len() - 20);
        }
        println!("  (no --out given; nothing written)");
        return Ok(ExitCode::SUCCESS);
    };
    std::fs::create_dir_all(out_dir).map_err(|e| format!("create_dir_all: {e}"))?;

    let cancelled = super::jobs::on_interrupt();
    let progress = or_job::Progress::new();
    let mut ticker = super::jobs::Ticker::new(&progress);
    let mut unreadable = 0u64;

    // ⚠ A file that will not read is counted, never propagated, and an
    // interrupt stops between items with the count so far. Both live in
    // `or_job::run` rather than in this loop, so the editor's extraction
    // shares them.
    let report = or_job::run(matched.clone(), &cancelled, &progress, |logical| {
        ticker.tick();
        progress.now_on(logical.clone());

        let Some(data) = game.vfs.open(&logical) else {
            unreadable += 1;
            return or_job::Step::Skipped;
        };

        let dest = destination(out_dir, &logical, options.flat);
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&dest, &data) {
            Ok(()) => or_job::Step::Done(data.len() as u64),
            Err(e) => or_job::Step::Failed(format!("write({}): {e}", dest.display())),
        }
    });
    ticker.finish(&report);

    for failure in &report.failures {
        eprintln!("warn: {failure}");
    }
    println!("{} into {}", report.summary(), out_dir.display());
    if unreadable > 0 {
        println!("  {unreadable} would not read and were skipped");
    }
    if report.cancelled || report.counts.failed > 0 {
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

/// Where one logical path lands on disk.
///
/// ⚠ Flat mode collides by design — two DLC packs both holding `vehicles.meta`
/// become one file — so it exists for "give me every `.ytd` to look at" and not
/// for anything that has to be put back.
fn destination(out_dir: &Path, logical: &str, flat: bool) -> PathBuf {
    if flat {
        let leaf = logical.rsplit('/').next().unwrap_or(logical);
        return out_dir.join(sanitise(leaf));
    }
    let mut dest = out_dir.to_path_buf();
    for segment in logical.split('/').filter(|s| !s.is_empty()) {
        dest.push(sanitise(segment));
    }
    dest
}

/// A logical segment made safe as a filename.
///
/// A mount name carries a colon (`common:`), and a colon in a Windows path is a
/// drive separator or an alternate data stream — neither of which is a
/// directory. Nothing else in a RAGE logical path needs touching.
fn sanitise(segment: &str) -> String {
    segment.replace([':', '<', '>', '"', '|', '?', '*'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mount_prefix_does_not_become_a_drive_letter() {
        let dest = destination(Path::new("out"), "common:/data/effects/fx.ypt", false);
        let as_text = dest.to_string_lossy().replace('\\', "/");
        assert_eq!(
            as_text, "out/common_/data/effects/fx.ypt",
            "a colon has to go: `out/common:/...` is a path Windows reads as an \
             alternate data stream on a file called `out/common`, which fails in \
             a way that names neither the mount nor the file"
        );
    }

    #[test]
    fn flat_mode_keeps_only_the_leaf() {
        let dest = destination(Path::new("out"), "common:/data/effects/fx.ypt", true);
        assert_eq!(
            dest.to_string_lossy().replace('\\', "/"),
            "out/fx.ypt",
            "flat mode is for looking at files, so the leaf is the whole name"
        );
    }

    #[test]
    fn a_path_with_no_directories_still_lands_somewhere() {
        let dest = destination(Path::new("out"), "loose.ytd", false);
        assert_eq!(dest.to_string_lossy().replace('\\', "/"), "out/loose.ytd");
    }
}
