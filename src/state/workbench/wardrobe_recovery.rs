//! What makes a Wardrobe edit recoverable: Redo, the one Undo of the workspace, the right to write a pack, a pack that does not read and the pack's earlier versions.
//!
//! ⚠ **Nothing here is a rule or a write of its own.** `or-wardrobe` redoes, locks and keeps versions; this
//! asks for them, runs them on the Wardrobe's worker one after the other, and carries what they said back to the screens.

use std::path::{Path, PathBuf};
use std::time::Duration;

use or_wardrobe::backup::{self, Version};
use or_wardrobe::Wardrobe;

use super::wardrobe::{Done, Moved};
use crate::state::AppState;

/// How long a content another window holds waits before this window asks for it again.
const RETRY_AFTER: Duration = Duration::from_secs(2);

/// Which of the two stacks of Undo a move is on: the Wardrobe's own changes, or the steps of the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Wardrobe,
    Project,
}

/// The earlier versions of the pack shown, newest first, each with the pack it held.
///
/// ponytail: every kept version is read and parsed on the UI thread, once each time the pack changes while the list is
/// open. The most a pack keeps is `backup::KEPT`; a worker if a pack's file ever makes that a hitch.
#[derive(Debug, Clone, PartialEq)]
pub struct Versions {
    pub module: String,
    /// The generation of the scan they were read at.
    pub generation: u64,
    pub list: Vec<(Version, Wardrobe)>,
}

impl AppState {
    /// Take the right to write the pack of `module` in `folder`, or say why this window may not.
    ///
    /// ⚠ The first time in a session also deletes what an earlier session left for a Redo that can no longer come.
    pub(super) fn hold_pack(&mut self, module: &str, folder: &Path) -> Result<(), String> {
        let work = &mut self.workbench.wardrobe;
        if work.locks.contains_key(folder) {
            return Ok(());
        }
        match or_wardrobe::lock::acquire(folder) {
            Ok(lock) => {
                or_wardrobe::purge_held(folder);
                work.locks.insert(folder.to_path_buf(), lock);
                work.locked_out.remove(module);
                Ok(())
            }
            Err(_) => {
                work.locked_out.insert(module.to_string());
                Err(tr!("Another OpenRage window is editing this pack, so it is read-only here: close it there, or open this project from that window").to_string())
            }
        }
    }

    /// Whether the clothing content shown is held by another window, which writes it while this one reads.
    pub fn wardrobe_locked_out(&self) -> bool {
        (self.wardrobe_module_id())
            .is_some_and(|module| self.workbench.wardrobe.locked_out.contains(&module))
    }

    /// Ask again which packs another window holds, now and then, so a window that opens a project another has open reads it at once, and one that closes hands its packs over without a click.
    ///
    /// ⚠ A pack this window holds is not asked about, and a pack whose lock file was never made is not asked about either: asking makes no lock file in it.
    pub(super) fn watch_locks(&mut self) {
        let work = &mut self.workbench.wardrobe;
        if work.last_retry.is_some_and(|at| at.elapsed() < RETRY_AFTER) {
            return;
        }
        work.last_retry = Some(std::time::Instant::now());
        let asked: Vec<(String, PathBuf)> = (self.workbench.packs().iter())
            .filter(|pack| {
                !pack.linked && !self.workbench.wardrobe.locks.contains_key(&pack.folder)
            })
            .map(|pack| (pack.module.clone(), pack.folder.clone()))
            .collect();
        for (module, folder) in asked {
            let work = &mut self.workbench.wardrobe;
            match or_wardrobe::lock::held_elsewhere(&folder) {
                true => work.locked_out.insert(module),
                false => work.locked_out.remove(&module),
            };
        }
    }

    /// Ask for the pack shown again, for a window that read it while another held it.
    pub fn try_pack_lock_again(&mut self) {
        self.workbench.wardrobe.last_retry = None;
        self.watch_locks();
        if self.wardrobe_locked_out() {
            self.show_info_toast(tr!("The pack is still held by another OpenRage window"));
        }
    }

    /// Move a change between the stacks, as the worker that took it back, made it again or was refused says.
    pub(super) fn take_moved(&mut self, moved: Moved) {
        let work = &mut self.workbench.wardrobe;
        match moved {
            Moved::Undone(undone) => {
                work.trail.push(Side::Wardrobe);
                work.redo.push(undone);
            }
            Moved::RedoRefused(undone) => work.redo.push(undone),
            Moved::Redone(undone) => {
                // Steps are numbered in the order they were made, so the number says where each goes back.
                for step in undone.steps {
                    let at = work.undo.partition_point(|s| s.number < step.number);
                    work.undo.insert(at, step);
                }
            }
        }
    }

    /// A new change ends the chance of a Redo, and what was kept for one is deleted.
    pub(super) fn end_redo(&mut self) {
        let work = &mut self.workbench.wardrobe;
        for undone in work.redo.drain(..) {
            or_wardrobe::purge_held(&undone.folder);
        }
        work.trail.retain(|side| *side != Side::Wardrobe);
    }

    /// What the button of the header would make again.
    pub fn redo_label(&self) -> Option<&str> {
        let undone = self.workbench.wardrobe.redo.last()?;
        undone.steps.first().map(|step| step.label.as_str())
    }

    /// Make again the change that was taken back last.
    pub fn redo_last_change(&mut self) {
        let Some(undone) = self.workbench.wardrobe.redo.pop() else {
            return;
        };
        let Some(folder) = self.folder_to_edit(&undone.module) else {
            self.workbench.wardrobe.redo.push(undone);
            return;
        };
        let label = (undone.steps.first())
            .map(|step| step.label.clone())
            .unwrap_or_default();
        let module = undone.module.clone();
        self.wardrobe_job(
            tr!("Making a change again"),
            &module,
            Box::new(move || match or_wardrobe::redo(&folder, &undone.redo) {
                Ok(()) => {
                    let mut done = Done::said(Ok(tr!("Made again: {label}", label = label)));
                    done.moved = Some(Moved::Redone(undone));
                    done
                }
                Err(why) => {
                    let mut done = Done::said(Err(why.to_string()));
                    done.moved = Some(Moved::RedoRefused(undone));
                    done
                }
            }),
        );
    }

    /// The side of the one Undo of the Wardrobe workspace: the newer of the Wardrobe's last change and the project's last step.
    pub fn joined_undo_side(&self) -> Option<Side> {
        let wardrobe = self.workbench.wardrobe.undo.last().map(|step| step.at);
        match (wardrobe, self.project_can_undo()) {
            (None, false) => None,
            (Some(_), false) => Some(Side::Wardrobe),
            (None, true) => Some(Side::Project),
            (Some(w), true) => match self.workbench.step_here_at() {
                Some(p) if p > w => Some(Side::Project),
                _ => Some(Side::Wardrobe),
            },
        }
    }

    /// What the one Undo of the Wardrobe workspace would take back.
    pub fn joined_undo_label(&self) -> Option<String> {
        match self.joined_undo_side()? {
            Side::Wardrobe => self.workbench.wardrobe.undo_label().map(String::from),
            Side::Project => self.project_undo_label(),
        }
    }

    /// Take back the newer of the Wardrobe's last change and the project's last step.
    pub fn undo_joined(&mut self) {
        match self.joined_undo_side() {
            Some(Side::Wardrobe) => self.undo_last_change(),
            Some(Side::Project) => match self.undo_project() {
                Ok(label) => self.show_toast(tr!("Undid '{label}'", label = label)),
                Err(why) => self.show_warning_toast(why),
            },
            None => {}
        }
    }

    fn can_redo_on(&self, side: Side) -> bool {
        match side {
            Side::Wardrobe => !self.workbench.wardrobe.redo.is_empty(),
            Side::Project => self.project_can_redo(),
        }
    }

    /// The side of the one Redo: the one Undo took from last that can still be made again, else any that can.
    pub fn joined_redo_side(&self) -> Option<Side> {
        (self.workbench.wardrobe.trail.iter().rev().copied())
            .chain([Side::Wardrobe, Side::Project])
            .find(|side| self.can_redo_on(*side))
    }

    /// What the one Redo of the Wardrobe workspace would make again.
    pub fn joined_redo_label(&self) -> Option<String> {
        match self.joined_redo_side()? {
            Side::Wardrobe => self.redo_label().map(String::from),
            Side::Project => self.project_redo_label(),
        }
    }

    /// Make again what the one Undo took back last.
    pub fn redo_joined(&mut self) {
        let side = self.joined_redo_side();
        let trail = &mut self.workbench.wardrobe.trail;
        if let Some(at) = trail.iter().rposition(|s| Some(*s) == side) {
            trail.remove(at);
        }
        match side {
            Some(Side::Wardrobe) => self.redo_last_change(),
            Some(Side::Project) => match self.redo_project() {
                Ok(label) => self.show_toast(tr!("Redid '{label}'", label = label)),
                Err(why) => self.show_warning_toast(why),
            },
            None => {}
        }
    }

    /// Put the last version of the pack of `module` that reads in place of a `wardrobe.json` that does not.
    pub fn restore_last_good(&mut self, module: String) {
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        self.wardrobe_job(
            tr!("Restoring the last good version"),
            &module,
            Box::new(move || {
                Done::said(
                    backup::restore_last_good(&folder)
                        .map(|version| {
                            tr!("Restored the pack as it was {when}: {version}. The text that did not read is kept in .openrage/unreadable", when = crate::state::prefs::ago(version.at), version = version.label)
                        })
                        .map_err(|e| e.to_string()),
                )
            }),
        );
    }

    /// Read the earlier versions of the pack shown, unless they are read already for this state of it.
    pub fn load_versions(&mut self) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let generation = self.workbench.wardrobe.generation;
        let fresh = |v: &Versions| v.module == module && v.generation == generation;
        if self.workbench.wardrobe.versions.as_ref().is_some_and(fresh) {
            return;
        }
        let Some(pack) = self.wardrobe_pack() else {
            return;
        };
        let folder = pack.folder.clone();
        let list = (backup::versions(&folder).into_iter().rev())
            .filter_map(|version| {
                Some((
                    version.clone(),
                    backup::read_version(&folder, version.n).ok()?,
                ))
            })
            .collect();
        self.workbench.wardrobe.versions = Some(Versions {
            module,
            generation,
            list,
        });
    }

    /// The pack as version `n` of the pack shown held it, from the versions already read.
    fn version_pack(&self, n: u64) -> Option<(Version, Wardrobe)> {
        let versions = self.workbench.wardrobe.versions.as_ref()?;
        (versions.list.iter())
            .find(|(version, _)| version.n == n)
            .cloned()
    }

    /// Make the whole pack what version `n` held it. Undo takes it back.
    pub fn restore_pack_version(&mut self, n: u64) {
        let (Some(module), Some((version, then))) =
            (self.wardrobe_module_id(), self.version_pack(n))
        else {
            return;
        };
        let ago = crate::state::prefs::ago(version.at);
        let step = (
            "version",
            tr!("Restored the pack to how it was {ago}", ago = ago),
        );
        self.pack_edit(tr!("Restoring a version"), step, &module, false, move |w| {
            backup::restore_pack(w, &then);
            Ok(tr!("The pack is as it was {ago}", ago = ago))
        });
    }

    /// Put garment `id` back to what version `n` held of it, and leave every other garment as it is. Undo takes it back.
    pub fn roll_back_garment(&mut self, id: String, n: u64) {
        let (Some(module), Some((version, then))) =
            (self.wardrobe_module_id(), self.version_pack(n))
        else {
            return;
        };
        let (name, ago) = (
            self.garment_name(&module, &id),
            crate::state::prefs::ago(version.at),
        );
        let step = (
            "rollback",
            tr!(
                "Put {name} back to how it was {ago}",
                name = name,
                ago = ago
            ),
        );
        self.pack_edit(
            tr!("Rolling a garment back"),
            step,
            &module,
            false,
            move |w| {
                backup::roll_back_garment(w, &then, &id)?;
                Ok(tr!("{name} is as it was {ago}", name = name, ago = ago))
            },
        );
    }

    /// Put texture `variation` of garment `id` back to what version `n` held of it, and leave every other texture as it is. Undo takes it back.
    pub fn roll_back_variation(&mut self, id: String, variation: usize, n: u64) {
        let (Some(module), Some((version, then))) =
            (self.wardrobe_module_id(), self.version_pack(n))
        else {
            return;
        };
        let (name, ago) = (
            self.garment_name(&module, &id),
            crate::state::prefs::ago(version.at),
        );
        let letter = super::wardrobe::variation_letter(variation);
        let step = (
            "rollback",
            tr!(
                "Put texture {letter} of {name} back to how it was {ago}",
                letter = letter,
                name = name,
                ago = ago
            ),
        );
        self.pack_edit(
            tr!("Rolling a texture back"),
            step,
            &module,
            false,
            move |w| {
                backup::roll_back_variation(w, &then, &id, variation)?;
                Ok(tr!(
                    "Texture {letter} of {name} is as it was {ago}",
                    letter = letter,
                    name = name,
                    ago = ago
                ))
            },
        );
    }
}
