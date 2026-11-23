//! Fix all safe: a pipeline of repair, textures, order and mask, each step reporting what it did, as one step of Undo, and, when asked, a fifth that adds the levels of detail a model lacks.
//!
//! "Safe" is what changes no picture on purpose, takes no garment out and names no file a person did not choose: a model whose
//! counts contradict their runs is repaired where that loses nothing ([`crate::repair`]), a texture is given its mips, its
//! power of two and a format the game's clothing uses ([`crate::textures`], with no change of size, and none of what a rule the project
//! switched off or waived is about), a pin that clashes with another or
//! lies past the end of its slot is let go, and a stated mask whose skin bit disagrees with the garment's skin states it again
//! ([`crate::edit::remedy`]). Everything else a pack is found at fault for is listed with
//! why this leaves it: a duplicate garment is the person's to take out, and a level of detail is the mesh doctor's to make unless [`Also`] asks for it here.
//!
//! ⚠ **Run twice, it changes nothing the second time**: no file is written and no folder made, since each step finds what the
//! first made right.

use std::path::Path;

use or_project::content::facts::Facts;
use serde::Serialize;

use crate::alloc::allocate;
use crate::edit::{garment_mut, remedy, Copying, Error, Revision, Watch};
use crate::facts::{inspect, Depth};
use crate::intake::Scratch;
use crate::mesh::{self, Op};
use crate::model::Wardrobe;
use crate::repair::{self, Repaired};
use crate::rules::{check, name_of};
use crate::textures;

/// A step of the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Repair,
    Textures,
    Order,
    Mask,
    /// Asked for with [`Also`], never run by default.
    Levels,
}

impl Step {
    pub fn label(self) -> &'static str {
        match self {
            Self::Repair => "Repair",
            Self::Textures => "Textures",
            Self::Order => "Order",
            Self::Mask => "Mask",
            Self::Levels => "Levels",
        }
    }
}

/// What Fix all safe does beyond its four steps, when it is asked to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Also {
    /// The levels of detail to add to a model that has a high level and lacks a medium or a low one, each with the share of the
    /// high level's triangles it keeps, as the mesh doctor's Add levels of detail makes them; a level a model has is left as it is.
    /// None runs no levels step.
    pub levels: Option<Vec<(usize, f32)>>,
}

/// A file, or a texture of one, that a step was to change and would not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub file: String,
    pub why: String,
}

/// What one step did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepReport {
    pub step: Step,
    pub changed: Vec<String>,
    pub refused: Vec<Refusal>,
}

impl StepReport {
    fn new(step: Step) -> Self {
        Self {
            step,
            changed: Vec::new(),
            refused: Vec::new(),
        }
    }
}

/// A finding the pipeline leaves standing, with why it is left.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Standing {
    pub code: &'static str,
    pub garment: Option<String>,
    pub message: String,
    pub why: &'static str,
}

/// What Fix all safe did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub steps: Vec<StepReport>,
    pub left: Vec<Standing>,
}

/// How many standing findings a report lists; the rest are counted.
pub const LISTED: usize = 100;

impl Report {
    /// Whether any step changed the pack.
    pub fn changed(&self) -> bool {
        self.steps.iter().any(|s| !s.changed.is_empty())
    }

    /// The report as lines a person reads: each step with what it changed and refused, then what stands.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for step in &self.steps {
            let said = match (step.changed.is_empty(), step.refused.is_empty()) {
                (true, true) => "nothing to do".to_string(),
                (false, _) => format!("{} change(s)", step.changed.len()),
                (true, false) => "nothing changed".to_string(),
            };
            lines.push(format!("{}: {said}", step.step.label()));
            lines.extend(step.changed.iter().map(|c| format!("  {c}")));
            lines.extend((step.refused.iter()).map(|r| format!("  refused {}: {}", r.file, r.why)));
        }
        lines.extend(
            (self.left.iter().take(LISTED))
                .map(|s| format!("stands: {} ({}): {}", s.message, s.code, s.why)),
        );
        if self.left.len() > LISTED {
            lines.push(format!("and {} more stand", self.left.len() - LISTED));
        }
        lines
    }

    /// One sentence for a toast: what changed in the pack, counted.
    pub fn sentence(&self) -> String {
        let counts: Vec<String> = (self.steps.iter())
            .filter(|s| !s.changed.is_empty())
            .map(|s| format!("{}: {}", s.step.label().to_lowercase(), s.changed.len()))
            .collect();
        let stands = match self.left.len() {
            0 => String::new(),
            n => format!("; {n} finding(s) stand"),
        };
        match counts.is_empty() {
            true => format!("Fix all safe found nothing it could change{stands}"),
            false => format!("Fix all safe changed {}{stands}", counts.join(", ")),
        }
    }
}

/// Repair the compiled models of the garments: a source for each that needed it, and a refusal by name for each that cannot be.
fn repair_step(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    watch: &Watch,
) -> Result<StepReport, Error> {
    let mut report = StepReport::new(Step::Repair);
    let mut scratch = Scratch::default();
    let mut done: std::collections::BTreeMap<String, Option<String>> = Default::default();
    let paths: Vec<String> = {
        let mut all: Vec<String> = Vec::new();
        for g in &wardrobe.garments {
            for model in g.model.iter().chain(g.alternates.iter()) {
                if !all.contains(model) {
                    all.push(model.clone());
                }
            }
        }
        all
    };
    let total = paths.len() as u64;
    for (n, rel) in paths.iter().enumerate() {
        watch.check()?;
        watch.progress.reached(n as u64, total);
        watch.progress.now_on(rel.clone());
        match repair::model(copying, &mut scratch, rel) {
            Ok(Repaired::Nothing) => {}
            Ok(Repaired::Source(into, lines)) => {
                report
                    .changed
                    .push(format!("{} is now {into}", repair::sentence(rel, &lines)));
                done.insert(rel.clone(), Some(into));
            }
            Err(why) => report.refused.push(Refusal {
                file: rel.clone(),
                why,
            }),
        }
    }
    for g in &mut wardrobe.garments {
        for model in g.model.iter_mut().chain(g.alternates.iter_mut()) {
            if let Some(Some(into)) = done.get(model.as_str()) {
                *model = into.clone();
            }
        }
    }
    Ok(report)
}

/// Optimise the textures with the options that change no size, less what a rule the project switched off or waived is about.
fn textures_step(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    watch: &Watch,
) -> Result<StepReport, Error> {
    let options = textures::safe_options(wardrobe);
    let found = textures::run(wardrobe, copying, None, &options, true, watch)?;
    let mut report = StepReport::new(Step::Textures);
    for one in &found.changed {
        report
            .changed
            .extend(one.changes.iter().map(|c| c.to_string()));
    }
    report.refused = (found.left.into_iter())
        .map(|l| Refusal {
            file: l
                .texture
                .map_or_else(|| l.file.clone(), |t| format!("{t} of {}", l.file)),
            why: l.why,
        })
        .collect();
    Ok(report)
}

/// Let go every pin that clashes with another or lies past the end of its slot, the way the finding's own remedy does.
fn order_step(wardrobe: &mut Wardrobe, watch: &Watch) -> Result<StepReport, Error> {
    let mut report = StepReport::new(Step::Order);
    let facts = Facts::default();
    // One pin is let go at a time, since letting one go can free the place another clashed over.
    for _ in 0..=wardrobe.garments.len() {
        watch.check()?;
        let found = check(wardrobe, &allocate(wardrobe), &facts);
        let next = (found.iter())
            .filter(|f| matches!(f.code, "position-clash" | "slot-full") && f.waived.is_none())
            .find_map(|f| remedy(wardrobe, f));
        let Some(fix) = next else { break };
        fix.apply(wardrobe)?;
        report.changed.push(fix.what);
    }
    Ok(report)
}

/// State again the mask of every garment whose only disagreement with its skin is the skin bit, the way the finding's own remedy does.
fn mask_step(wardrobe: &mut Wardrobe, watch: &Watch) -> Result<StepReport, Error> {
    let mut report = StepReport::new(Step::Mask);
    watch.check()?;
    // Each mask is its own: mending one frees nothing for another, so the findings are read once.
    let found = check(wardrobe, &allocate(wardrobe), &Facts::default());
    for finding in found.iter().filter(|f| f.code == "mask-skin") {
        if let Some(fix) = remedy(wardrobe, finding) {
            fix.apply(wardrobe)?;
            report.changed.push(fix.what);
        }
    }
    Ok(report)
}

/// Give the models that lack a medium or a low level those levels, with the mesh doctor, as a step of this pipeline.
fn levels_step(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    levels: &[(usize, f32)],
    watch: &Watch,
) -> Result<StepReport, Error> {
    let op = Op::Levels {
        levels: levels.to_vec(),
        replace: false,
    };
    let done = mesh::run_many_in(wardrobe, copying, None, &op, None, watch)?;
    let mut report = StepReport::new(Step::Levels);
    for model in &done.models {
        report.changed.push(format!(
            "{} is now {}: triangles by level {} -> {}",
            model.file,
            model.into,
            mesh::shown(&model.before),
            mesh::shown(&model.after)
        ));
    }
    report.refused = (done.refused.into_iter())
        .map(|r| Refusal {
            file: r.file,
            why: r.why,
        })
        .collect();
    Ok(report)
}

/// Why the pipeline leaves a finding of rule `code`.
fn why_left(code: &str) -> &'static str {
    match code {
        "mask-skin" => {
            "the mask asks for the match form of the files, which the pack never ships: taking that property off is yours to decide"
        }
        "lod-missing" | "lod-medium-heavy" | "lod-low-heavy" | "polycount-high" => {
            "a level of detail is a change to the model's geometry, which the mesh doctor makes, and this only when it is asked to add the missing levels"
        }
        "texture-size" | "dictionary-size" => {
            "a smaller picture is a choice: the texture optimiser scales by a ratio or to a size"
        }
        "duplicate-content" => "taking a garment out of the pack is yours to decide",
        "renumbered" => {
            "pinning the garment where it was built, or letting it move with a reason, is yours to decide"
        }
        "slot-cap" => {
            "a slot past the cap is written as several collections, and raising the cap is yours to decide"
        }
        "model-unreadable" | "texture-unreadable" | "cloth-unreadable" => {
            "the reader refuses the file, and nothing here mends a file it cannot open"
        }
        "texture-format" => {
            "only a colour texture stored uncompressed, or as BC7 for Legacy, is stored again; a mask or another format is kept"
        }
        "texture-bc7-legacy" | "texture-mips" | "texture-not-power-of-two" => {
            "the texture was left as it is: the textures step says why"
        }
        _ => "it needs a choice or a file of yours",
    }
}

/// The findings of `wardrobe` as the disk says them, those a person has not waived, with why the pipeline leaves each.
fn standing(folder: &Path, wardrobe: &Wardrobe) -> Vec<Standing> {
    let facts = inspect(folder, wardrobe, Depth::Measure);
    check(wardrobe, &allocate(wardrobe), &facts)
        .into_iter()
        .filter(|f| f.waived.is_none())
        .map(|f| Standing {
            code: f.code,
            garment: f
                .garment
                .as_deref()
                .and_then(|id| wardrobe.garment(id))
                .map(name_of),
            message: f.message,
            why: why_left(f.code),
        })
        .collect()
}

/// Run the pipeline over the pack in `folder`, with the Undo step that takes it all back; none where it changed nothing.
///
/// ⚠ Stopped by `watch`, the pack and its folder are as they were.
pub fn all_safe(folder: &Path, watch: &Watch) -> Result<(Report, Option<Revision>), Error> {
    all_safe_also(folder, watch, &Also::default())
}

/// [`all_safe`] and, where `also` asks, the steps a person turns on: still one Undo step, and run twice it changes nothing the second time.
pub fn all_safe_also(
    folder: &Path,
    watch: &Watch,
    also: &Also,
) -> Result<(Report, Option<Revision>), Error> {
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    let mut copying = Copying::new(folder);
    let steps = (|| {
        let mut steps = vec![
            repair_step(&mut wardrobe, &mut copying, watch)?,
            textures_step(&mut wardrobe, &mut copying, watch)?,
            order_step(&mut wardrobe, watch)?,
            mask_step(&mut wardrobe, watch)?,
        ];
        if let Some(levels) = &also.levels {
            steps.push(levels_step(&mut wardrobe, &mut copying, levels, watch)?);
        }
        Ok(steps)
    })();
    let steps = match steps {
        Ok(steps) => steps,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    let left = standing(folder, &wardrobe);
    let revision = textures::finish(folder, before, &wardrobe, copying)?;
    Ok((Report { steps, left }, revision))
}

/// Repair the model of garment `id`, and its alternates, as Fix does on a finding of the model: the Undo step, and what the repair said.
///
/// Refused, naming the file, where the file cannot be repaired.
pub fn repair_garment(folder: &Path, id: &str) -> Result<(StepReport, Option<Revision>), Error> {
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    let garment = (wardrobe.garment(id)).ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let paths: Vec<String> = (garment.model.iter().chain(garment.alternates.iter()))
        .cloned()
        .collect();
    let mut copying = Copying::new(folder);
    let mut scratch = Scratch::default();
    let mut report = StepReport::new(Step::Repair);
    for rel in &paths {
        match repair::model(&mut copying, &mut scratch, rel) {
            Ok(Repaired::Nothing) => {}
            Ok(Repaired::Source(into, lines)) => {
                report
                    .changed
                    .push(format!("{} is now {into}", repair::sentence(rel, &lines)));
                let g = garment_mut(&mut wardrobe, id)?;
                for model in g.model.iter_mut().chain(g.alternates.iter_mut()) {
                    if model == rel {
                        *model = into.clone();
                    }
                }
            }
            Err(why) => {
                copying.undo();
                return Err(Error::Refused(format!("{rel} cannot be repaired: {why}")));
            }
        }
    }
    let revision = textures::finish(folder, before, &wardrobe, copying)?;
    Ok((report, revision))
}
