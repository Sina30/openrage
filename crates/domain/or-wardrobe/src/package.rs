//! A garment as one package: its models and textures as sources and its settings as data, in a folder that another pack imports.
//!
//! The package holds no compiled game file of the pack's: a `.ydd` or a `.ytd` the garment names is decompiled on the way out, and a
//! reference is followed to what it leads to. A cloth has no source form, so it is the one file kept as it is. Importing copies every
//! source in under an id of its own, gives the garment an id of its own, and keeps every other setting but a shop id the pack already writes; the change is a
//! [`Revision`] that one Undo takes back whole.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::alloc::allocate;
use crate::edit::{Copying, Error, Revision, Source};
use crate::facts::stays_inside;
use crate::follow::{follow, Led};
use crate::intake::{decompiled, Scratch};
use crate::model::{new_id, Garment, Wardrobe};
use crate::shop;

/// The file of a package that states the garment.
pub const FILE: &str = "garment.json";

/// The version of the document a package holds.
pub const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: u32,
    garment: Garment,
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packaged {
    /// The model, texture and cloth files of the package, as paths inside it.
    pub files: Vec<String>,
}

/// What an import added.
#[derive(Debug, Clone, PartialEq)]
pub struct Added {
    /// The id the garment has in the pack.
    pub garment: String,
    /// What Undo needs.
    pub revision: Revision,
    /// What changed from the package on the way in, in words.
    pub notes: Vec<String>,
}

fn refused(why: impl Into<String>) -> Error {
    Error::Refused(why.into())
}

/// The source folder or file the name `rel` of the pack at `pack` leads to.
fn leads_to(pack: &Path, rel: &str) -> Result<PathBuf, Error> {
    match follow(pack, rel) {
        Led::Own if stays_inside(rel) => {
            Ok(pack.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
        }
        Led::Own => Err(refused(format!("{rel} is not a file inside the pack"))),
        Led::To { at, .. } => Ok(at),
        Led::Id(id) => Err(refused(format!(
            "{rel} is only the id {id}, which is not a file in any project, so there is none to package"
        ))),
        Led::Nowhere { why, .. } => Err(refused(why)),
    }
}

/// Write garment `id` of the pack in `pack` as a package into the folder `into`, which is made and must not hold anything.
pub fn export(pack: &Path, id: &str, into: &Path) -> Result<Packaged, Error> {
    let wardrobe = Wardrobe::read(pack)?;
    let mut garment = wardrobe
        .garment(id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?
        .clone();
    let existed = into.exists();
    if existed && std::fs::read_dir(into).is_ok_and(|mut d| d.next().is_some()) {
        return Err(refused(format!(
            "{} is not empty: a package is written into a folder of its own",
            into.display()
        )));
    }
    std::fs::create_dir_all(into).map_err(|e| Error::Io(into.display().to_string(), e))?;
    let made = packaged(pack, into, &mut garment);
    if made.is_err() && !existed {
        let _ = std::fs::remove_dir_all(into);
    }
    made
}

/// Copy every file garment `garment` of the pack in `pack` names into the folder `copying` fills, as sources, and name the copies; the paths of the copies are added to `files`.
///
/// A source folder is copied as it is and a game file is decompiled first, so the folder holds no compiled game file but a cloth, which has no source form. A source two garments name is copied once.
pub(crate) fn gather(
    pack: &Path,
    copying: &mut Copying,
    scratch: &mut Scratch,
    files: &mut Vec<String>,
    garment: &mut Garment,
) -> Result<(), Error> {
    let mut source = |dir: &str, rel: &str| -> Result<String, Error> {
        let mut from = leads_to(pack, rel)?;
        if !from.is_dir() {
            from = decompiled(scratch, &from)?;
        }
        let made = copying.put_tree_in(dir, &from, false)?;
        files.push(made.clone());
        Ok(made)
    };
    if let Some(rel) = garment.model.take() {
        garment.model = Some(source("models", &rel)?);
    }
    for rel in std::mem::take(&mut garment.alternates) {
        garment.alternates.push(source("models", &rel)?);
    }
    for v in &mut garment.variations {
        if let Some(rel) = v.texture.take() {
            v.texture = Some(source("textures", &rel)?);
        }
    }
    if let Some(rel) = garment.cloth.take() {
        let from = Source {
            path: leads_to(pack, &rel)?,
            shown: rel,
        };
        let made = copying.put("cloth", &from)?;
        files.push(made.clone());
        garment.cloth = Some(made);
    }
    Ok(())
}

fn packaged(pack: &Path, into: &Path, garment: &mut Garment) -> Result<Packaged, Error> {
    let mut copying = Copying::new(into);
    let mut scratch = Scratch::default();
    let mut files = Vec::new();
    gather(pack, &mut copying, &mut scratch, &mut files, garment)?;
    let document = Document {
        schema: SCHEMA,
        garment: garment.clone(),
    };
    let mut text = serde_json::to_string_pretty(&document).map_err(|e| refused(e.to_string()))?;
    text.push('\n');
    let at = into.join(FILE);
    std::fs::write(&at, text).map_err(|e| Error::Io(at.display().to_string(), e))?;
    Ok(Packaged { files })
}

/// The garment the package `package` holds, with every file it names checked to be in the package.
fn read(package: &Path) -> Result<Garment, Error> {
    let at = package.join(FILE);
    let text = std::fs::read_to_string(&at).map_err(|e| {
        refused(format!(
            "{} is not a garment package: {e}",
            package.display()
        ))
    })?;
    let document: Document = serde_json::from_str(&text)
        .map_err(|e| refused(format!("{} does not read: {e}", at.display())))?;
    if document.schema != SCHEMA {
        return Err(refused(format!(
            "{} is schema {}, and this build reads schema {SCHEMA}",
            at.display(),
            document.schema
        )));
    }
    let g = &document.garment;
    let models = (g.model.iter().chain(&g.alternates)).map(|rel| (rel, ".drawables.src"));
    let textures =
        (g.variations.iter().filter_map(|v| v.texture.as_ref())).map(|rel| (rel, ".textures.src"));
    for (rel, suffix) in models.chain(textures) {
        let inside = stays_inside(rel) && rel.ends_with(suffix);
        if !inside || !package.join(rel).is_dir() {
            return Err(refused(format!(
                "the package names {rel}, which is not a {suffix} folder in it"
            )));
        }
    }
    if let Some(rel) = &g.cloth {
        if !stays_inside(rel) || !package.join(rel).is_file() {
            return Err(refused(format!(
                "the package names {rel}, which is not a file in it"
            )));
        }
    }
    Ok(document.garment)
}

/// Add the garment of the package `package` to the pack in `pack`.
pub fn import(pack: &Path, package: &Path) -> Result<Added, Error> {
    let mut garment = read(package)?;
    let before = Wardrobe::read(pack)?;
    let mut copying = Copying::new(pack);
    if let Err(e) = copy_in(&mut copying, package, &mut garment) {
        copying.undo();
        return Err(e);
    }
    let mut notes = Vec::new();
    if let (Some(ped), Some(slot), Some(place)) = (garment.ped, garment.slot, garment.position) {
        let taken = (before.garments.iter())
            .any(|o| (o.ped, o.slot, o.position) == (Some(ped), Some(slot), Some(place)));
        if taken {
            garment.position = None;
            notes.push(format!(
                "place {place} of {slot} for {ped} is taken in this pack, so the garment takes the lowest free place"
            ));
        }
    }
    // ⚠ Two textures written with one id clash, so an id the pack already writes is not stated again: the texture takes the name a build gives.
    let taken: BTreeSet<String> = shop::written(&before, &allocate(&before))
        .into_iter()
        .map(|w| w.id.to_ascii_lowercase())
        .collect();
    for (n, v) in garment.variations.iter_mut().enumerate() {
        if let Some(stated) = v.stated_shop_id().map(String::from) {
            if taken.contains(&stated.to_ascii_lowercase()) {
                v.shop = None;
                v.shop_id = None;
                notes.push(format!(
                    "texture {n} states the shop id {stated}, which this pack already writes for a texture, so it takes the name a build gives"
                ));
            }
        }
    }
    garment.id = new_id();
    let id = garment.id.clone();
    let mut wardrobe = before.clone();
    wardrobe.garments.push(garment);
    if let Err(e) = wardrobe.write(pack) {
        copying.undo();
        return Err(e.into());
    }
    Ok(Added {
        garment: id,
        revision: Revision {
            after: Wardrobe::read(pack)?,
            before,
            copied: copying.copied,
            made: copying.made,
        },
        notes,
    })
}

/// Copy every file the garment names out of the package into the pack, under an id of its own, and name the copies.
fn copy_in(copying: &mut Copying, package: &Path, garment: &mut Garment) -> Result<(), Error> {
    if let Some(rel) = garment.model.take() {
        garment.model = Some(copying.put_tree_in("models", &package.join(rel), true)?);
    }
    for rel in std::mem::take(&mut garment.alternates) {
        let made = copying.put_tree_in("models", &package.join(rel), true)?;
        garment.alternates.push(made);
    }
    for v in &mut garment.variations {
        if let Some(rel) = v.texture.take() {
            v.texture = Some(copying.put_tree_in("textures", &package.join(rel), true)?);
        }
    }
    if let Some(rel) = garment.cloth.take() {
        let from = Source {
            path: package.join(&rel),
            shown: rel,
        };
        garment.cloth = Some(copying.put("cloth", &from)?);
    }
    Ok(())
}
