//! `openrage-cli wardrobe` verbs of the power features: a query over a pack, a bulk edit on a query with a dry run, the review of two versions, a
//! garment's credit and the size map, and a whole pack as one package.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_project::Project;
use or_wardrobe::edit::Edit;
use or_wardrobe::rules::name_of;
use or_wardrobe::{bulk, review, Depth, Wardrobe};
use serde_json::{json, Map, Value};

use crate::project::{parse, Failure};
use crate::wardrobe::clothing_of;
use Failure::{Run, Usage};

/// The switches these verbs take, which carry no value.
const SWITCHES: [&str; 6] = [
    "--json",
    "--apply",
    "--no-group",
    "--remove",
    "--list",
    "--sizes",
];

/// `args` without the switches among them, and which of them were there.
fn without_switches(args: &[String]) -> (Vec<String>, Vec<&'static str>) {
    let on: Vec<&'static str> = SWITCHES
        .into_iter()
        .filter(|s| args.iter().any(|a| a == s))
        .collect();
    let rest = args
        .iter()
        .filter(|a| !SWITCHES.contains(&a.as_str()))
        .cloned()
        .collect();
    (rest, on)
}

fn folder_of(project: &Project, id: &str) -> Result<PathBuf, Failure> {
    let module = project
        .container
        .module(id)
        .ok_or_else(|| Run(format!("no clothing content `{id}`")))?;
    or_wardrobe::module_folder(&project.folder(), module).map_err(|e| Run(e.to_string()))
}

/// `wardrobe query <project> "<query>" [--module <id>] [--json]`: the garments a query selects, as the Wardrobe's list shows them.
pub fn query_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (rest, on) = without_switches(args);
    let (positional, flags) = parse(&rest, &["--module"])?;
    let [path, text] = positional.as_slice() else {
        return Err(Usage(
            "query takes a <project.openrage-proj> and a query in quotes".into(),
        ));
    };
    let (project, modules) = clothing_of(std::slice::from_ref(path), &flags)?;
    let mut reports = Vec::new();
    for id in modules {
        let folder = folder_of(&project, &id)?;
        let ids = bulk::ids_of(&folder, text).map_err(|e| Run(e.to_string()))?;
        let wardrobe = Wardrobe::read(&folder).map_err(|e| Run(e.to_string()))?;
        if on.contains(&"--json") {
            reports.push(json!({ "module": id, "query": text, "garments": ids }));
            continue;
        }
        println!(
            "{id}: {} of {} garments",
            ids.len(),
            wardrobe.garments.len()
        );
        for garment in ids.iter().filter_map(|i| wardrobe.garment(i)) {
            println!("  {}  {}", garment.id, name_of(garment));
        }
    }
    if on.contains(&"--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&reports).map_err(|e| Run(e.to_string()))?
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// The edit the flags of `bulk` state, or why they state none or two.
fn edit_of(
    flags: &std::collections::HashMap<String, String>,
    on: &[&str],
) -> Result<Edit, Failure> {
    let mut edits: Vec<Edit> = Vec::new();
    if let Some(tag) = flags.get("--add-tag") {
        edits.push(Edit::AddTag(tag.clone()));
    }
    if let Some(tag) = flags.get("--remove-tag") {
        edits.push(Edit::RemoveTag(tag.clone()));
    }
    if let Some(group) = flags.get("--group") {
        edits.push(Edit::Group(Some(group.clone())));
    }
    if on.contains(&"--no-group") {
        edits.push(Edit::Group(None));
    }
    if on.contains(&"--remove") {
        edits.push(Edit::Remove);
    }
    match edits.len() {
        1 => Ok(edits.remove(0)),
        0 => Err(Usage(
            "bulk needs one change: --add-tag, --remove-tag, --group, --no-group or --remove"
                .into(),
        )),
        _ => Err(Usage("bulk makes one change at a time".into())),
    }
}

/// `wardrobe bulk <project> "<query>" <change> [--apply] [--module <id>] [--json]`: one change to the garments a query selects, said first as a dry run.
pub fn bulk_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (rest, on) = without_switches(args);
    let (positional, flags) = parse(&rest, &["--module", "--add-tag", "--remove-tag", "--group"])?;
    let [path, text] = positional.as_slice() else {
        return Err(Usage(
            "bulk takes a <project.openrage-proj> and a query in quotes".into(),
        ));
    };
    let edit = edit_of(&flags, &on)?;
    let (project, modules) = clothing_of(std::slice::from_ref(path), &flags)?;
    let [id] = modules.as_slice() else {
        return Err(Usage(
            "the project holds several clothing contents: name one with --module".into(),
        ));
    };
    let folder = folder_of(&project, id)?;
    let ids = bulk::ids_of(&folder, text).map_err(|e| Run(e.to_string()))?;
    let wardrobe = Wardrobe::read(&folder).map_err(|e| Run(e.to_string()))?;
    let run = bulk::dry_run(&wardrobe, &ids, &edit);
    if on.contains(&"--json") {
        let changed: Vec<Value> = (run.changed.iter())
            .map(|c| json!({ "id": c.id, "name": c.name, "lines": c.lines }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "module": id,
                "query": text,
                "selected": run.selected,
                "changed": changed,
                "unchanged": run.unchanged,
                "refused": run.refused,
                "applied": on.contains(&"--apply") && run.is_worth_making(),
            }))
            .map_err(|e| Run(e.to_string()))?
        );
    } else {
        println!("{}", run.sentence());
        for changed in &run.changed {
            println!("  {}", changed.name);
            for line in &changed.lines {
                println!("    {line}");
            }
        }
        for (name, why) in &run.refused {
            println!("  {name} {why}");
        }
    }
    if !run.refused.is_empty() {
        return Ok(ExitCode::FAILURE);
    }
    if on.contains(&"--apply") && run.is_worth_making() {
        let mut params = Map::new();
        params.insert("project".into(), json!(path));
        params.insert("module".into(), json!(id));
        let (_, folder, _lock) = crate::wardrobe_exchange::pack_of(&params).map_err(Run)?;
        let (made, _) = bulk::make(&folder, &ids, edit).map_err(|e| Run(e.to_string()))?;
        println!("made it to {} garment(s)", made.changed.len());
    } else if !on.contains(&"--apply") {
        println!("a dry run: --apply makes it");
    }
    Ok(ExitCode::SUCCESS)
}

/// `wardrobe review <project> [--from <version>] [--to <version>] [--module <id>] [--list]`: what differs between two versions of a pack, read-only.
pub fn review_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (rest, on) = without_switches(args);
    let (positional, flags) = parse(&rest, &["--module", "--from", "--to"])?;
    let (project, modules) = clothing_of(&positional, &flags)?;
    let [id] = modules.as_slice() else {
        return Err(Usage(
            "the project holds several clothing contents: name one with --module".into(),
        ));
    };
    let folder = folder_of(&project, id)?;
    if on.contains(&"--list") {
        for version in or_wardrobe::backup::versions(&folder) {
            println!("{}  {}", version.n, version.label);
        }
        return Ok(ExitCode::SUCCESS);
    }
    let number = |flag: &str| -> Result<Option<u64>, Failure> {
        flags
            .get(flag)
            .map(|v| {
                v.parse::<u64>()
                    .map_err(|_| Usage(format!("{flag} is a version number, as --list shows them")))
            })
            .transpose()
    };
    let (from, to) = (number("--from")?, number("--to")?);
    let Some(from) = from else {
        return Err(Usage("review needs --from <version> (or --list)".into()));
    };
    let found = review::of_versions(&folder, Some(from), to).map_err(|e| Run(e.to_string()))?;
    if on.contains(&"--json") {
        let garments: Vec<Value> = (found.garments.iter())
            .map(|g| {
                json!({
                    "id": g.id, "name": g.name, "kind": g.kind.word(), "lines": g.lines,
                    "textures": g.textures.iter().map(|t| json!({
                        "number": t.number, "kind": t.kind.word(), "file": t.file, "lines": t.lines,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({ "garments": garments, "settings": found.settings })
            )
            .map_err(|e| Run(e.to_string()))?
        );
    } else {
        println!("{}", found.summary());
        for line in found.lines() {
            println!("{line}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `wardrobe credit <project> --garment <id> [--author <text>] [--url <text>] [--licence <text>] [--module <id>]`: state who made the sources of a garment.
pub fn credit_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(
        args,
        &["--module", "--garment", "--author", "--url", "--licence"],
    )?;
    let Some(garment) = flags.get("--garment") else {
        return Err(Usage("credit needs --garment <id>".into()));
    };
    let [path] = positional.as_slice() else {
        return Err(Usage("expected one <project.openrage-proj>".into()));
    };
    let mut params = Map::new();
    params.insert("project".into(), json!(path));
    if let Some(module) = flags.get("--module") {
        params.insert("module".into(), json!(module));
    }
    let (_, folder, _lock) = crate::wardrobe_exchange::pack_of(&params).map_err(Run)?;
    let wardrobe = Wardrobe::read(&folder).map_err(|e| Run(e.to_string()))?;
    let said = |flag: &str| flags.get(flag).cloned().unwrap_or_default();
    let credit = or_source::asset::Credit {
        author: said("--author"),
        url: said("--url"),
        licence: said("--licence"),
    };
    let stated = or_wardrobe::credits::state_for(&folder, &wardrobe, garment, &credit)
        .map_err(|e| Run(e.to_string()))?;
    println!("credited {}: {}", stated.set.len(), stated.set.join(", "));
    for left in &stated.left {
        println!("left {left}: it is another project's, which states its own credit");
    }
    Ok(ExitCode::SUCCESS)
}

/// `wardrobe sizes <project> [--build <id>] [--module <id>] [--json]`: the files the pack's build writes ranked by garment and by slot, and what the garments name.
pub fn sizes_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (rest, on) = without_switches(args);
    let (positional, flags) = parse(&rest, &["--module"])?;
    let (project, modules) = clothing_of(&positional, &flags)?;
    let mut reports = Vec::new();
    for id in modules {
        let module = project
            .container
            .module(&id)
            .expect("chosen from the container");
        let (wardrobe, planned) =
            or_wardrobe::planned_with_model(&project.folder(), module, Depth::Measure)
                .map_err(|e| Run(format!("content `{id}`: {e}")))?;
        let sizes = or_wardrobe::sizes::sizes(&wardrobe, &planned, &[]);
        if on.contains(&"--json") {
            let row = |w: &or_project::sizes::Weight| json!({ "name": w.name, "files": w.files, "bytes": w.bytes });
            reports.push(json!({
                "module": id, "files": sizes.files, "bytes": sizes.bytes,
                "garments": sizes.garments.iter().map(row).collect::<Vec<_>>(),
                "slots": sizes.slots.iter().map(|s| json!({
                    "ped": s.ped.model(), "slot": s.slot.name(), "files": s.files, "bytes": s.bytes,
                })).collect::<Vec<_>>(),
                "rest": row(&sizes.rest),
                "needs": planned.dependencies.iter().map(|d| json!({
                    "path": d.path, "bytes": d.bytes, "used_by": d.used_by,
                })).collect::<Vec<_>>(),
            }));
            continue;
        }
        println!("{id}: {} file(s), {} byte(s)", sizes.files, sizes.bytes);
        for g in &sizes.garments {
            println!("  {:>12}  {:>4}  {}", g.bytes, g.files, g.name);
        }
        println!(
            "  {:>12}  {:>4}  {}",
            sizes.rest.bytes, sizes.rest.files, sizes.rest.name
        );
    }
    if on.contains(&"--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&reports).map_err(|e| Run(e.to_string()))?
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `wardrobe pack <project> --out <file> [--module <id>]`: the whole pack as one package file.
pub fn pack_of_a_project(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(args, &["--out", "--module"])?;
    let Some(out) = flags.get("--out") else {
        return Err(Usage("pack needs --out <file.openrage-pack>".into()));
    };
    let (project, modules) = clothing_of(&positional, &flags)?;
    let [id] = modules.as_slice() else {
        return Err(Usage(
            "the project holds several clothing contents: name one with --module".into(),
        ));
    };
    let packed = or_wardrobe::pack_package::export(&folder_of(&project, id)?, Path::new(out))
        .map_err(|e| Run(e.to_string()))?;
    println!(
        "wrote the package {out}: {} garment(s), {} file(s)",
        packed.garments, packed.files
    );
    Ok(ExitCode::SUCCESS)
}

/// `wardrobe unpack <file.openrage-pack> --out <folder>`: a package unpacked into a folder of its own, as a pack.
pub fn unpack_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(args, &["--out"])?;
    let (Some(out), [file]) = (flags.get("--out"), positional.as_slice()) else {
        return Err(Usage(
            "unpack takes a <file.openrage-pack> and --out <folder>".into(),
        ));
    };
    let imported = or_wardrobe::pack_package::import(Path::new(file), Path::new(out))
        .map_err(|e| Run(e.to_string()))?;
    println!(
        "unpacked the pack {} into {out}: {} garment(s), {} file(s)",
        imported.pack, imported.garments, imported.files
    );
    Ok(ExitCode::SUCCESS)
}
