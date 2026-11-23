//! Convert every `.ydr`, `.ytd`, `.ydd` and `.yft` under a folder to the other
//! target through `or_io`'s retarget, with one loss report for the batch.
//!
//! ⚠ A file's target is its RSC7 version, never the folder it sits in; a
//! version neither target declares is refused rather than read as Legacy.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_io::gta5::retarget::LossReport;
use or_io::gta5::shaders::shader_defs::{self, ParamRemap};
use or_job::{Cancel, Progress, Step};
use serde_json::{json, Value};

/// The file-level retarget lives in `or_io`, where a project build reaches it too.
pub use or_io::gta5::retarget::{retarget_file, Retargeted, CONVERTED};

/// The batch's report, written into the output folder.
const REPORT: &str = "retarget_report.json";

pub fn print_usage() {
    eprintln!("Usage: openrage-cli convert retarget <dir> --out <dir> [--shader-defs <json>]");
    eprintln!();
    eprintln!("Converts every .ydr, .ytd, .ydd and .yft under <dir> to the other target,");
    eprintln!("Legacy to Enhanced and Enhanced to Legacy, by each file's RSC7 version.");
    eprintln!("Each result is written under --out at the same relative path, and one");
    eprintln!("report, {REPORT}, names per file every value dropped, or why the file");
    eprintln!("was refused. An archive under <dir> is refused: extract it first.");
    eprintln!("Ctrl-C stops between files and still writes the report so far.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --out <dir>           Where to write; refused if it is <dir> itself.");
    eprintln!("  --shader-defs <json>  The shader database a material's parameters are");
    eprintln!("                        respelled through (or-shader-tool defs); default");
    eprintln!("                        OPENRAGE_SHADER_DEFS. With none, every parameter");
    eprintln!("                        of a drawable is dropped and named in the report.");
}

pub fn run(args: &[String]) -> ExitCode {
    let options = match parse(args) {
        Ok(Some(options)) => options,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            print_usage();
            return ExitCode::from(2);
        }
    };
    match convert_folder(&options, &crate::jobs::on_interrupt()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    input: PathBuf,
    out: PathBuf,
    shader_defs: Option<PathBuf>,
}

/// Whether `args` are ones `convert retarget` takes, read as it reads them and without converting.
pub(crate) fn accepts(args: &[String]) -> Result<(), String> {
    parse(args).map(drop)
}

fn parse(args: &[String]) -> Result<Option<Options>, String> {
    let (mut input, mut out, mut shader_defs) = (None, None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| {
            (args.next().map(PathBuf::from)).ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--out" => out = Some(value("--out")?),
            "--shader-defs" => shader_defs = Some(value("--shader-defs")?),
            "-h" | "--help" => {
                print_usage();
                return Ok(None);
            }
            a if a.starts_with("--") => return Err(format!("unknown option {a}")),
            a if input.is_none() => input = Some(PathBuf::from(a)),
            a => return Err(format!("unexpected argument {a}")),
        }
    }
    Ok(Some(Options {
        input: input.ok_or("missing the folder to convert")?,
        out: out.ok_or("missing --out")?,
        shader_defs,
    }))
}

/// The database a retarget respells a drawable's parameters through: the file `path` names, else the staged one.
///
/// ⚠ With neither, every parameter of a drawable is dropped and named, and this says so on stderr.
pub(crate) fn remap_of(path: Option<&Path>) -> Result<ParamRemap, String> {
    Ok(match path {
        Some(path) => ParamRemap::of(
            &shader_defs::load_from(path).map_err(|e| format!("{}: {e}", path.display()))?,
        ),
        None => shader_defs::staged().map_or_else(
            || {
                eprintln!(
                    "warning: no shader database (--shader-defs or {}); every drawable \
                     parameter is dropped",
                    shader_defs::SHADER_DEFS_ENV
                );
                ParamRemap::default()
            },
            |defs| ParamRemap::of(&defs),
        ),
    })
}

/// Convert the folder and write its report; `Ok(false)` when a file was refused or the run was stopped.
fn convert_folder(options: &Options, cancel: &Cancel) -> Result<bool, String> {
    if !options.input.is_dir() {
        return Err(format!("not a directory: {}", options.input.display()));
    }
    fs::create_dir_all(&options.out)
        .map_err(|e| format!("create {}: {e}", options.out.display()))?;
    if fs::canonicalize(&options.input).ok() == fs::canonicalize(&options.out).ok() {
        return Err("--out is the folder being converted, and would overwrite it".to_string());
    }
    let remap = remap_of(options.shader_defs.as_deref())?;

    let mut files = Vec::new();
    collect(&options.input, &mut files)?;
    files.sort();

    let progress = Progress::new();
    let mut ticker = crate::jobs::Ticker::new(&progress);
    let mut entries = Vec::new();
    let job = or_job::run(files, cancel, &progress, |path| {
        ticker.tick();
        let rel = path
            .strip_prefix(&options.input)
            .unwrap_or(&path)
            .to_path_buf();
        progress.now_on(rel.display().to_string());
        let (entry, step) = match one(&path, &options.out.join(&rel), &remap) {
            Ok((from, to, written, report)) => {
                println!("  {} -> {}  {}", from, to, rel.display());
                let dropped: Vec<Value> = (report.0.iter())
                    .map(|d| json!({ "at": d.at.to_string(), "field": d.lost.to_string() }))
                    .collect();
                let entry = json!({
                    "file": rel, "from": from, "to": to,
                    "status": "converted", "dropped": dropped,
                });
                (entry, Step::Done(written))
            }
            Err(why) => {
                println!("  REFUSED  {}: {why}", rel.display());
                let entry = json!({ "file": rel, "status": "refused", "why": why });
                (entry, Step::Failed(format!("{}: {why}", rel.display())))
            }
        };
        entries.push(entry);
        step
    });
    ticker.finish(&job);

    let report = json!({
        "input": options.input,
        "out": options.out,
        "cancelled": job.cancelled,
        "files": entries,
    });
    let at = options.out.join(REPORT);
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    fs::write(&at, text).map_err(|e| format!("write {}: {e}", at.display()))?;
    println!("{}; report in {}", job.summary(), at.display());
    Ok(!job.cancelled && job.counts.failed == 0)
}

/// Every file under `dir` this converts, and every archive, which it refuses.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if extension(&path).is_some_and(|e| e == "rpf" || CONVERTED.contains(&e.as_str())) {
            out.push(path);
        }
    }
    Ok(())
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
}

/// Convert `path` into `dest`: the two targets' labels, the bytes written and what was dropped.
fn one(
    path: &Path,
    dest: &Path,
    remap: &ParamRemap,
) -> Result<(&'static str, &'static str, u64, LossReport), String> {
    if extension(path).is_some_and(|e| e == "rpf") {
        return Err("an archive; extract its files first".to_string());
    }
    let bytes = fs::read(path).map_err(|e| format!("read: {e}"))?;
    let out = retarget_file(&path.to_string_lossy(), &bytes, remap)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::write(dest, &out.bytes).map_err(|e| format!("write {}: {e}", dest.display()))?;
    Ok((
        out.from.edition_label(),
        out.to.edition_label(),
        out.bytes.len() as u64,
        out.report,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ An interrupt before the first file converts nothing and still writes the report, marked stopped.
    #[test]
    fn a_stopped_run_writes_nothing_but_its_report() {
        let scratch = tempfile::tempdir().unwrap();
        let (input, out) = (scratch.path().join("in"), scratch.path().join("out"));
        fs::create_dir_all(&input).unwrap();
        fs::write(input.join("a.ydr"), b"not a resource").unwrap();
        let cancel = Cancel::new();
        cancel.set();
        let options = Options {
            input,
            out: out.clone(),
            shader_defs: None,
        };

        assert_eq!(
            convert_folder(&options, &cancel),
            Ok(false),
            "a stopped run is not a success"
        );
        let report: Value = serde_json::from_slice(&fs::read(out.join(REPORT)).unwrap()).unwrap();
        assert_eq!(report["cancelled"], true);
        assert_eq!(report["files"], json!([]));
        assert!(!out.join("a.ydr").exists());
    }

    #[test]
    fn the_folder_itself_is_refused_as_the_output() {
        let scratch = tempfile::tempdir().unwrap();
        let options = Options {
            input: scratch.path().to_path_buf(),
            out: scratch.path().to_path_buf(),
            shader_defs: None,
        };
        assert!(convert_folder(&options, &Cancel::new()).is_err());
    }
}
