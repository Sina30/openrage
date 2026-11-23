//! Writing a garment's files out of the pack, under the names the game gives them.
//!
//! The names are the ones a build ships and a drop reads (`<ped>_<collection>^jbib_012_u.ydd`), so a folder written here
//! and dropped on another pack makes the garment again, with its place, its textures and its tints. Nothing is
//! overwritten: a file of that name with other bytes is left alone and listed.

use std::path::Path;

use crate::alloc::{allocate, Allocation};
use crate::edit::Error;
use crate::facts::{digest, stays_inside};
use crate::follow::{follow, Led};
use crate::import::LeftOut;
use crate::layout;
use crate::model::{Garment, Wardrobe};
use crate::rules::name_of;

/// What an export wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exported {
    /// The names of the files that are in the folder now, each with the bytes of the pack's file.
    pub written: Vec<String>,
    /// What was not written, and why.
    pub left_out: Vec<LeftOut>,
}

/// The files of garment `index`, as paths inside the pack and the names to give them; the model's group when `texture` is none, else that texture only.
fn named(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    index: usize,
    texture: Option<usize>,
) -> Option<Vec<(String, String)>> {
    let place = allocation.of(index)?;
    let garment: &Garment = &wardrobe.garments[index];
    let collection = wardrobe.collection_of(place.ped, place.part);
    let model = layout::model_name(
        place.ped,
        &collection,
        place.slot,
        place.position,
        garment.skin,
    );
    let mut jobs = Vec::new();
    if texture.is_none() {
        if let Some(file) = &garment.model {
            jobs.push((file.clone(), model.clone()));
            for (i, alternate) in garment.alternates.iter().enumerate() {
                jobs.push((alternate.clone(), layout::alternate_name(&model, i + 1)));
            }
            if let (Some(cloth), false) = (&garment.cloth, place.slot.is_prop()) {
                jobs.push((cloth.clone(), layout::cloth_name(&model)));
            }
        }
    }
    for (i, variation) in garment.variations.iter().enumerate() {
        let Some(file) = &variation.texture else {
            continue;
        };
        if texture.is_some_and(|only| only != i) {
            continue;
        }
        let tint = variation.tint.as_deref().unwrap_or(garment.skin.tint());
        let name =
            layout::texture_name(place.ped, &collection, place.slot, place.position, i, tint);
        jobs.push((file.clone(), name));
    }
    Some(jobs)
}

/// Copy `from` to `to` in one step.
fn copy_whole(from: &Path, to: &Path) -> Result<(), Error> {
    let name = to
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let part = to.with_file_name(format!(".{name}.part"));
    let done = std::fs::copy(from, &part).and_then(|_| std::fs::rename(&part, to));
    if let Err(e) = done {
        let _ = std::fs::remove_file(&part);
        return Err(Error::Io(to.display().to_string(), e));
    }
    Ok(())
}

fn export(
    folder: &Path,
    ids: &[String],
    to: &Path,
    texture: Option<usize>,
) -> Result<Exported, Error> {
    let wardrobe = Wardrobe::read(folder)?;
    let mut wanted: Vec<usize> = Vec::new();
    for id in ids {
        let at = (wardrobe.garments.iter())
            .position(|g| g.id == *id)
            .ok_or_else(|| Error::NoSuchGarment(id.clone()))?;
        if !wanted.contains(&at) {
            wanted.push(at);
        }
    }
    if wanted.is_empty() {
        return Err(Error::Refused("there is no garment to export".into()));
    }
    std::fs::create_dir_all(to).map_err(|e| Error::Io(to.display().to_string(), e))?;
    let allocation = allocate(&wardrobe);
    let scratch = tempfile::tempdir().map_err(|e| Error::Io("a scratch folder".into(), e))?;
    let mut done = Exported::default();
    for index in wanted {
        let who = name_of(&wardrobe.garments[index]);
        let Some(jobs) = named(&wardrobe, &allocation, index, texture) else {
            done.left_out.push(LeftOut {
                path: who,
                why: "it has no place yet, so the game has no name for its files: give it a ped and a slot".into(),
            });
            continue;
        };
        for (rel, name) in jobs {
            // ⚠ wardrobe.json is text that travels through Git: a path that leaves the pack is not read.
            if !stays_inside(&rel) {
                done.left_out.push(LeftOut {
                    path: format!("{who}: {rel}"),
                    why: "it is not a file inside the pack".into(),
                });
                continue;
            }
            // ⚠ A reference is followed to the file or source it leads to: its own text is never a game file.
            let mut from = match follow(folder, &rel) {
                Led::Own => folder.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)),
                Led::To { at, .. } => at,
                Led::Id(id) => {
                    done.left_out.push(LeftOut {
                        path: format!("{who}: {rel}"),
                        why: format!("it is only the id {id}, which is not a file in any project, so there is none to write"),
                    });
                    continue;
                }
                Led::Nowhere { why, .. } => {
                    done.left_out.push(LeftOut {
                        path: format!("{who}: {rel}"),
                        why,
                    });
                    continue;
                }
            };
            if from.is_dir() {
                // A model that is a source folder is exported as the file a build makes of it.
                match or_source::cook::cook_source(&from, scratch.path()) {
                    Ok(cooked) => from = cooked,
                    Err(why) => {
                        done.left_out.push(LeftOut {
                            path: format!("{who}: {rel}"),
                            why: why.join("; "),
                        });
                        continue;
                    }
                }
            }
            let Ok(bytes) = std::fs::read(&from) else {
                done.left_out.push(LeftOut {
                    path: format!("{who}: {rel}"),
                    why: "it is not a file in the pack".into(),
                });
                continue;
            };
            let at = to.join(&name);
            if at.exists() {
                match std::fs::read(&at).is_ok_and(|held| digest(&held) == digest(&bytes)) {
                    true => done.written.push(name),
                    false => done.left_out.push(LeftOut {
                        path: name,
                        why: "a different file of that name is in the folder, and it is not overwritten".into(),
                    }),
                }
                continue;
            }
            copy_whole(&from, &at)?;
            done.written.push(name);
        }
    }
    Ok(done)
}

/// Write the files of the garments `ids` into the folder `to`, each under the name a build gives it.
pub fn garments(folder: &Path, ids: &[String], to: &Path) -> Result<Exported, Error> {
    export(folder, ids, to, None)
}

/// Write the file of texture `variation` of garment `id` into the folder `to`, under the name a build gives it.
pub fn variation(folder: &Path, id: &str, variation: usize, to: &Path) -> Result<Exported, Error> {
    export(
        folder,
        std::slice::from_ref(&id.to_string()),
        to,
        Some(variation),
    )
}
