//! The edits of the Map workspace: an entity placed, added or taken out, an archetype added, changed or taken out, and the Undo and Redo
//! that move along them.
//!
//! ⚠ **Thin by design.** Each edit asks `or-maps` to change the map file or the type file a source cooks to, which writes the result back as
//! the source's XML, and keeps the XML before and after as the step Undo moves along. Undo refuses once anything else has changed the file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glam::Vec3;
use or_maps::edit::Pose;
use or_maps::Maps;
use or_project::ModuleKind;

use super::maps::{FileChange, Pick, PickKind, Step, KEPT};
use crate::state::AppState;

impl AppState {
    /// The folder of the source `source` of the shown pack.
    fn map_source_dir(&self, source: &str) -> Option<PathBuf> {
        let pack = self.maps_pack()?;
        Some(
            pack.folder
                .join(source.replace('/', std::path::MAIN_SEPARATOR_STR)),
        )
    }

    /// Stand the entity `index` of the map file `source` at `pose`, as one step Undo takes back.
    pub fn place_entity_in(&mut self, source: &str, index: usize, pose: Pose, label: String) {
        // A scanned GTA VI map is its document's tree, which its own step of Undo holds.
        if self.scanned_map().is_some() {
            if let Err(why) = self.place_scanned_entity(index, pose, label) {
                self.show_error_toast(why);
            }
            self.workbench.maps.preview = None;
            return;
        }
        let Some((module, _)) = self.maps_folder_to_write() else {
            return;
        };
        let Some(dir) = self.map_source_dir(source) else {
            return;
        };
        let boxes = self.shown_boxes();
        match or_maps::edit::edit_map(&dir, |map| {
            or_maps::edit::place_entity(map, index, pose)?;
            or_maps::edit::grow_extents(map, |hash| boxes.get(&hash).copied());
            Ok(())
        }) {
            Ok(edited) if edited.changed() => {
                self.patch_scene(source, index, pose);
                self.push_maps_step(Step {
                    label,
                    changes: vec![FileChange {
                        path: edited.file,
                        before: edited.before,
                        after: edited.after,
                    }],
                });
                let set = self.workbench.maps.set.clone().unwrap_or_default();
                self.workbench.maps.module = Some(module.clone());
                if self.workbench.maps.selected.is_none() {
                    self.select_in_maps(
                        &module,
                        &set,
                        Some(Pick {
                            source: source.to_string(),
                            kind: PickKind::Entity(index),
                        }),
                    );
                }
            }
            Ok(_) => {}
            Err(why) => self.show_error_toast(why),
        }
        self.workbench.maps.preview = None;
    }

    /// Stand the entity selected at `pose`, as one step.
    pub fn place_selected_entity(&mut self, pose: Pose, label: String) {
        let Some(Pick {
            source,
            kind: PickKind::Entity(index),
        }) = self.workbench.maps.selected.clone()
        else {
            return;
        };
        self.place_entity_in(&source, index, pose, label);
    }

    /// The boxes the extents of a map of the shown set are grown to hold: what the type files of the set declare.
    fn shown_boxes(&self) -> BTreeMap<u32, (Vec3, Vec3, f32)> {
        let Some((pack, set)) = self.shown_set() else {
            return BTreeMap::new();
        };
        (pack.read.as_ref())
            .map(|read| or_maps::edit::archetype_boxes(set, &read.facts))
            .unwrap_or_default()
    }

    /// Put the scene the shown set was last read as to the pose written, so the entity does not jump back for the frames before the next scan.
    fn patch_scene(&mut self, source: &str, index: usize, pose: Pose) {
        let set = self.shown_set().map(|(_, set)| set.id.clone());
        let module = self.maps_pack().map(|p| p.module.clone());
        let (Some(set), Some(module)) = (set, module) else {
            return;
        };
        let Some(scan) = self.workbench.scan.as_mut() else {
            return;
        };
        let Some(scene) = (scan.maps.iter_mut())
            .find(|m| m.module == module)
            .and_then(|m| m.read.as_mut().ok())
            .and_then(|pack| pack.scenes.iter_mut().find(|(id, _)| *id == set))
            .map(|(_, scene)| scene)
        else {
            return;
        };
        if let Some(placed) = scene
            .find(source, index)
            .and_then(|at| scene.placed.get_mut(at))
        {
            placed.obb.position = pose.position;
            placed.obb.rotation = pose.rotation;
            placed.obb.scale = Vec3::new(pose.scale_xy, pose.scale_xy, pose.scale_z);
        }
    }

    /// Add an entity of `archetype` at `pose` to the map file the selection is in (else the first of the shown set), as a copy of the entity selected (else the first one the set holds).
    pub fn add_entity_at(&mut self, archetype: or_maps::Name, pose: Pose) {
        let Some((module, _)) = self.maps_folder_to_write() else {
            return;
        };
        let Some((pack, set)) = self.shown_set() else {
            return;
        };
        let read = pack.read.as_ref().ok();
        // The map file to add to: the selection's, else the first map file of the set.
        let target = self
            .workbench
            .maps
            .selected
            .as_ref()
            .map(|p| p.source.clone())
            .filter(|s| {
                read.and_then(|r| r.facts.source(s))
                    .is_some_and(|f| f.map().is_some())
            })
            .or_else(|| {
                set.sources
                    .iter()
                    .find(|s| {
                        read.and_then(|r| r.facts.source(s))
                            .is_some_and(|f| f.map().is_some())
                    })
                    .cloned()
            });
        let set_id = set.id.clone();
        let sources = set.sources.clone();
        let Some(target) = target else {
            self.show_warning_toast(tr!(
                "This set has no map file to add an entity to: add a .ymap first"
            ));
            return;
        };
        let Some(dir) = self.map_source_dir(&target) else {
            return;
        };
        // The record to copy: the entity selected, else the first the target holds, else the first any map file of the set holds.
        let donor = match self.workbench.maps.selected.clone() {
            Some(Pick {
                source,
                kind: PickKind::Entity(at),
            }) => self
                .map_source_dir(&source)
                .and_then(|d| or_maps::edit::read_entity(&d, at).ok()),
            _ => None,
        }
        .or_else(|| or_maps::edit::first_entity(&dir))
        .or_else(|| {
            sources
                .iter()
                .filter_map(|s| self.map_source_dir(s))
                .find_map(|d| or_maps::edit::first_entity(&d))
        });
        let Some(donor) = donor else {
            self.show_warning_toast(tr!(
                "No entity in this set can be copied: add the first from a map file that has one"
            ));
            return;
        };
        let text = archetype.text.clone();
        let mut index = 0;
        let label = tr!("Added an entity of {name}", name = archetype.shown());
        let boxes = self.shown_boxes();
        match or_maps::edit::edit_map(&dir, |map| {
            index = or_maps::edit::add_entity(map, &donor, archetype, pose)?;
            or_maps::edit::grow_extents(map, |hash| boxes.get(&hash).copied());
            Ok(())
        }) {
            Ok(edited) => {
                self.remember_map_names(&module, &[text.as_deref().unwrap_or_default()]);
                self.push_maps_step(Step {
                    label,
                    changes: vec![FileChange {
                        path: edited.file,
                        before: edited.before,
                        after: edited.after,
                    }],
                });
                self.select_in_maps(
                    &module,
                    &set_id,
                    Some(Pick {
                        source: target,
                        kind: PickKind::Entity(index),
                    }),
                );
            }
            Err(why) => self.show_error_toast(why),
        }
    }

    /// Take the entity selected out of its map file, with the LOD links renumbered, as one step.
    pub fn remove_selected_entity(&mut self) {
        let Some(Pick {
            source,
            kind: PickKind::Entity(index),
        }) = self.workbench.maps.selected.clone()
        else {
            return;
        };
        let Some((module, _)) = self.maps_folder_to_write() else {
            return;
        };
        let Some(dir) = self.map_source_dir(&source) else {
            return;
        };
        match or_maps::edit::edit_map(&dir, |map| or_maps::edit::remove_entity(map, index)) {
            Ok(edited) => {
                self.push_maps_step(Step {
                    label: tr!("Removed entity {index}", index = index),
                    changes: vec![FileChange {
                        path: edited.file,
                        before: edited.before,
                        after: edited.after,
                    }],
                });
                let set = self.workbench.maps.set.clone().unwrap_or_default();
                self.select_in_maps(&module, &set, None);
            }
            Err(why) => self.show_error_toast(why),
        }
    }

    /// Place another one of the entity selected (else the first the set places) where the view looks, as one step.
    pub fn add_entity_here(&mut self) {
        let Some(scene) = self.shown_scene() else {
            return;
        };
        let archetype = self
            .selected_placed()
            .or_else(|| scene.placed.first())
            .map(|p| p.archetype.clone());
        let Some(archetype) = archetype else {
            self.show_warning_toast(tr!(
                "This set places nothing to make another of: add a map file that places something"
            ));
            return;
        };
        let work = &self.workbench.maps;
        let mut at = work.camera.target;
        if work.snap.grid_on {
            at.x = (at.x / work.snap.grid).round() * work.snap.grid;
            at.y = (at.y / work.snap.grid).round() * work.snap.grid;
        }
        let pose = Pose {
            position: at,
            rotation: glam::Quat::IDENTITY,
            scale_xy: 1.0,
            scale_z: 1.0,
        };
        self.add_entity_at(archetype, pose);
    }

    /// Declare an archetype under a name nobody in the set holds, as one step.
    pub fn add_archetype_here(&mut self) {
        let Some((pack, set)) = self.shown_set() else {
            return;
        };
        let held: Vec<String> = pack
            .read
            .as_ref()
            .ok()
            .into_iter()
            .flat_map(|read| {
                set.sources
                    .iter()
                    .filter_map(|s| read.facts.source(s)?.types())
                    .flat_map(|t| t.archetypes.iter())
            })
            .map(|a| a.name.shown().to_ascii_lowercase())
            .collect();
        let mut n = 1;
        let name = loop {
            let name = if n == 1 {
                "prop_new".to_string()
            } else {
                format!("prop_new_{n}")
            };
            if !held.contains(&name) {
                break name;
            }
            n += 1;
        };
        self.add_archetype_named(&name, None);
    }

    /// Change the archetype selected with `change`, as one step; `typed` are the names the change writes, which the pack remembers.
    pub fn edit_selected_archetype(
        &mut self,
        label: String,
        typed: &[&str],
        change: impl FnOnce(&mut or_io::meta::typed::CBaseArchetypeDefAny),
    ) {
        let Some(Pick {
            source,
            kind: PickKind::Archetype(index),
        }) = self.workbench.maps.selected.clone()
        else {
            return;
        };
        let Some((module, _)) = self.maps_folder_to_write() else {
            return;
        };
        let Some(dir) = self.map_source_dir(&source) else {
            return;
        };
        match or_maps::edit::edit_types(&dir, |types| {
            let held = types.archetypes.len();
            let archetype = types.archetypes.get_mut(index).ok_or_else(|| {
                format!("archetype {index} is past the {held} the type file holds")
            })?;
            change(archetype);
            Ok(())
        }) {
            Ok(edited) if edited.changed() => {
                self.remember_map_names(&module, typed);
                self.push_maps_step(Step {
                    label,
                    changes: vec![FileChange {
                        path: edited.file,
                        before: edited.before,
                        after: edited.after,
                    }],
                });
            }
            Ok(_) => {}
            Err(why) => self.show_error_toast(why),
        }
    }

    /// Give the entity selected the archetype `name`, as one step.
    pub fn rename_selected_entity_archetype(&mut self, name: &str) {
        let name = name.trim();
        let Some(Pick {
            source,
            kind: PickKind::Entity(index),
        }) = self.workbench.maps.selected.clone()
        else {
            return;
        };
        if name.is_empty() {
            return;
        }
        let Some((module, _)) = self.maps_folder_to_write() else {
            return;
        };
        let Some(dir) = self.map_source_dir(&source) else {
            return;
        };
        let held = or_io::meta::instance::HashedString::new(name);
        match or_maps::edit::edit_map(&dir, |map| {
            let held_by = map.entities.len();
            let entity = map
                .entities
                .get_mut(index)
                .ok_or_else(|| format!("entity {index} is past the {held_by} the map holds"))?;
            entity.archetype_name = held;
            Ok(())
        }) {
            Ok(edited) if edited.changed() => {
                self.remember_map_names(&module, &[name]);
                self.push_maps_step(Step {
                    label: tr!(
                        "Placed {name} as entity {index}",
                        name = name,
                        index = index
                    ),
                    changes: vec![FileChange {
                        path: edited.file,
                        before: edited.before,
                        after: edited.after,
                    }],
                });
            }
            Ok(_) => {}
            Err(why) => self.show_error_toast(why),
        }
    }

    /// Remember the names a person typed in the pack, so the hash a file keeps of each shows as the name.
    fn remember_map_names(&mut self, module: &str, typed: &[&str]) {
        let Some(project) = self.project_path.as_deref() else {
            return;
        };
        for name in typed.iter().filter(|n| !n.trim().is_empty()) {
            self.name_index.add(
                crate::state::name_index::NameIndexCategory::Model,
                name.trim(),
            );
        }
        let Some(module) = (self.project_carried.container.modules.iter())
            .find(|m| m.id == module && m.kind == ModuleKind::Maps)
        else {
            return;
        };
        let Ok(folder) = or_maps::module_folder(&or_project::project_folder(project), module)
        else {
            return;
        };
        if let Ok(mut model) = Maps::read(&folder) {
            if model.remember(typed) {
                let _ = model.write(&folder);
            }
        }
    }

    /// Add an archetype called `name` to the type file the selection is in (else the first of the shown set), as a copy of the archetype selected, as one step.
    pub fn add_archetype_named(&mut self, name: &str, asset: Option<&str>) {
        let Some((module, _)) = self.maps_folder_to_write() else {
            return;
        };
        let Some((pack, set)) = self.shown_set() else {
            return;
        };
        let read = pack.read.as_ref().ok();
        let is_types = |s: &String| {
            read.and_then(|r| r.facts.source(s))
                .is_some_and(|f| f.types().is_some())
        };
        let target = self
            .workbench
            .maps
            .selected
            .as_ref()
            .map(|p| p.source.clone())
            .filter(is_types)
            .or_else(|| set.sources.iter().find(|s| is_types(s)).cloned());
        let set_id = set.id.clone();
        let Some(target) = target else {
            self.show_warning_toast(tr!(
                "This set has no type file to add an archetype to: add a .ytyp first"
            ));
            return;
        };
        let Some(dir) = self.map_source_dir(&target) else {
            return;
        };
        let donor = match self.workbench.maps.selected.clone() {
            Some(Pick {
                source,
                kind: PickKind::Archetype(at),
            }) => self
                .map_source_dir(&source)
                .and_then(|d| or_maps::edit::read_archetype(&d, at).ok()),
            _ => None,
        };
        let mut index = 0;
        match or_maps::edit::edit_types(&dir, |types| {
            index = or_maps::edit::add_archetype(types, donor.as_ref(), name, asset)?;
            Ok(())
        }) {
            Ok(edited) => {
                self.remember_map_names(&module, &[name, asset.unwrap_or(name)]);
                self.push_maps_step(Step {
                    label: tr!("Added the archetype {name}", name = name),
                    changes: vec![FileChange {
                        path: edited.file,
                        before: edited.before,
                        after: edited.after,
                    }],
                });
                self.select_in_maps(
                    &module,
                    &set_id,
                    Some(Pick {
                        source: target,
                        kind: PickKind::Archetype(index),
                    }),
                );
            }
            Err(why) => self.show_error_toast(why),
        }
    }

    /// Take the archetype selected out of its type file, as one step.
    pub fn remove_selected_archetype(&mut self) {
        let Some(Pick {
            source,
            kind: PickKind::Archetype(index),
        }) = self.workbench.maps.selected.clone()
        else {
            return;
        };
        let Some((module, _)) = self.maps_folder_to_write() else {
            return;
        };
        let Some(dir) = self.map_source_dir(&source) else {
            return;
        };
        match or_maps::edit::edit_types(&dir, |types| or_maps::edit::remove_archetype(types, index))
        {
            Ok(edited) => {
                self.push_maps_step(Step {
                    label: tr!("Removed archetype {index}", index = index),
                    changes: vec![FileChange {
                        path: edited.file,
                        before: edited.before,
                        after: edited.after,
                    }],
                });
                let set = self.workbench.maps.set.clone().unwrap_or_default();
                self.select_in_maps(&module, &set, None);
            }
            Err(why) => self.show_error_toast(why),
        }
    }

    /// Stand the entity selected on the ground under it, as one step.
    pub fn drop_selected_to_ground(&mut self) {
        let Some(Pick {
            kind: PickKind::Entity(index),
            ..
        }) = self.workbench.maps.selected.clone()
        else {
            return;
        };
        let Some(scene) = self.shown_scene() else {
            return;
        };
        let (Some(pose), Some(at)) = (
            self.selected_pose(),
            self.workbench
                .maps
                .selected
                .as_ref()
                .and_then(|p| scene.find(&p.source, index)),
        ) else {
            return;
        };
        let dropped =
            or_maps::gizmo::drop_to_ground(&scene.obbs(), at, pose, self.workbench.maps.snap.floor);
        if dropped.position == pose.position {
            return;
        }
        self.place_selected_entity(
            dropped,
            tr!("Stood entity {index} on the ground", index = index).to_string(),
        );
    }

    fn push_maps_step(&mut self, step: Step) {
        let work = &mut self.workbench.maps;
        work.undo.push(step);
        if work.undo.len() > KEPT {
            work.undo.remove(0);
        }
        work.redo.clear();
        work.generation += 1;
        self.workbench.want_scan();
    }

    /// Put the files of `step` as they were (`undo`) or as the step left them, once none of them was changed by anything but this history since.
    fn apply_maps_step(&mut self, step: &Step, undo: bool) -> Result<(), String> {
        for change in &step.changes {
            let (wanted, expected) = if undo {
                (&change.before, &change.after)
            } else {
                (&change.after, &change.before)
            };
            let now = std::fs::read(&change.path)
                .map_err(|e| format!("{}: {e}", change.path.display()))?;
            if &now != expected {
                return Err(tr!(
                    "{file} changed since: take back the changes made after this one first",
                    file = change.path.display().to_string()
                )
                .to_string());
            }
            let _ = wanted;
        }
        for change in step.changes.iter().rev() {
            let wanted = if undo { &change.before } else { &change.after };
            std::fs::write(&change.path, wanted)
                .map_err(|e| format!("{}: {e}", change.path.display()))?;
        }
        self.workbench.want_scan();
        self.workbench.maps.generation += 1;
        self.workbench.maps.preview = None;
        Ok(())
    }

    /// Take back the last change to a map.
    pub fn undo_maps_change(&mut self) {
        if self.refused_as_read_only() {
            return;
        }
        let Some(step) = self.workbench.maps.undo.pop() else {
            return;
        };
        match self.apply_maps_step(&step, true) {
            Ok(()) => {
                self.show_toast(tr!("Undid: {what}", what = step.label.as_str()));
                self.workbench.maps.redo.push(step);
            }
            Err(why) => {
                self.workbench.maps.undo.push(step);
                self.show_error_toast(why);
            }
        }
    }

    /// Make the change Undo took back again.
    pub fn redo_maps_change(&mut self) {
        if self.refused_as_read_only() {
            return;
        }
        let Some(step) = self.workbench.maps.redo.pop() else {
            return;
        };
        match self.apply_maps_step(&step, false) {
            Ok(()) => {
                self.show_toast(tr!("Did again: {what}", what = step.label.as_str()));
                self.workbench.maps.undo.push(step);
            }
            Err(why) => {
                self.workbench.maps.redo.push(step);
                self.show_error_toast(why);
            }
        }
    }
}
