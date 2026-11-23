//! The edits of the places that dummies make: filling the holes of a slot and putting its dummies last, and adding an empty texture to a garment and putting its empty textures last, each one step of Undo.

use or_wardrobe::{dummies, variations, Ped, Slot};

use crate::state::AppState;

impl AppState {
    /// Fill the holes of `slot` of `ped` in the pack shown with dummies, so that no place of it is a gap nobody holds.
    pub fn fill_slot(&mut self, ped: Ped, slot: Slot) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let step = (
            "dummies",
            tr!(
                "Filled the holes of {slot} of {ped} with dummies",
                slot = slot,
                ped = ped
            ),
        );
        self.pack_edit(tr!("Filling a slot"), step, &module, false, move |w| {
            Ok(match dummies::fill(w, ped, slot) {
                0 => tr!(
                    "{slot} of {ped} has no hole to fill",
                    slot = slot,
                    ped = ped
                ),
                n => tr!(
                    "Filled {n} hole(s) of {slot} of {ped} with dummies",
                    n = n,
                    slot = slot,
                    ped = ped
                ),
            })
        });
    }

    /// Put the dummies of `slot` of `ped` after its garments. The garments that move are pinned where they went, and a pack that freezes its drawables stops the next build until a reason says why.
    pub fn sort_slot(&mut self, ped: Ped, slot: Slot) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let step = (
            "dummies",
            tr!(
                "Put the dummies of {slot} of {ped} last",
                slot = slot,
                ped = ped
            ),
        );
        self.pack_edit(tr!("Sorting a slot"), step, &module, true, move |w| {
            Ok(match dummies::resort(w, ped, slot) {
                0 => tr!(
                    "The dummies of {slot} of {ped} are last already",
                    slot = slot,
                    ped = ped
                ),
                n => {
                    tr!(
                        "Put the dummies of {slot} of {ped} last; {n} garment(s) changed place",
                        slot = slot,
                        ped = ped,
                        n = n
                    )
                }
            })
        });
    }

    /// Add one empty texture at the end of the textures of garment `id`. Undo takes it away.
    pub fn hold_texture_place(&mut self, id: String) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let name = self.garment_name(&module, &id);
        // The letter it takes: the one after the textures the garment has.
        let held = (self.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
            .and_then(|read| read.wardrobe.garment(&id))
            .map_or(0, |g| g.variations.len());
        let letter = super::wardrobe::variation_letter(held);
        let step = (
            tr!("empty textures"),
            tr!(
                "Added an empty texture {letter} to {name}",
                letter = letter,
                name = name
            ),
        );
        self.pack_edit(tr!("Adding an empty texture"), step, &module, false, move |w| {
            variations::reserve(w, &id, 1)?;
            Ok(tr!("Added an empty texture {letter} to {name}: it ships no file, and a texture you add later can take its letter", letter = letter, name = name))
        });
    }

    /// Put the empty textures of garment `id` after its textures that have a file. Undo puts them back.
    pub fn sort_texture_places(&mut self, id: String) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let name = self.garment_name(&module, &id);
        let step = (
            tr!("empty textures"),
            tr!("Put the empty textures of {name} last", name = name),
        );
        self.pack_edit(
            tr!("Sorting empty textures"),
            step,
            &module,
            false,
            move |w| {
                Ok(match variations::resort(w, &id)? {
                    0 => tr!("The empty textures of {name} are last already", name = name),
                    n => {
                        tr!(
                            "Put the empty textures of {name} last; {n} texture(s) changed letter",
                            name = name,
                            n = n
                        )
                    }
                })
            },
        );
    }
}
