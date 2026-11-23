//! `openrage-cli wardrobe maps` and `wardrobe budget`: normal and specular maps made from the diffuse textures of a clothing pack, and a texture budget that
//! fits a memory target, with no window.
//!
//! Both are dry runs until they are told to write: `maps --dry-run` and `budget` without `--apply` copy and write nothing. What they write is a copy of the
//! sources they change, as the editor does, so a pack another OpenRage holds is refused and not raced.

use std::process::ExitCode;

use or_source::budget::Options as BudgetOptions;
use or_source::maps::Options as MapOptions;
use or_wardrobe::edit::Watch;
use or_wardrobe::settings::Target;
use serde_json::json;

use crate::project::{parse, Failure};
use crate::wardrobe_fix::{packs, without};
use Failure::{Run, Usage};

/// `wardrobe maps`.
pub fn maps(args: &[String]) -> Result<ExitCode, Failure> {
    let (rest, bare) = without(
        args,
        &["--json", "--dry-run", "--no-normal", "--no-specular"],
    );
    let (json_out, dry, no_normal, no_specular) = (bare[0], bare[1], bare[2], bare[3]);
    let (positional, flags) = parse(&rest, &["--module", "--garment", "--texture", "--strength"])?;
    let mut options = MapOptions {
        normal: !no_normal,
        specular: !no_specular,
        ..MapOptions::default()
    };
    if let Some(strength) = flags.get("--strength") {
        options.strength = strength
            .parse()
            .map_err(|_| Usage(format!("--strength needs a number, not {strength:?}")))?;
    }
    if let Some(named) = flags.get("--texture") {
        options.only = named.split(',').map(|n| n.trim().to_string()).collect();
    }
    let garments: Option<Vec<String>> = flags.get("--garment").map(|g| vec![g.clone()]);
    let mut documents = Vec::new();
    for (id, folder, _lock) in packs(&positional, &flags)? {
        let fail = |e: or_wardrobe::edit::Error| Run(format!("content `{id}`: {e}"));
        let (report, changed) = match dry {
            true => (
                or_wardrobe::maps::plan(&folder, garments.as_deref(), &options).map_err(fail)?,
                false,
            ),
            false => {
                let (report, revision) = or_wardrobe::maps::generate(
                    &folder,
                    garments.as_deref(),
                    &options,
                    &Watch::default(),
                )
                .map_err(fail)?;
                (report, revision.is_some())
            }
        };
        if json_out {
            documents.push(
                json!({ "module": id, "dry_run": dry, "changed": changed, "report": report }),
            );
            continue;
        }
        println!(
            "content {id}: {} map(s) {}",
            report.maps(),
            if dry { "would be made" } else { "made" }
        );
        report.lines().iter().for_each(|l| println!("  {l}"));
    }
    if json_out {
        crate::wardrobe_fix::print_json(documents);
    }
    Ok(ExitCode::SUCCESS)
}

/// A size such as `64MB`, `512 KB`, `2GB` or a plain count of bytes.
fn size_of(text: &str) -> Result<u64, Failure> {
    let said = || Usage(format!("--target needs a size such as 64MB, not {text:?}"));
    let text = text.trim();
    let digits = text.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == ' ');
    let unit = text[digits.len()..].trim().to_ascii_lowercase();
    let n: f64 = digits.trim().parse().map_err(|_| said())?;
    let times: f64 = match unit.as_str() {
        "" | "b" => 1.0,
        "kb" | "k" | "kib" => 1024.0,
        "mb" | "m" | "mib" => 1024.0 * 1024.0,
        "gb" | "g" | "gib" => 1024.0 * 1024.0 * 1024.0,
        _ => return Err(said()),
    };
    if !(n.is_finite() && n > 0.0) {
        return Err(said());
    }
    Ok((n * times) as u64)
}

/// `wardrobe budget`.
pub fn budget(args: &[String]) -> Result<ExitCode, Failure> {
    let (rest, bare) = without(args, &["--json", "--apply"]);
    let (json_out, apply) = (bare[0], bare[1]);
    let (positional, flags) = parse(&rest, &["--module", "--garment", "--target", "--smallest"])?;
    let target =
        size_of(flags.get("--target").ok_or_else(|| {
            Usage("--target <size> says the memory the textures are to fit".into())
        })?)?;
    let garments: Option<Vec<String>> = flags.get("--garment").map(|g| vec![g.clone()]);
    let mut documents = Vec::new();
    let mut unmet = false;
    for (id, folder, _lock) in packs(&positional, &flags)? {
        let wardrobe = or_wardrobe::Wardrobe::read(&folder)
            .map_err(|e| Run(format!("content `{id}`: {e}")))?;
        let mut options = BudgetOptions {
            legacy: wardrobe.rules.holds(Target::Legacy),
            ..BudgetOptions::default()
        };
        if let Some(side) = flags.get("--smallest") {
            options.smallest = side
                .parse()
                .map_err(|_| Usage(format!("--smallest needs a number of pixels, not {side:?}")))?;
        }
        let watch = Watch::default();
        let fail = |e: or_wardrobe::edit::Error| Run(format!("content `{id}`: {e}"));
        let plan =
            or_wardrobe::budget::plan(&folder, garments.as_deref(), target, &options, &watch)
                .map_err(fail)?;
        unmet |= !plan.met;
        let report = match apply {
            true => {
                let (report, revision) = or_wardrobe::budget::accept(
                    &folder,
                    garments.as_deref(),
                    &plan,
                    &options,
                    &watch,
                )
                .map_err(fail)?;
                Some((report, revision.is_some()))
            }
            false => None,
        };
        if json_out {
            documents.push(json!({
                "module": id,
                "applied": report.is_some(),
                "changed": report.as_ref().is_some_and(|(_, changed)| *changed),
                "plan": plan,
                "report": report.as_ref().map(|(report, _)| report),
            }));
            continue;
        }
        println!("content {id}:");
        plan.lines().iter().for_each(|l| println!("  {l}"));
        match &report {
            Some((report, _)) => {
                println!("  written: {} texture(s) changed", report.textures());
                report.lines().iter().for_each(|l| println!("    {l}"));
            }
            None => println!("  a dry run: nothing is written until --apply"),
        }
    }
    if json_out {
        crate::wardrobe_fix::print_json(documents);
    }
    Ok(if unmet {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// `wardrobe slot`: a folder an external editor saves into, bound to a slot of a garment's texture.
///
/// The editor does the watching, since a binding is what it keeps in the pack's private folder; here a binding is made or let go, listed, and the
/// newest picture saved in its folder put into the slot now.
pub fn slot(args: &[String]) -> Result<ExitCode, Failure> {
    use or_wardrobe::slots::{bind, bindings, deliver, newest, unbind, Binding, MapSlot};
    let (rest, bare) = without(args, &["--json", "--list", "--unbind", "--now"]);
    let (json_out, list, forget, now) = (bare[0], bare[1], bare[2], bare[3]);
    let (positional, flags) = parse(
        &rest,
        &["--module", "--garment", "--texture", "--slot", "--folder"],
    )?;
    let mut documents = Vec::new();
    for (id, pack, _lock) in packs(&positional, &flags)? {
        if list {
            let all = bindings(&pack);
            if json_out {
                documents.push(json!({ "module": id, "bindings": all }));
                continue;
            }
            println!("content {id}: {} binding(s)", all.len());
            for b in &all {
                println!(
                    "  {} of texture {} of garment {} <- {}",
                    b.slot.label(),
                    b.texture + 1,
                    b.garment,
                    b.folder
                );
            }
            continue;
        }
        let garment = flags
            .get("--garment")
            .ok_or_else(|| Usage("--garment <id> says which garment".into()))?;
        let folder = flags
            .get("--folder")
            .ok_or_else(|| Usage("--folder <dir> says where the editor saves".into()))?;
        let number: usize = flags
            .get("--texture")
            .map_or(Ok(1), |n| n.parse())
            .map_err(|_| Usage("--texture is the number of the texture, from 1".into()))?;
        let slot = match flags.get("--slot") {
            Some(word) => MapSlot::of_word(word)
                .ok_or_else(|| Usage("--slot is diffuse, specular or normal".into()))?,
            None => MapSlot::Diffuse,
        };
        let binding = Binding {
            folder: folder.clone(),
            garment: garment.clone(),
            texture: number.saturating_sub(1),
            slot,
        };
        let fail = |e: or_wardrobe::edit::Error| Run(format!("content `{id}`: {e}"));
        let mut said = Vec::new();
        if forget {
            unbind(&pack, &binding).map_err(fail)?;
            said.push(format!("{folder} is no longer bound"));
        } else {
            bind(&pack, binding.clone()).map_err(fail)?;
            said.push(format!(
                "{folder} is bound to the {} of texture {number}",
                slot.word()
            ));
            if now {
                match newest(&binding) {
                    Some(file) => {
                        let done = deliver(&pack, &binding, &file).map_err(fail)?;
                        said.push(format!(
                            "{} is now {}/{DIR}/{}",
                            file.display(),
                            done.source,
                            done.file
                        ));
                    }
                    None => said.push(format!("{folder} holds no picture yet")),
                }
            }
        }
        if json_out {
            documents.push(json!({ "module": id, "said": said }));
        } else {
            said.iter().for_each(|l| println!("content {id}: {l}"));
        }
    }
    if json_out {
        crate::wardrobe_fix::print_json(documents);
    }
    Ok(ExitCode::SUCCESS)
}

const DIR: &str = or_source::textures::DIR;
