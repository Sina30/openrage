//! Which whole passes of an effect build: a pass draws only when every stage it binds does, so this is the coverage a renderer sees.

use std::collections::BTreeMap;

use or_io::gta5::shaders::fxc::{GrcEffect, GrcProgramStage};
use serde_json::{json, Value};

use crate::signature::Linkage;

/// What became of one program of an effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Built,
    Failed(String),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ClassTally {
    pub passes: usize,
    pub built: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TechniqueTally {
    pub effects: usize,
    /// Effects where every pass of the technique built.
    pub built: usize,
}

/// Passes whose vertex and pixel stages were built together as one pipeline.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PipelineTally {
    /// Passes attempted: those that bind a vertex stage and at most a pixel stage, every program of which built.
    pub passes: usize,
    pub built: usize,
    /// Distinct pairs of programs those passes make.
    pub distinct: usize,
    /// Why pipelines failed, counted once for each pass that makes one.
    pub causes: BTreeMap<String, usize>,
}

impl PipelineTally {
    pub fn add(&mut self, result: &Result<(), String>) {
        self.passes += 1;
        match result {
            Ok(()) => self.built += 1,
            Err(cause) => *self.causes.entry(cause.clone()).or_default() += 1,
        }
    }
}

/// How the pixel inputs of the distinct vertex-and-pixel pairs relate to the vertex outputs, by semantic.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LinkageTally {
    pub pairs: BTreeMap<Linkage, usize>,
}

impl LinkageTally {
    pub fn total(&self) -> usize {
        self.pairs.values().sum()
    }
}

#[derive(Debug, Default)]
pub struct PassTally {
    pub effects: usize,
    /// Effects whose `.fxc` would not parse.
    pub unreadable: usize,
    pub programs: usize,
    pub programs_built: usize,
    /// Passes by the stages they bind.
    pub classes: BTreeMap<String, ClassTally>,
    pub techniques: BTreeMap<String, TechniqueTally>,
    /// Why programs in passes failed, counted once for each pass that uses one.
    pub causes: BTreeMap<String, usize>,
    /// Pipelines built from the passes whose programs all built, when a device was offered.
    pub pipelines: Option<PipelineTally>,
    /// The vertex-and-pixel pairs those passes make, by how their signatures meet.
    pub linkage: LinkageTally,
}

fn short(stage: GrcProgramStage) -> &'static str {
    match stage {
        GrcProgramStage::Vertex => "VS",
        GrcProgramStage::Pixel => "PS",
        GrcProgramStage::Compute => "CS",
        GrcProgramStage::Domain => "DS",
        GrcProgramStage::Geometry => "GS",
        GrcProgramStage::Hull => "HS",
    }
}

/// The index in `effect.programs` of the program a pass names by the one-based `position` among the programs of `stage`.
pub(crate) fn program_index(
    effect: &GrcEffect,
    stage: GrcProgramStage,
    position: usize,
) -> Option<usize> {
    effect
        .programs
        .iter()
        .enumerate()
        .filter(|(_, p)| p.stage == stage)
        .nth(position.checked_sub(1)?)
        .map(|(index, _)| index)
}

impl PassTally {
    /// Fold in one effect, whose programs `outcome` judges by their index in [`GrcEffect::programs`].
    pub fn add_effect(&mut self, effect: &GrcEffect, outcome: &dyn Fn(usize) -> Option<Outcome>) {
        self.effects += 1;
        for index in 0..effect.programs.len() {
            self.programs += 1;
            self.programs_built += usize::from(outcome(index) == Some(Outcome::Built));
        }
        for technique in &effect.techniques {
            let mut every_pass_built = true;
            for pass in &technique.passes {
                let mut stages: Vec<&str> = Vec::new();
                let mut built = true;
                for (slot, stage) in GrcProgramStage::ALL.iter().enumerate() {
                    // The pass names its program by a one-based position among that stage's programs.
                    let position = pass.programs[slot] as usize;
                    if position == 0 {
                        continue;
                    }
                    stages.push(short(*stage));
                    let failure = match program_index(effect, *stage, position) {
                        None => Some("a pass names a program the effect lacks".to_string()),
                        Some(index) => match outcome(index) {
                            Some(Outcome::Built) => None,
                            Some(Outcome::Failed(cause)) => Some(cause),
                            None => Some("a program with no outcome".to_string()),
                        },
                    };
                    if let Some(cause) = failure {
                        built = false;
                        *self.causes.entry(cause).or_default() += 1;
                    }
                }
                let class = if stages.is_empty() {
                    "(binds nothing)".to_string()
                } else {
                    stages.join("+")
                };
                let tally = self.classes.entry(class).or_default();
                tally.passes += 1;
                tally.built += usize::from(built);
                every_pass_built &= built;
            }
            let tally = self.techniques.entry(technique.name.clone()).or_default();
            tally.effects += 1;
            tally.built += usize::from(every_pass_built);
        }
    }

    pub fn passes(&self) -> usize {
        self.classes.values().map(|c| c.passes).sum()
    }

    pub fn report(&self) -> String {
        let mut out = format!(
            "{} effects ({} unreadable), {} programs, {} built\n\npasses: {}\n  stages                        passes   built\n",
            self.effects,
            self.unreadable,
            self.programs,
            self.programs_built,
            self.passes(),
        );
        for (class, c) in &self.classes {
            out.push_str(&format!(
                "  {class:<28} {:>7} {:>7}  {:.1}%\n",
                c.passes,
                c.built,
                100.0 * c.built as f64 / c.passes.max(1) as f64
            ));
        }
        if let Some(p) = &self.pipelines {
            out.push_str(&format!(
                "\npipelines, a pass's vertex and pixel stages built together on the device: {} of {} passes, from {} distinct pairs\n",
                p.built, p.passes, p.distinct
            ));
            let mut causes: Vec<_> = p.causes.iter().collect();
            causes.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            for (cause, n) in causes.into_iter().take(12) {
                out.push_str(&format!("  {n:>6}  {cause}\n"));
            }
        }
        if self.linkage.total() > 0 {
            out.push_str(&format!(
                "\nthe {} distinct vertex-and-pixel pairs, by how the pixel inputs meet the vertex outputs:\n",
                self.linkage.total()
            ));
            for (linkage, n) in &self.linkage.pairs {
                out.push_str(&format!("  {n:>6}  {linkage:?}\n"));
            }
        }
        out.push_str(
            "\ntechniques used by the most effects (effects, effects where every pass built):\n",
        );
        let mut techniques: Vec<_> = self.techniques.iter().collect();
        techniques.sort_by(|a, b| b.1.effects.cmp(&a.1.effects).then(a.0.cmp(b.0)));
        for (name, t) in techniques.into_iter().take(25) {
            out.push_str(&format!(
                "  {name:<34} {:>4} {:>4}  {:.1}%\n",
                t.effects,
                t.built,
                100.0 * t.built as f64 / t.effects.max(1) as f64
            ));
        }
        out.push_str("\nwhat the passes that did not build stop at:\n");
        let mut causes: Vec<_> = self.causes.iter().collect();
        causes.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (cause, n) in causes.into_iter().take(12) {
            out.push_str(&format!("  {n:>6}  {cause}\n"));
        }
        out
    }

    pub fn to_json(&self) -> Value {
        json!({
            "effects": self.effects,
            "unreadable": self.unreadable,
            "programs": self.programs,
            "programs_built": self.programs_built,
            "classes": self.classes.iter().map(|(c, t)| json!({
                "stages": c, "passes": t.passes, "built": t.built,
            })).collect::<Vec<_>>(),
            "techniques": self.techniques.iter().map(|(n, t)| json!({
                "name": n, "effects": t.effects, "built": t.built,
            })).collect::<Vec<_>>(),
            "causes": self.causes,
            "linkage": self.linkage.pairs.iter().map(|(l, n)| (format!("{l:?}"), n)).collect::<BTreeMap<_, _>>(),
            "pipelines": self.pipelines.as_ref().map(|p| json!({
                "passes": p.passes, "built": p.built, "distinct": p.distinct, "causes": p.causes,
            })),
        })
    }
}

#[cfg(test)]
mod tests;
