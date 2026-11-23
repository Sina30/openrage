//! Fix all safe, the repair of a model and the texture optimiser, as the Wardrobe runs them: off the UI thread, each one step of
//! Undo, with the report in the Log.
//!
//! ⚠ **Nothing here decides what is safe or what a texture comes to.** `or-wardrobe` does, and this asks for it, shows how far it
//! has got, stops it when asked and carries what it said back to the screen.

use or_source::optimise::{Options, Scale};
use or_wardrobe::edit::{self, Watch};
use or_wardrobe::{fix, textures};

use super::wardrobe::Done;
use super::wardrobe_words::with_moves;
use super::Question;
use crate::state::AppState;

/// How far the optimiser may cut the size of a picture, as the window offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    Keep,
    Half,
    Quarter,
    /// No side is longer than this many pixels.
    Side(u32),
}

impl Cut {
    pub const CHOICES: [Cut; 6] = [
        Cut::Keep,
        Cut::Half,
        Cut::Quarter,
        Cut::Side(2048),
        Cut::Side(1024),
        Cut::Side(512),
    ];

    pub fn label(self) -> String {
        match self {
            Self::Keep => tr!("Keep their size").to_string(),
            Self::Half => tr!("Half the size").to_string(),
            Self::Quarter => tr!("A quarter of the size").to_string(),
            Self::Side(n) => tr!("At most {n} pixels on a side", n = n),
        }
    }

    fn scale(self) -> Scale {
        match self {
            Self::Keep => Scale::Keep,
            Self::Half => Scale::Ratio(0.5),
            Self::Quarter => Scale::Ratio(0.25),
            Self::Side(n) => Scale::Side(n),
        }
    }
}

/// What the optimiser window is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Draft {
    /// Uncompressed textures, and BC7 where the pack is built for Legacy, become BC1 or BC3 by their alpha.
    pub format: bool,
    /// A texture that stores a single mip is given its chain.
    pub mips: bool,
    /// A side that is not a power of two goes to the nearest.
    pub power_of_two: bool,
    pub cut: Cut,
}

impl Default for Draft {
    /// What Fix all safe does: no picture is made smaller on purpose.
    fn default() -> Self {
        Self {
            format: true,
            mips: true,
            power_of_two: true,
            cut: Cut::Keep,
        }
    }
}

impl Draft {
    /// Whether it asks for anything at all.
    pub fn asks(&self) -> bool {
        self.format || self.mips || self.power_of_two || self.cut != Cut::Keep
    }

    fn options(&self, safe: Options) -> Options {
        Options {
            format: self.format,
            mips: self.mips,
            power_of_two: self.power_of_two,
            scale: self.cut.scale(),
            ..safe
        }
    }
}

impl AppState {
    /// Run Fix all safe over the pack shown: repair, textures, order and mask, and the levels of detail when the person ticked them, as one step of Undo with the report of each step in the Log.
    pub fn fix_all_safe(&mut self) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let desk = &self.workbench.wardrobe.mesh;
        let also = fix::Also {
            levels: desk
                .fix_levels
                .then(|| super::wardrobe_mesh::levels_of(desk)),
        };
        let watch = Watch::default();
        let work = watch.clone();
        self.wardrobe_job_watched(
            tr!("Fix all safe"),
            &module,
            watch,
            Box::new(move || match fix::all_safe_also(&folder, &work, &also) {
                Ok((report, revision)) => {
                    let said = match &revision {
                        Some(r) => with_moves(report.sentence(), &edit::moved(&r.before, &r.after)),
                        None => report.sentence(),
                    };
                    Done::revised(
                        said,
                        (tr!("fix all"), tr!("Ran Fix all safe").to_string()),
                        revision,
                    )
                    .with_notes(report.lines())
                }
                Err(why) => Done::said(Err(why.to_string())),
            }),
        );
    }

    /// Repair the model of garment `garment` of the content `module` where its counts contradict their runs, losslessly: Undo takes it back.
    pub fn repair_model(&mut self, module: String, garment: String) {
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let name = self.garment_name(&module, &garment);
        self.wardrobe_job(
            tr!("Repairing a model"),
            &module.clone(),
            Box::new(move || match fix::repair_garment(&folder, &garment) {
                Ok((report, _)) if report.changed.is_empty() => Done::said(Ok(tr!(
                    "Nothing to repair in the model of {name}: it states no count against its run",
                    name = name
                ))),
                Ok((report, revision)) => Done::revised(
                    tr!("Repaired the model of {name}", name = name),
                    ("repair", tr!("Repaired the model of {name}", name = name)),
                    revision,
                )
                .with_notes(report.changed),
                Err(why) => Done::said(Err(why.to_string())),
            }),
        );
    }

    /// Ask what the optimiser is to do to the textures of the garments picked, or of the whole pack where none is.
    pub fn ask_to_optimise(&mut self) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let picked = self.picked_garments();
        let garments = (!picked.is_empty()).then_some(picked);
        self.workbench.asking = Some(Question::Optimise {
            module,
            garments,
            draft: Draft::default(),
        });
    }

    /// Optimise the textures of `garments` (the whole pack where none) of the content `module` as `draft` says: the garments name lighter copies of their sources, and Undo takes it back.
    pub fn optimise_textures(
        &mut self,
        module: String,
        garments: Option<Vec<String>>,
        draft: Draft,
    ) {
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let safe = (self.workbench.packs().iter())
            .find(|p| p.module == module)
            .and_then(|p| p.read.as_ref().ok())
            .map(|read| textures::safe_options(&read.wardrobe));
        let Some(safe) = safe else { return };
        let options = draft.options(safe);
        let watch = Watch::default();
        let work = watch.clone();
        self.wardrobe_job_watched(
            tr!("Optimising textures"),
            &module,
            watch,
            Box::new(move || {
                match textures::optimise(&folder, garments.as_deref(), &options, &work) {
                    Ok((report, revision)) => {
                        let n = report.textures();
                        let said = match n {
                            0 => tr!("No texture needed anything the optimiser was asked to do").to_string(),
                            n => tr!("Optimised {n} texture(s); the garments name lighter copies and the sources they were are untouched", n = n),
                        };
                        Done::revised(said, ("optimise", tr!("Optimised {n} texture(s)", n = n)), revision)
                            .with_notes(report.lines())
                    }
                    Err(why) => Done::said(Err(why.to_string())),
                }
            }),
        );
    }
}
