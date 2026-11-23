//! Putting another file in the place of a garment's model or of one of its textures, the garment keeping all it states.
//!
//! The file becomes a source of the pack ([`crate::intake`]) and the garment names it, so the name the garment shipped
//! under and its settings do not move. A `.ydd` or a `.ytd` is decompiled, a picture is an authored textures source, a
//! source folder is taken as it is, and a reference leads to a source or file that is not the pack's. The file the garment
//! named before is not touched: nothing is written over, and Undo names it again. Every function returns the
//! [`Revision`] that Undo takes back, or the reason and changes nothing.

use std::path::Path;

use or_project::reference::Reference;

use crate::edit::{garment_mut, Copying, Error, Revision};
use crate::intake::{self, Scratch};
use crate::model::Wardrobe;

/// Name the copy `copying` made as the file of garment `id`, through `name`, and write the pack.
fn pointed_at(
    folder: &Path,
    before: Wardrobe,
    id: &str,
    mut copying: Copying,
    name: impl FnOnce(&mut crate::model::Garment) -> Result<(), Error>,
) -> Result<Revision, Error> {
    let mut wardrobe = before.clone();
    let named = garment_mut(&mut wardrobe, id).and_then(name);
    if let Err(e) = named.and_then(|()| Ok(wardrobe.write(folder)?)) {
        copying.undo();
        return Err(e);
    }
    Ok(Revision {
        after: Wardrobe::read(folder)?,
        before,
        copied: copying.copied,
        made: copying.made,
    })
}

/// Replace the model of garment `id` with `source`, a `.ydd` file or a `.drawables.src` folder.
pub fn model(folder: &Path, id: &str, source: &Path) -> Result<Revision, Error> {
    let before = Wardrobe::read(folder)?;
    let current = before
        .garment(id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?
        .model
        .clone();
    let mut copying = Copying::new(folder);
    let rel = match intake::model(&mut copying, &mut Scratch::default(), source) {
        Ok(rel) => rel,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    if current.as_deref() == Some(rel.as_str()) {
        copying.undo();
        return Err(Error::Refused(
            "that is the model the garment has already".into(),
        ));
    }
    pointed_at(folder, before, id, copying, |g| {
        g.model = Some(rel);
        Ok(())
    })
}

/// Replace the model of garment `id` with what `reference` leads to.
pub fn model_by_reference(
    folder: &Path,
    id: &str,
    reference: &Reference,
) -> Result<Revision, Error> {
    let before = Wardrobe::read(folder)?;
    before
        .garment(id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let mut copying = Copying::new(folder);
    let rel = match copying.put_reference("models", reference) {
        Ok(rel) => rel,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    pointed_at(folder, before, id, copying, |g| {
        g.model = Some(rel);
        Ok(())
    })
}

/// The garment's texture `variation` if it is there, else why not.
fn variation_of(before: &Wardrobe, id: &str, variation: usize) -> Result<(), Error> {
    let garment = before
        .garment(id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let held = garment.variations.len();
    match garment.variations.get(variation) {
        Some(_) => Ok(()),
        None => Err(Error::Refused(format!(
            "the garment has {held} texture(s), not {}",
            variation + 1
        ))),
    }
}

/// Replace texture `variation` of garment `id` with `source`, a `.ytd` file, a picture or a `.textures.src` folder; the tint, the ids and every other setting of the texture stay.
pub fn texture(
    folder: &Path,
    id: &str,
    variation: usize,
    source: &Path,
) -> Result<Revision, Error> {
    let before = Wardrobe::read(folder)?;
    variation_of(&before, id, variation)?;
    if !source.exists() {
        return Err(Error::Refused(format!(
            "{} is not a file",
            source.display()
        )));
    }
    let current = before
        .garment(id)
        .and_then(|g| g.variations[variation].texture.clone());
    let mut copying = Copying::new(folder);
    let rel = match intake::texture(&mut copying, &mut Scratch::default(), source) {
        Ok(rel) => rel,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    if current.as_deref() == Some(rel.as_str()) {
        copying.undo();
        return Err(Error::Refused(
            "that is the texture the garment has already".into(),
        ));
    }
    pointed_at(folder, before, id, copying, |g| {
        g.variations[variation].texture = Some(rel);
        Ok(())
    })
}

/// Replace texture `variation` of garment `id` with what `reference` leads to.
pub fn texture_by_reference(
    folder: &Path,
    id: &str,
    variation: usize,
    reference: &Reference,
) -> Result<Revision, Error> {
    let before = Wardrobe::read(folder)?;
    variation_of(&before, id, variation)?;
    let mut copying = Copying::new(folder);
    let rel = match copying.put_reference("textures", reference) {
        Ok(rel) => rel,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    pointed_at(folder, before, id, copying, |g| {
        g.variations[variation].texture = Some(rel);
        Ok(())
    })
}
