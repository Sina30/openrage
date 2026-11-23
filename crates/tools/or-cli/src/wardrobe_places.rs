//! `openrage-cli wardrobe freeze` and `wardrobe dummies`, and the freezing a build that went through does.

use std::path::Path;
use std::process::ExitCode;

use or_project::{Plan, Project};
use or_wardrobe::freeze::{self, Kept};
use or_wardrobe::{allocate, dummies, Ped, Slot, Wardrobe};

use crate::project::{parse, Failure};
use crate::wardrobe::clothing_of;
use Failure::{Run, Usage};

/// The folder the clothing content `id` of `project` is edited in; a linked content is read here and written by the project that holds it.
fn folder_of(project: &Project, id: &str) -> Result<std::path::PathBuf, Failure> {
    let module = project
        .container
        .module(id)
        .expect("a content of the project");
    or_wardrobe::module_folder_to_edit(&project.path, &project.container, module)
        .map_err(|e| Run(e.to_string()))
}

/// Freeze the drawables of each clothing content the builds of `plans` shipped, and say so: a build that went through is what a later one is held to.
pub(crate) fn frozen_after(project: &Project, plans: &[Plan]) {
    let mut seen: Vec<&str> = Vec::new();
    for plan in plans {
        if seen.contains(&plan.output.as_str()) {
            continue;
        }
        seen.push(&plan.output);
        for (module, kept) in freeze::keep_for(&project.container, &project.folder(), &plan.output)
        {
            match kept {
                Ok(Kept::Wrote { places, .. }) => println!(
                    "froze the {places} drawables of {module} in {}: a later build stops before it moves one, unless a reason says why",
                    freeze::FILE
                ),
                Ok(Kept::Same | Kept::Off) => {}
                Err(why) => {
                    eprintln!("warning: the drawables of {module} could not be frozen: {why}")
                }
            }
        }
    }
}

/// `wardrobe freeze`: how many drawables a clothing content has frozen and how many would move, or `--thaw` to let the pack renumber.
pub(crate) fn freeze_of(args: &[String]) -> Result<ExitCode, Failure> {
    let thaw = args.iter().any(|a| a == "--thaw");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--thaw").cloned().collect();
    let (positional, flags) = parse(&rest, &["--module"])?;
    let (project, modules) = clothing_of(&positional, &flags)?;
    for id in modules {
        let folder = folder_of(&project, &id)?;
        if thaw {
            let gone = freeze::thaw(&folder).map_err(|e| Run(e.to_string()))?;
            println!(
                "{id}: {}",
                if gone {
                    "thawed: the pack renumbers freely until its next build"
                } else {
                    "was not frozen"
                }
            );
            continue;
        }
        let wardrobe = Wardrobe::read(&folder).map_err(|e| Run(e.to_string()))?;
        match &wardrobe.lock {
            None => println!(
                "{id}: nothing is frozen; a build that goes through freezes the drawables it ships"
            ),
            Some(lock) => {
                let moved = lock.renumbered(&wardrobe, &allocate(&wardrobe)).len();
                println!(
                    "{id}: {} drawables frozen, {moved} would move",
                    lock.places.len()
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// A ped as a person types it: `male`, `female`, or the model's name.
fn ped_of(word: &str) -> Result<Ped, Failure> {
    match word {
        "male" | "m" => Ok(Ped::Male),
        "female" | "f" => Ok(Ped::Female),
        other => other.parse().map_err(Usage),
    }
}

/// `wardrobe dummies <fill|sort>`: fill the holes of a slot with dummies, or put its dummies after its garments.
pub(crate) fn dummies_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(args, &["--module", "--ped", "--slot"])?;
    let (action, rest) = positional
        .split_first()
        .ok_or_else(|| Usage("expected fill or sort, then <project.openrage-proj>".into()))?;
    if !matches!(action.as_str(), "fill" | "sort") {
        return Err(Usage(format!("{action} is not fill or sort")));
    }
    let (project, modules) = clothing_of(rest, &flags)?;
    let ped = ped_of(
        flags
            .get("--ped")
            .ok_or_else(|| Usage("--ped male|female is needed".into()))?,
    )?;
    let slot: Slot = flags
        .get("--slot")
        .ok_or_else(|| Usage("--slot <name> is needed, such as jbib".into()))?
        .parse()
        .map_err(Usage)?;
    for id in modules {
        let folder = folder_of(&project, &id)?;
        let said = change(&folder, action, ped, slot)?;
        println!("{id}: {said}");
    }
    Ok(ExitCode::SUCCESS)
}

fn change(folder: &Path, action: &str, ped: Ped, slot: Slot) -> Result<String, Failure> {
    or_wardrobe::edit::update(folder, |w| {
        Ok(match action {
            "fill" => {
                let made = dummies::fill(w, ped, slot);
                format!("filled {made} hole(s) of {slot} of {ped} with dummies")
            }
            _ => {
                let moved = dummies::resort(w, ped, slot);
                format!("put the dummies of {slot} of {ped} last; {moved} garment(s) changed place")
            }
        })
    })
    .map_err(|e| Run(e.to_string()))
}
