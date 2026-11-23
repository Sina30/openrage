//! A texture budget for a pack: a size and a format for each texture that fit a memory target, shown as a plan before anything is written.
//!
//! [`plan`] is the dry run: it measures the textures of the garments in scope, chooses the cheapest picture to give up across all
//! of them, and returns what each texture would become with the PSNR of the choice. It copies and writes nothing. [`accept`] carries
//! a plan out as the optimiser does: a source that changes is copied under a free name, the copy gets the chosen sizes and formats,
//! the garments in scope name the copy, and one Undo takes it back. A texture that is not as the plan measured it is left, with why.
//!
//! What a rung is, how the choice is made and what the bytes are is [`or_source::budget`]'s.

use std::path::Path;

use or_source::budget::{self, Change, Ladder, Measured, Options};
use serde::Serialize;

use crate::edit::{Copying, Error, Revision, Watch};
use crate::model::Wardrobe;
use crate::textures::{finish, join, over_sources, who, Left};

/// One texture a plan changes: its ladder cut to what it is and what it would be.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    /// Rung 0 is the texture as it is and rung 1 what it becomes.
    pub ladder: Ladder,
}

impl Choice {
    pub fn was(&self) -> &budget::Rung {
        &self.ladder.rungs[0]
    }

    pub fn now(&self) -> &budget::Rung {
        &self.ladder.rungs[1]
    }
}

/// The textures of one source a plan changes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Planned {
    pub source: String,
    pub garments: Vec<String>,
    pub choices: Vec<Choice>,
}

/// What a budget would do to a pack.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Plan {
    pub target: u64,
    /// What the textures it can change take as they are.
    pub before: u64,
    /// What they would take once the plan is carried out.
    pub after: u64,
    /// Whether `after` is within the target.
    pub met: bool,
    /// The part of the target left unused when it is met.
    pub slack: f64,
    pub sources: Vec<Planned>,
    pub left: Vec<Left>,
}

impl Plan {
    /// How many textures it changes.
    pub fn textures(&self) -> usize {
        self.sources.iter().map(|s| s.choices.len()).sum()
    }

    /// The plan as lines a person reads: each texture with its choice and the PSNR of it, then what was left and why.
    pub fn lines(&self) -> Vec<String> {
        let mb = |bytes: u64| format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0));
        let mut lines = vec![format!(
            "{} textures take {}; the target is {}, and the plan brings them to {}{}",
            self.sources.iter().map(|s| s.choices.len()).sum::<usize>(),
            mb(self.before),
            mb(self.target),
            mb(self.after),
            if self.met {
                ""
            } else {
                ", which is as low as they go"
            }
        )];
        for source in &self.sources {
            lines.push(format!(
                "{} for {}",
                source.source,
                source.garments.join(", ")
            ));
            for c in &source.choices {
                lines.push(format!(
                    "  {}: {} becomes {}",
                    c.ladder.texture,
                    c.was(),
                    c.now()
                ));
            }
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

/// A source that was copied and cut to a plan, and the garments that name the copy now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Budgeted {
    pub garments: Vec<String>,
    pub from: String,
    pub into: String,
    pub changes: Vec<Change>,
}

/// What carrying a plan out did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Report {
    pub changed: Vec<Budgeted>,
    pub left: Vec<Left>,
}

impl Report {
    /// How many textures were changed.
    pub fn textures(&self) -> usize {
        self.changed.iter().map(|o| o.changes.len()).sum()
    }

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

/// What the textures of the garments `ids` (every garment where none) of the pack in `folder` could be, a plan that fits `target` bytes; nothing is copied or written.
///
/// `stop` is asked between textures. The target is for the textures the budget can change, which are those that are a DDS in a source.
pub fn plan(
    folder: &Path,
    ids: Option<&[String]>,
    target: u64,
    options: &Options,
    watch: &Watch,
) -> Result<Plan, Error> {
    let mut wardrobe = Wardrobe::read(folder)?;
    let mut copying = Copying::new(folder);
    let mut measured: Vec<(String, Vec<String>, Measured)> = Vec::new();
    let mut left = Vec::new();
    let stop = || watch.cancel.is_set();
    over_sources(
        &mut wardrobe,
        &mut copying,
        ids,
        watch,
        &mut left,
        |wardrobe, _, rel, left| {
            let path = folder.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            match budget::measure(&path, options, &stop) {
                Ok(m) => {
                    left.extend(m.left.iter().map(|l| Left {
                        file: rel.to_string(),
                        texture: Some(l.texture.clone()),
                        why: l.why.clone(),
                    }));
                    measured.push((rel.to_string(), who(wardrobe, ids, rel), m));
                }
                Err(problems) => {
                    watch.check()?;
                    left.push(Left {
                        file: rel.to_string(),
                        texture: None,
                        why: join(&problems),
                    });
                }
            }
            Ok(None)
        },
    )?;
    let sets: Vec<&Measured> = measured.iter().map(|(_, _, m)| m).collect();
    let chosen = budget::choose(&sets, target);
    let mut sources = Vec::new();
    for ((rel, garments, m), picks) in measured.iter().zip(&chosen.picks) {
        let choices: Vec<Choice> = m
            .ladders
            .iter()
            .zip(picks)
            .filter(|(_, pick)| **pick > 0)
            .map(|(ladder, pick)| Choice {
                ladder: Ladder {
                    texture: ladder.texture.clone(),
                    file: ladder.file.clone(),
                    rungs: vec![ladder.rungs[0].clone(), ladder.rungs[*pick].clone()],
                },
            })
            .collect();
        if !choices.is_empty() {
            sources.push(Planned {
                source: rel.clone(),
                garments: garments.clone(),
                choices,
            });
        }
    }
    Ok(Plan {
        target,
        before: chosen.before,
        after: chosen.after,
        met: chosen.met,
        slack: chosen.slack(),
        sources,
        left,
    })
}

/// Carry `plan` out on the garments `ids` (every garment where none) of the pack in `folder`, with the Undo step that takes it back; none where nothing changed.
///
/// ⚠ Stopped by `watch`, the pack and its folder are as they were.
pub fn accept(
    folder: &Path,
    ids: Option<&[String]>,
    plan: &Plan,
    options: &Options,
    watch: &Watch,
) -> Result<(Report, Option<Revision>), Error> {
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    let mut copying = Copying::new(folder);
    let mut report = Report::default();
    let mut changed = Vec::new();
    let mut left = Vec::new();
    let result = over_sources(
        &mut wardrobe,
        &mut copying,
        ids,
        watch,
        &mut left,
        |wardrobe, copying, rel, left| {
            let Some(planned) = plan.sources.iter().find(|s| s.source == rel) else {
                return Ok(None);
            };
            let one = |why: String| Left {
                file: rel.to_string(),
                texture: None,
                why,
            };
            let mark = copying.mark();
            let into = match copying.duplicate_source(rel) {
                Ok(into) => into,
                Err(e) => {
                    copying.rollback_to(mark);
                    left.push(one(format!("it could not be copied: {e}")));
                    return Ok(None);
                }
            };
            let measured = Measured {
                ladders: planned.choices.iter().map(|c| c.ladder.clone()).collect(),
                left: Vec::new(),
            };
            let picks = vec![1; measured.ladders.len()];
            let stop = || watch.cancel.is_set();
            match budget::apply(
                &copying.folder.join(&into),
                &measured,
                &picks,
                options,
                &stop,
            ) {
                Ok(done) => {
                    changed.push(Budgeted {
                        garments: who(wardrobe, ids, rel),
                        from: rel.to_string(),
                        into: into.clone(),
                        changes: done,
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
    );
    if let Err(e) = result {
        copying.undo();
        return Err(e);
    }
    report.changed = changed;
    report.left = left;
    let revision = finish(folder, before, &wardrobe, copying)?;
    Ok((report, revision))
}
