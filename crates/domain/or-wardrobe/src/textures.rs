//! The texture optimiser of a pack: the textures sources of its garments made lighter, as one step of Undo.
//!
//! A source is never changed in place. One that has something to change is copied beside itself under a free name, with an id of
//! its own, the copy is optimised, and the garments in scope name the copy; the source it was copied from is untouched, so Undo
//! takes the copy out and the pack names the old source again. A source several garments name is copied once.
//!
//! What a texture comes to is [`or_source::optimise`]'s: this finds the sources, decides which are in scope, says what was left
//! and why, and keeps the pack and the copies together.

use std::collections::BTreeMap;
use std::path::Path;

use or_source::optimise::{self, Options};
use serde::Serialize;

use crate::edit::{Copying, Error, Revision, Watch};
use crate::facts::{source_kind, stays_inside};
use crate::model::{Garment, Wardrobe};
use crate::rules::{name_of, rule_of, waived_for};
use crate::settings::Target;

/// What an optimisation of a pack did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Report {
    pub changed: Vec<Optimised>,
    /// The models whose embedded textures were made lighter, each a new source the garments name now.
    pub models: Vec<crate::mesh::ModelReport>,
    pub left: Vec<Left>,
}

/// A source that was copied and made lighter, and the garments that name the copy now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Optimised {
    pub garments: Vec<String>,
    pub from: String,
    pub into: String,
    pub changes: Vec<optimise::Change>,
}

/// A texture, or a whole source, that was not changed, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Left {
    pub file: String,
    pub texture: Option<String>,
    pub why: String,
}

impl Report {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.models.is_empty() && self.left.is_empty()
    }

    /// How many textures were changed, in a source and embedded in a model.
    pub fn textures(&self) -> usize {
        self.changed.iter().map(|o| o.changes.len()).sum::<usize>()
            + self.models.iter().map(|m| m.textures).sum::<usize>()
    }

    /// The report as lines a person reads: each texture changed, then each one left with its reason.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for one in &self.changed {
            lines.push(format!(
                "{} is now {} for {}",
                one.from,
                one.into,
                one.garments.join(", ")
            ));
            lines.extend(one.changes.iter().map(|c| format!("  {c}")));
        }
        for model in &self.models {
            lines.push(format!("{} is now {}", model.file, model.into));
            lines.extend(model.lines.iter().map(|l| format!("  {l}")));
        }
        for left in &self.left {
            let which = left.texture.as_ref().map_or_else(
                || left.file.clone(),
                |texture| format!("{texture} of {}", left.file),
            );
            lines.push(format!("{which} was left as it is: {}", left.why));
        }
        lines
    }
}

/// What the pack's edition lets the optimiser change with no choice of the person's.
pub fn safe_options(wardrobe: &Wardrobe) -> Options {
    Options::safe(wardrobe.rules.holds(Target::Legacy))
}

/// How the pack keeps the rule `code` from being acted on for `garment`: switched off in its rules, or waived with a reason; none where the rule stands.
fn held_back(wardrobe: &Wardrobe, code: &str, garment: &Garment) -> Option<String> {
    if !wardrobe.rules.effective(rule_of(code)?).on {
        return Some("switched off in the project's rules".into());
    }
    waived_for(wardrobe, code, &garment.id).map(|reason| format!("waived ({reason})"))
}

/// `options` without what a rule that is off or waived for one of `garments` is about, with a sentence for each thing dropped.
///
/// ⚠ A source several garments name is held back by any one of them: it is one file, so changing it for the others changes it for that one too.
fn kept_by_rules(
    wardrobe: &Wardrobe,
    garments: &[&Garment],
    options: &Options,
) -> (Options, Vec<String>) {
    let mut narrow = *options;
    let mut said = Vec::new();
    let mut hold = |on: &mut bool, code: &str, kept: &str| {
        let held = garments.iter().find_map(|g| held_back(wardrobe, code, g));
        if let (true, Some(how)) = (*on, held) {
            *on = false;
            said.push(format!("{code} is {how}, so {kept}"));
        }
    };
    hold(&mut narrow.format, "texture-format", "its format is kept");
    hold(&mut narrow.mips, "texture-mips", "its mips are kept");
    hold(
        &mut narrow.power_of_two,
        "texture-not-power-of-two",
        "its size is kept",
    );
    if narrow.format {
        hold(
            &mut narrow.legacy,
            "texture-bc7-legacy",
            "a BC7 texture is kept as BC7",
        );
    }
    (narrow, said)
}

/// The garments of `wardrobe` in scope that name `rel` for a variation.
fn naming<'a>(
    wardrobe: &'a Wardrobe,
    in_scope: &dyn Fn(&str) -> bool,
    rel: &str,
) -> Vec<&'a Garment> {
    (wardrobe.garments.iter())
        .filter(|g| in_scope(&g.id))
        .filter(|g| (g.variations.iter()).any(|v| v.texture.as_deref() == Some(rel)))
        .collect()
}

/// Walk the textures sources the garments in scope name, a step for each, and have the garments name the copy a step made.
///
/// A name that leads out of the pack, a reference, a file that is not there and anything that is not a textures source is left and
/// said so, and the step is not asked about it. A step answers with the path of the copy it made, or none where it made none.
///
/// ⚠ Stopped by `watch`, it leaves `wardrobe` half changed and `copying` holding what it copied: the caller takes the copies back.
pub(crate) fn over_sources(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    ids: Option<&[String]>,
    watch: &Watch,
    left: &mut Vec<Left>,
    mut step: impl FnMut(&Wardrobe, &mut Copying, &str, &mut Vec<Left>) -> Result<Option<String>, Error>,
) -> Result<(), Error> {
    let folder = copying.folder.to_path_buf();
    let in_scope = |id: &str| ids.is_none_or(|ids| ids.iter().any(|i| i == id));
    let mut named: Vec<String> = Vec::new();
    for garment in wardrobe.garments.iter().filter(|g| in_scope(&g.id)) {
        for texture in garment.variations.iter().filter_map(|v| v.texture.as_ref()) {
            if !named.contains(texture) {
                named.push(texture.clone());
            }
        }
    }
    let mut copies: BTreeMap<String, String> = BTreeMap::new();
    let total = named.len() as u64;
    for (n, rel) in named.iter().enumerate() {
        watch.check()?;
        watch.progress.reached(n as u64, total);
        watch.progress.now_on(rel.clone());
        let one = |why: String| Left {
            file: rel.clone(),
            texture: None,
            why,
        };
        if !stays_inside(rel) {
            continue;
        }
        if or_project::reference::is_reference(rel) {
            left.push(one(
                "it leads to a file or a source somewhere else, which is changed where it is"
                    .into(),
            ));
            continue;
        }
        let path = folder.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !path.exists() {
            continue;
        }
        if source_kind(rel) != Some(or_source::textures::KIND) || !path.is_dir() {
            left.push(one(match path.is_file() {
                true => "it is a compiled file, which is not changed: put a source in its place (Replace texture takes the same .ytd and makes one) and optimise that".into(),
                false => "it is not a textures source".into(),
            }));
            continue;
        }
        if let Some(into) = step(wardrobe, copying, rel, left)? {
            copies.insert(rel.clone(), into);
        }
    }
    for garment in wardrobe.garments.iter_mut().filter(|g| in_scope(&g.id)) {
        for variation in &mut garment.variations {
            if let Some(into) = variation.texture.as_ref().and_then(|t| copies.get(t)) {
                variation.texture = Some(into.clone());
            }
        }
    }
    watch.progress.reached(total, total);
    Ok(())
}

/// The names of the garments in scope that name `rel` for a variation.
pub(crate) fn who(wardrobe: &Wardrobe, ids: Option<&[String]>, rel: &str) -> Vec<String> {
    let in_scope = |id: &str| ids.is_none_or(|ids| ids.iter().any(|i| i == id));
    naming(wardrobe, &in_scope, rel)
        .into_iter()
        .map(name_of)
        .collect()
}

/// Optimise the textures of the garments `ids` (every garment where none) of `wardrobe` as `options` say, copying into `copying`'s folder; the pack is changed to name the copies.
///
/// With `by_rules`, a source that a rule the pack switched off or waived is about is not changed in what that rule is about, and the report lists it.
///
/// ⚠ Stopped by `watch`, it leaves `wardrobe` half changed and `copying` holding what it copied: the caller takes the copies back.
pub(crate) fn run(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    ids: Option<&[String]>,
    options: &Options,
    by_rules: bool,
    watch: &Watch,
) -> Result<Report, Error> {
    if let Some(why) = options.refusal() {
        return Err(Error::Refused(why));
    }
    let folder = copying.folder.to_path_buf();
    let in_scope = |id: &str| ids.is_none_or(|ids| ids.iter().any(|i| i == id));
    let mut report = Report::default();
    let mut left = Vec::new();
    let mut changed = Vec::new();
    over_sources(
        wardrobe,
        copying,
        ids,
        watch,
        &mut left,
        |wardrobe, copying, rel, left| {
            let path = folder.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            let one = |why: String| Left {
                file: rel.to_string(),
                texture: None,
                why,
            };
            let (narrow, held) = match by_rules {
                true => kept_by_rules(wardrobe, &naming(wardrobe, &in_scope, rel), options),
                false => (*options, Vec::new()),
            };
            let plan = match optimise::plan(&path, &narrow) {
                Ok(plan) => plan,
                Err(problems) => {
                    left.push(one(join(&problems)));
                    return Ok(None);
                }
            };
            left.extend(plan.left.iter().map(|l| Left {
                file: rel.to_string(),
                texture: Some(l.texture.clone()),
                why: l.why.clone(),
            }));
            // Said only where the rules kept something the options would have changed.
            if !held.is_empty()
                && optimise::plan(&path, options).is_ok_and(|full| full.changes != plan.changes)
            {
                left.push(one(held.join("; ")));
            }
            if plan.changes.is_empty() {
                return Ok(None);
            }
            let mark = copying.mark();
            let into = match copying.duplicate_source(rel) {
                Ok(into) => into,
                Err(e) => {
                    copying.rollback_to(mark);
                    left.push(one(format!("it could not be copied: {e}")));
                    return Ok(None);
                }
            };
            let stop = || watch.cancel.is_set();
            match optimise::apply(&folder.join(&into), &narrow, &stop) {
                Ok(done) => {
                    changed.push(Optimised {
                        garments: who(wardrobe, ids, rel),
                        from: rel.to_string(),
                        into: into.clone(),
                        changes: done.changes,
                    });
                    Ok(Some(into))
                }
                Err(problems) => {
                    copying.rollback_to(mark);
                    watch.check()?;
                    left.push(one(join(&problems)));
                    Ok(None)
                }
            }
        },
    )?;
    report.changed = changed;
    report.left = left;
    Ok(report)
}

/// Optimise the textures embedded in the models of the garments `ids` (every garment where none) as `options` say, each model a new source the garments name; what it did and what it left are added to `report`.
pub(crate) fn run_embedded(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    ids: Option<&[String]>,
    options: &Options,
    watch: &Watch,
    report: &mut Report,
) -> Result<(), Error> {
    let mut scratch = crate::intake::Scratch::default();
    let mut done = BTreeMap::new();
    let wanted: Vec<String> = wardrobe
        .garments
        .iter()
        .map(|g| g.id.clone())
        .filter(|id| ids.is_none_or(|ids| ids.iter().any(|i| i == id)))
        .collect();
    for id in wanted {
        let reached = crate::mesh::embedded::garment(
            wardrobe,
            copying,
            &mut scratch,
            &id,
            options,
            &mut done,
            watch,
        )?;
        report.models.extend(reached.models);
        report
            .left
            .extend(reached.refused.into_iter().map(|r| Left {
                file: r.file,
                texture: None,
                why: r.why,
            }));
    }
    Ok(())
}

pub(crate) fn join(problems: &[or_source::Problem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Write `wardrobe` where it differs from `before`, and keep what a step of Undo needs; none where nothing changed.
///
/// ⚠ Nothing is written when nothing changed, so a second run changes no byte. A failure takes the copies back.
pub(crate) fn finish(
    folder: &Path,
    before: Wardrobe,
    wardrobe: &Wardrobe,
    mut copying: Copying,
) -> Result<Option<Revision>, Error> {
    if *wardrobe == before {
        copying.undo();
        return Ok(None);
    }
    if let Err(e) = wardrobe.write(folder) {
        copying.undo();
        return Err(e.into());
    }
    Ok(Some(Revision {
        before,
        after: Wardrobe::read(folder)?,
        copied: copying.copied,
        made: copying.made,
    }))
}

/// Optimise the textures of the garments `ids` (every garment where none) of the pack in `folder`, with the Undo step that takes it back; none where nothing changed.
///
/// ⚠ Stopped by `watch`, the pack and its folder are as they were.
pub fn optimise(
    folder: &Path,
    ids: Option<&[String]>,
    options: &Options,
    watch: &Watch,
) -> Result<(Report, Option<Revision>), Error> {
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    let mut copying = Copying::new(folder);
    let report =
        run(&mut wardrobe, &mut copying, ids, options, false, watch).and_then(|mut report| {
            run_embedded(
                &mut wardrobe,
                &mut copying,
                ids,
                options,
                watch,
                &mut report,
            )?;
            Ok(report)
        });
    let report = match report {
        Ok(report) => report,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    let revision = finish(folder, before, &wardrobe, copying)?;
    Ok((report, revision))
}
