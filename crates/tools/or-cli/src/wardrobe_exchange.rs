//! What moves a pack in and out of OpenRage with no window: `wardrobe reopen`, `package`, `add` and `verify`, and the four routes of
//! `serve` that change a pack, `wardrobe-add`, `wardrobe-set`, `wardrobe-fix` and `wardrobe-waive`.
//!
//! A route changes the clothing content of the project the caller names, and never the install. Each takes the right to write the
//! pack first, as a window does, so a pack another OpenRage holds is refused and not raced, and each is all or nothing: a refusal
//! changes nothing.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use or_project::{ModuleKind, Plan, Project};
use or_wardrobe::audio::Which;
use or_wardrobe::behaviour::Behaviour;
use or_wardrobe::edit::{self, Edit, Watch};
use or_wardrobe::flags::Flag;
use or_wardrobe::lock::PackLock;
use or_wardrobe::mask::RenderFlag;
use or_wardrobe::reopen::Options;
use or_wardrobe::{Depth, Ped, Skin, Slot};
use serde_json::{json, Map, Value};

use crate::project::{parse, planned as planned_plans, root_of, Failure};
use Failure::{Run, Usage};

// ---- The routes of `serve`: JSON in, JSON out, one sentence on a refusal.

fn text<'a>(params: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(format!("`{key}` is text")),
    }
}

fn required<'a>(params: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    text(params, key)?.ok_or_else(|| format!("`{key}` is required"))
}

/// The strings `key` holds: a list of them, or one.
fn strings(params: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(one)) => Ok(vec![one.clone()]),
        Some(Value::Array(many)) => many
            .iter()
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .ok_or_else(|| format!("`{key}` is a list of text"))
            })
            .collect(),
        Some(_) => Err(format!("`{key}` is text or a list of text")),
    }
}

/// The clothing content of the project `project` that `module` names (the only one when it names none), and the right to write it.
pub(crate) fn pack_of(params: &Map<String, Value>) -> Result<(String, PathBuf, PackLock), String> {
    let path = required(params, "project")?;
    let project = Project::load(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    let clothing: Vec<_> = (project.container.modules.iter())
        .filter(|m| m.kind == ModuleKind::Clothing)
        .collect();
    let module = match (text(params, "module")?, clothing.as_slice()) {
        (Some(id), _) => *clothing
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("{path} has no clothing content `{id}`"))?,
        (None, [only]) => only,
        (None, []) => return Err(format!("{path} holds no clothing content")),
        (None, many) => {
            let ids: Vec<&str> = many.iter().map(|m| m.id.as_str()).collect();
            return Err(format!(
                "{path} holds several clothing contents ({}): name one with `module`",
                ids.join(", ")
            ));
        }
    };
    let folder = or_wardrobe::module_folder_to_edit(&project.path, &project.container, module)
        .map_err(|e| format!("content `{}`: {e}", module.id))?;
    let lock = or_wardrobe::lock::acquire(&folder).map_err(|e| e.to_string())?;
    Ok((module.id.clone(), folder, lock))
}

fn left_out(list: &[or_wardrobe::import::LeftOut]) -> Vec<Value> {
    list.iter()
        .map(|l| json!({ "path": l.path, "why": l.why }))
        .collect()
}

/// `wardrobe-add`: drop files and folders on the pack (`from`), or add the garment of a package (`package`).
pub(crate) fn add(params: &Map<String, Value>) -> Result<Value, String> {
    let (id, folder, _lock) = pack_of(params)?;
    let (from, package) = (strings(params, "from")?, text(params, "package")?);
    match (from.is_empty(), package) {
        (false, None) => {
            let dropped: Vec<PathBuf> = from.iter().map(PathBuf::from).collect();
            // ⚠ A path that is not there is a typo, and a drop reads it as nothing: it would answer ok with nothing added.
            if let Some(gone) = dropped.iter().find(|p| !p.exists()) {
                return Err(format!(
                    "{} is not there: nothing was added",
                    gone.display()
                ));
            }
            let added = edit::add_clothes(&folder, &dropped).map_err(|e| e.to_string())?;
            if (added.garments.is_empty() && added.textures == 0 && added.cloth == 0)
                && (added.unread == 0 && added.already_held.is_empty())
                && (added.left_out.is_empty() && added.shop.is_empty())
            {
                return Err("the files hold nothing to add: no garment, texture, cloth or shop metadata, and nothing left out".into());
            }
            Ok(json!({
                "module": id,
                "garments": added.garments,
                "textures": added.textures,
                "cloth": added.cloth,
                "unread": added.unread,
                "alreadyHeld": added.already_held,
                "leftOut": left_out(&added.left_out),
            }))
        }
        (true, Some(package)) => {
            let added = or_wardrobe::package::import(&folder, Path::new(package))
                .map_err(|e| e.to_string())?;
            Ok(json!({ "module": id, "garments": [added.garment], "notes": added.notes }))
        }
        _ => Err(
            "say `from` (files and folders to add) or `package` (a garment package), one of them"
                .into(),
        ),
    }
}

/// A key's value as the text or the nothing it states.
fn nullable<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text)),
        _ => Err(format!("`{key}` is text, or null for none")),
    }
}

fn behave(change: Behaviour) -> Edit {
    Edit::Behaviour(change)
}

/// A key's number, or none for null.
fn number(value: &Value, key: &str) -> Result<Option<f32>, String> {
    match value {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .map(|n| Some(n as f32))
            .ok_or_else(|| format!("`{key}` is a number, or null for none")),
    }
}

/// The flag a name states: the engine's (`PV_FLAG_NOT_IN_CAR`) with or without its prefix, in any case.
fn flag_named(name: &str) -> Result<Flag, String> {
    let wanted = name.trim().to_ascii_uppercase();
    let wanted = wanted.strip_prefix("PV_FLAG_").unwrap_or(&wanted);
    Flag::ALL
        .into_iter()
        .find(|f| f.name().strip_prefix("PV_FLAG_") == Some(wanted))
        .ok_or_else(|| format!("`{name}` is no flag of the engine's ePedCompFlags"))
}

/// The edits a `wardrobe-set` request states, in the order of its keys.
fn edits_of(params: &Map<String, Value>) -> Result<Vec<Edit>, String> {
    let mut edits = Vec::new();
    for (key, value) in params {
        match key.as_str() {
            "label" => edits.push(Edit::Label(nullable(value, key)?.map(String::from))),
            "group" => edits.push(Edit::Group(nullable(value, key)?.map(String::from))),
            "ped" => {
                let ped = nullable(value, key)?
                    .map(|word| match word.to_ascii_lowercase().as_str() {
                        "male" | "m" => Ok(Ped::Male),
                        "female" | "f" => Ok(Ped::Female),
                        other => Ped::from_model(other)
                            .ok_or_else(|| format!("`{word}` is no freemode ped: male or female")),
                    })
                    .transpose()?;
                edits.push(Edit::Ped(ped));
            }
            "slot" => {
                let slot = nullable(value, key)?.map(Slot::from_str).transpose()?;
                edits.push(Edit::Slot(slot));
            }
            "position" => edits.push(Edit::Position(match value {
                Value::Null => None,
                v => Some(v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or(
                    "`position` is a whole number, or null to let it take the lowest free place",
                )?),
            })),
            "skin" => edits.push(Edit::Skin(match nullable(value, key)? {
                Some("universal" | "uni" | "u") => Skin::Universal,
                Some("race" | "whi" | "r") => Skin::Race,
                _ => return Err("`skin` is universal or race".into()),
            })),
            "heels" => edits.push(Edit::Heels(match value {
                Value::Null => None,
                v => Some(v.as_f64().ok_or("`heels` is a number, or null for none")? as f32),
            })),
            "flags" => edits.push(behave(Behaviour::Flags(
                value
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or("`flags` is a whole number of flag bits")?,
            ))),
            "flagOn" | "flagOff" => {
                for name in strings(params, key)? {
                    let flag = flag_named(&name)?;
                    edits.push(behave(Behaviour::Flag(flag, key == "flagOn")));
                }
            }
            "propMask" => edits.push(behave(Behaviour::Properties(
                value
                    .as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or("`propMask` is a whole number from 0 to 255")?,
            ))),
            "renderFlags" => {
                let bits = value
                    .as_u64()
                    .filter(|n| *n < 8)
                    .ok_or("`renderFlags` is a whole number from 0 to 7")?;
                for flag in RenderFlag::ALL {
                    edits.push(behave(Behaviour::Render(
                        *flag,
                        bits >> flag.bit() & 1 != 0,
                    )));
                }
            }
            "stickyness" => edits.push(behave(Behaviour::Stickyness(match value {
                Value::Null => None,
                v => Some(
                    v.as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or("`stickyness` is a whole number from 0 to 255, or null")?,
                ),
            }))),
            "hairScale" => edits.push(behave(Behaviour::HairScale(number(value, key)?))),
            "hairHeight" => edits.push(behave(Behaviour::HairHeight(number(value, key)?))),
            "audio" => edits.push(behave(Behaviour::Audio(
                Which::First,
                nullable(value, key)?.map(String::from),
            ))),
            "audio2" => edits.push(behave(Behaviour::Audio(
                Which::Second,
                nullable(value, key)?.map(String::from),
            ))),
            "firstPerson" => edits.push(behave(Behaviour::FirstPerson(match value {
                Value::Null => None,
                v => Some(
                    v.as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or("`firstPerson` is the number of an alternate model, or null")?,
                ),
            }))),
            "addTag" => edits.extend(strings(params, key)?.into_iter().map(Edit::AddTag)),
            "removeTag" => edits.extend(strings(params, key)?.into_iter().map(Edit::RemoveTag)),
            "remove" if value == &Value::Bool(true) => edits.push(Edit::Remove),
            "project" | "module" | "garments" | "remove" => {}
            other => return Err(format!("`{other}` is nothing a garment states")),
        }
    }
    // ⚠ Taking a garment out comes last, so that what else was said of it was said of a garment that was there.
    edits.sort_by_key(|e| matches!(e, Edit::Remove));
    Ok(edits)
}

/// `wardrobe-set`: say things of garments (`garments`), all or none.
pub(crate) fn set(params: &Map<String, Value>) -> Result<Value, String> {
    let ids = strings(params, "garments")?;
    if ids.is_empty() {
        return Err("`garments` names the garments to change".into());
    }
    let edits = edits_of(params)?;
    if edits.is_empty() {
        return Err("say what to set: label, group, ped, slot, position, skin, heels, flags, flagOn, flagOff, propMask, renderFlags, stickyness, hairScale, hairHeight, audio, audio2, firstPerson, addTag, removeTag or remove".into());
    }
    let (id, folder, _lock) = pack_of(params)?;
    let ((), revision) = edit::revise(&folder, |wardrobe| {
        edits
            .into_iter()
            .try_for_each(|e| edit::apply_all(wardrobe, &ids, e))
    })
    .map_err(|e| e.to_string())?;
    Ok(json!({ "module": id, "garments": ids.len(), "changed": revision.is_some() }))
}

/// `wardrobe-fix`: Fix all safe over the pack.
pub(crate) fn fix(params: &Map<String, Value>) -> Result<Value, String> {
    let (id, folder, _lock) = pack_of(params)?;
    let (report, revision) =
        or_wardrobe::fix::all_safe(&folder, &Watch::default()).map_err(|e| e.to_string())?;
    Ok(json!({
        "module": id,
        "changed": revision.is_some(),
        "sentence": report.sentence(),
        "report": report,
    }))
}

/// `wardrobe-waive`: record that a finding stands, for a reason.
pub(crate) fn waive(params: &Map<String, Value>) -> Result<Value, String> {
    let (code, reason) = (required(params, "code")?, required(params, "reason")?);
    let garment = text(params, "garment")?;
    let (id, folder, _lock) = pack_of(params)?;
    let waivers = edit::update(&folder, |wardrobe| {
        edit::waive(wardrobe, code, garment, reason)?;
        Ok(wardrobe.waivers.len())
    })
    .map_err(|e| e.to_string())?;
    Ok(json!({ "module": id, "waivers": waivers }))
}

// ---- The one-shot commands.

/// `wardrobe reopen <built> --out <folder> [--pack <name>]`: a built DLC, an `.oiv`, a resource laid out as one or a grzyClothTool project as a project of sources.
pub fn reopen_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(args, &["--out", "--pack"])?;
    let [built] = positional.as_slice() else {
        return Err(Usage(
            "expected one <dlc.rpf, .oiv, .gctproject or the folder that holds one>".into(),
        ));
    };
    let Some(out) = flags.get("--out") else {
        return Err(Usage(
            "--out <folder> says where the project is made".into(),
        ));
    };
    let options = Options {
        pack: flags.get("--pack").cloned(),
    };
    let (built, out) = (Path::new(built), Path::new(out));
    let foreign = built
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case(or_wardrobe::gctproject::EXTENSION));
    let made = match foreign {
        true => or_wardrobe::gctproject::reopen(built, out, &options),
        false => or_wardrobe::reopen::reopen(built, out, &options),
    }
    .map_err(|e| Run(e.to_string()))?;
    println!(
        "reopened the pack `{}` as {}: {} garment(s), {} texture(s)",
        made.pack,
        made.project.display(),
        made.garments,
        made.textures
    );
    for left in &made.unplaced {
        println!("not placed {}: {}", left.path, left.why);
    }
    for note in &made.notes {
        println!("note: {note}");
    }
    println!("This reads files. It does not say that GTA V loads the pack.");
    Ok(ExitCode::SUCCESS)
}

/// `wardrobe package <project> --garment <id> --out <folder> [--module <id>]`: a garment as one package of sources and data.
pub fn package_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(args, &["--garment", "--out", "--module"])?;
    let (Some(garment), Some(out)) = (flags.get("--garment"), flags.get("--out")) else {
        return Err(Usage(
            "package needs --garment <id> and --out <folder>".into(),
        ));
    };
    let [project] = positional.as_slice() else {
        return Err(Usage("expected one <project.openrage-proj>".into()));
    };
    let mut params = Map::new();
    params.insert("project".into(), json!(project));
    if let Some(module) = flags.get("--module") {
        params.insert("module".into(), json!(module));
    }
    let (_, folder, _lock) = pack_of(&params).map_err(Run)?;
    let packaged = or_wardrobe::package::export(&folder, garment, Path::new(out))
        .map_err(|e| Run(e.to_string()))?;
    println!(
        "wrote the package {out}: {} file(s) beside {}",
        packaged.files.len(),
        or_wardrobe::package::FILE
    );
    Ok(ExitCode::SUCCESS)
}

/// `wardrobe add <project> <file or folder>... | --package <folder> [--module <id>]`.
pub fn add_of(args: &[String]) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(args, &["--package", "--module"])?;
    let Some((project, from)) = positional.split_first() else {
        return Err(Usage("expected one <project.openrage-proj>".into()));
    };
    let mut params = Map::new();
    params.insert("project".into(), json!(project));
    params.insert("from".into(), json!(from));
    for (flag, key) in [("--package", "package"), ("--module", "module")] {
        if let Some(value) = flags.get(flag) {
            params.insert(key.into(), json!(value));
        }
    }
    let said = add(&params).map_err(Run)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&said).expect("JSON serialises")
    );
    Ok(ExitCode::SUCCESS)
}

// ---- Reading a build back, resource by resource.

/// The clothing checks of the build of `plan` that is in `folder`: for each clothing content of the project that the plan holds, what the metadata it wrote says against the pack.
///
/// ⚠ A split output is several plans, each holding whole contents, so a content is read in the resource that holds it and nowhere else.
pub(crate) fn clothing_checks(
    project: &Project,
    plan: &Plan,
    folder: &Path,
) -> Result<Vec<(String, or_wardrobe::verify::Verification)>, String> {
    let Some(output) = project.container.output(&plan.output) else {
        return Ok(Vec::new());
    };
    let mut read = Vec::new();
    for module in (project.container.modules.iter())
        .filter(|m| m.kind == ModuleKind::Clothing && output.take.contains(&m.id))
    {
        // ⚠ Planned for the build it was written by, and read through the archives of a pack or a package.
        let Ok((wardrobe, planned)) =
            or_wardrobe::planned_for(&project.folder(), module, Depth::Digest, output)
        else {
            continue;
        };
        let theirs = &planned.contribution.files;
        let held = !theirs.is_empty()
            && theirs
                .iter()
                .all(|f| plan.files.iter().any(|p| p.path == f.path));
        if !held {
            continue;
        }
        let files = or_project::built_files(plan, folder)?;
        let found = or_wardrobe::verify::verify_files(&wardrobe, &planned, files.as_ref());
        read.push((module.id.clone(), found));
    }
    Ok(read)
}

/// What reading one resource of a build back found.
pub(crate) struct ReadBack {
    pub resource: String,
    pub folder: PathBuf,
    /// Why the resource is not there or does not reload, one sentence each.
    pub problems: Vec<String>,
    pub reloaded: String,
    pub clothing: Vec<(String, or_wardrobe::verify::Verification)>,
}

impl ReadBack {
    pub(crate) fn is_clean(&self) -> bool {
        self.problems.is_empty() && self.clothing.iter().all(|(_, found)| found.is_clean())
    }

    pub(crate) fn document(&self) -> Value {
        let clothing: Vec<Value> = (self.clothing.iter())
            .map(|(module, found)| {
                let checks: Vec<Value> = (found.checks.iter())
                    .map(|c| json!({ "name": c.name, "passed": c.passed, "detail": c.detail }))
                    .collect();
                json!({ "module": module, "clean": found.is_clean(), "checks": checks })
            })
            .collect();
        json!({
            "resource": self.resource,
            "folder": self.folder.display().to_string(),
            "clean": self.is_clean(),
            "problems": self.problems,
            "reloaded": self.reloaded,
            "clothing": clothing,
        })
    }
}

/// Read each resource of the build of `plans` that `root` holds back, one at a time.
pub(crate) fn read_back(project: &Project, plans: &[Plan], root: &Path) -> Vec<ReadBack> {
    plans
        .iter()
        .map(|plan| {
            let folder = root.join(&plan.resource);
            let mut back = ReadBack {
                resource: plan.resource.clone(),
                folder: folder.clone(),
                problems: Vec::new(),
                reloaded: String::new(),
                clothing: Vec::new(),
            };
            if !folder.is_dir() {
                back.problems.push(format!(
                    "{} is not built: there is no such folder",
                    folder.display()
                ));
                return back;
            }
            let found = or_project::verify(plan, &folder);
            back.reloaded = found.reload_line();
            back.problems = found.problems.clone();
            match clothing_checks(project, plan, &folder) {
                Ok(checks) => back.clothing = checks,
                Err(why) => back.problems.push(why),
            }
            back
        })
        .collect()
}

/// `wardrobe verify <project> [--build <id>] [--out <root>] [--json]`: read what a build wrote back, each resource of it by itself, and say what held.
pub fn verify_of(args: &[String]) -> Result<ExitCode, Failure> {
    let json_out = args.iter().any(|a| a == "--json");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--json").cloned().collect();
    let (positional, flags) = parse(&rest, &["--build", "--out"])?;
    let (project, plans) = planned_plans(&positional, &flags)?;
    let root = root_of(&positional[0], &flags);
    let backs = read_back(&project, &plans, &root);
    if json_out {
        let documents: Vec<Value> = backs.iter().map(ReadBack::document).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&Value::Array(documents)).expect("JSON serialises")
        );
    } else {
        for back in &backs {
            println!("{}", back.folder.display());
            if !back.reloaded.is_empty() {
                println!("  {}", back.reloaded);
            }
            for problem in &back.problems {
                println!("  problem: {problem}");
            }
            for (module, found) in &back.clothing {
                println!("  content {module}:");
                for check in &found.checks {
                    let mark = if check.passed { "ok  " } else { "FAIL" };
                    println!("    {mark} {}: {}", check.name, check.detail);
                }
            }
        }
        println!("This checks the files. It does not test that GTA V loads them.");
    }
    Ok(if backs.iter().all(ReadBack::is_clean) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use or_project::{Container, Module, Output};

    use super::*;

    /// A clothing pack called `name` in `dir/name` holding one jacket, made as dropping its two files makes it.
    fn pack(dir: &Path, name: &str) {
        let folder = dir.join(name);
        or_wardrobe::edit::create(&folder, name).unwrap();
        let drop = dir.join(format!("{name}-drop"));
        std::fs::create_dir_all(&drop).unwrap();
        let c = format!("mp_m_freemode_01_mp_m_{name}");
        let model = or_source::sample::drawables_of(name);
        let texture = or_source::sample::textures_of(name);
        std::fs::write(drop.join(format!("{c}^jbib_000_u.ydd")), model).unwrap();
        std::fs::write(drop.join(format!("{c}^jbib_diff_000_a_uni.ytd")), texture).unwrap();
        or_wardrobe::edit::add_clothes(&folder, &[drop]).unwrap();
    }

    /// A project of the packs `alpha` and `beta` with one build that divides between them, because it is over its ceiling of files.
    fn split(dir: &Path) -> String {
        for name in ["alpha", "beta"] {
            pack(dir, name);
        }
        let whole = |max_files: Option<u64>| {
            let mut output = Output::fivem_resource("split", "demo", &["alpha", "beta"]);
            output.split = true;
            output.max_files = max_files;
            let container = Container {
                modules: vec![
                    Module::clothing("alpha", "alpha"),
                    Module::clothing("beta", "beta"),
                ],
                outputs: vec![output],
            };
            let path = dir.join(format!("demo-{}.openrage-proj", max_files.unwrap_or(0)));
            Project::create(&path, &container, &[]).unwrap();
            path.display().to_string()
        };
        let probe = whole(None);
        let (_, plans) = planned_plans(&[probe], &HashMap::new())
            .map_err(|_| ())
            .unwrap();
        assert_eq!(plans.len(), 1, "no ceiling, no split");
        whole(Some(plans[0].totals.files - 1))
    }

    #[test]
    fn a_split_build_is_read_back_one_resource_at_a_time_and_the_one_that_disagrees_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let project = split(dir.path());
        let out = tempfile::tempdir().unwrap();
        let root = out.path().to_path_buf();
        let root_text = root.display().to_string();
        let args = |verb: &str| {
            vec![
                verb.to_string(),
                project.clone(),
                "--out".into(),
                root_text.clone(),
            ]
        };

        assert_eq!(crate::wardrobe::run(&args("build")), ExitCode::SUCCESS);
        let (loaded, plans) = planned_plans(std::slice::from_ref(&project), &HashMap::new())
            .map_err(|_| ())
            .unwrap();
        assert_eq!(
            plans
                .iter()
                .map(|p| p.resource.as_str())
                .collect::<Vec<_>>(),
            ["demo_1", "demo_2"]
        );

        // Each resource is read by itself, and the content a resource holds is the one read in it.
        let backs = read_back(&loaded, &plans, &root);
        for (back, content) in backs.iter().zip(["alpha", "beta"]) {
            assert!(back.is_clean(), "{}: {:?}", back.resource, back.document());
            let read: Vec<&str> = back.clothing.iter().map(|(id, _)| id.as_str()).collect();
            assert_eq!(read, [content], "{}", back.resource);
        }
        assert_eq!(
            verify_of(&args("verify")[1..]).map_err(|_| ()).unwrap(),
            ExitCode::SUCCESS
        );

        // A variation file of the second resource that is gone is found in the second and not in the first.
        let ymt = walk(&root.join("demo_2"))
            .into_iter()
            .find(|f| f.extension().is_some_and(|x| x == "ymt"))
            .expect("the second resource writes metadata");
        std::fs::remove_file(ymt).unwrap();
        let backs = read_back(&loaded, &plans, &root);
        assert!(
            backs[0].is_clean() && !backs[1].is_clean(),
            "{:?}",
            backs[1].document()
        );
        assert_eq!(
            verify_of(&args("verify")[1..]).map_err(|_| ()).unwrap(),
            ExitCode::FAILURE
        );
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            match path.is_dir() {
                true => files.extend(walk(&path)),
                false => files.push(path),
            }
        }
        files
    }
}

#[cfg(test)]
mod behaviour_keys {
    use super::*;

    fn params(request: Value) -> Map<String, Value> {
        request.as_object().expect("an object").clone()
    }

    #[test]
    fn the_keys_of_behaviour_are_changes_of_the_engines_own_fields() {
        let edits = edits_of(&params(json!({
            "garments": ["a"],
            "flagOn": ["NOT_IN_CAR", "pv_flag_armoured"],
            "flagOff": "BULKY",
            "flags": 4096,
            "propMask": 17,
            "renderFlags": 5,
            "stickyness": 100,
            "hairScale": -0.5,
            "hairHeight": null,
            "audio": "hash_49BC7916",
            "audio2": null,
            "firstPerson": 2
        })))
        .expect("the keys are understood");
        let has = |change: Behaviour| edits.contains(&Edit::Behaviour(change));
        assert!(has(Behaviour::Flag(Flag::NotInCar, true)));
        assert!(has(Behaviour::Flag(Flag::Armoured, true)));
        assert!(has(Behaviour::Flag(Flag::Bulky, false)));
        assert!(has(Behaviour::Flags(4096)));
        assert!(has(Behaviour::Properties(17)));
        assert!(has(Behaviour::Render(RenderFlag::Alpha, true)));
        assert!(has(Behaviour::Render(RenderFlag::Decal, false)));
        assert!(has(Behaviour::Render(RenderFlag::Cutout, true)));
        assert!(has(Behaviour::Stickyness(Some(100))));
        assert!(has(Behaviour::HairScale(Some(-0.5))));
        assert!(has(Behaviour::HairHeight(None)));
        assert!(has(Behaviour::Audio(
            Which::First,
            Some("hash_49BC7916".into())
        )));
        assert!(has(Behaviour::Audio(Which::Second, None)));
        assert!(has(Behaviour::FirstPerson(Some(2))));
    }

    #[test]
    fn a_flag_the_engine_does_not_name_and_a_number_that_does_not_fit_are_refused_by_name() {
        for (request, key) in [
            (json!({ "flagOn": "WINGS" }), "WINGS"),
            (json!({ "propMask": 256 }), "propMask"),
            (json!({ "renderFlags": 8 }), "renderFlags"),
            (json!({ "stickyness": "firm" }), "stickyness"),
            (json!({ "hairScale": "much" }), "hairScale"),
            (json!({ "firstPerson": -1 }), "firstPerson"),
        ] {
            let error = edits_of(&params(request)).expect_err("refused");
            assert!(error.contains(key), "{error}");
        }
    }
}
