//! How many of an install's drawables a pack draws, and what stops the rest.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use or_renderer::game_shaders::frame::Rendered;
use or_renderer::game_shaders::mesh::MeshError;
use or_renderer::game_shaders::pack::GamePack;
use or_renderer::viewport::ViewportRenderer;
use serde_json::{json, Value};

use super::{render_drawable, render_drawable_shaded, Drawn, Install, ShadedOptions};
use crate::spirv_check::fold;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EffectTally {
    pub surfaces: usize,
    pub drawn: usize,
}

#[derive(Debug, Default)]
pub struct Census {
    /// Drawables the install holds with a wanted extension.
    pub available: usize,
    pub tried: usize,
    /// Drawables that would not read or draw at all, by cause.
    pub unreadable: BTreeMap<String, usize>,
    /// Drawables with a surface drawn and a pixel touched.
    pub shown: usize,
    /// Drawables with a surface drawn and no pixel touched.
    pub blank: usize,
    /// What the blank drawables were drawn with: the effects of each, once apiece and sorted, joined.
    pub blank_by_effects: BTreeMap<String, usize>,
    /// Drawables of which no surface drew.
    pub nothing_drawn: usize,
    pub surfaces: usize,
    pub surfaces_drawn: usize,
    /// Why surfaces were left out, counted for each surface.
    pub left_out: BTreeMap<String, usize>,
    pub effects: BTreeMap<String, EffectTally>,
    /// The first few paths of each kind that did not show, to look at.
    pub examples: BTreeMap<&'static str, Vec<String>>,
    /// What each drawable drew, kept when a contact sheet is asked for.
    pub pictures: Vec<(String, Rendered)>,
}

/// How many paths of each kind are kept.
const EXAMPLES: usize = 6;

fn cause_of(error: &MeshError) -> String {
    match error {
        MeshError::NoEffect(_) => "the pack has no such effect".into(),
        MeshError::NoPass(_) => "the effect has no pass this draws".into(),
        MeshError::Pack(m) | MeshError::Pipeline(m) | MeshError::Device(m) => fold(m),
    }
}

/// The kind of example a surface left out for `error` is kept under.
fn left_out_kind(error: &MeshError) -> &'static str {
    match error {
        MeshError::NoEffect(_) => "a surface left out, for want of its effect",
        MeshError::NoPass(_) => "a surface left out, for want of a pass",
        MeshError::Pack(_) | MeshError::Pipeline(_) | MeshError::Device(_) => {
            "a surface left out, by a failure"
        }
    }
}

impl Census {
    fn example(&mut self, kind: &'static str, path: &str) {
        let kept = self.examples.entry(kind).or_default();
        if kept.len() < EXAMPLES {
            kept.push(path.to_string());
        }
    }

    /// Fold in the drawable at `path`: what it drew, or why nothing could be.
    pub fn add(&mut self, path: &str, outcome: Result<&Drawn, &str>) {
        self.tried += 1;
        let drawn = match outcome {
            Ok(drawn) => drawn,
            Err(cause) => {
                *self.unreadable.entry(fold(cause)).or_default() += 1;
                self.example("unreadable", path);
                return;
            }
        };
        self.surfaces += drawn.effects.len();
        self.surfaces_drawn += drawn.render.drawn;
        // Each surface counts as drawn for its effect until it is found among those left out
        for effect in &drawn.effects {
            let tally = self.effects.entry(effect.clone()).or_default();
            tally.surfaces += 1;
            tally.drawn += 1;
        }
        for (effect, error) in &drawn.render.skipped {
            *self.left_out.entry(cause_of(error)).or_default() += 1;
            let named = if effect.is_empty() {
                "no material"
            } else {
                effect
            };
            self.example(left_out_kind(error), &format!("{path} ({named})"));
            if let Some(tally) = self.effects.get_mut(effect) {
                tally.drawn = tally.drawn.saturating_sub(1);
            }
        }
        match (drawn.render.drawn, drawn.render.rendered.touched()) {
            (0, _) => {
                self.nothing_drawn += 1;
                self.example("no surface drawn", path);
            }
            (_, 0) => {
                self.blank += 1;
                self.example("drawn blank", path);
                let mut effects: Vec<&str> = drawn
                    .effects
                    .iter()
                    .map(|e| if e.is_empty() { "no material" } else { e })
                    .collect();
                effects.sort_unstable();
                effects.dedup();
                *self.blank_by_effects.entry(effects.join(", ")).or_default() += 1;
            }
            _ => self.shown += 1,
        }
    }

    pub fn report(&self) -> String {
        let percent = |part: usize, whole: usize| 100.0 * part as f64 / whole.max(1) as f64;
        let mut out = format!(
            "{} drawables of {}: {} shown ({:.1}%), {} drawn blank, {} with no surface drawn, {} unreadable\n\
             surfaces: {} drawn of {} ({:.1}%)\n",
            self.tried,
            self.available,
            self.shown,
            percent(self.shown, self.tried),
            self.blank,
            self.nothing_drawn,
            self.unreadable.values().sum::<usize>(),
            self.surfaces_drawn,
            self.surfaces,
            percent(self.surfaces_drawn, self.surfaces),
        );
        let mut list = |title: &str, causes: &BTreeMap<String, usize>| {
            out.push_str(title);
            let mut causes: Vec<_> = causes.iter().collect();
            causes.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            for (cause, n) in causes.into_iter().take(12) {
                out.push_str(&format!("  {n:>6}  {cause}\n"));
            }
        };
        list("\nwhy drawables were not drawn:\n", &self.unreadable);
        list("\nwhy surfaces were left out:\n", &self.left_out);
        list(
            "\nwhat the blank drawables were drawn with:\n",
            &self.blank_by_effects,
        );
        for (kind, paths) in &self.examples {
            out.push_str(&format!("\n{kind}, for example:\n"));
            for path in paths {
                out.push_str(&format!("  {path}\n"));
            }
        }
        out.push_str("\neffects drawn most (surfaces, drawn):\n");
        let mut effects: Vec<_> = self.effects.iter().collect();
        effects.sort_by(|a, b| b.1.surfaces.cmp(&a.1.surfaces).then(a.0.cmp(b.0)));
        for (name, tally) in effects.into_iter().take(15) {
            let name = if name.is_empty() {
                "(no material)"
            } else {
                name
            };
            out.push_str(&format!(
                "  {name:<34} {:>6} {:>6}  {:.1}%\n",
                tally.surfaces,
                tally.drawn,
                percent(tally.drawn, tally.surfaces)
            ));
        }
        out
    }

    pub fn to_json(&self) -> Value {
        json!({
            "available": self.available,
            "tried": self.tried,
            "shown": self.shown,
            "blank": self.blank,
            "nothing_drawn": self.nothing_drawn,
            "unreadable": self.unreadable,
            "surfaces": self.surfaces,
            "surfaces_drawn": self.surfaces_drawn,
            "left_out": self.left_out,
            "blank_by_effects": self.blank_by_effects,
            "examples": self.examples,
            "effects": self.effects.iter().map(|(n, t)| json!({
                "name": n, "surfaces": t.surfaces, "drawn": t.drawn,
            })).collect::<Vec<_>>(),
        })
    }
}

/// `limit` of `paths`, evenly spaced from the first, or all of them where there are no more than that.
pub fn sample(paths: &[String], limit: usize) -> Vec<&String> {
    if limit == 0 || paths.len() <= limit {
        return paths.iter().collect();
    }
    (0..limit)
        .map(|i| &paths[i * paths.len() / limit])
        .collect()
}

/// Draw `limit` of the install's drawables with `extensions`, each into a picture of `size` pixels, and tally what they drew.
#[allow(clippy::too_many_arguments)]
pub fn census(
    install: &Install,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    extensions: &[&str],
    limit: usize,
    size: u32,
    shaded: bool,
    keep: bool,
    progress: &dyn Fn(usize, usize),
) -> Census {
    let paths = install.paths_with(extensions);
    let mut tally = Census {
        available: paths.len(),
        ..Census::default()
    };
    let chosen = sample(&paths, limit);
    // One viewport for the whole census when it draws as Shaded does: its passes are built once
    let mut shaded = shaded.then(|| {
        ViewportRenderer::new(
            device,
            queue,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            size,
            size,
        )
    });
    for (done, path) in chosen.iter().enumerate() {
        progress(done, chosen.len());
        let result = install.read(path).and_then(|found| {
            catch_unwind(AssertUnwindSafe(|| match shaded.as_mut() {
                Some(renderer) => render_drawable_shaded(
                    device,
                    queue,
                    pack,
                    renderer,
                    &found,
                    size,
                    true,
                    &ShadedOptions::default(),
                ),
                None => render_drawable(device, queue, pack, &found, size),
            }))
            .unwrap_or_else(|_| Err("building a pipeline panicked".into()))
        });
        if let (true, Ok(drawn)) = (keep, &result) {
            tally
                .pictures
                .push((path.to_string(), drawn.render.rendered.clone()));
        }
        tally.add(path, result.as_ref().map_err(String::as_str));
    }
    tally
}

#[cfg(test)]
mod tests;
