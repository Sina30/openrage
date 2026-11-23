//! `openrage-cli weapons`: validate the weapons of a project and say what the rules find, and read a build back against them.
//!
//! The plan and the build of weapons are `openrage-cli project plan` and `build`:
//! a weapons content is content like any other, and goes through the one plan.

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

use or_project::{ModuleKind, Project};
use or_weapons::{planned, Depth, Severity};
use serde_json::json;

use crate::project::{parse, planned as planned_plans, root_of, Failure};
use Failure::{Run, Usage};

pub fn run(args: &[String]) -> ExitCode {
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("validate") => validate(rest),
        Some("verify") => verify(rest),
        Some("-h" | "--help" | "help") => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(Usage(format!("unknown weapons command: {other}"))),
        None => {
            print_usage();
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(code) => code,
        Err(Usage(message)) => crate::usage_error(&message),
        Err(Run(message)) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!(
        "Usage:\n  \
         openrage-cli weapons validate <project.openrage-proj> [--module <id>] [--json]\n  \
         openrage-cli weapons verify   <project.openrage-proj> [--build <id>] [--out <folder>]\n\n\
         validate reads every source of the weapons content, opens its metadata through the typed\n\
         family, reads the weapons the metadata defines, and lists what the rules find, each with a\n\
         code and a severity. It exits 1 when a finding that is not waived is an error.\n\
         verify reads the resource a build wrote against the weapons it was made of: that each\n\
         source reached the place the plan gave it, that every data file reloads and defines the\n\
         weapons, and that the manifest registers it. It exits 1 when a check fails.\n\
         The plan and the build are `openrage-cli project plan` and `build`; weapons go through\n\
         them as any content does.\n\n\
         A build checks the files. It does not test that GTA V loads them.\n"
    );
}

/// The project at `path` and the ids of its weapons modules, or the one `--module` names.
fn weapons_of(
    positional: &[String],
    flags: &HashMap<String, String>,
) -> Result<(Project, Vec<String>), Failure> {
    let [path] = positional else {
        return Err(Usage("expected one <project.openrage-proj>".into()));
    };
    let project = Project::load(Path::new(path)).map_err(|e| Run(format!("{path}: {e}")))?;
    for finding in &project.findings {
        eprintln!("warning: {finding}");
    }
    let all: Vec<String> = (project.container.modules.iter())
        .filter(|m| m.kind == ModuleKind::Weapons)
        .map(|m| m.id.clone())
        .collect();
    let chosen = match flags.get("--module") {
        Some(id) if all.contains(id) => vec![id.clone()],
        Some(id) => return Err(Run(format!("{path} has no weapons content `{id}`"))),
        None => all,
    };
    if chosen.is_empty() {
        return Err(Run(format!("{path} holds no weapons content")));
    }
    Ok((project, chosen))
}

/// Read each resource the build of an output wrote against the weapons content it holds.
fn verify(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(args, &["--build", "--out"])?;
    let (project, plans) = planned_plans(&positional, &flags)?;
    let root = root_of(&positional[0], &flags);
    let mut clean = true;
    let mut seen = false;
    for plan in &plans {
        let Some(output) = project.container.output(&plan.output) else {
            continue;
        };
        let folder = root.join(&plan.resource);
        for module in (project.container.modules.iter())
            .filter(|m| m.kind == ModuleKind::Weapons && output.take.contains(&m.id))
        {
            // ⚠ Open, not Stat: the type a data file registers under comes from its opened metadata, and without it the manifest is never checked.
            let Ok((model, facts, planned)) = planned(&project.folder(), module, Depth::Open)
            else {
                continue;
            };
            // ⚠ A split divides between contents and never cuts one, so a module is read in the resource that holds its files.
            let held = (planned.contribution.files.iter())
                .any(|f| plan.files.iter().any(|p| p.path == f.path));
            if !held && plans.len() > 1 {
                continue;
            }
            seen = true;
            let found = or_weapons::verify::verify(&model, &facts, &folder);
            println!("content {} in {}:", module.id, folder.display());
            for check in &found.checks {
                let mark = if check.passed { "ok  " } else { "FAIL" };
                println!("  {mark} {}: {}", check.name, check.detail);
            }
            clean &= found.is_clean();
        }
    }
    if !seen {
        return Err(Run(format!(
            "{} holds no weapons content that a build takes",
            positional[0]
        )));
    }
    if clean {
        println!("This checks the files. It does not test that GTA V loads them.");
    }
    Ok(if clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn validate(args: &[String]) -> Result<ExitCode, Failure> {
    let json_out = args.iter().any(|a| a == "--json");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--json").cloned().collect();
    let (positional, flags) = parse(&rest, &["--module"])?;
    let (project, modules) = weapons_of(&positional, &flags)?;
    let mut blocked = false;
    let mut reports = Vec::new();
    for id in modules {
        let module = project
            .container
            .module(&id)
            .expect("chosen from the container");
        let (model, _, planned) = planned(&project.folder(), module, Depth::Open)
            .map_err(|e| Run(format!("content `{id}`: {e}")))?;
        blocked |= planned.findings.iter().any(or_weapons::Finding::blocks);
        if json_out {
            reports.push(json!({ "module": id, "findings": planned.findings }));
            continue;
        }
        let count = |severity| {
            planned
                .findings
                .iter()
                .filter(|f| f.severity == severity && f.waived.is_none())
                .count()
        };
        println!(
            "content {id}: {} weapon(s), {} error(s), {} warning(s), {} note(s), {} waived",
            model.weapons.len(),
            count(Severity::Error),
            count(Severity::Warning),
            count(Severity::Info),
            planned
                .findings
                .iter()
                .filter(|f| f.waived.is_some())
                .count()
        );
        for f in &planned.findings {
            let word = match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "note",
            };
            match &f.waived {
                Some(reason) => {
                    println!("  waived  {:<20} {} (waived: {reason})", f.code, f.message)
                }
                None => println!("  {word:<7} {:<20} {}", f.code, f.message),
            }
        }
    }
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&reports).map_err(|e| Run(e.to_string()))?
        );
    }
    Ok(if blocked {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
