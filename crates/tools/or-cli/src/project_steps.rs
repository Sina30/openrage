//! `openrage-cli project history`, `rollback`, `remove` and `recook`, and `source compile`: what the Project workspace's History, Remove and Recook do, and
//! the compile of one source on its own, with no window.
//!
//! ⚠ **The history is the one the editor keeps** (`.openrage/history`, ignored by Git). `remove` keeps a step as the editor does, so that `rollback`
//! brings the file back; the other verbs that edit a project file keep none, as they never did, and a project file they changed is the editor's
//! "changed outside OpenRage" step the next time it opens.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, UNIX_EPOCH};

use or_project::snapshot::{history_dir, Snapshots};
use or_project::{project_folder, resolve_member, shadow_root, store_path, Project};
use or_source::asset::Source;
use or_source::cook::{cook_source, Cooker, Recook};
use serde_json::{json, Value};

use crate::project::{parse, Failure};
use Failure::{Run, Usage};

/// The first step of a project that has none: the project as it was when its history began.
const OPENED: &str = "The project as it was opened";

/// `source compile`: one source cooked into `--out`, with nothing written beside the source.
pub fn compile(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(args, &["--out"])?;
    let [folder] = positional.as_slice() else {
        return Err(Usage("expected a <source folder>".into()));
    };
    let out = flags.get("--out").ok_or_else(|| {
        Usage("--out <folder> says where the compiled file is put: a project holds only sources, so it is not put beside one".into())
    })?;
    let dir = Path::new(folder);
    Source::open(dir).map_err(|p| Run(format!("{folder}: {p}")))?;
    let out = Path::new(out);
    // The cook writes into a folder of its own, which is moved from when it has made the file.
    let scratch = out.join(format!(".compile-{}", std::process::id()));
    let made = cook_source(dir, &scratch).map_err(|why| {
        let _ = std::fs::remove_dir_all(&scratch);
        Run(format!(
            "{folder} does not compile, and nothing was written:\n  {}",
            why.join("\n  ")
        ))
    });
    let result = made.and_then(|made| {
        let name = made
            .file_name()
            .ok_or_else(|| Run("the cook made no file".into()))?;
        let to = out.join(name);
        std::fs::rename(&made, &to)
            .or_else(|_| std::fs::copy(&made, &to).map(drop))
            .map_err(|e| Run(format!("cannot write {}: {e}", to.display())))?;
        Ok(to)
    });
    let _ = std::fs::remove_dir_all(&scratch);
    let to = result?;
    let bytes = std::fs::read(&to).map_err(|e| Run(format!("{}: {e}", to.display())))?;
    println!(
        "{folder} -> {} ({} bytes, sha256 {})",
        to.display(),
        bytes.len(),
        or_source::asset::sha256_hex(&bytes)
    );
    Ok(ExitCode::SUCCESS)
}

fn project_at(positional: &[String]) -> Result<(&String, PathBuf), Failure> {
    let (path, rest) = positional
        .split_first()
        .ok_or_else(|| Usage("expected a <project.openrage-proj>".into()))?;
    let _ = rest;
    let file = PathBuf::from(path);
    if !file.is_file() {
        return Err(Run(format!("{path} is not a file")));
    }
    Ok((path, file))
}

fn when(at_ms: u64) -> String {
    let at = UNIX_EPOCH + Duration::from_millis(at_ms);
    let ago = std::time::SystemTime::now()
        .duration_since(at)
        .map_or(0, |d| d.as_secs());
    match ago {
        0..=59 => format!("{ago} s ago"),
        60..=3599 => format!("{} min ago", ago / 60),
        3600..=86399 => format!("{} h ago", ago / 3600),
        _ => format!("{} days ago", ago / 86400),
    }
}

/// `project history`: the steps the project has, oldest first, and the one it stands at.
pub fn history_of(args: &[String]) -> Result<ExitCode, Failure> {
    let json_out = args.iter().any(|a| a == "--json");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--json").cloned().collect();
    let (positional, _) = parse(&rest, &[])?;
    let (path, file) = project_at(&positional)?;
    let store = Snapshots::open(&history_dir(&file));
    let here = store.current().map(|s| s.n);
    if json_out {
        let steps: Vec<Value> = store
            .steps()
            .iter()
            .map(|s| json!({ "step": s.n, "label": s.label, "at_ms": s.at_ms, "here": Some(s.n) == here }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "project": path, "steps": steps }))
                .expect("JSON serialises")
        );
        return Ok(ExitCode::SUCCESS);
    }
    if store.is_empty() {
        println!("{path} has no history yet: the editor keeps a step for each change it makes");
        return Ok(ExitCode::SUCCESS);
    }
    for step in store.steps() {
        println!(
            "{} {:>4}  {}  ({})",
            if Some(step.n) == here { ">" } else { " " },
            step.n,
            step.label,
            when(step.at_ms)
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// The sources an import step created, from what it kept beside the step: path and digest of each that is still there.
fn made_by(store: &Snapshots, n: u64, folder: &Path) -> Vec<(PathBuf, String)> {
    let Some(Value::Array(made)) = store
        .attachment(n)
        .and_then(|b| serde_json::from_slice(&b).ok())
    else {
        return Vec::new();
    };
    made.iter()
        .filter_map(|c| {
            Some((
                folder.join(c.get("path")?.as_str()?),
                c.get("digest")?.as_str()?.to_string(),
            ))
        })
        .collect()
}

/// `project rollback`: stand the project at a step, taking back the sources an import after it made while they are as the import left them.
pub fn rollback_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, _) = parse(args, &[])?;
    let (path, file) = project_at(&positional)?;
    let step: u64 = positional
        .get(1)
        .ok_or_else(|| {
            Usage("expected the number of a step, which `project history` lists".into())
        })?
        .parse()
        .map_err(|_| Usage("a step is a number".into()))?;
    let mut store = Snapshots::open(&history_dir(&file));
    let target = store
        .steps()
        .iter()
        .position(|s| s.n == step)
        .ok_or_else(|| Run(format!("{path} keeps no step {step}")))?;
    let here = store
        .position()
        .ok_or_else(|| Run(format!("{path} has no history")))?;
    if target == here {
        println!("the project already stands at step {step}");
        return Ok(ExitCode::SUCCESS);
    }
    let bytes = store
        .read(target)
        .map_err(|e| Run(format!("step {step} could not be read: {e}")))?;
    let folder = project_folder(&file);
    // Going back over an import takes back what it created, and refuses, with nothing moved, once a source was changed since.
    if target < here {
        let steps: Vec<u64> = store.steps()[target + 1..=here]
            .iter()
            .map(|s| s.n)
            .collect();
        let present: Vec<(PathBuf, String)> = steps
            .iter()
            .rev()
            .flat_map(|n| made_by(&store, *n, &folder))
            .filter(|(path, _)| path.exists())
            .collect();
        let changed: Vec<String> = present
            .iter()
            .filter(|(path, digest)| {
                or_source::game_import::tree_digest(path).ok().as_deref() != Some(digest)
            })
            .map(|(path, _)| path.display().to_string())
            .collect();
        if !changed.is_empty() {
            return Err(Run(format!(
                "not rolled back, and nothing was removed: {} changed since the import. Remove what you do not want by hand",
                changed.join(", ")
            )));
        }
        for (path, _) in &present {
            std::fs::remove_dir_all(path)
                .or_else(|_| std::fs::remove_file(path))
                .map_err(|e| Run(format!("cannot remove {}: {e}", path.display())))?;
        }
        if !present.is_empty() {
            println!(
                "took back {} source(s) the imports after it made",
                present.len()
            );
        }
    }
    or_project::project::replace_file(&file, &bytes).map_err(|e| Run(format!("{path}: {e}")))?;
    store
        .go(target)
        .map_err(|e| Run(format!("the place in the history was not written: {e}")))?;
    println!("the project stands at step {step}");
    Ok(ExitCode::SUCCESS)
}

/// `project remove`: take files out of the project's list of documents; they stay on disk, and the editor's Undo, or `rollback`, brings them back.
pub fn remove_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, _) = parse(args, &[])?;
    let (path, file) = project_at(&positional)?;
    let wanted = &positional[1..];
    if wanted.is_empty() {
        return Err(Usage("expected at least one <path> to take out".into()));
    }
    let loaded = Project::load(&file).map_err(|e| Run(format!("{path}: {e}")))?;
    let folder = project_folder(&file);
    let held: Vec<PathBuf> = loaded.members.iter().map(|m| m.path.clone()).collect();
    let resolved = |p: &str| {
        let given = PathBuf::from(p);
        let absolute = if given.is_absolute() {
            given
        } else {
            std::env::current_dir().unwrap_or_default().join(given)
        };
        std::fs::canonicalize(&absolute).unwrap_or(absolute)
    };
    let mut out = Vec::new();
    for named in wanted {
        let at = resolved(named);
        match held
            .iter()
            .find(|m| std::fs::canonicalize(m).unwrap_or_else(|_| (*m).clone()) == at)
        {
            Some(member) => out.push(member.clone()),
            None => {
                return Err(Run(format!(
                    "{named} is not a file of {path}: nothing was changed"
                )))
            }
        }
    }
    let before = std::fs::read(&file).map_err(|e| Run(format!("{path}: {e}")))?;
    let gone: Vec<String> = out.iter().map(|m| store_path(m, &folder)).collect();
    or_project::project::edit_file(&file, |top, _| {
        let documents = match top.remove("documents") {
            Some(Value::Array(documents)) => documents,
            _ => Vec::new(),
        };
        let kept: Vec<Value> = documents
            .into_iter()
            .filter(|d| {
                let stored = d.get("path").and_then(Value::as_str).unwrap_or_default();
                !out.iter().any(|m| {
                    resolve_member(stored, or_project::SCHEMA, &folder, Path::new(".")) == *m
                })
            })
            .collect();
        top.insert("documents".into(), Value::Array(kept));
        Ok(())
    })
    .map_err(|e| Run(format!("{path}: {e}")))?;
    let after = std::fs::read(&file).map_err(|e| Run(format!("{path}: {e}")))?;
    // The step the editor would keep, so that `rollback` and its Undo bring the files back.
    let mut store = Snapshots::open(&history_dir(&file));
    let label = match out.as_slice() {
        [one] => format!(
            "Removed {} from the project",
            one.file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        ),
        many => format!("Removed {} files from the project", many.len()),
    };
    let kept = (|| {
        if store.is_empty() {
            store.push(OPENED, &before)?;
        }
        store.push(&label, &after)
    })();
    if let Err(why) = kept {
        eprintln!(
            "warning: no step was kept, so the files cannot be brought back with rollback: {why}"
        );
    }
    println!("{label}; they stay on disk ({})", gone.join(", "));
    Ok(ExitCode::SUCCESS)
}

/// `project recook`: cook the sources named now into the project's shadow folder, as a saved file in one would.
pub fn recook_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, _) = parse(args, &[])?;
    let (path, file) = project_at(&positional)?;
    let wanted = &positional[1..];
    if wanted.is_empty() {
        return Err(Usage(
            "expected at least one <source folder> to cook".into(),
        ));
    }
    Project::load(&file).map_err(|e| Run(format!("{path}: {e}")))?;
    let mut cooker = Cooker::new(shadow_root(&file));
    let mut failed = false;
    for folder in wanted {
        let manifest = Path::new(folder).join(or_source::asset::MANIFEST);
        match cooker.recook(&manifest) {
            Recook::Cooked { output, .. } => println!("{folder} -> {}", output.display()),
            Recook::Unchanged { .. } => println!("{folder} is cooked already"),
            Recook::Failed { problems, .. } => {
                failed = true;
                eprintln!("{folder} did not cook, and the last output is as it was:");
                problems.iter().for_each(|p| eprintln!("  {p}"));
            }
            Recook::Ignored(why) => {
                failed = true;
                eprintln!("{folder} is not a source folder of a file the cook reads ({why:?})");
            }
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Keep the step the editor keeps for an import, so that `rollback` takes back what it created: the project file as it is, with the path and the digest of each source it made beside it.
pub(crate) fn keep_import_step(project: &Path, created: &[or_source::game_import::Created]) {
    if created.is_empty() {
        return;
    }
    let kept = (|| -> std::io::Result<()> {
        let bytes = std::fs::read(project)?;
        let mut store = Snapshots::open(&history_dir(project));
        if store.is_empty() {
            store.push(OPENED, &bytes)?;
        }
        let n = store.push(&format!("Imported {} source(s)", created.len()), &bytes)?;
        let folder = project_folder(project);
        let made: Vec<Value> = created
            .iter()
            .map(|c| json!({ "path": store_path(&c.path, &folder), "digest": c.digest }))
            .collect();
        store.attach(
            n,
            &serde_json::to_vec(&made).map_err(std::io::Error::other)?,
        )
    })();
    if let Err(why) = kept {
        eprintln!(
            "warning: no step was kept for the import, so rollback cannot take it back: {why}"
        );
    }
}
