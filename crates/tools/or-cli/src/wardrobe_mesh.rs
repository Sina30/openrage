//! `openrage-cli wardrobe mesh`: the mesh doctor of a clothing pack with no window: look at the models of a garment, give them levels of detail, mend their rig, clean their weights, turn them over or crop their texture coordinates.
//!
//! Every operation but the look writes: new sources the garment names, as the editor's buttons do. Each takes the right to write the
//! pack first, so a pack another OpenRage holds is refused and not raced.

use std::process::ExitCode;

use or_wardrobe::edit::Watch;
use or_wardrobe::mesh::{self, Op};
use serde_json::json;

use crate::project::{parse, Failure};
use crate::wardrobe_fix::{packs, without};
use Failure::{Run, Usage};

/// The shares `--levels` asks for: the Medium and the Low level, in percent, with 0 for none.
fn levels(asked: &str) -> Result<Vec<(usize, f32)>, Failure> {
    let said = || {
        Usage(format!("--levels needs the percent of the Medium level and then of the Low one, as 35,8, not {asked:?}"))
    };
    let numbers = asked
        .split(',')
        .map(|n| n.trim().parse::<f32>().map_err(|_| said()))
        .collect::<Result<Vec<_>, _>>()?;
    if numbers.is_empty() || numbers.len() > 2 || numbers.iter().any(|n| !(0.0..=100.0).contains(n))
    {
        return Err(said());
    }
    Ok(numbers
        .into_iter()
        .enumerate()
        .filter(|(_, percent)| *percent > 0.0)
        .map(|(at, percent)| (at + 1, percent / 100.0))
        .collect())
}

/// `wardrobe mesh`.
pub fn mesh(args: &[String]) -> Result<ExitCode, Failure> {
    let (rest, bare) = without(
        args,
        &[
            "--json",
            "--check",
            "--rig",
            "--weights",
            "--mirror",
            "--crop-uvs",
            "--replace",
            "--materials",
        ],
    );
    let (json_out, check, rig, weights, mirror, crop, replace, materials) = (
        bare[0], bare[1], bare[2], bare[3], bare[4], bare[5], bare[6], bare[7],
    );
    let (positional, flags) = parse(
        &rest,
        &[
            "--module",
            "--garment",
            "--levels",
            "--skeleton",
            "--shader-defs",
        ],
    )?;
    // The materials a shader declares for come from the database of an install: --shader-defs, or the one OPENRAGE_SHADER_DEFS names.
    if materials {
        let defs = match flags.get("--shader-defs") {
            Some(file) => Some(
                or_io::gta5::shaders::shader_defs::load_from(std::path::Path::new(file))
                    .map_err(|e| Run(format!("{file}: {e}")))?,
            ),
            None => or_io::gta5::shaders::shader_defs::staged(),
        };
        if let Some(defs) = defs {
            or_io::gta5::shaders::shader_defs::install(defs);
        }
    }
    let asked = [
        check,
        rig,
        weights,
        mirror,
        crop,
        materials,
        flags.contains_key("--levels"),
    ];
    if asked.iter().filter(|a| **a).count() != 1 {
        return Err(Usage(
            "say one of --check, --levels <medium,low>, --rig, --weights, --mirror, --crop-uvs and --materials"
                .into(),
        ));
    }
    // Completing materials is the one operation asked of a whole pack when no garment is named.
    let garment = flags.get("--garment");
    if garment.is_none() && !materials {
        return Err(Usage("--garment <id> says which garment".into()));
    }
    let skeleton = match flags.get("--skeleton") {
        Some(file) => {
            let bytes = std::fs::read(file).map_err(|e| Run(format!("{file}: {e}")))?;
            Some(
                or_source::models::reference_skeleton("yft", &bytes)
                    .map_err(|p| Run(format!("{file}: {p}")))?,
            )
        }
        None => None,
    };
    let op = match (rig, weights, mirror, crop, materials, flags.get("--levels")) {
        (true, ..) => Some(Op::Rig),
        (_, true, ..) => Some(Op::Weights),
        (_, _, true, ..) => Some(Op::Mirror),
        (_, _, _, true, ..) => Some(Op::UvCrop),
        (_, _, _, _, true, _) => Some(Op::Materials),
        (.., Some(shares)) => Some(Op::Levels {
            levels: levels(shares)?,
            replace,
        }),
        _ => None,
    };
    let mut documents = Vec::new();
    for (id, folder, _lock) in packs(&positional, &flags)? {
        match &op {
            None => {
                let garment =
                    garment.ok_or_else(|| Usage("--garment <id> says which garment".into()))?;
                let looked = mesh::examine(&folder, garment, skeleton.as_deref())
                    .map_err(|e| Run(format!("content `{id}`: {e}")))?;
                if json_out {
                    documents.push(json!({ "module": id, "examination": looked }));
                    continue;
                }
                println!("content {id}:");
                looked.lines().iter().for_each(|l| println!("  {l}"));
            }
            Some(op) => {
                let watch = Watch::default();
                let (report, revision) = match garment {
                    Some(garment) => mesh::run(&folder, garment, op, skeleton.as_deref(), &watch),
                    None => mesh::run_many(&folder, None, op, skeleton.as_deref(), &watch),
                }
                .map_err(|e| Run(format!("content `{id}`: {e}")))?;
                if json_out {
                    documents.push(
                        json!({ "module": id, "changed": revision.is_some(), "report": report }),
                    );
                    continue;
                }
                println!("content {id}:");
                report.lines().iter().for_each(|l| println!("  {l}"));
                println!("  {}", report.sentence());
                // A model refused for its own reason is a failure a script should see.
                if !report.refused.is_empty() && !report.changed() {
                    return Ok(ExitCode::FAILURE);
                }
            }
        }
    }
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::Value::Array(documents))
                .expect("JSON serialises")
        );
    }
    Ok(ExitCode::SUCCESS)
}
