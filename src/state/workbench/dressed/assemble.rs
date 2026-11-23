//! The scene of the dressed ped: what the plan wears, made of the files that have been read, and the sentences that say what is left out.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::{
    describe, hair_note, level_name, props, Aux, Base, Counts, Diffuse, DressedScene, Loaded,
    Model, Plan, ScenePart, Slot, LEVELS, NO_GAME, TINTS,
};
use crate::state::AppState;

impl AppState {
    /// Make the scene of what the plan wears from what has been read, and the notes that say what is left out.
    pub(super) fn assemble_dressed(&mut self, plan: &Plan) {
        let tint = {
            let dressed = &self.workbench.wardrobe.dressed;
            TINTS.get(dressed.tint).map_or([1.0; 3], |t| t.1)
        };
        let mut notes: Vec<String> = Vec::new();
        for hidden in &plan.dressing.hidden {
            let name = self
                .wardrobe_pack()
                .and_then(|p| p.read.as_ref().ok())
                .and_then(|read| read.wardrobe.garment(&hidden.by))
                .map_or_else(|| tr!("A hat").to_string(), or_wardrobe::rules::name_of);
            notes.push(hair_note(&name));
        }
        for left in &plan.dressing.left {
            let name = self
                .wardrobe_pack()
                .and_then(|p| p.read.as_ref().ok())
                .and_then(|read| read.wardrobe.garment(&left.garment))
                .map_or_else(|| tr!("A garment").to_string(), or_wardrobe::rules::name_of);
            notes.push(format!("{name} {}", left.why));
        }
        notes.extend(plan.dressing.unmet.iter().cloned());
        // The props, by name, for the sentence that says why one is not drawn.
        let prop_names: Vec<(String, String)> = (plan.slots.iter())
            .filter(|slot| slot.anchor.is_some())
            .filter_map(|slot| slot.garment.clone())
            .filter_map(|id| {
                let read = self.wardrobe_pack()?.read.as_ref().ok()?;
                let name = or_wardrobe::rules::name_of(read.wardrobe.garment(&id)?);
                Some((id, name))
            })
            .collect();
        let dressed = &mut self.workbench.wardrobe.dressed;
        match &dressed.base {
            Base::Failed(why) => notes.push(tr!("The ped's own drawables: {why}", why = why)),
            Base::Idle if dressed.base_root.is_none() => {
                notes.push(crate::i18n::tr(NO_GAME).to_string())
            }
            Base::Idle => notes.push(
                tr!("Reading the game's keys before the ped's own drawables can be read")
                    .to_string(),
            ),
            Base::Ready(mounted) => {
                if let Some(Err(why)) = mounted.outfits.get(&plan.ped) {
                    notes.push(tr!("The ped's own drawables: {why}", why = why));
                }
            }
            _ => {}
        }
        dressed.lift = plan.dressing.lift;
        dressed.driven = plan.dressing.driven.clone();
        // What the files read make of each slot, before any scene is built: the model, the texture, and where a prop is carried to.
        type Read<'a> = (
            &'a Slot,
            Arc<Model>,
            Option<Arc<Diffuse>>,
            Option<(usize, glam::Mat4)>,
        );
        let mut read: Vec<Read<'_>> = Vec::new();
        let mut levels = [Counts::default(); LEVELS];
        let mut held = [0; LEVELS];
        let lod = dressed.lod.min(LEVELS - 1);
        let mut key = std::collections::hash_map::DefaultHasher::new();
        lod.hash(&mut key);
        let mut waiting = false;
        let mut fell_back = 0;
        for slot in &plan.slots {
            let model = match dressed.loaded.get(&slot.model) {
                Some(Loaded::Model(model)) => Some(Arc::clone(model)),
                Some(Loaded::Failed(why)) => {
                    notes.push(format!("{}: {why}", describe(&slot.model)));
                    None
                }
                _ => {
                    waiting = true;
                    None
                }
            };
            let diffuse = slot
                .texture
                .as_ref()
                .and_then(|need| match dressed.loaded.get(need) {
                    Some(Loaded::Texture(diffuse)) => Some(Arc::clone(diffuse)),
                    Some(Loaded::Failed(why)) => {
                        notes.push(format!("{}: {why}", describe(need)));
                        None
                    }
                    _ => {
                        waiting = true;
                        None
                    }
                });
            let Some(model) = model else {
                continue;
            };
            let Some(drawn) = model.at(lod) else {
                continue;
            };
            // A prop is carried to the bone of its anchor, which only a skeleton read from the install gives.
            let carried_to = match slot.anchor {
                Some(prop) => match dressed.bone_at_rest(plan.ped, prop.bone()) {
                    Some(bone) => Some(bone),
                    None => {
                        notes.push(tr!("{garment} is worn at the {bone} bone of the ped, whose skeleton is read from the game install, which needs to be chosen in Settings", garment = slot.garment
                                .as_ref()
                                .and_then(|id| prop_names.iter().find(|(of, _)| of == id))
                                .map_or(tr!("A prop"), |(_, name)| name.as_str()), bone = prop.bone()));
                        continue;
                    }
                },
                None => None,
            };
            if slot.garment.is_some() {
                fell_back += usize::from(drawn.lod != lod);
                for (at, total) in levels.iter_mut().enumerate() {
                    held[at] += usize::from(model.holds(at));
                    if let Some(level) = model.at(at) {
                        total.add(Counts::of(level.geometry));
                    }
                }
            }
            slot.model.hash(&mut key);
            diffuse.as_ref().map(|d| d.id).hash(&mut key);
            if let (Some(prop), Some((bone, at))) = (slot.anchor, carried_to) {
                (prop, bone, at.to_cols_array().map(f32::to_bits)).hash(&mut key);
            }
            read.push((slot, model, diffuse, carried_to));
        }
        if fell_back > 0 {
            notes.push(tr_n!(
                "{count} garment worn hold no {level} model and is drawn at its first",
                "{count} garments worn hold no {level} model and are drawn at their first",
                fell_back,
                level = level_name(lod).to_lowercase()
            ));
        }
        if waiting && dressed.scene.is_some() {
            // The scene that stands is kept while the next one is read: a texture that arrives late does not blank the ped.
            dressed.notes = notes;
            return;
        }
        plan.dressing.lift.to_bits().hash(&mut key);
        tint.map(f32::to_bits).hash(&mut key);
        let key = key.finish();
        dressed.notes = notes;
        if dressed.scene.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let mut parts: Vec<ScenePart> = Vec::new();
        let (mut skins, mut aux, mut shaded) = (Vec::new(), Vec::new(), Vec::new());
        for (slot, model, diffuse, carried_to) in read {
            let Some(drawn) = model.at(lod) else {
                continue;
            };
            let (geometry, mesh_skins) = match carried_to {
                Some(bone) => props::carried(drawn.geometry, bone),
                None => (Arc::clone(drawn.geometry), drawn.skins.to_vec()),
            };
            // A prop is carried to its bone as its meshes are; what is worn at rest is shared with the model
            shaded.push(model.shaded().map(|surfaces| match carried_to {
                Some((bone, at)) => {
                    let mut placed = surfaces.to_vec();
                    crate::render::game_scene::carried(&mut placed, at);
                    crate::render::dressed::hold_to_bone(&mut placed, bone);
                    Arc::new(placed)
                }
                None => Arc::clone(surfaces),
            }));
            skins.push(mesh_skins);
            aux.push(
                (0..geometry.len())
                    .map(|mesh| drawn.aux.get(mesh).map(Arc::clone))
                    .collect::<Vec<Option<Arc<Aux>>>>(),
            );
            parts.push(ScenePart {
                geometry,
                diffuse,
                tint: if slot.garment.is_some() {
                    tint
                } else {
                    [1.0; 3]
                },
                lift: plan.dressing.lift,
                garment: slot.garment.clone(),
            });
        }
        let scene = DressedScene::new(key, parts)
            .with_levels(levels, held)
            .with_skins(skins)
            .with_aux(aux)
            .with_shaded(shaded);
        let to_frame = dressed.framed_for != Some(plan.ped) || dressed.needs_frame;
        if to_frame && !scene.parts.is_empty() {
            dressed.camera.frame_sphere(scene.centre, scene.radius);
            dressed.framed_for = Some(plan.ped);
            dressed.needs_frame = false;
        }
        dressed.scene = Some(Arc::new(scene));
    }
}
