//! Copying a garment, whole, to another ped, and the copies of files inside a pack that it and a source folder need.
//!
//! A copy owns its files: a texture made from an image is written over in place, so two garments that named one file
//! would change together. The copy is therefore made of copies, under free names beside the originals.

use std::path::Path;

use crate::edit::{Copying, Error, Revision};
use crate::facts::{is_source, stays_inside};
use crate::intake::content_digest;
use crate::model::{new_id, Garment, Wardrobe};
use crate::names::Ped;

/// What duplicating garments made.
#[derive(Debug, Clone, PartialEq)]
pub struct Duplicated {
    /// The id of each garment copied and the id of its copy, in the order asked.
    pub made: Vec<(String, String)>,
    /// What Undo needs: the pack on both sides, and the files that were copied.
    pub revision: Revision,
}

/// What a name is made of, to give a copy a free one: `jacket.ydd` is `jacket` and `.ydd`.
fn split(name: &str) -> (&str, String) {
    for suffix in [".drawables.src", ".textures.src"] {
        if let Some(stem) = name.strip_suffix(suffix) {
            return (stem, suffix.to_string());
        }
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) => (stem, format!(".{ext}")),
        None => (name, String::new()),
    }
}

impl Copying<'_> {
    /// The first path beside `rel` that nothing is at, `<stem>-2<tail>` and on.
    fn free_beside(&self, rel: &str) -> String {
        let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        let (stem, tail) = split(name);
        (2..)
            .map(|n| match dir.is_empty() {
                true => format!("{stem}-{n}{tail}"),
                false => format!("{dir}/{stem}-{n}{tail}"),
            })
            .find(|candidate| !self.folder.join(candidate).exists())
            .expect("a free name is found")
    }

    /// Copy the file `from` to `to`, which is inside the pack, and list it so that Undo takes it out.
    pub(crate) fn copy_file(&mut self, from: &Path, to: &str) -> Result<(), Error> {
        let at = self.folder.join(to);
        let shown = from.display().to_string();
        if let Some(dir) = Path::new(to).parent().filter(|d| !d.as_os_str().is_empty()) {
            self.make_folder(&dir.to_string_lossy().replace('\\', "/"))?;
        }
        let name = at
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let part = at.with_file_name(format!(".{name}.part"));
        let done = std::fs::copy(from, &part).and_then(|_| std::fs::rename(&part, &at));
        if let Err(e) = done {
            let _ = std::fs::remove_file(&part);
            return Err(Error::Io(shown, e));
        }
        self.copied.push(to.to_string());
        Ok(())
    }

    /// Make the folder `rel` inside the pack, and each of its parents that is not there, listing what was made so that Undo takes the empty ones out.
    fn make_folder(&mut self, rel: &str) -> Result<(), Error> {
        let mut path = String::new();
        for part in rel.split('/') {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            if !self.folder.join(&path).exists() {
                std::fs::create_dir_all(self.folder.join(&path))
                    .map_err(|e| Error::Io(self.folder.join(&path).display().to_string(), e))?;
                self.made.push(path.clone());
            }
        }
        Ok(())
    }

    /// Write `reference` as a reference file in `dir` of the pack, named for what it leads to, and list it so that Undo takes it out; its path inside the pack.
    pub(crate) fn put_reference(
        &mut self,
        dir: &str,
        reference: &or_project::reference::Reference,
    ) -> Result<String, Error> {
        reference.check().map_err(Error::Refused)?;
        self.make_folder(dir)?;
        let stem = reference_stem(reference);
        let rel = (1..)
            .map(|n| match n {
                1 => format!("{dir}/{stem}{}", or_project::reference::SUFFIX),
                n => format!("{dir}/{stem}-{n}{}", or_project::reference::SUFFIX),
            })
            .find(|rel| !self.folder.join(rel).exists())
            .expect("a free name is found");
        let at = self.folder.join(&rel);
        reference
            .write(&at)
            .map_err(|e| Error::Io(at.display().to_string(), e))?;
        self.copied.push(rel.clone());
        Ok(rel)
    }

    /// A copy of the file `rel` of the pack, beside it.
    pub(crate) fn duplicate(&mut self, rel: &str) -> Result<String, Error> {
        let to = self.free_beside(rel);
        self.copy_file(&self.folder.join(rel), &to)?;
        Ok(to)
    }

    /// Copy the whole of the folder `from` to `to`, which is inside the pack.
    fn copy_tree(&mut self, from: &Path, to: &str) -> Result<(), Error> {
        self.make_folder(to)?;
        let mut entries: Vec<_> = std::fs::read_dir(from)
            .map_err(|e| Error::Io(from.display().to_string(), e))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            // ⚠ A link may lead out of the folder or back into it.
            if kind.is_symlink() {
                continue;
            }
            let inside = format!("{to}/{name}");
            match kind.is_dir() {
                true => self.copy_tree(&entry.path(), &inside)?,
                false => self.copy_file(&entry.path(), &inside)?,
            }
        }
        Ok(())
    }

    /// Give the source folder at `rel` an id of its own: a copy is a new asset, and two sources with one id cook into one file.
    fn retag(&self, rel: &str) -> Result<(), Error> {
        let at = self.folder.join(rel);
        let refused = |p: or_source::Problem| Error::Refused(p.to_string());
        let mut source = or_source::asset::Source::open(&at).map_err(refused)?;
        source.manifest.id = or_source::asset::new_id().map_err(refused)?;
        source.write_manifest().map_err(refused)
    }

    /// How much has been copied and made so far, to take back to.
    pub(crate) fn mark(&self) -> (usize, usize) {
        (self.copied.len(), self.made.len())
    }

    /// Remove what was copied and made after `mark`, and nothing before it.
    pub(crate) fn rollback_to(&mut self, mark: (usize, usize)) {
        for rel in self.copied.split_off(mark.0) {
            let _ = std::fs::remove_file(self.folder.join(rel));
        }
        // The last made goes first, so that a folder made inside another is out of the way of it.
        for dir in self.made.split_off(mark.1).into_iter().rev() {
            let _ = std::fs::remove_dir(self.folder.join(dir));
        }
    }

    /// A copy of the source folder `rel` of the pack, beside it.
    pub(crate) fn duplicate_source(&mut self, rel: &str) -> Result<String, Error> {
        let to = self.free_beside(rel);
        self.copy_tree(&self.folder.join(rel), &to)?;
        self.retag(&to)?;
        Ok(to)
    }

    /// The path inside the pack of the source folder `source`: where it already is, or a copy in `dir` under a free name, with an id of its own when `retag`.
    ///
    /// A source made from a game file or a picture has an id of its own already, so only a source taken as it is is given one.
    pub(crate) fn put_tree_in(
        &mut self,
        dir: &str,
        source: &Path,
        retag: bool,
    ) -> Result<String, Error> {
        if let (Ok(root), Ok(found)) = (self.folder.canonicalize(), source.canonicalize()) {
            if let Ok(inside) = found.strip_prefix(&root) {
                return Ok(inside.to_string_lossy().replace('\\', "/"));
            }
        }
        let name = source
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if !or_project::paths::is_component(&name) {
            return Err(Error::Refused(
                "its name cannot be a file name on every machine".into(),
            ));
        }
        let first = format!("{dir}/{name}");
        // The same game file or picture is the source already there, as the same bytes are the file already there.
        let held = self.folder.join(&first);
        if !retag && held.exists() {
            let (there, wanted) = (content_digest(&held), content_digest(source));
            if there.is_some() && there == wanted {
                return Ok(first);
            }
        }
        let to = match held.exists() {
            true => self.free_beside(&first),
            false => first,
        };
        self.copy_tree(source, &to)?;
        if retag {
            self.retag(&to)?;
        }
        Ok(to)
    }
}

/// The name a reference file is given: what it leads to, as a name every host can create.
fn reference_stem(reference: &or_project::reference::Reference) -> String {
    use or_project::reference::Reference;
    let leaf = |path: &str| {
        let name = path.rsplit('/').next().unwrap_or(path);
        split(name).0.to_string()
    };
    let raw = match reference {
        Reference::Here { path, .. } | Reference::Other { path, .. } => leaf(path),
        Reference::ById { id } => id.clone(),
    };
    let clean: String = (raw.chars())
        .map(|c| match or_project::paths::is_component(&c.to_string()) {
            true => c,
            false => '_',
        })
        .take(60)
        .collect();
    let clean = clean.trim_matches(['.', ' ']).to_string();
    match clean.is_empty() {
        true => "reference".to_string(),
        false => clean,
    }
}

/// A copy of `from` for `ped`: its own files, its own id, and no shop id of the garment it was copied from.
fn copy_of(copying: &mut Copying, from: &Garment, ped: Ped) -> Result<Garment, Error> {
    let mut copy = from.clone();
    copy.id = new_id();
    copy.ped = Some(ped);
    // ⚠ A pin is a place of one ped's slot, and the copy takes the lowest place the other ped's slot has free.
    copy.position = None;
    let mut dup = |named: &mut String| -> Result<(), Error> {
        // ⚠ wardrobe.json is text that travels through Git: a path that leaves the pack (or is the pack) is never read or copied, and the copy names it as the garment does.
        if !stays_inside(named) {
            return Ok(());
        }
        let at = copying.folder.join(named.as_str());
        // A file that is not there is named by the copy as it is by the garment: it is the same problem, and Locate mends each.
        if at.is_dir() && is_source(named) {
            *named = copying.duplicate_source(named)?;
        } else if at.is_file() {
            *named = copying.duplicate(named)?;
        }
        Ok(())
    };
    if let Some(model) = &mut copy.model {
        dup(model)?;
    }
    for alternate in &mut copy.alternates {
        dup(alternate)?;
    }
    if let Some(cloth) = &mut copy.cloth {
        dup(cloth)?;
    }
    for variation in &mut copy.variations {
        // The shop entry and the id somebody chose name one texture of the first garment, and two textures with one id clash.
        variation.shop = None;
        variation.shop_id = None;
        if let Some(texture) = &mut variation.texture {
            dup(texture)?;
        }
    }
    Ok(copy)
}

/// Copy each of the garments `ids` to `ped`, with its textures, cloth and flags, as new garments that take the lowest free places.
///
/// ⚠ All or none: one garment that is not there, or one file that cannot be copied, leaves the pack and its folder as they were.
pub fn to(folder: &Path, ids: &[String], ped: Ped) -> Result<Duplicated, Error> {
    let before = Wardrobe::read(folder)?;
    if ids.is_empty() {
        return Err(Error::Refused("there is no garment to copy".into()));
    }
    let mut wardrobe = before.clone();
    let mut copying = Copying::new(folder);
    let mut made = Vec::new();
    for id in ids {
        let Some(from) = before.garment(id) else {
            copying.undo();
            return Err(Error::NoSuchGarment(id.clone()));
        };
        match copy_of(&mut copying, from, ped) {
            Ok(copy) => {
                made.push((id.clone(), copy.id.clone()));
                wardrobe.garments.push(copy);
            }
            Err(e) => {
                copying.undo();
                return Err(e);
            }
        }
    }
    if let Err(e) = wardrobe.write(folder) {
        copying.undo();
        return Err(e.into());
    }
    Ok(Duplicated {
        made,
        revision: Revision {
            before,
            after: Wardrobe::read(folder)?,
            copied: copying.copied,
            made: copying.made,
        },
    })
}
