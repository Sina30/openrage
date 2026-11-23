//! The mesh doctor as the Wardrobe runs it: a look at a garment's models, and levels of detail, a repaired rig, cleaned weights and a mirror, each one step of Undo.
//!
//! ⚠ **Nothing here decides what a mesh needs.** `or-wardrobe` looks and changes, and this asks for it off the UI thread, hands it
//! the ped's skeleton once the install is read, and carries what it said back to the Log and the panel. The model a step replaced is
//! kept, so that the viewport can draw it beside the one that took its place.

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

use or_interchange::Bone;
use or_wardrobe::edit::Watch;
use or_wardrobe::mesh::{self, Examination, Op};
use or_wardrobe::Ped;

use super::wardrobe::Done;
use crate::state::AppState;

/// What a look at the models of a garment found, or why there was none.
pub struct Looked {
    pub garment: String,
    pub result: Result<Examination, String>,
}

/// The model a step of the doctor replaced and the one that took its place, so the viewport can show either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Before {
    pub garment: String,
    /// The source the garment named before the step.
    pub was: String,
    /// The source it names now.
    pub now: String,
    /// The viewport draws `was` in place of `now`.
    pub shown: bool,
}

/// What the mesh doctor keeps between frames.
pub struct MeshDesk {
    /// The share of the high level's triangles the Medium level keeps, in percent.
    pub medium: u8,
    /// The same for the Low level.
    pub low: u8,
    /// Make again a level that the model has.
    pub replace: bool,
    /// Fix all safe also adds the Medium and Low levels above to the models that lack them.
    pub fix_levels: bool,
    /// What the last look at a garment's models found.
    pub looked: Option<Looked>,
    looking: Option<Receiver<Looked>>,
    /// The last step that changed a model.
    pub before: Option<Before>,
    done: (Sender<Before>, Receiver<Before>),
    /// The skeleton of a ped given in place of the install's, which a unit with no install stands up.
    given: Option<(Ped, Vec<Bone>)>,
}

impl Default for MeshDesk {
    fn default() -> Self {
        Self {
            medium: 35,
            low: 8,
            replace: false,
            fix_levels: false,
            looked: None,
            looking: None,
            before: None,
            done: channel(),
            given: None,
        }
    }
}

impl MeshDesk {
    /// The model to draw in place of `model` for `garment`, when the step before is being looked at.
    pub fn drawn_instead(&self, garment: &str, model: &str) -> Option<&str> {
        let before = self.before.as_ref()?;
        (before.shown && before.garment == garment && before.now == model)
            .then_some(before.was.as_str())
    }

    /// Stand up the skeleton of `ped`, for a unit with no install to read it from.
    #[cfg(test)]
    pub fn give_skeleton(&mut self, ped: Ped, bones: Vec<Bone>) {
        self.given = Some((ped, bones));
    }
}

/// The ratios the Levels dialog stands for, as asked: Medium and Low.
pub(super) fn levels_of(desk: &MeshDesk) -> Vec<(usize, f32)> {
    [(1, desk.medium), (2, desk.low)]
        .into_iter()
        .filter(|(_, percent)| *percent > 0)
        .map(|(level, percent)| (level, f32::from(percent) / 100.0))
        .collect()
}

impl AppState {
    /// The bones of the freemode ped `ped`, as a skin without a skeleton of its own is bound to them.
    fn mesh_bones(&self, ped: Ped) -> Option<Vec<Bone>> {
        if let Some((given, bones)) = &self.workbench.wardrobe.mesh.given {
            return (*given == ped).then(|| bones.clone());
        }
        self.workbench
            .wardrobe
            .dressed
            .bones(ped)
            .map(<[Bone]>::to_vec)
    }

    /// The ped the garment `id` of `module` dresses.
    fn mesh_ped(&self, module: &str, id: &str) -> Option<Ped> {
        (self.workbench.packs().iter())
            .find(|p| p.module == module)
            .and_then(|p| p.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garment(id))
            .and_then(|g| g.ped)
    }

    /// Look at the models of garment `id` of the content `module`: their levels, rig and weights, a shoe's heel and the texture they use. Nothing is written.
    pub fn check_mesh(&mut self, module: String, id: String) {
        let Some(folder) = self
            .workbench
            .packs()
            .iter()
            .find(|p| p.module == module)
            .map(|p| p.folder.clone())
        else {
            return;
        };
        let bones = self
            .mesh_ped(&module, &id)
            .and_then(|ped| self.mesh_bones(ped));
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let result = mesh::examine(&folder, &id, bones.as_deref()).map_err(|e| e.to_string());
            let _ = tx.send(Looked {
                garment: id,
                result,
            });
        });
        self.workbench.wardrobe.mesh.looking = Some(rx);
        self.workbench.wardrobe.mesh.looked = None;
    }

    /// Add the Medium and Low levels the desk is set to, to the models of garment `id`.
    pub fn add_levels(&mut self, module: String, id: String) {
        let desk = &self.workbench.wardrobe.mesh;
        let op = Op::Levels {
            levels: levels_of(desk),
            replace: desk.replace,
        };
        self.mesh_op(module, id, op);
    }

    /// Run `op` over the models of garment `id` of the content `module`: each becomes a new source the garment names, and Undo takes it back.
    pub fn mesh_op(&mut self, module: String, id: String, op: Op) {
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let name = self.garment_name(&module, &id);
        let bones = self
            .mesh_ped(&module, &id)
            .and_then(|ped| self.mesh_bones(ped));
        let own = (self.workbench.packs().iter())
            .find(|p| p.module == module)
            .and_then(|p| p.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garment(&id))
            .and_then(|g| g.model.clone());
        let tell = self.workbench.wardrobe.mesh.done.0.clone();
        let watch = Watch::default();
        let work = watch.clone();
        let noun = match &op {
            Op::Levels { .. } => "levels",
            Op::Rig => tr!("rig repair"),
            Op::Weights => "weights",
            Op::Mirror => "mirror",
            Op::UvCrop => "crop",
            Op::Embedded(_) => tr!("embedded textures"),
            Op::Materials => "materials",
        };
        let label = op.label();
        self.workbench.wardrobe.mesh.looked = None;
        self.wardrobe_job_watched(
            label,
            &module,
            watch,
            Box::new(move || match mesh::run(&folder, &id, &op, bones.as_deref(), &work) {
                Ok((report, _)) if !report.changed() && !report.refused.is_empty() => {
                    let mut why = (report.refused.iter())
                        .map(|r| format!("{}: {}", r.file, r.why))
                        .collect::<Vec<_>>()
                        .join("; ");
                    // A mirror refused for want of the skeleton says where it comes from.
                    if why.contains("give the ped's skeleton") && bones.is_none() {
                        why.push_str(tr!(" (the freemode skeleton is read from the game folder, which is not set or not read yet)"));
                    }
                    Done::said(Err(why))
                }
                Ok((report, _)) if !report.changed() => Done::said(Ok(report.sentence())),
                Ok((report, revision)) => {
                    // The model the garment named is the one the viewport can show as it was.
                    let shown = report
                        .models
                        .iter()
                        .find(|m| Some(m.file.as_str()) == own.as_deref())
                        .or(report.models.first());
                    if let Some(done) = shown {
                        let _ = tell.send(Before {
                            garment: id.clone(),
                            was: done.file.clone(),
                            now: done.into.clone(),
                            shown: false,
                        });
                    }
                    let said = report.sentence();
                    Done::revised(said, (noun, format!("{label} of {name}")), revision).with_notes(report.lines())
                }
                Err(why) => Done::said(Err(why.to_string())),
            }),
        );
    }

    /// Take what the worker sent: a look at the models, and the model a step replaced.
    pub(super) fn poll_mesh(&mut self) {
        let desk = &mut self.workbench.wardrobe.mesh;
        if let Some(rx) = &desk.looking {
            match rx.try_recv() {
                Ok(looked) => {
                    desk.looked = Some(looked);
                    desk.looking = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => desk.looking = None,
            }
        }
        while let Ok(before) = desk.done.1.try_recv() {
            desk.before = Some(before);
        }
    }

    /// Whether a look at the models is in flight, which wants a frame again.
    pub fn is_looking_at_a_mesh(&self) -> bool {
        self.workbench.wardrobe.mesh.looking.is_some()
    }
}
