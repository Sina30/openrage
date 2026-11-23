//! The textures of a garment: adding one, taking one out, moving one to another letter, and giving one a tint.
//!
//! A texture is lettered by its place in the garment's list (`a`, `b`, `c`), so moving one renames the letters of the
//! ones it passes, and the file each is shipped under changes with its letter. Every function takes the file the row it
//! acts on named when it was drawn, and refuses when that is no longer the texture at that place: a row drawn before an
//! edit moved the textures must not act on another one.

use std::path::Path;

use crate::edit::{garment_mut, Copying, Error, Revision};
use crate::intake::{self, Scratch};
use crate::model::{Garment, Variation, Wardrobe};

/// Texture `variation` of `garment` if it is the one that names `file`, else why not.
fn at<'a>(
    garment: &'a mut Garment,
    variation: usize,
    file: Option<&str>,
) -> Result<&'a mut Variation, Error> {
    let held = garment.variations.len();
    let found = garment.variations.get_mut(variation).ok_or_else(|| {
        Error::Refused(format!(
            "the garment has {held} texture(s), not {}",
            variation + 1
        ))
    })?;
    match found.texture.as_deref() == file {
        true => Ok(found),
        false => Err(Error::Refused(
            "that is not the texture it was any more: look at the garment again".into(),
        )),
    }
}

/// Take texture `variation` of garment `id`, which names `file`, out of it; its file stays, and the textures after it move up one letter.
pub fn remove(
    wardrobe: &mut Wardrobe,
    id: &str,
    variation: usize,
    file: Option<&str>,
) -> Result<(), Error> {
    let garment = garment_mut(wardrobe, id)?;
    at(garment, variation, file)?;
    garment.variations.remove(variation);
    Ok(())
}

/// Give texture `variation` of garment `id`, which names `file`, the letter at place `to`; the textures between move one letter the other way.
pub fn move_to(
    wardrobe: &mut Wardrobe,
    id: &str,
    variation: usize,
    file: Option<&str>,
    to: usize,
) -> Result<(), Error> {
    let garment = garment_mut(wardrobe, id)?;
    at(garment, variation, file)?;
    if to >= garment.variations.len() {
        return Err(Error::Refused(format!(
            "the garment has {} texture(s), so none can be moved to the place {}",
            garment.variations.len(),
            to + 1
        )));
    }
    let moved = garment.variations.remove(variation);
    garment.variations.insert(to, moved);
    Ok(())
}

/// The longest word a tint is.
const MAX_TINT: usize = 16;

/// Give texture `variation` of garment `id`, which names `file`, the tint `tint`: the word its file name ends with. None takes the choice back, and the skin's word is written.
pub fn tint(
    wardrobe: &mut Wardrobe,
    id: &str,
    variation: usize,
    file: Option<&str>,
    tint: Option<&str>,
) -> Result<(), Error> {
    let word = tint
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty());
    if let Some(word) = &word {
        // ⚠ A name is read between underscores, so a tint is one word of letters and digits.
        if word.len() > MAX_TINT || !word.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(Error::Refused(format!(
                "a tint is one word of letters and digits, at most {MAX_TINT} long, such as uni or whi"
            )));
        }
    }
    let garment = garment_mut(wardrobe, id)?;
    let default = garment.skin.tint();
    let found = at(garment, variation, file)?;
    found.tint = word.filter(|w| w != default);
    Ok(())
}

/// Add the texture `source`, a `.ytd` file, a picture or a textures source, to garment `id` as its next texture, as a source of the pack; Undo takes it out again.
pub fn add(folder: &Path, id: &str, source: &Path) -> Result<Revision, Error> {
    if !source.exists() {
        return Err(Error::Refused(format!(
            "{} is not a file",
            source.display()
        )));
    }
    added(folder, id, |copying| {
        intake::texture(copying, &mut Scratch::default(), source)
    })
}

/// [`add`] with what `reference` leads to, which stays where it is.
pub fn add_by_reference(
    folder: &Path,
    id: &str,
    reference: &or_project::reference::Reference,
) -> Result<Revision, Error> {
    added(folder, id, |copying| {
        copying.put_reference("textures", reference)
    })
}

/// Add the texture that `put` places in the pack to garment `id` as its next.
fn added(
    folder: &Path,
    id: &str,
    put: impl FnOnce(&mut Copying) -> Result<String, Error>,
) -> Result<Revision, Error> {
    let before = Wardrobe::read(folder)?;
    before
        .garment(id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let mut copying = Copying::new(folder);
    let rel = match put(&mut copying) {
        Ok(rel) => rel,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    let mut wardrobe = before.clone();
    let takes_a_place = wardrobe.rules.dummies.replace_on_add;
    let garment = garment_mut(&mut wardrobe, id)?;
    // A pack that replaces a dummy on add fills the first place the garment holds with it, so no letter is added.
    match garment
        .variations
        .iter_mut()
        .find(|v| takes_a_place && v.texture.is_none())
    {
        Some(place) => place.texture = Some(rel),
        None => garment.variations.push(Variation {
            texture: Some(rel),
            ..Variation::default()
        }),
    }
    if let Err(e) = wardrobe.write(folder) {
        copying.undo();
        return Err(e.into());
    }
    Ok(Revision {
        after: Wardrobe::read(folder)?,
        before,
        copied: copying.copied,
        made: copying.made,
    })
}

/// The most textures a garment holds: the letters `a` to `z` their names run through.
pub const MOST: usize = 26;

/// Hold `count` more places at the end of the textures of garment `id`: each is a letter that ships no file, as a dummy holds a number.
///
/// A place is a texture of the garment with no file, so the game counts it and the textures after it keep their letters. Filling one is
/// putting a file in its place ([`crate::replace::texture`]), which keeps the letter and what the place states.
pub fn reserve(wardrobe: &mut Wardrobe, id: &str, count: usize) -> Result<usize, Error> {
    let garment = garment_mut(wardrobe, id)?;
    if count == 0 {
        return Err(Error::Refused("hold at least one texture place".into()));
    }
    if garment.variations.len() + count > MOST {
        return Err(Error::Refused(format!(
            "a garment holds at most {MOST} textures, the letters a to z, and this one has {}",
            garment.variations.len()
        )));
    }
    garment
        .variations
        .extend((0..count).map(|_| Variation::default()));
    Ok(count)
}

/// Put the places of garment `id` after its textures that have a file, each group in the order it has; how many textures changed letter.
pub fn resort(wardrobe: &mut Wardrobe, id: &str) -> Result<usize, Error> {
    let garment = garment_mut(wardrobe, id)?;
    let before: Vec<Option<String>> = garment
        .variations
        .iter()
        .map(|v| v.texture.clone())
        .collect();
    // Stable, so the textures and the places each keep the order they had.
    garment.variations.sort_by_key(|v| v.texture.is_none());
    let moved = (garment.variations.iter().zip(&before))
        .filter(|(now, was)| &now.texture != *was)
        .count();
    Ok(moved)
}
