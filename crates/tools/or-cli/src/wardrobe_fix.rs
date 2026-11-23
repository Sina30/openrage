//! `openrage-cli wardrobe fix` and `wardrobe textures`: Fix all safe and the texture optimiser of a clothing pack, with no window.
//!
//! Both write: the pack and the copies of the sources they make lighter. Each takes the right to write the pack first, as a window does,
//! so a pack another OpenRage holds is refused and not raced.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use or_project::{ModuleKind, Project};
use or_source::optimise::Scale;
use or_wardrobe::edit::Watch;
use or_wardrobe::lock::PackLock;
use serde_json::{json, Value};

use crate::project::{parse, Failure};
use Failure::{Run, Usage};

/// The clothing contents asked for, each with the folder it is written in and the right to write it.
pub(crate) fn packs(
    positional: &[String],
    flags: &HashMap<String, String>,
) -> Result<Vec<(String, PathBuf, PackLock)>, Failure> {
    let [path] = positional else {
        return Err(Usage("expected one <project.openrage-proj>".into()));
    };
    let project =
        Project::load(std::path::Path::new(path)).map_err(|e| Run(format!("{path}: {e}")))?;
    let all: Vec<_> = (project.container.modules.iter())
        .filter(|m| m.kind == ModuleKind::Clothing)
        .collect();
    let chosen: Vec<_> = match flags.get("--module") {
        Some(id) => vec![*all
            .iter()
            .find(|m| &m.id == id)
            .ok_or_else(|| Run(format!("{path} has no clothing content `{id}`")))?],
        None => all,
    };
    if chosen.is_empty() {
        return Err(Run(format!("{path} holds no clothing content")));
    }
    chosen
        .into_iter()
        .map(|module| {
            let folder =
                or_wardrobe::module_folder_to_edit(&project.path, &project.container, module)
                    .map_err(|e| Run(format!("content `{}`: {e}", module.id)))?;
            let lock = or_wardrobe::lock::acquire(&folder).map_err(|e| Run(e.to_string()))?;
            Ok((module.id.clone(), folder, lock))
        })
        .collect()
}

/// The arguments that are none of the bare flags `bare`, and whether each of those was given.
pub(crate) fn without(args: &[String], bare: &[&str]) -> (Vec<String>, Vec<bool>) {
    let found = bare.iter().map(|b| args.iter().any(|a| a == b)).collect();
    let rest = args.iter().filter(|a| !bare.contains(&a.as_str())).cloned();
    (rest.collect(), found)
}

/// `wardrobe fix`.
pub fn fix(args: &[String]) -> Result<ExitCode, Failure> {
    let (rest, bare) = without(args, &["--json"]);
    let json_out = bare[0];
    let (positional, flags) = parse(&rest, &["--module"])?;
    let mut documents = Vec::new();
    for (id, folder, _lock) in packs(&positional, &flags)? {
        let (report, revision) = or_wardrobe::fix::all_safe(&folder, &Watch::default())
            .map_err(|e| Run(format!("content `{id}`: {e}")))?;
        if json_out {
            documents
                .push(json!({ "module": id, "changed": revision.is_some(), "report": report }));
            continue;
        }
        println!("content {id}:");
        report.lines().iter().for_each(|l| println!("  {l}"));
        println!("  {}", report.sentence());
    }
    if json_out {
        print_json(documents);
    }
    Ok(ExitCode::SUCCESS)
}

/// `wardrobe textures`.
pub fn textures(args: &[String]) -> Result<ExitCode, Failure> {
    let (rest, bare) = without(
        args,
        &["--json", "--no-format", "--no-mips", "--no-power-of-two"],
    );
    let (json_out, no_format, no_mips, no_pow2) = (bare[0], bare[1], bare[2], bare[3]);
    let (positional, flags) = parse(&rest, &["--module", "--garment", "--ratio", "--side"])?;
    let scale = match (flags.get("--ratio"), flags.get("--side")) {
        (Some(_), Some(_)) => return Err(Usage("say a ratio or a side, not both".into())),
        (Some(r), None) => Scale::Ratio(
            r.parse()
                .map_err(|_| Usage(format!("--ratio needs a number, not {r:?}")))?,
        ),
        (None, Some(n)) => Scale::Side(
            n.parse()
                .map_err(|_| Usage(format!("--side needs a number of pixels, not {n:?}")))?,
        ),
        (None, None) => Scale::Keep,
    };
    let garments: Option<Vec<String>> = flags.get("--garment").map(|g| vec![g.clone()]);
    let mut documents = Vec::new();
    for (id, folder, _lock) in packs(&positional, &flags)? {
        let wardrobe = or_wardrobe::Wardrobe::read(&folder)
            .map_err(|e| Run(format!("content `{id}`: {e}")))?;
        let mut options = or_wardrobe::textures::safe_options(&wardrobe);
        options.format &= !no_format;
        options.mips &= !no_mips;
        options.power_of_two &= !no_pow2;
        options.scale = scale;
        let (report, revision) = or_wardrobe::textures::optimise(
            &folder,
            garments.as_deref(),
            &options,
            &Watch::default(),
        )
        .map_err(|e| Run(format!("content `{id}`: {e}")))?;
        if json_out {
            documents
                .push(json!({ "module": id, "changed": revision.is_some(), "report": report }));
            continue;
        }
        println!("content {id}: {} texture(s) changed", report.textures());
        report.lines().iter().for_each(|l| println!("  {l}"));
    }
    if json_out {
        print_json(documents);
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn print_json(documents: Vec<Value>) {
    println!(
        "{}",
        serde_json::to_string_pretty(&Value::Array(documents)).expect("JSON serialises")
    );
}
